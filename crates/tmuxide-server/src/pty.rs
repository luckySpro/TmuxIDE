//! 把 `tmux attach` 运行在伪终端中，并通过 WebSocket 与前端 xterm.js 双向转发。

use std::{
    io::Read,
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use tmuxide_tmux::Tmux;
use tokio::sync::{mpsc, watch};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ClientMsg {
    Input {
        data: String,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    /// 远程端切换窗口（只对远程分组会话有效）
    Select {
        window: String,
    },
}

/// 一次终端桥接的参数
pub struct BridgeSpec {
    pub bin: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub size: (u16, u16),
    /// 只读：丢弃所有输入（tmux 客户端本身也以 read-only 方式连接）
    pub read_only: bool,
    pub on_select: Option<Box<dyn Fn(String) + Send>>,
    /// 变为 true 时立即断开（设备被移除、远程访问关闭）
    pub kill: Option<watch::Receiver<bool>>,
    /// 统计回车次数（近似“发送了几条命令”）
    pub enters: Option<Arc<AtomicU64>>,
}

pub async fn bridge(socket: WebSocket, tmux: Arc<Tmux>, session: String, size: (u16, u16)) {
    let (bin, args) = tmux.attach_argv(&session);
    bridge_with(
        socket,
        BridgeSpec {
            bin,
            args,
            env: tmux.extra_env(),
            size,
            read_only: false,
            on_select: None,
            kill: None,
            enters: None,
        },
    )
    .await
}

pub async fn bridge_with(socket: WebSocket, spec: BridgeSpec) {
    let BridgeSpec {
        bin,
        args,
        env,
        size: (cols, rows),
        read_only,
        on_select,
        kill,
        enters,
    } = spec;
    let (mut tx, mut rx) = socket.split();
    let pty = match native_pty_system().openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(p) => p,
        Err(e) => {
            let _ = tx
                .send(Message::Text(format!("\r\n无法创建终端：{e}\r\n").into()))
                .await;
            return;
        }
    };
    let mut cmd = CommandBuilder::new(bin);
    cmd.args(args);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    for (k, v) in env {
        cmd.env(k, v);
    }
    if std::env::var_os("LANG").is_none() && std::env::var_os("LC_ALL").is_none() {
        cmd.env("LANG", "en_US.UTF-8");
    }
    if let Some(home) = tmuxide_tmux::home_dir() {
        cmd.cwd(home);
    }
    let mut child = match pty.slave.spawn_command(cmd) {
        Ok(c) => c,
        Err(e) => {
            let _ = tx
                .send(Message::Text(format!("\r\n无法启动 tmux：{e}\r\n").into()))
                .await;
            return;
        }
    };
    drop(pty.slave);
    let master = pty.master;
    let mut reader = match master.try_clone_reader() {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut writer = match master.take_writer() {
        Ok(w) => w,
        Err(_) => return,
    };

    // 读终端输出（阻塞线程）→ 通道 → WebSocket
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(64);
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if out_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let send_task = tokio::spawn(async move {
        while let Some(chunk) = out_rx.recv().await {
            if tx.send(Message::Binary(chunk.into())).await.is_err() {
                break;
            }
        }
        let _ = tx.send(Message::Close(None)).await;
    });

    let mut kill = kill;
    loop {
        let msg = match &mut kill {
            Some(k) => tokio::select! {
                m = rx.next() => m,
                _ = k.changed() => break,
            },
            None => rx.next().await,
        };
        let Some(Ok(msg)) = msg else { break };
        match msg {
            Message::Text(t) => match serde_json::from_str::<ClientMsg>(&t) {
                Ok(ClientMsg::Input { data }) => {
                    if read_only {
                        continue;
                    }
                    if let Some(c) = &enters {
                        c.fetch_add(data.matches('\r').count() as u64, Ordering::Relaxed);
                    }
                    if writer.write_all(data.as_bytes()).is_err() {
                        break;
                    }
                }
                Ok(ClientMsg::Select { window }) => {
                    if let Some(f) = &on_select {
                        f(window);
                    }
                }
                Ok(ClientMsg::Resize { cols, rows }) => {
                    let _ = master.resize(PtySize {
                        rows: rows.clamp(5, 500),
                        cols: cols.clamp(10, 1000),
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                }
                Err(_) => {}
            },
            Message::Binary(b) => {
                if read_only {
                    continue;
                }
                if writer.write_all(&b).is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    // 关闭 tmux 客户端只是 detach，会话继续运行
    let _ = child.kill();
    let _ = child.wait();
    send_task.abort();
}

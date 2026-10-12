//! TmuxIDE 桌面应用：在进程内启动本地服务（随机端口 + 随机令牌），主窗口加载其界面。
//! 关闭窗口只退出界面，tmux 会话继续在后台运行。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};

use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};
use tmuxide_server::{AppPaths, Remote, ServeOptions};

mod updater;

/// 退出时需要关闭远程访问（例如撤销 Tailscale serve 配置）
#[derive(Default)]
struct RemoteSlot(Mutex<Option<Arc<Remote>>>);

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "tmuxide=info,warn".into()),
        )
        .init();

    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_updater::Builder::new()
                .pubkey(tmuxide_update::pubkey())
                .build(),
        )
        .manage(RemoteSlot::default())
        .setup(|app| {
            let paths = AppPaths::default_paths()?;
            // 服务运行在 Tauri 的 tokio 运行时上
            let handle = tauri::async_runtime::block_on(tmuxide_server::start(ServeOptions {
                port: 0,
                paths,
                installer: updater::installer(app.handle()),
            }));
            let url = match handle {
                Ok(h) => {
                    *app.state::<RemoteSlot>().0.lock().unwrap() = Some(h.remote.clone());
                    h.url()
                }
                Err(e) => {
                    let msg = format!("TmuxIDE 启动失败：{e:#}");
                    tracing::error!("{msg}");
                    format!("data:text/html;charset=utf-8,{}", urlencode(&error_page(&msg)))
                }
            };
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url.parse()?))
                .title("TmuxIDE")
                .inner_size(1440.0, 900.0)
                .min_inner_size(1024.0, 640.0)
                .center()
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("TmuxIDE 运行失败");
    app.run(|app, event| {
        if let RunEvent::Exit = event {
            let remote = app.state::<RemoteSlot>().0.lock().unwrap().take();
            if let Some(r) = remote {
                tauri::async_runtime::block_on(r.stop());
            }
        }
    });
}

fn error_page(msg: &str) -> String {
    let escaped = msg.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<body style=\"background:#0d1014;color:#e4e7eb;font-family:-apple-system,sans-serif;padding:40px\">\
         <h2>TmuxIDE</h2><p>{escaped}</p></body>"
    )
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

//! 远程端的终端列表：定时读取 tmux 会话结构和每个面板最后几行，有变化时推送给所有在线的手机。

use std::{sync::Arc, time::Duration};

use serde::Serialize;
use tokio::sync::watch;

use super::Remote;

/// 每次最多抓取预览的面板数
const MAX_PREVIEW_PANES: usize = 60;
const PREVIEW_LINES: usize = 6;
const PREVIEW_COLS: usize = 160;

#[derive(Debug, Default, Clone, Serialize, PartialEq)]
pub struct TermList {
    pub host: String,
    pub sessions: Vec<TermSession>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TermSession {
    pub name: String,
    /// 电脑上是否有客户端连着
    pub attached: u32,
    pub windows: Vec<TermWindow>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TermWindow {
    pub id: String,
    pub index: u32,
    pub name: String,
    pub active: bool,
    pub cols: u32,
    pub rows: u32,
    pub panes: Vec<TermPane>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TermPane {
    pub id: String,
    pub index: u32,
    pub active: bool,
    pub command: String,
    pub cwd: String,
    pub preview: String,
}

/// 从 tmux 布局字符串（`b25d,80x24,0,0{...}`）中取出窗口尺寸
pub fn layout_size(layout: &str) -> Option<(u32, u32)> {
    let size = layout.split(',').nth(1)?;
    let (w, h) = size.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

fn build(remote: &Remote) -> TermList {
    let tmux = remote.svc.tmux();
    let host = remote.svc.host().to_string();
    let sessions = match tmux.sessions() {
        Ok(s) => s,
        Err(e) => {
            return TermList {
                host,
                sessions: Vec::new(),
                error: Some(e.to_string()),
            }
        }
    };
    let ids: Vec<String> = sessions
        .iter()
        .flat_map(|s| s.windows.iter().flat_map(|w| w.panes.iter().map(|p| p.id.clone())))
        .take(MAX_PREVIEW_PANES)
        .collect();
    let previews = tmux.capture_many(&ids, PREVIEW_LINES).unwrap_or_default();
    let preview_of = |id: &str| -> String {
        ids.iter()
            .position(|x| x == id)
            .and_then(|i| previews.get(i))
            .map(|text| {
                text.lines()
                    .map(|l| l.chars().take(PREVIEW_COLS).collect::<String>())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    };
    let home = tmuxide_tmux::home_dir();
    TermList {
        host,
        error: None,
        sessions: sessions
            .iter()
            .map(|s| TermSession {
                name: s.name.clone(),
                attached: s.attached,
                windows: s
                    .windows
                    .iter()
                    .map(|w| {
                        let (cols, rows) = layout_size(&w.layout).unwrap_or((80, 24));
                        TermWindow {
                            id: w.id.clone(),
                            index: w.index,
                            name: w.name.clone(),
                            active: w.active,
                            cols,
                            rows,
                            panes: w
                                .panes
                                .iter()
                                .map(|p| TermPane {
                                    id: p.id.clone(),
                                    index: p.index,
                                    active: p.active,
                                    command: p.command.clone(),
                                    cwd: tmuxide_core::collapse_home(&p.cwd, home.as_deref()),
                                    preview: preview_of(&p.id),
                                })
                                .collect(),
                        }
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// 有手机在看列表时每 1.5 秒刷新一次；没人看时不调用 tmux
pub async fn broadcast(remote: Arc<Remote>, mut stop: watch::Receiver<bool>) {
    loop {
        if remote.terms.receiver_count() > 0 {
            let r = remote.clone();
            if let Ok(list) = tokio::task::spawn_blocking(move || build(&r)).await {
                remote.terms.send_if_modified(|cur| {
                    if **cur == list {
                        false
                    } else {
                        *cur = Arc::new(list);
                        true
                    }
                });
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(1500)) => {}
            _ = stop.wait_for(|s| *s) => return,
        }
    }
}

/// 立即生成一次（新连接时不必等下一个周期）
pub async fn snapshot(remote: Arc<Remote>) -> Arc<TermList> {
    let r = remote.clone();
    match tokio::task::spawn_blocking(move || build(&r)).await {
        Ok(list) => {
            let list = Arc::new(list);
            remote.terms.send_replace(list.clone());
            list
        }
        Err(_) => remote.terms.borrow().clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_layout_size() {
        assert_eq!(layout_size("b25d,80x24,0,0{40x24,0,0,0,39x24,41,0,1}"), Some((80, 24)));
        assert_eq!(layout_size("abcd,200x50,0,0,2"), Some((200, 50)));
        assert_eq!(layout_size("bad"), None);
    }
}

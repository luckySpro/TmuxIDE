use serde::Serialize;

const SEP: &str = "|~|";

/// list-panes -a 的输出格式；字段之间用不太可能出现在名字中的分隔符连接
pub const PANE_FORMAT: &str = "#{session_name}|~|#{session_id}|~|#{session_attached}|~|#{session_created}|~|#{window_index}|~|#{window_id}|~|#{window_active}|~|#{window_layout}|~|#{pane_index}|~|#{pane_id}|~|#{pane_active}|~|#{pane_pid}|~|#{pane_width}|~|#{pane_height}|~|#{pane_current_command}|~|#{pane_current_path}|~|#{window_name}";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SessionInfo {
    pub name: String,
    pub id: String,
    pub attached: u32,
    pub created: u64,
    pub windows: Vec<WindowInfo>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WindowInfo {
    pub index: u32,
    pub id: String,
    pub name: String,
    pub active: bool,
    pub layout: String,
    pub panes: Vec<PaneInfo>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PaneInfo {
    pub index: u32,
    pub id: String,
    pub active: bool,
    pub pid: u32,
    pub width: u32,
    pub height: u32,
    pub command: String,
    pub cwd: String,
}

pub fn parse_panes(text: &str) -> Vec<SessionInfo> {
    let mut sessions: Vec<SessionInfo> = Vec::new();
    for line in text.lines() {
        // 窗口名放在最后，即使包含分隔符也不会错位
        let f: Vec<&str> = line.splitn(17, SEP).collect();
        if f.len() != 17 {
            continue;
        }
        let num = |s: &str| s.parse::<u64>().unwrap_or(0);
        let pane = PaneInfo {
            index: num(f[8]) as u32,
            id: f[9].to_string(),
            active: f[10] == "1",
            pid: num(f[11]) as u32,
            width: num(f[12]) as u32,
            height: num(f[13]) as u32,
            command: f[14].to_string(),
            cwd: f[15].to_string(),
        };
        let si = match sessions.iter().position(|s| s.id == f[1]) {
            Some(i) => i,
            None => {
                sessions.push(SessionInfo {
                    name: f[0].to_string(),
                    id: f[1].to_string(),
                    attached: num(f[2]) as u32,
                    created: num(f[3]),
                    windows: Vec::new(),
                });
                sessions.len() - 1
            }
        };
        let s = &mut sessions[si];
        let wi = match s.windows.iter().position(|w| w.id == f[5]) {
            Some(i) => i,
            None => {
                s.windows.push(WindowInfo {
                    index: num(f[4]) as u32,
                    id: f[5].to_string(),
                    name: f[16].to_string(),
                    active: f[6] == "1",
                    layout: f[7].to_string(),
                    panes: Vec::new(),
                });
                s.windows.len() - 1
            }
        };
        s.windows[wi].panes.push(pane);
    }
    for s in &mut sessions {
        s.windows.sort_by_key(|w| w.index);
        for w in &mut s.windows {
            w.panes.sort_by_key(|p| p.index);
        }
    }
    sessions.sort_by(|a, b| a.name.cmp(&b.name));
    sessions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list_panes() {
        let text = "dev|~|$0|~|1|~|1700000000|~|1|~|@0|~|1|~|b25d,80x24,0,0{40x24,0,0,0,39x24,41,0,1}|~|0|~|%0|~|1|~|100|~|40|~|24|~|nvim|~|/home/u/app|~|editor\n\
dev|~|$0|~|1|~|1700000000|~|1|~|@0|~|1|~|b25d,80x24,0,0{40x24,0,0,0,39x24,41,0,1}|~|1|~|%1|~|0|~|101|~|39|~|24|~|zsh|~|/home/u/app|~|editor\n\
dev|~|$0|~|1|~|1700000000|~|2|~|@1|~|0|~|abcd,80x24,0,0,2|~|0|~|%2|~|1|~|102|~|80|~|24|~|zsh|~|/tmp|~|odd|~|name\n";
        let s = parse_panes(text);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].windows.len(), 2);
        assert_eq!(s[0].windows[0].panes.len(), 2);
        assert_eq!(s[0].windows[1].name, "odd|~|name");
        assert!(s[0].windows[0].panes[0].active);
    }
}

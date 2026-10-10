use serde::Serialize;

use crate::SessionSnapshot;

/// 恢复计划中的一步，界面用来展示，tmux 适配层逐步执行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RestoreStep {
    /// 新建会话（同时创建第一个窗口的第一个面板）
    NewSession {
        session: String,
        window: String,
        cwd: String,
    },
    /// 新建窗口（同时创建其第一个面板）
    NewWindow {
        window_pos: usize,
        window: String,
        cwd: String,
    },
    /// 在窗口中分出新面板
    SplitPane {
        window_pos: usize,
        cwd: String,
    },
    /// 应用保存的布局
    SelectLayout {
        window_pos: usize,
        layout: String,
    },
    /// 在面板中运行命令
    RunCommand {
        window_pos: usize,
        pane_pos: usize,
        cmd: String,
    },
    /// 恢复活动面板与活动窗口
    SelectPane {
        window_pos: usize,
        pane_pos: usize,
    },
    SelectWindow {
        window_pos: usize,
    },
}

/// 根据快照生成恢复计划。`window_pos`/`pane_pos` 是快照中的顺序位置，执行层把它映射到真实 tmux id。
pub fn restore_plan(snap: &SessionSnapshot, target_name: &str, run_commands: bool) -> Vec<RestoreStep> {
    let mut steps = Vec::new();
    let mut active_window = 0;
    for (wpos, w) in snap.windows.iter().enumerate() {
        let first_cwd = w.panes.first().map(|p| p.cwd.clone()).unwrap_or_else(|| "~".into());
        if wpos == 0 {
            steps.push(RestoreStep::NewSession {
                session: target_name.to_string(),
                window: w.name.clone(),
                cwd: first_cwd,
            });
        } else {
            steps.push(RestoreStep::NewWindow {
                window_pos: wpos,
                window: w.name.clone(),
                cwd: first_cwd,
            });
        }
        for p in w.panes.iter().skip(1) {
            steps.push(RestoreStep::SplitPane {
                window_pos: wpos,
                cwd: p.cwd.clone(),
            });
        }
        if w.panes.len() > 1 {
            steps.push(RestoreStep::SelectLayout {
                window_pos: wpos,
                layout: w.layout.clone(),
            });
        }
        if run_commands {
            for (ppos, p) in w.panes.iter().enumerate() {
                if let Some(cmd) = p.cmd.as_deref().filter(|c| !c.trim().is_empty()) {
                    steps.push(RestoreStep::RunCommand {
                        window_pos: wpos,
                        pane_pos: ppos,
                        cmd: cmd.to_string(),
                    });
                }
            }
        }
        if let Some(ppos) = w.panes.iter().position(|p| p.active) {
            steps.push(RestoreStep::SelectPane {
                window_pos: wpos,
                pane_pos: ppos,
            });
        }
        if w.active {
            active_window = wpos;
        }
    }
    steps.push(RestoreStep::SelectWindow {
        window_pos: active_window,
    });
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::sample;

    #[test]
    fn plan_order() {
        let plan = restore_plan(&sample(), "dev-2", true);
        assert!(matches!(&plan[0], RestoreStep::NewSession { session, .. } if session == "dev-2"));
        assert!(matches!(plan[1], RestoreStep::SplitPane { window_pos: 0, .. }));
        assert!(matches!(plan[2], RestoreStep::SelectLayout { window_pos: 0, .. }));
        assert!(matches!(&plan[3], RestoreStep::RunCommand { cmd, .. } if cmd == "nvim"));
        assert!(plan
            .iter()
            .any(|s| matches!(s, RestoreStep::NewWindow { window_pos: 1, .. })));
        assert_eq!(plan.last(), Some(&RestoreStep::SelectWindow { window_pos: 0 }));
    }

    #[test]
    fn plan_without_commands() {
        let plan = restore_plan(&sample(), "dev", false);
        assert!(!plan.iter().any(|s| matches!(s, RestoreStep::RunCommand { .. })));
    }
}

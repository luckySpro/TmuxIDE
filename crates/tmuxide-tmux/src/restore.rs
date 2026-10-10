use std::path::Path;

use serde::Serialize;
use tmuxide_core::{expand_home, restore_plan, RestoreStep, SessionSnapshot};

use crate::{home_dir, Result, Tmux, TmuxError};

#[derive(Debug, Clone, Serialize, Default)]
pub struct RestoreReport {
    pub session: String,
    pub steps: usize,
    /// 未能完成但不影响整体的步骤（例如布局不匹配、目录不存在）
    pub warnings: Vec<String>,
}

const SEP: &str = "|~|";

pub fn execute(t: &Tmux, snap: &SessionSnapshot, target: &str, run_commands: bool) -> Result<RestoreReport> {
    let home = home_dir();
    let mut report = RestoreReport {
        session: target.to_string(),
        ..Default::default()
    };
    let mut windows: Vec<(String, Vec<String>)> = Vec::new();

    let resolve_cwd = |cwd: &str, report: &mut RestoreReport| -> String {
        let p = expand_home(cwd, home.as_deref());
        if Path::new(&p).is_dir() {
            p
        } else {
            report.warnings.push(format!("目录 {cwd} 不存在，改用家目录"));
            home.as_ref()
                .map(|h| h.display().to_string())
                .unwrap_or_else(|| "/".into())
        }
    };

    let plan = restore_plan(snap, target, run_commands);
    report.steps = plan.len();
    for step in plan {
        match step {
            RestoreStep::NewSession { session, window, cwd } => {
                let cwd = resolve_cwd(&cwd, &mut report);
                let fmt = format!("#{{window_id}}{SEP}#{{pane_id}}");
                let out = t.run(&[
                    "new-session",
                    "-d",
                    "-s",
                    &session,
                    "-n",
                    &window,
                    "-c",
                    &cwd,
                    "-x",
                    "200",
                    "-y",
                    "50",
                    "-P",
                    "-F",
                    &fmt,
                ])?;
                windows.push(split_ids(&out)?);
            }
            RestoreStep::NewWindow { window, cwd, .. } => {
                let cwd = resolve_cwd(&cwd, &mut report);
                let fmt = format!("#{{window_id}}{SEP}#{{pane_id}}");
                let tgt = format!("={target}:");
                let out = t.run(&[
                    "new-window",
                    "-d",
                    "-t",
                    &tgt,
                    "-n",
                    &window,
                    "-c",
                    &cwd,
                    "-P",
                    "-F",
                    &fmt,
                ])?;
                windows.push(split_ids(&out)?);
            }
            RestoreStep::SplitPane { window_pos, cwd } => {
                let cwd = resolve_cwd(&cwd, &mut report);
                let (wid, panes) = &mut windows[window_pos];
                // 先平铺再分屏，避免面板过多时空间不足
                let _ = t.run(&["select-layout", "-t", wid, "tiled"]);
                let out = t.run(&["split-window", "-d", "-t", wid, "-c", &cwd, "-P", "-F", "#{pane_id}"])?;
                panes.push(out.trim().to_string());
            }
            RestoreStep::SelectLayout { window_pos, layout } => {
                let (wid, _) = &windows[window_pos];
                if let Err(e) = t.run(&["select-layout", "-t", wid, &layout]) {
                    report.warnings.push(format!("窗口布局未能完全还原：{e}"));
                }
            }
            RestoreStep::RunCommand {
                window_pos,
                pane_pos,
                cmd,
            } => {
                let pid = &windows[window_pos].1[pane_pos];
                t.run(&["send-keys", "-t", pid, "-l", &cmd])?;
                t.run(&["send-keys", "-t", pid, "Enter"])?;
            }
            RestoreStep::SelectPane { window_pos, pane_pos } => {
                let pid = &windows[window_pos].1[pane_pos];
                let _ = t.run(&["select-pane", "-t", pid]);
            }
            RestoreStep::SelectWindow { window_pos } => {
                let _ = t.run(&["select-window", "-t", &windows[window_pos].0]);
            }
        }
    }
    Ok(report)
}

fn split_ids(out: &str) -> Result<(String, Vec<String>)> {
    let mut it = out.trim().splitn(2, SEP);
    match (it.next(), it.next()) {
        (Some(w), Some(p)) if !w.is_empty() && !p.is_empty() => Ok((w.to_string(), vec![p.to_string()])),
        _ => Err(TmuxError::Command(format!("无法解析 tmux 输出：{out:?}"))),
    }
}

use serde::{Deserialize, Serialize};

use crate::CoreError;

/// 快照文件格式版本。新增字段保持向后兼容时不需要提升；不兼容变更必须提升并提供迁移。
pub const SNAPSHOT_VERSION: u32 = 1;

/// 一个 tmux 会话的可恢复描述，序列化为人类可读的 TOML，便于 Git diff。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub version: u32,
    pub name: String,
    /// 保存该快照的主机名（目录名同此值）
    pub host: String,
    /// RFC 3339 时间
    pub saved_at: String,
    /// 会话默认工作目录，家目录写成 `~`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(default)]
    pub windows: Vec<WindowSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSnapshot {
    pub index: u32,
    pub name: String,
    /// tmux `window_layout` 字符串
    pub layout: String,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub panes: Vec<PaneSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub index: u32,
    /// 工作目录，家目录写成 `~`
    pub cwd: String,
    /// 恢复时要重新执行的命令；面板停在 shell 时为空
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmd: Option<String>,
    #[serde(default)]
    pub active: bool,
}

impl SessionSnapshot {
    pub fn from_toml(text: &str) -> Result<Self, CoreError> {
        let snap: SessionSnapshot = toml::from_str(text)?;
        if snap.version > SNAPSHOT_VERSION {
            return Err(CoreError::TooNew {
                found: snap.version,
                supported: SNAPSHOT_VERSION,
            });
        }
        snap.validate()?;
        Ok(snap)
    }

    pub fn to_toml(&self) -> Result<String, CoreError> {
        Ok(toml::to_string_pretty(self)?)
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        if !crate::valid_session_name(&self.name) {
            return Err(CoreError::Invalid(format!("会话名 {:?} 不合法", self.name)));
        }
        if self.windows.is_empty() {
            return Err(CoreError::Invalid("会话至少需要一个窗口".into()));
        }
        for w in &self.windows {
            if w.panes.is_empty() {
                return Err(CoreError::Invalid(format!("窗口 {} 没有面板", w.name)));
            }
        }
        Ok(())
    }

    pub fn pane_count(&self) -> usize {
        self.windows.iter().map(|w| w.panes.len()).sum()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample() -> SessionSnapshot {
        SessionSnapshot {
            version: SNAPSHOT_VERSION,
            name: "dev".into(),
            host: "mbp".into(),
            saved_at: "2026-10-10T18:42:30+08:00".into(),
            root: Some("~/code/app".into()),
            windows: vec![
                WindowSnapshot {
                    index: 1,
                    name: "editor".into(),
                    layout: "b25d,162x44,0,0{96x44,0,0,1,65x44,97,0,2}".into(),
                    active: true,
                    panes: vec![
                        PaneSnapshot {
                            index: 0,
                            cwd: "~/code/app".into(),
                            cmd: Some("nvim".into()),
                            active: true,
                        },
                        PaneSnapshot {
                            index: 1,
                            cwd: "~/code/app".into(),
                            cmd: None,
                            active: false,
                        },
                    ],
                },
                WindowSnapshot {
                    index: 2,
                    name: "logs".into(),
                    layout: "c0d1,162x44,0,0,3".into(),
                    active: false,
                    panes: vec![PaneSnapshot {
                        index: 0,
                        cwd: "/var/log".into(),
                        cmd: Some("tail -f syslog".into()),
                        active: true,
                    }],
                },
            ],
        }
    }

    #[test]
    fn toml_roundtrip() {
        let s = sample();
        let text = s.to_toml().unwrap();
        assert!(text.contains("[[windows]]"));
        assert_eq!(SessionSnapshot::from_toml(&text).unwrap(), s);
        assert_eq!(s.pane_count(), 3);
    }

    #[test]
    fn rejects_newer_version() {
        let mut s = sample();
        s.version = SNAPSHOT_VERSION + 1;
        let text = s.to_toml().unwrap();
        assert!(matches!(
            SessionSnapshot::from_toml(&text),
            Err(CoreError::TooNew { .. })
        ));
    }

    #[test]
    fn rejects_empty_window() {
        let mut s = sample();
        s.windows[1].panes.clear();
        assert!(s.validate().is_err());
    }
}

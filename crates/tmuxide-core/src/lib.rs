//! TmuxIDE 领域模型：会话快照、模板、恢复计划与敏感信息过滤。
//!
//! 本 crate 不依赖 tmux、Git 或任何 GUI，只描述数据与规则，便于单元测试。

mod model;
mod names;
mod paths;
mod plan;
mod redact;

pub use model::{PaneSnapshot, SessionSnapshot, WindowSnapshot, SNAPSHOT_VERSION};
pub use names::{is_shell, sanitize_name, valid_session_name};
pub use paths::{collapse_home, expand_home};
pub use plan::{restore_plan, RestoreStep};
pub use redact::Redactor;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("快照格式无效：{0}")]
    Parse(#[from] toml::de::Error),
    #[error("快照序列化失败：{0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("快照版本 {found} 高于当前支持的 {supported}，请升级 TmuxIDE")]
    TooNew { found: u32, supported: u32 },
    #[error("快照内容无效：{0}")]
    Invalid(String),
}

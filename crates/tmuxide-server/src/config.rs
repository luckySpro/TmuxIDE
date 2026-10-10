use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 应用使用的目录
#[derive(Debug, Clone)]
pub struct AppPaths {
    /// 配置、内置 tmux、生成的 tmux.conf
    pub data_dir: PathBuf,
}

impl AppPaths {
    /// 平台数据目录：macOS `~/Library/Application Support/TmuxIDE`，Linux `~/.local/share/TmuxIDE`
    pub fn default_paths() -> anyhow::Result<AppPaths> {
        if let Some(dir) = std::env::var_os("TMUXIDE_HOME") {
            return Ok(AppPaths {
                data_dir: PathBuf::from(dir),
            });
        }
        let base = dirs::data_dir().ok_or_else(|| anyhow::anyhow!("无法确定数据目录"))?;
        Ok(AppPaths {
            data_dir: base.join("TmuxIDE"),
        })
    }

    pub fn config_file(&self) -> PathBuf {
        self.data_dir.join("config.toml")
    }

    pub fn default_repo_dir(&self) -> PathBuf {
        self.data_dir.join("repo")
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub git: GitConfig,
    pub save: SaveConfig,
    pub tmux: TmuxConfig,
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GitConfig {
    /// 本地快照仓库目录，空表示默认位置
    pub repo_dir: String,
    /// 远程仓库地址，空表示只在本地保存
    pub remote: String,
    pub branch: String,
    /// none | ssh-key | ssh-agent | https-token
    pub auth: String,
    pub ssh_key: String,
    pub username: String,
    pub author_name: String,
    pub author_email: String,
    pub auto_commit: bool,
    pub auto_push: bool,
    pub sync_tmux_conf: bool,
}

impl Default for GitConfig {
    fn default() -> Self {
        GitConfig {
            repo_dir: String::new(),
            remote: String::new(),
            branch: "main".into(),
            auth: "ssh-key".into(),
            ssh_key: "~/.ssh/id_ed25519".into(),
            username: String::new(),
            author_name: String::new(),
            author_email: String::new(),
            auto_commit: true,
            auto_push: false,
            sync_tmux_conf: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SaveConfig {
    /// 自动保存间隔（分钟），0 表示关闭
    pub autosave_minutes: u32,
    /// 是否保存终端滚动缓冲区（会先过滤敏感信息）
    pub scrollback: bool,
    pub scrollback_lines: u32,
    /// 不保存的会话名
    pub exclude: Vec<String>,
}

impl Default for SaveConfig {
    fn default() -> Self {
        SaveConfig {
            autosave_minutes: 5,
            scrollback: false,
            scrollback_lines: 2000,
            exclude: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TmuxConfig {
    pub use_system: bool,
    pub socket: String,
}

impl Default for TmuxConfig {
    fn default() -> Self {
        TmuxConfig {
            use_system: false,
            socket: "tmuxide".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UiConfig {
    /// zh-CN | en
    pub language: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        UiConfig {
            language: "zh-CN".into(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Config> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(toml::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        write_atomic(path, toml::to_string_pretty(self)?.as_bytes())?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.git.auth.as_str(), "none" | "ssh-key" | "ssh-agent" | "https-token") {
            return Err(format!("未知的认证方式：{}", self.git.auth));
        }
        let b = self.git.branch.trim();
        if b.is_empty() || b.contains(char::is_whitespace) || b.contains("..") || b.starts_with('-') {
            return Err("分支名不合法".into());
        }
        if !self
            .tmux
            .socket
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            || self.tmux.socket.is_empty()
        {
            return Err("tmux socket 名只能包含字母、数字、- 和 _".into());
        }
        if self.save.scrollback_lines > 100_000 {
            return Err("滚动缓冲区行数不能超过 100000".into());
        }
        Ok(())
    }
}

/// 同目录临时文件 + rename 的原子写入
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        std::process::id()
    ));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_roundtrip_and_partial_files() {
        let c: Config = toml::from_str("[git]\nremote = \"git@x:y.git\"\n").unwrap();
        assert_eq!(c.git.branch, "main");
        assert_eq!(c.save.autosave_minutes, 5);
        assert!(c.validate().is_ok());
        let text = toml::to_string_pretty(&c).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), c);
        let mut bad = c.clone();
        bad.git.auth = "x".into();
        assert!(bad.validate().is_err());
    }
}

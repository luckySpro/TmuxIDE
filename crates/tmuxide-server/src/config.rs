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
    pub remote: RemoteConfig,
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

/// 远程访问（手机/平板）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RemoteConfig {
    pub enabled: bool,
    /// cloudflare-quick | cloudflare-token | tailscale-serve | tailscale-direct | local
    /// （local：只监听本机端口，由用户自己的反向代理转发，例如 frp、ngrok、nginx）
    pub mode: String,
    /// 远程端口：隧道转发到 127.0.0.1 的这个端口（Tailscale 直连时监听 tailnet 地址）
    pub port: u16,
    /// Cloudflare 自有域名模式下的公开域名
    pub public_host: String,
    /// Tailscale serve 对外的 HTTPS 端口
    pub tailscale_https_port: u16,
    /// 已配对设备每次打开还要输入 PIN
    pub require_pin: bool,
    /// 空闲多少分钟后需要重新输入 PIN
    pub idle_minutes: u32,
    /// 设备多少天未使用后凭证失效
    pub device_days: u32,
    /// 远程连接时在电脑界面提醒
    pub notify: bool,
    /// Cloudflare 快速隧道额外的邮箱验证（Cloudflare Access 一次性验证码），空表示不启用
    pub allowed_emails: Vec<String>,
    /// 远程访问开启期间阻止电脑空闲睡眠（macOS）
    pub keep_awake: bool,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        RemoteConfig {
            enabled: false,
            mode: "cloudflare-quick".into(),
            port: 7681,
            public_host: String::new(),
            tailscale_https_port: 443,
            require_pin: true,
            idle_minutes: 30,
            device_days: 30,
            notify: true,
            allowed_emails: Vec::new(),
            keep_awake: true,
        }
    }
}

pub const REMOTE_MODES: &[&str] = &[
    "cloudflare-quick",
    "cloudflare-token",
    "tailscale-serve",
    "tailscale-direct",
    "local",
];

impl RemoteConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !REMOTE_MODES.contains(&self.mode.as_str()) {
            return Err(format!("未知的远程访问方式：{}", self.mode));
        }
        if self.port < 1024 {
            return Err("远程端口必须在 1024 到 65535 之间".into());
        }
        if self.tailscale_https_port == 0 {
            return Err("Tailscale HTTPS 端口不能为 0".into());
        }
        let h = self.public_host.trim();
        if !h.is_empty()
            && (h.len() > 253
                || !h.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
                || h.starts_with('.'))
        {
            return Err("公开域名只能包含字母、数字、- 和 .，不要带 https://".into());
        }
        if self.mode == "cloudflare-token" && h.is_empty() {
            return Err("Cloudflare 自有域名模式需要填写公开域名".into());
        }
        if !(1..=24 * 60).contains(&self.idle_minutes) {
            return Err("空闲锁定时间需在 1 分钟到 24 小时之间".into());
        }
        for e in &self.allowed_emails {
            let e = e.trim();
            let ok = match e.split_once('@') {
                Some((user, domain)) => {
                    (user == "*" || (!user.is_empty() && !user.contains(|c: char| c.is_whitespace() || c == ',')))
                        && domain.contains('.')
                        && !domain.contains(|c: char| c.is_whitespace() || c == ',' || c == '@')
                }
                None => false,
            };
            if !ok {
                return Err(format!("邮箱格式不正确：{e}（可以写 *@example.com 允许整个域名）"));
            }
        }
        if !(1..=365).contains(&self.device_days) {
            return Err("设备有效期需在 1 到 365 天之间".into());
        }
        Ok(())
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
        self.remote.validate()
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
        assert!(!c.remote.enabled);
        let mut r = c.clone();
        r.remote.mode = "cloudflare-token".into();
        assert!(r.validate().is_err(), "自有域名模式必须填域名");
        r.remote.public_host = "https://x.com".into();
        assert!(r.validate().is_err());
        r.remote.public_host = "tmux.example.com".into();
        assert!(r.validate().is_ok());
    }
}

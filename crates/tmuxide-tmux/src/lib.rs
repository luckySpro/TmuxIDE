//! tmux 适配层：定位（或释放）内置 tmux，使用专用 socket 执行命令，解析会话结构，保存与恢复。
//!
//! 所有命令都以参数数组调用，不经过 shell 拼接。

mod bundled;
mod parse;
mod ps;
mod restore;

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde::Serialize;
use tmuxide_core::{
    collapse_home, is_shell, valid_session_name, PaneSnapshot, SessionSnapshot, WindowSnapshot, SNAPSHOT_VERSION,
};

pub use bundled::{bundled_version, has_bundled};
pub use parse::{PaneInfo, SessionInfo, WindowInfo};

#[derive(Debug, thiserror::Error)]
pub enum TmuxError {
    #[error("找不到 tmux：{0}")]
    NotFound(String),
    #[error("无法执行 tmux：{0}")]
    Io(#[from] std::io::Error),
    #[error("tmux 返回错误：{0}")]
    Command(String),
    #[error("会话名 {0:?} 不合法（只能包含字母、数字、- 和 _）")]
    BadName(String),
    #[error("分组名 {0:?} 不合法（最多 32 个字符，不能包含 |~| 或控制字符）")]
    BadGroup(String),
    #[error("会话 {0} 不存在")]
    NoSession(String),
    #[error("会话 {0} 已存在")]
    Exists(String),
    #[error("快照无效：{0}")]
    Snapshot(#[from] tmuxide_core::CoreError),
}

pub type Result<T> = std::result::Result<T, TmuxError>;

#[derive(Debug, Clone)]
pub struct TmuxOptions {
    /// 应用数据目录：内置 tmux 释放到其下 `bin/`，生成的 tmux 配置放在这里
    pub data_dir: PathBuf,
    /// 为 true 时使用 PATH 中的系统 tmux
    pub use_system: bool,
    /// tmux `-L` socket 名
    pub socket: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TmuxInfo {
    pub path: String,
    pub version: String,
    pub bundled: bool,
    pub socket: String,
}

#[derive(Debug, Clone)]
pub struct Tmux {
    bin: PathBuf,
    socket: String,
    conf: PathBuf,
    bundled: bool,
    version: String,
}

impl Tmux {
    pub fn new(opts: &TmuxOptions) -> Result<Tmux> {
        std::fs::create_dir_all(&opts.data_dir)?;
        let (bin, bundled) = if !opts.use_system && has_bundled() {
            (bundled::extract(&opts.data_dir.join("bin"))?, true)
        } else {
            (
                find_in_path("tmux")
                    .ok_or_else(|| TmuxError::NotFound("此版本未内置 tmux，且系统 PATH 中没有 tmux".into()))?,
                false,
            )
        };
        let conf = opts.data_dir.join("tmux.conf");
        write_if_changed(&conf, DEFAULT_CONF)?;
        let mut t = Tmux {
            bin,
            socket: opts.socket.clone(),
            conf,
            bundled,
            version: String::new(),
        };
        t.version = t.raw(&["-V"])?.trim().trim_start_matches("tmux ").to_string();
        Ok(t)
    }

    pub fn info(&self) -> TmuxInfo {
        TmuxInfo {
            path: self.bin.display().to_string(),
            version: self.version.clone(),
            bundled: self.bundled,
            socket: self.socket.clone(),
        }
    }

    pub fn bin(&self) -> &Path {
        &self.bin
    }

    /// 带 socket、配置文件和 UTF-8 标志的基础命令
    fn base(&self) -> Command {
        let mut c = Command::new(&self.bin);
        c.arg("-u").arg("-L").arg(&self.socket).arg("-f").arg(&self.conf);
        c.envs(self.extra_env());
        c.stdin(Stdio::null());
        c
    }

    /// 运行内置 tmux 时需要补充的环境变量。
    ///
    /// Linux 版 tmux 在 Alpine 中静态编译，默认只查找 Alpine 的 terminfo 路径；
    /// 补上 Debian/Ubuntu 等发行版使用的位置。
    pub fn extra_env(&self) -> Vec<(String, String)> {
        let mut env = Vec::new();
        if self.bundled && cfg!(target_os = "linux") && std::env::var_os("TERMINFO_DIRS").is_none() {
            env.push((
                "TERMINFO_DIRS".into(),
                "/etc/terminfo:/lib/terminfo:/usr/share/terminfo:/usr/lib/terminfo".into(),
            ));
        }
        env
    }

    /// 交互 attach 使用的程序与参数（交给 PTY 执行）
    pub fn attach_argv(&self, session: &str) -> (PathBuf, Vec<String>) {
        (
            self.bin.clone(),
            vec![
                "-u".into(),
                "-L".into(),
                self.socket.clone(),
                "-f".into(),
                self.conf.display().to_string(),
                "attach-session".into(),
                "-t".into(),
                format!("={session}"),
            ],
        )
    }

    /// 透传任意参数（CLI `tmuxide tmux ...` 使用）
    pub fn passthrough_command<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut c = Command::new(&self.bin);
        c.arg("-u")
            .arg("-L")
            .arg(&self.socket)
            .arg("-f")
            .arg(&self.conf)
            .args(args);
        c.envs(self.extra_env());
        c
    }

    fn raw(&self, args: &[&str]) -> Result<String> {
        let out = Command::new(&self.bin)
            .args(args)
            .envs(self.extra_env())
            .stdin(Stdio::null())
            .output()?;
        if !out.status.success() {
            return Err(TmuxError::Command(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// 执行 tmux 子命令，返回标准输出
    pub fn run(&self, args: &[&str]) -> Result<String> {
        tracing::debug!(?args, "tmux");
        let out = self.base().args(args).output()?;
        if !out.status.success() {
            return Err(TmuxError::Command(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// 列出所有会话及其窗口和面板；server 未运行时返回空列表。
    /// 远程访问使用的内部分组会话（`_tmuxide_` 开头）不在其中。
    pub fn sessions(&self) -> Result<Vec<SessionInfo>> {
        match self.run(&["list-panes", "-a", "-F", parse::PANE_FORMAT]) {
            Ok(text) => Ok(parse::parse_panes(&text)
                .into_iter()
                .filter(|s| !is_hidden(&s.name))
                .collect()),
            Err(TmuxError::Command(msg)) if is_no_server(&msg) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// 为远程客户端创建一个与 `target` 共享窗口的分组会话：
    /// 远程端切换窗口不会影响电脑上的当前窗口。调用方在客户端断开后负责关闭它；
    /// 异常退出遗留的由 [`Tmux::cleanup_hidden`] 清理。
    pub fn new_grouped(&self, target: &str, name: &str) -> Result<()> {
        if !is_hidden(name) || !valid_session_name(name) {
            return Err(TmuxError::BadName(name.to_string()));
        }
        self.run(&["new-session", "-d", "-t", &format!("={target}"), "-s", name])?;
        // 状态栏显示原会话名，而不是内部名称
        let _ = self.run(&[
            "set-option",
            "-t",
            &format!("={name}:"),
            "status-left",
            &format!("[{}] ", target.replace('#', "##")),
        ]);
        Ok(())
    }

    /// 清理没有客户端的内部分组会话（例如上次异常退出留下的）
    pub fn cleanup_hidden(&self) {
        let Ok(text) = self.run(&["list-sessions", "-F", "#{session_name} #{session_attached}"]) else {
            return;
        };
        for line in text.lines() {
            if let Some((name, attached)) = line.rsplit_once(' ') {
                if is_hidden(name) && attached == "0" {
                    let _ = self.kill_session(name);
                }
            }
        }
    }

    /// 远程 attach：`read_only` 时客户端不能输入，`ignore_size` 时不改变窗口尺寸
    pub fn attach_argv_with(&self, session: &str, read_only: bool, ignore_size: bool) -> (PathBuf, Vec<String>) {
        let (bin, mut args) = self.attach_argv(session);
        let mut flags = Vec::new();
        if read_only {
            flags.push("read-only");
        }
        if ignore_size {
            flags.push("ignore-size");
        }
        if !flags.is_empty() {
            args.push("-f".into());
            args.push(flags.join(","));
        }
        (bin, args)
    }

    /// 一次 tmux 调用抓取多个面板当前屏幕的最后 `lines` 行（纯文本，去掉末尾空行）
    pub fn capture_many(&self, pane_ids: &[String], lines: usize) -> Result<Vec<String>> {
        if pane_ids.is_empty() {
            return Ok(Vec::new());
        }
        const MARK: &str = "@@tmuxide-pane-7f3a@@";
        let mut args: Vec<&str> = Vec::new();
        for (i, id) in pane_ids.iter().enumerate() {
            if i > 0 {
                args.push(";");
            }
            args.extend(["display-message", "-p", MARK, ";", "capture-pane", "-p", "-t", id]);
        }
        let out = self.run(&args)?;
        let parts: Vec<String> = out
            .split(&format!("{MARK}\n"))
            .skip(1)
            .map(|chunk| {
                let all: Vec<&str> = chunk.lines().collect();
                let end = all.iter().rposition(|l| !l.trim().is_empty()).map_or(0, |i| i + 1);
                all[end.saturating_sub(lines)..end].join("\n")
            })
            .collect();
        Ok(parts)
    }

    pub fn session(&self, name: &str) -> Result<SessionInfo> {
        self.sessions()?
            .into_iter()
            .find(|s| s.name == name)
            .ok_or_else(|| TmuxError::NoSession(name.to_string()))
    }

    pub fn has_session(&self, name: &str) -> bool {
        self.run(&["has-session", "-t", &format!("={name}")]).is_ok()
    }

    pub fn new_session(&self, name: &str, cwd: Option<&Path>) -> Result<()> {
        check_name(name)?;
        if self.has_session(name) {
            return Err(TmuxError::Exists(name.to_string()));
        }
        let cwd = cwd.map(|p| p.display().to_string()).unwrap_or_else(home_string);
        self.run(&["new-session", "-d", "-s", name, "-c", &cwd, "-x", "200", "-y", "50"])?;
        Ok(())
    }

    pub fn kill_session(&self, name: &str) -> Result<()> {
        self.run(&["kill-session", "-t", &format!("={name}")]).map(|_| ())
    }

    /// 设置会话分组（保存在 tmux 会话选项里，改名后仍然保留）；空字符串表示移出分组
    pub fn set_session_group(&self, session: &str, group: &str) -> Result<()> {
        let group = group.trim();
        if group.chars().count() > 32 || group.contains(parse::SEP) || group.chars().any(char::is_control) {
            return Err(TmuxError::BadGroup(group.to_string()));
        }
        if !self.has_session(session) || is_hidden(session) {
            return Err(TmuxError::NoSession(session.to_string()));
        }
        let target = format!("={session}:");
        if group.is_empty() {
            self.run(&["set-option", "-u", "-t", &target, "@tmuxide-group"])
                .map(|_| ())
        } else {
            self.run(&["set-option", "-t", &target, "@tmuxide-group", group])
                .map(|_| ())
        }
    }

    pub fn rename_session(&self, from: &str, to: &str) -> Result<()> {
        check_name(to)?;
        if self.has_session(to) {
            return Err(TmuxError::Exists(to.to_string()));
        }
        self.run(&["rename-session", "-t", &format!("={from}"), to]).map(|_| ())
    }

    pub fn new_window(&self, session: &str, name: Option<&str>) -> Result<()> {
        let target = format!("={session}:");
        let mut args = vec!["new-window", "-t", &target];
        if let Some(n) = name.filter(|n| !n.is_empty()) {
            args.extend(["-n", n]);
        }
        // 新窗口沿用当前活动面板目录
        args.extend(["-c", "#{pane_current_path}"]);
        self.run(&args).map(|_| ())
    }

    pub fn select_window(&self, window_id: &str) -> Result<()> {
        self.run(&["select-window", "-t", window_id]).map(|_| ())
    }

    pub fn kill_window(&self, window_id: &str) -> Result<()> {
        self.run(&["kill-window", "-t", window_id]).map(|_| ())
    }

    pub fn rename_window(&self, window_id: &str, name: &str) -> Result<()> {
        self.run(&["rename-window", "-t", window_id, name]).map(|_| ())
    }

    /// 面板的输出历史（含当前屏幕），最多 `lines` 行，去掉末尾空行
    pub fn history(&self, pane_id: &str, lines: u32) -> Result<String> {
        if !valid_pane_id(pane_id) {
            return Err(TmuxError::Command(format!("面板 id 不合法：{pane_id}")));
        }
        let text = self.capture(pane_id, lines)?;
        Ok(text.trim_end().to_string())
    }

    /// 抓取面板滚动缓冲区（纯文本）
    pub fn capture(&self, pane_id: &str, lines: u32) -> Result<String> {
        let start = format!("-{lines}");
        self.run(&["capture-pane", "-p", "-J", "-S", &start, "-t", pane_id])
    }

    /// 把运行中的会话描述为快照
    pub fn snapshot(&self, name: &str, host: &str, saved_at: &str) -> Result<SessionSnapshot> {
        let info = self.session(name)?;
        let home = home_dir();
        let procs = ps::ProcessTable::load();
        let windows = info
            .windows
            .iter()
            .map(|w| WindowSnapshot {
                index: w.index,
                name: w.name.clone(),
                layout: w.layout.clone(),
                active: w.active,
                panes: w
                    .panes
                    .iter()
                    .map(|p| PaneSnapshot {
                        index: p.index,
                        cwd: collapse_home(&p.cwd, home.as_deref()),
                        cmd: if is_shell(&p.command) {
                            None
                        } else {
                            procs
                                .as_ref()
                                .and_then(|t| t.foreground_args(p.pid))
                                .or_else(|| Some(p.command.clone()))
                                .filter(|c| !c.is_empty())
                        },
                        active: p.active,
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        let root = windows.first().and_then(|w| w.panes.first()).map(|p| p.cwd.clone());
        let snap = SessionSnapshot {
            version: SNAPSHOT_VERSION,
            name: info.name.clone(),
            host: host.to_string(),
            saved_at: saved_at.to_string(),
            root,
            windows,
        };
        snap.validate()?;
        Ok(snap)
    }

    /// 按快照恢复为名为 `target` 的会话
    pub fn restore(&self, snap: &SessionSnapshot, target: &str, run_commands: bool) -> Result<restore::RestoreReport> {
        check_name(target)?;
        snap.validate()?;
        if self.has_session(target) {
            return Err(TmuxError::Exists(target.to_string()));
        }
        restore::execute(self, snap, target, run_commands)
    }
}

pub use restore::RestoreReport;

const DEFAULT_CONF: &str = r#"# 由 TmuxIDE 生成：基础设置，随后加载用户自己的配置
set -g default-terminal "screen-256color"
set -g mouse on
set -g history-limit 50000
set -g base-index 1
setw -g pane-base-index 0
source-file -q ~/.tmux.conf
source-file -q ~/.config/tmux/tmux.conf
"#;

fn write_if_changed(path: &Path, content: &str) -> std::io::Result<()> {
    if std::fs::read_to_string(path).ok().as_deref() != Some(content) {
        std::fs::write(path, content)?;
    }
    Ok(())
}

/// 远程访问内部会话名前缀
pub const HIDDEN_PREFIX: &str = "_tmuxide_";

pub fn is_hidden(name: &str) -> bool {
    name.starts_with(HIDDEN_PREFIX)
}

fn is_no_server(msg: &str) -> bool {
    msg.contains("no server running")
        || msg.contains("error connecting to")
        || msg.contains("No such file or directory")
}

/// tmux 面板 id，如 `%12`
pub fn valid_pane_id(id: &str) -> bool {
    id.len() > 1 && id.starts_with('%') && id[1..].chars().all(|c| c.is_ascii_digit())
}

fn check_name(name: &str) -> Result<()> {
    if valid_session_name(name) && !is_hidden(name) {
        Ok(())
    } else {
        Err(TmuxError::BadName(name.to_string()))
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

fn home_string() -> String {
    home_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "/".into())
}

pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    // GUI 应用启动时 PATH 很短，补充常见位置以便找到系统 tmux
    for extra in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
        dirs.push(PathBuf::from(extra));
    }
    dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

//! 应用服务：把 tmux 适配、快照文件与 Git 仓库组合成界面需要的操作。
//!
//! 所有操作都是同步的（tmux 子进程、libgit2、文件 I/O），HTTP 层通过 `spawn_blocking` 调用。

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tmuxide_core::{restore_plan, sanitize_name, valid_session_name, Redactor, RestoreStep, SessionSnapshot};
use tmuxide_git::{diff_lines, Auth, CommitInfo, DiffLine, SnapshotRepo, SyncStatus};
use tmuxide_tmux::{RestoreReport, SessionInfo, Tmux, TmuxInfo, TmuxOptions};

use crate::{
    config::{write_atomic, AppPaths, Config},
    error::{AppError, AppResult},
    secrets::Secrets,
};

const KEY_HTTPS_TOKEN: &str = "git-https-token";
const KEY_SSH_PASSPHRASE: &str = "git-ssh-passphrase";

pub struct Service {
    paths: AppPaths,
    config: RwLock<Config>,
    tmux: RwLock<Arc<Tmux>>,
    repo: Mutex<SnapshotRepo>,
    secrets: Secrets,
    host: String,
    redactor: Redactor,
    /// 防止自动保存与手动保存同时写仓库
    save_lock: Mutex<()>,
}

#[derive(Debug, Serialize)]
pub struct AppInfo {
    pub version: &'static str,
    pub host: String,
    pub tmux: TmuxInfo,
    pub data_dir: String,
    pub repo_dir: String,
    pub git_engine: String,
    pub os: &'static str,
    pub arch: &'static str,
}

#[derive(Debug, Serialize)]
pub struct SavedEntry {
    pub host: String,
    pub name: String,
    pub saved_at: String,
    pub windows: usize,
    pub panes: usize,
    pub local: bool,
    pub running: bool,
}

#[derive(Debug, Serialize, Default)]
pub struct SaveResult {
    pub saved: Vec<String>,
    pub unchanged: Vec<String>,
    pub commit: Option<String>,
    pub pushed: bool,
    pub push_error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictMode {
    /// 恢复为另一个名字
    Rename,
    /// 关闭同名会话后恢复
    Replace,
    /// 已存在时什么也不做
    Skip,
}

#[derive(Debug, Deserialize)]
pub struct RestoreRequest {
    pub host: Option<String>,
    pub name: String,
    /// 恢复后的会话名，默认与快照同名
    pub target: Option<String>,
    #[serde(default = "default_mode")]
    pub mode: ConflictMode,
    #[serde(default = "yes")]
    pub run_commands: bool,
    /// 从某次历史提交恢复
    pub rev: Option<String>,
}

fn default_mode() -> ConflictMode {
    ConflictMode::Rename
}
fn yes() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct RestoreResult {
    pub session: String,
    pub skipped: bool,
    pub report: Option<RestoreReport>,
}

#[derive(Debug, Serialize)]
pub struct TemplateEntry {
    pub name: String,
    pub windows: Vec<String>,
    pub panes: usize,
}

#[derive(Debug, Serialize)]
pub struct SyncResult {
    pub pulled: bool,
    pub pushed: bool,
    pub status: SyncStatus,
}

/// 设置页读写的视图：真实配置 + 是否已保存凭据（凭据本身不回传）
#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsView {
    pub config: Config,
    #[serde(default)]
    pub has_token: bool,
    #[serde(default)]
    pub has_passphrase: bool,
    /// 写入时：Some("") 清除，Some(x) 设置，None 不变
    #[serde(default, skip_serializing)]
    pub token: Option<String>,
    #[serde(default, skip_serializing)]
    pub passphrase: Option<String>,
}

impl Service {
    pub fn new(paths: AppPaths) -> anyhow::Result<Service> {
        std::fs::create_dir_all(&paths.data_dir)?;
        let config = Config::load(&paths.config_file())?;
        if !paths.config_file().exists() {
            config.save(&paths.config_file())?;
        }
        let tmux = Tmux::new(&TmuxOptions {
            data_dir: paths.data_dir.clone(),
            use_system: config.tmux.use_system,
            socket: config.tmux.socket.clone(),
        })?;
        let repo = open_repo(&paths, &config)?;
        let host = sanitize_name(&gethostname::gethostname().to_string_lossy().replace(".local", ""));
        Ok(Service {
            secrets: Secrets::new(&paths.data_dir),
            paths,
            config: RwLock::new(config),
            tmux: RwLock::new(Arc::new(tmux)),
            repo: Mutex::new(repo),
            host,
            redactor: Redactor::default(),
            save_lock: Mutex::new(()),
        })
    }

    pub fn tmux(&self) -> Arc<Tmux> {
        self.tmux.read().unwrap().clone()
    }

    pub fn config(&self) -> Config {
        self.config.read().unwrap().clone()
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    fn repo_dir(&self) -> PathBuf {
        self.repo.lock().unwrap().workdir().to_path_buf()
    }

    pub fn info(&self) -> AppInfo {
        AppInfo {
            version: env!("CARGO_PKG_VERSION"),
            host: self.host.clone(),
            tmux: self.tmux().info(),
            data_dir: self.paths.data_dir.display().to_string(),
            repo_dir: self.repo_dir().display().to_string(),
            git_engine: "libgit2（内置）".into(),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        }
    }

    // ---------- 运行中的会话 ----------

    pub fn sessions(&self) -> AppResult<Vec<SessionInfo>> {
        Ok(self.tmux().sessions()?)
    }

    // ---------- 快照文件 ----------

    fn session_rel(host: &str, name: &str) -> String {
        format!("hosts/{host}/sessions/{name}.toml")
    }

    fn check_host(host: &str) -> AppResult<()> {
        if valid_session_name(host) {
            Ok(())
        } else {
            Err(AppError::BadRequest(format!("主机名 {host:?} 不合法")))
        }
    }

    fn check_name(name: &str) -> AppResult<()> {
        if valid_session_name(name) {
            Ok(())
        } else {
            Err(AppError::BadRequest(format!(
                "名称 {name:?} 不合法（只能包含字母、数字、- 和 _）"
            )))
        }
    }

    pub fn saved(&self) -> AppResult<Vec<SavedEntry>> {
        let running: Vec<String> = self
            .sessions()
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.name)
            .collect();
        let hosts_dir = self.repo_dir().join("hosts");
        let mut out = Vec::new();
        let Ok(hosts) = std::fs::read_dir(&hosts_dir) else {
            return Ok(out);
        };
        for h in hosts.flatten() {
            let host = h.file_name().to_string_lossy().into_owned();
            let Ok(files) = std::fs::read_dir(h.path().join("sessions")) else {
                continue;
            };
            for f in files.flatten() {
                let p = f.path();
                if p.extension().and_then(|e| e.to_str()) != Some("toml") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&p) else {
                    continue;
                };
                match SessionSnapshot::from_toml(&text) {
                    Ok(s) => out.push(SavedEntry {
                        local: host == self.host,
                        running: host == self.host && running.contains(&s.name),
                        host: host.clone(),
                        windows: s.windows.len(),
                        panes: s.pane_count(),
                        name: s.name,
                        saved_at: s.saved_at,
                    }),
                    Err(e) => tracing::warn!("跳过无法解析的快照 {}: {e}", p.display()),
                }
            }
        }
        out.sort_by(|a, b| (!a.local, &a.host, &a.name).cmp(&(!b.local, &b.host, &b.name)));
        Ok(out)
    }

    fn read_snapshot(&self, host: &str, name: &str, rev: Option<&str>) -> AppResult<SessionSnapshot> {
        Self::check_host(host)?;
        Self::check_name(name)?;
        let rel = Self::session_rel(host, name);
        let text = match rev {
            Some(rev) => self
                .repo
                .lock()
                .unwrap()
                .file_at(rev, &rel)?
                .ok_or_else(|| AppError::NotFound(format!("该版本中没有快照 {name}")))?,
            None => std::fs::read_to_string(self.repo_dir().join(&rel))
                .map_err(|_| AppError::NotFound(format!("没有找到快照 {host}/{name}")))?,
        };
        Ok(SessionSnapshot::from_toml(&text)?)
    }

    pub fn snapshot_text(&self, host: &str, name: &str, rev: Option<&str>) -> AppResult<String> {
        Ok(self.read_snapshot(host, name, rev)?.to_toml()?)
    }

    /// 保存会话（names 为空时保存全部未排除的运行中会话）
    pub fn save(&self, names: Option<Vec<String>>, reason: &str) -> AppResult<SaveResult> {
        let _guard = self.save_lock.lock().unwrap();
        let cfg = self.config();
        let tmux = self.tmux();
        let names = match names {
            Some(n) if !n.is_empty() => n,
            _ => tmux
                .sessions()?
                .into_iter()
                .map(|s| s.name)
                .filter(|n| !cfg.save.exclude.contains(n))
                .collect(),
        };
        let repo_dir = self.repo_dir();
        let now = chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
        let mut result = SaveResult::default();
        for name in &names {
            Self::check_name(name)?;
            let snap = tmux.snapshot(name, &self.host, &now)?;
            let path = repo_dir.join(Self::session_rel(&self.host, name));
            // 只有内容（除保存时间外）变化时才写入，避免自动保存制造空提交
            let unchanged = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| SessionSnapshot::from_toml(&t).ok())
                .map(|mut old| {
                    old.saved_at = snap.saved_at.clone();
                    old == snap
                })
                .unwrap_or(false);
            if unchanged {
                result.unchanged.push(name.clone());
                continue;
            }
            write_atomic(&path, snap.to_toml()?.as_bytes())?;
            if cfg.save.scrollback {
                self.save_scrollback(&tmux, name, &repo_dir, cfg.save.scrollback_lines)?;
            }
            result.saved.push(name.clone());
        }
        if cfg.git.sync_tmux_conf {
            self.copy_tmux_conf(&repo_dir)?;
        }
        ensure_repo_files(&repo_dir)?;
        if cfg.git.auto_commit {
            let msg = if result.saved.is_empty() {
                format!("{reason}: 更新配置 @ {}", self.host)
            } else {
                format!("{reason}({}) @ {}", result.saved.join(", "), self.host)
            };
            result.commit = self
                .repo
                .lock()
                .unwrap()
                .commit_all(&msg, &cfg.git.author_name, &cfg.git.author_email)?;
            if result.commit.is_some() && cfg.git.auto_push && !cfg.git.remote.is_empty() {
                match self.sync_inner(&cfg) {
                    Ok(_) => result.pushed = true,
                    Err(e) => result.push_error = Some(e.to_string()),
                }
            }
        }
        Ok(result)
    }

    fn save_scrollback(&self, tmux: &Tmux, name: &str, repo_dir: &Path, lines: u32) -> AppResult<()> {
        let info = tmux.session(name)?;
        let dir = repo_dir.join(format!("hosts/{}/scrollback/{name}", self.host));
        let _ = std::fs::remove_dir_all(&dir);
        for w in &info.windows {
            for p in &w.panes {
                let text = tmux.capture(&p.id, lines)?;
                let text = self.redactor.redact(text.trim_end());
                write_atomic(&dir.join(format!("{}-{}.txt", w.index, p.index)), text.as_bytes())?;
            }
        }
        Ok(())
    }

    fn copy_tmux_conf(&self, repo_dir: &Path) -> AppResult<()> {
        let Some(home) = tmuxide_tmux::home_dir() else {
            return Ok(());
        };
        for candidate in [home.join(".tmux.conf"), home.join(".config/tmux/tmux.conf")] {
            if let Ok(text) = std::fs::read_to_string(&candidate) {
                write_atomic(&repo_dir.join("tmux/tmux.conf"), text.as_bytes())?;
                break;
            }
        }
        Ok(())
    }

    pub fn delete_saved(&self, host: &str, name: &str) -> AppResult<()> {
        Self::check_host(host)?;
        Self::check_name(name)?;
        let _guard = self.save_lock.lock().unwrap();
        let repo_dir = self.repo_dir();
        let path = repo_dir.join(Self::session_rel(host, name));
        std::fs::remove_file(&path).map_err(|_| AppError::NotFound(format!("没有找到快照 {host}/{name}")))?;
        let _ = std::fs::remove_dir_all(repo_dir.join(format!("hosts/{host}/scrollback/{name}")));
        let cfg = self.config();
        if cfg.git.auto_commit {
            self.repo.lock().unwrap().commit_all(
                &format!("delete({name}) @ {}", self.host),
                &cfg.git.author_name,
                &cfg.git.author_email,
            )?;
        }
        Ok(())
    }

    pub fn restore_preview(&self, req: &RestoreRequest) -> AppResult<Vec<RestoreStep>> {
        let host = req.host.clone().unwrap_or_else(|| self.host.clone());
        let snap = self.read_snapshot(&host, &req.name, req.rev.as_deref())?;
        let target = req.target.clone().unwrap_or_else(|| snap.name.clone());
        Ok(restore_plan(&snap, &target, req.run_commands))
    }

    pub fn restore(&self, req: RestoreRequest) -> AppResult<RestoreResult> {
        let host = req.host.clone().unwrap_or_else(|| self.host.clone());
        let snap = self.read_snapshot(&host, &req.name, req.rev.as_deref())?;
        let tmux = self.tmux();
        let mut target = req.target.clone().unwrap_or_else(|| snap.name.clone());
        Self::check_name(&target)?;
        if tmux.has_session(&target) {
            match req.mode {
                ConflictMode::Skip => {
                    return Ok(RestoreResult {
                        session: target,
                        skipped: true,
                        report: None,
                    })
                }
                ConflictMode::Replace => tmux.kill_session(&target)?,
                ConflictMode::Rename => target = free_name(&tmux, &target),
            }
        }
        let report = tmux.restore(&snap, &target, req.run_commands)?;
        Ok(RestoreResult {
            session: target,
            skipped: false,
            report: Some(report),
        })
    }

    /// 恢复本机所有已保存但未运行的会话（开机后一键恢复）
    pub fn restore_all(&self) -> AppResult<Vec<RestoreResult>> {
        let mut out = Vec::new();
        for e in self.saved()?.into_iter().filter(|e| e.local && !e.running) {
            out.push(self.restore(RestoreRequest {
                host: Some(e.host),
                name: e.name,
                target: None,
                mode: ConflictMode::Skip,
                run_commands: true,
                rev: None,
            })?);
        }
        Ok(out)
    }

    // ---------- 模板 ----------

    pub fn templates(&self) -> AppResult<Vec<TemplateEntry>> {
        let dir = self.repo_dir().join("templates");
        let mut out = Vec::new();
        let Ok(files) = std::fs::read_dir(&dir) else {
            return Ok(out);
        };
        for f in files.flatten() {
            let Ok(text) = std::fs::read_to_string(f.path()) else {
                continue;
            };
            if let Ok(s) = SessionSnapshot::from_toml(&text) {
                out.push(TemplateEntry {
                    panes: s.pane_count(),
                    windows: s.windows.iter().map(|w| w.name.clone()).collect(),
                    name: s.name,
                });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub fn save_template(&self, session: &str, template: &str) -> AppResult<()> {
        Self::check_name(template)?;
        let _guard = self.save_lock.lock().unwrap();
        let now = chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
        let mut snap = self.tmux().snapshot(session, &self.host, &now)?;
        snap.name = template.to_string();
        let repo_dir = self.repo_dir();
        write_atomic(
            &repo_dir.join(format!("templates/{template}.toml")),
            snap.to_toml()?.as_bytes(),
        )?;
        ensure_repo_files(&repo_dir)?;
        let cfg = self.config();
        if cfg.git.auto_commit {
            self.repo.lock().unwrap().commit_all(
                &format!("template({template}) @ {}", self.host),
                &cfg.git.author_name,
                &cfg.git.author_email,
            )?;
        }
        Ok(())
    }

    pub fn delete_template(&self, template: &str) -> AppResult<()> {
        Self::check_name(template)?;
        let _guard = self.save_lock.lock().unwrap();
        std::fs::remove_file(self.repo_dir().join(format!("templates/{template}.toml")))
            .map_err(|_| AppError::NotFound(format!("没有找到模板 {template}")))?;
        let cfg = self.config();
        if cfg.git.auto_commit {
            self.repo.lock().unwrap().commit_all(
                &format!("delete template({template})"),
                &cfg.git.author_name,
                &cfg.git.author_email,
            )?;
        }
        Ok(())
    }

    pub fn new_session(&self, name: &str, cwd: Option<&str>, template: Option<&str>) -> AppResult<String> {
        Self::check_name(name)?;
        let tmux = self.tmux();
        match template.filter(|t| !t.is_empty()) {
            Some(t) => {
                Self::check_name(t)?;
                let text = std::fs::read_to_string(self.repo_dir().join(format!("templates/{t}.toml")))
                    .map_err(|_| AppError::NotFound(format!("没有找到模板 {t}")))?;
                let snap = SessionSnapshot::from_toml(&text)?;
                tmux.restore(&snap, name, true)?;
            }
            None => {
                let cwd = cwd
                    .filter(|c| !c.trim().is_empty())
                    .map(|c| PathBuf::from(tmuxide_core::expand_home(c.trim(), tmuxide_tmux::home_dir().as_deref())));
                if let Some(c) = &cwd {
                    if !c.is_dir() {
                        return Err(AppError::BadRequest(format!("目录 {} 不存在", c.display())));
                    }
                }
                tmux.new_session(name, cwd.as_deref())?;
            }
        }
        Ok(name.to_string())
    }

    // ---------- 历史 ----------

    pub fn history(&self, host: &str, name: &str) -> AppResult<Vec<CommitInfo>> {
        Self::check_host(host)?;
        Self::check_name(name)?;
        Ok(self.repo.lock().unwrap().history(&Self::session_rel(host, name), 200)?)
    }

    pub fn log(&self) -> AppResult<Vec<CommitInfo>> {
        Ok(self.repo.lock().unwrap().log(50)?)
    }

    /// 对比两个版本；`to` 为空表示工作区当前文件
    pub fn diff(&self, host: &str, name: &str, from: &str, to: Option<&str>) -> AppResult<Vec<DiffLine>> {
        Self::check_host(host)?;
        Self::check_name(name)?;
        let rel = Self::session_rel(host, name);
        let repo = self.repo.lock().unwrap();
        let old = repo.file_at(from, &rel)?.unwrap_or_default();
        let new = match to {
            Some(to) => repo.file_at(to, &rel)?.unwrap_or_default(),
            None => std::fs::read_to_string(repo.workdir().join(&rel)).unwrap_or_default(),
        };
        Ok(diff_lines(&old, &new))
    }

    // ---------- Git 同步 ----------

    fn auth(&self, cfg: &Config) -> Auth {
        match cfg.git.auth.as_str() {
            "ssh-key" => Auth::SshKey {
                path: PathBuf::from(&cfg.git.ssh_key),
                passphrase: self.secrets.get(KEY_SSH_PASSPHRASE),
            },
            "ssh-agent" => Auth::SshAgent,
            "https-token" => Auth::HttpsToken {
                username: cfg.git.username.clone(),
                token: self.secrets.get(KEY_HTTPS_TOKEN).unwrap_or_default(),
            },
            _ => Auth::None,
        }
    }

    pub fn git_status(&self) -> AppResult<SyncStatus> {
        Ok(self.repo.lock().unwrap().status()?)
    }

    fn sync_inner(&self, cfg: &Config) -> AppResult<SyncResult> {
        let auth = self.auth(cfg);
        let repo = self.repo.lock().unwrap();
        let pulled = repo.pull(&auth, &cfg.git.author_name, &cfg.git.author_email)?;
        let mut pushed = false;
        let st = repo.status()?;
        if st.ahead > 0 {
            repo.push(&auth)?;
            pushed = true;
        }
        Ok(SyncResult {
            pulled,
            pushed,
            status: repo.status()?,
        })
    }

    /// 先拉取再推送
    pub fn sync(&self) -> AppResult<SyncResult> {
        let _guard = self.save_lock.lock().unwrap();
        let cfg = self.config();
        if cfg.git.remote.trim().is_empty() {
            return Err(AppError::BadRequest("请先在设置中填写远程仓库地址".into()));
        }
        // 把未提交的变化先提交，避免拉取覆盖
        self.repo.lock().unwrap().commit_all(
            &format!("sync @ {}", self.host),
            &cfg.git.author_name,
            &cfg.git.author_email,
        )?;
        self.sync_inner(&cfg)
    }

    pub fn test_connection(&self) -> AppResult<()> {
        let cfg = self.config();
        if cfg.git.remote.trim().is_empty() {
            return Err(AppError::BadRequest("请先填写远程仓库地址".into()));
        }
        let auth = self.auth(&cfg);
        Ok(self.repo.lock().unwrap().test_connection(&auth)?)
    }

    // ---------- 设置 ----------

    pub fn settings(&self) -> SettingsView {
        SettingsView {
            config: self.config(),
            has_token: self.secrets.get(KEY_HTTPS_TOKEN).is_some(),
            has_passphrase: self.secrets.get(KEY_SSH_PASSPHRASE).is_some(),
            token: None,
            passphrase: None,
        }
    }

    pub fn update_settings(&self, view: SettingsView) -> AppResult<SettingsView> {
        let new = view.config;
        new.validate().map_err(AppError::BadRequest)?;
        if let Some(t) = view.token {
            self.secrets
                .set(KEY_HTTPS_TOKEN, &t)
                .map_err(|e| AppError::Other(e.to_string()))?;
        }
        if let Some(p) = view.passphrase {
            self.secrets
                .set(KEY_SSH_PASSPHRASE, &p)
                .map_err(|e| AppError::Other(e.to_string()))?;
        }
        let old = self.config();
        if new.tmux != old.tmux {
            let t = Tmux::new(&TmuxOptions {
                data_dir: self.paths.data_dir.clone(),
                use_system: new.tmux.use_system,
                socket: new.tmux.socket.clone(),
            })?;
            *self.tmux.write().unwrap() = Arc::new(t);
        }
        if new.git.repo_dir != old.git.repo_dir || new.git.branch != old.git.branch {
            let repo = open_repo(&self.paths, &new).map_err(|e| AppError::Other(e.to_string()))?;
            *self.repo.lock().unwrap() = repo;
        }
        self.repo.lock().unwrap().set_remote(&new.git.remote)?;
        new.save(&self.paths.config_file())
            .map_err(|e| AppError::Other(e.to_string()))?;
        *self.config.write().unwrap() = new;
        Ok(self.settings())
    }

    // ---------- 自动保存 ----------

    pub fn spawn_autosave(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut last = std::time::Instant::now();
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let minutes = self.config().save.autosave_minutes;
                if minutes == 0 || last.elapsed() < Duration::from_secs(minutes as u64 * 60) {
                    continue;
                }
                last = std::time::Instant::now();
                let svc = self.clone();
                match tokio::task::spawn_blocking(move || svc.save(None, "autosave")).await {
                    Ok(Ok(r)) if !r.saved.is_empty() => tracing::info!("自动保存：{:?}", r.saved),
                    Ok(Err(e)) => tracing::warn!("自动保存失败：{e}"),
                    _ => {}
                }
            }
        });
    }
}

fn open_repo(paths: &AppPaths, cfg: &Config) -> anyhow::Result<SnapshotRepo> {
    let dir = if cfg.git.repo_dir.trim().is_empty() {
        paths.default_repo_dir()
    } else {
        PathBuf::from(tmuxide_core::expand_home(
            cfg.git.repo_dir.trim(),
            tmuxide_tmux::home_dir().as_deref(),
        ))
    };
    let repo = SnapshotRepo::open_or_init(&dir, &cfg.git.branch)?;
    repo.set_remote(&cfg.git.remote)?;
    ensure_repo_files(&dir)?;
    Ok(repo)
}

fn ensure_repo_files(dir: &Path) -> std::io::Result<()> {
    let marker = dir.join("tmuxide.toml");
    if !marker.exists() {
        write_atomic(&marker, b"# TmuxIDE snapshot repository\nformat = 1\n")?;
    }
    let readme = dir.join("README.md");
    if !readme.exists() {
        write_atomic(
            &readme,
            "# tmux 会话快照\n\n由 [TmuxIDE](https://github.com/luckySpro/TmuxIDE) 维护。\n\n\
             - `hosts/<主机>/sessions/*.toml`：各电脑保存的会话\n\
             - `templates/*.toml`：会话模板\n\
             - `tmux/tmux.conf`：tmux 配置\n"
                .as_bytes(),
        )?;
    }
    Ok(())
}

fn free_name(tmux: &Tmux, base: &str) -> String {
    (2..)
        .map(|i| format!("{base}-{i}"))
        .find(|n| !tmux.has_session(n))
        .expect("总能找到可用名字")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> Option<(Service, tempfile::TempDir)> {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config {
            tmux: crate::config::TmuxConfig {
                use_system: false,
                socket: format!("tmuxide-svc-{}", std::process::id()),
            },
            ..Config::default()
        };
        cfg.save(&dir.path().join("config.toml")).unwrap();
        match Service::new(AppPaths {
            data_dir: dir.path().to_path_buf(),
        }) {
            Ok(s) => Some((s, dir)),
            Err(e) => {
                eprintln!("跳过：{e}");
                None
            }
        }
    }

    #[test]
    fn save_restore_templates_and_history() {
        let Some((svc, dir)) = service() else { return };
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        svc.new_session("alpha", Some(work.to_str().unwrap()), None).unwrap();

        let r = svc.save(None, "snapshot").unwrap();
        assert_eq!(r.saved, vec!["alpha"]);
        assert!(r.commit.is_some());
        // 内容没有变化时不重复提交
        let r2 = svc.save(None, "autosave").unwrap();
        assert_eq!(r2.unchanged, vec!["alpha"]);
        assert!(r2.commit.is_none());

        let saved = svc.saved().unwrap();
        assert_eq!(saved.len(), 1);
        assert!(saved[0].running);

        svc.tmux().new_window("alpha", Some("two")).unwrap();
        svc.save(Some(vec!["alpha".into()]), "snapshot").unwrap();
        let hist = svc.history(svc.host(), "alpha").unwrap();
        assert_eq!(hist.len(), 2);
        let diff = svc.diff(svc.host(), "alpha", &hist[1].id, Some(&hist[0].id)).unwrap();
        assert!(diff.iter().any(|l| l.tag == '+' && l.text.contains("two")));

        // 同名会话运行中：默认恢复为新名字；旧版本只有一个窗口
        let res = svc
            .restore(RestoreRequest {
                host: None,
                name: "alpha".into(),
                target: None,
                mode: ConflictMode::Rename,
                run_commands: true,
                rev: Some(hist[1].id.clone()),
            })
            .unwrap();
        assert_eq!(res.session, "alpha-2");
        assert_eq!(svc.tmux().session("alpha-2").unwrap().windows.len(), 1);

        svc.save_template("alpha", "tpl").unwrap();
        assert_eq!(svc.templates().unwrap()[0].windows.len(), 2);
        svc.new_session("fromtpl", None, Some("tpl")).unwrap();
        assert_eq!(svc.tmux().session("fromtpl").unwrap().windows.len(), 2);

        svc.delete_saved(svc.host(), "alpha").unwrap();
        assert!(svc.saved().unwrap().is_empty());
        let _ = svc.tmux().run(&["kill-server"]);
    }
}

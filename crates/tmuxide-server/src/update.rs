//! 自动升级：定期检查签名的升级目录，下载并安装新版本。
//!
//! 安装方式由宿主决定（[`Installer`]）：桌面版用 Tauri 升级插件替换整个应用并重启；
//! 命令行版替换当前的 `tmuxide` 可执行文件，下次启动生效。tmux 会话在独立的 tmux 服务里，升级和重启不会中断它们。

use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::Serialize;
use tmuxide_update::Catalog;

use crate::{
    error::{AppError, AppResult},
    service::Service,
};

pub type Progress = Arc<dyn Fn(u64, u64) + Send + Sync>;
pub type InstallFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

/// 宿主提供的安装方式
pub trait Installer: Send + Sync {
    /// desktop | cli
    fn kind(&self) -> &'static str;
    /// 下载、校验并安装目录中的版本
    fn install(&self, catalog: Catalog, progress: Progress) -> InstallFuture;
    /// 安装完成后能否直接重启进入新版本
    fn can_restart(&self) -> bool {
        false
    }
    fn restart(&self) {}
    /// 这台电脑不能自动安装时的原因（例如安装目录不可写），界面改为提供下载链接
    fn manual_reason(&self) -> Option<String> {
        None
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateView {
    pub enabled: bool,
    pub kind: &'static str,
    pub current: String,
    /// idle | checking | uptodate | available | downloading | ready | error
    pub state: &'static str,
    pub latest: Option<String>,
    pub notes: String,
    /// 当前版本低于最低支持版本，必须升级
    pub required: bool,
    pub downloaded: u64,
    pub total: u64,
    pub error: Option<String>,
    pub checked_at: Option<DateTime<Utc>>,
    pub release_url: Option<String>,
    pub manual_reason: Option<String>,
    pub can_restart: bool,
}

struct Inner {
    state: &'static str,
    catalog: Option<Catalog>,
    downloaded: u64,
    total: u64,
    error: Option<String>,
    checked_at: Option<DateTime<Utc>>,
}

pub struct Updater {
    svc: Arc<Service>,
    installer: Arc<dyn Installer>,
    inner: Mutex<Inner>,
}

const CURRENT: &str = env!("CARGO_PKG_VERSION");

impl Updater {
    pub fn new(svc: Arc<Service>, installer: Arc<dyn Installer>) -> Arc<Updater> {
        let u = Arc::new(Updater {
            svc,
            installer,
            inner: Mutex::new(Inner {
                state: "idle",
                catalog: None,
                downloaded: 0,
                total: 0,
                error: None,
                checked_at: None,
            }),
        });
        // 离线时沿用上次验证过的目录（例如“必须升级”的状态不会因为断网而消失）
        if let Some(c) = u.load_cache() {
            let mut i = u.inner.lock().unwrap();
            i.state = if tmuxide_update::newer(&c.version, CURRENT) {
                "available"
            } else {
                "uptodate"
            };
            i.catalog = Some(c);
        }
        u
    }

    fn cache_path(&self) -> PathBuf {
        self.svc.data_dir().join("update-cache.json")
    }

    /// 缓存里保存目录原文与签名，读取时重新校验
    fn load_cache(&self) -> Option<Catalog> {
        let text = std::fs::read_to_string(self.cache_path()).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        let data = v.get("catalog")?.as_str()?;
        let sig = v.get("sig")?.as_str()?;
        tmuxide_update::parse_catalog(data.as_bytes(), sig, tmuxide_update::pubkey()).ok()
    }

    fn save_cache(&self, raw: &str, sig: &str) {
        let v = serde_json::json!({ "catalog": raw, "sig": sig });
        let _ = std::fs::write(self.cache_path(), v.to_string());
    }

    pub fn view(&self) -> UpdateView {
        let i = self.inner.lock().unwrap();
        let c = i.catalog.as_ref();
        let latest = c.map(|c| c.version.clone());
        UpdateView {
            enabled: tmuxide_update::enabled(),
            kind: self.installer.kind(),
            current: CURRENT.to_string(),
            state: i.state,
            notes: c.map(|c| c.notes.clone()).unwrap_or_default(),
            required: c.is_some_and(|c| tmuxide_update::required(c, CURRENT)),
            release_url: latest.as_deref().map(tmuxide_update::release_page),
            latest,
            downloaded: i.downloaded,
            total: i.total,
            error: i.error.clone(),
            checked_at: i.checked_at,
            manual_reason: self.installer.manual_reason(),
            can_restart: self.installer.can_restart(),
        }
    }

    /// 检查新版本。只有签名有效、且不比已知目录旧的目录才会被采用（防止旧镜像降级）
    pub async fn check(self: &Arc<Self>) -> AppResult<UpdateView> {
        if !tmuxide_update::enabled() {
            return Err(AppError::BadRequest("此版本没有配置升级签名，不能自动升级".into()));
        }
        {
            let mut i = self.inner.lock().unwrap();
            if matches!(i.state, "checking" | "downloading" | "ready") {
                return Ok(drop_view(i, self));
            }
            i.state = "checking";
            i.error = None;
        }
        let fetched = tokio::task::spawn_blocking(tmuxide_update::fetch_catalog_raw)
            .await
            .map_err(|e| AppError::Other(e.to_string()))?;
        let mut i = self.inner.lock().unwrap();
        i.checked_at = Some(Utc::now());
        match fetched {
            Ok((c, raw, sig)) => {
                let older = i
                    .catalog
                    .as_ref()
                    .is_some_and(|known| tmuxide_update::newer(&known.version, &c.version));
                if !older {
                    self.save_cache(&raw, &sig);
                    i.catalog = Some(c);
                }
                let latest = i.catalog.as_ref().map(|c| c.version.clone()).unwrap_or_default();
                i.state = if tmuxide_update::newer(&latest, CURRENT) {
                    "available"
                } else {
                    "uptodate"
                };
            }
            Err(e) => {
                tracing::info!("检查更新失败：{e:#}");
                // 有已知的新版本时仍保留它，只记录这次失败
                i.state = match &i.catalog {
                    Some(c) if tmuxide_update::newer(&c.version, CURRENT) => "available",
                    _ => "error",
                };
                i.error = Some(format!("检查更新失败：{e:#}"));
            }
        }
        Ok(drop_view(i, self))
    }

    /// 下载并安装已知的新版本
    pub async fn install(self: &Arc<Self>) -> AppResult<UpdateView> {
        let catalog = {
            let mut i = self.inner.lock().unwrap();
            let Some(c) = i.catalog.clone().filter(|c| tmuxide_update::newer(&c.version, CURRENT)) else {
                return Err(AppError::BadRequest("没有可安装的新版本，请先检查更新".into()));
            };
            if matches!(i.state, "downloading" | "ready") {
                return Ok(drop_view(i, self));
            }
            if let Some(r) = self.installer.manual_reason() {
                return Err(AppError::BadRequest(r));
            }
            i.state = "downloading";
            i.downloaded = 0;
            i.total = 0;
            i.error = None;
            c
        };
        let me = self.clone();
        let progress: Progress = Arc::new(move |done, total| {
            let mut i = me.inner.lock().unwrap();
            i.downloaded = done;
            i.total = total;
        });
        let me = self.clone();
        let fut = self.installer.install(catalog.clone(), progress);
        tokio::spawn(async move {
            let r = fut.await;
            let mut i = me.inner.lock().unwrap();
            match r {
                Ok(()) => {
                    tracing::info!("已安装 {}", catalog.version);
                    i.state = "ready";
                }
                Err(e) => {
                    tracing::warn!("升级失败：{e}");
                    i.state = "available";
                    i.error = Some(e);
                }
            }
        });
        Ok(self.view())
    }

    pub fn restart(&self) -> AppResult<()> {
        if self.inner.lock().unwrap().state != "ready" {
            return Err(AppError::BadRequest("新版本还没有安装好".into()));
        }
        if !self.installer.can_restart() {
            return Err(AppError::BadRequest("请手动重新启动 TmuxIDE".into()));
        }
        // 稍等再重启，让这次请求的响应先返回给界面
        let installer = self.installer.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            installer.restart();
        });
        Ok(())
    }

    /// 启动后稍等再检查，之后每 6 小时检查一次（设置里可关闭）
    pub fn spawn_periodic(self: &Arc<Self>) {
        if !tmuxide_update::enabled() {
            return;
        }
        let me = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(20)).await;
            loop {
                if me.svc.config().ui.check_updates {
                    let _ = me.check().await;
                }
                tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
            }
        });
    }
}

fn drop_view(i: std::sync::MutexGuard<'_, Inner>, u: &Updater) -> UpdateView {
    drop(i);
    u.view()
}

/// 命令行版：替换当前的 tmuxide 可执行文件
pub struct CliInstaller;

impl CliInstaller {
    fn target() -> Result<PathBuf, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        std::fs::canonicalize(&exe).map_err(|e| e.to_string())
    }
}

impl Installer for CliInstaller {
    fn kind(&self) -> &'static str {
        "cli"
    }

    fn install(&self, catalog: Catalog, progress: Progress) -> InstallFuture {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || -> Result<(), String> {
                let platform = tmuxide_update::cli_platform().ok_or("这个平台没有命令行版安装包")?;
                let pkg = catalog
                    .cli
                    .get(&platform)
                    .ok_or_else(|| format!("新版本没有 {platform} 的安装包"))?;
                let bytes =
                    tmuxide_update::download_package(pkg, |d, t| progress(d, t)).map_err(|e| format!("{e:#}"))?;
                let target = Self::target()?;
                tmuxide_update::install_cli(&bytes, &target).map_err(|e| format!("{e:#}"))
            })
            .await
            .map_err(|e| e.to_string())?
        })
    }

    fn manual_reason(&self) -> Option<String> {
        let target = Self::target().ok()?;
        let dir = target.parent()?;
        let probe = dir.join(format!(".tmuxide-write-test-{}", std::process::id()));
        match std::fs::write(&probe, b"") {
            Ok(()) => {
                let _ = std::fs::remove_file(&probe);
                None
            }
            Err(_) => Some(format!(
                "没有权限写入 {}，请手动下载新版本，或用有权限的用户执行 tmuxide update",
                dir.display()
            )),
        }
    }
}

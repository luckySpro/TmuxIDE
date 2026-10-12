//! 桌面版升级：用 Tauri 升级插件下载安装包，校验签名后替换整个应用，再重启。

use std::sync::Arc;

use tauri::{AppHandle, Url};
use tauri_plugin_updater::UpdaterExt;
use tmuxide_server::update::{InstallFuture, Progress};
use tmuxide_update::Catalog;

pub struct DesktopInstaller {
    pub app: AppHandle,
}

/// latest.json 地址：先加速再官方
fn endpoints() -> Vec<Url> {
    let urls = match std::env::var("TMUXIDE_UPDATE_BASE") {
        Ok(base) => vec![format!("{}/latest.json", base.trim_end_matches('/'))],
        Err(_) => vec![
            format!(
                "{}{}",
                tmuxide_update::MIRROR,
                tmuxide_update::latest_url("latest-mirror.json")
            ),
            tmuxide_update::latest_url("latest.json"),
        ],
    };
    urls.iter().filter_map(|u| u.parse().ok()).collect()
}

async fn install_from(app: &AppHandle, endpoint: Url, catalog: &Catalog, progress: &Progress) -> Result<(), String> {
    let updater = app
        .updater_builder()
        .pubkey(tmuxide_update::pubkey())
        .endpoints(vec![endpoint])
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("读取安装包信息失败：{e}"))?
        .ok_or("没有找到新的安装包")?;
    // latest.json 没有签名：版本和安装包签名必须与已签名的升级目录一致
    if update.version.trim_start_matches('v') != catalog.version.trim_start_matches('v') {
        return Err(format!(
            "安装包版本 {} 与升级目录 {} 不一致",
            update.version, catalog.version
        ));
    }
    if !catalog.desktop.values().any(|s| s.trim() == update.signature.trim()) {
        return Err("安装包签名不在升级目录中，已拒绝".into());
    }
    let mut done = 0u64;
    update
        .download_and_install(
            |chunk, total| {
                done += chunk as u64;
                progress(done, total.unwrap_or(0));
            },
            || {},
        )
        .await
        .map_err(|e| format!("安装失败：{e}"))
}

impl tmuxide_server::Installer for DesktopInstaller {
    fn kind(&self) -> &'static str {
        "desktop"
    }

    fn install(&self, catalog: Catalog, progress: Progress) -> InstallFuture {
        let app = self.app.clone();
        Box::pin(async move {
            let mut last = String::from("没有可用的下载地址");
            for ep in endpoints() {
                match install_from(&app, ep.clone(), &catalog, &progress).await {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        tracing::warn!("从 {ep} 升级失败：{e}");
                        last = e;
                    }
                }
            }
            Err(last)
        })
    }

    fn can_restart(&self) -> bool {
        true
    }

    fn restart(&self) {
        self.app.request_restart();
    }

    fn manual_reason(&self) -> Option<String> {
        // Linux 只有 AppImage 和 deb 能自动升级；直接运行解压出的程序时只能手动下载
        #[cfg(target_os = "linux")]
        if std::env::var_os("APPIMAGE").is_none() && !is_deb_install() {
            return Some("当前运行方式不支持自动升级，请下载新版安装包".into());
        }
        None
    }
}

#[cfg(target_os = "linux")]
fn is_deb_install() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::canonicalize(p).ok())
        .is_some_and(|p| p.starts_with("/usr/bin") || p.starts_with("/usr/lib"))
}

pub fn installer(app: &AppHandle) -> Arc<dyn tmuxide_server::Installer> {
    Arc::new(DesktopInstaller { app: app.clone() })
}

//! TmuxIDE 应用服务：配置、快照存储、Git 同步，以及给桌面窗口和浏览器使用的本地 HTTP/WebSocket 接口。
//!
//! 服务只监听 127.0.0.1，并要求每个 API 请求携带启动时随机生成的访问令牌。
//! 远程访问（手机）使用单独的端口和认证，见 [`remote`]。

mod api;
mod assets;
pub mod config;
mod error;
mod pty;
pub mod remote;
mod secrets;
pub mod service;
pub mod update;

use std::{net::SocketAddr, sync::Arc};

pub use config::{AppPaths, Config};
pub use remote::Remote;
pub use service::Service;
pub use update::{CliInstaller, Installer, Updater};

pub struct ServeOptions {
    pub port: u16,
    pub paths: AppPaths,
    /// 安装新版本的方式（桌面版 / 命令行版）
    pub installer: Arc<dyn Installer>,
}

pub struct ServerHandle {
    pub addr: SocketAddr,
    pub token: String,
    pub service: Arc<Service>,
    pub remote: Arc<Remote>,
    pub updater: Arc<Updater>,
}

impl ServerHandle {
    /// 带令牌的界面地址（令牌放在 # 后面，不会出现在服务器日志或 Referer 中）
    pub fn url(&self) -> String {
        format!("http://{}/#token={}", self.addr, self.token)
    }
}

/// 启动服务并在后台运行，返回监听地址与令牌
pub async fn start(opts: ServeOptions) -> anyhow::Result<ServerHandle> {
    let service = Arc::new(Service::new(opts.paths)?);
    service.clone().spawn_autosave();
    let updater = Updater::new(service.clone(), opts.installer);
    let remote = Remote::new(service.clone(), updater.clone())?;
    let token = random_token();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", opts.port)).await?;
    let addr = listener.local_addr()?;
    let app = api::router(
        service.clone(),
        remote.clone(),
        updater.clone(),
        token.clone(),
        addr.port(),
    );
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("HTTP 服务退出：{e}");
        }
    });
    tracing::info!("TmuxIDE 服务已启动：http://{addr}");
    remote.autostart();
    updater.spawn_periodic();
    Ok(ServerHandle {
        addr,
        token,
        service,
        remote,
        updater,
    })
}

fn random_token() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    (0..32).map(|_| format!("{:02x}", rng.random::<u8>())).collect()
}

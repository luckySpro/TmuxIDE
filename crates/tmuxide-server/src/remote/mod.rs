//! 远程访问：让已配对的手机/平板经 Cloudflare 隧道或 Tailscale 查看、控制运行中的终端。
//!
//! - 远程流量只进入单独的远程端口，那里默认只有终端相关接口（`remote::api`）。
//!   只有在电脑上明确“允许远程管理”的设备，才能在 `/admin` 使用完整管理界面（远程访问本身的设置除外）。
//! - 认证：一次性配对码 → 设备凭证（Cookie，服务端只存 SHA-256）→ 可选 PIN 解锁（空闲超时）。
//! - 电脑端可看到在线设备与正在打开的终端，随时断开、设为只读或移除。

mod api;
pub mod auth;
pub mod store;
mod terminals;
pub mod tunnel;

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::{sync::watch, task::JoinHandle};

use crate::{
    config::RemoteConfig,
    error::{AppError, AppResult},
    service::Service,
};
use auth::{FailureWindow, Pairing, UnlockTable};
use store::{AuditEvent, Device, Store};
use terminals::TermList;
use tunnel::{SharedStatus, ToolInfo, TunnelStatus};

pub const KEY_CF_TOKEN: &str = "cloudflare-tunnel-token";

/// 一个远程连接（终端列表或某个终端）
struct Conn {
    device_id: String,
    device_name: String,
    ip: String,
    /// list | terminal
    kind: &'static str,
    target: String,
    started: DateTime<Utc>,
    kill: watch::Sender<bool>,
    enters: Arc<AtomicU64>,
}

#[derive(Debug, Serialize)]
pub struct ConnView {
    pub id: u64,
    pub device_id: String,
    pub device_name: String,
    pub ip: String,
    pub kind: &'static str,
    pub target: String,
    pub started: DateTime<Utc>,
    pub enters: u64,
}

struct Running {
    stop: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
    /// Tailscale serve 需要在停止时关闭：(tailscale 路径, HTTPS 端口)
    serve_off: Option<(String, u16)>,
}

/// 当前对外暴露方式，请求处理时用于校验 Host/Origin 与决定 Cookie 属性
#[derive(Debug, Clone, Default)]
struct Endpoint {
    /// HTTPS（隧道、Tailscale serve）时 Cookie 带 Secure 并使用 __Host- 前缀
    secure: bool,
    /// 本机端口模式：由用户的反向代理通过 X-Forwarded-Proto 告知是否为 HTTPS
    forwarded_proto: bool,
}

pub struct Remote {
    svc: Arc<Service>,
    pub store: Store,
    pairing: Mutex<Option<Pairing>>,
    failures: Mutex<FailureWindow>,
    unlocked: Mutex<UnlockTable>,
    conns: Mutex<HashMap<u64, Conn>>,
    next_id: AtomicU64,
    status: SharedStatus,
    endpoint: Mutex<Endpoint>,
    running: tokio::sync::Mutex<Option<Running>>,
    terms: watch::Sender<Arc<TermList>>,
}

#[derive(Debug, Serialize)]
pub struct PairingView {
    pub code: String,
    pub expires_in: u64,
    pub url: Option<String>,
    pub qr_svg: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RemoteView {
    pub config: RemoteConfig,
    pub has_token: bool,
    pub has_pin: bool,
    pub status: TunnelStatus,
    pub devices: Vec<DeviceView>,
    pub connections: Vec<ConnView>,
    pub audit: Vec<AuditEvent>,
    pub audit_seq: u64,
    pub pairing: Option<PairingView>,
    pub cloudflared: ToolInfo,
    pub tailscale: ToolInfo,
}

/// 远程管理请求的发起者，由远程端口在认证后放进请求扩展
#[derive(Clone)]
pub(crate) struct AdminCaller {
    pub device: Device,
    pub ip: String,
}

#[derive(Debug, Serialize)]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub last_ip: String,
    pub read_only: bool,
    pub admin: bool,
    pub locked: bool,
    pub online: bool,
}

impl Remote {
    pub fn new(svc: Arc<Service>) -> anyhow::Result<Arc<Remote>> {
        let store = Store::open(svc.data_dir())?;
        let (terms, _) = watch::channel(Arc::new(TermList::default()));
        Ok(Arc::new(Remote {
            svc,
            store,
            pairing: Mutex::new(None),
            failures: Mutex::new(FailureWindow::default()),
            unlocked: Mutex::new(UnlockTable::default()),
            conns: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            status: Arc::new(Mutex::new(TunnelStatus::off())),
            endpoint: Mutex::new(Endpoint::default()),
            running: tokio::sync::Mutex::new(None),
            terms,
        }))
    }

    fn config(&self) -> RemoteConfig {
        self.svc.config().remote
    }

    pub fn status(&self) -> TunnelStatus {
        self.status.lock().unwrap().clone()
    }

    fn set_status(&self, state: &str, url: Option<String>, message: impl Into<String>) {
        let mut s = self.status.lock().unwrap();
        s.state = state.into();
        s.url = url;
        s.message = message.into();
    }

    /// 配置开启时启动（失败只记录在状态里，不影响主程序）
    pub fn autostart(self: &Arc<Self>) {
        if self.config().enabled {
            let me = self.clone();
            tokio::spawn(async move {
                if let Err(e) = me.start().await {
                    tracing::warn!("远程访问启动失败：{e}");
                }
            });
        }
    }

    pub async fn restart(self: &Arc<Self>) -> AppResult<()> {
        self.stop().await;
        if self.config().enabled {
            self.start().await?;
        }
        Ok(())
    }

    pub async fn start(self: &Arc<Self>) -> AppResult<()> {
        let mut running = self.running.lock().await;
        if running.is_some() {
            return Ok(());
        }
        let cfg = self.config();
        cfg.validate().map_err(AppError::BadRequest)?;
        let result = self.start_inner(&cfg).await;
        match result {
            Ok(r) => {
                *running = Some(r);
                self.store.log(
                    "start",
                    None,
                    "",
                    format!("远程访问已开启（{}）", mode_label(&cfg.mode)),
                );
                Ok(())
            }
            Err(e) => {
                self.set_status("error", None, e.to_string());
                self.store.log("error", None, "", e.to_string());
                Err(e)
            }
        }
    }

    async fn start_inner(self: &Arc<Self>, cfg: &RemoteConfig) -> AppResult<Running> {
        if cfg.require_pin && !self.store.has_pin() {
            return Err(AppError::BadRequest("请先设置解锁 PIN，或关闭“解锁 PIN”".into()));
        }
        self.set_status("starting", None, "正在启动…");
        let data_dir = self.svc.data_dir().to_path_buf();

        // 先确认外部工具可用，再占用端口
        let (bind_ip, serve_bin, cf_bin) = match cfg.mode.as_str() {
            "local" => (IpAddr::V4(Ipv4Addr::LOCALHOST), None, None),
            "tailscale-direct" | "tailscale-serve" => {
                let ts = tunnel::tailscale();
                if !ts.found {
                    return Err(AppError::BadRequest(
                        "没有找到 Tailscale。请先在这台电脑上安装并登录 Tailscale（手机上也需要安装）".into(),
                    ));
                }
                if cfg.mode == "tailscale-direct" {
                    let ip = tunnel::tailscale_ipv4(&ts.path).await.map_err(AppError::BadRequest)?;
                    (IpAddr::V4(ip), Some(ts.path), None)
                } else {
                    (IpAddr::V4(Ipv4Addr::LOCALHOST), Some(ts.path), None)
                }
            }
            _ => {
                let cf = tokio::task::spawn_blocking(move || tunnel::cloudflared(&data_dir))
                    .await
                    .map_err(|e| AppError::Other(e.to_string()))?;
                if !cf.found {
                    return Err(AppError::BadRequest(
                        "此版本没有内置 cloudflared，系统中也没有找到".into(),
                    ));
                }
                (IpAddr::V4(Ipv4Addr::LOCALHOST), None, Some(cf.path))
            }
        };
        let token = if cfg.mode == "cloudflare-token" {
            let t = self.svc.secrets().get(KEY_CF_TOKEN).unwrap_or_default();
            if t.is_empty() {
                return Err(AppError::BadRequest("请先填写 Cloudflare Tunnel Token".into()));
            }
            Some(t)
        } else {
            None
        };

        let listener = tokio::net::TcpListener::bind(SocketAddr::new(bind_ip, cfg.port))
            .await
            .map_err(|e| AppError::BadRequest(format!("无法监听远程端口 {bind_ip}:{}：{e}", cfg.port)))?;
        *self.endpoint.lock().unwrap() = Endpoint {
            secure: !matches!(cfg.mode.as_str(), "tailscale-direct" | "local"),
            forwarded_proto: cfg.mode == "local",
        };

        let (stop_tx, stop_rx) = watch::channel(false);
        let mut tasks = Vec::new();
        let app = api::router(self.clone());
        let mut srv_stop = stop_rx.clone();
        tasks.push(tokio::spawn(async move {
            let shutdown = async move {
                let _ = srv_stop.wait_for(|s| *s).await;
            };
            if let Err(e) = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
                .with_graceful_shutdown(shutdown)
                .await
            {
                tracing::error!("远程端口服务退出：{e}");
            }
        }));
        tasks.push(tokio::spawn(terminals::broadcast(self.clone(), stop_rx.clone())));
        {
            let tmux = self.svc.tmux();
            let _ = tokio::task::spawn_blocking(move || tmux.cleanup_hidden()).await;
        }

        if cfg.keep_awake {
            if let Some(t) = keep_awake(stop_rx.clone()) {
                tasks.push(t);
            }
        }

        let mut serve_off = None;
        match cfg.mode.as_str() {
            "local" => {
                let url = if cfg.public_host.trim().is_empty() {
                    format!("http://127.0.0.1:{}", cfg.port)
                } else {
                    format!("https://{}", cfg.public_host.trim())
                };
                self.set_status(
                    "online",
                    Some(url),
                    format!("只监听 127.0.0.1:{}，请用自己的反向代理转发", cfg.port),
                );
            }
            "tailscale-direct" => {
                self.set_status(
                    "online",
                    Some(format!("http://{bind_ip}:{}", cfg.port)),
                    "已连接（仅 tailnet 内可访问）",
                );
            }
            "tailscale-serve" => {
                let bin = serve_bin.unwrap();
                let fail = |e: String| {
                    let _ = stop_tx.send(true);
                    AppError::BadRequest(e)
                };
                let name = tunnel::tailscale_dns_name(&bin).await.map_err(fail)?;
                tunnel::tailscale_serve_on(&bin, cfg.tailscale_https_port, cfg.port)
                    .await
                    .map_err(fail)?;
                let url = if cfg.tailscale_https_port == 443 {
                    format!("https://{name}")
                } else {
                    format!("https://{name}:{}", cfg.tailscale_https_port)
                };
                self.set_status("online", Some(url), "已连接（仅 tailnet 内可访问）");
                serve_off = Some((bin, cfg.tailscale_https_port));
            }
            _ => {
                let spec = tunnel::CloudflaredSpec {
                    bin: cf_bin.unwrap().into(),
                    local_port: cfg.port,
                    token,
                    public_host: cfg.public_host.trim().to_string(),
                    allowed_emails: if cfg.mode == "cloudflare-quick" {
                        cfg.allowed_emails.clone()
                    } else {
                        Vec::new()
                    },
                    config_file: self.svc.data_dir().join("remote").join("cloudflared.yml"),
                };
                tasks.push(tokio::spawn(tunnel::run_cloudflared(
                    spec,
                    self.status.clone(),
                    stop_rx,
                )));
            }
        }
        Ok(Running {
            stop: stop_tx,
            tasks,
            serve_off,
        })
    }

    pub async fn stop(&self) {
        let Some(r) = self.running.lock().await.take() else {
            return;
        };
        let _ = r.stop.send(true);
        if let Some((bin, port)) = &r.serve_off {
            tunnel::tailscale_serve_off(bin, *port).await;
        }
        self.kill_conns(|_| true);
        self.unlocked.lock().unwrap().clear();
        for t in r.tasks {
            if tokio::time::timeout(Duration::from_secs(3), t).await.is_err() {
                tracing::warn!("远程访问后台任务未能按时结束");
            }
        }
        self.set_status("off", None, "");
        self.store.log("stop", None, "", "远程访问已关闭");
    }

    // ---------- 配对 ----------

    pub fn create_pairing(&self) -> PairingView {
        *self.pairing.lock().unwrap() = Some(Pairing::new());
        self.pairing_view().expect("刚创建的配对码")
    }

    pub fn cancel_pairing(&self) {
        *self.pairing.lock().unwrap() = None;
    }

    pub fn pairing_view(&self) -> Option<PairingView> {
        let mut guard = self.pairing.lock().unwrap();
        if guard.as_ref().is_some_and(Pairing::expired) {
            *guard = None;
        }
        let p = guard.as_ref()?;
        let url = self
            .status()
            .url
            .map(|u| format!("{}/#pair={}", u.trim_end_matches('/'), p.code));
        let qr_svg = url.as_deref().and_then(qr_svg);
        Some(PairingView {
            code: p.display(),
            expires_in: p.remaining().as_secs(),
            url,
            qr_svg,
        })
    }

    /// 校验并消耗配对码；错误时计数
    fn redeem_pairing(&self, input: &str) -> Result<(), &'static str> {
        let mut guard = self.pairing.lock().unwrap();
        let Some(p) = guard.as_mut() else {
            return Err("没有正在进行的配对，请在电脑上点击“配对新设备”");
        };
        if p.expired() {
            *guard = None;
            return Err("配对码已过期，请在电脑上重新生成");
        }
        if auth::code_matches(&p.code, input) {
            *guard = None;
            Ok(())
        } else {
            p.failures += 1;
            if p.expired() {
                *guard = None;
                Err("配对码错误次数过多，已作废，请在电脑上重新生成")
            } else {
                Err("配对码不正确")
            }
        }
    }

    // ---------- 设备与连接 ----------

    pub fn update_device(
        &self,
        id: &str,
        name: Option<&str>,
        read_only: Option<bool>,
        admin: Option<bool>,
        unlock: bool,
    ) -> AppResult<()> {
        let before = self
            .store
            .device(id)
            .ok_or_else(|| AppError::NotFound("设备不存在".into()))?;
        if admin == Some(true) && !(self.config().require_pin && self.store.has_pin()) {
            return Err(AppError::BadRequest("允许远程管理前，请先设置并开启解锁 PIN".into()));
        }
        let dev = self
            .store
            .update_device(id, name, read_only, admin, unlock)
            .map_err(|e| AppError::Other(e.to_string()))?;
        if before.admin != dev.admin {
            self.kill_conns(|c| c.device_id == id);
            self.store.log(
                "update",
                Some(&dev.name),
                "",
                if dev.admin {
                    "允许远程管理"
                } else {
                    "取消远程管理"
                },
            );
        }
        if before.read_only != dev.read_only {
            // 权限变化后断开，让手机按新权限重新连接
            self.kill_conns(|c| c.device_id == id);
            self.store.log(
                "update",
                Some(&dev.name),
                "",
                if dev.read_only {
                    "设为只读"
                } else {
                    "设为可控制"
                },
            );
        }
        if unlock && before.locked {
            self.store.log("update", Some(&dev.name), "", "已在电脑上解除锁定");
        }
        Ok(())
    }

    pub fn remove_device(&self, id: &str) -> AppResult<()> {
        let dev = self
            .store
            .remove_device(id)
            .ok_or_else(|| AppError::NotFound("设备不存在".into()))?;
        self.unlocked.lock().unwrap().remove_device(id);
        self.kill_conns(|c| c.device_id == id);
        self.store.log("revoke", Some(&dev.name), "", "已在电脑上移除");
        Ok(())
    }

    /// “会话 › 序号 窗口名”，用于电脑上显示手机正在看哪个终端
    fn window_label(&self, session: &str, window: Option<&str>) -> String {
        let list = self.terms.borrow().clone();
        let name = window.and_then(|w| {
            list.sessions
                .iter()
                .find(|s| s.name == session)
                .and_then(|s| s.windows.iter().find(|x| x.id == w))
                .map(|x| format!("{} {}", x.index, x.name))
        });
        match name {
            Some(n) => format!("{session} › {n}"),
            None => session.to_string(),
        }
    }

    fn register_conn(
        &self,
        dev: &Device,
        ip: &str,
        kind: &'static str,
        target: String,
    ) -> (u64, watch::Receiver<bool>, Arc<AtomicU64>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (kill, rx) = watch::channel(false);
        let enters = Arc::new(AtomicU64::new(0));
        self.conns.lock().unwrap().insert(
            id,
            Conn {
                device_id: dev.id.clone(),
                device_name: dev.name.clone(),
                ip: ip.to_string(),
                kind,
                target,
                started: Utc::now(),
                kill,
                enters: enters.clone(),
            },
        );
        (id, rx, enters)
    }

    fn unregister_conn(&self, id: u64) {
        self.conns.lock().unwrap().remove(&id);
    }

    fn kill_conns(&self, pred: impl Fn(&Conn) -> bool) {
        for c in self.conns.lock().unwrap().values().filter(|c| pred(c)) {
            let _ = c.kill.send(true);
        }
    }

    /// 远程管理界面里的内嵌终端：登记连接，电脑上可看到并断开，移除设备时立即断开
    pub(crate) async fn admin_terminal(
        self: Arc<Self>,
        socket: axum::extract::ws::WebSocket,
        caller: AdminCaller,
        session: String,
        size: (u16, u16),
    ) {
        let target = format!("{}（管理界面）", self.window_label(&session, None));
        let (conn, kill, enters) = self.register_conn(&caller.device, &caller.ip, "terminal", target);
        self.store.log(
            "attach",
            Some(&caller.device.name),
            &caller.ip,
            format!("管理界面打开终端 {session}"),
        );
        let tmux = self.svc.tmux();
        let (bin, args) = tmux.attach_argv(&session);
        crate::pty::bridge_with(
            socket,
            crate::pty::BridgeSpec {
                bin,
                args,
                env: tmux.extra_env(),
                size,
                read_only: false,
                on_select: None,
                kill: Some(kill),
                enters: Some(enters.clone()),
            },
        )
        .await;
        self.unregister_conn(conn);
        let n = enters.load(Ordering::Relaxed);
        self.store.log(
            "detach",
            Some(&caller.device.name),
            &caller.ip,
            format!("管理界面关闭终端 {session}，发送 {n} 次回车"),
        );
    }

    pub fn disconnect(&self, id: u64) -> AppResult<()> {
        let conns = self.conns.lock().unwrap();
        let c = conns.get(&id).ok_or_else(|| AppError::NotFound("连接已断开".into()))?;
        let _ = c.kill.send(true);
        self.store.log(
            "kick",
            Some(&c.device_name),
            &c.ip,
            format!("在电脑上断开：{}", c.target),
        );
        Ok(())
    }

    pub fn connections(&self) -> Vec<ConnView> {
        let mut v: Vec<ConnView> = self
            .conns
            .lock()
            .unwrap()
            .iter()
            .map(|(id, c)| ConnView {
                id: *id,
                device_id: c.device_id.clone(),
                device_name: c.device_name.clone(),
                ip: c.ip.clone(),
                kind: c.kind,
                target: c.target.clone(),
                started: c.started,
                enters: c.enters.load(Ordering::Relaxed),
            })
            .collect();
        v.sort_by_key(|c| c.id);
        v
    }

    pub fn view(&self) -> RemoteView {
        let conns = self.connections();
        let devices = self
            .store
            .devices()
            .into_iter()
            .map(|d| DeviceView {
                online: conns.iter().any(|c| c.device_id == d.id),
                id: d.id,
                name: d.name,
                created_at: d.created_at,
                last_seen: d.last_seen,
                last_ip: d.last_ip,
                read_only: d.read_only,
                admin: d.admin,
                locked: d.locked,
            })
            .collect();
        RemoteView {
            config: self.config(),
            has_token: self.svc.secrets().get(KEY_CF_TOKEN).is_some(),
            has_pin: self.store.has_pin(),
            status: self.status(),
            devices,
            connections: conns,
            audit: self.store.audit(0, 50),
            audit_seq: self.store.audit_seq(),
            pairing: self.pairing_view(),
            cloudflared: tunnel::cloudflared(self.svc.data_dir()),
            tailscale: tunnel::tailscale(),
        }
    }

    pub fn set_token(&self, token: &str) -> AppResult<()> {
        self.svc
            .secrets()
            .set(KEY_CF_TOKEN, token.trim())
            .map_err(|e| AppError::Other(e.to_string()))
    }

    pub fn set_pin(&self, pin: &str) -> AppResult<()> {
        self.store
            .set_pin(pin)
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        // 修改 PIN 后所有设备需要重新输入
        self.unlocked.lock().unwrap().clear();
        self.store.log(
            "pin",
            None,
            "",
            if pin.is_empty() {
                "已清除 PIN"
            } else {
                "已修改 PIN"
            },
        );
        Ok(())
    }

    pub fn set_config(&self, cfg: RemoteConfig) -> AppResult<()> {
        if cfg.require_pin && cfg.enabled && !self.store.has_pin() {
            return Err(AppError::BadRequest("请先设置解锁 PIN，或关闭“解锁 PIN”".into()));
        }
        self.svc.set_remote_config(cfg)
    }
}

/// macOS：远程访问开启期间用系统自带的 caffeinate 阻止空闲睡眠（合上盖子仍会睡眠）
fn keep_awake(mut stop: watch::Receiver<bool>) -> Option<JoinHandle<()>> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let mut child = tokio::process::Command::new("/usr/bin/caffeinate")
        .args(["-i", "-w", &std::process::id().to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| tracing::warn!("无法启动 caffeinate：{e}"))
        .ok()?;
    Some(tokio::spawn(async move {
        let _ = stop.wait_for(|s| *s).await;
        let _ = child.kill().await;
    }))
}

fn mode_label(mode: &str) -> &'static str {
    match mode {
        "cloudflare-quick" => "Cloudflare 快速隧道",
        "cloudflare-token" => "Cloudflare 自有域名",
        "tailscale-serve" => "Tailscale",
        "tailscale-direct" => "Tailscale 直连",
        "local" => "本机端口",
        _ => "未知方式",
    }
}

fn qr_svg(text: &str) -> Option<String> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    Some(
        code.render::<svg::Color>()
            .min_dimensions(200, 200)
            .dark_color(svg::Color("#0d1014"))
            .light_color(svg::Color("#ffffff"))
            .build(),
    )
}

/// 在终端里显示的二维码（CLI `serve --pair`）
pub fn qr_text(text: &str) -> Option<String> {
    use qrcode::render::unicode;
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    Some(
        code.render::<unicode::Dense1x2>()
            .dark_color(unicode::Dense1x2::Light)
            .light_color(unicode::Dense1x2::Dark)
            .build(),
    )
}

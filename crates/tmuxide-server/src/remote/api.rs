//! 远程端口上的接口（手机页面使用）。这里只有配对、解锁和终端相关的接口。

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    extract::{
        ws::{Message, WebSocket},
        ConnectInfo, Query, Request, State, WebSocketUpgrade,
    },
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;

use super::{
    auth::{self, cookie, set_cookie},
    store::{sha256_hex, Device, PinError},
    terminals, Remote,
};
use crate::pty::{bridge_with, BridgeSpec};

const DEVICE_COOKIE: &str = "tmx_d";
const SESSION_COOKIE: &str = "tmx_s";

type R = Arc<Remote>;

pub fn router(remote: R) -> Router {
    Router::new()
        .route("/api/me", get(me))
        .route("/api/pair", post(pair))
        .route("/api/unlock", post(unlock))
        .route("/api/lock", post(lock))
        .route("/api/forget", post(forget))
        .route("/api/events", get(events))
        .route("/api/attach", get(attach))
        .fallback(static_files)
        .layer(middleware::from_fn_with_state(remote.clone(), guard))
        .with_state(remote)
}

/// 请求来源信息
#[derive(Clone)]
struct Client {
    ip: String,
    /// 浏览器看到的站点（scheme://host），用于校验 Origin
    origin: String,
    secure: bool,
}

fn client(remote: &Remote, headers: &HeaderMap, peer: SocketAddr) -> Client {
    let ep = remote.endpoint.lock().unwrap().clone();
    let hv = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim);
    let secure = ep.secure || (ep.forwarded_proto && hv("x-forwarded-proto") == Some("https"));
    let mut ip = hv("cf-connecting-ip")
        .or_else(|| hv("x-forwarded-for").and_then(|v| v.split(',').next()))
        .map(str::to_string)
        .unwrap_or_else(|| peer.ip().to_string());
    if let Some(login) = hv("tailscale-user-login") {
        ip = format!("{ip}（{login}）");
    }
    let host = hv("host").unwrap_or_default();
    Client {
        ip: ip.chars().take(120).collect(),
        origin: format!("{}://{host}", if secure { "https" } else { "http" }),
        secure,
    }
}

/// 所有请求：校验 Host，补上安全响应头
async fn guard(State(remote): State<R>, req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    // 已知对外地址时，只接受发往该地址的请求（防 DNS rebinding 访问 127.0.0.1:端口）
    if let Some(url) = remote.status().url {
        let expected = url
            .split("://")
            .nth(1)
            .unwrap_or("")
            .trim_end_matches('/')
            .to_ascii_lowercase();
        let host_only = |h: &str| h.split(':').next().unwrap_or("").to_string();
        let ok = host == expected || host_only(&host) == host_only(&expected);
        if !expected.is_empty() && !ok {
            return (StatusCode::FORBIDDEN, "forbidden host").into_response();
        }
    }
    let is_api = req.uri().path().starts_with("/api/");
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    if is_api {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    resp
}

async fn static_files(headers: HeaderMap, uri: axum::http::Uri) -> Response {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    // 只允许连接本站自己的 WebSocket
    let safe_host: String = host
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
        .collect();
    let csp = format!(
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
         font-src 'self' data:; connect-src 'self' wss://{safe_host} ws://{safe_host}; manifest-src 'self'; \
         frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
    );
    crate::assets::mobile_asset(uri.path(), &csp)
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": msg }))).into_response()
}

/// 写操作和 WebSocket 必须来自本站页面
#[allow(clippy::result_large_err)]
fn check_origin(c: &Client, headers: &HeaderMap) -> Result<(), Response> {
    match headers.get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
        Some(o) if o.eq_ignore_ascii_case(&c.origin) => Ok(()),
        _ => Err(err(StatusCode::FORBIDDEN, "请求来源不正确")),
    }
}

enum AuthFail {
    /// 没有配对或凭证失效
    NotPaired,
    Locked,
    NeedPin,
}

impl AuthFail {
    fn response(&self) -> Response {
        match self {
            AuthFail::NotPaired => err(StatusCode::UNAUTHORIZED, "这台设备还没有配对，或配对已失效"),
            AuthFail::Locked => err(StatusCode::LOCKED, "这台设备已被锁定，请在电脑上解除"),
            AuthFail::NeedPin => (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "请输入 PIN 解锁", "need_pin": true })),
            )
                .into_response(),
        }
    }
}

/// 校验设备凭证；`with_pin` 时还要求 PIN 解锁会话有效
fn authenticate(remote: &Remote, c: &Client, headers: &HeaderMap, with_pin: bool) -> Result<Device, AuthFail> {
    let cookies = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()).unwrap_or("");
    let Some(token) = cookie(cookies, &auth::cookie_name(DEVICE_COOKIE, c.secure)) else {
        return Err(AuthFail::NotPaired);
    };
    let cfg = remote.config();
    let dev = match remote.store.find_by_token(token, cfg.device_days) {
        Ok(Some(d)) => d,
        Ok(None) => return Err(AuthFail::NotPaired),
        Err(name) => {
            remote.store.log(
                "expire",
                Some(&name),
                &c.ip,
                format!("{} 天未使用，凭证已失效", cfg.device_days),
            );
            return Err(AuthFail::NotPaired);
        }
    };
    if dev.locked {
        return Err(AuthFail::Locked);
    }
    if with_pin && cfg.require_pin {
        let ok = cookie(cookies, &auth::cookie_name(SESSION_COOKIE, c.secure))
            .map(|s| {
                remote.unlocked.lock().unwrap().check(
                    &sha256_hex(s),
                    &dev.id,
                    Duration::from_secs(cfg.idle_minutes as u64 * 60),
                )
            })
            .unwrap_or(false);
        if !ok {
            return Err(AuthFail::NeedPin);
        }
    }
    remote.store.touch(&dev.id, &c.ip);
    Ok(dev)
}

async fn me(State(remote): State<R>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    let c = client(&remote, &headers, peer);
    let host = remote.svc.host().to_string();
    let base = json!({ "host": host, "version": env!("CARGO_PKG_VERSION") });
    let state = match authenticate(&remote, &c, &headers, true) {
        Ok(dev) => json!({ "paired": true, "unlocked": true, "device": dev.name, "read_only": dev.read_only }),
        Err(AuthFail::NeedPin) => json!({ "paired": true, "unlocked": false, "need_pin": true }),
        Err(AuthFail::Locked) => json!({ "paired": true, "unlocked": false, "locked": true }),
        Err(AuthFail::NotPaired) => json!({ "paired": false, "unlocked": false }),
    };
    let mut v = base;
    v.as_object_mut().unwrap().extend(state.as_object().unwrap().clone());
    Json(v).into_response()
}

#[derive(Deserialize)]
struct PairBody {
    code: String,
    name: String,
}

async fn pair(
    State(remote): State<R>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(b): Json<PairBody>,
) -> Response {
    let c = client(&remote, &headers, peer);
    if let Err(r) = check_origin(&c, &headers) {
        return r;
    }
    if remote.failures.lock().unwrap().blocked() {
        return err(StatusCode::TOO_MANY_REQUESTS, "尝试次数过多，请稍后再试");
    }
    if let Err(msg) = remote.redeem_pairing(&b.code) {
        remote.failures.lock().unwrap().record();
        remote.store.log("pair_fail", None, &c.ip, msg);
        return err(StatusCode::BAD_REQUEST, msg);
    }
    let (dev, token) = match remote.store.add_device(&b.name, &c.ip) {
        Ok(x) => x,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    remote.store.log("pair", Some(&dev.name), &c.ip, "配对成功");
    let cfg = remote.config();
    let max_age = cfg.device_days as u64 * 86400;
    let mut resp = Json(json!({ "ok": true, "need_pin": cfg.require_pin })).into_response();
    resp.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&set_cookie(DEVICE_COOKIE, &token, c.secure, Some(max_age))).unwrap(),
    );
    resp
}

#[derive(Deserialize)]
struct UnlockBody {
    pin: String,
}

async fn unlock(
    State(remote): State<R>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(b): Json<UnlockBody>,
) -> Response {
    let c = client(&remote, &headers, peer);
    if let Err(r) = check_origin(&c, &headers) {
        return r;
    }
    if remote.failures.lock().unwrap().blocked() {
        return err(StatusCode::TOO_MANY_REQUESTS, "尝试次数过多，请稍后再试");
    }
    let dev = match authenticate(&remote, &c, &headers, false) {
        Ok(d) => d,
        Err(f) => return f.response(),
    };
    let r = remote.clone();
    let id = dev.id.clone();
    let pin = b.pin;
    let checked = tokio::task::spawn_blocking(move || r.store.check_pin(&id, &pin))
        .await
        .unwrap_or(Err(PinError::NotSet));
    match checked {
        Ok(()) => {
            let session = super::store::random_hex(32);
            remote
                .unlocked
                .lock()
                .unwrap()
                .insert(sha256_hex(&session), dev.id.clone());
            remote.store.log("unlock", Some(&dev.name), &c.ip, "PIN 验证通过");
            let mut resp = Json(json!({ "ok": true })).into_response();
            resp.headers_mut().append(
                header::SET_COOKIE,
                HeaderValue::from_str(&set_cookie(SESSION_COOKIE, &session, c.secure, None)).unwrap(),
            );
            resp
        }
        Err(PinError::Wrong(left)) => {
            remote.failures.lock().unwrap().record();
            remote.store.log(
                "unlock_fail",
                Some(&dev.name),
                &c.ip,
                format!("PIN 错误，剩余 {left} 次"),
            );
            err(StatusCode::UNAUTHORIZED, &PinError::Wrong(left).to_string())
        }
        Err(PinError::Locked) => {
            remote.failures.lock().unwrap().record();
            remote
                .store
                .log("locked", Some(&dev.name), &c.ip, "PIN 连续输错，设备已锁定");
            remote.kill_conns(|x| x.device_id == dev.id);
            err(StatusCode::LOCKED, &PinError::Locked.to_string())
        }
        Err(PinError::NotSet) => err(StatusCode::BAD_REQUEST, "电脑上还没有设置 PIN"),
    }
}

fn clear_cookie(resp: &mut Response, name: &str, secure: bool) {
    resp.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&set_cookie(name, "", secure, None)).unwrap(),
    );
}

async fn lock(State(remote): State<R>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    let c = client(&remote, &headers, peer);
    if let Err(r) = check_origin(&c, &headers) {
        return r;
    }
    let cookies = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()).unwrap_or("");
    if let Some(s) = cookie(cookies, &auth::cookie_name(SESSION_COOKIE, c.secure)) {
        remote.unlocked.lock().unwrap().remove(&sha256_hex(s));
    }
    let mut resp = Json(json!({ "ok": true })).into_response();
    clear_cookie(&mut resp, SESSION_COOKIE, c.secure);
    resp
}

/// 手机端“忘记这台电脑”：删除自己的设备记录
async fn forget(State(remote): State<R>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    let c = client(&remote, &headers, peer);
    if let Err(r) = check_origin(&c, &headers) {
        return r;
    }
    let dev = match authenticate(&remote, &c, &headers, false) {
        Ok(d) => d,
        Err(f) => return f.response(),
    };
    remote.store.remove_device(&dev.id);
    remote.unlocked.lock().unwrap().remove_device(&dev.id);
    remote.kill_conns(|x| x.device_id == dev.id);
    remote.store.log("revoke", Some(&dev.name), &c.ip, "设备自行取消配对");
    let mut resp = Json(json!({ "ok": true })).into_response();
    clear_cookie(&mut resp, SESSION_COOKIE, c.secure);
    clear_cookie(&mut resp, DEVICE_COOKIE, c.secure);
    resp
}

/// WebSocket 前的统一检查
#[allow(clippy::result_large_err)]
fn ws_auth(remote: &Remote, c: &Client, headers: &HeaderMap) -> Result<Device, Response> {
    check_origin(c, headers)?;
    authenticate(remote, c, headers, true).map_err(|f| f.response())
}

async fn events(
    State(remote): State<R>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let c = client(&remote, &headers, peer);
    let dev = match ws_auth(&remote, &c, &headers) {
        Ok(d) => d,
        Err(r) => return r,
    };
    ws.on_upgrade(move |socket| events_loop(socket, remote, dev, c))
}

async fn events_loop(socket: WebSocket, remote: R, dev: Device, c: Client) {
    let (id, mut kill, _) = remote.register_conn(&dev, &c.ip, "list", "终端列表".into());
    let mut rx = remote.terms.subscribe();
    let (mut tx, mut incoming) = socket.split();
    let first = terminals::snapshot(remote.clone()).await;
    let mut ok = tx
        .send(Message::Text(serde_json::to_string(&*first).unwrap().into()))
        .await
        .is_ok();
    let mut ping = tokio::time::interval(Duration::from_secs(25));
    while ok {
        tokio::select! {
            changed = rx.changed() => {
                if changed.is_err() { break; }
                let list = rx.borrow_and_update().clone();
                ok = tx.send(Message::Text(serde_json::to_string(&*list).unwrap().into())).await.is_ok();
            }
            msg = incoming.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
            _ = ping.tick() => {
                ok = tx.send(Message::Ping(Vec::new().into())).await.is_ok();
            }
            _ = kill.changed() => break,
        }
    }
    let _ = tx.send(Message::Close(None)).await;
    remote.unregister_conn(id);
}

#[derive(Deserialize)]
struct AttachQuery {
    session: String,
    window: Option<String>,
    /// 适配手机尺寸（会改变电脑上的窗口大小）
    #[serde(default)]
    fit: bool,
    cols: Option<u16>,
    rows: Option<u16>,
}

fn valid_window_id(id: &str) -> bool {
    id.len() > 1 && id.starts_with('@') && id[1..].chars().all(|c| c.is_ascii_digit())
}

async fn attach(
    State(remote): State<R>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<AttachQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    let c = client(&remote, &headers, peer);
    let dev = match ws_auth(&remote, &c, &headers) {
        Ok(d) => d,
        Err(r) => return r,
    };
    if !tmuxide_core::valid_session_name(&q.session) || tmuxide_tmux::is_hidden(&q.session) {
        return err(StatusCode::BAD_REQUEST, "会话名不合法");
    }
    if q.window.as_deref().is_some_and(|w| !valid_window_id(w)) {
        return err(StatusCode::BAD_REQUEST, "窗口 id 不合法");
    }
    let tmux = remote.svc.tmux();
    let id = remote.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let grouped = format!("{}m{id}{}", tmuxide_tmux::HIDDEN_PREFIX, super::store::random_hex(3));
    let setup = {
        let tmux = tmux.clone();
        let session = q.session.clone();
        let grouped = grouped.clone();
        let window = q.window.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            if !tmux.has_session(&session) {
                return Err(format!("会话 {session} 不存在"));
            }
            tmux.new_grouped(&session, &grouped).map_err(|e| e.to_string())?;
            if let Some(w) = window {
                let _ = tmux.select_window(&format!("={grouped}:{w}"));
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string()))
    };
    if let Err(e) = setup {
        return err(StatusCode::NOT_FOUND, &e);
    }
    let size = (q.cols.unwrap_or(80).clamp(10, 1000), q.rows.unwrap_or(24).clamp(5, 500));
    ws.on_upgrade(move |socket| async move {
        let target = remote.window_label(&q.session, q.window.as_deref());
        let (conn, kill, enters) = remote.register_conn(&dev, &c.ip, "terminal", target.clone());
        remote.store.log(
            "attach",
            Some(&dev.name),
            &c.ip,
            format!("打开终端 {}{}", q.session, if dev.read_only { "（只读）" } else { "" }),
        );
        let (bin, args) = tmux.attach_argv_with(&grouped, dev.read_only, !q.fit);
        let select_tmux = tmux.clone();
        let select_target = grouped.clone();
        bridge_with(
            socket,
            BridgeSpec {
                bin,
                args,
                env: tmux.extra_env(),
                size,
                read_only: dev.read_only,
                on_select: Some(Box::new(move |w: String| {
                    if valid_window_id(&w) {
                        let _ = select_tmux.select_window(&format!("={select_target}:{w}"));
                    }
                })),
                kill: Some(kill),
                enters: Some(enters.clone()),
            },
        )
        .await;
        remote.unregister_conn(conn);
        let t = tmux.clone();
        let g = grouped.clone();
        let _ = tokio::task::spawn_blocking(move || t.kill_session(&g)).await;
        let n = enters.load(std::sync::atomic::Ordering::Relaxed);
        remote.store.log(
            "detach",
            Some(&dev.name),
            &c.ip,
            format!("关闭终端 {}，输入了 {n} 次回车", q.session),
        );
    })
}

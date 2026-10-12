use std::sync::Arc;

use axum::{
    extract::{Path, Query, Request, State, WebSocketUpgrade},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config::RemoteConfig,
    error::{AppError, AppResult},
    remote::{AdminCaller, Remote},
    service::{RestoreRequest, Service, SettingsView},
    update::Updater,
};

#[derive(Clone)]
struct Ctx {
    svc: Arc<Service>,
    remote: Arc<Remote>,
    updater: Arc<Updater>,
    token: Arc<String>,
    port: u16,
    /// 经远程端口转发的管理请求（已在远程端口完成设备认证与来源校验）
    remote_admin: bool,
}

pub fn router(svc: Arc<Service>, remote: Arc<Remote>, updater: Arc<Updater>, token: String, port: u16) -> Router {
    let ctx = Ctx {
        svc,
        remote,
        updater,
        token: Arc::new(token),
        port,
        remote_admin: false,
    };
    let api = routes(false).layer(middleware::from_fn_with_state(ctx.clone(), require_token));
    Router::new()
        .nest("/api", api)
        .fallback(crate::assets::static_handler)
        .layer(middleware::from_fn_with_state(ctx.clone(), check_host))
        .with_state(ctx)
}

/// 远程管理使用的接口：由远程端口在设备认证之后转发进来。
/// 不包含远程访问自身的写接口，远程设备不能给自己或别的设备提权、改 PIN 或配对新设备。
pub(crate) fn admin_router(svc: Arc<Service>, remote: Arc<Remote>, updater: Arc<Updater>) -> Router {
    let ctx = Ctx {
        svc,
        remote,
        updater,
        token: Arc::new(String::new()),
        port: 0,
        remote_admin: true,
    };
    routes(true).with_state(ctx)
}

fn routes(remote_admin: bool) -> Router<Ctx> {
    let r = Router::new()
        .route("/info", get(info))
        .route("/sessions", get(sessions).post(new_session))
        .route("/sessions/{name}", delete(kill_session))
        .route("/sessions/{name}/rename", post(rename_session))
        .route("/sessions/{name}/group", put(set_group))
        .route("/sessions/{name}/windows", post(new_window))
        .route("/windows/{id}/select", post(select_window))
        .route("/panes/{id}/history", get(pane_history))
        .route("/windows/{id}/rename", post(rename_window))
        .route("/windows/{id}", delete(kill_window))
        .route("/saved", get(saved))
        .route("/saved/{host}/{name}", get(saved_text).delete(delete_saved))
        .route("/save", post(save))
        .route("/restore", post(restore))
        .route("/restore/preview", post(restore_preview))
        .route("/restore-all", post(restore_all))
        .route("/templates", get(templates).post(save_template))
        .route("/templates/{name}", delete(delete_template))
        .route("/history/{host}/{name}", get(history))
        .route("/diff/{host}/{name}", get(diff))
        .route("/git/status", get(git_status))
        .route("/git/log", get(git_log))
        .route("/git/sync", post(git_sync))
        .route("/git/test", post(git_test))
        .route("/settings", get(settings).put(update_settings))
        .route("/attach", get(attach))
        .route("/remote", get(remote_view))
        .route("/remote/events", get(remote_events))
        .route("/update", get(update_view));
    if remote_admin {
        return r;
    }
    // 安装与重启只能在这台电脑上操作：远程重启会断开远程连接本身
    r.route("/update/check", post(update_check))
        .route("/update/install", post(update_install))
        .route("/update/restart", post(update_restart))
        .route("/remote/config", put(remote_config))
        .route("/remote/pin", post(remote_pin))
        .route("/remote/pair", post(remote_pair).delete(remote_pair_cancel))
        .route(
            "/remote/devices/{id}",
            patch(remote_device).delete(remote_device_delete),
        )
        .route("/remote/connections/{id}", delete(remote_disconnect))
        .route("/remote/restart", post(remote_restart))
}

/// 防止 DNS rebinding：只接受发往本机地址的请求
async fn check_host(State(ctx): State<Ctx>, req: Request, next: Next) -> Response {
    let ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|h| h == format!("127.0.0.1:{}", ctx.port) || h == format!("localhost:{}", ctx.port))
        .unwrap_or(false);
    if !ok {
        return (StatusCode::FORBIDDEN, "forbidden host").into_response();
    }
    next.run(req).await
}

async fn require_token(State(ctx): State<Ctx>, req: Request, next: Next) -> Response {
    let header_token = req
        .headers()
        .get("x-tmuxide-token")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    // WebSocket 无法自定义请求头，允许通过查询参数传递
    let query_token = req.uri().query().and_then(|q| {
        q.split('&')
            .find_map(|kv| kv.strip_prefix("token=").map(str::to_string))
    });
    let given = header_token.or(query_token).unwrap_or_default();
    if !constant_eq(given.as_bytes(), ctx.token.as_bytes()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "访问令牌无效，请从 TmuxIDE 重新打开界面"})),
        )
            .into_response();
    }
    next.run(req).await
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 在阻塞线程池中执行服务方法
async fn run<T, F>(ctx: &Ctx, f: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce(&Service) -> AppResult<T> + Send + 'static,
{
    let svc = ctx.svc.clone();
    tokio::task::spawn_blocking(move || f(&svc))
        .await
        .map_err(|e| AppError::Other(format!("内部错误：{e}")))?
}

type J = AppResult<Json<Value>>;

fn ok<T: serde::Serialize>(v: T) -> J {
    Ok(Json(
        serde_json::to_value(v).map_err(|e| AppError::Other(e.to_string()))?,
    ))
}

async fn info(State(ctx): State<Ctx>) -> J {
    ok(run(&ctx, |s| Ok(s.info())).await?)
}

async fn sessions(State(ctx): State<Ctx>) -> J {
    ok(run(&ctx, |s| s.sessions()).await?)
}

#[derive(Deserialize)]
struct NewSession {
    name: String,
    cwd: Option<String>,
    template: Option<String>,
}

async fn new_session(State(ctx): State<Ctx>, Json(b): Json<NewSession>) -> J {
    ok(run(&ctx, move |s| {
        s.new_session(&b.name, b.cwd.as_deref(), b.template.as_deref())
    })
    .await?)
}

async fn kill_session(State(ctx): State<Ctx>, Path(name): Path<String>) -> J {
    ok(run(&ctx, move |s| Ok(s.tmux().kill_session(&name)?)).await?)
}

#[derive(Deserialize)]
struct Rename {
    to: String,
}

async fn rename_session(State(ctx): State<Ctx>, Path(name): Path<String>, Json(b): Json<Rename>) -> J {
    ok(run(&ctx, move |s| Ok(s.tmux().rename_session(&name, &b.to)?)).await?)
}

#[derive(Deserialize)]
pub(crate) struct HistoryQuery {
    pub lines: Option<u32>,
}

/// 面板的输出历史：最多 50000 行，默认 5000 行
pub(crate) fn history_lines(q: &HistoryQuery) -> u32 {
    q.lines.unwrap_or(5000).clamp(100, 50_000)
}

async fn pane_history(State(ctx): State<Ctx>, Path(id): Path<String>, Query(q): Query<HistoryQuery>) -> J {
    let lines = history_lines(&q);
    let text = run(&ctx, move |s| Ok(s.tmux().history(&id, lines)?)).await?;
    ok(json!({ "text": text }))
}

#[derive(Deserialize)]
struct Group {
    group: String,
}

async fn set_group(State(ctx): State<Ctx>, Path(name): Path<String>, Json(b): Json<Group>) -> J {
    ok(run(&ctx, move |s| Ok(s.tmux().set_session_group(&name, &b.group)?)).await?)
}

#[derive(Deserialize)]
struct NewWindow {
    name: Option<String>,
}

async fn new_window(State(ctx): State<Ctx>, Path(name): Path<String>, Json(b): Json<NewWindow>) -> J {
    ok(run(&ctx, move |s| Ok(s.tmux().new_window(&name, b.name.as_deref())?)).await?)
}

fn check_window_id(id: &str) -> AppResult<()> {
    if id.starts_with('@') && id[1..].chars().all(|c| c.is_ascii_digit()) && id.len() > 1 {
        Ok(())
    } else {
        Err(AppError::BadRequest("窗口 id 不合法".into()))
    }
}

async fn select_window(State(ctx): State<Ctx>, Path(id): Path<String>) -> J {
    check_window_id(&id)?;
    ok(run(&ctx, move |s| Ok(s.tmux().select_window(&id)?)).await?)
}

async fn rename_window(State(ctx): State<Ctx>, Path(id): Path<String>, Json(b): Json<Rename>) -> J {
    check_window_id(&id)?;
    ok(run(&ctx, move |s| Ok(s.tmux().rename_window(&id, &b.to)?)).await?)
}

async fn kill_window(State(ctx): State<Ctx>, Path(id): Path<String>) -> J {
    check_window_id(&id)?;
    ok(run(&ctx, move |s| Ok(s.tmux().kill_window(&id)?)).await?)
}

async fn saved(State(ctx): State<Ctx>) -> J {
    ok(run(&ctx, |s| s.saved()).await?)
}

#[derive(Deserialize)]
struct RevQuery {
    rev: Option<String>,
}

async fn saved_text(
    State(ctx): State<Ctx>,
    Path((host, name)): Path<(String, String)>,
    Query(q): Query<RevQuery>,
) -> J {
    ok(run(&ctx, move |s| s.snapshot_text(&host, &name, q.rev.as_deref())).await?)
}

async fn delete_saved(State(ctx): State<Ctx>, Path((host, name)): Path<(String, String)>) -> J {
    ok(run(&ctx, move |s| s.delete_saved(&host, &name)).await?)
}

#[derive(Deserialize, Default)]
struct SaveBody {
    sessions: Option<Vec<String>>,
}

async fn save(State(ctx): State<Ctx>, body: Option<Json<SaveBody>>) -> J {
    let b = body.map(|Json(b)| b).unwrap_or_default();
    ok(run(&ctx, move |s| s.save(b.sessions, "snapshot")).await?)
}

async fn restore(State(ctx): State<Ctx>, Json(b): Json<RestoreRequest>) -> J {
    ok(run(&ctx, move |s| s.restore(b)).await?)
}

async fn restore_preview(State(ctx): State<Ctx>, Json(b): Json<RestoreRequest>) -> J {
    ok(run(&ctx, move |s| s.restore_preview(&b)).await?)
}

async fn restore_all(State(ctx): State<Ctx>) -> J {
    ok(run(&ctx, |s| s.restore_all()).await?)
}

async fn templates(State(ctx): State<Ctx>) -> J {
    ok(run(&ctx, |s| s.templates()).await?)
}

#[derive(Deserialize)]
struct SaveTemplate {
    session: String,
    name: String,
}

async fn save_template(State(ctx): State<Ctx>, Json(b): Json<SaveTemplate>) -> J {
    ok(run(&ctx, move |s| s.save_template(&b.session, &b.name)).await?)
}

async fn delete_template(State(ctx): State<Ctx>, Path(name): Path<String>) -> J {
    ok(run(&ctx, move |s| s.delete_template(&name)).await?)
}

async fn history(State(ctx): State<Ctx>, Path((host, name)): Path<(String, String)>) -> J {
    ok(run(&ctx, move |s| s.history(&host, &name)).await?)
}

#[derive(Deserialize)]
struct DiffQuery {
    from: String,
    to: Option<String>,
}

async fn diff(State(ctx): State<Ctx>, Path((host, name)): Path<(String, String)>, Query(q): Query<DiffQuery>) -> J {
    ok(run(&ctx, move |s| s.diff(&host, &name, &q.from, q.to.as_deref())).await?)
}

async fn git_status(State(ctx): State<Ctx>) -> J {
    ok(run(&ctx, |s| s.git_status()).await?)
}

async fn git_log(State(ctx): State<Ctx>) -> J {
    ok(run(&ctx, |s| s.log()).await?)
}

async fn git_sync(State(ctx): State<Ctx>) -> J {
    ok(run(&ctx, |s| s.sync()).await?)
}

async fn git_test(State(ctx): State<Ctx>) -> J {
    ok(run(&ctx, |s| s.test_connection().map(|_| json!({ "ok": true }))).await?)
}

async fn settings(State(ctx): State<Ctx>) -> J {
    ok(run(&ctx, |s| Ok(s.settings())).await?)
}

async fn update_settings(State(ctx): State<Ctx>, Json(b): Json<SettingsView>) -> J {
    ok(run(&ctx, move |s| s.update_settings(b)).await?)
}

#[derive(Deserialize)]
struct AttachQuery {
    session: String,
    cols: Option<u16>,
    rows: Option<u16>,
}

async fn attach(
    State(ctx): State<Ctx>,
    headers: HeaderMap,
    caller: Option<Extension<AdminCaller>>,
    Query(q): Query<AttachQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if ctx.remote_admin {
        let Some(Extension(caller)) = caller else {
            return (StatusCode::FORBIDDEN, "forbidden").into_response();
        };
        if !tmuxide_core::valid_session_name(&q.session) {
            return AppError::BadRequest("会话名不合法".into()).into_response();
        }
        let size = (
            q.cols.unwrap_or(120).clamp(10, 1000),
            q.rows.unwrap_or(40).clamp(5, 500),
        );
        let remote = ctx.remote.clone();
        return ws.on_upgrade(move |socket| remote.admin_terminal(socket, caller, q.session, size));
    }
    // 浏览器会带 Origin：只允许本服务自己的页面连接
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
        let allowed = [
            format!("http://127.0.0.1:{}", ctx.port),
            format!("http://localhost:{}", ctx.port),
        ];
        if !allowed.iter().any(|a| a == origin) {
            return (StatusCode::FORBIDDEN, "forbidden origin").into_response();
        }
    }
    if !tmuxide_core::valid_session_name(&q.session) {
        return AppError::BadRequest("会话名不合法".into()).into_response();
    }
    let tmux = ctx.svc.tmux();
    let size = (
        q.cols.unwrap_or(120).clamp(10, 1000),
        q.rows.unwrap_or(40).clamp(5, 500),
    );
    ws.on_upgrade(move |socket| crate::pty::bridge(socket, tmux, q.session, size))
}

// ---------- 远程访问（电脑端管理） ----------

async fn remote_view(State(ctx): State<Ctx>) -> J {
    let r = ctx.remote.clone();
    ok(tokio::task::spawn_blocking(move || r.view())
        .await
        .map_err(|e| AppError::Other(e.to_string()))?)
}

#[derive(Deserialize)]
struct RemoteConfigBody {
    config: RemoteConfig,
    /// Some("") 清除，Some(x) 设置，None 不变
    token: Option<String>,
}

async fn remote_config(State(ctx): State<Ctx>, Json(b): Json<RemoteConfigBody>) -> J {
    if let Some(t) = &b.token {
        ctx.remote.set_token(t)?;
    }
    let before = ctx.svc.config().remote;
    ctx.remote.set_config(b.config.clone())?;
    // 网络相关设置变化或开关切换时重启；只改安全选项时立即生效，不断开
    let mut net_before = before.clone();
    let mut net_after = b.config.clone();
    for c in [&mut net_before, &mut net_after] {
        c.require_pin = false;
        c.idle_minutes = 0;
        c.device_days = 0;
        c.notify = false;
    }
    if net_before != net_after || b.token.is_some() {
        if let Err(e) = ctx.remote.restart().await {
            tracing::warn!("远程访问启动失败：{e}");
        }
    }
    remote_view(State(ctx)).await
}

#[derive(Deserialize)]
struct PinBody {
    pin: String,
}

async fn remote_pin(State(ctx): State<Ctx>, Json(b): Json<PinBody>) -> J {
    let r = ctx.remote.clone();
    tokio::task::spawn_blocking(move || r.set_pin(&b.pin))
        .await
        .map_err(|e| AppError::Other(e.to_string()))??;
    ok(json!({ "ok": true }))
}

async fn remote_pair(State(ctx): State<Ctx>) -> J {
    ok(ctx.remote.create_pairing())
}

async fn remote_pair_cancel(State(ctx): State<Ctx>) -> J {
    ctx.remote.cancel_pairing();
    ok(json!({ "ok": true }))
}

#[derive(Deserialize)]
struct DeviceBody {
    name: Option<String>,
    read_only: Option<bool>,
    admin: Option<bool>,
    #[serde(default)]
    unlock: bool,
}

async fn remote_device(State(ctx): State<Ctx>, Path(id): Path<String>, Json(b): Json<DeviceBody>) -> J {
    ctx.remote
        .update_device(&id, b.name.as_deref(), b.read_only, b.admin, b.unlock)?;
    ok(json!({ "ok": true }))
}

async fn remote_device_delete(State(ctx): State<Ctx>, Path(id): Path<String>) -> J {
    ctx.remote.remove_device(&id)?;
    ok(json!({ "ok": true }))
}

async fn remote_disconnect(State(ctx): State<Ctx>, Path(id): Path<u64>) -> J {
    ctx.remote.disconnect(id)?;
    ok(json!({ "ok": true }))
}

async fn remote_restart(State(ctx): State<Ctx>) -> J {
    ctx.remote.restart().await?;
    remote_view(State(ctx)).await
}

#[derive(Deserialize)]
struct SinceQuery {
    #[serde(default)]
    since: u64,
}

/// 电脑界面轮询：远程访问状态摘要和新的访问记录（用于弹出提醒）
async fn remote_events(State(ctx): State<Ctx>, Query(q): Query<SinceQuery>) -> J {
    let r = &ctx.remote;
    let status = r.status();
    let online = r.connections().iter().filter(|c| c.kind == "terminal").count();
    ok(json!({
        "state": status.state,
        "url": status.url,
        "notify": ctx.svc.config().remote.notify,
        "terminals": online,
        "seq": r.store.audit_seq(),
        "events": if q.since == 0 { Vec::new() } else { r.store.audit(q.since, 20) },
    }))
}

// ---------- 自动升级 ----------

async fn update_view(State(ctx): State<Ctx>) -> J {
    ok(ctx.updater.view())
}

async fn update_check(State(ctx): State<Ctx>) -> J {
    ok(ctx.updater.check().await?)
}

async fn update_install(State(ctx): State<Ctx>) -> J {
    ok(ctx.updater.install().await?)
}

async fn update_restart(State(ctx): State<Ctx>) -> J {
    ctx.updater.restart()?;
    ok(json!({ "ok": true }))
}

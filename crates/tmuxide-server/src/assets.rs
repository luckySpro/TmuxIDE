//! 内嵌的前端资源（web/dist），运行时不需要 Node 或任何外部文件。

use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../web/dist"]
#[allow_missing = true]
struct Assets;

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self' ws://127.0.0.1:* ws://localhost:*; frame-ancestors 'none'";

pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let (file, name) = match Assets::get(path) {
        Some(f) => (f, path),
        None if path == "index.html" || !path.contains('.') => match Assets::get("index.html") {
            Some(f) => (f, "index.html"),
            None => return missing(),
        },
        None => return (StatusCode::NOT_FOUND, "not found").into_response(),
    };
    respond(name, file.data.into_owned(), CSP)
}

fn respond(name: &str, data: Vec<u8>, csp: &str) -> Response {
    let mime = mime_guess::from_path(name).first_or_octet_stream();
    let cache = if name.ends_with(".html") || name.ends_with(".webmanifest") {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    };
    (
        [
            (header::CONTENT_TYPE, mime.as_ref().to_string()),
            (header::CACHE_CONTROL, cache.to_string()),
            (header::CONTENT_SECURITY_POLICY, csp.to_string()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        data,
    )
        .into_response()
}

/// 远程端口上的静态资源：只提供手机页面（m.html）及其脚本样式，不提供电脑界面
pub fn mobile_asset(path: &str, csp: &str) -> Response {
    let path = path.trim_start_matches('/');
    let name = match path {
        "" | "index.html" | "m.html" => "m.html",
        p if p.starts_with("assets/") || p == "manifest.webmanifest" || p.starts_with("icon-") => p,
        _ => return (StatusCode::NOT_FOUND, "not found").into_response(),
    };
    match Assets::get(name) {
        Some(f) => respond(name, f.data.into_owned(), csp),
        None if name == "m.html" => missing(),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn missing() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        "<h1>TmuxIDE</h1><p>此版本构建时没有包含前端界面，请先执行 <code>pnpm --dir web build</code> 再编译。</p>",
    )
        .into_response()
}

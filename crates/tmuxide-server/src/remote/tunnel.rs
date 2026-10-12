//! 把远程端口暴露出去：内置 cloudflared（快速隧道 / Tunnel Token）或本机已安装的 Tailscale。
//!
//! 所有外部程序都用参数数组启动，Tunnel Token 通过环境变量传入，不出现在进程参数里。

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::watch,
};

static CLOUDFLARED_BIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/cloudflared.bin"));

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TunnelStatus {
    /// off | starting | online | error
    pub state: String,
    pub url: Option<String>,
    pub message: String,
}

impl TunnelStatus {
    pub fn off() -> TunnelStatus {
        TunnelStatus {
            state: "off".into(),
            url: None,
            message: String::new(),
        }
    }
}

pub type SharedStatus = Arc<Mutex<TunnelStatus>>;

fn set(status: &SharedStatus, state: &str, url: Option<String>, message: impl Into<String>) {
    let mut s = status.lock().unwrap();
    s.state = state.into();
    if url.is_some() || state == "off" {
        s.url = url;
    }
    s.message = message.into();
}

// ---------- cloudflared ----------

#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    pub found: bool,
    pub bundled: bool,
    pub path: String,
    pub version: String,
}

pub fn cloudflared_bundled_version() -> Option<&'static str> {
    #[cfg(bundled_cloudflared)]
    {
        Some(env!("TMUXIDE_BUNDLED_CLOUDFLARED_VERSION"))
    }
    #[cfg(not(bundled_cloudflared))]
    {
        None
    }
}

/// 内置版释放到 `<数据目录>/bin/`；没有内置时查找系统安装
pub fn cloudflared(data_dir: &Path) -> ToolInfo {
    if !CLOUDFLARED_BIN.is_empty() {
        #[cfg(bundled_cloudflared)]
        let hash = env!("TMUXIDE_CLOUDFLARED_HASH");
        #[cfg(not(bundled_cloudflared))]
        let hash = "dev";
        match extract(&data_dir.join("bin"), &format!("cloudflared-{hash}"), CLOUDFLARED_BIN) {
            Ok(p) => {
                return ToolInfo {
                    found: true,
                    bundled: true,
                    path: p.display().to_string(),
                    version: cloudflared_bundled_version().unwrap_or("bundled").into(),
                }
            }
            Err(e) => tracing::warn!("无法释放内置 cloudflared：{e}"),
        }
    }
    match find_tool(
        "cloudflared",
        &["/opt/homebrew/bin/cloudflared", "/usr/local/bin/cloudflared"],
    ) {
        Some(p) => ToolInfo {
            found: true,
            bundled: false,
            path: p.display().to_string(),
            version: String::new(),
        },
        None => ToolInfo {
            found: false,
            bundled: false,
            path: String::new(),
            version: String::new(),
        },
    }
}

fn extract(dir: &Path, name: &str, data: &[u8]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let target = dir.join(name);
    if std::fs::metadata(&target)
        .map(|m| m.len() == data.len() as u64)
        .unwrap_or(false)
    {
        return Ok(target);
    }
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, &target)?;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("cloudflared-") && e.path() != target {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    Ok(target)
}

fn find_tool(name: &str, extra: &[&str]) -> Option<PathBuf> {
    // 从访达启动的应用 PATH 很短，所以还要检查常见安装位置
    tmuxide_tmux::find_in_path(name).or_else(|| extra.iter().map(PathBuf::from).find(|p| p.is_file()))
}

pub struct CloudflaredSpec {
    pub bin: PathBuf,
    pub local_port: u16,
    /// Some(token) = 自有域名（Tunnel Token），None = 快速隧道
    pub token: Option<String>,
    /// 自有域名模式的公开地址
    pub public_host: String,
    /// 快速隧道额外的邮箱验证
    pub allowed_emails: Vec<String>,
    /// 写给 cloudflared 的配置文件，避免读到用户 ~/.cloudflared/config.yml 里的 ingress
    pub config_file: PathBuf,
}

/// 运行并守护 cloudflared，退出后按退避时间重启，直到 `stop` 变为 true
pub async fn run_cloudflared(spec: CloudflaredSpec, status: SharedStatus, mut stop: watch::Receiver<bool>) {
    if let Err(e) = std::fs::write(
        &spec.config_file,
        "# 由 TmuxIDE 生成：隧道参数全部来自命令行\nno-autoupdate: true\n",
    ) {
        set(&status, "error", None, format!("无法写入 cloudflared 配置：{e}"));
        return;
    }
    let url_re = regex::Regex::new(r"https://[a-z0-9-]+\.trycloudflare\.com").unwrap();
    let mut backoff = 2u64;
    loop {
        if *stop.borrow() {
            break;
        }
        let mut cmd = Command::new(&spec.bin);
        cmd.arg("tunnel")
            .arg("--no-autoupdate")
            .arg("--config")
            .arg(&spec.config_file);
        match &spec.token {
            Some(token) => {
                cmd.arg("run").env("TUNNEL_TOKEN", token);
                set(
                    &status,
                    "starting",
                    Some(format!("https://{}", spec.public_host)),
                    "正在连接 Cloudflare…",
                );
            }
            None => {
                cmd.arg("--url").arg(format!("http://127.0.0.1:{}", spec.local_port));
                if !spec.allowed_emails.is_empty() {
                    cmd.arg("--allowed-mail").arg(spec.allowed_emails.join(","));
                }
                set(&status, "starting", None, "正在申请 Cloudflare 快速隧道…");
            }
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                set(&status, "error", None, format!("无法启动 cloudflared：{e}"));
                return;
            }
        };
        let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
        let mut last_error = String::new();
        let mut online = false;
        loop {
            tokio::select! {
                line = lines.next_line() => {
                    let Ok(Some(line)) = line else { break };
                    tracing::debug!(target: "cloudflared", "{line}");
                    if spec.token.is_none() {
                        // 失败信息里也会出现 api.trycloudflare.com，不是隧道地址
                        if let Some(m) = url_re.find(&line).filter(|m| m.as_str() != "https://api.trycloudflare.com") {
                            let mut s = status.lock().unwrap();
                            s.url = Some(m.as_str().to_string());
                        }
                    }
                    if line.contains("Registered tunnel connection") {
                        online = true;
                        backoff = 2;
                        set(&status, "online", None, "已连接");
                    } else if line.contains(" ERR ")
                        || line.contains("error=")
                        || !(line.contains(" INF ") || line.contains(" WRN ") || line.contains(" DBG ") || line.trim().is_empty())
                    {
                        if line.starts_with("See '") {
                            continue;
                        }
                        last_error = clean_log_line(&line);
                        if !online {
                            let mut s = status.lock().unwrap();
                            s.message = last_error.clone();
                        }
                    }
                }
                _ = stop.changed() => {
                    let _ = child.kill().await;
                    return;
                }
            }
        }
        let code = child.wait().await.ok().and_then(|s| s.code());
        let msg = if last_error.is_empty() {
            format!(
                "cloudflared 已退出（{}），{backoff} 秒后重试",
                code.map_or("信号".into(), |c| c.to_string())
            )
        } else {
            format!("{last_error}；{backoff} 秒后重试")
        };
        set(&status, "error", None, msg);
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(backoff)) => {}
            _ = stop.changed() => return,
        }
        backoff = (backoff * 2).min(60);
    }
}

/// cloudflared 的日志行形如 `2026-10-12T08:00:00Z ERR failed to ... error="..."`，去掉时间戳
fn clean_log_line(line: &str) -> String {
    let has_ts = line.as_bytes().first().is_some_and(u8::is_ascii_digit);
    let rest = if has_ts {
        line.split_once(' ').map_or(line, |(_, r)| r)
    } else {
        line
    };
    rest.trim_start_matches("ERR ").trim().chars().take(240).collect()
}

// ---------- Tailscale ----------

pub fn tailscale() -> ToolInfo {
    let p = find_tool(
        "tailscale",
        &[
            "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
            "/opt/homebrew/bin/tailscale",
            "/usr/local/bin/tailscale",
            "/usr/bin/tailscale",
        ],
    );
    ToolInfo {
        found: p.is_some(),
        bundled: false,
        path: p.map(|p| p.display().to_string()).unwrap_or_default(),
        version: String::new(),
    }
}

/// 执行 tailscale 子命令，超时后结束（serve 在 tailnet 未开启 HTTPS 时会一直等待用户去网页上开启）
async fn ts(bin: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = cmd.spawn().map_err(|e| format!("无法执行 tailscale：{e}"))?;
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            if out.status.success() {
                Ok(text)
            } else {
                Err(explain_tailscale(&text))
            }
        }
        Ok(Err(e)) => Err(format!("tailscale 执行失败：{e}")),
        // 超时：子进程随 kill_on_drop 结束
        Err(_) => Err("tailscale 没有响应。如果提示需要在网页上开启 Serve/HTTPS，请先开启后重试".into()),
    }
}

fn explain_tailscale(text: &str) -> String {
    let t = text.trim();
    if let Some(url) = t
        .split_whitespace()
        .find(|w| w.starts_with("https://login.tailscale.com"))
    {
        return format!("需要先在 Tailscale 后台开启此功能：{url}");
    }
    if t.contains("NeedsLogin") || t.contains("Logged out") || t.contains("not logged in") {
        return "Tailscale 未登录，请先打开 Tailscale 登录".into();
    }
    if t.contains("failed to connect") || t.contains("is Tailscale running") {
        return "Tailscale 没有运行，请先打开 Tailscale".into();
    }
    t.lines()
        .last()
        .unwrap_or("tailscale 返回错误")
        .chars()
        .take(240)
        .collect()
}

/// 本机的 tailnet IPv4 地址
pub async fn tailscale_ipv4(bin: &str) -> Result<std::net::Ipv4Addr, String> {
    let out = ts(bin, &["ip", "-4"], Duration::from_secs(10)).await?;
    out.lines()
        .find_map(|l| l.trim().parse().ok())
        .ok_or_else(|| "没有获取到 Tailscale 地址，请确认 Tailscale 已连接".into())
}

/// 本机在 tailnet 中的域名（MagicDNS）
pub async fn tailscale_dns_name(bin: &str) -> Result<String, String> {
    let out = ts(bin, &["status", "--json"], Duration::from_secs(10)).await?;
    let v: serde_json::Value = serde_json::from_str(&out).map_err(|_| "无法解析 tailscale status 输出".to_string())?;
    if let Some(state) = v["BackendState"].as_str() {
        if state != "Running" {
            return Err(explain_tailscale(state));
        }
    }
    v["Self"]["DNSName"]
        .as_str()
        .map(|s| s.trim_end_matches('.').to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "这台电脑没有 MagicDNS 名称，请在 Tailscale 后台开启 MagicDNS，或改用直连模式".into())
}

pub async fn tailscale_serve_on(bin: &str, https_port: u16, local_port: u16) -> Result<(), String> {
    ts(
        bin,
        &[
            "serve",
            "--bg",
            &format!("--https={https_port}"),
            &format!("http://127.0.0.1:{local_port}"),
        ],
        Duration::from_secs(20),
    )
    .await
    .map(|_| ())
}

pub async fn tailscale_serve_off(bin: &str, https_port: u16) {
    if let Err(e) = ts(
        bin,
        &["serve", &format!("--https={https_port}"), "off"],
        Duration::from_secs(10),
    )
    .await
    {
        tracing::warn!("关闭 tailscale serve 失败：{e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_tailscale_errors() {
        assert!(explain_tailscale("Serve is not enabled on your tailnet.\nTo enable, visit:\n\n         https://login.tailscale.com/f/serve?node=abc\n")
            .contains("https://login.tailscale.com/f/serve?node=abc"));
        assert!(explain_tailscale("NeedsLogin").contains("未登录"));
        assert!(
            explain_tailscale("failed to connect to local tailscaled; it doesn't appear to be running")
                .contains("没有运行")
        );
    }

    #[test]
    fn cleans_cloudflared_lines() {
        assert_eq!(
            clean_log_line("2026-10-12T08:00:00Z ERR Failed to fetch features error=\"x\""),
            "Failed to fetch features error=\"x\""
        );
    }

    /// 用脚本模拟 cloudflared：输出快速隧道地址和连接成功的日志
    #[cfg(unix)]
    #[tokio::test]
    async fn supervises_fake_cloudflared() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("cloudflared");
        let args = dir.path().join("args");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\necho \"$@\" > {}\necho \"$TUNNEL_TOKEN\" >> {}\n\
                 echo '2026-10-12T07:59:59Z ERR failed to request quick Tunnel: Post \"https://api.trycloudflare.com/tunnel\"' >&2\n\
                 echo '2026-10-12T08:00:00Z INF |  https://fancy-words-here.trycloudflare.com  |' >&2\n\
                 echo '2026-10-12T08:00:01Z INF Registered tunnel connection connIndex=0' >&2\nsleep 30\n",
                args.display(),
                args.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let status: SharedStatus = Arc::new(Mutex::new(TunnelStatus::off()));
        let (tx, rx) = watch::channel(false);
        let spec = CloudflaredSpec {
            bin,
            local_port: 7681,
            token: None,
            public_host: String::new(),
            allowed_emails: vec!["me@example.com".into()],
            config_file: dir.path().join("cf.yml"),
        };
        let task = tokio::spawn(run_cloudflared(spec, status.clone(), rx));
        for _ in 0..50 {
            if status.lock().unwrap().state == "online" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let s = status.lock().unwrap().clone();
        assert_eq!(s.state, "online", "{s:?}");
        assert_eq!(s.url.as_deref(), Some("https://fancy-words-here.trycloudflare.com"));
        let a = std::fs::read_to_string(&args).unwrap();
        assert!(a.contains("--url http://127.0.0.1:7681"), "{a}");
        assert!(a.contains("--allowed-mail me@example.com"), "{a}");
        assert!(a.contains("--config"), "{a}");
        tx.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
    }
}

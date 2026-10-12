//! tmuxide 命令行：无界面服务器、脚本和定时任务使用；`serve` 提供浏览器界面。

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tmuxide_server::{
    service::{ConflictMode, RestoreRequest},
    AppPaths, ServeOptions, Service,
};

#[derive(Parser)]
#[command(name = "tmuxide", version, about = "TmuxIDE：保存、恢复并用 Git 同步 tmux 会话")]
struct Cli {
    /// 以 JSON 输出结果
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 启动本地 Web 界面（只监听 127.0.0.1）
    Serve {
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// 不自动打开浏览器
        #[arg(long)]
        no_open: bool,
        /// 远程访问已开启时，在终端里显示手机配对二维码
        #[arg(long)]
        pair: bool,
    },
    /// 列出运行中的会话和已保存的快照
    Ls,
    /// 保存会话快照（不指定名称时保存全部）
    Save { names: Vec<String> },
    /// 恢复会话
    Restore {
        /// 快照名；使用 --all 时省略
        name: Option<String>,
        /// 快照来自哪台主机（默认本机）
        #[arg(long)]
        host: Option<String>,
        /// 恢复后的会话名
        #[arg(long = "as")]
        target: Option<String>,
        /// 同名会话已存在时：rename | replace | skip
        #[arg(long, default_value = "rename")]
        mode: String,
        /// 不重新运行面板中的命令
        #[arg(long)]
        no_commands: bool,
        /// 从历史提交恢复
        #[arg(long)]
        rev: Option<String>,
        /// 恢复本机所有未运行的会话
        #[arg(long)]
        all: bool,
    },
    /// 与远程 Git 仓库同步（先拉取再推送）
    Sync,
    /// 查看某个快照的历史
    History {
        name: String,
        #[arg(long)]
        host: Option<String>,
    },
    /// 显示版本、内置 tmux 与数据目录
    Info,
    /// 检查并安装新版本（校验发布签名后替换当前程序）
    Update {
        /// 只检查，不安装
        #[arg(long)]
        check: bool,
    },
    /// 创建一个空的 bare 仓库作为远程仓库（例如放在 NAS 或同步盘上）
    InitRemote {
        path: std::path::PathBuf,
        #[arg(long, default_value = "main")]
        branch: String,
    },
    /// 直接调用内置 tmux（使用 TmuxIDE 的专用 socket），例如 `tmuxide tmux attach -t dev`
    #[command(disable_help_flag = true)]
    Tmux {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "tmuxide=info,warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("错误：{e:#}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    let paths = AppPaths::default_paths()?;
    if let Cmd::Serve { port, no_open, pair } = cli.cmd {
        return serve(paths, port, no_open, pair);
    }
    if let Cmd::Update { check } = cli.cmd {
        return self_update(check, cli.json);
    }
    if let Cmd::InitRemote { path, branch } = &cli.cmd {
        tmuxide_git::init_bare(path, branch)?;
        println!("已创建远程仓库：{}", path.display());
        return Ok(ExitCode::SUCCESS);
    }
    let svc = Service::new(paths)?;
    let json = cli.json;
    let print = |v: serde_json::Value| -> anyhow::Result<()> {
        println!("{}", serde_json::to_string_pretty(&v)?);
        Ok(())
    };
    match cli.cmd {
        Cmd::Serve { .. } | Cmd::InitRemote { .. } | Cmd::Update { .. } => unreachable!(),
        Cmd::Ls => {
            let live = svc.sessions()?;
            let saved = svc.saved()?;
            if json {
                print(serde_json::json!({ "running": live, "saved": saved }))?;
            } else {
                println!("运行中：");
                if live.is_empty() {
                    println!("  （无）");
                }
                for s in &live {
                    let panes: usize = s.windows.iter().map(|w| w.panes.len()).sum();
                    println!("  ● {:<20} {} 窗口 · {} 面板", s.name, s.windows.len(), panes);
                }
                println!("已保存：");
                if saved.is_empty() {
                    println!("  （无）");
                }
                for s in &saved {
                    let mark = if s.running { "●" } else { "○" };
                    println!(
                        "  {mark} {:<20} {:<14} {} 窗口 · {} 面板 · {}",
                        s.name, s.host, s.windows, s.panes, s.saved_at
                    );
                }
            }
        }
        Cmd::Save { names } => {
            let r = svc.save(if names.is_empty() { None } else { Some(names) }, "snapshot")?;
            if json {
                print(serde_json::to_value(&r)?)?;
            } else {
                println!(
                    "已保存：{}",
                    if r.saved.is_empty() {
                        "（无变化）".into()
                    } else {
                        r.saved.join(", ")
                    }
                );
                if let Some(c) = &r.commit {
                    println!("提交：{}", &c[..7]);
                }
                if r.pushed {
                    println!("已推送到远程");
                }
                if let Some(e) = &r.push_error {
                    eprintln!("推送失败：{e}");
                }
            }
        }
        Cmd::Restore {
            name,
            host,
            target,
            mode,
            no_commands,
            rev,
            all,
        } => {
            if all {
                let r = svc.restore_all()?;
                if json {
                    print(serde_json::to_value(&r)?)?;
                } else {
                    for x in &r {
                        println!("{} {}", if x.skipped { "跳过" } else { "已恢复" }, x.session);
                    }
                }
                return Ok(ExitCode::SUCCESS);
            }
            let name = name.ok_or_else(|| anyhow::anyhow!("请指定快照名，或使用 --all"))?;
            let mode = match mode.as_str() {
                "rename" => ConflictMode::Rename,
                "replace" => ConflictMode::Replace,
                "skip" => ConflictMode::Skip,
                m => anyhow::bail!("未知的 --mode {m}，可选 rename | replace | skip"),
            };
            let r = svc.restore(RestoreRequest {
                host,
                name,
                target,
                mode,
                run_commands: !no_commands,
                rev,
            })?;
            if json {
                print(serde_json::to_value(&r)?)?;
            } else if r.skipped {
                println!("会话 {} 已存在，已跳过", r.session);
            } else {
                println!("已恢复为会话 {}", r.session);
                for w in r.report.iter().flat_map(|r| &r.warnings) {
                    eprintln!("提示：{w}");
                }
                println!("连接：tmuxide tmux attach -t {}", r.session);
            }
        }
        Cmd::Sync => {
            let r = svc.sync()?;
            if json {
                print(serde_json::to_value(&r)?)?;
            } else {
                println!(
                    "{}{}领先 {} · 落后 {}",
                    if r.pulled { "已拉取更新；" } else { "" },
                    if r.pushed { "已推送；" } else { "" },
                    r.status.ahead,
                    r.status.behind
                );
            }
        }
        Cmd::History { name, host } => {
            let host = host.unwrap_or_else(|| svc.host().to_string());
            let h = svc.history(&host, &name)?;
            if json {
                print(serde_json::to_value(&h)?)?;
            } else {
                for c in h {
                    println!("{}  {}", c.short, c.summary);
                }
            }
        }
        Cmd::Info => {
            let i = svc.info();
            if json {
                print(serde_json::to_value(&i)?)?;
            } else {
                println!("TmuxIDE {}  ({}/{})", i.version, i.os, i.arch);
                println!("主机：    {}", i.host);
                println!(
                    "tmux：    {}{}  {}",
                    i.tmux.version,
                    if i.tmux.bundled { "（内置）" } else { "（系统）" },
                    i.tmux.path
                );
                println!("Git：     {}", i.git_engine);
                match tmuxide_server::remote::tunnel::cloudflared_bundled_version() {
                    Some(v) => println!("隧道：    cloudflared {v}（内置）"),
                    None => println!("隧道：    未内置 cloudflared（远程访问会使用系统中安装的版本）"),
                }
                println!("数据目录：{}", i.data_dir);
                println!("快照仓库：{}", i.repo_dir);
            }
        }
        Cmd::Tmux { args } => {
            let status = svc.tmux().passthrough_command(&args).status()?;
            return Ok(ExitCode::from(status.code().unwrap_or(1) as u8));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn serve(paths: AppPaths, port: u16, no_open: bool, pair: bool) -> anyhow::Result<ExitCode> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let h = tmuxide_server::start(ServeOptions {
            port,
            paths,
            installer: std::sync::Arc::new(tmuxide_server::CliInstaller),
        })
        .await?;
        let url = h.url();
        println!("TmuxIDE 已启动：{url}");
        println!(
            "（只监听本机；通过 SSH 使用时可执行 ssh -L {0}:127.0.0.1:{0} <服务器>）",
            h.addr.port()
        );
        if !no_open {
            open_browser(&url);
        }
        if pair {
            show_pairing(&h.remote, &h.service).await;
        }
        tokio::signal::ctrl_c().await?;
        h.remote.stop().await;
        println!("\n已停止。tmux 会话仍在后台运行。");
        anyhow::Ok(())
    })?;
    Ok(ExitCode::SUCCESS)
}

/// 等远程访问连通后在终端打印配对二维码
async fn show_pairing(remote: &tmuxide_server::Remote, svc: &Service) {
    if !svc.config().remote.enabled {
        println!("远程访问未开启：请在界面“设置 → 远程访问”中开启后再使用 --pair");
        return;
    }
    println!("正在等待远程访问连通…");
    for _ in 0..120 {
        let s = remote.status();
        if s.state == "online" && s.url.is_some() {
            break;
        }
        if s.state == "error" && !s.message.is_empty() {
            println!("远程访问：{}", s.message);
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let p = remote.create_pairing();
    match &p.url {
        Some(url) => {
            if let Some(qr) = tmuxide_server::remote::qr_text(url) {
                println!("{qr}");
            }
            println!(
                "用手机扫码，或打开 {} 后输入配对码",
                url.split("/#").next().unwrap_or(url)
            );
        }
        None => println!("远程地址还没有就绪，可稍后在手机上打开远程地址后输入配对码"),
    }
    println!("配对码：{}（{} 秒内有效，只能使用一次）", p.code, p.expires_in);
}

fn open_browser(url: &str) {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    // 打开失败不影响服务（例如无图形界面的服务器）
    let _ = std::process::Command::new(opener)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// `tmuxide update`：下载签名的升级目录，校验后替换当前程序
fn self_update(check_only: bool, json: bool) -> anyhow::Result<ExitCode> {
    use tmuxide_server::Installer;
    let current = env!("CARGO_PKG_VERSION");
    if !tmuxide_update::enabled() {
        anyhow::bail!(
            "此版本没有配置升级签名，请到 {} 手动下载",
            tmuxide_update::release_page(current)
        );
    }
    let catalog = tmuxide_update::fetch_catalog()?;
    let newer = tmuxide_update::newer(&catalog.version, current);
    if json {
        println!(
            "{}",
            serde_json::json!({
                "current": current,
                "latest": catalog.version,
                "update_available": newer,
                "required": tmuxide_update::required(&catalog, current),
            })
        );
    }
    if !newer {
        if !json {
            println!("已是最新版本 {current}");
        }
        return Ok(ExitCode::SUCCESS);
    }
    if !json {
        println!("发现新版本 {}（当前 {current}）", catalog.version);
        if !catalog.notes.is_empty() {
            println!("{}", catalog.notes.trim());
        }
    }
    if check_only {
        return Ok(ExitCode::SUCCESS);
    }
    let installer = tmuxide_server::CliInstaller;
    if let Some(reason) = installer.manual_reason() {
        anyhow::bail!(reason);
    }
    let version = catalog.version.clone();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let last = std::sync::Mutex::new(0u64);
    let progress = std::sync::Arc::new(move |done: u64, total: u64| {
        let pct = (done * 100).checked_div(total).unwrap_or(0).min(100);
        let mut l = last.lock().unwrap();
        if pct >= *l + 10 || done == total {
            *l = pct;
            if !json {
                eprintln!("下载 {pct}%");
            }
        }
    });
    rt.block_on(installer.install(catalog, progress))
        .map_err(anyhow::Error::msg)?;
    if !json {
        println!("已升级到 {version}。正在运行的 tmuxide serve 需要重新启动后才会使用新版本；tmux 会话不受影响。");
    }
    Ok(ExitCode::SUCCESS)
}

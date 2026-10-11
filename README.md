# TmuxIDE

图形化管理 tmux 会话：查看与操作所有会话、窗口和面板，一键保存为快照，关机或换电脑后一键恢复，并把快照同步到你自己的 Git 仓库。

> 署名：M&J Group · 473276@qq.com · MIT License

## 特点

- **IDE 式界面**：左侧会话树（运行中 / 已保存 / 模板），中间是真实的 tmux 终端，右侧检查器显示布局、命令和快照历史，`⌘K` 命令面板。
- **保存与恢复**：保存窗口、分屏布局、工作目录和正在运行的命令；可按历史版本恢复，同名会话可选择恢复为新会话 / 替换 / 跳过。
- **Git 同步**：快照是人类可读的 TOML，每次保存一个提交；支持 SSH 密钥、ssh-agent、HTTPS Token；每台电脑写自己的目录，多机同步不冲突。
- **一次安装，没有其他依赖**：tmux 静态编译后嵌入程序；Git 使用静态链接的 libgit2（含 libssh2、OpenSSL），不需要系统 git；前端资源和字体都在二进制里，运行时不联网。
- **隔离**：TmuxIDE 使用专用 tmux socket（默认 `tmuxide`），不影响你自己的 tmux；仍会加载 `~/.tmux.conf`。

## 下载

在 [Actions](https://github.com/luckySpro/TmuxIDE/actions) 的构建产物或 Releases 中下载：

| 平台 | 桌面版 | 命令行 + Web 界面 |
| --- | --- | --- |
| macOS Apple Silicon | `TmuxIDE_*_aarch64.dmg` | `tmuxide-*-macos-arm64.tar.gz` |
| macOS Intel | `TmuxIDE_*_x64.dmg` | `tmuxide-*-macos-x86_64.tar.gz` |
| Linux x86_64 | `.deb` / `.AppImage` | `tmuxide-*-linux-x86_64.tar.gz`（完全静态） |
| Linux arm64 | — | `tmuxide-*-linux-aarch64.tar.gz`（完全静态） |

Windows 不提供：tmux 无法在 Windows 上原生运行。

> macOS 安装包没有 Apple 开发者签名。第一次打开被拦下时，到“系统设置 → 隐私与安全性”底部点“仍要打开”；或在终端执行一次 `xattr -cr /Applications/TmuxIDE.app`。
>
> Linux 桌面版需要系统自带的 WebKitGTK 4.1（主流桌面发行版默认已安装）。服务器或无桌面环境请使用命令行版。

## 命令行

```sh
tmuxide serve                 # 启动本地 Web 界面（只监听 127.0.0.1，带随机访问令牌）
tmuxide ls                    # 运行中的会话与已保存的快照
tmuxide save [会话…]          # 保存（不指定则保存全部）
tmuxide restore dev [--as dev2] [--mode rename|replace|skip] [--rev <提交>]
tmuxide restore --all         # 开机后恢复本机所有会话
tmuxide sync                  # 先拉取再推送
tmuxide history dev
tmuxide tmux attach -t dev    # 直接使用内置 tmux
tmuxide init-remote /Volumes/NAS/tmux-sessions.git   # 在 NAS / 同步盘上建一个远程仓库
```

在服务器上使用：`tmuxide serve --port 7878 --no-open`，再在本机 `ssh -L 7878:127.0.0.1:7878 服务器`，用输出的地址打开。

## 快照仓库结构

```
hosts/<主机名>/sessions/<会话>.toml     # 每台电脑一个目录
hosts/<主机名>/scrollback/<会话>/*.txt  # 可选：终端输出（默认关闭，保存前过滤 Token/密码/私钥）
templates/<模板>.toml
tmux/tmux.conf                         # 可选
tmuxide.toml
```

## 开发

需要 Rust（stable）、Node 22 与 pnpm 10。

```sh
pnpm --dir web install && pnpm --dir web build   # 前端（嵌入到 Rust 二进制）
cargo test                                        # 单元测试 + 真实 tmux 集成测试（需要系统 tmux）
cargo run -p tmuxide-cli -- serve                 # Web 界面
cd apps/desktop && pnpm install && pnpm tauri dev # 桌面版
```

不设置 `TMUXIDE_TMUX_BIN` 时使用系统 tmux（仅开发用）。发行构建先运行 `vendor/tmux/build.sh` 编译内置 tmux，再设置
`TMUXIDE_TMUX_BIN=vendor/tmux/out/tmux-<os>-<arch>` 编译，见 `.github/workflows/build.yml`。

| 目录 | 内容 |
| --- | --- |
| `crates/tmuxide-core` | 快照模型、恢复计划、敏感信息过滤（无 I/O） |
| `crates/tmuxide-tmux` | 内置 tmux 释放、会话操作、保存与恢复 |
| `crates/tmuxide-git` | libgit2：提交、拉取合并、推送、历史、对比 |
| `crates/tmuxide-server` | 配置、凭据、应用服务、本地 HTTP/WebSocket、内嵌前端 |
| `apps/cli` | `tmuxide` 命令行 |
| `apps/desktop` | Tauri 2 桌面壳 |
| `web` | Vue 3 + xterm.js 界面 |
| `vendor/tmux` | 内置 tmux 的固定版本、校验值与构建脚本 |

规划见 [docs/PLAN.md](docs/PLAN.md)。

## 第三方许可

tmux (ISC) · libevent (BSD-3-Clause) · ncurses (MIT) · utf8proc (MIT) · libgit2 (GPL-2.0 with linking exception) · libssh2 (BSD-3-Clause) · OpenSSL (Apache-2.0) · xterm.js (MIT) · Vue (MIT) · Tauri (MIT/Apache-2.0) · JetBrains Mono (OFL-1.1)

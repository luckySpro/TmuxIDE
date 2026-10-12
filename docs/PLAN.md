# TmuxIDE 规划（v0.1 草案）

> 状态：阶段 0–3 已实现第一版（见文末“实施进度”）。遵循 `lucky-rust-fullstack` 技能（Rust 全栈 / Tauri 2，离线可安装、开箱即用）。
> 署名：M&J Group · 473276@qq.com

## 1. 要解决的问题

tmux 能力强，但有三个痛点：

1. **没有图形化的总览**：有哪些会话、每个会话里有哪些窗口和面板、各自在哪个目录跑什么命令，只能靠命令和快捷键查看。
2. **会话不持久**：机器重启或 tmux server 退出后，布局、工作目录、运行命令全部丢失（tmux-resurrect 能部分解决，但要装插件、配脚本）。
3. **无法跨机器同步和留档**：会话配置、`.tmux.conf`、常用会话模板没有版本管理。

TmuxIDE 的核心链路（输入 → 操作 → 输出）：

> **正在运行的 tmux 会话** → 在 IDE 中查看、编辑、一键保存为快照 → **人类可读的快照文件提交到用户自己的 Git 仓库**，在任意机器上拉取并一键恢复。

验收样例：在机器 A 建一个含 3 个窗口、5 个面板的会话 → 保存并推送 → 在机器 B（未装 tmux、未装 git）安装 TmuxIDE → 配置同一仓库 → 拉取 → 恢复，得到相同布局、目录和启动命令。

## 2. 界面方案

### 2.1 候选形态对比

| 形态 | 优点 | 缺点 | 结论 |
| --- | --- | --- | --- |
| **A. Tauri 2 桌面应用**（Rust + Vue 3 + xterm.js） | 真正的 IDE 体验：侧栏、标签、检查器、设置页、托盘；macOS 用系统自带 WebView，无额外依赖；前端资源打进安装包 | Linux 桌面需要 WebKitGTK；服务器无图形界面时用不了 | **主形态（推荐）** |
| B. 终端内 TUI（ratatui） | 单文件、SSH 下可用 | 本质还是终端界面，"IDE 感"弱，与 tmux 自身界面嵌套易混乱 | 不做独立产品 |
| C. 单二进制内置 Web 服务（axum + 内嵌前端，浏览器访问） | 服务器 / 无桌面环境可用，零系统库依赖 | 需要处理本地端口、访问令牌等安全问题 | **补充形态（后期）**，复用 A 的前端 |

**推荐：A 为主，C 为补充。** 两者共用同一套 Rust 核心 crate 和同一套 Vue 前端，C 只是把前端换成由 axum 托管、IPC 换成 HTTP/WebSocket。另外附带一个 `tmuxide` 命令行，用于脚本、定时保存和无界面服务器。

### 2.2 主界面布局（线框）

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●  TmuxIDE        [＋新建会话] [💾保存快照] [⟲恢复] [⇅同步]   ⌘K 命令面板 │
├───────────────┬──────────────────────────────────────────┬───────────────────┤
│ 会话          │  dev ▸ [1:editor] [2:server] [3:logs] ＋ │ 检查器            │
│ ▾ 运行中      │ ┌──────────────────────┬───────────────┐ │ 会话：dev         │
│   ● dev    3w │ │                      │               │ │ 窗口 3 · 面板 5   │
│   ● ops    1w │ │   xterm.js 终端      │   pane 2      │ │ 目录 ~/code/app   │
│ ▾ 已保存      │ │   (attach 到 tmux)   │               │ │ ───────────────── │
│   ○ blog      │ │                      ├───────────────┤ │ 布局预览 ▣▣ / ▣   │
│   ○ infra     │ │                      │   pane 3      │ │ ───────────────── │
│ ▾ 模板        │ └──────────────────────┴───────────────┘ │ 快照历史 (Git)    │
│   ◇ rust-dev  │                                          │  • 10-10 18:20 ✓  │
│   ◇ web-full  │                                          │  • 10-09 09:02 ✓  │
│               │                                          │  [对比] [恢复此版]│
├───────────────┴──────────────────────────────────────────┴───────────────────┤
│ tmux 3.5a(内置) · 运行中 2 · Git: main ↑1 ↓0 · 自动保存 5 分钟 · 上次推送 18:20 │
└──────────────────────────────────────────────────────────────────────────────┘
```

区域说明：

- **左侧栏 · 会话树**：三组。运行中（来自 tmux server 的实时状态）、已保存（来自快照仓库，可一键恢复）、模板（可复用的会话定义，如"Rust 开发：编辑器 + cargo watch + 日志"）。右键菜单：重命名、复制为模板、保存、关闭、删除。
- **中间 · 终端区**：顶部标签对应 tmux 窗口，点击即 `select-window`；下方是 xterm.js 终端，通过 PTY 执行 `tmux attach`，分屏由 tmux 自己绘制，保证 100% 原生行为（第一阶段）。后期可切换到 tmux control mode（`-CC`）由 IDE 原生绘制每个面板。
- **右侧 · 检查器**：当前会话 / 窗口 / 面板的详细信息（目录、命令、PID、尺寸）、布局缩略图、该会话的快照历史（来自 Git log），支持对比两个版本、恢复任意版本。可折叠。
- **底部状态栏**：内置 tmux 版本、Git 分支与领先/落后提交数、自动保存状态、最近同步时间、错误提示。
- **命令面板（⌘K）**：所有操作可搜索执行，键盘优先。

其他页面：

- **设置 › Git 仓库**：远程地址、分支、认证方式（SSH 密钥 / HTTPS Token，存入系统钥匙串）、提交作者、自动推送开关、"测试连接"按钮。
- **设置 › 保存策略**：自动保存间隔、是否保存滚动缓冲区（默认关闭）、敏感信息过滤规则、排除的会话。
- **设置 › tmux**：使用内置 tmux / 系统 tmux、`.tmux.conf` 编辑器（带语法高亮，纳入 Git 版本管理）。
- **设置 › 通用**：语言（简中 / 繁中 / 英 / 日）、主题、快捷键、关闭窗口后是否驻留托盘继续自动保存。
- **首次启动向导**：检测内置 tmux → 选择是否导入现有会话 → 配置 Git（可跳过，先本地保存）。

## 3. 技术架构

```
TmuxIDE/
├─ Cargo.toml                 # workspace
├─ crates/
│  ├─ tmuxide-core/           # 领域模型与规则：Session/Window/Pane/Snapshot/Template，恢复计划生成，不依赖 GUI
│  ├─ tmuxide-tmux/           # tmux 适配：定位内置 tmux、专用 socket、参数数组调用、解析 list-* -F 输出、capture-pane
│  ├─ tmuxide-store/          # 快照文件读写（TOML）、原子写入、schema 版本与迁移
│  ├─ tmuxide-git/            # Git 适配：git2（libgit2 静态内嵌）init/clone/commit/push/pull/log/diff
│  └─ tmuxide-pty/            # portable-pty：attach 会话，桥接到前端
├─ apps/
│  ├─ desktop/                # Tauri 2 + Vue 3 + TypeScript + xterm.js
│  └─ cli/                    # tmuxide 命令行：list/save/restore/sync/serve
├─ vendor/tmux/               # 内置 tmux 的构建脚本、版本、哈希、许可证
└─ docs/
```

关键设计：

- **专用 tmux socket**：默认使用 `tmux -L tmuxide`，与用户已有的 tmux server 隔离，互不干扰；设置里可切换到"管理系统默认 socket"，并能导入已有会话。
- **快照格式（人类可读，适合 Git diff）**：

  ```
  <仓库>/
  ├─ hosts/<主机名>/sessions/dev.toml     # 每台机器一个目录，避免多机同时推送冲突
  ├─ hosts/<主机名>/scrollback/…          # 可选，默认不保存
  ├─ templates/rust-dev.toml               # 跨机器共享的模板
  ├─ tmux/tmux.conf                        # 配置文件版本管理
  └─ tmuxide.toml                          # 仓库格式版本
  ```

  `dev.toml` 记录会话名、窗口顺序与名称、布局字符串（`window_layout`）、每个面板的工作目录、启动命令、环境变量白名单。路径中的家目录写成 `~`，便于跨机器恢复。
- **恢复**：由 core 生成"恢复计划"（建会话 → 建窗口 → 分屏 → `select-layout` → `cd` → 发送启动命令），tmux 适配层逐步执行，每步失败可报告并跳过；同名会话已存在时提示"跳过 / 重命名 / 覆盖"。
- **Git 同步**：每次保存产生一个提交（信息形如 `snapshot(dev): 3 windows, 5 panes @ host`）；推送前先拉取，按主机分目录后正常不会冲突，真正冲突时保留双方并在界面上让用户选择。
- **安全**：所有外部命令用参数数组，不拼 shell 字符串；凭据只进系统钥匙串；滚动缓冲区默认不保存，开启时先经过脱敏规则（Token、密码、私钥等模式）；首次推送到公开仓库时明确警告。
- **跨平台范围**：macOS（ARM64 / Intel）为首发平台；Linux 第二阶段。**不做 Windows 版**（tmux 不能原生运行于 Windows，按技能约定不生成不可用的空壳；以后可考虑 WSL 方案）。

## 4. "一次安装解决所有依赖"如何做到

原则：终端用户安装 TmuxIDE 后，**不需要再安装 tmux、git、Node、Python、Homebrew 或任何其他东西**。构建阶段可以联网取依赖，运行阶段一律不需要。

| 运行时需求 | 做法 | 说明 |
| --- | --- | --- |
| **tmux 本体** | **随包内置静态编译的 tmux**（固定版本，例如 3.5a，连同 libevent、ncurses、utf8proc 静态链接），放入应用的资源目录 | CI 在各目标平台从源码构建并校验哈希；tmux(ISC)、libevent(BSD)、ncurses(MIT 类)、utf8proc(MIT) 均允许再分发，许可证随包附带。终端描述 terminfo 一并内置并通过 `TERMINFO` 指向。用户已装系统 tmux 也不受影响 |
| **Git** | **git2 crate 开启 `vendored-libgit2`**，libgit2 静态链接进二进制；SSH 用内置 libssh2，HTTPS 在 macOS 用系统 Security 框架、Linux 用内置 OpenSSL（`vendored-openssl`） | 不调用系统 `git` 命令。选 git2 而不是 gitoxide，是因为 gitoxide 的 push 支持尚不完整（实施时复查） |
| **凭据存储** | `keyring` crate：macOS 用系统钥匙串；Linux 有 Secret Service 时使用，否则退回到权限 0600 的本地加密文件 | 也支持直接使用 ssh-agent |
| **前端界面** | Vite 构建产物（Vue、xterm.js、图标、字体如 JetBrains Mono / 中文回退字体）全部打进安装包，CSP 禁止远程资源 | 运行时完全离线 |
| **WebView** | macOS 使用系统自带 WKWebView | 无需安装。Linux 桌面需 WebKitGTK，因此 Linux 首选提供"命令行 + 内置 Web 界面"的静态 musl 单文件（形态 C），不依赖任何系统库 |
| **命令行工具** | `tmuxide` CLI 打包在 .app 内，设置里一键创建到 `~/.local/bin` 的链接 | 不需要单独安装 |
| **数据存储** | 快照用 TOML 文件，不引入数据库；如后续需要索引，使用 `rusqlite` 的 `bundled` 特性静态内嵌 SQLite | 不依赖系统数据库 |

**如何证明做到了**：CI 增加"干净环境验收"任务——在未安装 tmux、git、Homebrew 的 macOS runner（以及不带这些包的最小 Linux 容器）上安装产物，自动执行"新建会话 → 保存 → 推送到本地裸仓库 → 杀掉 tmux server → 拉取 → 恢复"全流程，并检查二进制没有链接到 `/opt/homebrew`、`/usr/local` 下的动态库（`otool -L` / `ldd`）。

## 5. 分阶段开发计划

每个阶段都以"可检查结果"作为完成标准，前一阶段验收后再进入下一阶段。

### 阶段 0 · 骨架与内置 tmux
- Cargo workspace、Tauri 2 + Vue 3 工程、CI（fmt / clippy / test）。
- 内置 tmux 的构建脚本（macOS ARM64 / Intel），版本与哈希固定，资源路径解析。
- 应用数据目录、日志、关于页（版本、内置 tmux 版本、许可证）。
- **验收**：在没有 Homebrew tmux 的 Mac 上启动应用，状态栏显示"tmux 3.5a（内置）"，可启动专用 tmux server。

### 阶段 1 · 会话管理（最小可用 IDE）
- 实时会话树：列出 / 新建 / 重命名 / 关闭会话、窗口；状态轮询或 hook 通知刷新。
- 终端区：PTY attach + xterm.js，窗口标签切换，尺寸自适应，复制粘贴。
- 检查器：面板目录、命令、尺寸；空状态、错误状态。
- **验收**：完全在 IDE 中完成日常 tmux 操作；关闭 IDE 窗口后会话仍在运行，重新打开可再次连接。

### 阶段 2 · 保存与恢复
- 快照模型与 TOML 格式（含 schema 版本），手动保存、定时自动保存、退出前保存。
- 恢复计划与执行，同名冲突处理，逐步失败报告。
- 模板：从会话生成模板、从模板新建会话。
- 可选滚动缓冲区保存 + 脱敏。
- **验收**：杀掉 tmux server 或重启电脑后，一键恢复所有会话的布局、目录与启动命令。

### 阶段 3 · Git 同步
- 设置页：仓库地址、分支、认证（SSH 密钥 / HTTPS Token / ssh-agent）、作者、测试连接。
- 本地仓库初始化或克隆；保存即提交；手动 / 自动推送；拉取；按主机分目录。
- 检查器中的快照历史、两个版本对比、从任意历史版本恢复。
- `.tmux.conf` 纳入版本管理。
- 首次推送公开仓库的风险提示。
- **验收**：第 1 节的跨机器验收样例在两台 Mac 之间跑通，且机器 B 未安装 git。

### 阶段 4 · 体验完善
- 命令面板、全键盘操作、快捷键自定义。
- 多语言（简中 / 繁中 / 英 / 日）、明暗主题、窄窗口适配。
- 托盘驻留：关闭主窗口后继续自动保存与同步。
- 首次启动向导、导入系统 tmux 现有会话。

### 阶段 5 · 打包、升级与发布（macOS）
- DMG 安装包、签名与公证、Tauri updater 签名升级。
- 干净环境验收（见第 4 节）进入 CI 门禁。
- GitHub Release：附件、SHA256、升级清单。

### 阶段 6 · Linux 与无界面模式
- 静态 musl 版 `tmuxide`：CLI + `tmuxide serve`（axum 托管同一套前端，仅监听 127.0.0.1，带一次性访问令牌），内置静态 tmux。
- 可选 AppImage 桌面版（标注 WebKitGTK 系统要求）。

### 阶段 7 · 进阶（按需）
- tmux control mode 原生面板渲染（拖拽调整分屏、面板级搜索）。
- 会话内容全文搜索、快照自动清理策略、多仓库支持。

## 6. 已采用的默认选择（可调整）

- 主界面：Tauri 2 桌面应用；前端 Vue 3 + TypeScript。
- 首发平台：macOS（Apple Silicon 优先，同时出 Intel 包），不做 Windows。
- 默认使用专用 tmux socket，不触碰用户现有 tmux。
- 滚动缓冲区默认不保存，避免把敏感输出推到 Git。
- 快照按主机分目录，模板和 tmux 配置跨机器共享。

## 7. 实施进度（2026-10-10）

| 阶段 | 状态 | 说明 |
| --- | --- | --- |
| 0 骨架与内置 tmux | 已完成 | workspace、`vendor/tmux/build.sh`（Linux musl 全静态 / macOS 仅链接系统库）、CI |
| 1 会话管理 | 已完成 | 会话树、xterm.js 终端（PTY attach）、窗口标签、重命名/关闭、检查器 |
| 2 保存与恢复 | 已完成 | TOML 快照、内容不变不提交、自动保存、历史版本恢复、同名冲突处理、模板、可选滚动缓冲区 + 脱敏 |
| 3 Git 同步 | 已完成 | libgit2 静态内嵌；SSH 密钥 / ssh-agent / HTTPS Token；按主机分目录；历史与逐行对比 |
| 4 体验完善 | 部分 | 命令面板、简体中文/英文已完成；繁中/日文、托盘、快捷键自定义、tmux.conf 编辑器待做 |
| 5 macOS 打包 | 部分 | 未签名 DMG（arm64 / Intel）；签名公证与自动升级待做 |
| 6 Linux 与无界面模式 | 已完成 | 静态 `tmuxide`（x86_64 / aarch64）含 Web 界面；x86_64 deb / AppImage |
| 7 进阶 | 未开始 | control mode 原生面板渲染等 |

与原计划的差异：桌面窗口和 `tmuxide serve` 共用同一个本地 HTTP/WebSocket 服务（127.0.0.1 + 随机令牌），而不是 Tauri IPC，
这样两种形态只有一套接口。内置 tmux 直接嵌入二进制，首次运行释放到数据目录。

## 8. 远程访问（v0.2 规划，2026-10-12）

目标：离开电脑时，用手机或平板查看这台电脑上正在运行的所有终端，并继续输入命令。

### 8.1 架构

```
手机浏览器 ──HTTPS──> Cloudflare 隧道 / Tailscale ──> 127.0.0.1:7681（远程端口）──> tmux
电脑界面   ──────────────────────────────────────────> 127.0.0.1:随机端口（本机端口）
```

- **两个端口，权限分开。** 本机端口和现在一样，只给电脑上的界面用；远程端口只提供手机页面和终端相关接口（终端列表、打开终端、输入），改设置、Git、删除快照等接口在远程端口上根本不存在。
- **远程端口只监听本机**，外部流量只能经过隧道进来；只有 Tailscale 直连模式监听 tailnet 地址（100.x，WireGuard 加密）。
- **终端列表实时推送**：`/m/api/events` WebSocket 推送会话 → 窗口 → 面板（当前命令、目录）和每个面板最后几行预览，tmux 有变化时推送。
- **打开终端**：为手机创建一个分组会话（`new-session -t`），手机切窗口不会影响电脑；默认以 `ignore-size` 连接，不改变电脑端的窗口尺寸；只读设备用 tmux 自带的 `read-only` 客户端。

### 8.2 网络方式（在 IDE 中配置）

| 方式 | 需要什么 | 地址 |
| --- | --- | --- |
| Cloudflare 快速隧道 | 什么都不需要（cloudflared 内置） | `*.trycloudflare.com`，每次启动会变 |
| Cloudflare 自有域名 | Cloudflare 账号 + Tunnel Token | 固定，如 `tmux.example.com` |
| Tailscale（serve） | 电脑和手机都装 Tailscale 并登录 | `https://<机器名>.<tailnet>.ts.net` |
| Tailscale（直连） | 同上，tailnet 未开 HTTPS 证书时用 | `http://100.x.x.x:7681` |

- cloudflared（Apache-2.0）按平台锁定版本和 SHA256，像 tmux 一样编进程序，用户不需要另外安装。
- Tailscale 不能内置（它是系统 VPN），IDE 自动检测已安装的 Tailscale 并引导；手机端本来也需要装 Tailscale。
- 快速隧道的地址重启后会变，已配对的手机需要重新扫码，因此长期使用推荐自有域名或 Tailscale。

### 8.3 认证全流程

1. **配对**：电脑上点“配对新设备”生成一次性配对码（二维码 + 8 位码，5 分钟有效，只能用一次，输错 5 次作废）。
2. **设备凭证**：配对成功后手机获得 256 位随机凭证，放在 `HttpOnly; Secure; SameSite=Strict` Cookie 中；电脑只保存它的 SHA-256。每台设备可单独移除、设为只读，30 天未使用自动失效。
3. **解锁 PIN（可选，默认开）**：设备凭证之外，每次打开还要输入 PIN（Argon2 存储），空闲 30 分钟自动锁定，连续输错 5 次锁定该设备，需要在电脑上解除。
4. **每个请求**：校验设备凭证与解锁状态；WebSocket 和写操作还校验 Origin；只读设备的输入在服务端丢弃。
5. **可见与可撤销**：电脑端显示在线设备和它正在看的终端，可一键断开；所有配对、解锁、打开终端、失败尝试写入访问记录；远程连接时电脑端弹出提醒。

### 8.4 开发步骤

1. 配置与存储：`[remote]` 配置段、设备与访问记录文件（0600）、Token/PIN 存储。
2. 远程端口：认证中间件、配对/解锁接口、终端列表推送、终端连接（分组会话、只读）。
3. 隧道管理：内置 cloudflared（快速隧道、Token）、Tailscale serve / 直连，状态与自动重连。
4. 手机页面：配对、PIN、终端列表、终端（xterm.js + 快捷键栏 + 命令输入框）。
5. 电脑设置页“远程访问”：网络方式、状态、二维码配对、设备管理、访问记录、连接提醒。
6. CLI：`tmuxide serve --pair` 在终端打印配对二维码，方便无界面服务器。
7. 测试与发布：认证单元测试、端到端（配对 → 解锁 → 列表 → 输入 → 只读 → 撤销）、CI 内置 cloudflared、v0.2.0。

### 8.x 实施状态（2026-10-12，v0.2.0）

- 已实现上述全部模式与认证流程；cloudflared 固定为 2026.10.0，由 `vendor/cloudflared/fetch.sh` 下载并按 `versions.env` 中的 SHA-256 校验后嵌入。
- 本地验证：Playwright 端到端（电脑开启 → 设 PIN → 配对 → 手机输错/输对 PIN → 列表 → 打开终端发命令 → 切换窗口与适配尺寸 → 设为只读 → 移除），以及 `scripts/remote-smoke.sh` 的本机端口模式。
- CI：Linux x86_64 与 macOS arm64 用 `scripts/remote-smoke.sh` 经真实 Cloudflare 快速隧道跑通配对、认证与终端连接；Linux arm64 跑本机端口模式。
- 远程管理（设备级选项，默认关闭）：授权设备在远程地址 `/admin` 使用完整界面，接口经 `/api/admin` 转发到本机接口的子集（不含远程访问的写接口）；需要 PIN；每个写操作记入访问记录。已用 Playwright 验证。
- 未验证：Tailscale 两种模式（需要已登录 tailnet 的机器）、Cloudflare 自有域名模式（需要 Tunnel Token）、真机 iOS/Android 浏览器。

## 9. 自动升级、会话选项卡与分组（v0.3.0，2026-10-12）

### 9.1 升级
- 升级单位：桌面版为整个应用（macOS `.app.tar.gz`、Linux AppImage / deb，由 Tauri 升级插件安装）；命令行版为单个 `tmuxide` 可执行文件。tmux 会话在独立的 tmux 服务里，不受升级影响。
- 签名：CI 用 `TAURI_SIGNING_PRIVATE_KEY`（GitHub Secrets）签名桌面版安装包和 `update.json`；程序只内置公钥（`crates/tmuxide-update/updater.pub`）。私钥不进仓库。
- 目录：`update.json`（版本、最低支持版本、说明、命令行版各平台地址与 SHA-256、桌面版各平台安装包签名）+ `update.json.sig`；`latest.json` / `latest-mirror.json` 供 Tauri 插件读取。Tauri 的 latest.json 本身没有签名，桌面版安装前核对其版本和安装包签名与已签名的 update.json 一致。
- 下载：GH-Proxy 加速优先，失败回到 github.com；地址按正式标签构造。
- 防降级：缓存保存目录原文与签名，读取时重新校验；比已知目录旧的目录不采用。
- 必需升级：`min_version` 来自已签名目录，界面弹出无法关闭的对话框。
- 发布：`workflow_dispatch` 输入 `release_tag` 与可选的 `min_version`；`updater.pub` 非空而 Secrets 缺失时发布失败，公钥为空时跳过升级目录并给出警告。

### 9.2 会话选项卡与分组
- 分组保存在 tmux 会话选项 `@tmuxide-group`，随会话改名保留，不另建存储。
- 工作区顶部会话选项卡（KeepAlive 保留最多 8 个终端连接）和分组筛选；会话树按分组折叠。
- 手机页面按分组排列，终端页顶部可切换会话。

### 9.3 实施状态
- 已完成并在本机验证：命令行版完整自升级与篡改拒绝（测试密钥）、桌面版 deb / AppImage 升级包生成与签名、界面必需升级弹窗（Esc 与点击背景无法关闭）、选项卡与分组、远程页面回归。
- 未验证：桌面版应用内安装与重启（需要真实 macOS / Linux 桌面）。首次真实自动升级将发生在 0.3.0 之后的版本。

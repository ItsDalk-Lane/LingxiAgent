# R01-T06 Tauri v2 桌面壳 Spike 报告（TAURI_SPIKE_REPORT）

- 任务：R01-T06（系统能力与桌面安全原型）；验收场景 R01-A11（未授权 webview 负向）/ R01-A12（测试插件双产物探测）
- 基线 HEAD：`76bd42c439e180163b150a0058bd6c9e4875c940`；分支 `codex/rust-tauri-migration`
- 平台：macOS 27.0 arm64（Mac15,14）实测；Windows / Linux / macOS x64 全部 UNVERIFIED（owner=R09/R10 平台验收负责人，verify_by_stage=R09 最迟 R10）
- 日期：2026-09-25
- 证据根：`artifacts/rust-tauri/R01/T06/`（runner-summary.txt 逐步 expected/actual/exit）；重放：`spike/tauri-shell/scripts/run_e2e.sh <证据目录>`
- Spike 工程：`spike/tauri-shell/`（独立 manifest：两个 spike Cargo.toml 均无显式 `[workspace]` 段，仓库根及 spike 祖先目录均无 Cargo.toml，无祖先 workspace 可归入，隔离实效成立；**不是 rust/ workspace 成员**，DEP-07 不受影响；不接入任何生产入口）
- 机器可读能力矩阵：`docs/rust-tauri/R01/SHELL_CAPABILITY_MATRIX.json`（本报告的事实源之一，两文件一致）

## 1. 技术栈与锁定

Tauri 2.11.6（wry 0.55.1 / tao 0.35.3，macOS 后端 WKWebView WebKit 605.1.15）。插件（全部 Cargo.lock 锁定）：tauri-plugin-shell 2.3.6 / notification 2.4.0 / dialog 2.7.3 / clipboard-manager 2.3.3 / global-shortcut 2.3.2 / autostart 2.5.1 / updater 2.12.0 / process 2.3.1；测试专用 tauri-plugin-wdio-webdriver 1.4.0（feature `e2e-test` 可选依赖）。锁树 603 crates。版本选择与理由见 DEPENDENCY_DECISIONS.md **D-11**。工具链沿用仓库根 rust-toolchain.toml（rustc 1.98.1，rustup 代理）。

## 2. 系统能力逐项实测（macOS arm64）

详表见 SHELL_CAPABILITY_MATRIX.json `capabilities[]`（含现役 Electron 实现行号定位）。摘要：

| 能力 | 结果 | 关键证据（run-test/host-report.ndjson 等） |
|---|---|---|
| 窗口 | VERIFIED | window.ops 全 Ok（maximize/always_on_top/inner_size/monitor 2560x1440） |
| 托盘 | VERIFIED | tray.created ok（图标+菜单+回调注册；点击未人工实测） |
| 全局快捷键 | VERIFIED | shortcut.register registered=true + 合成 CGEvent 真实触发 shortcut.fired |
| 通知 | VERIFIED | request_permission=Granted，单次真实 show（最小调用，无轰炸） |
| 剪贴板 | VERIFIED | roundtrip_ok=true（中文 marker），写后恢复用户原剪贴板 |
| 文件对话框 | VERIFIED（带重要限制） | NSOpenPanel 真实弹出（CGWindowList 取证），合成 Cmd+period（cancel: 等价物，postToPid）取消后 callback picked=null 兑现；**blocking_pick_file 永不返回 + Escape 关窗不触发回调（实测缺陷，见 §7-R1）** |
| 录音（麦克风） | VERIFIED 拒绝路径 / UNVERIFIED 授权路径 | TCC not_determined 下 WKWebView 真实拒绝 getUserMedia；enumerateDevices 不泄露设备标签；不伪造授权 |
| 语音（听写） | VERIFIED 查询路径 | SFSpeechRecognizer.authorizationStatus=not_determined（只读，不弹窗）；识别链路需自定义 Swift 桥（R09） |
| 屏幕 | VERIFIED 权限查询 / UNVERIFIED 采集 | CGPreflightScreenCaptureAccess=granted；getDisplayMedia 需用户手势（WKWebView 约束） |
| 辅助功能 | VERIFIED | AXIsProcessTrusted=trusted（运行终端）；合成事件注入真实生效 |
| 代理 | UNVERIFIED | 全程 loopback；WebView 级代理控制归 R09 |
| 更新器 | VERIFIED | 正向验签下载通过（未 install）；负向字节篡改="The signature verification failed"；打包 .app 内复测通过 |
| 登录自启动 | VERIFIED | enable→is_enabled=true→disable→false 真实往返 |
| 应用重启 | VERIFIED | tauri::process::restart 第二实例观测（run-relaunch） |

TCC 快照（全部只读查询，零弹窗零伪造）：accessibility=trusted、screen_recording=granted、microphone/camera/speech=not_determined。

## 3. capabilities 授权面与 A11 负向（VERIFIED）

允许的自定义命令（`tauri_build::AppManifest::commands` 注册 7 个，自动生成的权限为**无前缀** `allow-<command>`）：`shell_public_info`、`shell_sensitive_probe`（执行落 marker 文件）、`shell_secondary_probe`、`sidecar_status`、`sidecar_ping`、`sidecar_restart`、`finish_e2e`。

capabilities 文件：
- `capabilities/main.json` → windows=["main"]：core:default + 6 个自定义命令 allow + notification/dialog/clipboard-manager/global-shortcut/autostart/updater/process 的 default 与所需 allow 项。**任何 capability 中均无 shell 插件权限**——tauri-plugin-shell 只在 Rust 宿主侧使用，前端无任意 shell 执行面。
- `capabilities/main-secondary.json` → windows=["main"]：仅 `allow-shell-secondary-probe`。main 窗同时命中两个 capability，权限**并集合并**（secondary probe 仅经第二文件授权，main 调用 OK = 合并语义实测成立）。

窗口/webview 作用域与负向用例（run-test + run-release 双产物一致结论）：

| 用例 | 预期 | 实测 | 证据 |
|---|---|---|---|
| main（tauri://localhost，已授权）调 6 自定义命令+notification/clipboard 插件命令 | 全通 | 全通（10 OK） | page-reports label=main |
| main 调 `plugin:shell\|execute` 与未知命令 `shell_exec_arbitrary` | 拒 | "not allowed by ACL" ×2 | 同上 |
| untrusted（打包页 untrusted.html，不在任何 capability windows 列表）调全部 11 项 | 全拒 | 全拒（11 DENIED） | page-reports label=untrusted |
| remote（WebviewUrl::External loopback http 源 = 远程 URL 等价）调全部 11 项 | 全拒（远程 URL 默认零 native 权限） | 全拒（11 DENIED） | page-reports label=remote |
| WebDriver 驱动端在 untrusted/remote 上下文主动 invoke `shell_sensitive_probe`（非页面自报） | 拒 | 两窗均 "not allowed by ACL"；main 同调用 OK——页面自报与驱动端互证 | run-test/webdriver-evidence.json driverSensitiveProbe |
| release 产物同矩阵 | 同判 | untrusted/remote 各 11 拒、main 10 通 | run-release/page-reports.jsonl |

结论：未授权 webview（无论打包页还是远程源）无法调用任何敏感命令，授权与命令绑定到窗口 label，而不是隐藏按钮层面——A11 成立。

## 4. sidecar 生命周期（VERIFIED）

sidecar 原型 `spike/tauri-shell/sidecar/`（零依赖 Rust 二进制 lingxi-t06-sidecar 1.0.0）：stdout hello 行（JSON：marker/name/version/protocol），stdin 行命令 ping/version/shutdown，未知命令 error，非 `--report-version` 参数 exit 64，stdin EOF 自行 exit 0。

宿主侧（src/sidecar.rs）：tauri-plugin-shell sidecar API spawn → 5s 握手超时 → hello 纯函数校验（版本不符/garbage 拒绝并 kill）→ 异步事件泵（Stdout 按 seq 配对的 pending 请求；Terminated 置 dead 并 drain pending）→ send_command 3s 超时 → restart 预算 MAX_RESTARTS=1 → shutdown_blocking（发 shutdown、等 bye、失败则 kill）。

| 场景 | 命令/操作 | 预期 | 实测 | 证据 |
|---|---|---|---|---|
| 随宿主启动+握手 | spawn_and_handshake | hello 通过 | sidecar.handshake_ok（v1.0.0/p1） | run-test/host-report |
| 版本不匹配拒绝 | `sidecar --report-version 9.9.9` | 拒绝+kill | 拒绝（集成测试断言） | sidecar-it/cargo-test.log 8/8 |
| garbage hello 拒绝 | 测试替身输出垃圾 | 拒绝 | 拒绝 | 同上 |
| 崩溃恢复 | `kill -9 <sidecar pid>` → WebDriver 驱动 sidecar_restart+ping | dead 检测+重启+ping 通 | terminated signal=9 → alive=false → restart ok → pong seq=2 → restarts=1 | run-test/sidecar-recovery.json |
| 有序退出 | finish_e2e → shutdown | bye + code 0 | bye=true orderly=true，terminated code=0 | run-test/host-report 尾部 |
| 宿主 SIGKILL 无孤儿 | `kill -9` 宿主（Run B） | sidecar stdin EOF 自退 | no sidecar process after host SIGKILL | run-release/sidecar-after-sigkill.txt |
| 受限参数 | `sidecar --evil` | exit 64 | exit 64 | sidecar-it |
| stdin EOF 自退出 | 关闭 stdin | exit 0 | exit 0 | sidecar-it |
| 打包随包 | .app 内运行 | 握手成功 | run-bundle sidecar.handshake_ok | run-bundle/host-report |

实测坑（已修入 runner）：sidecar 运行时解析 `current_exe()` 同级目录的**无 triple 后缀**文件名（后缀是打包期约定）；macOS 上 /tmp 是 /private/tmp 的符号链接，Tauri 拒绝符号链接路径下的 current_exe（"StartingBinary found current_exe() that contains a symlink"），打包冒烟须复制到 realpath 目录运行——复测通过。

## 5. W03 测试路径与 A12 双产物探测（VERIFIED）

- **tauri-driver（官方 WebDriver 路径）仅支持 Windows/Linux**：macOS 无 WKWebView 的 WebDriver 驱动，本平台不可用——如实记录，不伪造。
- **macOS 可用路径 = tauri-plugin-wdio-webdriver 1.4.0**：嵌入式 W3C WebDriver 服务（默认 4445，本 spike 用 `TAURI_WEBDRIVER_PORT=19275`）。实测 session 创建（browserName=webkit 605.1.15）、window handles=3（main/untrusted/remote）、execute sync/async（读 `window.__T06_RESULT`、驱动端敏感命令负向 invoke）全通；probe_webdriver.mjs 提供 session-test/sidecar-recovery/finish/closed-check 四个可重放子命令。
- **A12 双产物**：
  - test 构建（`cargo build --release --features e2e-test`）：webdriver 端口监听，会话可用（runA-webdriver-session exit=0）。
  - release 构建（无 feature，插件为可选依赖根本不编译进锁树产物）：端口 ECONNREFUSED（closed-check exit=0）；二进制 strings 探测 wdio 计数 40 vs **0**、webdriver 计数 41 vs **0**（run-release/binary-strings-probe.txt）。
  - 结论：测试入口只存在于测试构建；发布构建无监听、无可达入口、无字符串痕迹。A12 成立。
- @wdio/tauri-service 为可选封装，本 spike 直接用裸 WebDriver HTTP 协议（probe_webdriver.mjs），减少一层依赖。

## 6. updater 验签（VERIFIED，loopback 夹具）

- 夹具：合成 payload tar.gz（非真实 app），`tauri signer generate` 测试密钥对（**私钥在 /tmp/lingxi-t06-keys/，不入库不打印**；pubkey 入 tauri.conf.json）；`createUpdaterArtifacts: true` 打包时真实产出 .app.tar.gz + .sig（bundle/bundle-artifacts.txt）。
- 正向：latest.json（有效 minisign 签名）→ update_found 0.2.0 → download 406B 验签通过 → **未调用 install**（红线：不触发真实系统更新）。
- 负向：latest-tampered.json = 同一合法签名但 url 指向**字节被篡改**的包 → `download: The signature verification failed`（验签强制生效，不是编码错误）。
- 打包 .app 冒烟中 updater 探针复测通过（run-bundle/host-report.ndjson）。
- 已知 spike 限定：release 产物要求 https endpoint，本任务流量限 loopback，故配置 `dangerousInsecureTransportProtocol: true`——**仅限 spike，生产必须 https，禁止带入**；即使明文传输，minisign 验签仍强制执行（负向实测证明）。

## 7. 实测风险与坑（R09 直接输入）

| # | 发现 | 影响与对策 |
|---|---|---|
| R1 | **tauri-plugin-dialog 回调兑现依赖 cancel: 语义**：(a) blocking_pick_file 面板被外部取消后永不返回（线程挂到进程退出）；(b) 即使 callback API，合成 Escape（cghidEventTap 或 postToPid 两种投递都实测）让 NSOpenPanel 走 performClose:——面板消失但回调不触发；只有 Cmd+period（cancel: 键盘等价物）postToPid 能确定性兑现 picked=null | 生产路径一律用 callback/异步 API；自动化测试取消对话框必须用 cancel: 等价物而非 Escape；阻塞变体禁止用于可被外部干预的对话框 |
| R2 | /tmp→/private/tmp 符号链接触发 sidecar/updater 的 current_exe() 拒绝 | 开发与 CI 运行目录用 realpath；打包产物从 /Applications 等真实路径运行无此问题 |
| R3 | app 命令自动生成权限为无前缀 `allow-<command>`（不是 `<identifier>:allow-...`），标识符含点会被拒 | capabilities 写法以 tauri-build 源码实测为准（本 spike 已验证） |
| R4 | tauri-driver 无 macOS 支持 | 桌面 E2E 走 wdio-webdriver 插件 + feature 门控，双产物探测作为 A12 常设门禁 |
| R5 | WKWebView 媒体约束：getDisplayMedia 必须用户手势；未授权 TCC 下 getUserMedia 直接拒绝且不弹窗（与 Chromium 提示后拒绝不同） | R09 录音/截屏交互设计按 WKWebView 语义：先 TCC 预检（宿主侧），再引导用户手势 |
| R6 | release 构建 updater 强制 https endpoint | 生产配置必须 https；spike 的 dangerousInsecureTransportProtocol 不得进入生产配置 |
| R7 | 未打包二进制的 macOS 通知/自启动归属终端宿主运行上下文（通知经 Notification Center 真实送达，权限 Granted） | 通知与 SMAppService 的最终形态验收归 R09 打包链路 |
| R8 | CSP 需显式放行 loopback connect-src 才能让打包页向证据服务器 POST | 生产 CSP 不含任何 http 源；本项仅 spike 证据管道 |

## 8. 平台范围与遗留（全部如实 UNVERIFIED）

- Windows x64 / Linux x64 / macOS x64：本报告全部能力项 UNVERIFIED；owner=R09/R10 平台验收负责人（总控指派），verify_by_stage=R09（宿主集成）最迟 R10（跨平台产物）。Linux 额外预计需要 webkit2gtk 系统依赖（未测）。
- 授权后录音/听写正链路（需真实用户点 TCC 弹窗）：UNVERIFIED-interactive，归 R09 人工验收；本任务红线禁止伪造授权。
- 屏幕真实采集（ScreenCaptureKit 或 WebView 截图等价物）：UNVERIFIED，归 R09 选型。
- 代理矩阵（WebView 级与 reqwest 级）：UNVERIFIED，归 R09（D-03 已锁 service 侧 rustls+system-proxy）。
- WebContentsView 多视图层级合成、窗口状态持久化、托盘交互点击、save/message 对话框、剪贴板富格式：未逐项迁移/实测，归 R09 设计输入。

## 9. 重放

```bash
# 前置：rustup 工具链（仓库根 rust-toolchain.toml）、node/npm（仅 npx tauri CLI 与证据服务器）、
#       swiftc（Xcode CLT）；测试密钥首次会自动生成到 /tmp/lingxi-t06-keys/（若重新生成需同步
#       tauri.conf.json pubkey 并重跑）
zsh spike/tauri-shell/scripts/run_e2e.sh <证据目录绝对路径>
```

runner 每步写 expected/actual/exit 到 `<证据目录>/runner-summary.txt`；本轮全部步骤 exit=0（15 步，含打包与打包冒烟）。sidecar 集成测试可独立重放：`cd spike/tauri-shell/app/src-tauri && SIDECAR_BIN=<sidecar二进制> cargo test --release --test sidecar_handshake`。

# ADR-002 浏览器宿主选型（R01-T04）

- 状态：提议（prototype 证据完备，待评审）
- 日期：2026-09-25
- 基线：`08b9f075032455be5f57f8fc882bf366b25b4269`
- 证据：`docs/rust-tauri/R01/BROWSER_SPIKE_REPORT.md`、`artifacts/rust-tauri/R01/T04/`

## 背景

Lingxi 现役浏览器能力由 Electron 主进程承载（desktop/main.cjs，契约矩阵见 spike 报告 §1：分区隔离、data-hana-ref 快照、点击/insertText 输入/select/滚动、视口截图、弹窗转新 tab、挂起不销毁、cookie 开关、不可信页 sandbox）。Rust/Tauri 迁移必须给出等价宿主：「打开外部浏览器」或「仅后台截图」不构成等价。

## 候选与实际验证

### 候选 A：系统 WebView（macOS=WKWebView，可经 Tauri/wry）
- 裸 WKWebView Swift 原型实测 15/18 VERIFIED（`artifacts/…/wkwebview/`）。
- 缺口：W6 上传（无 setFileInputFiles 等价物）；W13 代理（API 存在但 loopback 隐式绕过 + 红线禁外发 → 负向不可验）；W16 用户接管（未签名 CLI 不可激活，OS 键入不可达；签名 app 应可解但未实测）。
- 经 wry 0.57.0 封装覆盖面**严格更小**：无截图公共 API、macOS 下载完成回调路径恒空、代理需 mac-proxy feature、WebContext 仅进程级数据目录、无程序化 file input（grep 取证 `artifacts/…/wry-api-surface/`）。
- 判定：**不覆盖全部契约，拒绝作为主宿主**。

### 候选 B：受控 Chromium（CDP `--remote-debugging-pipe`，fd3/fd4 无 TCP 调试口）
- Rust 原型 `rust/crates/lingxi-browser-spike` 实测四阶段 **全 VERIFIED**（main 20 步 / isolation 6 步 / proxy 2 步 / takeover 4 步，exit 全 0）。
- 覆盖全部 REQUIRED 语义，且超出现役基线两项：整页截图（captureBeyondViewport）与 agent 驱动上传（setFileInputFiles）——现役均无（C8 视口级、C16 缺口）。
- 会话隔离（A08）经 BrowserContext 验证；不可信页拿不到 CDP/宿主面（探针 B4）。
- 成本：宿主 ~1.4GB 级（Chrome.app 实测；正式分发可为随包 Chromium，R09 定）；驱动 2.3MB；会话 RSS ~1.36GB。
- 判定：**采纳**。

### 现役对照（A07 旧侧）
Electron 42.8.1 harness 复刻生产语义：main 17 VERIFIED / 1 UNVERIFIED（上传=生产自身缺口），登录持久化跨进程 VERIFIED，用户接管 VERIFIED。等价基线完整建立。

## 决策

浏览器宿主方向 = **受控 Chromium + CDP over pipe（fd3/fd4）**，由 Rust 侧直接驱动；不引入 wry/Tauri webview 作为浏览器能力承载（Tauri 仍可用于壳/窗口，R09 另行决策）。外部浏览器方案与纯后台截图方案明确排除（不等价）。

## 理由

1. 唯一全项 VERIFIED 的候选（macOS arm64 实测）。
2. CDP pipe 无 TCP 调试口，攻击面小于 remote-debugging-port；不可信页探针实测拿不到宿主面。
3. 语义对齐生产契约（快照脚本逐字节复用生产 `SNAPSHOT_SCRIPT`，sha256=387b2e10…0e21 交叉核对三宿主一致）。
4. 实测发现已固化为迁移要求（F1 可信手势弹窗、F2 loopback 代理绕过、F3 会话 cookie 重启语义、F4 模态对话框事件驱动）。

## 退出条件（出现以下任一情况须重开本 ADR）

- R09 决定随包分发 Chromium 时体积/许可/更新通道不满足发布约束；
- Windows/Linux 门禁（R09/R10）实测 CDP pipe 语义不成立；
- Chromium 上游移除 `--remote-debugging-pipe` 或相关 CDP 域；
- 契约新增 WKWebView 独有且 Chromium 无法满足的能力。

## 后续强制门禁

- Windows / Linux 逐能力重验（本 ADR 仅 macOS arm64 VERIFIED）→ R09/R10。
- 生产缺口（C14 代理只管 defaultSession、C15 无下载 handler、C16 无上传路径）进 R09 需求清单，迁移实现不得低于现役且应评估补齐。

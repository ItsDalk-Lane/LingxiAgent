# R01-T04 浏览器宿主替换 Spike 报告（BROWSER_SPIKE_REPORT）

- 任务：R01-T04（浏览器宿主替换原型），验收 R01-A07（旧↔新等价）/ R01-A08（会话隔离），均为 REQUIRED。
- 基线：`08b9f075032455be5f57f8fc882bf366b25b4269`，分支 `codex/rust-tauri-migration`。
- 平台：macOS arm64（Darwin 27.0.0）**实测**；Windows / Linux **UNVERIFIED**（R09/R10 强制门禁）。
- 方法：全部场景跑在本地受控测试站 `tests/migration/r01-t04/testsite.mjs`（127.0.0.1:18281，表单/上传/下载/弹窗/长页/登录/探针），代理用 `proxy.mjs`（127.0.0.1:18282，**仅转发 loopback**）。无真实账号、无真实用户数据；**测试流量全部指向 loopback**。注意「无真实外发」不成立于浏览器自带后台流量：非 proxy 阶段（main/isolation/takeover）浏览器直连运行，Chrome 自身的 GCM 注册等后台外联真实发生（内容为浏览器自身遥测，无任务数据/凭证），披露、定性、缓解与证据见 R01-T04_REPORT.md §4 末段及 `artifacts/rust-tauri/R01/T04/docfix-r1-gcm/`。
- 证据根：`artifacts/rust-tauri/R01/T04/`（`chromium{,-iso,-proxy,-takeover}/`、`wkwebview/`、`electron/`、`wry-api-surface/`、`reverify/`、`host-cost.txt`）。

## 1. 现役浏览器契约矩阵（desktop/main.cjs 行级取证）

| # | 能力 | 生产语义 | 源码锚点（desktop/main.cjs） |
|---|------|----------|------------------------------|
| C1 | 会话分区 | 每会话独立 Electron partition：`persist:hana-browser-<sha256(sessionKey)[:32]>` | 3465-3472（`_browserPartitionName`） |
| C2 | DOM 引用快照 | `SNAPSHOT_SCRIPT` 注入 `data-hana-ref`，30k 截断，兄弟行合并 | 3288-3453（sha256=387b2e10…0e21） |
| C3 | 快照下发 | `wc.executeJavaScript(SNAPSHOT_SCRIPT)` | 4289-4298（`case "snapshot"`） |
| C4 | 点击 | `querySelector('[data-hana-ref=n]')` → scrollIntoView + `el.click()` | 4319-4333（`case "click"`） |
| C5 | 输入（含中文） | ref 聚焦/select → `wc.insertText`；可选 `sendInputEvent Return` | 4335-4357（`case "type"`） |
| C6 | 选择 | ref → `el.value=…` + `change` 事件 | 4369-4381（`case "select"`） |
| C7 | 滚动 | `window.scrollBy` 后重取快照 | 4360-4368（`case "scroll"`） |
| C8 | 截图 | `wc.capturePage()`（**视口级**，无整页）；thumbnail 同路径 resize | 4301-4316 |
| C9 | 弹窗 | `setWindowOpenHandler` → **deny + 转为新 tab** | 3805-3809 |
| C10 | 多 tab | workspace.tabs: tabId→WebContentsView，切/关 tab IPC | 504、5512-5554 |
| C11 | 挂起/恢复 | detach view **不销毁**，页面状态全保留；闲置回收双超时 | 4106-4123（`case "suspend"`）、4533 |
| C12 | cookie 开关 | webRequest 剥离/放行 Cookie 与 Set-Cookie | 3721-3748 |
| C13 | 清站点数据 | `ses.clearStorageData`（cookies/localstorage/indexdb/…） | 3751-3766 |
| C14 | 代理 | `applyDesktopNetworkProxy` **只作用 defaultSession**（契约缺口：浏览器 partition 不在其内） | 291-308 |
| C15 | 下载 | **无显式 handler**（契约缺口，desktop 全文无 will-download/setFileInputFiles） | grep 全文无命中 |
| C16 | 上传 | **无程序化 file input 赋值路径**（契约缺口，同上 grep） | grep 全文无命中 |
| C17 | 不可信页隔离 | `nodeIntegration:false, sandbox:true`，无 preload 注入到浏览器 view | 3984-3988 |

## 2. 候选 A：系统 WebView（WKWebView 原生 Swift spike）

宿主来源：macOS 系统 WebKit（无第三方运行时）。原型 `tests/migration/r01-t04/wkwebview_spike.swift`（swiftc -O，190KB）。
结果：run5 `artifacts/rust-tauri/R01/T04/wkwebview/transcript.jsonl`，**15 VERIFIED / 0 FAILED / 3 UNVERIFIED**，exit=0。

| 步 | 能力 | 结果 | 实际 |
|----|------|------|------|
| W1 | 导航+生产 snapshot | VERIFIED | 同一 SNAPSHOT_SCRIPT（sha256 相同）跑出 ref 快照 |
| W2 | 视口截图 | VERIFIED | `takeSnapshot` 42968B PNG |
| W3 | 中文输入 | VERIFIED | JS 值注入路径（无 insertText IME 等价物，语义弱于生产 C5） |
| W4 | select+提交 | VERIFIED | echo 含中文标题与 choice=c |
| W5a/b | 滚动/整页截图 | VERIFIED | scrollY=900；`takeSnapshot(rect=全文档高)` 实测得到 1280x11848 整页图 |
| W6 | **上传(agent驱动)** | **UNVERIFIED** | WKWebView 无 `setFileInputFiles` 等价物；JS files 赋值 len=0（生产同样无此能力，见 C16） |
| W7 | 下载 | VERIFIED | WKDownload 落盘，内容与固定 payload 逐字节一致 |
| W8 | 弹窗 | VERIFIED | `createWebViewWith` 捕获（需 `javaScriptCanOpenWindowsAutomatically=true`） |
| W9 | JS 对话框 | VERIFIED | alert/confirm 委托回调拿到文案，confirm 拒绝→`confirm:false` |
| W10 | 登录+cookie | VERIFIED | WKHTTPCookieStore 可读 session cookie |
| W11 | 两会话隔离 | VERIFIED | 两个 `nonPersistent` dataStore 互不串（A08） |
| W12 | 登录持久化 | VERIFIED | `forIdentifier` dataStore 重建后仍 LOGGED_IN（macOS 14+ API） |
| W13 | 代理 | **UNVERIFIED** | `proxyConfigurations` API 存在且 per-dataStore，但 WK 对 loopback 目标隐式绕过代理；红线禁外发 → 负向验证不可完成 |
| W14 | 不可信页探针 | VERIFIED | hana/process/require/CDP 全不可得 |
| W15 | 挂起/热恢复 | VERIFIED | removeFromSuperview→重挂载，textarea 保留 |
| W16 | 用户接管 | **UNVERIFIED** | 页面侧就绪（firstResponder=wv、focusEl=t1），但无签名 CLI 进程 `appActive=false`——OS 键入到不了窗口；正规签名 app 可激活（Electron 对照 E16 证明通道本身可用） |

### 2b. 经 Tauri/wry 路径（API 面取证，未另行建 app）
wry 0.57.0 源码取证（`artifacts/rust-tauri/R01/T04/wry-api-surface/grep-evidence.txt`，crate sha256=a819957a…ff0）：
- **无截图公共 API**（grep `screenshot|snapshot|capturePage|take_snapshot` 零命中）——C8 无法经 wry 覆盖，须绕到原生；
- 下载 completed handler 在 macOS 上**路径参数恒为空**（lib.rs:1371-1375 文档明写 "macOS … always empty, due to API limitations"）；
- 代理需 macOS 14+ 且要 `mac-proxy` feature；Android/iOS 不支持；
- WebContext 只有进程级 `data_directory`，无 per-webview 非持久 dataStore 暴露；
- 无程序化 file input（仅原生 `runOpenPanel` 用户手势）。

结论：wry/Tauri 封装的 WKWebView 覆盖面**严格小于**裸 WKWebView；裸 WKWebView 已有 W6/W13/W16 三项不足 → 经 Tauri 不可能覆盖全部契约。故对「经 Tauri 原型」不做重复实测，以 API 面取证为据（任务书允许"取证不支持"替代实测）。

## 3. 候选 B：受控 Chromium（CDP `--remote-debugging-pipe`，fd3/fd4，无 TCP 调试口）

宿主来源：本机 `/Applications/Google Chrome.app`（153.0.8010.52，host-provided；正式方案可换 Chromium 运行时随包分发）。驱动：`rust/crates/lingxi-browser-spike`（新 prototype crate，已注册 DEPENDENCY_RULES）。

| 阶段 | 证据 | 结果 |
|------|------|------|
| main（A1-A14，20 步） | `chromium/transcript.jsonl` | **全 VERIFIED**，exit=0。含：无 TCP 9222（nc 退出码=1）、生产 snapshot、中文输入、select、计数点击、表单提交、滚动、**整页截图**（captureBeyondViewport，高>3000px）、上传（setFileInputFiles + 服务端 sha256 校验）、下载落盘逐字节一致、弹窗（可信手势+重试，见发现 F1）、alert/confirm 接受/拒绝、登录+cookie、挂起热恢复、RSS 采样（树 1.36GB）、冷重启登录仍在 |
| isolation（B1-B6） | `chromium-iso/transcript.jsonl` | **全 VERIFIED**，exit=0。双 BrowserContext 互不串（A08）；探针页拿不到 hana/process/CDP；dispose 后 cookie 查询被拒 |
| proxy（C1/C2） | `chromium-proxy/transcript.jsonl` + `proxy-server.log` | **全 VERIFIED**，exit=0。死代理导航失败（`--proxy-bypass-list=<-loopback>` 关闭隐式绕过，见 F2）；活代理日志见 `HTTP GET http://127.0.0.1:18281/form` |
| takeover（D1-D4） | `chromium-takeover/transcript.jsonl` + `d3-screen.png` | **全 VERIFIED**，exit=0。窗口可见（CGWindowList onscreen=1）；osascript 独立 OS 通道键入 `HUMAN-TAKEOVER` 落进页面输入框且 CDP 读回；整屏 screencapture 1.8MB |

## 4. 现役 Electron 宿主对照（A07「旧浏览器」一侧）

harness `tests/migration/r01-t04/electron_harness.cjs`（复刻分区命名/SNAPSHOT_SCRIPT/insertText/capturePage/windowOpenHandler/cookie 剥离语义），Electron 42.8.1，证据 `electron/transcript-{main,login-write,login-verify}.jsonl`：
- main：**17 VERIFIED / 1 UNVERIFIED（E6 上传=契约缺口 C16）/ 0 FAILED**，exit=0；
- 登录持久化跨进程（P1 写入进程 → P2 新进程同 userData 读回）：均 VERIFIED；
- E16 用户接管 VERIFIED（正规 .app 可激活，osascript 键入落页面）；
- E15 记录 C14 缺口：defaultSession resolveProxy 生效，浏览器 partition 不在生产代理配置内。

## 5. 候选对照与结论

| 契约项 | WKWebView(裸) | WKWebView(经 wry) | 受控 Chromium(CDP) | Electron(现役对照) |
|--------|:---:|:---:|:---:|:---:|
| 导航+snapshot(C2/C3) | ✅ | ✅(evaluate_script) | ✅ | ✅ |
| 点击/输入/select/scroll(C4-C7) | ✅(JS 路径) | ✅(JS 路径) | ✅(含可信手势) | ✅ |
| 截图(C8) | ✅视口+整页(rect) | ❌无 API | ✅视口+整页 | ✅视口（生产即视口语义） |
| 弹窗(C9) | ✅ | ✅(new_window handler) | ✅ | ✅ |
| 下载 | ✅(WKDownload) | ⚠️macOS 完成回调无路径 | ✅ | ✅(will-download 观测) |
| 上传(agent 驱动) | ❌无 API | ❌无 API | ✅setFileInputFiles | ❌（生产亦无） |
| cookie/登录持久化 | ✅(forIdentifier) | ✅(cookies API) | ✅ | ✅(partition 跨进程) |
| 两会话隔离(A08) | ✅(dataStore) | ⚠️仅进程级 data_directory | ✅(BrowserContext) | ✅(partition) |
| 代理(C14) | ⚠️API 有、loopback 绕过不可负验 | ⚠️同左+feature gate | ✅正负双验 | ⚠️（生产缺口本身） |
| 挂起/恢复(C11) | ✅ | ✅ | ✅ | ✅ |
| 用户接管 | ⚠️未签名 CLI 受限 | 未测 | ✅osascript 键入 | ✅osascript 键入 |
| 不可信页隔离(C17) | ✅ | ✅ | ✅ | ✅ |

**结论：受控 Chromium（CDP over pipe）是唯一覆盖全部 REQUIRED 语义且有 macOS arm64 实测的候选**，选为 ADR-002 方向。WKWebView 在上传驱动、代理负验、未签名接管上不足，且经 wry 封装缺口更大（无截图 API）。

## 6. 成本与外部依赖（host-cost.txt）

- Chrome.app：1.4GB（host-provided 实测值；正式分发若随包带 Chromium 运行时是同级成本，归 R09 决策）。
- spike 驱动二进制：2.3MB（debug）；wkwebview_spike：190KB；Electron.app：273MB。
- Chromium 会话 RSS：进程树 1.36GB（A14 采样，5 步会话后）。
- 外部依赖：CDP 协议（Chromium 内置，无 TCP 口）；fd3/fd4 pipe 需 libc（已锁定版本提升为直接依赖，D-09）。

## 7. 实测发现（迁移必须保留的语义）

- **F1 弹窗需可信手势**：JS `el.click()` 触发的 `window.open` 被 Chromium 弹窗拦截；须 CDP `Input.dispatchMouseEvent` 合成可信手势 + `Page.bringToFront`，且 attach 过早会使弹窗停在 about:blank（先轮询 `Target.getTargets` URL 再 attach）；重负载后偶发首击停滞，重试一次成功。
- **F2 loopback 隐式绕过代理**：Chromium 与 WKWebView 都默认绕过 loopback 代理；Chromium 需 `--proxy-bypass-list=<-loopback>` 才能对本地站做代理负验。
- **F3 会话 cookie 不过重启**：无 Max-Age 的 cookie 冷重启即丢（Chromium 语义）——测试站已改 Max-Age=86400；正式迁移若要求跨重启登录，须确认站点/策略。
- **F4 模态 JS 对话框悬挂 renderer JS**：evaluate/click 响应都会阻塞，驱动必须事件驱动（等 `javascriptDialogOpening`）而非请求-响应。
- **F5 WKWebView 未签名 CLI 不可激活**：OS 级用户接管需要正规签名 app 身份（E16 证明通道存在）。
- **F6 生产缺口（非本次引入）**：下载无显式 handler（C15）、上传无程序化路径（C16）、代理只管 defaultSession（C14）。迁移等价基线=「不低于现役」，但缺口须记入 R09 需求清单。

## 8. 逐平台状态

| 平台 | 状态 | 说明 |
|------|------|------|
| macOS arm64 | **VERIFIED** | 本机实测（三宿主全部跑通） |
| Windows | UNVERIFIED | R09/R10 强制门禁；Chromium CDP pipe 与 WKWebView→WebView2 语义需重验 |
| Linux | UNVERIFIED | 同上（webkit2gtk/CDP） |

# R09-T05 浏览器、PDF 与媒体展示最终接线 — 实现设计调查

- 任务：R09-T05「浏览器、PDF和媒体展示最终接线」（验收 R09-A09 / R09-A10，均 REQUIRED）
- 仓库：`/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`
- 基点：HEAD `aac8cb9fe6bb7bfa7a41d75528221136bfe55e50`。开工时工作区仅有 R08 未跟踪缓存；调查期间（2026-10-26）出现其它 R09 任务的并发未提交改动（如 `transport.rs`/`preview.rs` 的 Tauri origin 接缝、`package*.json`、xtask 注册），本文件的“现状”结论以 HEAD 源码为准，并发改动不视为本任务结论（见 OQ-06 注与 JSON `workspaceConcurrentChanges`）。本任务只创建/覆盖本设计文件与证据 JSON。
- 调查方式：**只读源码/文档取证**。本文件不修改生产代码、配置或阶段图；文中所有“现状”结论均来自阅读源码/既有报告，**本次调查未运行任何测试或 GUI**；标注“测试存在”不等于本轮已通过。
- 配套机器证据：`docs/rust-tauri/R09/T05_BROWSER_PDF_EVIDENCE.json`（schema `lingxi.r09.t05-design.v1`）
- 相邻必读：`docs/rust-tauri/R01/ADR-002-browser-host.md`、`docs/rust-tauri/R01/ADR-003-document-renderer.md`、`docs/rust-tauri/R09/START_RECEIPT.md`、`docs/rust-tauri/R09/BLOCKED.md`

---

## 0. 结论摘要（TL;DR）

1. **浏览器与 PDF 的服务端能力已在 R07 落地并接线**：受控 Chromium + CDP pipe + 每会话独立进程/profile + `BrowserService` 授权代次/急停撤权 + HTTP/工具/WS 宿主面；PDF 走同一受控 Chromium 的 `Page.printToPDF`，独立一次性 headless 进程，带 Fetch allowlist/CSP/DenyProxy/超时/清理。现状无需重写渲染器，R09-T05 的工作是**宿主侧最终接线与验收取证**。
2. **可见浏览器窗口不应注入 Tauri 窗口**。Tauri/wry 是 WKWebView/WebView2/WebKitGTK，没有承载原生 Chromium 窗口的能力；跨进程窗口重parent 依赖私有/平台特定 API 且破坏输入法/焦点/Chrome 自身 UI。ADR-002 已裁定“受控 Chromium 自带窗口 + Rust 驱动”（`docs/rust-tauri/R01/ADR-002-browser-host.md:32`）。因此 Tauri 侧只需要一个**控制面板窗口（viewer）**，网页本身仍在该服务控制的 Chromium 顶层窗口里（当前 Electron Rust-node 路径正是如此：`desktop/src/react/browser-viewer/RustBrowserViewer.tsx:7`、`desktop/main.cjs:5753-5766`）。
3. **viewer 的浏览器控制桥必须宿主化**：连接凭据（loopback token）只留在 Tauri 原生宿主，viewer webview 只拿不透明 connectionKey/sessionPath，命令由宿主 reqwest 发往服务；这等价于今天 Electron main 的做法（`desktop/src/shared/rust-browser-control.cjs:12-60`、`desktop/main.cjs:5737-5751`），并天然规避 Tauri origin 不在服务 Origin allowlist 的问题（`rust/crates/lingxi-service/src/transport.rs:224-267`）。
4. **发现两个 Tauri 独有的硬接线缺口（必须在实现期解决，属 R09-T05 及 T02/T06 交界）**：
   - **服务 Origin allowlist 不含 Tauri webview origin**：`check_origin` 只允许 loopback http(s)、`file://`、`null`（`rust/crates/lingxi-service/src/transport.rs:224-267`），Tauri 默认 origin（macOS/Linux `tauri://localhost`、Windows `http://tauri.localhost`）会被 `bad_origin` 拒绝。现状 renderer 的 `fetch`（文件预览、聊天、设置等）在 Tauri 下会全部失败，除非 T02/T06 选定“宿主代理 HTTP”或扩展精确 origin 白名单。
   - **HTML 预览 iframe 的 CSP `frame-ancestors` 不包含 Tauri origin**：`build_html_preview_csp` 只允许 `'self' file: http://127.0.0.1:* http://localhost:*`（`rust/crates/lingxi-service/src/preview.rs:426-433`），Tauri 页面嵌入会被浏览器按 CSP 拒绝；同时对本地文件的媒体/markdown 图片没有 `file://` 等价物（Tauri webview 不能像 Electron 那样直接吃 `file://`）。
5. **PDF 链路在 Tauri 下不需要宿主命令**：`office_html-to-pdf` 是模型工具（经真实 Run + 网关 + 审批），服务内部调 `BrowserPort::render_pdf`（`rust/crates/lingxi-service/src/office_tools.rs:625-747`、`rust/crates/lingxi-service/src/office_pdf.rs:305-374`、`rust/crates/lingxi-adapters/src/browser/host.rs:407-514`）。Tauri 只负责显示结果文件（会话文件预览）。渲染器进程所有权在 lingxi-service，不在 Tauri host。
6. **“无 Electron”要求本质是安装/进程/调用链探针 + 旧链产物对照**：当前仓库没有任何“发行版自带 Chromium”的实现，服务只做路径发现（`rust/crates/lingxi-service/src/browser.rs:1271-1331`）；A09 需要冻结一份与安装包隔离的测试安装（Tauri app，不含 Electron 文件/进程），并行用隔离 harness 重放旧链（Electron 只在对照环境中运行，不进入测试安装）。
7. **A09 的旧链基线当前缺失**：R07 PDF 质量门禁依赖 `artifacts/rust-tauri/R01/T05/a09/new/probe/probe.json`（`rust/crates/lingxi-adapters/tests/r07_pdf_chromium.rs:391-397`），但本工作区 `artifacts/rust-tauri/` 只有 R02/R07/R08，R01 证据不在库内。A09 比对必须先按 `scripts/rust-tauri/r01-t05-replay.sh` 的旧链路径重新生成并冻结基线（旧链只在独立对照环境运行）。
8. **A10 的两个攻击面已在渲染器或服务侧有结构性防线，但必须在 Tauri 下重新证明**：HTML 预览是跨源 token 化 iframe + `sandbox="allow-scripts"` + CSP `connect-src 'none'`（`desktop/src/react/components/preview/PreviewRenderer.tsx:145-166`、`rust/crates/lingxi-service/src/preview.rs:377-433`）；SVG 走 `<img>`（不执行脚本）。恶意内容能运行脚本但拿不到 app origin/Tauri IPC/服务 token；需要用“Tauri 测试命令 canary + loopback canary + token 扫描 + 正常预览对照”取证。
9. **Chromium 分发与许可仍是开放决策**：ADR-002/ADR-003 把“随包 / 复用系统”留给 R09（`docs/rust-tauri/R01/ADR-002-browser-host.md:24,41-46`、`docs/rust-tauri/R01/ADR-003-document-renderer.md:54-59,61-67`）。建议分两步：R09 实现按“部署提供 Chromium 路径（显式失败，不静默降级）”接线并以安装目录内 Chromium 资源或测试 fixture 完成本机证据；**随包/签名/公证/更新通道的最终方案归 R10 发布决策**，但 R09 必须落一份可执行的分发方案或登记为 OPEN 风险。
10. **平台阻塞沿用**：Windows x64 / Linux x64 / macOS x64 无真机（`docs/rust-tauri/R09/BLOCKED.md:8-10`），A09/A10 的跨平台份额只能记 `BLOCKED_PLATFORM`，R09 不能声称跨平台通过。

---

## 1. 现状（A：服务端能力、PDF 链、媒体/富文本预览、现有宿主）

### 1.1 服务端浏览器能力总表

| # | 能力 | 真实实现（file:line） | 测试/证据（file:line） | 现状判断 |
|---|---|---|---|---|
| B1 | 浏览器端口契约（窄端口，不泄露窗口/进程/凭据句柄） | `rust/crates/lingxi-kernel/src/browser.rs:90-147`（`BrowserPort`：execute/workspaces/restore/revoke/configure/clear_site_data/read_auxiliary_page/render_pdf/pdf_scopes/cancel_pdf_scope/acknowledge_downloads/reap_idle/shutdown） | — | 已冻结 |
| B2 | 服务级授权/归属闸（bridge/phone 拒绝、会话 active、owner、生成 BrowserAuthority 行） | `rust/crates/lingxi-service/src/browser.rs:493-537` | `rust/crates/lingxi-service/tests/r07_browser_routes.rs:476-547`（chat device 只读、仅显式远端 admin 可控制本机节点） | 已实现 |
| B3 | 工具调用绑定真实 Run/尝试/generation/审批/授权代次 | `rust/crates/lingxi-service/src/browser.rs:538-620`（实际行 SQL 与 lineage/bridge 拒绝、`authorizationEpoch` 绑定） | `rust/crates/lingxi-service/tests/r07_browser_tools.rs:665-687`（导航需审批且只派发精确 URL）、`:689-707`（页面变化/异 tab 使审批失效） | 已实现 |
| B4 | 命令执行/记录：操作幂等收据、并发上限 64、45s 截止、取消与清理未确认 | `rust/crates/lingxi-service/src/browser.rs:621-747`（claim receipt 653-674；active 上限 676-697；select+cancel 709-713） | `rust/crates/lingxi-adapters/tests/r07_browser_store.rs:62-115`（收据去重/输入变化/unknown 不重派发）、`rust/crates/lingxi-service/tests/r07_browser_routes.rs:434-474` | 已实现 |
| B5 | 回执落地：state 持久化、截图/下载登记为会话文件、`browser_status` 事件、授权变化时旧状态不发布 | `rust/crates/lingxi-service/src/browser.rs:748-858`（截图 770-787、下载 788-837、事件 856） | `rust/crates/lingxi-service/tests/r07_browser_routes.rs:666-708`（真实 PNG 会话文件 + HTTP 类型/字节） | 已实现 |
| B6 | 截图 | `rust/crates/lingxi-adapters/src/browser/ops.rs:650-680`（`Page.captureScreenshot` PNG 校验、24MB 预算） | `rust/crates/lingxi-adapters/tests/r07_browser_chromium.rs:269-352`（真实截图 PNG magic 断言） | 已实现（视口级；整页截图在 R01 spike 已验但生产契约保持视口语义） |
| B7 | 启动/冷恢复 | `rust/crates/lingxi-adapters/src/browser/host.rs:707-956`（首个 target 建窗口 887-923；冷恢复只建 metadata 不自动打开 1206-1260） | `rust/crates/lingxi-service/tests/r07_browser_routes.rs:613-665`（重启只恢复元数据，不自动打开页面） | 已实现 |
| B8 | 多 tab：新标签/切换/关闭/上限 | `rust/crates/lingxi-adapters/src/browser/ops.rs:71-141`（`Target.createTarget`，首个 target `newWindow:true`，MAX_TABS）、`:843-862`（SwitchTab）、`:863-880`（CloseTab 等真实 `Target.targetDestroyed`） | `rust/crates/lingxi-adapters/tests/r07_browser_chromium.rs:269-352`（close/next tab 后 state 断言） | 已实现 |
| B9 | 导航/前进/后退/刷新 + load 等待 | `rust/crates/lingxi-adapters/src/browser/ops.rs:142-239`（`Page.navigate` + `Page.lifecycleEvent load`；下载分支 165-212） | `rust/crates/lingxi-adapters/tests/r07_browser_chromium.rs:269-352`（真实 back/forward/reload 文档事实断言） | 已实现 |
| B10 | 快照/点击/输入/select/滚动/key/wait/evaluate | `rust/crates/lingxi-adapters/src/browser/ops.rs:240-505`（快照引用上下文）、`:680-790`（click 可信手势/type/select/scroll/key/wait/evaluate） | `rust/crates/lingxi-adapters/tests/r07_browser_chromium.rs:804-890`（快照不泄密码、页面不能伪造 ref） | 已实现 |
| B11 | 弹窗转新 tab + 模态 dialog 事件驱动 | `rust/crates/lingxi-adapters/src/browser/runtime.rs:464-520`（新 target/弹窗并入 tabs 与 active 刷新）、`:736-743`（`Page.javascriptDialogOpening` 在非接管时自动 accept/deny）；可信手势点击 `ops.rs:680-720` | `rust/crates/lingxi-adapters/tests/r07_browser_chromium.rs:677-756`（真实 popup/dialog 不阻塞） | 已实现（R07 T07 仍保留一次真实 Chromium popup 失败记录，见 B27） |
| B12 | 用户接管 / 交还 | `rust/crates/lingxi-adapters/src/browser/ops.rs:816-843`（Show/Takeover→`visible(normal)`+takeover 标志；Release 清标志；清空快照 ref）；操作阻断 `:590-605`（takeover 时 agent 修改/滚动/切 tab 报 `browser_user_takeover`） | 单测/适配器覆盖；真实 OS 键入证据见 R07 A07 补验 `docs/rust-tauri/R07/T04_REPORT.md:108-117`（Cua 原生 Chrome 窗口 click/type，AX 读回 human-latest） | 已实现；OS 前台激活能力未实现（见 OQ-08） |
| B13 | 挂起/恢复/闲置回收 | `rust/crates/lingxi-adapters/src/browser/ops.rs:844-852`（Suspend 最小化）、`rust/crates/lingxi-adapters/src/browser/host.rs:1781-1826`（reap_idle：仅回收非 visible 且无忙），上限 5 工作区逐出（`host.rs:754-837`） | `rust/crates/lingxi-adapters/tests/r07_browser_chromium.rs:648-676`（LRU 有界 + 冷元数据保留）、`:585-647`（revoke 真实中断 pending 页面） | 已实现 |
| B14 | 急停撤权（服务层）：cancel active→revoke_human_intent（epoch 递增）→port.revoke→记录撤销态→发布事件 | `rust/crates/lingxi-service/src/browser.rs:905-993`（cancel 944-948；revoke 951-957；record 859-904；事件 988-991） | `rust/crates/lingxi-adapters/tests/r07_browser_store.rs:116-172`（急停需真实新用户 run，旧请求 replay 不能再撤权） | 已实现 |
| B15 | 新用户消息撤销急停 | `rust/crates/lingxi-service/src/runs.rs:2836-2852`（RunOrigin::User 时 `release_for_user_run`）；`rust/crates/lingxi-service/src/browser.rs:191-234` | `rust/crates/lingxi-service/tests/r07_browser_tools.rs:787-848`（新用户提交释放急停并产生新 epoch）、`:1112+`（重启后冷状态和新用户启动） | 已实现 |
| B16 | 会话浏览器状态目录 / 冷恢复网址 / 状态查询 | `rust/crates/lingxi-service/src/browser.rs:994-1057`（live+持久合并、撤销态覆盖）；`rust/crates/lingxi-service/src/browser_routes.rs:80-108` | `rust/crates/lingxi-service/tests/r07_browser_routes.rs:613-665` | 已实现 |
| B17 | 维护循环 30s：生命周期复查→撤权/forget、PDF 租约回收、reap_idle、状态同步 | `rust/crates/lingxi-service/src/browser.rs:1058-1139`（PDF scope 1061-1070；session 失活撤权 1078-1136）、`:1140-1173`（状态同步 + browser_status） | `rust/crates/lingxi-service/tests/r07_browser_routes.rs:548-612`（归档/永久删除只回收真实会话浏览器） | 已实现 |
| B18 | 受控 Chromium 发现与不可用降级 | `rust/crates/lingxi-service/src/browser.rs:1271-1331`（`LINGXI_CHROMIUM_PATH` 绝对可执行校验 1280-1285；查找 `data_home/bundled-bin/chromium/chrome`、`/Applications/Google Chrome.app/...`、`/usr/bin/chromium*`、`/usr/bin/google-chrome` 1287-1297；缺失→`UnavailableBrowser` + 显式原因 1319-1330） | `rust/crates/lingxi-service/src/browser.rs:36-78`（UnavailableBrowser 全部显式失败） | 已实现“发现”，**未实现“打包/随包”** |
| B19 | 启动参数与进程所有权：CDP pipe fd3/fd4、无 TCP 调试口、独立 profile、进程组、SIGTERM→SIGKILL、Unknown 保留 owner | `rust/crates/lingxi-adapters/src/browser/launcher.rs:94-110`（pipe）、`:119-220`（launch；`--remote-debugging-pipe`、`--user-data-dir`、`--no-startup-window`、`--window-size`；headless 可选 151-153；`process_group(0)` 154；fd dup2 155-163）、`:221-318`（stop；Browser.close 可选、kill(-pgid) 276-286、wait 287-317）；`rust/crates/lingxi-adapters/src/browser/host.rs:1087-1174`（close_all 并发清理 + Unknown 保持 stopping） | `rust/crates/lingxi-adapters/src/browser/startup_tests.rs`（启动故障注入 7 项，R07 门禁 `native-startup-faults` 通过，见 `docs/rust-tauri/R07/T04_REPORT.md:46-53`） | 已实现 |
| B20 | 会话隔离：每会话独立 Chromium 进程 + profile 目录（identity=sha256(node,owner,session)），cookie 策略 per-profile，站点数据清理 | `rust/crates/lingxi-adapters/src/browser/host.rs:158-170`（identity_key）、`:847-869`（profile/downloads/private_dir、`managed_files::configure_cookies`）、`:1347-1409`（configure 关闭当前页面保留登录资料） | `rust/crates/lingxi-adapters/tests/r07_browser_chromium.rs:372-548`（独立持久 profile/辅助 locale/站点数据清理）、`:757-803`（永久 forget 删除自有 profile、拒绝迟到 scope）；A08 隔离探针见 `docs/rust-tauri/R01/BROWSER_SPIKE_REPORT.md:72` | 已实现（进程级隔离，强于 BrowserContext） |
| B21 | 下载 | `runtime.rs:781-835`（`Browser.downloadWillBegin/Progress`、64 件上限、50MiB 预算）；服务登记 `rust/crates/lingxi-service/src/browser.rs:788-837`；确认 `host.rs:1730-1780`（acknowledge_downloads） | `rust/crates/lingxi-adapters/tests/r07_browser_chromium.rs:549-583`（真实下载字节 + ack 只移除确认 GUID）；`r07_browser_routes.rs:735-798`（unknown ack 后重放复用真实文件） | 已实现（生产缺口 C15 在迁移实现中已补齐） |
| B22 | 上传（agent 驱动） | **生产未实现**：`BrowserAction` 枚举无 upload（`rust/crates/lingxi-protocol/src/browser.rs:6-33`），browser 工具 ACTIONS 无 upload（`rust/crates/lingxi-service/src/browser_tools.rs:21-35`），adapters 无 `DOM.setFileInputFiles`。R01 spike 曾验证 CDP 上传可行（`docs/rust-tauri/R01/BROWSER_SPIKE_REPORT.md:71,89-93`），未进 R07 生产面 | 无生产测试；spike 证据仅原型 | **旧链 C16 缺口保留**（ADR-002 要求“评估补齐”，R07 未补齐）；R09 不得声称上传已实现，需在 T05 报告登记 |
| B23 | 网络平面/代理 | `rust/crates/lingxi-adapters/src/browser/host.rs:1837+`（`proxy_arguments`）；服务 `browser.rs:1305-1307`（NetworkPlane 注入） | R01 spike proxy 阶段；R07 adapter 组（`docs/rust-tauri/R07/T04_REPORT.md:90-101`） | 已实现；生产旧缺口 C14（只管 defaultSession）以 NetworkPlane 统一注入解决 |
| B24 | HTTP 面（viewer/设置/静态） | `rust/crates/lingxi-service/src/browser_routes.rs:235-248`：workspaces/sessions/session-states/open-session/close-session/command/preferences GET+PUT/clear-cookies；错误映射 14-27 | `rust/crates/lingxi-service/tests/r07_browser_routes.rs`（12 项）、`r07_browser_tools.rs`（8 项） | 已实现 |
| B25 | 宿主 WS 面（仅“恢复”命令，避免第二套浏览器控制通道） | `rust/crates/lingxi-service/src/browser_host.rs:34-73`（resume）、`:76-153`（仅本机 owner、单连接、WS 升级）、`:155-217`（browser-cmd/browser-result、6s 超时、心跳鉴权） | `rust/crates/lingxi-service/src/browser_host.rs`（无独立测试文件；由 R07 flows 覆盖，`docs/rust-tauri/R07/T04_REPORT.md:40-44`） | 已实现（Tauri 若不需要“宿主恢复旧 tab”可暂不使用） |
| B26 | 浏览器工具（模型面） | `rust/crates/lingxi-service/src/browser_tools.rs:21-35`（ACTIONS 白名单）、`:36-60`（URL 规范化仅 http/https 无凭据）、`:61-130`（审批 scope 绑定真实 workspace/tab/takeover/revoked）、`:131-181`（执行器）、`:195-216`（注册 + `ArgumentAwareBrowser` 权限） | `rust/crates/lingxi-service/tests/r07_browser_tools.rs:640-786`（只读收据、审批后精确派发、页面变化失效、unknown 不报成功、取消可达） | 已实现 |
| B27 | 现有固有失败记录（不得抹掉） | `docs/rust-tauri/R07/R07_HANDOFF.json:36-44`（t07BrowserFailures：真实 Chromium 弹窗、cookie 后 Resume 导航锁屏受阻未改写） | `docs/rust-tauri/R07/T07_FAILURE_HISTORY.json:4798-4824`（`trusted_click_discovers_a_real_popup...` 失败行） | R09 必须重跑并保留/修复记录 |

**阶段结论**：服务端浏览器能力在 R07 已按 ADR-002 实现，Tauri 迁移不需要新的浏览器引擎或新的协议；T05 的“接线”集中在宿主窗口、宿主控制桥、Tauri 安全边界（origin/CSP/权限）与验收取证。

### 1.2 浏览器安全与授权（与 R09 直接相关）

- 每条命令都以 `operation` 幂等键落 `browser_commands_v1`，先 claim 再派发；unknown 状态不重复执行（`rust/crates/lingxi-service/src/browser.rs:652-697`，存储函数 `rust/crates/lingxi-adapters/src/storage/browser.rs:185-241`）。
- 授权代次 `epoch` 是急停/接管的唯一闸：host 侧 `reserve` 校验 owner/epoch/revoked（`rust/crates/lingxi-adapters/src/browser/host.rs:650-706`），tool scope 校验审批时的 `authorizationEpoch`（`rust/crates/lingxi-service/src/browser.rs:611-618`），Run 血缘回溯超过 64 层或 bridge/channel/dm 来源直接拒绝（`:559-588`）。
- 撤销后旧 Run、旧审批、旧插话都不能解除急停；只有新用户提交产生新 epoch（`rust/crates/lingxi-service/src/runs.rs:2836-2852`；测试 `r07_browser_tools.rs:787-848`）。
- 不可信网页隔离：页面只通过 CDP 被驱动，无 preload/Node 能力；R01 隔离探针验证页面拿不到 hana/process/CDP（`docs/rust-tauri/R01/BROWSER_SPIKE_REPORT.md:72,90-99`）。

### 1.3 PDF 导出链（现状）

**链路（真实生产）**：

```
模型工具调用 office 工具 html-to-pdf（display_name=office_html-to-pdf，wire name=local_name=html-to-pdf）
  → ToolInvocationGateway（真实 Run/审批/权限）
  → OfficeApi::pdf（rust/crates/lingxi-service/src/office_tools.rs:625-747）
      → office_pdf::collect：资产授权读取/本地引用改写/三族字体冻结（office_pdf.rs:305-374）
      → BrowserPort::render_pdf(PdfScope, PdfRenderJob)（office_tools.rs:673-684）
  → ChromiumHost::render_pdf（rust/crates/lingxi-adapters/src/browser/host.rs:407-514）
      → 一次性隔离进程：headless=true、独立 profile `<root>/aux/<identity>`、DenyProxy（host.rs:463-483）
      → pdf::render（rust/crates/lingxi-adapters/src/browser/pdf.rs:652-706）
          → Fetch.enable 全请求拦截 + 冻结资源 allowlist（pdf.rs:791-800、资源处理 302-490）
          → 导航到 `https://lingxi-pdf.invalid/<sha256>/index.html`（office_pdf.rs:336-339）
          → settle + 隔离 world 等待字体/图片就绪（pdf.rs:820-880）
          → Page.printToPDF（pdf.rs:883-916）→ 有界流读取/校验（pdf.rs:590-650）
      → 清理确认（host.rs:506-514；PdfGuard 106-133；cancel_pdf 362-406）
  → 产物校验 %PDF-/%%EOF/sha256/50MiB（office_tools.rs:685-695）
  → 写 outputPath 或登记为 SessionFile（office_tools.rs:696-747）
```

**关键事实（file:line）**：

| 契约 | 现状实现 |
|---|---|
| 入口仅为模型工具，无 HTTP 发起面 | `office_tools.rs:1188`（注册 8 个内置工具，html-to-pdf 是写工具）、`:1074-1076`；`office_routes.rs` 只有 beautify 面（`rust/crates/lingxi-service/src/lib.rs:8254`） |
| 模型可见性（office 工具） | 注册为 `Availability::Deferred`（`office_tools.rs:1229`）；模型声明快照只声明 `Available` 且 resident 的目标（`model_exchange.rs:107-116`、`toolcatalog.rs:1812-1827`）；本调查未发现 office 工具的 resident/激活路径（对照 `browser_tools.rs:204-207` 显式 `add_model_resident`）。A09 证据用确定性 stub 按 wire name（`local_name`=`html-to-pdf`）直接派发，只证明工具链可执行；真实模型可发现性登记 OQ-16。 |
| job 字段/单位 | `office_pdf.rs:56-108`（pageSize/margins 英寸/scale/printBackground/preferCSSPageSize）、`:363-374`（timeout 1000..300000 默认 60000、viewport 320..4096、settle 0..30000、allowJavaScript 默认 false） |
| 资源边界 | 资产必须位于 base 目录、去重、MIME 白名单、单件/总量预算（`pdf.rs:230-273`）；页面请求全部经 Fetch 拦截，非冻结 URL 一律 DENY（`pdf.rs:302-490`）；注入 CSP `default-src 'none'; connect-src 'none'; worker/frame/object 'none'; sandbox`（`pdf.rs:213-228`）；**WS 边界比 R01 spike 更强**（生产 `connect-src 'none'`） |
| 字体三族 | 白名单 EB Garamond / Noto Serif SC / JetBrains Mono，缺一即显式失败（`office_pdf.rs:251-303`；字体来源 `OfficeAssets.themes_dir`，`office_tools.rs:29-34`，路径装配 `host_auxiliary.rs:28-77`） |
| 超时/失败语义 | `PDF_TIMEOUT` 显式错误码（`pdf.rs:333-335`），无产物；清理未确认时 `effect_unknown` 且不改成确认失败（`host.rs:399-402,506-514`）；生产以错误码表达 R01 spike 的 exit=2 超时语义（ADR 要求等价，见 `docs/rust-tauri/R01/ADR-003-document-renderer.md:87-88`） |
| 旧链对照 | R01 spike 报告已冻结 A09 数据：74 页、140/140 表格行、三族字体、文本相似度 0.99910、视觉网格 max 7.16（`docs/rust-tauri/R01/PDF_SPIKE_REPORT.md:54-70`）；样本与工具在 `tests/migration/r01-t05/`（s1..s6、fonts、pdf_probe.swift、compare_outputs.py、old_chain_harness.cjs） |
| R07 生产等价测试 | `rust/crates/lingxi-adapters/tests/r07_pdf_chromium.rs:353-455`（S1 文本/表格/字体/图片/分页/JS 开关/纸张英寸）、`:502-539`（iframe/img/file:///worker/WS/fetch/beacon/meta-refresh 到 loopback canary 零命中）、`:541-578`（总超时与取消，清理核验）、`:631-695`（打印不切换/清空已登录工作区） |
| 现有测试对旧基线的依赖 | `r07_pdf_chromium.rs:391-397` 读取 `artifacts/rust-tauri/R01/T05/a09/new/probe/probe.json`；**该目录在当前工作区不存在**（`artifacts/rust-tauri/` 仅 R02/R07/R08），R09 A09 需重生成基线，见 3.3 |
| 已知差异 | `preferCSSPageSize=false` 角例 KNOWN-DIFF（`docs/rust-tauri/R01/PDF_SPIKE_REPORT.md:47`）；R09 需明确处置（接受 KNOWN-DIFF 或注入 `@page{size:auto}`）；旧链 Ext-B 字形缺失、旧链无资源拦截（同报告 100-107） |

### 1.4 媒体 / 富文本 / Office 预览（现状）

| 预览类型 | 渲染链路（file:line） | 边界/测试 |
|---|---|---|
| 图片/视频/SVG | `openFilePreview` 按类型分流到 MediaViewer（`desktop/src/react/utils/file-preview.ts:105-123`）；FileRef→URL：优先服务资源 content link（带 loopback token query），否则 `platform.getFileUrl`（`desktop/src/react/components/shared/MediaViewer/media-source.ts:20-33`、`desktop/src/react/services/resource-url.ts:25-73`）；`getFileUrl` 在 Electron preload 是纯 `file://` 字符串（`desktop/preload.cjs:140`、`desktop/src/shared/path-to-file-url.cjs:24-41`）；SVG 渲染为 `<img>`（`desktop/src/react/components/shared/MediaViewer/ImageStage.tsx:26-56`） | token 进 URL 仅限 local loopback（`desktop/src/react/services/server-connection.ts:179-181,766-777`）；媒体单测 `desktop/src/react/__tests__/components/shared/MediaViewer/MediaViewer.test.tsx`、`media-source.test.ts`（存在，未运行） |
| Markdown | `renderMarkdownPreview` + sanitizer（`desktop/src/react/utils/markdown.ts:1110-1119`、`desktop/src/react/utils/markdown-html-sanitizer`）；相对图片经 `getFileUrl` 解析（`desktop/src/react/utils/markdown.ts:244-258,1005-1012`）；主渲染在 `PreviewRenderer.tsx:170-600` | 单测 `markdown.test.ts`、`PreviewRenderer.html.test.tsx`（存在，未运行） |
| HTML | 渲染侧 POST `/api/preview/html` 换 token 化 URL（`desktop/src/react/components/preview/PreviewRenderer.tsx:115-148`），iframe `sandbox="allow-scripts"` + `referrerPolicy=no-referrer`（`:159-166`）；服务侧 10min TTL、32B token、2MB 上限、CSP、素材域 realpath 收容、跨源 CORP（`rust/crates/lingxi-service/src/preview.rs:1-30,377-433,807-940`） | 单测 `rust/crates/lingxi-service/tests/r06_t05_preview.rs:73-313,477-621`（创建/token/CSP/素材域/50MB/HTTP 面）；渲染侧 `PreviewRenderer.html.test.tsx` |
| DOCX / XLSX | 服务侧 `GET /lingxi/v1/fs/docx-html\|xlsx-html?path=`（`rust/crates/lingxi-service/src/lib.rs:8788-8791`、`fsread.rs:198-252`）；渲染侧 `readRustPreview` 按类型选路由（`desktop/src/react/utils/preview-file-content.ts:12-50`）；展示 `dangerouslySetInnerHTML`（`PreviewRenderer.tsx:755-767`） | `rust/crates/lingxi-service/tests/r07_office`（文档 worker 真实读 docx/xlsx/分页 PDF，见 `docs/rust-tauri/R07/T05_REPORT.md:42-45`） |
| PDF | 渲染侧 `readFileBase64` 取 base64→Blob→iframe（`desktop/src/react/components/preview/PreviewRenderer.tsx:657-689`；`preview-file-content.ts:20-50`；`BINARY_PREVIEW_TYPES` 含 pdf） | `PreviewRenderer.html.test.tsx:184`（source URL PDF 预览，存在，未运行）；**Tauri/WKWebView 内 Blob PDF 渲染未验证（OQ-07）** |
| 文件预览刷新/只读窗口 | 文件 watch + 版本冲突回写（Electron：`desktop/main.cjs:6394-6471`；Tauri 资源宿主同名命令：`desktop/src-tauri/src/lib.rs:230-289`）；派生只读 viewer（Electron `main.cjs:5866-5919`；Tauri `lib.rs:382-437`，只读窗口写操作被 `main_only` 拒绝 `lib.rs:44-50`、读取限于自身文件 `lib.rs:52-66`） | 单测 `preview-file-refresh.test.ts`、`preview-document-refresh.test.ts`、`desktop/src-tauri/tests/desktop_host.rs`（存在，未运行） |
| Office 预览/PDF 产物会话文件面 | 服务端 50MiB PDF 产物登记、`%PDF` 校验（`office_tools.rs:685-747`；`sessionfiles.rs:920` 注释）、会话文件内容 HTTP | R07 T05 回执（`docs/rust-tauri/R07/T05_REPORT.md:42-45`） |

### 1.5 现有宿主接线

**Electron（当前生产壳）**

- Rust-node 浏览器 viewer：`open-browser-viewer` 带 connectionKey 时创建独立 `BrowserWindow`（`desktop/main.cjs:5753-5766`），加载 `browser-viewer` 页面（多入口 `vite.config.ts:353`），`BrowserViewerApp` 以 `?mode=native` 走 `RustBrowserViewer`（`desktop/src/react/browser-viewer/BrowserViewerApp.tsx:46-62`）；viewer **不承载网页**，网页在服务控制的 Chromium 窗口。
- 连接注册：只有主窗口可以 `set-browser-connection`，viewer 只拿 `get-browser-viewer-context`（`desktop/main.cjs:5737-5744`）；所有服务命令由 main 的 `rustBrowserControl.call` 发 HTTP（`desktop/main.cjs:5745-5751`、`desktop/src/shared/rust-browser-control.cjs:35-58`），凭据不进 viewer。
- 旧 Node 连接仍保留嵌入式 `WebContentsView` viewer（`desktop/main.cjs:3435-3521`；tab/导航/急停旧 IPC `:5800-5864`），这是 Electron 退役对象。
- viewer 控制面板轮询 `/browser/workspaces` 每 2s，操作后立即刷新（`desktop/src/react/browser-viewer/RustBrowserViewer.tsx:17-46`）；按钮禁用逻辑/空态/错误展示 `:48-77`；打开 viewer 不创建标签（`desktop/src/react/services/rust-browser.ts:54-75`，无 tab 命令）。
- 主界面浮动卡片通过 app-events `browser_status` 更新（`desktop/src/react/services/rust-automation.ts:209-214,252`、`ws-message-handler.ts:858-861`），跨连接/跨节点防串（`desktop/src/react/stores/browser-slice.ts:46-75`）。

**Tauri（现有资源宿主，R06 子集）**

- 独立 manifest/锁树、独立 `[workspace]`、`bundle.active=false`、`frontendDist=../dist-tauri-resources`、单 main 窗口、最小 CSP（`desktop/src-tauri/Cargo.toml:7-33`、`desktop/src-tauri/tauri.conf.json:6-14`）。
- 已有命令：文件读写/快照/二进制、watch、资源订阅（走 `instance.json`+`local-token.json` 的宿主内 token：`desktop/src-tauri/src/resource_bridge.rs:10-50`）、`spawn_viewer` 只读窗口、技能 viewer、对话框（`desktop/src-tauri/src/lib.rs:530-599`）。
- capabilities 按窗口 label 分权（`desktop/src-tauri/capabilities/main.json`、`viewer.json`、`skill-viewer.json`）；DNS/HTTP 直连权限未开放给前端。
- E2E 路径：`tauri-plugin-wdio-webdriver` 1.4.0 可选依赖 + `e2e-test` feature（`desktop/src-tauri/Cargo.toml:27,32-33`），测试通过 `TAURI_WEBDRIVER_PORT` 驱动真实 Tauri 进程（`desktop/src-tauri/tests/desktop_host.rs:137-190`）；R01 spike 结论：macOS 只能用嵌入式 wdio（`docs/rust-tauri/R01/DEPENDENCY_DECISIONS.md:243`；`docs/rust-tauri/R01/TAURI_SPIKE_REPORT.md:79-82`）。

### 1.6 R09 账：A09/A10 与 19 叶

- 验收场景（`docs/rust-tauri/R00/ACCEPTANCE_MAP.json:7235-7324`）：
  - **R09-A09**（`kind=base`, REQUIRED）：`given=测试安装未包含Electron`；`when=执行浏览器用户接管及Office PDF输出`；`then=功能可用且产物达标，不暗中调用旧安装`；`evidence=无Electron进程与文件探针`（`:7235-7280`）。
  - **R09-A10**（`kind=base`, REQUIRED）：`given=SVG/HTML预览含脚本与IPC尝试`；`when=在预览区打开`；`then=不能获得token或原生命令权限；正常预览保留`；`evidence=不可信内容安全测试`（`:7280-7324`）。
- 19 个执行叶（`FEATURE_STAGE_ACCEPTANCE.json` 中 `execution_task_ids` 含 `R09-T05`，分家：浏览器/设置 15 + web 工具 3 + 文件预览 1）：
  1. `R00-T02-LA-A67D342381C0`（BROWSER-TABS）：标签集合/活动标签在 viewer 同步、服务端快照跟随；无效 tabId 不执行、关闭失败不得显示已关闭。
  2. `R00-T02-LA-A09B78985A14`（BROWSER-EMERGENCY-STOP）：急停关闭并撤权至下一条消息；无会话旧实例仅本地销毁；关闭失败可见，不得只隐藏窗口。
  3. `R00-T02-LA-8A45B19A18B4`（BROWSER-NAVIGATION）：后退/前进/刷新/地址更新；无活动 view 不执行、非法地址不加载。
  4. `R00-T02-LA-13049521EA0C`（CLOSE-BROWSER-VIEWER）：viewer 关闭与会话浏览器清理分离；关闭窗口不误记为急停。
  5. `R00-T02-LA-6E13E5DF05EB`（OPEN-BROWSER-VIEWER）：显示指定会话标签/允许 URL 新开标签/空态；非法 URL 不加载、打开窗口不建空标签。
  6. `R00-T02-LA-7D212CBD9EEF`（搜索供应商验证）：成功才保存 provider/key，失败不保存。
  7. `R00-T02-LA-1809C8E13EA3`（读取浏览器偏好）：返回设置+运行状态，不写。
  8. `R00-T02-LA-03A9E92909C6`（清除站点资料）：清除成功/失败真实回执。
  9. `R00-T02-LA-ED5FD5B469B8`（更新浏览器偏好）：保存并返回新状态；应用失败 400 时读回真实状态，不假设回滚。
  10. `R00-T02-LA-367052089D11`（浏览器会话状态）：active/resumable/unavailable。
  11. `R00-T02-LA-593534A4F2AB`（已运行会话目录）：BrowserManager 当前会话列表。
  12. `R00-T02-LA-178C8CBA29C2`（关闭会话浏览器）：关闭并广播状态。
  13. `R00-T02-LA-F1A5597E3E78`（关闭并撤权）：关闭且撤销该会话授权。
  14. `R00-T02-LA-DCD85A04A7E9`（打开会话浏览器）：尝试恢复并让 viewer 显示该会话；失败可空态。
  15. `R00-T02-LA-A30E4BDF2FA2`（browser 工具）：会话隔离动作/快照/图片/状态/记录；过期 ref、跨会话泄漏、显式失败。
  16. `R00-T02-LA-FD7A5A76B8EA`（web_fetch）：正文/最终 URL/摘要；阻断页不当正常内容；失败显式。
  17. `R00-T02-LA-D1B56A275C3A`（web_search）：标题/链接/摘要/供应商；未配置不假成功。
  18. `R00-T02-LA-4173A08A51CC`（FILE-PREVIEW）：文本/Markdown/Office 预览；资源变更多读；可派生只读窗口；二进制/过大信息卡；失败错误；独立 viewer 不回写。
  19. `R00-T02-LA-8BD3BCC6D725`（设置页浏览器 UI 动作）：逐动作真实状态；失败不伪装空数据。
- R07 已对这 19 叶给出 LOCAL_VERIFIED（`docs/rust-tauri/R07/T04_REPORT.md:20-38,40-57`、`docs/rust-tauri/R07/T05_REPORT.md:42-46`），但 R07 结论绑定 Electron 壳 + R07 候选，**不能替代 R09 的 Tauri 安装证据**。

---

## 2. Tauri 接线设计（B）

### 2.1 核心决策：网页窗口保持独立，Tauri 只做控制面板

**设计决策 D1：受控 Chromium 的网页窗口仍是操作系统级独立顶层窗口；Tauri 不尝试嵌入/重parent 它。**

依据：

- ADR-002 明确“不引入 wry/Tauri webview 作为浏览器能力承载（Tauri 仍可用于壳/窗口）”（`docs/rust-tauri/R01/ADR-002-browser-host.md:32`），并记录 wry 0.57 的 API 缺口（无截图、下载回调 macOS 路径恒空、WebContext 仅进程级 data dir、无程序化 file input）（`docs/rust-tauri/R01/BROWSER_SPIKE_REPORT.md:56-63`）。
- 生产实现按“每会话独立 Chromium 进程 + 原生窗口”构建（`rust/crates/lingxi-adapters/src/browser/launcher.rs:119-220`、`host.rs:847-869`）；`--no-startup-window` + 首个 target `newWindow:true` 创建真实窗口（`launcher.rs:144`、`ops.rs:94-102`）。
- Electron 生产壳在 Rust-node 模式下已经采用“viewer 只是状态/控制面板，网页在同服务 Chromium 窗口”的结构（`desktop/src/react/browser-viewer/RustBrowserViewer.tsx:7,50-53`、`desktop/main.cjs:5753-5766`），Tauri 只需 1:1 复刻该结构。

**备选与风险对比**：

| 方案 | 可行性 | 主要风险 | 结论 |
|---|---|---|---|
| A. 受控 Chromium 自有窗口 + Tauri 控制面板（本设计） | 已验证于 R07/R01（真实 Chrome headful + CDP；A07 原生接管已验） | 两个 app/window 的焦点衔接；窗口管理策略由 Chrome 决定；无法把网页裁剪进 Tauri 布局 | **采用** |
| B. 把 Chromium 窗口重parent 成 Tauri 子窗口 | macOS 需 `addChildWindow`/私有 view 提取；Windows `SetParent` 破坏 Chrome 输入/弹窗；Linux X11 脆弱、Wayland 不可行 | 私有 API、Chrome 更新即坏、IME/焦点/拖拽/弹窗/下载 UI 全面异常；崩溃域耦合；无跨平台一致路径 | 拒绝（无实测动机；ADR 已排除） |
| C. 用 Tauri webview 直接当浏览器 | wry 能力缺口（截图/隔离 profile/文件上传/代理不可负验）；本地 webview 不能提供 Chrome 的完整站点兼容与用户接管 | 与 ADR-002 冲突；等于重写浏览器；不可信内容与宿主同进程域 | 拒绝（R01 已取证） |
| D. 外部系统浏览器（openExternal） | 简单 | 不满足会话/nav/tab/接管/截图等价语义；A09/A10 不覆盖 | 拒绝（ADR-002“明确排除”） |

### 2.2 Tauri viewer 窗口与能力边界

**D2：Tauri 侧新增 `browser-viewer` 窗口（label `browser-viewer`），内容复用现有 React `BrowserViewerApp`（native 模式），能力按窗口 label 最小授权。**

- 窗口创建：Tauri host 收到主窗口 `open_browser_viewer` 命令后创建 `WebviewWindow`（可参照现有 `spawn_viewer`/技能窗口模板：`desktop/src-tauri/src/lib.rs:382-422,439-475`），url 使用 `index.html?browserViewer=1&mode=native` 或独立 `browser-viewer.html` 入口；Electron 侧的入口已存在（`desktop/src/browser-viewer.html`、`vite.config.ts:353`），Tauri 全量前端构建（R09-T01）需产出同一入口。
- **窗口语义**：
  - 打开 viewer 仅创建窗口/读取状态，**不创建空标签、不启动浏览器**（Rust 路径现有语义：`desktop/src/react/services/rust-browser.ts:54-75`，打开时可选 `new_tab`/`open` 之后再开窗；leaf 6E13E5 断言）。
  - 关闭 viewer 窗口只销毁窗口并清 viewer 上下文（Electron 语义 `desktop/main.cjs:5847-5850` 与 `:5764` closed 清理），**不得发 stop/revoke**（leaf 130495 第二断言）。
  - viewer 窗口能力：仅 `get_browser_viewer_context`、`rust_browser_request`、`close_browser_viewer` + 主题/窗口控制；**不给**文件写、系统命令、shell/HTTP 直连权限（对照现有 capabilities 分权方式：`desktop/src-tauri/capabilities/viewer.json`）。
  - `withGlobalTauri` 仅暴露到 app 窗口；viewer 是 app 窗口（可信），网页永远不在 app 窗口（见 2.4 / 2.9）。
- **连接注册语义（安全关键）**：
  - `set_browser_connection` 只接受主窗口调用（Electron `desktop/main.cjs:5738`）；Tauri 侧校验 `window.label()=="main"`。
  - 宿主保存 `{connectionId, baseUrl, token, serverNodeId, serverNodeKind, ...}`，生成随机 `connectionKey`（Electron `desktop/src/shared/rust-browser-control.cjs:12-34`）；viewer 只拿 `connectionKey + sessionPath + executionNode + serviceLabel/serviceUrl`（同文件 `:31-34`）。
  - `rust_browser_request(target, action, options)` 只接受主窗口或当前 `browser-viewer` 窗口；viewer 的 `sessionPath` 必须等于本窗口 target（Electron `desktop/main.cjs:5745-5751`）；HTTP 由宿主 reqwest 发出，附 `Authorization: Bearer`，30s 超时、redirect error（`rust-browser-control.cjs:35-58`）。
  - 服务切换/重连：连接身份变化清空 viewer 上下文并销毁旧窗口（Electron `setConnection` 回调 `desktop/main.cjs:514-519`；renderer 侧迟到丢弃 `desktop/src/react/services/rust-browser.ts:42-53,54-75`）。Tauri 沿用同一套规则。
- **命令清单（Tauri 命令名，实施期可微调但语义固定）**：
  - `open_browser_viewer(target) -> {opened:true}`（main only；先 `context(target)` 校验 key/session；销毁旧 viewer、记录 target、创建窗口）。
  - `get_browser_viewer_context() -> RustBrowserViewerContext | null`（仅 browser-viewer 窗口；否则 null）。
  - `rust_browser_request(target, action, options) -> JSON`（main 或当前 viewer；动作白名单 back/forward/reload/new_tab/switch_tab/close_tab/show/takeover/release/workspaces/stop/open；参数校验同 `rust-browser-control.cjs:37-39`）。
  - `close_browser_viewer()`（仅当前 viewer 可关自己；main 时等价于关闭 viewer 窗口）。
  - `set_browser_connection(connection | null)`（main only；返回 connectionKey）。
  - 失效路径：`browser_connection_changed` / `browser_sender_denied` / `browser_session_changed` / `native_session_required`（沿用现有错误词表，避免 renderer 分支重写）。

### 2.3 用户接管 / 焦点 / 快捷键 / 输入法

- **接管动作**：viewer「接管」→ 宿主 `rust_browser_request(target,'takeover')` → HTTP `POST /lingxi/v1/browser/command` `{sessionPath, action:"takeover"}`（`rust-browser-control.cjs:44-49`、`browser_routes.rs:181-216`）→ `BrowserService::command(human=true)`（`browser.rs:621-747`）→ host `ops::execute` 的 `A::Show|A::Takeover` 分支：`Target.activateTarget` + `Browser.getWindowForTarget` + `Browser.setWindowBounds(windowState:"normal")` + `live.takeover=true` + 清 snapshot 引用（`ops.rs:816-830`）。同时服务端把 `user_takeover` 反映到 workspace 状态（`host.rs:189-221`），viewer 轮询后显示「你正在接管」（`RustBrowserViewer.tsx:56`）。
- **接管期间 agent 被服务端硬阻断**：修改页面/滚动/切 tab/show/suspend 返回 `browser_user_takeover`（`ops.rs:590-605`）；审批 scope 也携带 `userTakeover`（`browser_tools.rs:99-121`）。
- **交还**：`release` 清除标志并清空快照引用（`ops.rs:832-843`），agent 可继续；viewer 刷新状态。
- **焦点**：网页窗口是 Chrome 自己的原生窗口，**焦点/输入法/快捷键全部由 Chrome 处理**（这是方案 A 的核心收益）。当前实现只把窗口从最小化恢复为 normal（`ops.rs:521-537`），**不保证把 Chrome 应用带到前台**。现实交互：用户点「显示/接管」后若窗口在其它窗口后面，仍需点击窗口（R07 A07 补验即由 Cua 在真实 Chrome 窗口上 click/type：`docs/rust-tauri/R07/T04_REPORT.md:108-117`）。
- **设计处置**：
  - R09-T05 接受“由用户点击目标窗口完成前台确认”，viewer 文案明确“网页在该服务的受控 Chromium 窗口中”（已有 `RustBrowserViewer.tsx:53`）。
  - 可选增强（不阻塞本任务，见 OQ-08）：宿主在 takeover 后按服务提供的可执行路径做平台级激活（macOS `open -a`/`NSRunningApplication.activate`、Windows `SetForegroundWindow`）；必须限定为服务已确认的受控可执行，且不能成为新的任意命令面。
- **快捷键**：Chrome 窗口内快捷键是 Chrome 语义（Cmd/Ctrl+W 关标签、Cmd/Ctrl+T 新标签等）。用户关闭窗口/tab 后，真实 target 消失由 `refresh` 重新枚举 `Target.getTargets` 并剔除（`rust/crates/lingxi-adapters/src/browser/runtime.rs:427-602`），`CloseTab` 显式等待真实 `Target.targetDestroyed`（`ops.rs:863-880`）；没有页面时显式 `browser_closed`/`browser_empty_workspace`（`ops.rs:631`、`host.rs:707-747`），viewer 展示空态/停止态，不伪造运行。
- **输入法**：不做任何宿主侧 IME 桥；用户直接使用 Chrome IME。agent 输入仍走 CDP `Input.insertText`（中文路径已测，`docs/rust-tauri/R01/BROWSER_SPIKE_REPORT.md:71`）。

### 2.4 tab 同步与 viewer/标签页状态

- **唯一事实源 = 服务端 `BrowserWorkspace`**（`rust/crates/lingxi-protocol/src/browser.rs:205-233`：tabs/activeTabId/url/title/running/resumable/suspended/userTakeover/revoked/visibleCapable）。
- viewer 每 2s 轮询 `GET /lingxi/v1/browser/workspaces` 并按 sessionPath 取自己会话（`RustBrowserViewer.tsx:17-36`）；每次控制动作后立即刷新（`:37-46`）；防迟到/卸载（`:17-23,25-36`）。Tauri 下同实现（数据经宿主命令返回）。
- 主窗口浮动卡片通过 app-events `browser_status` 更新并按 connection/executionNode 过滤（`rust-automation.ts:209-214,252`、`browser-slice.ts:46-75`），不跨连接串状态。
- **活动 tab 同步**：服务端 `refresh` 在每次命令前读页面 `document.visibilityState==='visible' && document.hasFocus()` 决定 activeTab（`runtime.rs:596-602`）——用户直接在 Chrome 里点击/切换标签后，下一次 agent 命令/快照会把活动标签校正为真实焦点页；`Target.targetDestroyed` 与 `Target.targetInfoChanged` 事件同步 tabs/url/title（`runtime.rs` 事件循环）。viewer 不需要也不应该自己推断标签。
- **无效 tabId**：协议校验拒绝空/空白/控制字符/`*?`（`rust/crates/lingxi-protocol/src/browser.rs:130-142`）；host `attach` 对非本会话/已关闭 tab 返回 `browser_stale_tab`（`runtime.rs:614-630`）；服务返回错误，viewer 显示错误且不改「已关闭」（`RustBrowserViewer.tsx:41-45,54`）。
- **关闭失败不显示已关闭**：`A::CloseTab` 必须等到真实 `Target.targetDestroyed` 才返回成功（`ops.rs:863-880`）；失败返回错误、状态不变。
- **打开 viewer 的 URL 语义**：`openRustBrowserViewer` 在开窗之前按需 `new_tab`（url 已校验 http/https，`rust-browser.ts:54-70`）或 `open`（恢复）；开窗本身不建 tab。非法 URL 在 renderer（`rust-browser.ts:55`）、工具面（`browser_tools.rs:48-60`）、host 侧（`ops.rs:78-84`、导航 `:149-155`）三处拒绝。

### 2.5 急停撤权：进程/授权路径

**D3：急停是服务端事务，不是 viewer 窗口操作；Tauri 宿主只转发一次 HTTP 并如实回显结果。**

路径：

1. viewer「急停」→ `rust_browser_request(target,'stop')` → `POST /lingxi/v1/browser/close-session {sessionPath, revoke:true}`（`rust-browser-control.cjs:44-49`）。
2. 路由 → `BrowserService::close(row, revoke=true, operation)`（`browser_routes.rs:150-180` → `browser.rs:905-993`）：
   - command 收据 claim（避免重复执行，`:922-943`）；
   - 取消该会话所有 in-flight 浏览器命令（`:944-948`）；
   - `store::revoke_human_intent` 递增 epoch 并写撤销意图（`:951-957`；存储 `storage/browser.rs:132-183`）；
   - `port.revoke(scope)`：host 侧 `cancel_pdf` + 清 takeover/标记 revoked + 等 busy 归零后销毁进程组（`host.rs:1261-1346`），失败返回 `browser_cleanup_unknown`；
   - `record_revoke_state`：确认则清空 tabs/url/running；未确认则 `unavailable_reason="BROWSER_CLEANUP_UNCONFIRMED…"`（`browser.rs:859-904`），**不把失败写成成功**；
   - 发布 `browser_status` revoked（`:988-991`）。
3. viewer 显示 `revoked` 与不可用原因（`RustBrowserViewer.tsx:56-57`），按钮禁用；急停后 agent 工具调用命中 revoked → `BROWSER_REVOKED`（`browser.rs:638-643,565-568`）。
4. **解除条件**：新用户提交创建真实 user root Run 时 `release_for_user_run` 才放行（`runs.rs:2836-2852`），旧 run/旧审批/旧插话不能解除（测试 `r07_browser_tools.rs:787-848`）。
5. **无会话旧实例**：Tauri 只连接 lingxi-service，不存在 Electron 旧嵌入式浏览器；若 `sessionPath` 缺失/无效，viewer 不显示急停按钮（`RustBrowserViewer.tsx:66` 仅在 workspace 存在时渲染）→ 满足 leaf“无会话旧实例仅本地销毁”的删除面（该分支在 Tauri 下不可达，实施时以显式无状态空态替代，不留旧 Electron 本地销毁路径）。
6. **关闭失败可见**：返回错误 → viewer `role=alert` 展示（`:54`），不隐藏窗口代替错误（leaf A09B78 第二断言）。

### 2.6 会话隔离

- 服务按 `(execution_node, owner_subject, session_id)` 生成 identity，每会话一个 Chromium 进程 + 独立 `user-data-dir`/downloads 目录（`host.rs:158-170,847-869`）；cookie 接受策略按 profile 配置（`host.rs:849-853`）；站点数据清理 `clear_site_data`（`browser.rs:484-492` + host 实现）。
- 同会话并发命令串行（busy CAS，`host.rs:681-687`）；跨会话互不影响；tool scope 校验 session 所有权/execution node/审批 epoch（`browser.rs:538-620`）。
- Tauri viewer 只显示当前会话的 workspace；主窗口卡片跨会话只看真实 `browser_status` 并校验 connection scope（`browser-slice.ts:46-75`）。
- A08 类“双会话互不串”的既有证据：`r07_browser_chromium.rs:372-548`（独立持久 profile）、R01 isolation 阶段 6 步（`docs/rust-tauri/R01/BROWSER_SPIKE_REPORT.md:72`）。

### 2.7 PDF 在 Tauri 下的发起与 renderer 进程所有权

- **发起面 = 服务 API（模型工具），不是 Tauri 宿主命令**：`office_html-to-pdf` 经真实 Run/网关/审批进入 `OfficeApi::pdf`（`office_tools.rs:625-747`、`:1188`），无 HTTP 发起路由（`lib.rs:8254` 仅 office_routes beautify）。Tauri 侧不新增 PDF 命令；UI 只做“文件卡/预览/在 Finder 中显示”等既有能力。
- **renderer 进程所有权**：lingxi-service 进程是 Chromium 父进程（`host.rs:463-483` 一次性进程、`Process::launch`），Tauri app 不是 owner，不能 kill/注入；Tauri 退出时由服务退出流程清理（`BrowserService::shutdown` `browser.rs:1262-1267` + host `close_all` `host.rs:1087-1174`）。
- **字体/资产供给**：服务需要 `OfficeAssets.themes_dir`（三族字体与 CSS）与（可选）office worker/node；发行版必须把 `desktop/dist-renderer/themes`（或等价资源）提供给服务，装配点 `host_auxiliary.rs:28-77`（env `LINGXI_OFFICE_NODE/WORKER/THEMES/COVERS` 或开发态沿 bridge worker 根推导）。Tauri 打包时的资源路径映射属 R09-T02/T05 联合接线。
- **超时/失败契约**：生产以 `PDF_TIMEOUT` 显式错误 + 零产物 + 清理确认表达 R01 spike 的“超时 exit=2”语义（`pdf.rs:333-335`、`host.rs:506-514`）；R09 证据断言错误码/无产物/`pdf_scopes()` 为空/无残留进程，不要求进程 exit code。
- **已知差异处置**：`preferCSSPageSize=false` + CSS `@page size` 的 KNOWN-DIFF（`docs/rust-tauri/R01/PDF_SPIKE_REPORT.md:47`）。R09-T05 建议**注入 `@page{size:auto}` 覆盖样式以对齐旧链**（改动仅影响显式 false 且 CSS 声明 size 的角例），若实施风险大则接受 KNOWN-DIFF 并在 T05 报告显式登记；默认 `preferCSSPageSize=true`（office_pdf.rs:370）不受影响。
- **A09 质量目标**：与 R01 A09 同判据——74 页、140/140 表格行、三族字体、无空白页、文本相似度 ≥0.99、视觉网格 max|Δ|≤20（`tests/migration/r01-t05/compare_outputs.py:97-160`；R01 报告 `docs/rust-tauri/R01/PDF_SPIKE_REPORT.md:54-70`）。

### 2.8 媒体/富文本/文件预览在 Tauri 下的权限与边界

**D4：会话/服务文件一律走服务 HTTP + token；宿主本地文件（desk 路径、markdown 相对图片）走“宿主中介的自定义协议”，绝不把 `file://` 直接暴露给 webview。**

- 会话文件/生成媒体：`FileRef.resource.links.content` 已存在（`desktop/src/react/services/resource-url.ts:25-73`），URL 为 loopback HTTP + token query（仅 `kind==='local' && credentialKind==='loopback_token'` 才允许 query token，`desktop/src/react/services/server-connection.ts:179-181,766-777`）。Tauri webview 子资源（`<img>/<video>/iframe`）不受服务 Origin allowlist 影响（不发送 Origin），CSP 需允许 loopback。
- HTML 预览：保持跨源 iframe + 服务 token（预览 token 是 10min 单文档令牌，不是服务 bearer token，`preview.rs:807-846`）。**CSP `frame-ancestors` 需扩展**：在服务 `build_html_preview_csp` 的 `frame-ancestors` 中加入 Tauri origin 精确值（建议 `tauri://localhost`、`http://tauri.localhost`；实施期以实际 WebView origin 实测为准），否则 macOS/Linux（及可能的 Windows）预览被 CSP 拒绝。该改动小且精确，需配负向测试（任意异源仍拒绝）。**注意**：调查期间工作区已有并发未提交修改在做这两处服务端接缝（见 OQ-06 注），但 T05 仍必须以实际候选重新取证，不能把未提交改动当作已通过。
- **Tauri app CSP（T06 收口）**：全量壳必须至少允许
  - `frame-src 'self' blob: http://127.0.0.1:* http://localhost:*`（HTML 预览 iframe）
  - `img-src/media-src 'self' data: blob: http://127.0.0.1:* http://localhost:*`（会话媒体/预览素材）
  - `connect-src 'self' ipc: http://ipc.localhost`（现有）+ 若选择 renderer 直连服务则再加 loopback origin；若选宿主代理则不需要
  - 参考现状最小 CSP：`desktop/src-tauri/tauri.conf.json:11`；R09-T06 是“Tauri 权限、CSP 与自定义命令收口”，T05 只提需求与清单。
- **本地文件（非会话）媒体**：Tauri 不能使用 Electron 的 `file://` 直读（`desktop/preload.cjs:140`、`path-to-file-url.cjs`）。设计采用 **`lingxi-local-file://` 自定义协议**：main/viewer 向宿主登记“本窗口可读的绝对路径 + 用途（媒体/图片）+ 一次性 token”，宿主校验 `absolute + 非 symlink + 在批准根（workspace/approved dirs）内 + 未命中受保护路径（recovery/.lingxi-recovery-/敏感目录）` 后流式返回正确 MIME；token 不可猜测、绑定窗口 label 与路径、短 TTL、目录穿越拒绝。**A10 负向**：预览内容无法伪造 token 读其他文件；未登记路径 403。
  - 替代方案（成本更高）：所有本地文件都经宿主命令转 base64/data URL（大文件/视频不可行），或仅支持服务资源（desk 媒体功能退化，不满足 leaf 4173A0）。
- **PDF 预览**：现有渲染侧把 base64 转 Blob URL 后 iframe（`PreviewRenderer.tsx:657-689`）。WKWebView 对 blob PDF 的渲染行为未验证（OQ-07）；先做实测：若 WKWebView 拒绝，候选回退：① 宿主命令落临时只读文件 + 自定义协议（`application/pdf`）打开；② 独立 Tauri viewer 窗口加载自定义协议 PDF；③ 引入 PDF.js（新依赖，需 R09-T01 依赖决策，倾向不引入）。**不得**为了预览引入 Electron/Chrome 依赖。
- **Office DOCX/XLSX**：内容由服务解析成 HTML 字符串（`fsread.rs:198-252`），在 app webview 内 `dangerouslySetInnerHTML` 渲染（`PreviewRenderer.tsx:755-767`）。该 HTML 来自受控解析器（非直接用户 HTML），但 A10 测试应加入“恶意 docx/xlsx 生成 HTML 是否可注入脚本”的负向用例（若当前解析器未转义则记录并修复/隔离；不在 T05 调查中擅自改代码）。
- **下载/打开外部**：媒体/文件内容下载在服务端登记为 SessionFile（`browser.rs:788-837`），预览走统一文件面。

### 2.9 Chromium 分发/查找设计（实现期）

**D5：R09 实现按“部署提供 Chromium”接线，缺失显式失败；打包归属 R10，但 R09 必须冻结方案与证据。**

- 现状发现顺序：`LINGXI_CHROMIUM_PATH`（绝对、存在、可用）→ `<data_home>/bundled-bin/chromium/chrome` → macOS `/Applications/Google Chrome.app/...` → `/usr/bin/chromium*` → `/usr/bin/google-chrome`（`rust/crates/lingxi-service/src/browser.rs:1280-1298`）。缺失返回 `UnavailableBrowser` 并在 workspace 状态中显示明确原因（`:1319-1330`、`:1049-1052`）。
- 选项（按成本/风险排序，推荐 A+C 分阶段）：
  - **A. 测试/首发依赖系统 Chromium/Chrome**（当前代码路径）：零打包成本；必须冻结实测版本并在 A09 记录 executable 绝对路径 + sha256 + 版本字符串；版本漂移影响打印语义/字体渲染；用户机器可能没有 Chrome（Linux 尤其）→ 服务显式不可用。
  - **B. 随包 Chromium 运行时**（Tauri `bundle.resources` 放 `chromium/`，服务从资源目录启动）：满足“安装含渲染器”，许可为 Chromium BSD-3 + 第三方许可树（与 Electron 同构，不新增许可类别，`docs/rust-tauri/R01/ADR-003-document-renderer.md:54-59`）；成本约 400MB 压缩级、签名/公证必须签嵌套二进制、自动更新必须整包替换/差分；macOS 需注意 helper/framework 的嵌套签名与 Hardened Runtime 权限（JIT/unsigned executable memory）。
  - **C. 首启下载/按需安装**（下载到安装目录）：避免安装包体积，但引入网络、校验、磁盘写权限与“无网即不可用”，不符合 A09“安装即工作”的默认预期；仅作为 B 失败时的兜底。
  - **D. 复用 Tauri WebView 渲染 PDF**：不同打印引擎，ADR-003 已拒绝。
- **建议**：R09-T05 实现 + 本机证据使用“受控 Chromium 资源目录或系统 Chrome（冻结版本）”双路径，宿主/服务以 env/资源路径注入；R10 发布决策在 A/B 中定稿，B 是满足“移除 Electron 后仍工作”的最稳形态。R09 需把“随包体积/许可/签名/更新通道”登记为 OPEN 风险（OQ-01）。

### 2.10 Electron 移除的前置条件清单（T05 视角）

| # | 前置条件 | 现状/缺口 | 责任 |
|---|---|---|---|
| E1 | Tauri 主窗口承载全量 React UI，并实现 `PlatformApi` 的 browser/file/preview 成员（openBrowserViewer/setBrowserConnection/getBrowserViewerContext/rustBrowserRequest/closeBrowserViewer/readFile*/readDocxHtml/readXlsxHtml/getFileUrl(可选)/watchFile...） | Tauri 资源宿主只有 R06 子集（`desktop/src-tauri/src/lib.rs:530-599`）；无 Tauri host 实现文件（`desktop/src/react/host/` 只有 Electron/Web host） | T01/T05 |
| E2 | 服务 HTTP 从 Tauri webview 可达（origin/CORS/或宿主代理） | 现状服务 Origin allowlist 不含 Tauri origin（`transport.rs:224-267`）；renderer `fetch` 面广泛（`preview-file-content.ts:12-50` 等） | T02/T06（T05 提供浏览器/预览需求） |
| E3 | HTML 预览 iframe 可嵌入（CSP frame-ancestors 扩展） | 现状只允许 `'self' file: loopback`（`preview.rs:426-433`） | T05（服务小改）+T06（验收） |
| E4 | 本地文件媒体/markdown 图片在 Tauri 可用（自定义协议/宿主中介） | 现状仅 `file://`（Electron preload）；Tauri 无等价 | T05 |
| E5 | PDF 内联预览在 WKWebView/WebView2/WebKitGTK 实测/回退 | 未验证（OQ-07） | T05 |
| E6 | 浏览器 viewer 窗口 + 控制桥 + 急停/接管/标签同步 | Electron 已有 Rust-node 路径（`main.cjs:5737-5766`）；Tauri 未实现 | T05 |
| E7 | 受控 Chromium 可被服务发现/随包提供；PDF 字体资产可被服务读取 | 只有路径发现（`browser.rs:1280-1298`）；资产装配 `host_auxiliary.rs:28-77` 需安装布局 | T05 + R10 |
| E8 | 服务由 Tauri 壳启动/退出（T02），并传递 Chromium/Office 资源 env；退出时浏览器/PDF 进程清理 | T02 范围；T05 需消费其接口 | T02/T05 |
| E9 | A09 探针：安装树无 Electron 文件、进程树无 Electron、无旧 helper 调用 | 旧 Electron 生产仍存在；Tauri 测试安装需隔离目录 | T05/T08 |
| E10 | 旧 Node 嵌入式浏览器路径（`main.cjs:3435-3521,5800-5864`）不再被任何连接模式使用 | 现役 Electron 仍保留；Tauri 壳只连 lingxi-service | T01/T05 |
| E11 | A10 安全边界：预览不可达 Tauri IPC/服务 token/原生命令；app CSP 与 capabilities 最小化 | T06 收口；T05 提供测试 | T05/T06 |
| E12 | 跨平台证据（macOS x64/Windows/Linux） | 无真机（`R09/BLOCKED.md:8-10`） | T07/R10 |
| E13 | 验收基线可复现（旧链 PDF 基线当前缺失） | `artifacts/rust-tauri/R01/T05/...` 不在工作区（见 1.3） | T05 证据脚本 |

### 2.11 跨任务接口契约（写清“谁是决定项”）

| 接口 | T05 需要 | 对方任务 |
|---|---|---|
| 壳启动服务、传递 `LINGXI_CHROMIUM_PATH`/`LINGXI_OFFICE_*`/资源根、退出清理 | 服务就绪契约 + 环境变量注入 + 退出时浏览器/PDF 关停（`BrowserService::shutdown`） | R09-T02 |
| Tauri 前端构建产出 `browser-viewer` 入口与全量 app 入口；`PlatformApi` 注入方式 | 与 Electron 一致的多窗口入口（`vite.config.ts:353`） | R09-T01 |
| Tauri capabilities/CSP 最小化；自定义协议注册；`withGlobalTauri` 边界 | `lingxi-local-file://`、预览 iframe/媒体 CSP、viewer label 权限 | R09-T06 |
| 门禁命令注册（`verify-stage R09`）+ 证据合同 | A09/A10 脚本可被 xtask 执行并绑定源码指纹 | R09-T08 |
| 跨平台 E2E/平台证据 | Windows/Linux 的浏览器窗口、PDF 预览、焦点 | R09-T07 |

---

## 3. 证据计划（C）

> 红线：所有证据在隔离目录（`/private/tmp/lingxi-r09-t05-*`）生成；`LINGXI_HOME` 隔离；不替换日用安装、不迁移真实数据、不联网（除 loopback 替身）。所有命令为草案，实施期以脚本落盘并经 R09-T08 门禁执行。

### 3.0 环境与隔离

```
export ROOT=/private/tmp/lingxi-r09-t05
export LINGXI_HOME=$ROOT/home
export ART=$ROOT/artifacts            # 最终归档到 artifacts/rust-tauri/R09/T05/
mkdir -p "$LINGXI_HOME" "$ART"
# 测试安装：Tauri app（不含 Electron），构建产物目录 + 独立安装根
# 服务：由 Tauri 壳启动（T02）或以显式 env 启动同一 lingxi-service
```

- 测试安装必须来自 **Tauri 构建产物**（`desktop/src-tauri`，含全量壳；`bundle.active=false` 的现有资源宿主不能充当），并记录产物目录清单 sha256。
- 旧链对照环境与测试安装**物理分离**（不同目录/不同时间窗），且只在对照环境中使用仓库 `node_modules/.bin/electron`。

### 3.1 A09 证据（移除 Electron 后能力仍工作）

**判据映射**：无 Electron 进程/文件探针 + 浏览器用户接管 + Office PDF 输出达标（对照旧链样本 74 页中文长文档/表格/字体三族）。

| 步骤 | 内容 | 命令/脚本草案 | 通过判据 | 证据文件 |
|---|---|---|---|---|
| A09-01 | 安装树/进程探针 | `find "$APP_DIR" -iname '*electron*' -o -iname '*Electron Framework*'`；`ps -axo pid,ppid,command \| grep -Ei 'Electron(\\.app)?'`；`pgrep -fl 'Electron Helper'`；对 app/service/chromium 的 `ps -axo pid,ppid,comm` 全树保存 | 零 Electron 文件、零 Electron 进程；进程树中浏览器/PDF 子进程均为受控 Chromium（路径记录） | `$ART/a09/no-electron/{inventory.json,ps-before.txt,ps-after.txt}` |
| A09-02 | 安装可运行 + 服务就绪 | 启 Tauri app（真实二进制），通过 WebDriver 建 session；调 `/lingxi/v1/health\|identity` 确认服务/节点/版本 | UI ready、服务 `lingxi-service`、节点 id 与 app 一致 | `$ART/a09/boot/{webdriver-session.json,identity.json,screenshot.png}` |
| A09-03 | 浏览器会话（agent） | 用确定性模型 stub（`scripts/rust-tauri/r00-t06-stub-provider.mjs`，支持 `TOOL:<name>:<json>`，见 `:90-108`）发 `TOOL:browser:{"action":"start","url":"<loopback fixture>"}` + `snapshot` | 真实 Chrome 窗口存在（窗口列表/进程），workspace running、url 匹配、tabs 出现；快照含 fixture 标记 | `$ART/a09/browser/{stub-journal.json,workspace.json,snapshot.txt,window-list.txt}` |
| A09-04 | viewer 打开/空态 | main 窗口调用 `open_browser_viewer({sessionPath})`（或点击 BrowserCard）；另开一个无浏览器会话验证空态 | viewer 窗口出现；空会话显示“此聊天没有可用的浏览器状态”；打开 viewer 不新增 tab（tabs 前后一致） | `$ART/a09/viewer/{open.json,empty-state.png,tabs-before-after.json}` |
| A09-05 | 标签/导航同步 | viewer 触发 `new_tab`/`switch_tab`/`close_tab`/`back`/`forward`/`reload`；agent 端快照 | 服务端 tabs/activeTabId 与 viewer 一致；非法 tabId 返回错误且状态不变；关闭失败不显示已关闭 | `$ART/a09/tabs/transcript.jsonl` |
| A09-06 | **用户接管（真实 OS 输入）** | viewer 点「显示」→「接管」；断言 workspace `userTakeover=true`；用 `osascript`（或已批准的 Cua 通道）向真实 Chrome 窗口键入 `HUMAN-TAKEOVER-…`（如 `osascript -e 'tell application "System Events" to keystroke "HUMAN-TAKEOVER-R09" '`；中文用剪贴板+`Cmd+V`）；`screencapture -x`；随后 viewer「交还」，agent `snapshot` 读取 | 接管期间 agent 修改被 `browser_user_takeover` 拒绝；OS 键入落进同一 target 并被服务快照读回；交还后 agent 可继续；页面 target/session 全程同一 | `$ART/a09/takeover/{state.json,typed-readback.txt,blocked-agent.json,screen.png,target-ids.json}` |
| A09-07 | Office PDF 输出（Tauri 链） | 同一确定性 stub 发 `TOOL:html-to-pdf:{"htmlPath":"<abs>/tests/migration/r01-t05/samples/s1-long-zh.html","outputPath":"$ART/a09/pdf/r09-s1.pdf","pageSize":"A4","printBackground":true,"preferCSSPageSize":true,"allowJavaScript":false,"embedLingxiFonts":true,"settleMs":250,"timeoutMs":60000}`（wire name=`local_name`；stub 见 `scripts/rust-tauri/r00-t06-stub-provider.mjs:90-108`；工具权限/审批/真实 Run 按正式链）。注意 office 工具注册为 `Availability::Deferred`（`office_tools.rs:1229`），模型声明快照只声明 Available+resident（`model_exchange.rs:107-116`、`toolcatalog.rs:1812-1827`），stub 直接发 tool_call 只证明真实工具链可执行，**不证明真实模型能自动发现该工具**（见 OQ-16）。 | 产物存在、`%PDF-`/`%%EOF`、sha256 与工具回执一致、经会话文件/或 outputPath 可读；engine 字段为 `Chromium CDP (…)` | `$ART/a09/pdf/{tool-result.json,pdf-meta.json}` |
| A09-08 | 旧链基线重生成（独立对照环境） | `node tests/migration/r01-t05/run_old_chain.mjs --job <job.json> --log …`（`tests/migration/r01-t05/run_old_chain.mjs:1-70`，只读复用 `desktop/src/office-pdf-helper.cjs`；旧链基线在当前工作区缺失，见 1.3） | exit=0、PDF 页数 74、文本/表格/字体探针可读 | `$ART/a09/pdf/old-chain/` |
| A09-09 | 产物比对（A09 判据） | `swiftc -O -o /tmp/pdf_probe tests/migration/r01-t05/pdf_probe.swift && /tmp/pdf_probe <pdf> <outdir> 2`；`python3 tests/migration/r01-t05/compare_outputs.py --old-probe … --new-probe … --old-pdf … --new-pdf … --expect <expect.json> --out compare.json`（expect 参照 `scripts/rust-tauri/r01-t05-replay.sh:103-112`） | 页数相等=74；140/140 表格行；无空白页；三族字体嵌入；文本归一化相似度≥0.99；视觉 16×16 网格 max\|Δ\|≤20；`all_pass=true` | `$ART/a09/compare/{old/,new/,compare.json}` |
| A09-10 | 无暗中调用旧安装 | 在测试安装环境设置 PATH 不含旧 Electron；检查服务/Tauri 打开文件（`lsof -p` 采样）与进程树；若系统存在旧安装，记录其路径并证明未被访问（对照 A09-01 前后） | 无旧安装路径的 execute/open；旧 helper env（`LINGXI_OFFICE_PDF_HELPER_EXEC`）不被使用（Rust 服务无该路径） | `$ART/a09/no-electron/{lsof.txt,env.json}` |
| A09-11 | 19 叶其余能力（浏览器设置/搜索/web/预览） | 设置页/HTTP：preferences GET/PUT、clear-cookies、session-states、open/close-session、search verify（确定性替身）、web_fetch/web_search（loopback 替身）、文件预览（text/markdown/docx/xlsx/pdf 正常样本 + 资源变化重读 + 独立 viewer 只读） | 逐叶断言（见 `FEATURE_STAGE_ACCEPTANCE.json` 对应叶 assertions；R07 已有同源测试：`r07_browser_routes.rs`、`r07_browser_tools.rs`、`r07_web_tools`、`r07_office_tools`） | `$ART/a09/leaves/<leaf-id>.json` |

**A09 产物清单（建议）**：`no-electron/`、`boot/`、`browser/`、`viewer/`、`tabs/`、`takeover/`、`pdf/`、`compare/`、`leaves/`、`manifest.json`（逐文件 sha256 + 命令 + 时间 + 退出码）。

**用户接管证据的可信度要求**（比照 R07 A07 补验口径，`docs/rust-tauri/R07/T04_REPORT.md:108-117`）：
- 必须证明输入来自 OS 通道而非页面 JS（osascript 键入/剪贴板粘贴 + 录屏帧 + AX/文本回读 + agent 被阻断证据）；
- 同一 target/session 全程一致（记录 targetId/sessionPath）；
- 截图/录屏只覆盖本次创建的测试 fixture 页面；
- 失败环境（锁屏/无辅助功能权限）如实记 `ENV_BLOCKED`，不得用脚本改 DOM 冒充。

### 3.2 A10 证据（恶意预览内容无宿主权限）

**判据映射**：SVG/HTML 预览含脚本与 IPC 尝试 → 不能获得 token 或原生命令权限；正常预览保留。

**样本（新增，放 `tests/migration/r09-t05/fixtures/`；只使用合成内容、不放真实系统文件内容）**：

1. `malicious.html`：
   - `<script>` 尝试 `window.__TAURI__?.core?.invoke('a10_canary_write_marker')` / `window.__TAURI_INTERNALS__?.invoke(...)`；
   - `fetch('http://127.0.0.1:<canary>/token')`、`fetch('/lingxi/v1/fs/read?path=/etc/passwd')`、`fetch('http://127.0.0.1:<service>/lingxi/v1/browser/commands')`（携带/不携带垃圾 token）、`WebSocket('ws://127.0.0.1:<canary>/ws')`、`navigator.sendBeacon`；
   - `parent.localStorage`/`top.document`、`postMessage` 到 parent 请求 token；
   - 读 `location.search` 中的 preview token 并回显到 DOM（证明预览 token 不是服务 token，也无法换取服务能力）。
2. `malicious.svg`：`<script>`、`onload`、`<image href="file:///etc/passwd">`、外链 `http://127.0.0.1:<canary>/exfil`、`foreignObject` 尝试嵌入 HTML 脚本；经 MediaViewer `<img>` 加载。
3. `benign.html` / `benign.svg` / `normal.docx` / `normal.pdf`：正常预览对照（含本地相对图片/CSS/字体）。

**步骤**：

| 步骤 | 内容 | 命令/脚本草案 | 通过判据 |
|---|---|---|---|
| A10-01 | 启动 Tauri app + 服务 + loopback canary（记录每次 TCP/HTTP 命中） | `node tests/migration/r09-t05/canary.mjs --port 0 --log $ART/a10/canary.log`；沿用 R01 canary 思路（`tests/migration/r01-t05/canary_server.mjs`） | canary 就绪 |
| A10-02 | HTML 预览：打开恶意文件 | WebDriver 驱动主窗口打开文件预览（或点文件卡）；等待 iframe 加载；screenshot | iframe 出现；恶意脚本运行（页面内标记自证），但 canary 命中 0；`securitypolicyviolation` 记录 connect-src 违规 |
| A10-03 | Tauri 原生命令 canary | 在 `e2e-test` feature 下注册测试命令 `a10_canary_write_marker`（release 构建不得包含，参考 wdio feature 门控先例 `desktop/src-tauri/Cargo.toml:27,32-33`）；恶意预览尝试 `__TAURI_INTERNALS__`/`__TAURI__` | marker 文件不存在；返回 undefined；无 IPC 调用记录 |
| A10-04 | token 暴露面 | 预览文档 URL/DOM/日志扫描：断言服务 bearer token（`local-token.json` 值）不出现在预览 iframe URL、DOM、console、网络请求中；预览 token 仅 10min/单文档（`preview.rs:807-846`），尝试用它调 `/lingxi/v1/browser/*` 或 `/fs/*` 必须 401/403/404 | 服务 token 零出现；preview token 无法越权 |
| A10-05 | SVG 恶意样本 | MediaViewer 打开 `malicious.svg`；检查 script/onload 未执行（页面无副作用标记）、file:// 图片不可读、外链请求被 CSP/CORP 拦截（canary 0 命中） | 无脚本执行痕迹、0 外发、UI 仍显示图片或显式错误（不崩溃） |
| A10-06 | 正常预览保留 | benign HTML/SVG + markdown + docx/xlsx + PDF 逐一打开并断言内容/图片/分页可见 | 正常渲染；无 CSP/权限误伤 |
| A10-07 | 浏览器不可信页对照 | 用受控浏览器打开恶意 HTML 页面，页面侧尝试 `window.__TAURI__`、访问本机文件、调服务 API | 页面拿不到 Tauri/服务 token；服务 API 拒绝（无 bearer）；这与浏览器“用户网页”边界一致（对照 R01 隔离探针，`docs/rust-tauri/R01/BROWSER_SPIKE_REPORT.md:72`） |
| A10-08 | 服务侧回归 | `cargo test -p lingxi-service --test r06_t05_preview`（CSP/token/素材域/50MB，`rust/crates/lingxi-service/tests/r06_t05_preview.rs:73-313,477-621`）；`cargo test -p lingxi-service --test r07_office_tools`；`cargo test -p lingxi-adapters --test r07_pdf_chromium -- --include-ignored`（PDF 资源门禁/WS，`r07_pdf_chromium.rs:502-578`） | 既有回归全绿；R09 不回归到旧链无拦截形态 |
| A10-09 | 独立命令权限面 | `release` 构建 `strings` 探测 `a10_canary`/`wdio` 为 0；`netstat -an \| grep <wdio port>` 无监听（参考 R01 W03/A12 做法 `docs/rust-tauri/R01/DEPENDENCY_DECISIONS.md:243`） | 发布构建无测试后门、无调试端口 |

**A10 威胁模型要点**：
- 预览内容运行在 **跨源 + sandbox（无 allow-same-origin）** iframe：即使脚本执行，也没有 app origin 的 storage/DOM/Tauri IPC；CSP `connect-src 'none'` 禁止 fetch/WS/beacon（`preview.rs:426-433`）。
- Tauri IPC 只注入 app 窗口主 frame；preview iframe 是不同 origin（loopback），不能调用 `window.__TAURI_INTERNALS__`。测试用 canary 命令把“理论不可达”变成“实测零副作用”。
- 服务 bearer token 从不进入 preview 文档/URL；preview token 只能读该文档及素材域（`preview.rs:848-940`）。
- SVG 走 `<img>`：浏览器禁止 img 内脚本执行与外链 file:// 读取；仍需实测证明与 UI 正常。
- 若 DOCX/XLSX/Markdown 渲染路径存在 HTML 注入面，A10 需覆盖并修复/隔离（不要在 T05 调查中假定安全）。

### 3.3 样本与比对方法

- **样本来源**：`tests/migration/r01-t05/samples/`（S1 中文长文档 74 页/140 行表格/三族字体、S2 @page、S3 纸张边距、S4 JS 开关、S5 危险资源、S6 无限脚本）；字体/许可 `tests/migration/r01-t05/FONTS.md`；生成器 `generate_samples.py --check` 可重放（`scripts/rust-tauri/r01-t05-replay.sh:33-36`）。
- **探针**：`tests/migration/r01-t05/pdf_probe.swift`（PDFKit/Quartz：页数/MediaBox/文本 sha256/墨迹/16×16 网格/逐页 PNG）。
- **判据脚本**：`tests/migration/r01-t05/compare_outputs.py`（页数/空白页/哨兵/表格行/文本相似度/视觉网格/字体/图片 XObject）。
- **基线缺失处置**：`artifacts/rust-tauri/R01/T05/a09/new/probe/probe.json` 不在当前工作区（R07 测试也依赖它，`r07_pdf_chromium.rs:391-397`）。A09 重生成旧链基线：
  1. 在独立对照环境按 `scripts/rust-tauri/r01-t05-replay.sh` 的 `run_old()` 逻辑生成 S1 旧链 PDF；
  2. `pdf_probe` 生成 `probe.json/text.txt/page-*.png`；
  3. 冻结到 `artifacts/rust-tauri/R09/T05/a09/old/`，记录 Electron 版本与 helper 源码 sha256；
  4. Tauri 链产物与冻结基线比对，不重新解释旧链行为（R01 已冻结的 KNOWN-DIFF 保留）。
- **PDF 资源/WS 边界回归**：直接使用生产测试 `rust/crates/lingxi-adapters/tests/r07_pdf_chromium.rs`（Fetch/iframe/img/file:///worker/WS/fetch/beacon/meta-refresh canary 零命中，`:502-539`；超时/取消清理 `:541-578`），并在 R09 脚本中以 `--include-ignored` 执行、保存退出码与日志摘要；生产 CSP 已是 `connect-src 'none'`（`pdf.rs:213-228`），比 R01 spike 的 CSP 白名单更强，满足 ADR-003 的 WS 门禁。
- **比对方法**：不逐像素强制一致；语义零缺失 + 页数/表格行/字体族 + 文本相似度 + 视觉网格阈值（与 R01 A09 同口径）。所有 PDF/探针/对照产物归档并写 sha256 清单。

### 3.4 平台证据与阻塞

| 平台 | A09/A10 | 处理 |
|---|---|---|
| macOS arm64（本机） | 可执行本设计全部步骤 | R09 证据主体 |
| macOS x64 / Windows x64 / Linux x64 | 无真机（`docs/rust-tauri/R09/BLOCKED.md:8-10`） | `BLOCKED_PLATFORM`；R09 不得写 PASS；交叉编译/虚拟机只能证明构建，不能替代原生窗口/焦点/预览/安装证据 |
| 正式签名/公证 | 无 Developer ID（`BLOCKED.md:12`） | ad-hoc 本地功能证据；不得声称正式分发验证 |

### 3.5 证据归档结构（建议）

```
artifacts/rust-tauri/R09/T05/
  manifest.json                 # 命令、退出码、时间、环境、逐文件 sha256
  a09/
    env/                        # 本机版本、Chromium 路径+sha256+product、Tauri 产物目录清单
    no-electron/                # 安装树 inventory、进程树前后、lsof
    boot/                       # WebDriver session、identity、screenshot
    browser/                    # stub journal、workspace、snapshot、window list
    viewer/                     # 打开/空态/不建 tab
    tabs/                       # 标签/导航 transcript
    takeover/                   # 状态、OS 键入回读、agent 拒绝、录屏帧、target 一致性
    pdf/                        # 工具回执、PDF 元数据、旧链基线、探针
    compare/                    # compare.json + old/new probes
    leaves/                     # 19 叶逐条断言回执
  a10/
    fixtures/                   # 恶意/正常样本与 sha256
    canary.log                  # loopback TCP/HTTP 命中
    preview/                    # 恶意/正常预览截图、iframe DOM 快照、CSP 违规记录
    ipc/                        # Tauri canary 命令 marker 缺失证明、release strings 探测
    tokens/                     # 服务 token 零出现扫描、preview token 越权拒绝
    regression/                 # r06_t05_preview / r07_pdf_chromium 等回执
```

---

## 4. OpenQuestions / 风险（D）

| ID | 问题/风险 | 现状依据 | 影响 | 建议/处置 | 责任/截止 |
|---|---|---|---|---|---|
| OQ-01 | **Chromium 分发方式与许可**：随包（~400MB 压缩级、嵌套签名/公证/更新）vs 系统 Chrome/Chromium（版本漂移、Linux 无保证）vs 首启下载 | 现状仅路径发现（`browser.rs:1280-1298`）；ADR 决策留 R09（`ADR-002:24,41-46`、`ADR-003:54-59`）；许可与 Electron 同构（`ADR-003:56-58`） | A09 可复现性与“安装即工作” | R09 实现按部署供给接线并冻结方案；建议 R10 选“随包 Chromium”，R09 记录实测版本/路径/sha256；体积/许可/签名/更新通道登记 OPEN | T05 设计 + R10 发布决策 |
| OQ-02 | **Windows/Linux 未验证**：CDP pipe、窗口焦点、autostart、PDF 预览/打印语义、路径与权限 | ADR-002:50、ADR-003:65,79；`BLOCKED.md:8-10` | R09 不能声称跨平台 A09/A10 | 保留 `BLOCKED_PLATFORM`；R09 只出本机证据，跨平台门禁归 T07/R10 | 业主/环境，R10 前 |
| OQ-03 | **macOS 仅 CLT（无 Xcode.app）能否构建/打包 Tauri**：`tauri build` 的 bundling/dmg/签名链未实测 | `docs/rust-tauri/R09/START_RECEIPT.md:6`（Xcode 仅 CLT 27.0，安装包/签名链需实测） | 若不能，只能 `--no-bundle` 产物做功能证据 | T01/T07 先做最小 `tauri build` 冒烟并记录；失败则用 dev 产物 + 明确边界；正式签名仍 BLOCKED（`BLOCKED.md:12`） | T01/T07/R10 |
| OQ-04 | **sidecar 参数限制**：Tauri sidecar/externalBin 只适合单文件、triple 后缀、打包期固定参数，不能承载 Chrome.app 目录/多文件运行时；Tauri shell 插件的能力参数校验只作用于 JS→shell 调用，Rust 侧 spawn 不受其约束 | R01 spike sidecar 结论（`docs/rust-tauri/R01/TAURI_SPIKE_REPORT.md:59-77`）；服务端口是 Rust 侧 `Process::launch`（`launcher.rs:119-220`），不是 Tauri sidecar | 误把 Chromium 当 sidecar 会导致 macOS 目录/签名/更新失败 | **不要把 Chromium 当 Tauri sidecar**：作为 `bundle.resources` 资源目录或系统路径，由服务启动；Tauri 仅传路径/校验 | T05/R10 |
| OQ-05 | **token 不落 URL**：服务 bearer token 绝不出现在 URL（宿主中介）；但媒体/预览 token 走 loopback query（现有设计，仅 local loopback 允许） | `desktop/src/react/services/server-connection.ts:179-181,766-777`；preview token 独立（`preview.rs:807-846`）；`browser-control` 由 host 发 HTTP（`rust-browser-control.cjs:35-58`） | 预览/媒体 URL 在 DOM/devtools 可见 preview token（10min/单文档，不足以换服务能力） | 保持现状 + A10 证明 preview token 越权失败；可选后续用宿主持有 cookie/一次性 URL 代理消除 URL token；**禁止**把服务 token 放进 URL | T05（证明）+T06（可选项） |
| OQ-06 | **Tauri origin 被服务 Origin allowlist 拒绝**：`fetch` 面在 Tauri 下全部 `bad_origin`；HTML 预览 iframe 被 `frame-ancestors` 拒绝（调查基线 HEAD `aac8cb9`）。调查期间观察到工作区已有并发未提交修改：`transport.rs:247` 精确允许三形态 Tauri origin、`preview.rs:432` frame-ancestors 追加同源（2026-10-26 观察，**非 HEAD，未提交、未验证**）——若候选包含该修改，T05 必须以实际候选重新取证并补精确匹配负向用例 | `transport.rs:25-52,224-267`；`preview.rs:426-433`；工作区未提交：`transport.rs:239-249,443-470`、`preview.rs:430-433,947-948` | 直接阻塞 preview/file/browser UI 的服务调用与 HTML 预览 | 方案 A：Tauri 宿主代理全部/关键服务 HTTP（推荐给浏览器/预览面；全量代理属 T01/T02 架构决策）；方案 B：服务 origin allowlist 精确追加 `tauri://localhost`、`http://tauri.localhost`（需要安全评审 + 负向测试）+ CSP frame-ancestors 追加同源；方案 C：Tauri `dangerousUseHttpScheme` 不解决（仍是 tauri.localhost） | T02/T06（T05 提供需求） |
| OQ-07 | **WKWebView 内联 PDF 预览（blob URL）未验证**：Electron/Chromium 可用，WKWebView 可能空白 | 现状 `PreviewRenderer.tsx:657-689`；Tauri 平台 = WKWebView（`docs/rust-tauri/R01/DEPENDENCY_DECISIONS.md:243`） | A09 file-preview 叶/正常 Office PDF 预览可能不达标 | 实施首个 Tauri 冒烟先测 blob PDF；不可用则回退自定义协议临时文件/独立窗口；PDF.js 作为最后选项（需依赖决策） | T05 |
| OQ-08 | **接管后 Chrome 窗口前台激活不保证**：`Target.activateTarget`+`setWindowBounds(normal)` 不等于 OS 级前台/焦点 | `ops.rs:506-539,816-830`；R07 A07 补验由真实点击完成（`docs/rust-tauri/R07/T04_REPORT.md:108-117`） | 用户点“显示/接管”后窗口可能在后方，体验退化 | 接受“用户点击确认”+viewer 文案；可选增强：宿主按服务确认的受控可执行做 macOS/Windows 激活（需 T06 权限评审；不做任意命令面） | T05（最小）+T06（增强） |
| OQ-09 | **服务在 Tauri 安装布局中找不到 Chromium/Office 字体资产**：服务只有路径发现与 env 装配 | `browser.rs:1280-1298`、`host_auxiliary.rs:28-77` | 浏览器全部 `UnavailableBrowser`、PDF 失败（显式，不静默） | T02 启动服务时注入 `LINGXI_CHROMIUM_PATH`/`LINGXI_OFFICE_*`/资源根；缺失时服务显式不可用（A09 探针记录）；R10 打包固化路径 | T02/T05/R10 |
| OQ-10 | **Tauri 壳只支持 lingxi-service 连接后的旧 Node 回退清理**：renderer 多处 `serverNodeKind !== 'lingxi-service'` 分支（local-file、legacy viewer） | `rust-browser.ts:7-9`；`resource-url.ts:25-73`；`BrowserViewerApp.tsx:46-62` | Electron 移除后必须显式失败而非回落旧 viewer | Tauri host 只实现 lingxi-service 路径；非该节点请求返回显式 `browser_connection_required`/`unsupported`；A09 负向：远程/异节点不能触发本机浏览器 | T01/T05 |
| OQ-11 | **A09 旧链基线缺失**：`artifacts/rust-tauri/R01/T05/...` 不在工作区，R07 PDF 质量测试无法直接跑 | `r07_pdf_chromium.rs:391-397`；`ls artifacts/rust-tauri` 仅 R02/R07/R08（本次调查） | A09 无法直接复用 R01 数值 | 用 `tests/migration/r01-t05/` 脚本在独立对照环境重生成并冻结旧链基线（记录 Electron 版本/helper sha256），再与新链 compare；属证据环境构造，不进入测试安装 | T05 |
| OQ-12 | **A10 中 docx/xlsx/markdown 渲染的 HTML 注入面**：docx/xlsx 用 `dangerouslySetInnerHTML` 渲染服务解析出的 HTML；markdown 有 sanitizer | `PreviewRenderer.tsx:755-767`；`markdown.ts:1110-1119`；服务解析 `fsread.rs:198-252` | 若解析器不转义，恶意文档可在 app origin 执行脚本（Tauri IPC 可达） | A10 增加恶意 docx/xlsx/markdown 用例；实测若可注入则修复解析器/隔离渲染（不在本调查中改代码）；T05 报告记录 | T05（测试）+ 修复归 T05/T06 |
| OQ-13 | **发布构建必须无测试后门/调试端口**：A10 需要 `e2e-test` canary/wdio，但 release 不得携带 | `desktop/src-tauri/Cargo.toml:27,32-33`；R01 W03/A12 先例（`DEPENDENCY_DECISIONS.md:243`） | 安全回归 | release `strings` 探测 + 端口探测（A10-09）；canary 只在 `e2e-test` feature | T05/T06 |
| OQ-14 | **浏览器/PDF 进程与 Tauri 生命周期**：Tauri 崩溃时服务/Chromium 可能残留 | 服务退出清理存在（`browser.rs:1262-1267`、`host.rs:1087-1174`），但 Tauri 崩溃未测 | 残留进程/窗口 | T02/T05 增加“宿主任意退出（SIGKILL）后进程树回收”证据；服务侧进程组清理已实现，需实测 | T02/T05 |
| OQ-15 | **真实 Chromium 弹窗/cookie 后 Resume 等 R07 保留失败** | `docs/rust-tauri/R07/R07_HANDOFF.json:36-44`、`T07_FAILURE_HISTORY.json:4798-4824` | 浏览器行为保真 | R09 A09 重跑并保留/修复记录；不得用 R07 失败“已解释”冒充通过 | T05 |
| OQ-16 | **office/pdf 工具的模型可发现性**：office 工具注册为 `Availability::Deferred`（`office_tools.rs:1229`），模型声明快照只声明 `Available` 且 resident 的目标（`model_exchange.rs:107-116`、`toolcatalog.rs:1812-1827`）；本调查未发现 office 工具的 resident/激活路径（`add_model_resident` 只在其它工具面调用，例如 `browser_tools.rs:204-207`）。A09 用确定性 stub 直接发 tool_call 可证明真实工具链可执行，但不能证明真实模型能发现并调用该工具。 | `office_tools.rs:1206-1229`、`model_exchange.rs:107-116,132`、`toolcatalog.rs:1812-1827` | A09 证据只能证明工具链可执行；若真实模型无法发现该工具，office 功能在 Rust 服务不可用（功能回归） | A09 记录“stub 直接派发”口径；把模型可发现性作为 R09 产品面/门禁风险另测（必要时补 on-demand 激活或 resident 注册） | T05 证据口径 + R09 产品面复核 |

### 4.1 三条最高阻塞风险（提交给 lead）

1. **Tauri origin 与 CSP 双重阻断（OQ-06）**：服务 Origin allowlist 不含 Tauri origin、HTML 预览 `frame-ancestors` 不含 Tauri origin。若不先解决，Tauri 下文件预览/服务 fetch/HTML 预览整面不可用，A09 leaf 4173A0 与 A10 都无法通过。
2. **Chromium 分发/签名（OQ-01/OQ-03/OQ-04）**：当前无随包实现、无正式签名身份、CLT-only 构建未验；A09 的“安装内含渲染器且不暗中调用旧安装”在发布形态上仍未定稿。
3. **跨平台与旧链基线（OQ-02/OQ-11）**：Windows/Linux/macOS x64 无真机（BLOCKED_PLATFORM），且 R01 旧链 PDF 基线不在库内；A09 的跨平台份额与对照基线必须先补“独立对照环境重生成”，否则证据不完整。

---

## 5. 参考

- ADR：`docs/rust-tauri/R01/ADR-002-browser-host.md`、`docs/rust-tauri/R01/ADR-003-document-renderer.md`
- Spike：`docs/rust-tauri/R01/BROWSER_SPIKE_REPORT.md`、`docs/rust-tauri/R01/PDF_SPIKE_REPORT.md`、`docs/rust-tauri/R01/TAURI_SPIKE_REPORT.md`
- 依赖决策：`docs/rust-tauri/R01/DEPENDENCY_DECISIONS.md`（D-09/D-10/D-11/D-13）
- R07：`docs/rust-tauri/R07/T04_REPORT.md`（浏览器）、`docs/rust-tauri/R07/T05_REPORT.md`（Office/PDF）、`docs/rust-tauri/R07/T06_MEDIA_STATUS.md`、`docs/rust-tauri/R07/R07_HANDOFF.json`
- R09：`docs/rust-tauri/R09/START_RECEIPT.md`、`docs/rust-tauri/R09/BLOCKED.md`
- 账本：`docs/rust-tauri/R00/ACCEPTANCE_MAP.json`（R09-A09/A10）、`docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json`（19 叶）
- 样本/工具：`tests/migration/r01-t05/`（samples、pdf_probe.swift、compare_outputs.py、old_chain_harness.cjs、run_old_chain.mjs、run_repair_r1_negative.sh）、`scripts/rust-tauri/r01-t05-replay.sh`、`scripts/rust-tauri/r00-t06-stub-provider.mjs`
- 机器证据：`docs/rust-tauri/R09/T05_BROWSER_PDF_EVIDENCE.json`

## 附录 A：R09-T05 的 19 叶原文（id / feature / then / assertions）

数据源：`docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json` 中 `execution_task_ids` 含 `R09-T05` 的补充场景（19 叶，逐字；仅把多条 assertions 用 `<br>` 连接）。

| 叶 ID | feature | when | then | assertions |
|---|---|---|---|---|
| `R00-T02-LA-A67D342381C0` | `F-D14-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-BROWSER-TABS-A67D34` | 在可见浏览器新建、切换或关闭标签 | 会话对应的标签集合和活动标签在 viewer 同步，服务端快照跟随变化 | 1. 会话对应的标签集合和活动标签在 viewer 同步，服务端快照跟随变化<br>2. 无效 tabId 不执行；关闭失败不得显示已关闭 |
| `R00-T02-LA-A09B78985A14` | `F-D14-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-BROWSER-EMERGENCY-STOP-A09B78` | 在可见浏览器点击紧急停止 | 有会话时 server BrowserManager 关闭浏览器并撤销 Agent 授权至下一条消息 | 1. 有会话时 server BrowserManager 关闭浏览器并撤销 Agent 授权至下一条消息<br>2. 无会话旧实例仅本地销毁；关闭失败应可见，不得只隐藏窗口 |
| `R00-T02-LA-8A45B19A18B4` | `F-D14-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-BROWSER-NAVIGATION-8A45B1` | 在可见浏览器后退、前进、刷新或输入地址 | 活动会话 WebContents 导航并更新地址和可用状态 | 1. 活动会话 WebContents 导航并更新地址和可用状态<br>2. 无活动 view 不执行；非法地址不加载 |
| `R00-T02-LA-13049521EA0C` | `F-D14-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-CLOSE-BROWSER-VIEWER-130495` | 关闭可见浏览器窗口 | viewer 窗口关闭；会话浏览器清理应按已有会话策略另验 | 1. viewer 窗口关闭；会话浏览器清理应按已有会话策略另验<br>2. 关闭窗口不能误记为急停撤权 |
| `R00-T02-LA-6E13E5DF05EB` | `F-D14-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-OPEN-BROWSER-VIEWER-6E13E5` | 打开可见浏览器查看器 | 窗口显示指定会话标签；传入允许 URL 时新开标签，空会话显示空态 | 1. 窗口显示指定会话标签；传入允许 URL 时新开标签，空会话显示空态<br>2. 非法 URL 不加载；打开窗口本身不创建空标签 |
| `R00-T02-LA-7D212CBD9EEF` | `F-D14-SEMANTIC_EFFECT-SEMANTIC-EFFECT-CONFIG-SEARCH-VERIFY-POST-7D212C` | 用户验证搜索供应商 | 外部验证成功后保存选定 provider/key，失败不保存 | 1. 成功时核对：外部验证成功后保存选定 provider/key，失败不保存<br>2. 拒绝/无效输入时核对：无越权或错误持久化，且错误对用户可见 |
| `R00-T02-LA-1809C8E13EA3` | `F-D14-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-BROWSER--1809C8` | 用户读取浏览器偏好 | 返回浏览器设置及运行中 BrowserManager 状态，不改动设置 | 1. 成功：执行“读取浏览器偏好”后，返回浏览器设置及运行中 BrowserManager 状态，不改动设置<br>2. 拒绝/边界：engine.getBrowserPreferences/setBrowserPreferences；PUT 后调用 applyBrowserPreferences；不应错误扩大写入或披露范围 |
| `R00-T02-LA-03A9E92909C6` | `F-D14-ROUTE_BEHAVIOR-BEHAVIOR-PREFERENCES-PREFERENCES-BROWSER-CLEAR-C-03A9E9` | 用户清除内置浏览器站点资料 | 清除 Cookie 与站点数据后返回成功，失败时返回错误 | 1. 执行“清除内置浏览器站点资料”后核对：清除 Cookie 与站点数据后返回成功，失败时返回错误<br>2. 失败、冲突或拒绝时核对返回的状态与实际副作用一致 |
| `R00-T02-LA-ED5FD5B469B8` | `F-D14-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-BROWSER--ED5FD5` | 用户更新浏览器偏好 | 保存浏览器设置及运行中 BrowserManager 状态并返回新状态 | 1. 成功：执行“更新浏览器偏好”后，保存浏览器设置及运行中 BrowserManager 状态并返回新状态<br>2. 应用失败返回 400 时读回已保存偏好与运行中浏览器状态，不假设自动回滚 |
| `R00-T02-LA-367052089D11` | `F-D14-SEMANTIC_EFFECT-SEMANTIC-EFFECT-SESSIONS-BROWSER-SESSION-STATES-367052` | 用户查看浏览器会话状态 | 返回活跃、可恢复或不可用状态 | 1. 给定目标与相应权限，执行“查看浏览器会话状态”后核对：返回活跃、可恢复或不可用状态<br>2. 输入无效、权限不足或下游失败时应返回明确错误，不呈现成功结果 |
| `R00-T02-LA-593534A4F2AB` | `F-D14-SEMANTIC_EFFECT-SEMANTIC-EFFECT-SESSIONS-BROWSER-SESSIONS-593534` | 用户查看已运行浏览器会话 | 返回 BrowserManager 当前浏览器会话列表 | 1. 给定目标与相应权限，执行“查看已运行浏览器会话”后核对：返回 BrowserManager 当前浏览器会话列表<br>2. 输入无效、权限不足或下游失败时应返回明确错误，不呈现成功结果 |
| `R00-T02-LA-178C8CBA29C2` | `F-D14-SEMANTIC_EFFECT-SEMANTIC-EFFECT-SESSIONS-BROWSER-CLOSE-SESSION-C-178C8C` | 用户关闭会话浏览器 | 会话浏览器关闭并广播状态 | 1. 给定目标与相应权限，执行“关闭会话浏览器”后核对：会话浏览器关闭并广播状态<br>2. 输入无效、权限不足或下游失败时应返回明确错误，不呈现成功结果 |
| `R00-T02-LA-F1A5597E3E78` | `F-D14-SEMANTIC_EFFECT-SEMANTIC-EFFECT-SESSIONS-BROWSER-CLOSE-SESSION-R-F1A559` | 用户关闭并撤销浏览器授权 | 会话浏览器关闭且该会话的浏览器授权被撤销 | 1. 给定目标与相应权限，执行“关闭并撤销浏览器授权”后核对：会话浏览器关闭且该会话的浏览器授权被撤销<br>2. 输入无效、权限不足或下游失败时应返回明确错误，不呈现成功结果 |
| `R00-T02-LA-DCD85A04A7E9` | `F-D14-SEMANTIC_EFFECT-SEMANTIC-EFFECT-SESSIONS-BROWSER-OPEN-SESSION-DCD85A` | 用户打开会话浏览器 | 尝试恢复选定会话浏览器并让查看器显示该会话；恢复失败可显示空态 | 1. 给定目标与相应权限，执行“打开会话浏览器”后核对：尝试恢复选定会话浏览器并让查看器显示该会话；恢复失败可显示空态<br>2. 输入无效、权限不足或下游失败时应返回明确错误，不呈现成功结果 |
| `R00-T02-LA-A30E4BDF2FA2` | `F-D14-TOOL-TOOL-BROWSER-A30E4B` | 分别调用 browser 的主要分支，并从会话、文件/状态或外部结果读回 | 按会话隔离的浏览器完成动作，返回快照/图片/状态并记录操作；可见模式有远控状态。 | 1. 导航后标题或快照匹配<br>2. 过期ref明确失败<br>3. 会话A状态不泄给B<br>4. 无会话、过期元素ref、阻断网页或浏览器错误显式失败，不误操作另一会话。 |
| `R00-T02-LA-FD7A5A76B8EA` | `F-D14-TOOL-TOOL-WEB-FETCH-FD7A5A` | 分别调用 web_fetch 的主要分支，并从会话、文件/状态或外部结果读回 | 返回页面/接口正文、最终URL或提取摘要，供会话引用。 | 1. 正文与URL对应<br>2. 阻断页不当正常内容<br>3. URL无效、重定向/网络失败或受限内容给错误；不自动写工作区。 |
| `R00-T02-LA-D1B56A275C3A` | `F-D14-TOOL-TOOL-WEB-SEARCH-D1B56A` | 分别调用 web_search 的主要分支，并从会话、文件/状态或外部结果读回 | 返回标题、链接、摘要及供应商信息，结果可在会话中引用。 | 1. 结果含真实链接<br>2. 无配置不假成功<br>3. 未配置/缺key、限流/服务故障显式失败，不伪造搜索命中。 |
| `R00-T02-LA-4173A08A51CC` | `F-D09-UI_BEHAVIOR-UI-BEHAVIOR-FILE-PREVIEW-4173A0` | 从书桌/资源打开文件预览并等待外部改动 | 文本/Markdown/Office 等显示相应预览；资源改变事件后重读；可派生只读窗口 | 1. 文本/Markdown/Office 等显示相应预览；资源改变事件后重读；可派生只读窗口<br>2. 二进制/过大文件显示信息卡；读取失败错误；独立 viewer 不回写主编辑状态 |
| `R00-T02-LA-8BD3BCC6D725` | `F-D14-UI-UI-SETTINGS-BROWSER-8BD3BC` | 逐一执行 UI 动作矩阵记录的自动加载与可见控件 | 逐动作核对实际状态与页面投影；失败不伪装成空数据 | 1. 打开设置容器自动初始化：连接、语言、助手、快照、模型和供应商摘要（各页共用）：成功核对 冷启动完成后 ready；已有缓存直接显示旧数据并后台刷新；供应商摘要不阻塞 ready；空态/未初始化核对 无缓存时显示 loading mask；15 秒超时强制 ready；供应商摘要可暂缺；错误/无权限核对 初始化异常会 log 并 ready=true，可能展示旧/空数据；供应商摘要失败被 catch 静默，不能当空列表成功<br>2. 自动从设置快照或 GET 读取浏览器偏好：成功核对 Cookie 与开标签行为呈现；空态/未初始化核对 prefs null 时控件禁用；错误/无权限核对 读取失败 toast<br>3. Cookie 接受开关和 Agent 打开行为：成功核对 乐观更新后以服务端结果回写快照；空态/未初始化核对 prefs 尚无数据时禁用；错误/无权限核对 失败回滚旧值并 toast<br>4. 清除 Cookie：成功核对 成功 toast；空态/未初始化核对 无适用空态；错误/无权限核对 失败 toast；清理中按钮禁用 |

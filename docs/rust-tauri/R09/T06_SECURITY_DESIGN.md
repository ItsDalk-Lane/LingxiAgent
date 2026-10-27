# R09-T06 安全设计：Tauri 权限、CSP 与自定义命令收口（A11/A12）

状态：**FROZEN_CANDIDATE**（静态策略/命令面/capability/交付 release 候选 + G1 evidence 候选已交付；运行期负向矩阵 blocked，等待 Lead visible clearance）
作者：`r09-t06-security`（T06 实现者）
日期：2026-10-26 | 分支 `codex/rust-tauri-migration` | 基点 HEAD `aac8cb9fe6bb`
策略唯一来源：`docs/rust-tauri/R09/R09_COMMAND_POLICY.json`（下称 **policy**，sha256 见 T06_PROGRESS）
审计工具：`scripts/rust-tauri/r09_capability_audit.py`
发布扫描：`scripts/rust-tauri/r09_release_attack_surface.py`
证据生产者：`scripts/rust-tauri/r09_t06.py`（套件 `r09_t06_capability_matrix` / `r09_t06_release_attack_surface`）

> 本文是**设计意图与裁决记录**，不是验收通过声明；任何“通过”都以真实命令退出码与
> 证据文件为准（见 `T06_PROGRESS.md`）。**本轮未启动任何 Tauri app/GUI/安装包**；
> `desktop/src-tauri/**` 由 r09-host-core 独占写，本文件只给出待交接 patch。

---

## 1. 范围、权威与任务映射

| 任务书验收 | 原文通过条件 | 本文/工具覆盖 | 状态 |
|---|---|---|---|
| R09-A11 capability 叠加不意外扩权 | 同 webview 被多个 capability 命中，请求敏感命令集合；最终权限与白名单完全一致，越权集合为空；证据=配置展开 + 运行负向测试 | §2 逐窗口策略、§4 叠加场景、§10 运行期矩阵设计；静态展开/比对=`r09_capability_audit.py`（audit-05-freeze **PASS exit 0**）；运行负向=**blocked**（禁止 GUI） | 静态 PASS；运行期 BLOCKED_GUI_SMOKE |
| R09-A12 生产无测试控制面 | release 包已安装；扫描监听端口并调用测试命令；测试 WebDriver/任意 Rust 执行接口不可达；证据=产物级负向探针 | §6/§7 调试面移除策略（D11 test-hooks feature 门控）、§8 sidecar/无泛化 shell、§9 发布产物扫描=`r09_release_attack_surface.py`（release-scan-05-freeze **PASS exit 0**）；“安装 + 端口扫描 + 调用测试命令”=**blocked**（禁止启动/安装 GUI） | 静态 PASS；运行期 BLOCKED_GUI_SMOKE |

输入权威：
- `docs/rust-tauri/R09/T03_HOST_CAPABILITY_INVENTORY.json/md`：50 能力/91 命令与 per-capability `permissionScope`（逐窗口最小化范围）；
- `docs/rust-tauri/R09/T03_T04_PROGRESS.md` §4：按窗口 capability 分组草案；
- `docs/rust-tauri/R09/T05_PROGRESS.md` §7：browser-viewer 5 命令/权限契约；
- `docs/rust-tauri/R09/T01_T02_HOST_SERVICE_DESIGN.md` §3.6/§4.4：CSP、withGlobalTauri、build.json 协议身份；
- `docs/rust-tauri/R09/IMPLEMENTATION_PLAN.md` D5/D8/D9：sidecar、asset scope、四集合一致；
- `desktop/src-tauri/src/native/window_labels.rs`：窗口 label 角色/glob 唯一来源；
- `desktop/src-tauri/src/native/install.rs::INVOKE_COMMANDS`：新增命令名唯一清单。

**独立性原则**：policy 的 `allowedCommands` 由上述意图来源手工推导；`capabilities/*.json`、
`gen/schemas/*` 只作为被审计对象。audit 对 `actual - allowed` **一律报红**，policy 永不
吸收当前配置。意图已定但实现未到的缺口进入 `gapLedger`（显式账本），audit 逐条核对
“缺失集合 == 账本”；越权集合不允许进入账本。

---

## 2. 威胁模型

| # | 威胁 | 入口 | T06 对策 | 剩余风险 |
|---|---|---|---|---|
| T1 | 壳内 renderer 被 XSS/第三方脚本劫持，调用超范围命令 | 主窗/设置/快聊等本地 webview | 逐窗口命令白名单 + 插件权限只留 `core:event:default` + CSP 禁 `unsafe-eval`/远程脚本 + 命令内 label 二次校验 | 同源 XSS 仍可调用该窗白名单内命令（最小化面而非零信任） |
| T2 | 不可信网页内容获得宿主能力（IPC/token/文件） | 浏览器 viewer、HTML/PDF 预览、远程内容 | 远程网页不加入任何 capability；浏览器可见内容由服务控制的独立 Chromium 承载；预览为跨源 sandbox iframe；`dangerousRemoteDomainIpcAccess` 不配置；asset scope 不覆盖远程内容 | 依赖 Tauri 跨源 IPC 拒绝实测（运行期 BLOCKED） |
| T3 | 服务 bearer token 经 URL/query/DOM/日志泄露 | get_server_connection_info / get_server_token | token 只在 Rust managed state + 命令返回值；不写 URL/query/localStorage/诊断；browser-viewer 只拿不透明 connectionKey | viewer 窗口为自身 ResourceIO WS 需 token（受保护命令，见 §3.4） |
| T4 | 本地攻击者用测试 env/调试开关改变 release 行为或打开 WebDriver | env / 启动参数 / feature | release 二进制不得含 `LINGXI_R09_AUTOMATION`/`LINGXI_R09_HOST_DIALOG_SINK`；wdio 仅 `e2e-test` feature；release 不启用 devtools feature；debug 命令 cfg 剔除 | 当前 host 二进制仍含自动化 sink 字符串（F5，待 patch） |
| T5 | sidecar 被替换/伪造，或宿主出现泛化 shell | `Contents/MacOS/lingxi-service` | 固定 externalBin 二进制 + `build.json` 校验（protocol/sha/contentSha）+ 只允许 `--home` 参数 + 零 shell 插件权限 + 无 renderer 可控 argv | ad-hoc 签名（BLOCKED-SIGNING-IDENTITY）；Win/Linux 未真机验证 |
| T6 | capability 叠加（多文件命中同一 webview）悄悄扩权 | main 命中 resource-main + r09-host | 并集展开后与 policy 全等比对；`stackingWindows.main.minCapabilities=2` 显式要求叠加场景存在 | 新增 capability 文件必须重跑 audit |

---

## 3. 逐窗口权限意图

窗口 label 唯一来源 `src/native/window_labels.rs`（main / settings / quick-chat /
browser-viewer / splash / onboarding / `viewer-*` / `skill-viewer-*`）。policy 的
`windows` 键与之一一对应（audit case `caps::policy-window-labels-match-native`）。
远程网页内容**不是**窗口 label，不加入任何 capability。

命令数（policy 意图面，release）：main 70 / settings 33 / quick-chat 10 / splash 3 /
onboarding 7 / browser-viewer 6 / viewer-* 12 / skill-viewer-* 4；去重命令面 **79 条**。
插件权限基线统一为 `["core:event:default"]`（见 §3.9）。

### 3.1 main（主窗口）

- 职责：唯一同时持有服务连接信息、文件读写/预览、浏览器控制、生命周期控制、系统集成设置写入的窗口。
- 授权组：R06 文件 IO/监听/资源桥/派生 viewer/技能只读/open-external/trash/拖出/clipboard；
  T01/T02 boot 与窗口控制（app_ready/window_*/get_platform/get_app_version/theme）；
  T03/T04 设置广播、通知、keep-awake、auto-launch、快捷键、quick-chat 控制/ack、speech 权限；
  T05 browser-viewer 控制面（set/open/rust_browser_request/close）；observability export。
- 明确拒绝：`skill_viewer_load/close`、`viewer_request_load/close`、`get_browser_viewer_context`（属专用窗口）、
  debug 命令（release）。
- 二次校验：`get_server_connection_info/get_server_port/get_server_token` 在命令实现内只接受
  `label == main`（T01/T02 契约），capability 只是第一道门。
- 来源：host:file-dialogs、file-text-io、file-watch、docx-xlsx-preview、observability-export、
  detached-viewer、skill-viewer、external-open、trash、clipboard-edit-command、drag-out、
  app-ready、window-controls、theme-sync、app-info-platform、reload-restart、app-lifecycle-quit、
  settings-window、settings-broadcast、global-shortcut、notification、quick-chat-window、
  quick-chat-session-handoff、keep-awake、auto-launch、server-connection-info、browser-viewer、speech-permission。

### 3.2 settings（设置窗口）

- 职责：设置读写、系统集成开关、密钥/快捷键编辑、观测导出。
- 授权：文件选择（select_files/select_folder/select_skill）、open_file/open_folder/show_in_finder/open_external、
  run_edit_command、open_settings/settings_changed/restart_app、连接信息三件套、get_platform/get_app_version、
  窗口控制（win/linux 自绘标题栏）、theme、observability export、keep-awake/auto-launch（读写）、
  快捷键三命令 + 状态。
- 明确拒绝：`reload_main_window`、quick-chat 窗口控制、`show_notification`（T03 scope=main only）、
  全部文件内容 IO（read/write/watch/spawn_viewer/viewer_*）、技能查看器、浏览器控制、debug。
- 裁决说明：T03_T04_REGISTRATION §4.2 草案曾把 `show_notification`/`restart_app` 列入设置窗；
  T03 inventory scope 明确 notification=main only，故本策略收敛为 main-only；`restart_app` 保留
  设置窗（真实消费者 `AccessTab.tsx:366/390`），属“消费者驱动”对 scope 的显式例外。

### 3.3 quick-chat（快速对话窗口）

- 职责：全局快捷键/托盘唤起的轻量输入窗，需要本地服务连接（WS）与窗口自身 show/hide/resize、
  会话交接（发起）、外链打开。
- 授权：get_platform、get_server_connection_info/port/token（含 R06 fallback）、window_theme_changed、
  open_external、quick_chat_show/hide/resize、quick_chat_open_session。
- 明确拒绝：settings_changed（T03 scope 未含快聊窗，且无消费者）、文件/设置/浏览器/debug 命令。

### 3.4 browser-viewer（浏览器控制面板）

- 职责：Tauri 侧控制面板（`browser-viewer.html?mode=native`），显示服务侧 Chromium 的标签/导航状态。
  远程网页内容不在该 webview 内。
- 授权（T05 §7 契约）：`get_browser_viewer_context`（只返回本窗口 target 的不透明 context）、
  `rust_browser_request`（12 项 action 白名单 + sessionPath 校验 + 无重定向 + 30s）、
  `close_browser_viewer`（只关自己，绝不发 stop/revoke）、get_platform、window_theme_changed、
  settings_changed（主题广播）。
- 明确拒绝：`set_browser_connection`/`open_browser_viewer`（仅 main）、token 三件套（面板不需要、
  拿不到）、文件/外链/debug 命令。`browserGoBack/Forward/Reload/NewTab/SwitchTab/CloseTab/EmergencyStop`
  不再单独注册命令，统一走 `rust_browser_request` 的 action 白名单（T05 最终契约）。
- 远程内容零 capability：独立 Chromium 顶层窗口（服务控制）+ 面板只转发白名单动作；`dangerousRemoteDomainIpcAccess`
  不配置，远程 webview 拿不到 Tauri IPC。

### 3.5 viewer-*（派生只读文件预览）

- 职责：只读展示主窗派生的本地文件 tab；自身 ResourceIO 变更重读；关闭自己。
- 授权：read_file/read_file_snapshot/read_file_base64/file_info（只读且命令实现按窗口限制到自身文件）、
  resource_subscribe/resource_events/resource_unsubscribe、viewer_request_load/viewer_close、
  get_server_connection_info/port/token（自身窗口 WS 需要；受保护命令）。
- 明确拒绝：一切写/删除/拖动/外链/技能/浏览器/debug 命令。
- 风险裁决：R06 现役 viewer 直接以 token + `127.0.0.1:<port>` 建 WS（`viewer-resource-events.ts:53-58`），
  Tauri 下无法避免 token 到 viewer；归类为**受保护命令**（policy.protectedCommands），R10 收敛为
  宿主代理的 subscription 命令（viewer 不持 token）。该收敛不阻塞 R09，但必须在 R10 安全清单中。

### 3.6 splash / onboarding

- splash：只读 splash 数据/头像/平台（get_splash_info/get_avatar_path/get_platform）；不接服务连接、
  不发通知、不控制窗口。
- onboarding：首设完成（onboarding_complete）、读 splash 信息/头像、平台、本地服务连接三件套。
- 裁决：T03 scope 写 “onboarding_complete（splash/onboarding/主窗）”；本策略收敛为 onboarding only
  （SplashApp 不调用、主窗不调用；最小权限）。debug 命令只在 dev capability（§7），release 不存在。

### 3.7 skill-viewer-*（技能只读窗口）

- 授权：skill_viewer_load/list_skill_files/read_skill_file/skill_viewer_close；只读自身解压目录。
- 明确拒绝：open_skill_viewer（main 命令）、写/读任意文件、外链、debug。
- 实现约束（T03/T06 共同要求）：打开 .skill/.zip 必须 Rust `zip` crate 解压（路径穿越/压缩比/文件数限制），
  **禁止** exec `unzip`/`powershell Expand-Archive`（现役 Electron `main.cjs:6165-6175` 是真实命令面）。

### 3.8 远程/不可信内容隔离

1. 远程网页不是窗口 label，不加入任何 capability（audit 拒绝 windows 含 `*`/URL/remote/webview 的模式）。
2. 浏览器可见内容 = 服务控制的独立 Chromium 顶层窗口（D7），不注入 wry/Tauri 桥；Tauri 侧只有控制面板。
3. HTML/PDF 预览由服务 `127.0.0.1:<port>` 提供，在跨源 sandbox iframe 中渲染；CSP `frame-src 'self' blob:`，
   远程脚本/`unsafe-eval` 被 CSP 阻止；不授予 `http://127.0.0.1:*` 页面任何 capability（capability 只按窗口 label 分组）。
4. `app.security.dangerousRemoteDomainIpcAccess` 必须保持未配置（默认拒绝远程 IPC）；
   release 扫描 case `release::no-remote-domain-ipc-access` 静态验证。
5. asset protocol scope 不覆盖远程内容；只按已授权本地根运行时添加（D8）。

### 3.9 插件权限基线裁决（F1）

T03 草案大量使用 `core:default`。经 `gen/schemas/acl-manifests.json` 展开，
`core:default` = `core:path:default` + `core:event:default` + `core:window:default` +
`core:webview:default` + `core:app:default` + `core:image:default` + `core:resources:default` +
`core:menu:default` + `core:tray:default`。其中：
- `core:tray:default` 允许 renderer `new/remove/set-*` 托盘；
- `core:menu:default` 允许创建/弹窗/`set-as-app-menu`；
- `core:webview:default` 含 `allow-internal-toggle-devtools`。

renderer 实际只用 `invoke` + 事件监听（`tauri-desktop-host.ts`），**不需要**上述能力。
裁决（已落地）：capability 插件权限只授予 **leaf** `core:event:allow-listen` /
`core:event:allow-unlisten`（renderer 只用 `invoke` + `event.listen`）；`core:default` 视为越权（F1）。
最终 9 个 release capability 均无 `core:default`，audit `caps::window-plugin-permissions::*` 与
`caps::no-dangerous-plugin-permissions` PASS；release ACL 不再含 tray/menu/devtools **grant**
（二进制 ACL 定义数据里仍可能内嵌 core 插件 permission 定义，扫描器按“定义≠授权”只检查
capability 实际 grant，见 §9）。

### 3.10 withGlobalTauri 裁决（F6 相关）

T01/T02 设计 §3.6 推荐保留 `withGlobalTauri: true`（同一 React 产物不引入 `@tauri-apps/api`，
避免把 Tauri 假设打进 Electron 产物）；T03 §7.4 建议 release 置 false。
裁决：**R09 保留 true**，理由是：`withGlobalTauri` 只暴露 ACL 允许的命令，不是 ACL 绕过
（R01 spike 已实测未授权窗口全拒）；真正边界是 capability 白名单 + CSP。
但必须记录：`window.__TAURI__` 的存在扩大同源 XSS 的可调用面（受白名单限制）。
R10 若拆出独立 Tauri renderer 构建，再评估 `withGlobalTauri=false` + 打包时注入
`@tauri-apps/api`（届时更新 policy 的 `releaseConfig.withGlobalTauri` 与 audit 期望）。
audit case `caps::with-global-tauri-matches-policy` / `release::withglobaltauri-matches-policy` 固化该裁决。

---

## 4. A11：capability 叠加场景设计

- 现役 main 同时命中 `resource-main`（R06 文件资源）与 `r09-host`（T01/T02 boot 桥）两条 capability；
  audit `caps::window-stacking::main` 要求 `minCapabilities>=2`，即**叠加场景真实存在**。
- 静态比对：对 8 类窗口分别求「命中 capability 的 permissions 并集」→ 自定义命令集合 +
  插件权限集合，与 policy 全等（插件权限要求 actual ⊆ allowed）；越权集合（extraPrivilege）
  直接 feature 级红，缺失集合必须与 gapLedger 完全一致。
- policy 还要求 stack 不产生越权：即两条 capability 各自最小、叠加后仍等于白名单；host 新增
  任何第三条 capability 都会自动进入并集比对。
- 运行期负向矩阵（§10）在 GUI 解禁后对每个窗口逐一调用「白名单外命令」断言 ACL 拒绝；
  叠加场景额外验证：仅被另一窗口授权的敏感命令（如 viewer 的 `viewer_close`、browser-viewer 的
  `get_browser_viewer_context`）在 main/settings 也必须被拒。

---

## 5. CSP 设计（逐指令理由）

CSP 由 Tauri 配置注入（`app.security.csp`）；页面内 `modules/connection-csp.js` 仍会
`document.write` 一个 meta CSP（Electron 遗留），两者取交集（T01 Q-07）。release 必须
独立收紧，不能依赖页面 meta。

| 指令 | 允许源（policy 基线） | 理由 / 风险 |
|---|---|---|
| `default-src` | `'self'` | 默认拒绝所有远程源；Tauri 资源只能来自打包产物 |
| `script-src` | `'self'` | 禁 `unsafe-eval`/`unsafe-inline`/远程脚本；Vite 产物无内联脚本（`index.html` 只有 script src） |
| `style-src` | `'self' 'unsafe-inline'` | React/CSS-in-JS 运行时内联样式需要；不放宽 script |
| `connect-src` | `'self' ipc: http://ipc.localhost http://127.0.0.1:* ws://127.0.0.1:*` | Tauri IPC（mac/linux `ipc:`，Windows `http://ipc.localhost`）+ 本地服务 HTTP/WS（临时端口，故 `127.0.0.1:*`）。禁止远程 source/子域 |
| `img-src` | `'self' data: blob: asset: http://asset.localhost http://127.0.0.1:*` | 头像/预览图（asset protocol，D8）、data/blob 缩略图、服务本地图片；**不允许** `file:`（Tauri webview 不能吃 file://，D8） |
| `media-src` | `'self' data: blob: asset: http://asset.localhost http://127.0.0.1:*` | 音视频预览（本地文件走 asset/服务流） |
| `font-src` | `'self' data:` | 打包字体 + data font；无远程 CDN |
| `frame-src` | `'self' blob:` | HTML 预览只允许同源页面/blob；**不允许** `data:`/`file:`/任意 `http://localhost:*`（当前配置含这些，F2） |
| `object-src` | `'none'` | 禁插件/PDF 内嵌对象 |
| `base-uri` | `'none'` | 防 `<base>` 劫持相对 URL（当前缺失，F2） |
| `form-action` | `'none'` | 壳内页面不应提交表单到远程（当前缺失，F2） |

平台差异（CSP 源写法）：
- macOS（WKWebView）：自定义协议为 `tauri://localhost` / `asset://localhost`；CSP 用 `asset:`。
- Windows（WebView2）：`http://ipc.localhost` / `http://asset.localhost`（plan 已列入允许源）。
- Linux（WebKitGTK）：`tauri://localhost` / `asset://localhost`。
- 三平台 release 共享同一份 release config；因此允许源同时包含 `ipc:`、`http://ipc.localhost`、
  `asset:`、`http://asset.localhost`，但**不得**包含 `http://localhost:*`、`https://`、`file:`、
  `unsafe-eval`。运行期 CSP violation 控制台断言（T01 Q-07/A01/A10）在 GUI 解禁后补证。

当前证据（release-scan-02 实测）：`connect-src`/`default-src`/`script-src`/`style-src`/`font-src`/
`object-src` 已符合；`img-src`/`media-src` 多 `file:`、缺 `blob:`/`asset:`/`http://asset.localhost`；
`frame-src` 多 `data:`/`file:`/`http://127.0.0.1:*`/`http://localhost:*`；缺 `base-uri`/`form-action`；
并命中禁用子串 `http://localhost:*`（F2）。这些是 release config 待交接 patch（§11 P6）。

---

## 6. token 不落 URL / 页面

1. **交付面**：`get_server_connection_info`（主命令，T02 supervisor 从 managed state 读
   `instance.json`/`local-token.json`）与 R06 兼容的 `get_server_port`/`get_server_token`；
   三者都在 `policy.protectedCommands` 中按窗口最小化（main/settings/quick-chat/onboarding/viewer-*）。
2. **禁止面**：token 不得进入 URL/query/hash/localStorage/sessionStorage/DOM 文本/日志/诊断 JSON；
   不得通过 asset URL、CSP report、title 或 window state 落盘。Rust 侧 `HostConnection` 不实现
   `Serialize/Debug`（T05 `browser_viewer.rs` 已如此）；browser-viewer 只拿 32B CSPRNG
   `connectionKey`（opaque），拿不到 token。
3. **viewer 例外**：见 §3.5；viewer 为自身 ResourceIO WS 需 token，属受保护命令 + R10 收敛项。
4. **Web fallback 边界**：`web-desktop-host.ts:79-81` 从 URL query/localStorage 取 token 属
   Web 部署形态，不是 Tauri/Electron 桌面形态；T06 文档标注边界，R09 不把它当桌面证据。
5. **运行期断言**（blocked）：wdio 在主窗/viewer/browser-viewer 读取 DOM/URL/localStorage/
   `window.__TAURI__` 上下文，断言 token 值只出现在命令返回值（测试侧不打印 token，
   用 `sha256(token) prefix` 比较）；远程内容 iframe 内 fetch HttpOnly? 不适用——断言 iframe
   无法拿到 token（跨源 + 无 capability）。

---

## 7. 生产无测试控制面（debug/测试移除策略）

| 控制面 | 现状 | 目标（patch） | 验证 |
|---|---|---|---|
| wdio WebDriver | `Cargo.toml` optional + `e2e-test` feature；`lib.rs` `#[cfg(feature="e2e-test")]` 才 `tauri_plugin_wdio_webdriver::init()` | 保持 feature-gated；release 构建不启用 | audit `caps::wdio-feature-gated`；release 扫描二进制无 wdio/webdriver 符号（PASS） |
| debug onboarding 命令 | `native_commands.rs` 实现，`r09-test-hooks` feature 下由 build.rs manifest + invoke_handler 两个 cfg 变体注册；`capabilities-test/r09-test-hooks.json` 由 wrapper 仅 evidence 构建 stage | 交付 release 不生成 debug 权限、不 stage capability | audit `caps::release-denied-commands-not-granted` / `caps::debug-commands-declared-dev-only`；交付 release 二进制 `debug_open_onboarding`=0（PASS） |
| `--dev` 启动 flag | release 不接受（native `reload_restart::reject_restart_arguments` 拒绝 restart 参数；config 无 devUrl） | 保持；release 启动参数不含 dev 语义 | audit/release config 无 devUrl；运行期（blocked）以 `--dev` 启动 release 断言不进入 dev 行为 |
| devtools | `tauri` features 不含 `devtools`；源码无 devtools toggle；capability 无 `core:default`/`core:webview:*` grant | 保持不启用 | audit `caps::tauri-crate-no-devtools-feature`/`caps::no-devtools-control-in-source` + 扫描 `release::capabilities-plugin-grants-minimal`（**PASS**） |
| 自动化 UI sink | 已按 D11 全部收进 `r09-test-hooks` feature（隐藏窗口/Accessory/抑制弹窗/sink/DOM 投影/debug 命令） | 交付 release 仅 `--features r09-host`，不编译该分支 | audit `caps::test-hooks-feature-declared`/`delivery-release-excludes-test-hooks`/`delivery-release-binary-no-test-env` + 扫描 `release::no-test-control-env-in-artifacts`（**PASS**；binary 0 命中） |
| e2e canary 命令 | T05 需要 Tauri IPC canary（A10），尚未实现 | `#[cfg(feature="e2e-test")]` 注册 `r09_e2e_canary`（仅 test 构建）；release 调用返回 not found | 运行期（blocked）逐窗口调用断言 not found；release 扫描 forbidden 命令模式 |

---

## 8. sidecar：固定二进制 + 受限参数（无泛化 shell）

**布局（L1，D5）**：`bundle.externalBin: ["binaries/lingxi-service"]` → 构建后
`Contents/MacOS/lingxi-service`；构建清单 `bundle.resources` → `Contents/Resources/rust-service/build.json`。

**固定性校验（宿主 spawn 前，T02 `service/manifest.rs`）**：
- `build.json`：`schemaVersion=1`、platform/arch/target/toolchain/appVersion/binary、
  `sha256`（字节）、`contentSha256`（内容摘要，签名可重写区清零）、`sourceSha256`、
  `protocol {serverKind, wireProtocolMin=1, wireProtocolMax=1, dataEpoch=2}`；
- ad-hoc 签名后 `sha256` 必然变化：以 `contentSha256` 判内容不变 + `codesign --verify --strict`
  验 bundle；Win/Linux 用对应内容摘要/签名者规则（`rust-local-service.cjs:43-117` 移植）；
- 实测（release-scan-02）：sidecar `sha256=55d6edb1…` ≠ manifest `852912ef…`（预期，ad-hoc），
  但 `contentSha256=73471fbf…` 与重新计算结果**一致** → 内容未被替换。
- 参数面：宿主只传 `--home <LINGXI_HOME>`（固定 argv，无 renderer 输入）；sidecar 不接受任意
  URL/路径/命令；release 下禁止 env 替换 bin（T02 C04）。

**无泛化 shell**：
- 不引入 `tauri-plugin-shell`（Cargo.toml 无该依赖）；capability 无 shell/http/fs/process/updater 插件权限；
- 宿主 `std::process::Command` 调用点固定为 5 处（`instance.rs` Windows 私读 reader、`manifest.rs`
  ×2 `/usr/bin/codesign`、`shutdown.rs` `/bin/ps` 诊断、`supervisor.rs` 固定 sidecar），全部固定
  二进制 + 固定参数；扫描 case `release::process-spawn-sites-match-ledger` 以 ledger 形式防新增；
  `/bin/sh -c` 仅存在于 `#[cfg(test)]`（扫描前剥离测试模块）。
- skill zip 解压必须 Rust `zip` crate（§3.7），不得 exec `unzip`/`powershell`。

---

## 9. 发布产物扫描（A12 静态半边）

工具：`scripts/rust-tauri/r09_release_attack_surface.py`（只读，不启动 app）。输入：policy
`securityBaseline` + 当前 `.app`。扫描前记录树 sha256，扫描后复算；产物变化则
`artifactChangedDuringScan=true` 并如实标注候选（不重试伪装稳定）。

最终实测（证据 `dev/t06/release-scan-05-freeze/`，交付 release host sha `17523a17…`，扫描期间稳定，45/45 PASS）：

> 首轮扫描（release-scan-01/02）曾发现真实红：`core:default` 聚合的 tray/menu/devtools grant、CSP 缺口、
> release 二进制含自动化 env sink、`allow-debug-*` ACL 残留。按 D11/leaf capability/CSP/构建卫生修复后复扫转绿；
> 历史红记录保留在 `release-scan-01/02` 目录，不回写。以下为最终结果：
- ① wdio/webdriver/`tauri-plugin-wdio` 字符串：host/sidecar **0**（PASS）；
- ② `devUrl`/`localhost:5173`/`127.0.0.1:5173`/`../dist-tauri-resources`：**0**；内嵌前端
  `../dist-renderer` 存在（PASS）；
- ③ devtools/test 控制面：Cargo 无 devtools feature、源码无 toggle、capability grant 最小
  （event leaves）→ PASS；交付二进制 `LINGXI_R09_AUTOMATION`/`HOST_DIALOG_SINK`/`debug_open_onboarding`/
  `__r09HostErrors` 均 **0**（D11 PASS）；
- ④ sidecar：Mach-O arm64、非脚本、externalBin 布局、build.json 字段/协议/内容摘要全部 PASS；
  `--home` 单参数 + 无 shell 插件 + spawn ledger PASS；
- ⑤ capability 静态：9 文件 grant 最小、无 debug 命令 grant（PASS）；
- ⑥ release ACL 命令面 71 ⊆ policy 意图面 71（含分段序列化兜底匹配）；无 releaseDenied/deferred/dropped/forbidden（PASS）；
- CSP：10 指令与 policy 精确一致（含 `base-uri/form-action`），无禁用子串（PASS，见 §5）；
- 签名：`codesign --verify --deep --strict` exit 0，签名 `adhoc`（BLOCKED-SIGNING-IDENTITY，不冒充正式分发）；
- 产物扫描期间稳定（`artifactChangedDuringScan=false`）。

**局限（如实）**：
- ACL 提取依赖 tauri-build 序列化布局（`__app-acl__` 标记 + 分段 printable run 拼接），
  升级 tauri/tauri-build 后需复核（提取不到 → `release::acl-segment-extracted` 红，不会假绿）；
- devtools “可达性”最终需要运行期 WebDriver/端口探针（A12 原文“安装后扫描端口+调用测试命令”），
  本轮禁止 GUI，故运行期部分 `blocked::`；
- 平台差异：扫描器当前按 macOS arm64 产物实现（BLOCKED-PLATFORM-* 其余平台）。

---

## 10. 运行期负向矩阵设计（GUI cleared 后执行）

由 policy 生成计划（生产者写出 `runtime-matrix-plan.json`，实测正路径 145 条 / 负路径 503 条）：

| 组 | 断言 | 实现方式 |
|---|---|---|
| 逐窗口负向 | 对每个窗口 label，遍历「全局命令面 − 该窗口 allowedCommands」（包含 dev-only/debug 命令），断言 `invoke` 抛 ACL deny（错误码/文案稳定），且宿主诊断无执行迹象 | `e2e-test` 构建 + `tauri-plugin-wdio-webdriver`；WebDriver 按窗口切换（或页面内 `window.__TAURI__.core.invoke`），每个窗口一个 session；失败即记录 window/command/实际结果 |
| 逐窗口正路径 | 每个窗口 allowedCommands 真实 renderer 路径可调用（至少 boot 必需命令 + 敏感命令的正路径） | 同上；正路径用真实 renderer hook，不 mock 被测宿主 |
| 敏感命令集合（A11 原文） | `protectedCommands`（token/文件写/外链/生命周期/browser 控制）逐窗口：仅 policy 声明的窗口可调用 | 负向矩阵过滤 + 正向；token 类命令额外在 main 以外窗口断言 deny |
| 叠加场景 | main 命中的两条 capability 叠加后，仅 main 授权的敏感命令在 settings/quick-chat 等窗口中 deny | 同上；记录 capability 命中列表 |
| 远程内容 | 远程/预览 iframe 内脚本调用 `window.__TAURI__`/`invoke`/读取 token 全部失败；CSP violation 记录 | A10 真机 webview 探针（T05 已用真实 Chrome 等价验证，Tauri 真机待 GUI 放行） |
| release canary | test 构建 canary 命令可用、release 调用 not found；release 不监听 WebDriver 端口 | 双产物：debug/e2e 构建与 release 包各一次；端口扫描（`lsof`/connect 探针） |
| A12 端口/测试接口 | release 安装后：无 WebDriver 监听端口；调用 debug/canary 命令全部 not found；无任意 Rust 执行接口 | 安装隔离副本（`/private/tmp`），`--automation` sink 关闭，逐项探针 |

启动纪律：**Lead 明确 cleared 之前禁止启动任何 Tauri app/GUI/安装包**；运行期套件
（`r09_t07_native_e2e` / T06 运行期部分）另行排期；每个套件 finally 断言无 app/服务/窗口残留。
运行期失败/阻塞不改变静态策略文件；若运行期发现新事实（如 Tauri 对 iframe 的 IPC 注入行为），
先修 policy/本设计再重跑。

---

## 11. Host patch 执行记录（D11 后由 T06 实现者接管写权；已完成项 + 剩余项）

| # | 文件/范围 | 变更 | 对应发现/验收 |
|---|---|---|---|
| P1 ✅ | `capabilities/*.json`（9 文件） | 删除 `core:default`；插件权限改为 `core:event:allow-listen/unlisten` leaf | F1/A11；audit plugin/dangerous 全 PASS |
| P2 ✅ | `capabilities/settings.json`、`quick-chat.json`、`splash.json`、`onboarding.json`（新）；viewer/browser-viewer 补齐 | 按 policy 逐窗口 allow-*；gapLedger 由 66→0 | A11 8 窗口并集==policy |
| P3 ✅ | `build.rs` AppManifest | 27 条新命令入 release manifest；debug 两条按 `CARGO_FEATURE_R09_TEST_HOOKS` 条件加入 | 四集合 71/71/71/71 |
| P4 ✅ | `src/lib.rs` invoke_handler（两个 cfg 变体）+ `src/native_commands.rs` | 71 命令实现；debug 命令仅 hooks；主窗 CloseRequested/move-resize 防抖接线 | 四集合一致；A11/A12 静态 PASS |
| P5 | `permissions/*` | 由 tauri-build 从 P3 自动生成，不手改 | — |
| P6 ✅ | `tauri.release.conf.json`（CSP） | 按 §5 收紧 10 指令（去 file:/data:/localhost:*，补 asset/blob/base-uri/form-action） | F2 已修；扫描 CSP 全 PASS |
| P7 ✅ | `app.security.assetProtocol`（release+base config） | enable=true、静态 scope=[]；`tauri` feature `protocol-asset` 启用；运行时按授权根 add 归 T03-FILE-01 | CSP asset: 对齐；D8 运行时接线仍 deferred |
| P8 ✅ | `src/host.rs`、`src/commands.rs`（D11） | 自动化 env/sink/隐藏窗/抑制弹窗/DOM 投影全部 `cfg(feature="r09-test-hooks")`；交付 release 不编译 | F5 已修；交付二进制 test env/symbol 0 命中 |
| P9 ✅ | `capabilities-test/r09-test-hooks.json` + wrapper staging | 仅 `--features r09-test-hooks` 构建时复制到 `capabilities/`，构建后清理；release 不含 | debug 命令 release 不可达（二进制/A​CL 0 命中） |
| P10 ✅ | `bundle.macOS.infoPlist` + `Info.plist` | 增加 `NSMicrophone/NSSpeechRecognitionUsageDescription` | T04/T07 真机准备；正式签名仍 BLOCKED |
| P11 ✅ | `scripts/tauri-build.mjs` | features/evidenceBuild/codesign 回执；自动清理陈旧 autogenerated 权限防 ACL 污染；`--refresh` 冻结候选元数据刷新；hooks capability staging | A11/A12 证据链 |
| P12 | `scripts/rust-tauri/r09_stage_inputs.py`/`R09.json` | 无需改动（T06 两个套件已注册）；host 合并后由 Lead 重生成地图并跑正式门 | — |

窗口 label 清单（`native/window_labels.rs`，policy 键逐字一致）：
`main`、`settings`、`quick-chat`、`browser-viewer`、`splash`、`onboarding`、`viewer-*`、
`skill-viewer-*`。新增任何窗口必须先改 `window_labels.rs` + policy + capability，否则 audit 红。

---

## 12. 平台差异与阻塞

| 平台 | WebView / 协议 | T06 差异 | 状态 |
|---|---|---|---|
| macOS arm64 | WKWebView；`tauri://localhost`、`asset://localhost`；ad-hoc 签名 | 本机唯一可执行平台；CSP `asset:`；devtools release 默认关；签名非正式分发 | 本机可验（静态已完成，运行期 blocked） |
| macOS x64 | WKWebView | 同 mac，但无真机/VM | BLOCKED-PLATFORM-MACOS-X64 |
| Windows x64 | WebView2；`http://ipc.localhost`、`http://asset.localhost` | CSP 需含 Windows 源；PE 内容摘要 + Authenticode 签名者匹配；Job Object 进程树；无真机 | BLOCKED-PLATFORM-WINDOWS-X64 |
| Linux x64 | WebKitGTK；`tauri://localhost`/`asset://localhost` | 无 codesign 等价物；AppImage/deb sidecar 路径；无真机 | BLOCKED-PLATFORM-LINUX-X64 |

签名/公证：无 Developer ID → `BLOCKED-SIGNING-IDENTITY`；release 本地 ad-hoc 可功能验证，
不声称正式分发签名/公证。T04 系统权限（麦克风/语音识别/屏幕录制）需真实签名身份与人工 TCC
操作 → `BLOCKED-SYSTEM-PERMISSIONS`。

---

## 13. 已知风险与后续

1. **F1-F5 均为真实红**（见 T06_PROGRESS §3），需 host-core 按 §11 patch 后重扫；本任务不直接改
   `desktop/src-tauri/**`（host-core 独占写）。
2. 运行期矩阵与 A12 “安装后端口扫描/调用测试命令”被 GUI 禁令阻塞（Lead 裁决）；解禁后由
   T06/T07 运行期套件执行，静态策略与扫描工具已就绪。
3. Tauri ACL 序列化布局不是稳定 API：升级 tauri/tauri-build 后必须重跑扫描（提取失败即红）。
4. `withGlobalTauri=true` 的同源 XSS 暴露面见 §3.10；R10 再评估。
5. viewer 持 token 的收敛见 §3.5（R10）。
6. asset protocol/File URL（D8）静态缺口由 T03/T05 接线，T06 只保证 CSP/scope 不与策略冲突。
7. 运行期若发现 Tauri 对 iframe/远程页面的 IPC 注入与预期不符，必须回写 §3.8/§10 并重验，
   不得以静态 PASS 代替。

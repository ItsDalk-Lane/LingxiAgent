# R09 T08 缺口修复：Tauri 宿主能力合并 + Rust/T06 v2 patch 清单

- 触发：T08 复核发现 `desktop/src/react/host/tauri-host-capabilities.ts`（T03/T04 44+ 成员片段）
  **从未被 `tauri-desktop-host.ts` import/merge**（全仓无引用）→ Tauri 下这些 PlatformApi 成员
  全部被 Proxy 拦截抛 `HostCapabilityError`；A16“每项真实可执行”不成立。候选 v1（app tree
  `2dc38bc3…`）作废，v2 重建。
- 本文件由 **r09-host-core** 产出；renderer 侧已改范围仅 `desktop/src/react/host/**` 与
  `desktop/src/react/__tests__/host/**`；**未碰** `desktop/src-tauri`（冻结，写权 T06）。
- 验证（复跑命令见 §6）：`npx tsc --noEmit` exit 0；`npx vitest run
  desktop/src/react/__tests__/host/r09-t01-tauri-desktop-host.test.tsx` 10/10 pass。

## 0. renderer 侧已修复（v2 候选将包含）

| 文件 | 改动 |
|---|---|
| `desktop/src/react/host/tauri-desktop-host.ts` | 正式 import 并**合并** `createTauriHostCapabilities`：`...fragment.api` 真实实现进入 `implemented`；boot 基线成员（getServerConnectionInfo/Port/Token、appReady、getPlatform、getAppVersion、onServerRestarted、openExternal）保持优先；`capabilityReason` 先查显式 no-op、再查 pendingRust、已实现返回 null、其余回退 fragment 理由表（不再泛化兜底）。`TauriGlobal.core.convertFileSrc?` 声明。 |
| 同上 | `getFileUrl(path)`：`convertFileSrc(path)` 真实返回 asset URL；无 `convertFileSrc` 时**抛 HostCapabilityError + `TAURI_FILE_URL_REASON`**（不 undefined/no-op）。 |
| 同上 | `onboardingComplete`：真实 invoke `onboarding_complete`，Rust 返回 NOT_IMPLEMENTED 时**抛 HostCapabilityError + `TAURI_ONBOARDING_REASON`**（登记待 T06 v2 实现）。 |
| 同上 | `installTauriContextMenuPolicy()`：与 Electron `main.cjs:490-507` 等价的 renderer 策略——非可编辑区域 `contextmenu.preventDefault()`，可编辑区域保留系统菜单（右键粘贴）；幂等安装，测试可重置。 |
| 同上 | Proxy 受限成员抛出的 reason 改为**具体理由**（fragment.capabilityReasons[name]），不再一律泛化。 |
| `__tests__/host/r09-t01-tauri-desktop-host.test.tsx` | 新增 4 个 T08 用例：片段真实 invoke/listen/convertFileSrc、getFileUrl 缺失显式错误、右键策略等价、onboarding NOT_IMPLEMENTED 登记。 |

### PlatformApi 覆盖数（109 成员，静态核对）

| 类别 | 数量 | 说明 |
|---|---|---|
| 真实实现（invoke/listen/convertFileSrc） | **48** | T03/T04 片段 43 + boot 基线 7（去重）+ getFileUrl/onboardingComplete |
| 显式 no-op + 理由 | **1** | `onShowSkillViewer`（空监听，理由登记） |
| 可选缺失 + 理由 | **6** | onTrainUpdateAvailable/Progress、trainUpdateStatus、setBrowserConnection、onTrainFallbackNotice、ackTrainFallbackNotice（消费侧 `?.()` 判空，理由见 noopReasons） |
| 显式受限（调用抛 HostCapabilityError + 具体理由） | **54** | T05 浏览器/文件 IO/观察导出/R10 更新链/平台契约限制；理由来自 `TAURI_CAPABILITY_REASONS` 或统一兜底 |
| 合计 | **109** | 0 个静默 undefined（除登记的可选缺失） |

## 1. 必须 patch：asset 协议 scope（`getFileUrl` 真实可用；host:file-url-asset）

**现状**：`desktop/src-tauri/tauri.conf.json` 与 `tauri.release.conf.json` 均为
`security.assetProtocol = { "enable": true, "scope": [] }`；release CSP 的 `img-src/media-src` 已含
`asset: http://asset.localhost`，但**基础 `tauri.conf.json` 的 CSP（dev/非 release 合并前）**
`img-src 'self' data:` 不含 asset；scope 为空时任何 `convertFileSrc` URL 都会被 asset protocol 拒绝。

**Patch A1** — `desktop/src-tauri/tauri.conf.json` → `app.security`：

```json
"assetProtocol": {
  "enable": true,
  "scope": [
    "$HOME/.lingxi/**",
    "$HOME/.lingxi-dev/**",
    "$HOME/.hanako-dev/**",
    "$TEMP/**"
  ]
}
```

> scope 根必须覆盖 R06 实际路径：LINGXI_HOME（头像 `avatars/`、附件、预览缓存、截图）与临时目录
> （screenshot.ts）；用户工作区若可配置在 home 之外，T06 需在 `host.rs` setup 用 Rust
> `app.asset_protocol_scope().allow_directory(workspace, recursive)` 做**动态授权**（只加
> 已验证的真实 workspace 根，不得 `/**` 全盘）。

**Patch A2** — `desktop/src-tauri/tauri.conf.json` → `app.security.csp` `img-src`/`media-src`
补 `asset: http://asset.localhost`；`connect-src` 不动：

```
img-src 'self' data: asset: http://asset.localhost;
media-src 'self' data: blob: asset: http://asset.localhost;
```

**Patch A3** — `desktop/src-tauri/tauri.release.conf.json` → 同 A1 scope（当前 `[]`）；CSP 已含
asset/localhost，无需改。

**Patch A4** — `desktop/src-tauri/tauri.dev.conf.json`：当前无 security 块（继承基础 conf，
若 T06 为 dev 覆盖 CSP，需同步含 asset）。
**验证**：wdio 内 `getFileUrl` 指向真实 LINGXI_HOME 图片 → `<img>` onload 成功；越界路径
（如 `/etc/passwd`）asset 加载失败（A10 负向）；页面无 CSP violation。
**平台**：mac/mac-arm64 用 `asset:`；Windows 用 `http://asset.localhost`；Linux `asset://localhost`
（Windows/Linux 未实测 → 保留 BLOCKED_PLATFORM 口径）。

## 2. 必须 patch：右键菜单策略 Rust/配置残留（host:context-menu-policy）

**renderer 已实现等价策略**（§0）。Rust 侧无需新增命令；需 T06 确认/补齐：

| 位置 | 内容 | 原因 |
|---|---|---|
| `desktop/src-tauri/tauri.conf.json` `app.windows[*].devtools` | **release 必须 false**；仅 dev/debug 允许 true | 防 release 出现 devtools 右键/菜单入口 |
| `desktop/src-tauri/src/native/install.rs` / `host.rs` | 不注入任何原生 WebKit 上下文菜单；确认无 `MenuBuilder` popup 绑定到 webview 右键 | 与 Electron `main.cjs:490-507` 行为对齐 |
| 运行期验证（v2 visible） | 非编辑区域右键**无默认菜单**；`textarea/input/contenteditable` 右键有系统粘贴菜单 | A13/手动+C GEvent 截图；WebView2/Linux 差异记平台矩阵 |

> 备注：Tauri/WKWebView 默认菜单受系统控制；renderer `preventDefault` 已覆盖非编辑区；
> 编辑区保留系统菜单是**期望行为**（右键粘贴可用），不得用全局 preventDefault 破坏。

## 3. `onboarding_complete`：按 Lead v2 裁决**不实施**，保持 NOT_IMPLEMENTED 并登记

**v2 裁决（Lead）**：onboarding 不实施，Rust 保持 `E_NOT_IMPLEMENTED`；renderer 已显式登记
（`TAURI_ONBOARDING_REASON`，调用抛 `HostCapabilityError`）。T06 v2 只需把它登记进
`docs/rust-tauri/R09/BLOCKED.md`（或 `T06_PROGRESS.md` 的 deferred/blocked 条目），
让 A16 placeholder 扫描的 `NOT_IMPLEMENTED-unregistered` 归零；**不需要**实现 §3 以下内容（保留为未来范围）。

**现状**：`desktop/src-tauri/src/native/onboarding.rs:55-80`
`plan_onboarding_complete(DesktopRuntime::RustService)` 固定返回
`E_NOT_IMPLEMENTED "Rust desktop onboarding completion is unavailable..."`；
`CompletePlan::SubmitThenOpenMain` 已定义但只用于 NodeServer。未来实现（非 v2）：

| 位置 | Patch | 原因 |
|---|---|---|
| `native/onboarding.rs:55-80`（**未来范围，非 v2**） | `DesktopRuntime::RustService` 返回 `SubmitThenOpenMain { require_setup_complete_true: true }`，并按 `COMPLETE_STEPS` 执行：`post-setup-complete`（Rust 服务 preferences 路由，持久化 `setupComplete=true`）→ `validate-ok-and-setup-complete`（缺一不进入主界面）→ `create-main-window`（host.rs 既有 READY 后建主窗路径，需暴露 helper）→ `register-quick-chat-shortcut`（`install::register_quick_chat_shortcuts`） | A13/A16：真实完成路径，不得伪造 |
| `native_commands.rs` / `commands.rs` 的 `onboarding_complete` glue | 调 `plan_onboarding_complete` 后执行计划；失败返回显式错误码 `E_NOT_IMPLEMENTED` 替换为真实错误 | 当前 renderer 已登记显式受限；T06 实现后自动转真实 |
| `capabilities/splash.json`、`capabilities/onboarding.json` | 确认 `allow-onboarding-complete` 存在（onboarding 已有；splash 若由 Splash 触发需补） | 命令 ACL |
| Rust 服务 preferences 路由（未来范围） | 核实 Rust 服务存在等价 `setupComplete` 持久化路由；无则 T06 补（不得写 Electron user/preferences.json 冒充） | 数据归属 |
| v2 必须补 | `docs/rust-tauri/R09/BLOCKED.md` 或 `T06_PROGRESS.md` 增加 `onboarding_complete` 的 NOT_IMPLEMENTED/deferred 登记条目（含 reason 与证据） | A16 placeholder 扫描要求；否则 `host::no-unregistered-placement-gaps` 红 |

**renderer 侧**：已按“T06 实现后自动成功”设计（invoke 成功即完成；失败抛 HostCapabilityError+reason），
**v2 不需要再改 renderer**。

## 4. 必须 patch：capability allow-* 与 policy 命令清单（T06）

### 4.1 `desktop/src-tauri/capabilities/r09-host.json`（main 窗口）

片段在 main 窗口真实调用的命令中，当前缺 allow 条目的（按 capability 现表核对）：

| 缺少 | 建议追加 | 依据 |
|---|---|---|
| select_folder | `allow-select-folder` | 片段 selectFolder → invoke('select_folder')；main 设置/文件 UI |
| select_files | `allow-select-files` | selectFiles（默认 multiple=true） |
| open_file | `allow-open-file` | 片段 openFile |
| trash_item | `allow-trash-item` | 片段 trashItem（返回 boolean） |
| run_edit_command | `allow-run-edit-command` | 片段 runEditCommand（4 允许值，Rust 侧校验） |
| quick_chat_open_session | `allow-quick-chat-open-session` | 片段 quickChatOpenSession（ack 条目已有） |
| get_avatar_path / get_splash_info | **先核对消费者窗口**：SplashApp 在 splash 窗口、主窗不调用则**不加**（splash/onboarding capability 已有） | 避免越权 |
| onboarding_complete | **不加到 main**：Rust `authorize_onboarding_complete` 只允许 splash/onboarding | 与 Rust 门一致 |

### 4.2 其他窗口（如对应 React 页面调用片段成员）

`settings.json` 已在 4.1 表的条目外还需检查 `open_avatar`/`show_notification`/`start_drag`/
`trash_item`/`reload_main_window`/`quick_chat_*` 等；`quick-chat.json`/`onboarding.json`/
`splash.json` 同理。**方法**：v2 构建后跑
`python3 -B scripts/rust-tauri/r09_capability_audit.py --evidence <v2-evidence>`，
按实际消费者补；本表给 main 的确定缺口，其他窗口以 audit + T06 policy 为准，不凭猜。

### 4.3 `docs/rust-tauri/R09/R09_COMMAND_POLICY.json`

- `windows.main.allowedCommands` 追加（与 4.1 一致）：`select_folder`, `select_files`,
  `open_file`, `trash_item`, `run_edit_command`, `quick_chat_open_session`。
- `get_avatar_path`/`get_splash_info` 是否入 main：按真实消费者核对（若 main 的 `getSplashInfo`
  仅 splash 使用则只留 splash/onboarding）；`onboarding_complete` 只在 splash/onboarding。
- 命令已由 T06 注册（71 条）且 Rust authorize 门存在，本 patch 只补 ACL/policy 一致性，不新增命令。

### 4.4 事件面（已确认无需 patch）

Rust 已 emit：`window-maximized`/`window-unmaximized`（`native/window_controls.rs`）、
`settings-changed`（`native/settings_broadcast.rs`+`native_commands.rs`）、
`open-settings-modal`（`install.rs`/`settings_window.rs`）、`settings-switch-tab`（`settings_window.rs`）、
`quick-chat-open-session`（`quick_chat_handoff.rs`）、`quick-chat-shown`（`quick_chat.rs`）。
片段监听名与之一致。

## 5. T06 操作顺序（v2 候选）

1. §1/§2/§3/§4 patch；2. renderer 重建（包含本次 `host/**` 改动）；3. `cargo fmt/check --
   features r09-host` 与 T06 单测；4. capability audit PASS；5. 生成 delivery release + evidence
   构建（含 `e2e-test` 的 wrapper）并交 T07 只读消费重跑 G1-hidden；6. visible A/B/C；7. 最终门。

## 6. 验证命令（renderer，不重建 app/GUI）

```bash
npx tsc --noEmit                                                     # exit 0
npx vitest run desktop/src/react/__tests__/host/r09-t01-tauri-desktop-host.test.tsx  # 10/10 pass
```

## 7. v2 重跑命令与 T08 输入变化（r09-host-core，v2 候选就绪后执行）

### 7.1 T08 生产者已按 v2 动态化（只读消费，不构建）

- `scripts/rust-tauri/r09_t08.py` 新增 candidate handoff 动态加载：优先
  `LINGXI_R09_T08_HANDOFF` env → `artifacts/rust-tauri/R09/dev/t06/candidate-handoff-v2/candidate-handoff.json`
  → 回退 v1 `candidate-handoff-02`；映射 delivery（app tree/主 exe/sidecar/build.json/protocol/
  features/receipt）、G1 evidence（exe sha/features/receipt/wrapper sha）、renderer
  digest/theme/fileCount；`fixedFactors.candidate` + `host-matrix.candidate` 记录版本/来源/gitHead。
  **v2 handoff 文件名/路径若不同，请用 `LINGXI_R09_T08_HANDOFF` 指向实际文件。**
- `renderer.fileCount` 若 v2 handoff 未提供 → 按磁盘实测记录（build-once-bind-once），
  不再硬编码 644。
- shell_parity 的 renderer/service sha 字段取自动态 FROZEN（v2 handoff），不得手填 v1 值。
- `host-matrix.json` 新增 `platformApi` 段：109 成员逐项 `classification/realExecutable/capabilityReason`；
  summary = **48 real / 1 noop / 6 absent-optional / 54 restricted-reasoned / 0 silent**。
- A16 context-menu 真实行为断言：`a16::context-menu-policy.real-behavior` 运行
  `npx vitest run ... -t 右键策略`（真实 DOM 事件：非编辑区 defaultPrevented=true、
  textarea defaultPrevented=false），日志 `context-menu-behavior.log`。
- `host:file-url-asset` / `host:context-menu-policy` 证据映射到 T07 G1 新用例
  `t07::native-e2e.file-url-asset` / `t07::native-e2e.context-menu-policy`；
  file-url 的 a16Ok 仍要求运行期证据（scope 命中 + 内容校验），静态 scope 存在不算通过。

### 7.2 v2 候选就绪后命令（按顺序；T08 为只读聚合）

```bash
# 0) 指向 v2 handoff（若 T06 输出路径/名称不同）
export LINGXI_R09_T08_HANDOFF=artifacts/rust-tauri/R09/dev/t06/candidate-handoff-v2/candidate-handoff.json

# 1) T08 shell parity（A15，只读聚合；需要 v2 renderer/delivery/G1 evidence 已落盘）
python3 -B scripts/rust-tauri/r09_t08.py --suite r09_t08_shell_parity \
  --evidence artifacts/rust-tauri/R09/dev/t08/<v2-run>/01-shell-parity

# 2) T08 host completeness（A16，只读；含 platformApi matrix + context-menu 行为）
python3 -B scripts/rust-tauri/r09_t08.py --suite r09_t08_host_completeness \
  --evidence artifacts/rust-tauri/R09/dev/t08/<v2-run>/02-host-completeness

# 3) T07 G1-hidden（需 Lead cleared；wrapper 只读消费 v2 evidence exe）
LINGXI_R09_ALLOW_GUI=1 \
LINGXI_R09_T07_APP_BIN=/private/tmp/lingxi-r09-t07/<v2-wrapper>/Lingxi.app/Contents/MacOS/lingxi-resource-desktop \
LINGXI_R09_T07_E2E_RECEIPT=desktop/src-tauri/.tauri-build/build-receipt-debug.json \
python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t07_native_e2e \
  --evidence artifacts/rust-tauri/R09/dev/t07/<v2-run>
```

期望：T07 G1 26+2 cases（新增 `file-url-asset` 与 `context-menu-policy`）；T08 host matrix
`platformApi.summary` 与上文一致、`a16::context-menu-policy.real-behavior` PASS、
`host::file-url-asset` 仅在 T07 运行期证据 PASS 后 a16Ok=true。

## 8. v3 renderer 补全（T08 A16 六能力族；只改 `desktop/src/react/host/**`；等待 T06 handoff-v3）

### 8.1 已接线（映射现有 Rust 命令/事件，均为真实 invoke/listen）

| 能力族 | 成员 | Rust 映射 |
|---|---|---|
| file-text-io | readFile/readFileSnapshot/readFileBase64/writeFile/writeFileBinary/writeFileIfUnchanged/copyFile/watchFile/unwatchFile/onFileChanged | `read_file`/`read_file_snapshot`/`read_file_base64`/`write_file`(path,content)/`write_file_binary`(path,data)/`write_file_if_unchanged`(path,content,expectedVersion)/`copy_file`(source,destination)/`watch_file`/`unwatch_file` + `file-changed`{filePath} |
| detached-viewer | spawnViewer/viewerRequestLoad/viewerClose | `spawn_viewer`{path}→label（`viewer-<n>`→数字 windowId）；`viewer_request_load`（camelCase 归一 + type 由扩展名推导 + windowId 由当前 label 派生）；`viewer_close` |
| skill-viewer | openSkillViewer/listSkillFiles/readSkillFile | `open_skill_viewer`{data:{name,baseDir,filePath,installed}}；`list_skill_files`{baseDir}；`read_skill_file`{path} |
| browser-viewer | setBrowserConnection/getBrowserViewerContext/openBrowserViewer/rustBrowserRequest/closeBrowserViewer/onBrowserUpdate | `set_browser_connection`{connection:...}（返回 key 存宿主闭包）；`get_browser_viewer_context`；`open_browser_viewer`{target}（缺失 connectionKey 用已存 key 补齐）；`rust_browser_request`{target,action,options}；`close_browser_viewer`；`onBrowserUpdate` 监听 `browser:connection-changed` → `{running,reason}`（部分更新，见 8.2） |
| file-watch | watchFile/unwatchFile | 同 file-text-io（Rust `watch_file`/`unwatch_file`） |
| file-drop-paths | getFilePath | Tauri `webviewWindow.getCurrentWebviewWindow().onDragDropEvent`（`drop.payload.paths` 绝对路径）→ 宿主 FIFO，`getFilePath(file)` 按顺序返回（5s 窗口，否则 null；不伪造） |
| workspace（v3.1） | watchWorkspace/unwatchWorkspace/onWorkspaceChanged | `watch_workspace`{rootPath}/`unwatch_workspace`{rootPath} + `workspace-changed`{rootPath,changedPath,affectedDir,eventType} |
| detached-viewer（v3.1） | onViewerClosed | `viewer-closed`（主窗事件，payload 数字 windowId） |
| skill-viewer（v3.1） | onShowSkillViewer | `show-skill-viewer`{name,baseDir,installed[,filePath]}（open_skill_viewer 改为向主窗 emit） |

宿主新增注入：`currentWindowLabel`、`browserConnectionKey/setBrowserConnectionKey`、`getFilePath`
（`TauriGlobal.webviewWindow?` 声明）；`TauriHostCapabilityDeps` 同步扩展。

### 8.2 仍需 T06 v3 Rust patch（renderer 已显式受限，不 no-op）

| # | 缺口 | 建议 patch | 影响成员 |
|---|---|---|---|



| 4 | `browser-update` 完整流 **deferred**（Lead 裁决；当前仅 `browser:connection-changed`） | 宿主把浏览器服务事件/tabs/nav 状态转 `browser-update`（或提供 poll 命令）；renderer 已按 `browser:connection-changed` 映射 running/reason 部分 | onBrowserUpdate（部分，已登记 deferred） |

| 6 | `native/file_drop.rs`/`file_drop_paths` 命令面 **deferred**（Lead 裁决；renderer drop FIFO 已可用） | 若未来要更稳 File↔path 关联可再加命令/事件 | getFilePath（当前可用，已登记 deferred） |
| 7 | capability allow 联合集 | 新接线命令在 main（及 viewer/skill-viewer/browser-viewer 按窗口）需 capability 联合授予；T06 audit 逐窗口核对：read-file/write-file*/copy-file/watch-file/unwatch-file/spawn-viewer/viewer-request-load/viewer-close/open-skill-viewer/list-skill-files/read-skill-file/set-browser-connection/get-browser-viewer-context/open-browser-viewer/rust-browser-request/close-browser-viewer | 全部 |
| 8 | 语义 note（非阻塞） | `spawn_viewer` 返回 label（adapter 解析数字）；`ViewerPayload` 无 type/language（adapter 推导）；如 T06 愿规范化可加字段 | detached-viewer |

### 8.3 覆盖数字（v3.1 renderer final）

PlatformApi 109：**真实实现 76（静态解析 75 + getFilePath 运行期条件注入 1）/ 显式 no-op 0 / 可选缺失+理由 6 / 显式受限+理由 27**（T05 浏览器细粒度命令、R10 更新链、observability 导出、DOCX/XLSX 预览、卡片导航守卫、语音 TCC 等仍显式受限+理由）。
v3.1 补齐 5 成员：watchWorkspace/unwatchWorkspace/onWorkspaceChanged/onViewerClosed/onShowSkillViewer（T06 v3 Rust 命令/事件已实现）。
验证：`npx tsc --noEmit` exit 0；`npx vitest run desktop/src/react/__tests__/host/r09-t01-tauri-desktop-host.test.tsx` **16/16 pass**（新增 v3.1 用例：workspace invoke/listen、viewer-closed 数字 id、show-skill-viewer payload）。

> v3 裁决（Lead）：完整 `browser-update` 流与 `file_drop_paths` 命令面由 T06 登记 deferred/blocked；renderer 当前保持
> `browser:connection-changed` 部分映射与 drag-drop 队列映射（可用、显式理由），v3 handoff 后按实际登记更新理由。

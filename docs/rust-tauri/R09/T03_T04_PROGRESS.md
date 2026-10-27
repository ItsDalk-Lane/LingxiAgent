# R09-T03/T04 实现进度与注册交接（2026-10-26，r09-native-caps）

状态：**IN_PROGRESS / 门真红**。本文件是 T03（桌面基础能力等价）+ T04（语音/屏幕/辅助功能权限）
的实现者交付说明：已完成文件、可复跑验证、证据、**host-core 注册清单**、建议 assertion_cases 与
未完成/阻塞项。任何「PASS」措辞仅指本文件列出的可独立验证项；真实系统动作与宿主注册后
端到端在完成前一律为 BLOCKED（套件 exit 2 = 门真红）。

写作用域遵守 Lead 2026-10-26 裁决：T03/T04 落点 = **`desktop/src-tauri/src/native/`**
（不叫 `host/`，不与 host-core 的 `src/host.rs`（T01/T02 生命周期）冲突）；本轮**未改**
`lib.rs` / `Cargo.toml` / `build.rs` / `tauri*.conf.json` / `capabilities/**` / `permissions/**` /
`package.json`/锁文件 / `xtask/stage_maps` / `r09_stage_inputs.py` / R00 冻结账。

---

## 1. 交付物清单（本轮新增，未提交）

| 路径 | 内容 | 规模 |
|---|---|---|
| `desktop/src-tauri/src/native/*.rs` | 37 个文件：`mod.rs` + 31 个能力/状态机模块 + `errors.rs`/`argument_guard.rs`/`ipc_negative_matrix.rs` + `install.rs`（注册安装面）+ `notification_center_macos.rs`（OQ-03 spike） | 约 12.4k 行（含 `#[cfg(test)]`） |
| `desktop/src/react/host/tauri-host-capabilities.ts` | DesktopHost-Tauri 能力片段：`createTauriHostCapabilities(deps)` → `{ api: Partial<PlatformApi>, capabilityReasons }`，供 host-core 的 `tauri-desktop-host.ts` 合并（`tsc --noEmit` exit 0） | 约 350 行 |
| `scripts/rust-tauri/r09_t03.py` | T03 两个套件生产者（`r09_t03_native_matrix` / `r09_t03_ipc_negative`） | 约 520 行 |
| `scripts/rust-tauri/r09_t04.py` | T04 两个套件生产者（`r09_t04_permission_matrix` / `r09_t04_remote_boundary`），含**真实 lingxi-service 进程 + 真实 HTTP** | 约 600 行 |
| `docs/rust-tauri/R09/T03_T04_REGISTRATION.md` | **host-core 可直接套用的注册补丁**：Cargo 依赖精确行、lib.rs 3 处 diff、build.rs 命令、capabilities 每窗口 allow-*、合并顺序 | 约 240 行 |
| `docs/rust-tauri/R09/T03_LEAF_CASE_PLAN.json` | **22 HOST_NATIVE + 8 HOST_BRIDGE 共 30 叶 / 60 条原断言的逐断言证据计划**：leafId/hostClass/assertionIndex/assertionText/existingLogicCases/plannedActionCase/requiredObservation/needsVisibleGUI/blockedReason + implementationStatus/manualSteps/neededHostSurface | 机器可读；`plan::*` 15 项质量门 + 1 项前置文件检查 |
| `scripts/rust-tauri/r09_gui_actions.py` | GUI-cleared 真实动作执行器：W3C WebDriver 驱动宿主命令 + CGWindowList/平均色/通知投递/CGEvent 快捷键/pmset/LaunchAgent 系统探针 + 人工证据槽 | 约 900 行（未在无 GUI 环境运行） |
| `artifacts/rust-tauri/R09/dev/t03/{native-matrix-01,ipc-negative-01,install-compile-01}/` | `result.json` + `cases.json` + `output.log` + `runtime.json` + 编译后的 harness / 真实 crate 编译测试回执 | 开发证据 |
| `artifacts/rust-tauri/R09/dev/t04/{permission-matrix-01,remote-boundary-01}/` | 同上 + `service-stdout.log`/`service-stderr.log` | 开发证据 |

`native/` 模块清单（每文件纯逻辑 + 单测；不写命令注册/插件 init）：
`app_info, app_ready, argument_guard, auto_launch, clipboard_edit, drag_out, errors, external_open,
file_dialogs, file_drop, global_shortcut, i18n, ipc_negative_matrix, keep_awake, notification,
onboarding, permissions, quick_chat, quick_chat_handoff, reload_restart, remote_boundary, screenshot,
settings_broadcast, settings_window, single_instance, speech, splash, theme_sync, trash, tray,
window_controls, window_labels, window_lifecycle, window_state`。

编译验证方式（不依赖 tauri、不改 lib.rs）：生产者把 `native/mod.rs` 作为独立 crate 根
`rustc --test --edition 2021` 编译为测试二进制；模块内 tauri/插件接线预留
`#[cfg(not(r09_standalone_test))]`（该 cfg 只在独立验证时设置，cargo 构建默认包含 glue）。
本轮**未写** tauri 命令 glue（属 host-core 注册面，按 §4 清单由 host-core 合并）。

## 2. 可复跑验证结果（真实执行，非声明）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `… --suite r09_t03_native_matrix --evidence artifacts/rust-tauri/R09/dev/t03/native-matrix-02` | **2（FAIL，门真红）** | cases.json 111：**81 PASS**、**30 `blocked::gui-cleared::<leafId>`**（receipt 112）；含 rustc 纯逻辑单测全绿 + **54 个 `leaf::*` 专属 case** + **15 项 `plan::*` 计划质量门**（叶集合=权威 30、22/8 分类、60 断言原文逐字一致、计划 case 命名/唯一、人工步骤、阻塞原因、逻辑 case 引用存在）+ 7 项跨文件一致性 + 2 项源码静态门；无 GUI env 时按叶诚实红 |
| `… --suite r09_t03_ipc_negative --evidence artifacts/rust-tauri/R09/dev/t03/ipc-negative-01` | **2（FAIL）** | cases.json 53：**50 PASS**（46 条 A06 负向矩阵逐 case + 静态 label 门/零 spawn + 2 进程证据排除）、3 `blocked::`（receipt 54）（需已注册宿主的真实 invoke/ACL/进程探针） |
| `… --suite r09_t04_permission_matrix --evidence artifacts/rust-tauri/R09/dev/t04/permission-matrix-01` | **2（FAIL）** | cases.json 40：**33 PASS**（15 个 Rust 状态机 case + 17 个真实服务 HTTP case + 服务退出无残留）、7 `blocked::`（receipt 41）（真实 TCC/helper/截图/ASR/注册后 E2E） |
| `… --suite r09_t04_remote_boundary --evidence artifacts/rust-tauri/R09/dev/t04/remote-boundary-01` | **2（FAIL）** | cases.json 23：**20 PASS**（8 个策略单测 + 未认证设备面拒绝 + 本机 owner 分界 + 无权限请求事件）、3 `blocked::`（receipt 24）（双宿主 E2E/真实设备链/TCC 撤销） |
| `node_modules/.bin/tsc --noEmit`（全仓） | **0** | 新增 TS 片段与 host-core 的 `tauri-desktop-host.ts` 均通过 |
| 临时 crate（tauri 2.12.1 + 5 插件）`cargo check` + `cargo test --lib` | **0 / 182 passed**（含 install.rs 2 tests + mac delegate class test） | `native/` 全部文件（含 `install.rs` 与 mac 通知 delegate）在真实 tauri/插件依赖下编译并通过测试；证据 `dev/t03/install-compile-01/`（含解析版本、源码 sha、toolchain） |

关键真实证据（不是 mock）：
- A06 负向矩阵 **46 条**逐 case 输出 `NEGATIVE_CASE <name> <command> <expect> <actual> OK`（生产者解析成 cases.json）；
- T04 用隔离 home 启动**真实** `rust/target/debug/lingxi-service`（sha 记在 result.json），Bearer 鉴权后真实
  `GET/PUT /lingxi/v1/preferences/computer-use`、`POST/DELETE .../approvals`、负向 `request-permissions`
  （provider 不匹配 400、数组 body 400，**不触发 TCC 弹窗**）、`GET /lingxi/v1/app-events` 观测到
  `computer-use-settings-changed` 事件；服务当前诚实声明 `provider.status.available=false,
  reason="computer_control_engine_unavailable"`（helper 未实现，未冒充可控制）。

### 2.1 OQ-03（通知点击聚焦）有界 spike 结论：**可行，已实现待真实点击验证**

- 不可行路径（已排除）：`tauri-plugin-notification` 桌面走 `notify-rust`（`NSUserNotificationCenter`
  旧路径），点击只有**阻塞式** `NotificationHandle::wait_for_response`（要求主 run loop 泵消息），
  且插件 API 不暴露 handle → 无法在插件之上接点击。
- 可行路径（已实现）：宿主 **不用插件展示**，直接 `UNUserNotificationCenter`（objc2-user-notifications
  0.3.2）：`install_click_handler` 安装 delegate，`willPresent` 返回前台展示选项，
  `didReceiveNotificationResponse` → `focus_main(app)`（激活 app + 显示/聚焦主窗）；展示走
  `addNotificationRequest`。代码：`native/notification_center_macos.rs`；接入：
  `native/install.rs::install`（`install_click_handler`）与 `show_notification`（mac 优先用该路径）。
- 证据边界：编译 + delegate 注册在临时 crate 验证通过（182 tests）；**真实点击后的聚焦必须等
  host-core 注册后人工/GUI 点击验证**，未验证前 A05/叶不得声称通过（见 §6 T03-NOTIFY-02）。
- 新增依赖：`objc2-foundation=0.3.2`、`objc2-user-notifications=0.3.2`、`block2=0.6.2`。

### 2.2 GUI-cleared 真实动作批次（T03 30 叶）

- **门控**：`LINGXI_R09_GUI_CLEARED=1` + `LINGXI_R09_TAURI_APP=<e2e-test 构建 .app/可执行>` +
  `LINGXI_R09_WEBDRIVER_PORT=<tauri-plugin-wdio-webdriver 端口>`；人工断言另需
  `LINGXI_R09_MANUAL_EVIDENCE_DIR=<dir>`。
- **无 env（当前）**：按 30 叶记 `blocked::gui-cleared::<leafId>`，exit 2（诚实红），不冒充通过。
- **有 env**：`r09_gui_actions.run_gui_plan` 启动真实宿主 → WebDriver `execute/sync` 调
  `window.__TAURI__.core.invoke` 驱动宿主命令 → 用系统探针观察真实副作用（CGWindowList 窗口存在/
  bounds/可见性、窗口截图平均色、UNUserNotificationCenter 已投递通知、CGEvent 合成快捷键、
  `pmset -g assertions`、LaunchAgent plist、`ps` 进程树/重启 PID）；断言通过才 PASS。
- **人工证据槽**：计划中 `implementationStatus=manual` 的 3 条断言（托盘菜单语言、通知点击聚焦、
  拖出到 Finder）读 `LINGXI_R09_MANUAL_EVIDENCE_DIR/<case>.json`，schema
  `lingxi.r09.manual-observation.v1`；缺失证据记 `manual-pending`（仍红），不冒充自动化通过。
- **计划统计**：30 叶 / 60 断言 = 49 条已实现 driver + **3 条人工**（`R00-T02-LA-15DFB1AED8F2#1`
  拖出到 Finder、`R00-T02-LA-BEC2074CC33C#1` 托盘菜单语言、`R00-T02-LA-DF4B5168ABB5#1` 通知点击聚焦）
  + **8 条 blocked**（onboarding 服务路由 4 + DOCX/XLSX 命令 4）；每条断言均有唯一
  `plannedActionCase`（60/60）与 `requiredObservation`，逻辑 case 不能单独支撑动作断言。
- **本文件写入时未启动 GUI / 未构建 desktop/src-tauri**：GUI 路径代码未在本机执行过，
  仅做语法/导入/计划质量门验证；visible 批次首次运行的真实结果以当时 evidence 为准。

## 3. 能力矩阵落点与状态（50 能力中 T03/T04 范围）

| 能力（清点 ID） | 模块 | 可独立验证 | 状态 |
|---|---|---|---|
| `host:single-instance` | `single_instance.rs` | home 命名空间隔离/第二实例计划/renderer 不得指定 home | ✅ 纯逻辑；真实双进程需注册后 |
| `host:main-window`, `host:app-lifecycle-quit` | `window_lifecycle.rs` | 关闭到托盘/彻底退出/owned-only 清理/窗口全关/activate | ✅ 纯逻辑；真实进程需注册后 |
| `host:window-state` | `window_state.rs` | sanitize 移植（多显示器/越界居中/最大化）+ 路径常量 | ✅ 纯逻辑；窗口事件接线需注册后 |
| `host:window-controls` | `window_controls.rs` | 调用窗口门（main/settings）/close 路由/toggle/事件名/拒绝 window id | ✅ 纯逻辑；真实窗口需注册后 |
| `host:app-ready` | `app_ready.rs` | 主窗门/幂等/30s 兜底/hidden-at-login/200ms 关 splash | ✅ 纯逻辑；真实时序需注册后 |
| `host:theme-sync` | `theme_sync.rs` | 11 主题 + auto/别名/深色判定 + quick-chat 透明特例 + 调用窗口门 | ✅ 纯逻辑 + 与 registry 逐项一致 |
| `host:i18n-main` | `i18n.rs` | 5 语言归一 + 托盘文案逐字一致 + locale 变更重建菜单 | ✅ 纯逻辑 + 与 locales 逐字一致 |
| `host:reload-restart` | `reload_restart.rs` | 仅主窗 reload/重启窗口门/更新中禁重入/拒绝 argv | ✅ 纯逻辑 |
| `host:tray` | `tray.rs` | 4 项菜单模型（show/settings/—/quit）/事件 id→动作/图标候选/tooltip；**「修复组件…」按 Lead 裁决移除并登记**（T03-REPAIR-02） | ✅ 纯逻辑；真实托盘图标需注册后 |
| `host:global-shortcut` | `global_shortcut.rs` | 键位归一/校验（≤3 组）/注册差异计划/冲突回执/test-register 清理/状态形状 | ✅ 纯逻辑；OS 注册需注册后 |
| `host:notification` | `notification.rs` + `notification_center_macos.rs` | focus 抑制/长度与 agentId 校验/真实送达回执语义；**点击聚焦 mac 已实现（UNUserNotificationCenter delegate，待真实点击验证）**，win/linux 显式 Unsupported（OQ-03） | ✅ 纯逻辑 + mac 实现编译；真实投递/点击需注册后 |
| `host:keep-awake` | `keep_awake.rs` | 状态机/幂等/失败不假报/Linux 显式 unsupported/查询失败 fail-closed | ✅ 纯逻辑；IOPMAssertion 适配需注册后 |
| `host:auto-launch` | `auto_launch.rs` | 平台支持/openedAtLogin 三平台规则/set 后重读/调用窗门 | ✅ 纯逻辑；SMAppService 受签名阻塞 |
| `host:file-dialogs` | `file_dialogs.rs` | 三对话框选项/取消形状/绝对路径归一/技能扩展名/调用窗门 | ✅ 纯逻辑；真实面板需注册后 |
| `host:clipboard-edit-command` | `clipboard_edit.rs` | 4 值白名单/未知命令拒绝/调用窗门/mac selector | ✅ 纯逻辑 |
| `host:drag-out` | `drag_out.rs` | 绝对路径/数量/单多形状/去重/调用窗门 | ✅ 纯逻辑；真实拖拽需注册后 |
| `host:file-drop-paths` | `file_drop.rs` | Tauri 原生绝对路径/URI 拒绝/数量/调用窗门 + getFilePath 不可用契约 | ✅ 纯逻辑；事件适配需注册后 |
| `host:external-open` | `external_open.rs` | 仅 http/https/打开文件/目录/Finder 类型门/导航外链判定/回执形状 | ✅ 纯逻辑；系统调用需注册后 |
| `host:trash` | `trash.rs` | 平台绝对路径（含 win 语义）/存在性/主窗 only | ✅ 纯逻辑；R06 trash 命令复用 |
| `host:settings-window` | `settings_window.rs` | 主窗优先 modal/崩溃回退/独立窗 focus+切 tab/tab 语法门/尺寸 | ✅ 纯逻辑；窗口创建需注册后 |
| `host:settings-broadcast` | `settings_broadcast.rs` | 18 事件白名单/载荷必填项/5 项真实副作用计划/fanout/sender 门 | ✅ 纯逻辑；副作用执行需注册后 |
| `host:quick-chat-window` | `quick_chat.rs` | 常量/模式/高度只增不缩/bounds 恢复/状态合并/show·hide 计划/Linux 透明窗显式 unsupported | ✅ 纯逻辑；窗口创建需注册后 |
| `host:quick-chat-session-handoff` | `quick_chat_handoff.rs` | 目标校验/可信 sender/pending 顶替/15s/ack 目标一致/焦点确认后 hide | ✅ 纯逻辑；事件/焦点轮询需注册后 |
| `host:app-info-platform` | `app_info.rs` | Tauri 平台名→Node 字面值映射/壳版本语义/版本校验 | ✅ 纯逻辑 |
| `host:splash` | `splash.rs` | splash/onboarding 窗口+页面来源门/头像路径 scope/格式与大小/信息归一 | ✅ 纯逻辑；YAML/解码需注册后 |
| `host:onboarding` | `onboarding.rs` | Rust 服务模式**显式 unsupported**（与现役 Electron 一致）/保存成功才建主窗/debug 仅 dev/preview 无副作用 | ✅ 纯逻辑；Node 服务路径需宿主决定 |
| `host:speech-permission`（T04） | `speech.rs` | 六态映射/只读不弹窗/单飞/开发壳不弹/终结态不重复/dispose fail-closed/采集取消·撤销·超时·设备缺失/UI 状态 | ✅ 纯逻辑；TCC 原生桥需注册后 + 人工授权 |
| T04 系统权限 + helper 租约 | `permissions.rs` | TCC 查询/请求状态机 + 应用批准去重 + 租约（批准/过期/控件换代/撤权/远端会话拒绝）+ 动作门 | ✅ 纯逻辑；真实 TCC/输入引擎不可用（服务诚实声明） |
| T04 远端设备边界（A08） | `remote_boundary.rs` | 远端无 grant → 要求本机授权/远端不能自授权/grant TTL·范围/与 TCC 合取/审计 | ✅ 纯逻辑 + 真实服务鉴权边 |
| T04 截图渲染 | `screenshot.rs` | 6 主题/分段索引/保存目录 scope/文件名/结果形状 | ⚠️ 仅策略层；**捕获执行 BLOCKED（OQ-04）** |
| `host:context-menu-policy`（T06） | — | 未实现（T06 范围） | ⛔ 非本轮 |
| `host:card-frame-navigation-guard`（T06） | — | 保持 flag=false + 同源降级（OQ-07） | ⛔ T06 |
| `host:browser-viewer` | — | T05 范围（`browser_viewer.rs` host-core 已合并） | ⛔ T05 |
| `host:file-text-io` / `file-watch` / `docx-xlsx-preview` / `observability-export` / `file-url-asset` / `detached-viewer` / `skill-viewer` | — | R06 已有部分命令可复用；T03 本轮未写独立模块（见 §7 未完成） | ⛔ 下一轮 |
| `host:server-connection-info` | — | T02 supervisor 持有（host-core） | ⛔ T02 |
| `host:update-github-check` / `host:update-auto` / `host:update-train-ota` | — | R10 验收；TS 片段已登记显式理由 | ⛔ R10 |
| 7 项 `security:*` | — | T06 收口 | ⛔ T06 |

## 4. host-core 注册清单（**可直接套用版本见 `T03_T04_REGISTRATION.md`**）

> 2026-10-26 第二轮新增：§4 的 diff 级补丁已整理进 `T03_T04_REGISTRATION.md`（含 Cargo 精确行、
> lib.rs 3 处 diff、build.rs 命令、capabilities 每窗口 allow-*、合并顺序、mac Info.plist）。
> 以下 §4.1–4.6 保留为背景与签名契约。

### 4.1 Cargo.toml（精确版本；host-core 统一写入并更新锁）

```toml
tauri = { version = "=2.12.1", features = ["image-png", "tray-icon"] }  # 现有依赖追加 features
tauri-plugin-single-instance = "=2.5.4"   # tauri ^2.12；Rust setup 注册，前端零权限
tauri-plugin-global-shortcut = "=2.4.1"   # tauri ^2.12；仅 Rust 侧
tauri-plugin-notification = "=2.7.0"      # tauri ^2.12；仅 Rust 侧
tauri-plugin-autostart = "=2.7.0"         # tauri ^2.12；MacosLauncher::LaunchAgent
block2 = "=0.6.2"                         # objc2 0.6 配套：Speech TCC requestAuthorization 的 completion block
# 不需要：tauri-plugin-window-state（自管 LINGXI_HOME/user/window-state.json）
# 不需要：tauri-plugin-opener（open crate 已用于 openExternal/openFolder/openFile；
#         showInFinder 用 objc2=0.6.4 NSWorkspace selectFile:inFileViewerRootedAtPath:）
```

`[lints.rust]`（或等价 build.rs 声明）建议加：

```toml
[lints.rust]
unexpected_cfgs = { level = "allow", check-cfg = ['cfg(r09_standalone_test)'] }
```

（`r09_standalone_test` 只由 `r09_t03.py`/`r09_t04.py` 的独立 rustc 验证设置；cargo 构建不设置，
glue 正常包含。若不加 lint 配置，Rust 1.98 可能对未知 cfg 发 warning，不影响构建。）

### 4.2 lib.rs

```rust
pub mod native;   // 与 host.rs 平级；native/mod.rs 已存在且无 tauri 依赖
```

setup 中（在 `host::setup` 之后、窗口创建之前/按序）：

```rust
// 1) 单实例：仅 dev/默认 home 语义；回调 = second_instance::plan_second_instance
//    注意：插件按 app identifier 收敛，不同 LINGXI_HOME 并行需先验证；
//    若不满足隔离语义，改用 native::single_instance::lock_namespace 自建锁（见 §6 OQ）。
let instance_plan = native::single_instance::plan_second_instance(has_main, has_onboarding);
// 2) 托盘：native::tray::build_menu_model(&native::i18n::tray_strings(locale))
//    - TrayIconBuilder::with_id(native::tray::TRAY_ID)
//    - .icon(Image::from_bytes(include_bytes!("../../src/assets/tray-template.png"))?)
//    - .icon_as_template(native::tray::icon_as_template(platform))
//    - .tooltip(native::tray::tooltip(cfg!(debug_assertions)))
//    - .on_menu_event: native::tray::action_for_id(id) → native::tray::plan_action 执行
// 3) 全局快捷键：插件 init 后由 native::global_shortcut::plan_registration/汇总结果注册
// 4) keep-awake / speech / handoff / window-state 的状态对象 `.manage(...)`
// 5) 窗口创建：main/onboarding/splash 按 native::window_labels 的 label 常量；
//    settings/quick-chat 由命令懒创建；窗口 close 事件走 native::window_lifecycle 判定
```

`invoke_handler` **只用裸命令名**（audit 不剥注释；括号内不要写注释；函数用 `use` 导入）：

```text
reload_main_window, restart_app, open_settings, settings_changed,
show_notification, select_skill, open_folder, open_file, show_in_finder,
start_drag, get_keep_awake_status, set_keep_awake_enabled,
get_auto_launch_status, set_auto_launch_enabled,
keybindings_reload_global, quick_chat_reload_shortcut, keybindings_test_register,
quick_chat_shortcut_status, quick_chat_show, quick_chat_hide, quick_chat_resize,
quick_chat_open_session, quick_chat_open_session_ack,
get_splash_info, get_avatar_path, onboarding_complete,
speech_permission_status, speech_permission_request
```

既有命令（host-core 已注册，函数体改为调用 native 纯函数，**不重复注册**）：
`get_platform, get_app_version, app_ready, window_minimize, window_maximize, window_close,
window_is_maximized, window_theme_changed, open_external, get_server_connection_info,
get_server_port, get_server_token`。

debug-only（`#[cfg(debug_assertions)]` 注册 + build.rs 同门）：`debug_open_onboarding,
debug_open_onboarding_preview`（release 不可达，见 `native::onboarding`）。

**本轮不注册**：`screenshot_render`（OQ-04 捕获未实现；TS 片段登记显式理由）、
computer-use helper 命令（服务当前 `computer_control_engine_unavailable`，无 helper；
偏好/批准/撤销/申请权限继续走服务 HTTP 路由，宿主无需新命令）、
`select_folder/select_files/reload 等 R06 已有命令`（保持现状）。

### 4.3 build.rs AppManifest

把上节新命令全部加入 `AppManifest::new().commands(&[...])`（与 invoke_handler 同名同集合；
T06 四集合一致性：invoke_handler / AppManifest / permissions/autogenerated / capabilities）。

### 4.4 capabilities（按窗口 label 分组；T06 最终收口，以下为本轮最小草案）

| capability 文件 | windows | allow-* 草案（除 `core:default` 外） |
|---|---|---|
| `main.json` | `["main"]` | 全部新命令 + 既有命令；**不下放** notification/global-shortcut/autostart 插件 JS 权限 |
| `settings.json`（新） | `["settings"]` | `reload_main_window` 不放；`restart_app, open_settings, settings_changed, show_notification, select_skill, open_folder, open_file, show_in_finder, get_keep_awake_status, set_keep_awake_enabled, get_auto_launch_status, set_auto_launch_enabled, keybindings_*, quick_chat_shortcut_status, settings-changed 事件` |
| `quick-chat.json`（新） | `["quick-chat"]` | `quick_chat_hide, quick_chat_resize, quick_chat_show, quick_chat_open_session, settings_changed, open_external` |
| `browser-viewer.json` | `["browser-viewer"]` | 保持 T05 现状 + `settings_changed`（主题广播） |
| `viewer.json` / `skill-viewer.json` | 现状 | 不加新命令 |
| `splash.json`（新） | `["splash"]` | `get_splash_info, get_avatar_path, onboarding_complete` |
| `onboarding.json`（新） | `["onboarding"]` | 同上 + debug 命令仅 dev capability（或 T06 用独立 dev capability 文件） |

窗口 label 常量以 `native/window_labels.rs` 为唯一来源（`ROLE_*`），capabilities 的 windows 与之一致。

### 4.5 命令签名契约（host-core 写命令函数时按此调用纯逻辑；参数为 camelCase JS → snake_case Rust）

| command | Rust 参数 | 调用的 native 纯函数 |
|---|---|---|
| `reload_main_window` | `window: WebviewWindow` | `reload_restart::authorize_reload(label)` |
| `restart_app` | `window: WebviewWindow` | `authorize_restart(label)` + `begin_restart(guard)` + 退出清理（T02） |
| `open_settings` | `window, tab: Option<String>` | `settings_window::plan_open_settings(label, tab, main_alive, main_crashed, settings_alive)` |
| `settings_changed` | `window, event_type: String, data: Value` | `settings_broadcast::authorize_sender(label)` → 从 `Value` 抽字段构造 `SettingsEventInput` → `plan_broadcast` → 执行 side effects → emit 给其它窗口 |
| `show_notification` | `window, title, body, agent_id: Option<String>, options: Value` | `notification::validate_notification_request(...)` → 插件 show → `NotificationReceipt` |
| `select_skill` | `window` | `file_dialogs::dialog_options(DialogKind::Skill)` + 取消形状 |
| `open_folder` / `open_file` / `show_in_finder` | `window, path: String` | `external_open::plan_open_folder/plan_open_file/plan_reveal_in_finder` → `open` crate / objc2 NSWorkspace |
| `start_drag` | `window, paths: Vec<String>` | `drag_out::plan_start_drag(label, &paths)` → `start_dragging` |
| `get_keep_awake_status` / `set_keep_awake_enabled` | `window, [enabled]` | `keep_awake::authorize_caller` + `KeepAwakeMachine`（mac IOPMAssertion / win SetThreadExecutionState；Linux unsupported） |
| `get_auto_launch_status` / `set_auto_launch_enabled` | `window, [enabled]` | `auto_launch::authorize_*` + `plan_set_enabled` → 插件 `autolaunch()` → 重读后 `normalize_status` |
| `keybindings_reload_global` / `quick_chat_reload_shortcut` | `window` | `global_shortcut::select_quick_chat_shortcuts` → `plan_registration` → 插件注册 → `summarize_registration` |
| `keybindings_test_register` | `window, shortcut: String` | `plan_test_register` → 注册后立即注销（或已注册 self:true）→ 插件 `is_registered` |
| `quick_chat_shortcut_status` | `window` | `shortcut_status(desired, registered)` |
| `quick_chat_show` / `quick_chat_hide` / `quick_chat_resize` | `window, [mode, height]` | `quick_chat::authorize_*` + `normalize_resize_request` + `ShowPlan/HidePlan` + 透明窗平台门 |
| `quick_chat_open_session` / `quick_chat_open_session_ack` | `window, target/result: Value` | `quick_chat_handoff::plan_open/decide_ack/finish_focus` + managed pending state |
| `get_splash_info` / `get_avatar_path` | `window, [role]` | `splash::authorize_splash_caller` + 页面门 + 路径/格式策略 |
| `onboarding_complete` | `window` | `onboarding::authorize_onboarding_complete` + `plan_onboarding_complete(RustService)`（当前显式 NOT_IMPLEMENTED，与现役一致） |
| `speech_permission_status` / `speech_permission_request` | `window` | `speech::authorize_speech_caller` + `SpeechPermissionController`（TCC 桥见 4.6） |

### 4.6 平台适配（host-core 写 glue；纯逻辑已就绪）

- **macOS Speech TCC**：`SFSpeechRecognizer authorizationStatus`（class method）用 objc2 查询；
  `requestAuthorization:` 用 `block2::RcBlock` 回调。`host_can_prompt = 打包壳 && Info.plist 含
  `NSSpeechRecognitionUsageDescription`（BLOCKED-SIGNING-IDENTITY 下本地 ad-hoc 包也能测）。
  非 mac 返回 `Unsupported`，桥加载失败返回 `BridgeUnavailable`（fail-closed）。
- **macOS computer-use TCC**：服务进程已用 ApplicationServices/CoreGraphics FFI 查询/请求
  （`computer_use_settings.rs::native`）；宿主侧 helper 未实现，租约状态机在 `permissions.rs`，
  真实输入注入在 helper 落地前保持 BLOCKED。
- **keep-awake**：mac IOKit `IOPMAssertionCreateWithName(kIOPMAssertionTypePreventUserIdleSystemSleep)`
  / Release；win `SetThreadExecutionState(ES_CONTINUOUS|ES_SYSTEM_REQUIRED)`；Linux 显式 unsupported。
- **showInFinder**：objc2 `NSWorkspace` `selectFile:inFileViewerRootedAtPath:`；win/linux 平台阻塞。
- **托盘**：`desktop/src/assets/tray[-dev]-template.png` / `tray[-dev].ico` 用 `include_bytes!`
  编译进宿主（`../src/assets` 相对 `src-tauri/src/native/`），`Image::from_bytes` 需 `image-png`。
  mac 设 `icon_as_template(true)`；资源缺失必须显式报错（`tray::creation_failure`）。
- **窗口状态持久化**：host-core 在窗口 move/resize 用 `window_state::sanitize_window_state` +
  500ms 防抖写 `LINGXI_HOME/user/window-state.json`（quick-chat 用 `quick_chat::merge_saved_state`，
  路径 `user/quick-chat-window-state.json`）。
- **settings 广播 side effects**：`theme-changed`（browser-viewer 主题缓存 + `window.set_theme`）、
  `network-proxy-changed`（代理）、`keep-awake-changed`、`quick-chat-shortcut-changed`、
  `locale-changed`（重建托盘菜单）必须真实执行后再 fanout（A05）。

## 5. 建议 assertion_cases 映射（Lead 统一登记 FULL_BINDINGS；本轮不登记 T03 叶）

### 5.1 T04：9 叶最终绑定计划（真实服务证据 4 叶可登记；其余按原因 manual/blocked）

producer=`r09_t04_permission_matrix`，cases 文件 `cases.json`（`r09_t04.py::T04_BINDING_PLAN`
为唯一机器源，`t04::binding-plan-*` 质量门校验 case 真实存在且 PASS）。

| 叶 ID | 状态 | 断言 1 cases | 断言 2 cases | 备注 |
|---|---|---|---|---|
| `R00-T02-LA-EF9EEE38D8F6`（读取电脑操作偏好） | **bindable** | `leaf::computer-use.prefs-read`, `leaf::computer-use.permission-subject-and-names` | `leaf::computer-use.prefs-update-invalid-bounded`, `leaf::computer-use.unauthenticated-denied`, `leaf::computer-use.platform-linux-preview` | 真实服务 HTTP + app-events；Linux Preview 真机负向仍 BLOCKED_PLATFORM（由 Rust 单测证 unsupported 形状） |
| `R00-T02-LA-6E8307F57F42`（更新电脑操作偏好） | **bindable** | `leaf::computer-use.prefs-update`, `leaf::computer-use.engine-unavailable-honest` | `leaf::computer-use.prefs-update-invalid-bounded`, `leaf::computer-use.platform-linux-preview` | PUT 布尔白名单 + 回读 + 无效 body 400 |
| `R00-T02-LA-3FA31D4C3100`（批准电脑操作应用） | **bindable** | `leaf::computer-use.approve-app` | `leaf::computer-use.approval-invalid-body`, `leaf::computer-use.unauthenticated-denied` | 含 app-events `computer-use-settings-changed` 真实观测 |
| `R00-T02-LA-38B14B7B34BD`（撤销应用批准） | **bindable** | `leaf::computer-use.revoke-app` | `leaf::computer-use.approval-invalid-body`, `leaf::computer-use.unauthenticated-denied` | 同上 |
| `R00-T02-LA-CA446FE1DB71`（申请 OS 权限） | **manual** | （正向会触发真实 TCC → 人工） | `leaf::computer-use.request-permissions-negative`, `leaf::computer-use.platform-linux-preview` | 人工步骤：系统设置授权/拒绝 + 录屏 + TCC 截图；负向已自动覆盖 |
| `R00-T02-LA-D34F2D718CF3`（打包壳 Speech 授权弹窗） | blocked | `leaf::speech.permission-state-mapping` | `leaf::speech.request-single-flight`, `leaf::speech.dispose-fail-closed` | 真实弹窗需签名身份 + 人工（BLOCKED-SIGNING-IDENTITY/BLOCKED-SYSTEM-PERMISSIONS） |
| `R00-T02-LA-FD5B420182C6`（ASR 识别结果） | blocked | `leaf::speech.cancel-timeout-not-success` | `leaf::speech.cancel-timeout-not-success`, `leaf::speech.ui-states` | BLOCKED-REAL-PROVIDER（服务 system-speech-recognition 显式 refusal） |
| `R00-T02-LA-D2575672A18D`（helper 租约/真实控制） | blocked | `leaf::computer-use.lease-requires-approval` | `leaf::computer-use.lease-expiry-and-controls` / `revoke-invalidates-lease` / `remote-session-denied` | 服务 `computer_control_engine_unavailable`，无 helper；真实输入不可验 |
| `R00-T02-LA-54DC0A05A0DE`（页截图） | blocked | `leaf::screenshot.preview-vs-save-shape` | `leaf::screenshot.theme-and-save-policy` | OQ-04：Wry 无稳定捕获 API |

> **登记口径**：4 个 bindable 叶可在本轮直接登记 FULL_BINDINGS（case 已 PASS）；manual/blocked 叶
> 在人工证据或对应能力落地前不登记（不拿策略层/单测冒充系统动作）。与 Lead 裁决「producer 转绿再登记」一致：
> 4 叶的 producer 已转绿；其余保持红。

### 5.2 T03：本轮不建议登记 FULL_BINDINGS（原因：真实系统动作需注册后）

T03/T07 共享 46 叶的权威分类见 **§9**：**22 HOST_NATIVE + 8 HOST_BRIDGE + 16 SERVER_REACT**。
其中 16 叶是真正的 server/React 面（不属宿主证据）；8 叶宿主只参与桥接（主题/通知/快捷键/i18n/onboarding 广播与窗口动作），权威在 server/React；
**旧文“24 叶是 server/React 面”为误述（24 实为 R00 非 DESKTOP_BEHAVIOR 类别数 = 16 SERVER_REACT + 8 HOST_BRIDGE）**。
其余 22 叶 DESKTOP_BEHAVIOR（+ 主题/通知/快捷键/i18n 相关桥接叶）的断言要求「用户动作真实生效」
（托盘/快捷键/通知/拖放/文件选择/系统权限），在 host-core 合并注册并提供真实 GUI 会话前均为
`blocked::`。本轮的 `leaf::<capability>.<behavior>` 专属 case（52 个，§2）是注册后逐断言绑定的
**支持证据基础**，建议登记时按「叶断言 → 真实动作 case + 纯逻辑负例」成对组织，例如：
- 拖拽叶：真实 Finder 拖出/拖入（待补） + `leaf::drag-out.shapes`、`leaf::file-drop.native-paths`（负例）；
- 快捷键叶：合成 CGEvent 触发 quick-chat（待补） + `leaf::global-shortcut.registration-unavailable`、
  `leaf::global-shortcut.max-three-groups`；
- 设置广播叶：双窗真实广播（待补） + `leaf::settings-broadcast.side-effects`、`.whitelist`；
- 关闭/退出叶：进程树探针（待补，T02/T07） + `leaf::window-lifecycle.*`、`leaf::window-state.*`。

## 6. 阻塞与开放项（如实）

| ID | 类型 | 内容 | 解除条件 |
|---|---|---|---|
| T03-REG-01 | BLOCKED_PENDING_REGISTRATION | native/ 尚未被 host-core 合并（`pub mod native;` + 命令 + capability + 插件 + 窗口创建），真实系统动作全部不可执行 | host-core 按 §4 合并并 `cargo build --locked --features r09-host` 通过；本生产者重跑（去掉 `blocked::`） |
| T03-NOTIFY-02 | 已实现待验证 | mac 通知点击聚焦已用 `UNUserNotificationCenter` delegate 实现（`notification_center_macos.rs`，编译验证通过）；但**真实点击→聚焦**需注册后人工/GUI 证据；win/linux 仍 `Unsupported`（插件无通用 click 回调）。A05 在真实验证前不得声称通过 | host-core 合并后人工点击系统通知并记录窗口聚焦探针；win/linux 平台补丁或业主裁决降级 |
| T03-REPAIR-02 | 显式移除（已登记） | 现役托盘第 4 项「修复组件…」依赖 Electron `triggerArtifactRepairFlow()`（`desktop/src/shared/artifact-repair.cjs`），Tauri 无等价；按 Lead 裁决**从菜单移除**（4 项），并在本文件/宿主矩阵登记「该原能力在 R09 缺失」 | 实现等价修复流程（R08 产物修复迁移到 Rust，属 R09-T08/后续任务）后恢复菜单项；**需要 Lead 在 `BLOCKED.md` 增登记条目**（本文件非 BLOCKED 所有者） |
| T03-INSTALL-01 | 已编译验证/待真实 crate 合并 | `native/install.rs`（with_plugins/install/窗口/托盘/快捷键/窗口状态/mac keep-awake/speech TCC/Finder reveal）在临时 crate（tauri 2.12.1 + 5 插件）`cargo check`+`cargo test --lib` 通过（182 tests，证据 install-compile-01）；尚未并入 `src-tauri` crate | host-core 按 `T03_T04_REGISTRATION.md` 合并后 `cargo build --locked --features r09-host` 复核 |
| T03-REPAIR-01 | 未实现（显式） | 托盘「修复组件…」动作：Tauri 无 `triggerArtifactRepairFlow` 等价；点击返回 `NOT_IMPLEMENTED` 并写诊断 | R09-T08 迁移修复流程后接线；不得隐藏菜单项冒充 |
| T03-ONBOARD-01 | 语义差异（与现役一致） | Rust 服务模式 onboarding completion 现役 Electron 即显式 throw；Rust 服务无 `setup-complete` 路由，Tauri 只能返回 NOT_IMPLEMENTED | 产品决定 Tauri+Rust 首设流程（服务补路由或新契约），另立任务 |
| T03-SINGLE-01 | 验证缺口 | tauri-plugin-single-instance 按 app identifier 而非 LINGXI_HOME；隔离目录并行语义需注册后真实双进程验证 | 注册后 `lock_namespace` 对比 + 双实例进程探针；不满足则自建锁（纯逻辑已备） |
| T03-FILE-01 | 未完成（下一轮） | file-text-io/file-watch/docx-xlsx/observability-export/file-url-asset/detached-viewer/skill-viewer 独立模块与 TS 片段接线 | R06 命令复用 + 新模块（工作区监听需 notify；skill 解压需 zip；asset scope 需 T06/CSP） |
| T04-TCC-01 | BLOCKED_SYSTEM_PERMISSIONS | 真实 TCC 授权/撤销、打包壳弹窗、录制中撤权观察；自动化不触发 | 人工在系统设置操作并录屏（步骤见 `r09_t04.py` blocked 列表） |
| T04-HELPER-01 | 未实现/服务声明 | 服务 `computer_control_engine_unavailable`，无输入注入 helper：点击/输入/按键/滚动真实生效不可验 | helper 落地（AXUIElement/CGEvent + 租约）并接入服务；T04 叶再评估 |
| T04-SHOT-01 | BLOCKED_PLATFORM/未实现 | Tauri/Wry 无稳定页面捕获 API（OQ-04）；策略层已实现，捕获执行 BLOCKED | wry 平台钩子或服务端 HTML→PNG spike；业主裁决方案 |
| T04-ASR-01 | BLOCKED-REAL-PROVIDER | 真实语音识别结果需供应商/本地识别链；服务 `system-speech-recognition` 显式 refusal | 授权真实供应商或实现本地识别；另立任务 |
| PLATFORM | BLOCKED_PLATFORM | macOS x64 / Windows x64 / Linux x64 无真机：快捷键/托盘/通知/自启动/TCC Linux 差异不可验 | `BLOCKED.md` 既有条目，不冒充 |
| SIGNING | BLOCKED-SIGNING-IDENTITY | ad-hoc 签名下自启动（SMAppService）与正式分发形态不完整 | Developer ID + 公证凭据；R10/R11 |

## 7. 复跑命令

```bash
cd /Users/study_superior/Desktop/Code/LingxiAgent

# R09 套件分发（门内真实入口；生产者在宿主注册前应保持 exit 2 = 门真红）
python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t03_native_matrix \
  --evidence artifacts/rust-tauri/R09/dev/t03/native-matrix-01
python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t03_ipc_negative \
  --evidence artifacts/rust-tauri/R09/dev/t03/ipc-negative-01
python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t04_permission_matrix \
  --evidence artifacts/rust-tauri/R09/dev/t04/permission-matrix-01
python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t04_remote_boundary \
  --evidence artifacts/rust-tauri/R09/dev/t04/remote-boundary-01

# 纯逻辑单测（不依赖 tauri；179 tests）——生产者内部即用同一方式编译
HARNESS=$(mktemp -d); printf '#[path = "%s"]\npub mod native;\n' \
  "$PWD/desktop/src-tauri/src/native/mod.rs" > "$HARNESS/lib.rs"
~/.cargo/bin/rustup run 1.98.1 rustc --test --edition 2021 --crate-name native_host_harness \
  "$HARNESS/lib.rs" -o "$HARNESS/run" && "$HARNESS/run"

# host-core 注册面编译验证（临时 crate；tauri 2.12.1 + 5 插件；182 tests）
#   见 artifacts/rust-tauri/R09/dev/t03/install-compile-01/result.json（解析版本/源码 sha）

# TS 能力片段类型检查（全仓）
node_modules/.bin/tsc --noEmit
```

## 8. 建议下一步（给 Lead / host-core）

1. host-core 按 **`T03_T04_REGISTRATION.md`**（§4 的 diff 版）合并 native/：Cargo 依赖 → `pub mod native;`
   → `with_plugins` 首行 → `install(app)?` → 命令函数/invoke_handler/build.rs/capabilities；
   合并后立即 `cargo build --locked --features r09-host`，编译错误按 §4.5 签名契约修（纯逻辑不改）。
2. 合并后本生产者去掉对应 `blocked::`，补真实系统动作证据（托盘菜单点击/合成快捷键/通知投递/文件面板/
   拖拽/进程树/窗口树），再跑 `verify-stage R09` 新目录。
3. T03 下一轮补 file-io/watch/docx/export/asset/viewer/skill 模块与 TS 接线；T05/T06 归各自任务。
4. T04：待 TCC 人工步骤执行后补 `request-permissions` 正向与撤权证据；helper/截图按 §6 裁决。
   通知点击（OQ-03）注册后做一次真实点击→主窗聚焦探针，替换本文件的「待验证」。
5. Lead 登记 FULL_BINDINGS：T04 先按 §5.1 登记 4 叶（approve/revoke/read/update），其余等真实动作；
   T03 等注册+真实 E2E 后逐叶登记，禁止用纯逻辑单测冒充系统动作。
6. Lead 需在 `BLOCKED.md` 增登记：托盘「修复组件…」R09 缺失（T03-REPAIR-02）、mac 通知点击待真实验证
   （T03-NOTIFY-02）、Windows/Linux click 聚焦 unsupported；本文件只登记事实，不代替 BLOCKED 所有者。
7. **T06 命令面缺口（visible 批次前需 host-core/Lead 处理）**：
   - `read_docx_html` / `read_xlsx_html` 宿主命令未注册（T03-FILE-01；计划中 4 条断言 `blocked_pending_command`）；
   - Rust 服务无 `/api/preferences/setup-complete` 等价路由（onboarding 4 条断言 blocked，T03-ONBOARD-01）；
   - e2e-test 构建 + `TAURI_WEBDRIVER_PORT` 启动契约需在 visible 批次固定（`r09_gui_actions` 按此约定）；
   - settings/quick-chat/splash/onboarding capability allow-* 与事件 payload 已列于 `T03_T04_REGISTRATION.md` §4，
     visible 批次前需确认为最终四集合一致（T06 审计）。

## 9. T03/T07 共享 46 叶权威分类（唯一事实源）

### 9.0 权威统计与「24 vs 16」对账（2026-10-26，r09-native-caps 只读复核）

46 叶按**实现/证据归属**分三类（逐叶判据见 §9.1；分类与 r09-host-core 的
`T07_LEAF_MAPPING.json` 的 `HOST_UI=30 / NON_HOST_SERVER_REACT=16` 对齐，但把 30 细分为
“纯宿主原生面 22”与“宿主参与但权威在 server/React 8”）：

| 分类 | 数量 | 含义 | T03 责任 |
|---|---|---|---|
| **HOST_NATIVE** | **22** | R00 `DESKTOP_BEHAVIOR`，宿主原生能力直接实现（托盘/快捷键/通知/窗口/对话框/拖拽/自启/防休眠/i18n 等） | `desktop/src-tauri/src/native/*` + `DesktopHost-Tauri`；T03/T07 双证 |
| **HOST_BRIDGE** | **8** | R00 `SEMANTIC_EFFECT`/`UI_BEHAVIOR`，权威实现/持久化在 server 或 React；宿主参与事件广播/窗口主题/全局注册/i18n 托盘 | 宿主侧只证“桥接行为”（如 theme→window、locale→tray、快捷键→global）；持久化与页面行为由 server/React 证据 |
| **SERVER_REACT** | **16** | R00 `SEMANTIC_EFFECT`/`UI`，实现与证据都归 server 路由或 React 页面；宿主无新增命令 | **不属 T03 原生面**，由 server/React 套件或 T07 原生 WebView E2E 覆盖；T07 不得用宿主 E2E 冒充 |
| **合计** | **46** | — | — |

**「24」的来源与纠正**：清点 §9 注释里的 24 = R00 非 `DESKTOP_BEHAVIOR` 类别总数
（`SEMANTIC_EFFECT` 18 + `UI` 5 + `UI_BEHAVIOR` 1）。其中 **8 条仍映射到 host 能力**（HOST_BRIDGE），
**真正 SERVER_REACT = 16**。因此：

- 46 = **22 HOST_NATIVE + 8 HOST_BRIDGE + 16 SERVER_REACT**；
- 若按“是否涉及宿主”二分：**30（22+8）涉及宿主 / 16 不涉及**（与 T07 映射的 30/16 一致）；
- T03_T04_PROGRESS 旧 §5.2 的“24 叶是 server/React 面”**表述错误**（把 R00 类别数当成非宿主数），
  已在本节纠正；旧 §9 附录的“非宿主 16”逐叶列表本身正确，但缺少 HOST_BRIDGE 归类，现补全。

子类说明（按 Lead 要求分开列清）：

- **HOST_BRIDGE 8 叶**：外观偏好读/写（`theme-sync`）、快捷键读/写（`global-shortcut`）、
  通知偏好读/写（`notification`）、五语言切换（`i18n-main`）、首次设置完成 setupComplete（`onboarding`）。
  这些叶的持久化/页面在 server/React，宿主只负责窗口主题/系统注册/托盘文案/事件广播或“保存成功后开主窗”。
- **SERVER_REACT 16 叶**：user profile 读/写、env-deps 读/刷新、斜杠命令 registry、workspace/sidebar 界面状态读/写、
  settings snapshot、旧 GPU 安全模式清理、5 个设置页（interface/me/keybindings/general/envdeps）。
  无 T05 浏览器叶（T05 的 19 叶独立，不在 46 叶内）。

### 9.1 46 叶逐叶权威分类（leafId / featureId / 摘要 / 分类 / 判据）

| # | leafId | featureId | then 摘要 | 分类 | 判据（真实消费者 file:line 或 R00 归属） |
|---|---|---|---|---|---|
| 1 | `R00-T02-LA-0DEEE4813B0A` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-DETACHED-VIEWER-0DEEE4` | 新 BrowserWindow 按窗口 ID 拉取文件载荷并显示，关闭后清除 pinned 状态 | **HOST_NATIVE** | `host:detached-viewer` 消费者: desktop/src/react/stores/preview-actions.ts:41（spawn）；desktop/src/react/stores/preview-actions.ts:63（onViewerClosed）；R00 execution_owner: Tauri DesktopHost＋原React |
| 2 | `R00-T02-LA-1120B2AA320D` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-ONBOARDING-COMPLETE-1120B2` | server PreferencesManager 持久化后打开主窗口并注册快捷键 | **HOST_NATIVE** | `host:onboarding` 消费者: desktop/src/react/onboarding/OnboardingApp.tsx:94；desktop/src/react/onboarding/steps/TutorialStep.tsx:87；R00 execution_owner: Tauri DesktopHost＋原React |
| 3 | `R00-T02-LA-1329EA46F72C` | `F-D23-UI-UI-SETTINGS-INTERFACE-1329EA` | 逐动作核对实际状态与页面投影；失败不伪装成空数据 | **SERVER_REACT** | React `desktop/src/react/settings/SettingsContent.tsx` + `settings/tabs/InterfaceTab.tsx`（R00 source `SettingsNav.tsx:17`）；服务端 preferences/appearance 路由 `lib.rs:8905` 仅消费方 |
| 4 | `R00-T02-LA-15DFB1AED8F2` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-START-DRAG-15DFB1` | Electron 原生拖拽携带单个或多个路径 | **HOST_NATIVE** | `host:drag-out` 消费者: desktop/src/react/components/desk/DeskTree.tsx:562；desktop/src/react/components/chat/AssistantMessage.tsx:608；R00 execution_owner: Tauri DesktopHost＋原React |
| 5 | `R00-T02-LA-15E163FBFBE8` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-SETTINGS-CHANGED-15E163` | 主窗、设置窗、浏览器查看器收到同类变更并刷新相关显示 | **HOST_NATIVE** | `host:settings-broadcast` 消费者: desktop/src/react/settings/tabs/KeybindingsTab.tsx:251/362；desktop/src/react/settings/tabs/ExperimentsTab.tsx:416；R00 execution_owner: Tauri DesktopHost＋原React |
| 6 | `R00-T02-LA-1629CDD9EA4F` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-CONFIG-USER-PROFILE-PUT-1629CD` | 写 user.md 并重建 system prompt | **SERVER_REACT** | Rust `lingxi-service/settings_configuration.rs:799 user_profile_put_route`（`/lingxi/v1/user-profile` PUT，`lib.rs:9055`）；React `settings/tabs/MeTab.tsx:53`；R00 owner `server/routes/config.ts:386` |
| 7 | `R00-T02-LA-173C6332527D` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-ENV-DEPS-SYSTEM-ENV-DEPS-REFRESH-173C63` | 强制重探并返回新报告 | **SERVER_REACT** | Rust `lib.rs:8916 /lingxi/v1/system/env-deps/refresh` + `env_deps.rs`；React `settings/tabs/EnvDepsTab.tsx:76`；R00 owner `server/routes/env-deps.ts:47` |
| 8 | `R00-T02-LA-18FE3FDFF9A5` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-APPEARAN-18FE3F` | 返回主题/字体等外观状态，不改动设置 | **HOST_BRIDGE** | 权威面=R00 owner `server/routes/preferences.ts`（server/routes/preferences.ts:256）；host 侧参与: `host:theme-sync` 消费者: desktop/src/shared/theme.ts:28（hana?.syncWindowTheme?.） |
| 9 | `R00-T02-LA-1C41525D379C` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-DEBUG-OPEN-ONBOARDING-1C4152` | 仅打开或聚焦引导窗口 | **HOST_NATIVE** | `host:onboarding` 消费者: desktop/src/react/onboarding/OnboardingApp.tsx:94；desktop/src/react/onboarding/steps/TutorialStep.tsx:87；R00 execution_owner: Tauri DesktopHost＋原React |
| 10 | `R00-T02-LA-1D4C7E9C713C` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-COMMANDS-COMMANDS-1D4C7E` | 从 registry 返回名称、别名、说明、权限、作用域和来源；未就绪返回 503 | **SERVER_REACT** | React `hooks/use-slash-items.ts:100`（`/api/commands`）；R00 owner `server/routes/commands.ts:18`；**Rust 服务未检出 `/api/commands` 等价迁移路由（R07 缺口，非宿主面）** |
| 11 | `R00-T02-LA-1E6EDB731186` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-DEBUG-OPEN-ONBOARDING-PREVIEW-1E6EDB` | 预览窗显示引导步骤 | **HOST_NATIVE** | `host:onboarding` 消费者: desktop/src/react/onboarding/OnboardingApp.tsx:94；desktop/src/react/onboarding/steps/TutorialStep.tsx:87；R00 execution_owner: Tauri DesktopHost＋原React |
| 12 | `R00-T02-LA-322E02C7F2F9` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-RELOAD-MAIN-WINDOW-322E02` | 主窗/设置窗/viewer 从激活目录重载 | **HOST_NATIVE** | `host:reload-restart` 消费者: desktop/src/react/settings/tabs/AccessTab.tsx:366；desktop/src/react/settings/tabs/AccessTab.tsx:390；R00 execution_owner: Tauri DesktopHost＋原React |
| 13 | `R00-T02-LA-37E6C3B9A7B8` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-READ-DOCX-HTML-37E6C3` | DOCX 转 HTML 后在预览中显示 | **HOST_NATIVE** | `host:docx-xlsx-preview` 消费者: desktop/src/react/utils/preview-file-content.ts:127（readDocxHtml）；desktop/src/react/utils/preview-file-content.ts:131（readXlsxHtml）；R00 execution_owner: Tauri DesktopHost＋原React |
| 14 | `R00-T02-LA-394DD07D2968` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-KEYBINDI-394DD0` | 保存快捷键映射并返回新状态 | **HOST_BRIDGE** | 权威面=R00 owner `server/routes/preferences.ts`（server/routes/preferences.ts:368）；host 侧参与: `host:global-shortcut` 消费者: desktop/src/react/settings/tabs/KeybindingsTab.tsx:112（test probe）、:209（test-register）、:254/:257（reload）、desktop/src/react/settings/tabs/GeneralTab.tsx:179,:191（reload）；no-consumer-found：quickChatShortcutStatus 无渲染调用（R00 叶子登记但当前 React 未调用） |
| 15 | `R00-T02-LA-429D86C4F952` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-CONFIG-USER-PROFILE-READ-429D86` | 返回共享 user.md 内容 | **SERVER_REACT** | Rust `settings_ui.rs user_profile_route`（`/lingxi/v1/user-profile` GET，`lib.rs:9054`）；React `settings/actions.ts:180`；R00 owner `server/routes/config.ts:376` |
| 16 | `R00-T02-LA-57728C0A6D0A` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-OPEN-SETTINGS-57728C` | 创建或聚焦设置窗，切到指定标签 | **HOST_NATIVE** | `host:settings-window` 消费者: desktop/src/react/app-init.ts:407（onOpenSettingsModal）；desktop/src/react/settings/SettingsContent.tsx:191（onSwitchTab）；R00 execution_owner: Tauri DesktopHost＋原React |
| 17 | `R00-T02-LA-58822B6B138B` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-WINDOW-READINESS-58822B` | splash/onboarding 关闭后主窗显示 | **HOST_NATIVE** | `host:main-window` 消费者: no-consumer-found（host-internal；renderer 经 app-ready/window-controls 间接）；`host:app-ready` 消费者: desktop/src/react/app-init.ts:211；desktop/src/react/app-init.ts:234；`host:window-state` 消费者: no-consumer-found（host-internal）；`host:app-lifecycle-quit` 消费者: no-consumer-found（host-internal）；R00 execution_owner: Tauri DesktopHost＋原React |
| 18 | `R00-T02-LA-59987671E5AD` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-WORKSPAC-599876` | 返回按 workspace 与 surface 保存的界面状态，不改动设置 | **SERVER_REACT** | Rust `lib.rs:8923 /lingxi/v1/preferences/workspace-ui-state` GET（`workspace_ui_state.rs`）；React `stores/workspace-ui-state-actions.ts:104`；R00 owner `server/routes/preferences.ts:490` |
| 19 | `R00-T02-LA-5D5938D61A00` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-GLOBAL-SHORTCUT-5D5938` | 有效组合被 OS 注册并可拉起快捷聊天；冲突反馈不可注册 | **HOST_NATIVE** | `host:global-shortcut` 消费者: desktop/src/react/settings/tabs/KeybindingsTab.tsx:112（test probe）、:209（test-register）、:254/:257（reload）、desktop/src/react/settings/tabs/GeneralTab.tsx:179,:191（reload）；no-consumer-found：quickChatShortcutStatus 无渲染调用（R00 叶子登记但当前 React 未调用）；R00 execution_owner: Tauri DesktopHost＋原React |
| 20 | `R00-T02-LA-6006FD0BF07D` | `F-D23-UI-UI-SETTINGS-ME-6006FD` | 逐动作核对实际状态与页面投影；失败不伪装成空数据 | **SERVER_REACT** | React `desktop/src/react/settings/SettingsContent.tsx` + `settings/tabs/MeTab.tsx`（R00 source `SettingsNav.tsx:16`） |
| 21 | `R00-T02-LA-6774E79F6714` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-SETUP-CO-6774E7` | 持久设置 setupComplete=true；后续启动不再进入初设流程 | **HOST_BRIDGE** | 权威面=R00 owner `server/routes/preferences.ts`（server/routes/preferences.ts:476）；host 侧参与: `host:onboarding` 消费者: desktop/src/react/onboarding/OnboardingApp.tsx:94；desktop/src/react/onboarding/steps/TutorialStep.tsx:87 |
| 22 | `R00-T02-LA-70B07E378F42` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-LEGACY-G-70B07E` | 仅旧值 false 时比较后删除；其他当前值原样保留，返回是否改动 | **SERVER_REACT** | R00 owner `server/routes/preferences.ts:154`（Electron 启动闸门调用 `desktop/main.cjs:2265`）；**Rust 服务无等价路由，Rust 模式显式拒绝**；R09 不迁移真实数据 → 非宿主、Tauri 不实现 |
| 23 | `R00-T02-LA-713E908B7B2F` | `F-D23-UI-UI-SETTINGS-ENVDEPS-713E90` | 逐动作核对实际状态与页面投影；失败不伪装成空数据 | **SERVER_REACT** | React `desktop/src/react/settings/SettingsContent.tsx` + `settings/tabs/EnvDepsTab.tsx`（R00 source `SettingsNav.tsx:32`） |
| 24 | `R00-T02-LA-8FE26C132D8B` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-SIDEBAR--8FE26C` | 返回分组/显示状态，不改动设置 | **SERVER_REACT** | Rust `lib.rs:8899 /lingxi/v1/preferences/sidebar-ui` GET（`userprefs.rs`）；React `settings/tabs/InterfaceTab.tsx:182`；R00 owner `server/routes/preferences.ts:525` |
| 25 | `R00-T02-LA-9350C3E12165` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-APP-RESTART-9350C3` | 应用按既定退出/重启路径重新启动 | **HOST_NATIVE** | `host:reload-restart` 消费者: desktop/src/react/settings/tabs/AccessTab.tsx:366；desktop/src/react/settings/tabs/AccessTab.tsx:390；`host:app-lifecycle-quit` 消费者: no-consumer-found（host-internal）；R00 execution_owner: Tauri DesktopHost＋原React |
| 26 | `R00-T02-LA-98B458390B7E` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-KEYBINDI-98B458` | 返回快捷键映射，不改动设置 | **HOST_BRIDGE** | 权威面=R00 owner `server/routes/preferences.ts`（server/routes/preferences.ts:360）；host 侧参与: `host:global-shortcut` 消费者: desktop/src/react/settings/tabs/KeybindingsTab.tsx:112（test probe）、:209（test-register）、:254/:257（reload）、desktop/src/react/settings/tabs/GeneralTab.tsx:179,:191（reload）；no-consumer-found：quickChatShortcutStatus 无渲染调用（R00 叶子登记但当前 React 未调用） |
| 27 | `R00-T02-LA-9C679D21A188` | `F-D23-UI-UI-SETTINGS-KEYBINDINGS-9C679D` | 逐动作核对实际状态与页面投影；失败不伪装成空数据 | **SERVER_REACT** | React `desktop/src/react/settings/SettingsContent.tsx` + `settings/tabs/KeybindingsTab.tsx`（R00 source `SettingsNav.tsx:18`）；快捷键持久化走 `lib.rs:8895 /lingxi/v1/preferences/keybindings` |
| 28 | `R00-T02-LA-A14D3027C881` | `F-D23-UI-UI-SETTINGS-GENERAL-A14D30` | 逐动作核对实际状态与页面投影；失败不伪装成空数据 | **SERVER_REACT** | React `desktop/src/react/settings/SettingsContent.tsx` + `settings/tabs/GeneralTab.tsx`（R00 source `SettingsNav.tsx:19`） |
| 29 | `R00-T02-LA-A2E507EBDEF2` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-AUTO-LAUNCH-A2E507` | 读取/修改 OS 登录项并回显真实状态 | **HOST_NATIVE** | `host:auto-launch` 消费者: desktop/src/react/settings/tabs/GeneralTab.tsx:101（get）；desktop/src/react/settings/tabs/GeneralTab.tsx:211（set）；R00 execution_owner: Tauri DesktopHost＋原React |
| 30 | `R00-T02-LA-AC5B6EDA3FBD` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-GET-SPLASH-INFO-AC5B6E` | 显示启动所需名称/头像等资料 | **HOST_NATIVE** | `host:splash` 消费者: desktop/src/react/splash/SplashApp.tsx:59/60；desktop/src/react/onboarding/OnboardingApp.tsx:117/126；R00 execution_owner: Tauri DesktopHost＋原React |
| 31 | `R00-T02-LA-ACA2C4C1158D` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-OPEN-EXTERNAL-ACA2C4` | 仅 http/https 交由系统浏览器打开 | **HOST_NATIVE** | `host:external-open` 消费者: desktop/src/react/settings/tabs/McpTab.tsx:260；desktop/src/react/settings/tabs/AboutTab.tsx:372；R00 execution_owner: Tauri DesktopHost＋原React |
| 32 | `R00-T02-LA-B6A97A19F2CB` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-OPEN-SETTINGS-MODAL-B6A97A` | 主界面设置弹窗打开并切到目标 tab | **HOST_NATIVE** | `host:settings-window` 消费者: desktop/src/react/app-init.ts:407（onOpenSettingsModal）；desktop/src/react/settings/SettingsContent.tsx:191（onSwitchTab）；`host:settings-broadcast` 消费者: desktop/src/react/settings/tabs/KeybindingsTab.tsx:251/362；desktop/src/react/settings/tabs/ExperimentsTab.tsx:416；R00 execution_owner: Tauri DesktopHost＋原React |
| 33 | `R00-T02-LA-B72A6D3886B7` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-KEEP-AWAKE-B72A6D` | 系统 powerSaveBlocker 状态与开关结果一致 | **HOST_NATIVE** | `host:keep-awake` 消费者: desktop/src/react/settings/tabs/GeneralTab.tsx:227（setKeepAwakeEnabled）；no-consumer-found：getKeepAwakeStatus 无渲染调用（R00 叶子登记但当前 React 未调用）；R00 execution_owner: Tauri DesktopHost＋原React |
| 34 | `R00-T02-LA-BEC2074CC33C` | `F-D23-UI_BEHAVIOR-UI-BEHAVIOR-FIVE-LANGUAGE-BEC207` | zh/en/ja/ko/zh-TW 文案随偏好更新且重启保留 | **HOST_BRIDGE** | 权威面=R00 owner `desktop/src/react/settings/tabs/InterfaceTab.tsx`（desktop/src/react/settings/tabs/InterfaceTab.tsx:592, desktop/src/react/settings/tabs/InterfaceTab.tsx:600, desktop/src/react/settings/tabs/InterfaceTab.tsx:601）；host 侧参与: `host:i18n-main` 消费者: desktop/src/react/services/app-event-actions.ts:309（locale-changed 源头）→ settingsChanged |
| 35 | `R00-T02-LA-C5AAF360D10F` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-WORKSPAC-C5AAF3` | 保存按 workspace 与 surface 保存的界面状态并返回新状态 | **SERVER_REACT** | Rust `lib.rs:8923 /lingxi/v1/preferences/workspace-ui-state` PUT（`workspace_ui_state.rs`）；React `stores/workspace-ui-state-actions.ts`；R00 owner `server/routes/preferences.ts:505` |
| 36 | `R00-T02-LA-CD873C715D96` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-ENV-DEPS-SYSTEM-ENV-DEPS-READ-CD873C` | 读取缓存报告或 refresh=1 强制重探 | **SERVER_REACT** | Rust `lib.rs:8912 /lingxi/v1/system/env-deps` GET（`env_deps.rs`）；React `settings/tabs/EnvDepsTab.tsx:77`；R00 owner `server/routes/env-deps.ts:36` |
| 37 | `R00-T02-LA-D15E0ADC0608` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-NOTIFICA-D15E0A` | 保存通知设置并返回新状态 | **HOST_BRIDGE** | 权威面=R00 owner `server/routes/preferences.ts`（server/routes/preferences.ts:318）；host 侧参与: `host:notification` 消费者: desktop/src/react/services/ws-message-handler.ts:988；desktop/src/react/services/rust-automation.ts:298 |
| 38 | `R00-T02-LA-D29DE479076C` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-WINDOW-THEME-CHANGED-D29DE4` | 主窗及派生窗口收到主题消息并重新绘制 | **HOST_NATIVE** | `host:theme-sync` 消费者: desktop/src/shared/theme.ts:28（hana?.syncWindowTheme?.）；R00 execution_owner: Tauri DesktopHost＋原React |
| 39 | `R00-T02-LA-DF4B5168ABB5` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-SHOW-NOTIFICATION-DF4B51` | 未聚焦且 OS 支持时弹通知；点击通知聚焦主窗 | **HOST_NATIVE** | `host:notification` 消费者: desktop/src/react/services/ws-message-handler.ts:988；desktop/src/react/services/rust-automation.ts:298；R00 execution_owner: Tauri DesktopHost＋原React |
| 40 | `R00-T02-LA-E6FB08FBBFAA` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-NOTIFICA-E6FB08` | 返回通知设置，不改动设置 | **HOST_BRIDGE** | 权威面=R00 owner `server/routes/preferences.ts`（server/routes/preferences.ts:310）；host 侧参与: `host:notification` 消费者: desktop/src/react/services/ws-message-handler.ts:988；desktop/src/react/services/rust-automation.ts:298 |
| 41 | `R00-T02-LA-E84F01456B6C` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-READ-XLSX-HTML-E84F01` | 首工作表转换为转义 HTML 表格 | **HOST_NATIVE** | `host:docx-xlsx-preview` 消费者: desktop/src/react/utils/preview-file-content.ts:127（readDocxHtml）；desktop/src/react/utils/preview-file-content.ts:131（readXlsxHtml）；R00 execution_owner: Tauri DesktopHost＋原React |
| 42 | `R00-T02-LA-F5DBF2A80E28` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-APPEARAN-F5DBF2` | 保存主题/字体等外观状态并返回新状态 | **HOST_BRIDGE** | 权威面=R00 owner `server/routes/preferences.ts`（server/routes/preferences.ts:264）；host 侧参与: `host:theme-sync` 消费者: desktop/src/shared/theme.ts:28（hana?.syncWindowTheme?.） |
| 43 | `R00-T02-LA-F6235778F816` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-SETTINGS-SNAPSHOT-SETTINGS-SNAPS-F62357` | 返回配置、人格、偏好、接入及 bridge 状态 | **SERVER_REACT** | Rust `lib.rs:9006 /lingxi/v1/settings/snapshot` GET（`settings_ui.rs:234`）；React settings store（`settings/__tests__/...` 真实调用）；R00 owner `server/routes/settings-snapshot.ts:185` |
| 44 | `R00-T02-LA-F9786414DD11` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-WINDOW-CONTROLS-F97864` | 仅调用者所属 BrowserWindow 发生相应状态变化 | **HOST_NATIVE** | `host:window-controls` 消费者: desktop/src/react/components/WindowControls.tsx:24（onMaximizeChange）；desktop/src/react/components/WindowControls.tsx:26/29/30/31（isMaximized/minimize/maximize/close）；R00 execution_owner: Tauri DesktopHost＋原React |
| 45 | `R00-T02-LA-FB5C78308878` | `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-SIDEBAR--FB5C78` | 保存分组/显示状态并返回新状态 | **SERVER_REACT** | Rust `lib.rs:8899 /lingxi/v1/preferences/sidebar-ui` PUT；React `settings/tabs/InterfaceTab.tsx:302`；R00 owner `server/routes/preferences.ts:533` |
| 46 | `R00-T02-LA-FF1B331F1019` | `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-SETTINGS-SWITCH-TAB-FF1B33` | SettingsContent activeTab 更新并渲染目标页 | **HOST_NATIVE** | `host:settings-window` 消费者: desktop/src/react/app-init.ts:407（onOpenSettingsModal）；desktop/src/react/settings/SettingsContent.tsx:191（onSwitchTab）；R00 execution_owner: Tauri DesktopHost＋原React |

校验方式（只读、可复跑）：以 `R09_LEAF_LEDGER.json` 的 46 个 `R09-T03` 叶为键，
`FEATURE_STAGE_ACCEPTANCE.json` 取原断言/execution_owner，`FEATURE_INVENTORY.json` 取
`current_owners`/`source_ref`，`T03_HOST_CAPABILITY_INVENTORY.md` §9 取 capability 映射；
当前实现 file:line 用 `grep -n` 复核（Rust 路由在 `lingxi-service/src/lib.rs`，React 在
`desktop/src/react/**`）。无 GUI、无写入 `desktop/src-tauri`。

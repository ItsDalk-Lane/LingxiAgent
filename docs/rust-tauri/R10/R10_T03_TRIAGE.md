# R10 T03 失败项定性（v39-native-03）

> 状态：`TRIAGE_ONLY_NO_RERUN`
> 数据源：`artifacts/rust-tauri/R09/dev/t03/v39-native-03/cases.json`
> （143 cases；其中 27 条 `ok=false`，内含 3 条 manual-pending 与 1 条 plan 自检失败）
> 依据：`docs/rust-tauri/R10/R10_IMPLEMENTATION_PLAN.md` §5 WS3；`R09_OWNER_DECISIONS.md` OWNER-04。
> 约束：本轮**不重跑** T03/R10、不改断言/测试脚本；"产品修复" 类为按 v39 证据 + 源码定位后的预期，
> 需 Lead 重建宿主后复跑确认；未重跑前不声称任何 case 已转绿。

## 1. 分类计数

| 分类 | 计数 | 含义 |
|---|---|---|
| 产品修复（WS3 已改，待重建复跑） | 8 | 根因在宿主/服务产品代码，本轮已按 §5 修复 |
| harness-only（工具/断言/证据缺陷） | 15 | 产品行为不构成失败根因；需改 GUI 脚本或断言（本轮不改） |
| manual-GUI（人工证据） | 3 | R09 登记为 manual，缺人工证据目录 |
| deferred（延后） | 1 | 产品缺陷不在本轮 WS3 授权文件/范围，需另行裁决 |
| **合计** | **27** | 与 `cases.json` 中 `ok=false` 全集一致 |

## 2. WS3 修复索引（代码）

| # | 修复 | 文件 |
|---|---|---|
| 1 | main 窗口允许 `get_splash_info`/`get_avatar_path`，页面门不变 | `native/splash.rs`、`native_commands.rs` |
| 2 | mac keep-awake 断言类型改 NUL 结尾 `c"PreventUserIdleSystemSleep"` | `native/install.rs` |
| 3 | `plan_test_register(raw, current)` 识别宿主已注册 → `self`/`selfRegistered`，不重复注册 | `native/global_shortcut.rs`、`native_commands.rs` |
| 4 | 窗口控制四命令增加 `window_id`，出现即 `reject_window_id_argument` 拒绝 | `commands.rs` |
| 5 | `window_theme_changed` 白名单 `dark/light/auto/system`，非法显式错误且不 `set_theme` | `commands.rs` |
| 6 | `show_notification` 兼容 `desktopFocusPolicy`/`desktop_focus_policy`/`focusPolicy`/`focus_policy` | `native_commands.rs` |
| 7 | 通知偏好 PUT patch 白名单 + 键/类型/枚举校验（400 不落盘），保留既有未知历史字段 | `rust/crates/lingxi-service/src/automation.rs` |

## 3. 产品修复（8）

| case id | v39 失败证据 | 根因 / 修复 | 残余风险 |
|---|---|---|---|
| `leaf:R00-T02-LA-AC5B6EDA3FBD::a1-splash-info-avatar` | `info`/`avatar` 均 `WINDOW_NOT_ALLOWED: 该能力只允许 ["splash","onboarding"]` | OWNER-04 未落到 Rust 命令门；WS3#1 允许 main，并新增 `validate_main_page_url`（精确 `index.html` + 可信来源 + 拒绝 query），splash/onboarding 原页门不变 | 无；T03 fixture home 已有 `user/avatars/user.png`（`_fixture_files`），头像存在/大小断言可满足 |
| `leaf:R00-T02-LA-AC5B6EDA3FBD::a2-splash-info-fallback` | 同上（损坏 config 前先被窗口门拒绝） | 同 WS3#1；`native_commands::read_splash_info` 已有损坏配置回退默认值语义 | 无 |
| `leaf:R00-T02-LA-B72A6D3886B7::a1-keepawake-on-off-assertion` | `IOPMAssertionCreateWithName 失败: -536870206`（`PLATFORM_UNSUPPORTED`） | `ASSERTION_TYPE: &str` 的 `.as_ptr()` 无 NUL 终止符 → IOKit 返回 `kIOReturnBadArgument`；WS3#2 改为 `const ASSERTION_TYPE: &CStr = c"PreventUserIdleSystemSleep"` 并以 `.as_ptr()` 传入 | 无（失败仍保持 enabled=false 的既有语义未动） |
| `leaf:R00-T02-LA-5D5938D61A00::a1-shortcut-register-and-trigger` | `keybindings_test_register("Alt+Space")` 返回 `{ok:false,registered:false,selfRegistered:false}`（宿主启动时已注册该键） | WS3#3：`plan_test_register(raw, current)` 命中宿主注册表 → `already_registered=true` / `must_unregister_after=false`，命令直接返回 `{ok:true,registered:true,self:true,selfRegistered:true}`，不重复 register | 无；`quick_chat_reload_shortcut` + 合成 Alt+Space 在 v39 已能拉出 Quick Chat 窗口（该 case 仅 test-register 一步红） |
| `leaf:R00-T02-LA-5D5938D61A00::a2-shortcut-conflict-unavailable` | 两次 test-register 都是 `ok:false`（自身占用被误报冲突） | 同 WS3#3；回执同时保留 Electron 现役字段 `self`（R09 断言读 `self`）与 R10 契约名 `selfRegistered` | 无；malformed `Alt+` 仍 `INVALID_ARGUMENT` |
| `leaf:R00-T02-LA-D15E0ADC0608::a2-notification-prefs-invalid` | `invalid=200`（`{"notifications":{"desktopFocusPolicy":42}}` 被忽略后照常 200） | WS3#7：patch 先做白名单/类型/枚举校验，未知键与非字符串值一律 400，且在加锁/读盘/写盘之前返回 | 无；既有 `notification_preference_updates_preserve_other_settings_and_reject_damage` 集成测试的 400/保留字段语义保持不变 |
| `leaf:R00-T02-LA-DF4B5168ABB5::a2-notification-suppressed-or-unsupported` | receipt `{shown:true}`：`options.desktopFocusPolicy` 未识别 → 策略回落 always | WS3#6：`show_notification` 依次识别 `desktopFocusPolicy`/`desktop_focus_policy`/`focusPolicy`/`focus_policy` 的首个键再校验；主窗聚焦时 `shown:false / hana_focused` | 依赖 `window.focus()` 真的让主窗 `is_focused()==true`；重建复跑确认（v39 因策略未识别，无法从证据判断聚焦态） |
| `leaf:R00-T02-LA-18FE3FDFF9A5::a2-appearance-invalid-write` | `window_theme_changed({"theme":"\u0000"})` 返回 `ok:true`，窗口未变但断言要求拒绝 | WS3#5：主题白名单 `dark/light/auto/system`；空/控制字符/未知返回显式错误且不调用 `set_theme` | **残余**：断言还要求前后截图 sha256 相等。v39 两张图差异已定位为输入框光标闪烁（bbox `(353,531)-(355,550)`，仅 38px 亮度差），修复后仍可能因光标相位红；属 harness 证据缺陷（建议比较稳定区域/忽略光标），不是产品回归 |

## 4. harness-only（15）

| case id | v39 失败证据 | 根因（产品无责） | harness 修复方向（本轮不改） |
|---|---|---|---|
| `plan::logic-case-references-exist` | missing 18 条 `ipc::*` 逻辑 case | 这些 case 全部存在于 `native/ipc_negative_matrix.rs`；rustfmt 把 `case!(\n "id", ...)` 折成多行，`scripts/rust-tauri/r09_t03.py` 的 regex `case!\("([^"]+)"` 只匹配单行 | regex 允许宏参数换行（`case!\(\s*"([^"]+)"`） |
| `leaf:R00-T02-LA-0DEEE4813B0A::a2-viewer-missing-payload` | `ValueError: not enough values to unpack (expected 2, got 1)` | `scripts/rust-tauri/r09_gui_actions.py::_viewer_payload_isolation` 的 return 写坏：`return (cond),` 换行后的 dict 成为不可达语句，实际返回 1-tuple；`run_gui_plan` 的 `ok, detail = handler(...)` 解包失败 | 改为 `return (cond, {...})` |
| `leaf:R00-T02-LA-1C41525D379C::a2-onboarding-open-no-complete` | `GET /lingxi/v1/settings/snapshot?agent_id=…` → 400 `missing field agentId` | server `SnapshotQuery` 是 `#[serde(rename_all="camelCase")]`，query 参数应为 `agentId`；`_server_prefs` 追加的是 `agent_id` | 参数名改 `agentId` |
| `leaf:R00-T02-LA-1E6EDB731186::a1-onboarding-preview-shows-steps` | invoke `ok:true`、截图 3 张，但 `appeared=false` | 前序 `debug_open_onboarding`（已 PASS）打开了 onboarding 窗且未关闭；`debug_open_onboarding_preview` 与 Electron 一致地复用已有 onboarding 窗口（`desktop/main.cjs:6573-6580`），窗口数不增 | case 隔离（先关窗）或断言改为"窗口可见/preview DOM"而非窗口数增加 |
| `leaf:R00-T02-LA-1E6EDB731186::a2-onboarding-preview-no-side-effects` | snapshot 400 无有效 JSON | 同 snapshot 参数名 | `agentId` |
| `leaf:R00-T02-LA-57728C0A6D0A::a2-open-settings-not-saved` | snapshot 400 无有效 JSON | 同 snapshot 参数名 | `agentId` |
| `leaf:R00-T02-LA-58822B6B138B::a2-window-readiness-not-early` | `firstSnapshot` 已含 onscreen 主窗（标题 `Lingxi · Rust 预览`） | 断言注释写"首个采样无 main onscreen **或** main 首现不早于 service 创建"，实现只做前者；而 `Harness.start()` 在 WebDriver session + server port 就绪后才 `poll_timeline(5s)`，首个采样必然可能晚于 `app_ready`。另 `"Lingxi"` 子串也会匹配 `Lingxi · Rust 预览` 等其它窗口标题 | 按注释实现第二种判据（a1 已用 `instanceCreatedAt` 通过）或从进程启动即采样 |
| `leaf:R00-T02-LA-9350C3E12165::a1-restart-process-identity` | `RemoteDisconnected: Remote end closed connection without response` | `restart_app` 断开 WebDriver 连接时抛 `http.client.RemoteDisconnected`（不是 `HarnessError`），`_restart_process` 的 `except HarnessError` 接不住 → 未执行 `_recover_after_restart`，`h.app_pid` 仍为旧 PID | 捕获连接类异常并始终走恢复路径；重启后刷新 app_pid |
| `leaf:R00-T02-LA-98B458390B7E::a1-keybindings-read-effective` | `status` OK（registered:true）、`posted:true`、`window:null` | 上一条重启失败后 `h.app_pid` 是旧 PID，`app_windows()`（按 pid 过滤）永远匹配不到新进程窗口；不是注册/触发失败 | 同上（恢复后更新 app_pid）；隔离复核 CGEvent 触发 |
| `leaf:R00-T02-LA-B6A97A19F2CB::a2-settings-modal-unknown-tab` | snapshot 400 无有效 JSON | 同 snapshot 参数名 | `agentId` |
| `leaf:R00-T02-LA-D29DE479076C::a1-theme-broadcast-redraw` | invoke `ok:true`、`sideEffects` 正常；`avgBefore/After={"ok":false}` | 同 9350 stale app_pid → `wait_window` 返回 None → 无截图/亮度证据；无产品失败证据 | 修复重启恢复 + 主窗选择（勿用 `"Lingxi"` 宽匹配）后复跑 |
| `leaf:R00-T02-LA-F5DBF2A80E28::a1-appearance-update-window-theme` | `put:200`、`restore:200`；`avg={"ok":false}` | 同上：stale app_pid 导致主窗截图缺失；不能据此判"外观未应用" | 同 D29DE479076C |
| `leaf:R00-T02-LA-F9786414DD11::a1-window-controls-caller-only` | `injectedWindowIdCall.ok=true` 且 `isMaximizedAfterInjected=false`（第二次调用把窗口还原） | 产品侧 WS3#4 改为：出现 `windowId` 即显式拒绝、不再作用于目标窗 → `isMaximizedAfterInjected` 会符合期望，但断言同时要求 `injected.ok==true`（R00 注释"忽略参数"语义）。严格拒绝后该检查必为 false | 断言接受错误回执（`not injected.ok`）或只校验状态未变；产品行为按 WS3 收紧，不改 |
| `leaf:R00-T02-LA-FF1B331F1019::a2-settings-unknown-tab-no-action` | snapshot 400 无有效 JSON | 同 snapshot 参数名 | `agentId` |
| `leaf:R00-T02-LA-D15E0ADC0608::a1-notification-prefs-update-bridge` | receipt `shown:true`（PUT 已 200 但 options 未识别） | PUT body `{"notifications":{"desktopFocusPolicy":"when_unfocused"}}`：`desktopFocusPolicy` 是宿主通知载荷/选项键（`shared/desktop-notification-policy.cjs`），不是 server 偏好键；WS3#7 白名单（`chatCompletion`/`turnCompletion`/`scheduledTaskCompletion`/`patrolCompletion`）后该 PUT 返回 400，断言首个条件 `put.status==200` 不成立 | 用白名单键（如 `chatCompletion`）做持久化前提，或允许 PUT 400 而只验证 `options` 语义 |

## 5. manual-GUI（3）

| case id | v39 状态 | 所需人工证据 |
|---|---|---|
| `leaf:R00-T02-LA-15DFB1AED8F2::a1-drag-out-single-multi` | manual-pending（未提供 `LINGXI_R09_MANUAL_EVIDENCE_DIR`） | 鼠标从书桌树拖出单/多文件到 Finder，录屏 + 目标目录列表 |
| `leaf:R00-T02-LA-BEC2074CC33C::a1-locale-switch-tray-labels` | manual-pending（同上） | 切换语言后托盘菜单截图 + 重启后再截图，与 locale 文案表逐项对照 |
| `leaf:R00-T02-LA-DF4B5168ABB5::a1-notification-delivered-and-click-focus` | manual-pending（同上） | 点击通知中心通知，观察主窗获得焦点并截图/lsappinfo 前台记录 |

## 6. deferred（1）

| case id | v39 失败证据 | 根因 | 延后原因 |
|---|---|---|---|
| `leaf:R00-T02-LA-F5DBF2A80E28::a2-appearance-update-invalid` | PUT `/lingxi/v1/preferences/appearance` body `{"appearance":{"theme":"\u0000"}}` → `invalid=200`、`prefsUnchanged=false` | `rust/crates/lingxi-service/src/userprefs.rs::normalize_appearance` 只做 `as_str().map(trim).filter(!empty)`，未拒绝控制字符/未知 theme，非法值被写盘 | 不在 WS3 授权文件面；且 Electron 端为 `200 + 宿主回退默认`，与 R09 断言"必须 >=400"存在口径冲突，需业主/server 裁决"非法 theme 400 还是 200+回退"后再修 |

## 7. 汇总与已知残余

- 27 条失败 = 8 产品修复 + 15 harness-only + 3 manual-GUI + 1 deferred；本轮**未重跑**，8 条产品修复项需 Lead 重建宿主后复跑确认。
- 三条产品修复项带残余风险（见 §3 备注）：
  1. `18FE3FDFF9A5::a2`：截图 sha256 相等这一半断言受光标闪烁影响，可能仍红（产品拒绝已修）；
  2. `F9786414DD11::a1`：断言要求"忽略 windowId 且 ok"，与 WS3 的严格拒绝语义冲突，需 harness 调整；
  3. `DF4B5168ABB5::a2`：抑制依赖主窗真实聚焦，重建后确认。
- 与 WS3 相邻但**不在**本轮文件面的产品缺陷只有 `F5DBF2A80E28::a2`（appearance server 路由未校验 theme），已记入 deferred。

# T03/T04 → host-core 注册补丁（可直接套用）

目标：把 `desktop/src-tauri/src/native/` 的能力接入 `desktop/src-tauri`（host-core 唯一写者）。
**本文件只给 diff 级提示与代码片段，不代替 host-core 改文件**；依赖/版本已在临时 crate
`tauri=2.12.1 + 插件` 下 `cargo check/test` 通过（182 tests，证据
`artifacts/rust-tauri/R09/dev/t03/install-compile-01/`）。合并后请以
`cargo build --locked --features r09-host` 复核，并跑 Lead 的 `r09_capability_audit.py`。

命令函数本体仍由 host-core 写（只调 `native::*` 纯逻辑 + `native::install::*` helper）；
`native::install::INVOKE_COMMANDS` 是**新命令名唯一清单**，build.rs/capabilities/audit 都从它抄。

---

## 1. Cargo.toml（`desktop/src-tauri/Cargo.toml`）

```diff
- tauri = { version = "=2.12.1", features = [] }
+ tauri = { version = "=2.12.1", features = ["image-png", "tray-icon"] }
+ tauri-plugin-single-instance = "=2.5.4"
+ tauri-plugin-global-shortcut = "=2.4.1"
+ tauri-plugin-notification = "=2.7.0"
+ tauri-plugin-autostart = "=2.7.0"
+ objc2-foundation = "=0.3.2"
+ objc2-user-notifications = "=0.3.2"   # OQ-03 点击聚焦（native/notification_center_macos.rs）
+ block2 = "=0.6.2"                     # Speech TCC request + 通知 delegate completion

 [lints.rust]
+unexpected_cfgs = { level = "allow", check-cfg = ['cfg(r09_standalone_test)'] }
```

说明：
- `tauri-plugin-dialog` 保持 `=2.7.3`；`objc2="=0.6.4"` 保持（objc2-foundation/user-notifications
  0.3.2 要求 objc2 >=0.6.2,<0.8）。
- **不需要** `tauri-plugin-window-state`（壳自态继续写 `LINGXI_HOME/user/window-state.json`）与
  `tauri-plugin-opener`（`open` crate + objc2 NSWorkspace）。
- 临时 crate 解析出的完整版本（证据 result.json）：tauri 2.12.1 / dialog 2.7.3 /
  single-instance 2.5.4 / global-shortcut 2.4.1 / notification 2.7.0（notify-rust 4.18.2）/
  autostart 2.7.0 / objc2 0.6.4 / objc2-foundation 0.3.2 / objc2-user-notifications 0.3.2 /
  block2 0.6.2。
- `[lints.rust]` 若 Cargo 版本不接受该表，用 build.rs：
  `println!("cargo::rustc-check-cfg=cfg(r09_standalone_test)");`

## 2. `src/lib.rs`

### 2.1 模块声明

```diff
 pub mod commands;
 pub mod files;
+pub mod native;
 #[cfg_attr(not(feature = "r09-host"), allow(dead_code, unused_imports))]
 pub mod host;
```

### 2.2 Builder：**必须第一个 plugin 调用**（single-instance 要求最先注册）

```diff
 pub fn run() {
-    let builder = tauri::Builder::default()
+    // native 插件组（single-instance/global-shortcut/notification/autostart）
+    // 必须最先调用：single-instance 要求自己第一个注册。
+    let builder = native::install::with_plugins(tauri::Builder::default())
         .plugin(tauri_plugin_dialog::init())
```

（`with_plugins` 在隔离 `LINGXI_HOME` 下自动跳过 single-instance，保证「不同 home 可并行」；
生产默认 home 启用单实例。诊断事件见 `native/install.rs::isolation_mode`。）

### 2.3 setup：install 一次（manage state + 托盘 + 快捷键 + mac 通知 delegate）

```diff
         .setup(|app| {
             #[cfg(feature = "r09-host")]
             {
                 host::setup(app)?;
             }
+            native::install::install(app)?;
             #[cfg(not(feature = "r09-host"))]
             {
                 let _ = app;
             }
             Ok(())
         })
```

注意顺序：`host::setup` 先（服务启动异步），`native::install` 不阻塞（托盘/快捷键立即就绪）。
若 host-core 主窗创建在 `host::setup` 异步任务里，安装托盘/快捷键不依赖主窗存在（helper 内已处理）。

### 2.4 invoke_handler：把 `INVOKE_COMMANDS` 对应的函数加入裸名列表

host-core 现有风格是 `use commands::{...}` + 裸名。需要新增的命令函数（host-core 在
`src/commands.rs` 或新文件写，函数体调 `native::*`；签名见 PROGRESS §4.5）：

```rust
// 渲染层 boot/窗口（既有）…保持不变
// R09-T03/T04 新增（host-core 写函数，import 后裸名注册）：
reload_main_window, restart_app, open_settings, settings_changed,
show_notification, select_skill, open_folder, open_file, show_in_finder,
start_drag, get_keep_awake_status, set_keep_awake_enabled,
get_auto_launch_status, set_auto_launch_enabled,
keybindings_reload_global, quick_chat_reload_shortcut, keybindings_test_register,
quick_chat_shortcut_status, quick_chat_show, quick_chat_hide, quick_chat_resize,
quick_chat_open_session, quick_chat_open_session_ack,
get_splash_info, get_avatar_path, onboarding_complete,
speech_permission_status, speech_permission_request,
// debug 构建专属（`#[cfg(debug_assertions)]` 单独 generate_handler 分支或 cfg 属性）：
debug_open_onboarding, debug_open_onboarding_preview
```

`install.rs` 提供可直接复用的 helper（命令函数只做 label/DTO 校验 + 调这些）：
`focus_main, focus_primary, open_settings_window, show_quick_chat, hide_quick_chat,
register_quick_chat_shortcuts, show_notification, apply_window_theme, reveal_in_finder,
load_window_state, save_window_state, lingxi_home, preferences_json(私有), current_locale`。
状态类型 `native::install::NativeState` 由 install 已 `manage`，命令签名用
`State<'_, native::install::NativeState>`（`keep_awake/speech/computer_use/remote_boundary/
quick_chat_mode/handoff/registered_shortcuts/speech_tcc` 字段）。

### 2.5 窗口事件：主窗关闭策略 + 窗口状态保存

```rust
.on_window_event(|window, event| {
    match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            let flags = native::window_lifecycle::LifecycleFlags {
                quitting: /* host-core 退出标志 */,
                updating: /* 更新标志 */,
                force_quit: false,
            };
            if native::window_lifecycle::main_close_disposition(flags, /* tray_available */ true)
                == native::window_lifecycle::CloseDisposition::Hide
            {
                api.prevent_close();
                let _ = window.hide();
                // 从属窗一起隐藏：native::window_lifecycle::windows_hidden_on_main_close
            }
        }
        tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_)
            if window.label() == native::window_labels::MAIN =>
        {
            // 500ms 防抖后：native::install::save_window_state(window)
        }
        // 既有 Destroyed 清理保持
        _ => {}
    }
    /* 既有 Destroyed 分支 … */
})
```

窗口创建（main/splash/onboarding 在 host-core 侧）：用
`native::install::windows()` 的 `WindowSpec` 作为唯一配置来源（main 960x820 min 420x500、
创建时 `visible=false` 等 `app_ready`；quick-chat 480x142 transparent/always-on-top）。
主窗恢复位置：`native::install::load_window_state(&window)` → `set_position/set_size`。

### 2.6 macOS Info.plist（release/dev 打包配置；host-core/Lead 写 tauri.release.conf.json）

```xml
<key>NSSpeechRecognitionUsageDescription</key>
<string>灵犀使用系统语音识别把你的语音转换为文字。</string>
<key>NSMicrophoneUsageDescription</key>
<string>灵犀在你按下录音时使用麦克风采集语音。</string>
```

## 3. `build.rs` AppManifest

在既有 `commands(&[...])` 列表追加（与 invoke_handler 完全同名同集合；release 不含 debug 两项，
用 `cfg!(debug_assertions)` 或 feature 门控——**AppManifest 构造在 build 期，推荐用环境变量
`PROFILE`/`TAURI_ENV_DEBUG` 判断 debug 构建**；若难以区分，release capability 不授予 debug 命令 +
`cfg(debug_assertions)` 注册也可满足 A12）：

```text
reload_main_window restart_app open_settings settings_changed show_notification
select_skill open_folder open_file show_in_finder start_drag
get_keep_awake_status set_keep_awake_enabled get_auto_launch_status set_auto_launch_enabled
keybindings_reload_global quick_chat_reload_shortcut keybindings_test_register
quick_chat_shortcut_status quick_chat_show quick_chat_hide quick_chat_resize
quick_chat_open_session quick_chat_open_session_ack
get_splash_info get_avatar_path onboarding_complete
speech_permission_status speech_permission_request
debug_open_onboarding debug_open_onboarding_preview   # dev 构建
```

同时 `permissions/autogenerated/allow-<command>.toml` 需要每条命令一个（Tauri 自动生成或手写模板；
R06 已有 26 个模板）。

## 4. capabilities（每窗口 allow-* 草案）

窗口 label 唯一来源：`native::window_labels`（main/settings/quick-chat/browser-viewer/
viewer-*/skill-viewer-*/splash/onboarding）。前端**不**下放 notification/global-shortcut/
autostart 插件权限（全部 Rust 侧）。

### 4.1 `capabilities/main.json`（在现有列表追加）

```json
"allow-reload-main-window", "allow-restart-app", "allow-open-settings",
"allow-settings-changed", "allow-show-notification", "allow-select-skill",
"allow-open-folder", "allow-open-file", "allow-show-in-finder", "allow-start-drag",
"allow-get-keep-awake-status", "allow-set-keep-awake-enabled",
"allow-get-auto-launch-status", "allow-set-auto-launch-enabled",
"allow-keybindings-reload-global", "allow-quick-chat-reload-shortcut",
"allow-keybindings-test-register", "allow-quick-chat-shortcut-status",
"allow-quick-chat-show", "allow-quick-chat-hide", "allow-quick-chat-resize",
"allow-quick-chat-open-session", "allow-quick-chat-open-session-ack",
"allow-get-splash-info", "allow-get-avatar-path", "allow-onboarding-complete",
"allow-speech-permission-status", "allow-speech-permission-request"
```

### 4.2 `capabilities/settings.json`（新建，`windows:["settings"]`）

`core:default` + 以下子集：`open-settings, settings-changed, show-notification, select-skill,
get-keep-awake-status, set-keep-awake-enabled, get-auto-launch-status, set-auto-launch-enabled,
keybindings-*, quick-chat-shortcut-status, open-folder, open-file, show-in-finder,
restart-app, speech-permission-status`（不给 reload-main-window/quick-chat-* 窗口控制）。

### 4.3 `capabilities/quick-chat.json`（新建，`windows:["quick-chat"]`）

`core:default` + `allow-quick-chat-show, allow-quick-chat-hide, allow-quick-chat-resize,
allow-quick-chat-open-session, allow-settings-changed, allow-open-external`。

### 4.4 `capabilities/splash.json` / `onboarding.json`（新建）

`core:default` + `allow-get-splash-info, allow-get-avatar-path, allow-onboarding-complete,
allow-settings-changed`；debug 命令只在 dev capability（或只授予 dev 构建的 onboarding capability）。

### 4.5 既有文件

`browser-viewer.json` 保持 T05 现状（如需接收主题广播，加 `allow-settings-changed`）；
`viewer.json`/`skill-viewer.json` 不加新命令。

## 5. 合并顺序（最小风险）

1. Cargo.toml 依赖 + `[lints.rust]` → `cargo update` + `cargo build --locked`（此时 native 仅编译，
   无注册；若报错按错误修，不静默降级）。
2. `pub mod native;` + `native::install::with_plugins` 包裹 Builder 首行 + `install(app)?`。
3. 写命令函数（PROGRESS §4.5 签名契约）→ invoke_handler 裸名 + build.rs + capabilities 四集合一致。
4. `cargo build --locked --features r09-host` + `r09_capability_audit.py` + R06 `cargo test --test desktop_host`
   （R06 行为不回退）。
5. 把 `r09_t03.py`/`r09_t04.py` 的 `blocked::` 去掉重跑 → Lead 新目录跑 `verify-stage R09`。

## 6. 合并后仍需 host-core 决定的实现细节

- 主窗创建时序：`native::install::install` 在 setup 同步执行；主窗仍由 `host::setup` 异步创建
  （托盘/快捷键不依赖主窗已存在）。
- 退出路径：`window_lifecycle::QuitPlan` 与 T02 `service/shutdown.rs` 对接（owned-only 清理、
  remote/reused 零接触），不要在 `native/` 里自行杀进程。
- 更新标志（`_isUpdating`）与 `LifecycleFlags.updating` 的接线点由 host-core 提供（updater 属 R10）。
- 通知展示：mac 用 `native::install::show_notification`（UNUserNotificationCenter delegate 点击聚焦）；
  其它平台同一 helper 内部回落插件。
- 事件名/载荷与 TS 片段 `desktop/src/react/host/tauri-host-capabilities.ts` 保持一致
  （`settings-changed` 事件 payload `{type,data}`、`open-settings-modal {tab}`、
  `settings-switch-tab {tab}`、`quick-chat-shown {}`、`quick-chat-open-session <request>`）。

//! R09-T03/T04 桌面宿主能力模块（Tauri 产品宿主 `DesktopHost-Tauri` 的原生面）。
//!
//! 结构约定（Lead 2026-10-26 裁决）：本目录属于 T03/T04 实现者；T01/T02 的
//! 产品宿主生命周期在 `src/host.rs`（host-core 独占），两者互不改写。
//!
//! 每个能力一个文件，文件内只有两类内容：
//! 1. **纯逻辑**：参数/窗口身份/路径/URL/枚举校验、状态机、菜单模型、事件
//!    白名单等；不依赖 tauri，可用 `rustc --test` 独立编译运行（A06 的 IPC
//!    负向证据、T04 的权限状态机证据均来自这些单测）。
//! 2. `#[cfg(not(r09_standalone_test))] mod tauri_glue`：真实命令/插件接线；
//!    `r09_standalone_test` 只由证据生产者独立验证时设置，正式构建包含 glue。
//!
//! 注册点不在这里：`mod native;`、`invoke_handler`、`build.rs` AppManifest、
//! `capabilities/*.json`、插件初始化与窗口创建由 host-core 按
//! `docs/rust-tauri/R09/T03_T04_PROGRESS.md` §注册清单合并，本目录禁止自注册。

pub mod app_info;
pub mod app_ready;
pub mod argument_guard;
pub mod auto_launch;
pub mod clipboard_edit;
pub mod drag_out;
pub mod errors;
pub mod external_open;
pub mod file_dialogs;
pub mod file_drop;
pub mod global_shortcut;
pub mod i18n;
/// host-core 注册安装面（tauri 依赖，仅在真实 crate 构建；独立验证时排除）。
#[cfg(not(r09_standalone_test))]
pub mod install;
pub mod ipc_negative_matrix;
pub mod keep_awake;
pub mod notification;
/// OQ-03 有界 spike：macOS 通知点击聚焦（UNUserNotificationCenter delegate）。
#[cfg(all(not(r09_standalone_test), target_os = "macos"))]
pub mod notification_center_macos;
pub mod onboarding;
pub mod permissions;
pub mod quick_chat;
pub mod quick_chat_handoff;
pub mod reload_restart;
pub mod remote_boundary;
pub mod screenshot;
pub mod settings_broadcast;
pub mod settings_window;
pub mod single_instance;
pub mod speech;
pub mod splash;
pub mod theme_sync;
pub mod trash;
pub mod tray;
pub mod window_controls;
pub mod window_labels;
pub mod window_lifecycle;
pub mod window_state;

pub use errors::{deny, HostError, HostResult};

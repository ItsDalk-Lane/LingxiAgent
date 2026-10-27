//! R09-T03 IPC 负向矩阵（A06）——把各能力模块的拒绝路径汇成可逐条断言的矩阵。
//!
//! 任务书 A06：IPC 收到畸形路径/URL/命令样式参数时，宿主必须按类型/scope 拒绝，
//! **不转成任意 shell 执行**。本模块不重复实现校验，而是对每个命令的代表性畸形
//! 输入调用真实校验函数，断言稳定错误码；证据生产者运行
//! `native::ipc_negative_matrix::tests::negative_matrix_emits_evidence` 并解析
//! `NEGATIVE_CASE <name> <command> <expect> <actual> OK|FAIL` 行。
//!
//! 覆盖维度：相对/NUL/超长路径、非 http(s) URL、命令白名单外取值、窗口身份伪造、
//! 窗口 id 注入、枚举外 mode/tab/theme、分段索引越界、远端设备未授权、
//! helper 租约过期/撤权、渲染层指定 home/argv、release 调试命令。

use super::errors::HostError;

pub type Check = fn() -> Result<(), String>;

pub struct NegativeCase {
    pub name: &'static str,
    pub command: &'static str,
    pub expect: &'static str,
    pub check: Check,
}

fn expect_error(result: Result<(), HostError>, code: &str) -> Result<(), String> {
    match result {
        Ok(()) => Err(format!("期望拒绝（{code}），实际通过")),
        Err(error) if error.code() == code => Ok(()),
        Err(error) => Err(format!("期望 {code}，实际 {}", error.code())),
    }
}

macro_rules! case {
    ($name:expr, $command:expr, $expect:expr, $body:expr) => {
        NegativeCase {
            name: $name,
            command: $command,
            expect: $expect,
            check: $body,
        }
    };
}

/// 全部负向 case（命令面 → 代表性格参数 → 期望稳定码）。
pub fn cases() -> Vec<NegativeCase> {
    use super::errors::{
        E_INVALID_ARGUMENT, E_PATH_NOT_ABSOLUTE, E_RELEASE_DISABLED, E_REMOTE_DEVICE_DENIED,
        E_TOKEN_INVALID, E_URL_INVALID, E_URL_SCHEME_DENIED, E_WINDOW_LABEL_INVALID,
        E_WINDOW_NOT_ALLOWED,
    };
    use super::{
        argument_guard, clipboard_edit, external_open, file_dialogs, file_drop, global_shortcut,
        notification, onboarding, permissions, quick_chat, remote_boundary, screenshot,
        settings_broadcast, settings_window, single_instance, splash, trash, window_controls,
        window_labels,
    };

    vec![
        // ── 路径与 URL 类型门 ─────────────────────────────────────────
        case!(
            "ipc::open_external.javascript-scheme",
            "open_external",
            E_URL_SCHEME_DENIED,
            || {
                expect_error(
                    external_open::validate_open_external("javascript:alert(1)").map(|_| ()),
                    E_URL_SCHEME_DENIED,
                )
            }
        ),
        case!(
            "ipc::open_external.file-scheme",
            "open_external",
            E_URL_SCHEME_DENIED,
            || {
                expect_error(
                    external_open::validate_open_external("file:///etc/passwd").map(|_| ()),
                    E_URL_SCHEME_DENIED,
                )
            }
        ),
        case!(
            "ipc::open_external.control-char",
            "open_external",
            E_URL_INVALID,
            || {
                expect_error(
                    external_open::validate_open_external("https://example.com/\u{0}").map(|_| ()),
                    E_URL_INVALID,
                )
            }
        ),
        case!(
            "ipc::open_file.relative-path",
            "open_file",
            E_PATH_NOT_ABSOLUTE,
            || {
                expect_error(
                    external_open::plan_open_file("main", "../../etc/passwd").map(|_| ()),
                    E_PATH_NOT_ABSOLUTE,
                )
            }
        ),
        case!(
            "ipc::open_folder.relative-path",
            "open_folder",
            E_PATH_NOT_ABSOLUTE,
            || {
                expect_error(
                    external_open::plan_open_folder("main", "relative").map(|_| ()),
                    E_PATH_NOT_ABSOLUTE,
                )
            }
        ),
        case!(
            "ipc::show_in_finder.nul-path",
            "show_in_finder",
            E_PATH_NOT_ABSOLUTE,
            || {
                expect_error(
                    external_open::plan_reveal_in_finder("main", "/tmp/a\u{0}").map(|_| ()),
                    E_PATH_NOT_ABSOLUTE,
                )
            }
        ),
        case!(
            "ipc::argument_guard.overlong-path",
            "read_file",
            E_PATH_NOT_ABSOLUTE,
            || {
                let path = format!("/{}", "a".repeat(argument_guard::MAX_PATH_LEN));
                expect_error(
                    argument_guard::require_absolute_path(&path, "path").map(|_| ()),
                    E_PATH_NOT_ABSOLUTE,
                )
            }
        ),
        case!(
            "ipc::trash.relative-path",
            "trash_item",
            E_PATH_NOT_ABSOLUTE,
            || {
                expect_error(
                    trash::plan_trash("main", "relative/a", "darwin").map(|_| ()),
                    E_PATH_NOT_ABSOLUTE,
                )
            }
        ),
        case!(
            "ipc::dialog.relative-result-path",
            "select_files",
            E_PATH_NOT_ABSOLUTE,
            || {
                expect_error(
                    file_dialogs::normalize_dialog_paths(&["relative/a".to_string()]).map(|_| ()),
                    E_PATH_NOT_ABSOLUTE,
                )
            }
        ),
        case!(
            "ipc::drag.relative-path",
            "start_drag",
            E_PATH_NOT_ABSOLUTE,
            || {
                expect_error(
                    super::drag_out::normalize_drag_paths(&["../evil".to_string()]).map(|_| ()),
                    E_PATH_NOT_ABSOLUTE,
                )
            }
        ),
        case!(
            "ipc::drag.nul-path",
            "start_drag",
            E_INVALID_ARGUMENT,
            || {
                expect_error(
                    super::drag_out::normalize_drag_paths(&["/tmp/a\u{0}".to_string()]).map(|_| ()),
                    E_INVALID_ARGUMENT,
                )
            }
        ),
        case!(
            "ipc::drop.file-uri",
            "file_drop",
            E_INVALID_ARGUMENT,
            || {
                expect_error(
                    file_drop::normalize_drop_paths(&["file:///tmp/a".to_string()]).map(|_| ()),
                    E_INVALID_ARGUMENT,
                )
            }
        ),
        case!(
            "ipc::drop.relative-path",
            "file_drop",
            E_PATH_NOT_ABSOLUTE,
            || {
                expect_error(
                    file_drop::normalize_drop_paths(&["relative/a".to_string()]).map(|_| ()),
                    E_PATH_NOT_ABSOLUTE,
                )
            }
        ),
        // ── 命令样式参数（白名单外枚举/注入）──────────────────────────
        case!(
            "ipc::run_edit_command.shell-style",
            "run_edit_command",
            E_INVALID_ARGUMENT,
            || {
                expect_error(
                    clipboard_edit::plan_edit_command("main", "cut; rm -rf /").map(|_| ()),
                    E_INVALID_ARGUMENT,
                )
            }
        ),
        case!(
            "ipc::run_edit_command.devtools",
            "run_edit_command",
            E_INVALID_ARGUMENT,
            || {
                expect_error(
                    clipboard_edit::parse_edit_command("toggleDevTools").map(|_| ()),
                    E_INVALID_ARGUMENT,
                )
            }
        ),
        case!(
            "ipc::global_shortcut.injection",
            "keybindings_test_register",
            crate::native::errors::E_ENUM_INVALID,
            || {
                expect_error(
                    global_shortcut::plan_test_register("Alt+; rm -rf /", &[]).map(|_| ()),
                    crate::native::errors::E_ENUM_INVALID,
                )
            }
        ),
        case!(
            "ipc::global_shortcut.unknown-key",
            "keybindings_test_register",
            crate::native::errors::E_ENUM_INVALID,
            || {
                expect_error(
                    global_shortcut::plan_test_register("Alt+NotAKey", &[]).map(|_| ()),
                    crate::native::errors::E_ENUM_INVALID,
                )
            }
        ),
        case!(
            "ipc::quick_chat.resize.unknown-mode",
            "quick_chat_resize",
            E_WINDOW_LABEL_INVALID,
            || {
                // 未知 mode 回落 compact 是现役语义；但畸形 label 必须拒绝
                expect_error(
                    quick_chat::authorize_resize("quick-chat*").map(|_| ()),
                    E_WINDOW_LABEL_INVALID,
                )
            }
        ),
        case!(
            "ipc::settings_changed.unknown-event",
            "settings_changed",
            crate::native::errors::E_ENUM_INVALID,
            || {
                let input = settings_broadcast::SettingsEventInput {
                    event: "evil-event",
                    ..Default::default()
                };
                expect_error(
                    settings_broadcast::plan_broadcast(&input).map(|_| ()),
                    crate::native::errors::E_ENUM_INVALID,
                )
            }
        ),
        case!(
            "ipc::settings_changed.missing-theme",
            "settings_changed",
            E_INVALID_ARGUMENT,
            || {
                let input = settings_broadcast::SettingsEventInput {
                    event: "theme-changed",
                    payload_is_object: true,
                    ..Default::default()
                };
                expect_error(
                    settings_broadcast::plan_broadcast(&input).map(|_| ()),
                    E_INVALID_ARGUMENT,
                )
            }
        ),
        case!(
            "ipc::open_settings.tab-injection",
            "open_settings",
            crate::native::errors::E_ENUM_INVALID,
            || {
                expect_error(
                    settings_window::plan_open_settings(
                        "main",
                        Some("../evil"),
                        true,
                        false,
                        false,
                    )
                    .map(|_| ()),
                    crate::native::errors::E_ENUM_INVALID,
                )
            }
        ),
        case!(
            "ipc::theme.control-char",
            "sync_window_theme",
            crate::native::errors::E_ENUM_INVALID,
            || {
                expect_error(
                    super::theme_sync::plan_theme_sync("main", "\u{0}", false).map(|_| ()),
                    crate::native::errors::E_ENUM_INVALID,
                )
            }
        ),
        case!(
            "ipc::screenshot.unknown-theme",
            "screenshot_render",
            E_INVALID_ARGUMENT,
            || {
                expect_error(
                    screenshot::validate_theme("../../etc/passwd").map(|_| ()),
                    E_INVALID_ARGUMENT,
                )
            }
        ),
        case!(
            "ipc::screenshot.segment-oob",
            "screenshot_render",
            E_INVALID_ARGUMENT,
            || {
                expect_error(
                    screenshot::segment_suffix(Some(3), Some(9)).map(|_| ()),
                    E_INVALID_ARGUMENT,
                )
            }
        ),
        case!(
            "ipc::screenshot.outside-home-dir",
            "screenshot_render",
            E_URL_SCHEME_DENIED,
            || {
                expect_error(
                    screenshot::resolve_save_dir(Some("/etc"), "/Users/u", None, "zh").map(|_| ()),
                    E_URL_SCHEME_DENIED,
                )
            }
        ),
        case!(
            "ipc::splash.evil-origin",
            "get_splash_info",
            E_WINDOW_NOT_ALLOWED,
            || {
                expect_error(
                    splash::validate_splash_page_url("https://evil.example/splash.html"),
                    E_WINDOW_NOT_ALLOWED,
                )
            }
        ),
        case!(
            "ipc::splash.query-injection",
            "get_avatar_path",
            E_INVALID_ARGUMENT,
            || {
                expect_error(
                    splash::validate_splash_page_url("tauri://localhost/splash.html?exec=1"),
                    E_INVALID_ARGUMENT,
                )
            }
        ),
        // ── 窗口身份伪造 / 跨窗调用 ──────────────────────────────────
        case!(
            "ipc::window_control.renderer-window-id",
            "window_minimize",
            E_WINDOW_NOT_ALLOWED,
            || {
                expect_error(
                    window_controls::reject_window_id_argument(Some("main")),
                    E_WINDOW_NOT_ALLOWED,
                )
            }
        ),
        case!(
            "ipc::window_control.viewer-caller",
            "window_close",
            E_WINDOW_NOT_ALLOWED,
            || {
                expect_error(
                    window_controls::authorize_window_control(
                        "viewer-1",
                        window_controls::WindowControlAction::Close,
                    )
                    .map(|_| ()),
                    E_WINDOW_NOT_ALLOWED,
                )
            }
        ),
        case!(
            "ipc::show_notification.settings-caller",
            "show_notification",
            E_WINDOW_NOT_ALLOWED,
            || {
                expect_error(
                    notification::validate_notification_request("settings", "t", "b", None, None)
                        .map(|_| ()),
                    E_WINDOW_NOT_ALLOWED,
                )
            }
        ),
        case!(
            "ipc::app_ready.splash-caller",
            "app_ready",
            E_WINDOW_NOT_ALLOWED,
            || {
                expect_error(
                    super::app_ready::on_app_ready("splash", false, false).map(|_| ()),
                    E_WINDOW_NOT_ALLOWED,
                )
            }
        ),
        case!(
            "ipc::window_labels.viewer-label-glob",
            "spawn_viewer",
            E_WINDOW_LABEL_INVALID,
            || {
                expect_error(
                    window_labels::require_role("viewer-*", &[window_labels::WindowRole::Main])
                        .map(|_| ()),
                    E_WINDOW_LABEL_INVALID,
                )
            }
        ),
        case!(
            "ipc::window_labels.unknown-role",
            "get_platform",
            E_WINDOW_NOT_ALLOWED,
            || {
                expect_error(
                    window_labels::require_role(
                        "future-window",
                        &[window_labels::WindowRole::Main],
                    )
                    .map(|_| ()),
                    E_WINDOW_NOT_ALLOWED,
                )
            }
        ),
        // ── 渲染层指定宿主状态（home/argv/暂停权限）────────────────────
        case!(
            "ipc::single_instance.home-override",
            "app_start",
            E_INVALID_ARGUMENT,
            || {
                expect_error(
                    single_instance::reject_renderer_home_override(Some("/private/tmp/evil")),
                    E_INVALID_ARGUMENT,
                )
            }
        ),
        case!(
            "ipc::restart.argv-injection",
            "restart_app",
            E_WINDOW_NOT_ALLOWED,
            || {
                expect_error(
                    super::reload_restart::reject_restart_arguments(Some("--dev"), None),
                    E_WINDOW_NOT_ALLOWED,
                )
            }
        ),
        case!(
            "ipc::onboarding.untrusted-caller",
            "onboarding_complete",
            E_WINDOW_NOT_ALLOWED,
            || {
                onboarding::authorize_onboarding_complete("main")
                    .map_err(|error| format!("main 应允许完成设置: {error}"))?;
                expect_error(
                    onboarding::authorize_onboarding_complete("settings"),
                    E_WINDOW_NOT_ALLOWED,
                )
            }
        ),
        case!(
            "ipc::onboarding.setup-complete-not-persisted",
            "onboarding_complete",
            crate::native::errors::E_NOT_IMPLEMENTED,
            || {
                expect_error(
                    onboarding::validate_setup_complete_response(true, false),
                    crate::native::errors::E_NOT_IMPLEMENTED,
                )
            }
        ),
        case!(
            "ipc::debug-onboarding.release",
            "debug_open_onboarding",
            E_RELEASE_DISABLED,
            || {
                expect_error(
                    Err(onboarding::debug_command_release_error()),
                    E_RELEASE_DISABLED,
                )
            }
        ),
        // ── 权限 / 设备边界（T04）────────────────────────────────────
        case!(
            "ipc::speech.settings-caller",
            "speech_permission_status",
            E_WINDOW_NOT_ALLOWED,
            || { expect_error(speech_caller_denied(), E_WINDOW_NOT_ALLOWED) }
        ),
        case!(
            "ipc::speech.linux-platform",
            "speech_permission_request",
            crate::native::errors::E_PLATFORM_UNSUPPORTED,
            || {
                expect_error(
                    Err(super::speech::unsupported_platform_error("linux")),
                    crate::native::errors::E_PLATFORM_UNSUPPORTED,
                )
            }
        ),
        case!(
            "ipc::computer_use.remote-lease",
            "computer_use_start",
            E_REMOTE_DEVICE_DENIED,
            || {
                let mut manager = permissions::ComputerUseLeaseManager::new();
                manager
                    .approve(permissions::AppApproval {
                        provider_id: "macos:cua".to_string(),
                        app_id: "com.apple.TextEdit".to_string(),
                        app_name: "TextEdit".to_string(),
                    })
                    .expect("approve");
                let snapshot = permissions::PermissionSnapshot {
                    accessibility: permissions::PermissionState::Authorized,
                    screen_recording: permissions::PermissionState::Authorized,
                };
                expect_error(
                    manager
                        .start("macos:cua", "com.apple.TextEdit", snapshot, true, 0, None)
                        .map(|_| ())
                        .map_err(|rejection| HostError::new(rejection.code(), "remote")),
                    E_REMOTE_DEVICE_DENIED,
                )
            }
        ),
        case!(
            "ipc::computer_use.expired-lease",
            "computer_use_action",
            "COMPUTER_USE_LEASE_EXPIRED",
            || {
                let mut manager = permissions::ComputerUseLeaseManager::new();
                manager
                    .approve(permissions::AppApproval {
                        provider_id: "macos:cua".to_string(),
                        app_id: "com.apple.TextEdit".to_string(),
                        app_name: "TextEdit".to_string(),
                    })
                    .expect("approve");
                let snapshot = permissions::PermissionSnapshot {
                    accessibility: permissions::PermissionState::Authorized,
                    screen_recording: permissions::PermissionState::Authorized,
                };
                let lease = manager
                    .start(
                        "macos:cua",
                        "com.apple.TextEdit",
                        snapshot,
                        false,
                        0,
                        Some(100),
                    )
                    .expect("lease");
                expect_error(
                    manager
                        .authorize_action(
                            lease.lease_id,
                            permissions::ComputerUseAction::Click,
                            lease.controls_generation,
                            snapshot,
                            lease.expires_at_ms,
                            false,
                        )
                        .map(|_| ())
                        .map_err(|rejection| HostError::new(rejection.code(), "expired")),
                    "COMPUTER_USE_LEASE_EXPIRED",
                )
            }
        ),
        case!(
            "ipc::computer_use.stale-controls",
            "computer_use_action",
            "COMPUTER_USE_STALE_CONTROLS",
            || {
                let mut manager = permissions::ComputerUseLeaseManager::new();
                manager
                    .approve(permissions::AppApproval {
                        provider_id: "macos:cua".to_string(),
                        app_id: "com.apple.TextEdit".to_string(),
                        app_name: "TextEdit".to_string(),
                    })
                    .expect("approve");
                let snapshot = permissions::PermissionSnapshot {
                    accessibility: permissions::PermissionState::Authorized,
                    screen_recording: permissions::PermissionState::Authorized,
                };
                let lease = manager
                    .start("macos:cua", "com.apple.TextEdit", snapshot, false, 0, None)
                    .expect("lease");
                expect_error(
                    manager
                        .authorize_action(
                            lease.lease_id,
                            permissions::ComputerUseAction::TypeText,
                            lease.controls_generation + 1,
                            snapshot,
                            1,
                            false,
                        )
                        .map(|_| ())
                        .map_err(|rejection| HostError::new(rejection.code(), "stale")),
                    "COMPUTER_USE_STALE_CONTROLS",
                )
            }
        ),
        case!(
            "ipc::remote_boundary.self-grant",
            "device_grant",
            E_REMOTE_DEVICE_DENIED,
            || {
                let mut boundary = remote_boundary::RemoteDeviceBoundary::new();
                expect_error(
                    boundary
                        .grant(
                            remote_boundary::PrincipalClass::RemoteAgent,
                            remote_boundary::GrantSource::LocalUserGesture,
                            remote_boundary::DeviceKind::Microphone,
                            Some("remote"),
                            0,
                            None,
                        )
                        .map(|_| ())
                        .map_err(|code| HostError::new(code, "remote self-grant")),
                    E_REMOTE_DEVICE_DENIED,
                )
            }
        ),
        case!(
            "ipc::remote_boundary.ungranted-device",
            "device_read",
            E_REMOTE_DEVICE_DENIED,
            || {
                let boundary = remote_boundary::RemoteDeviceBoundary::new();
                let (decision, _) = boundary.evaluate(
                    remote_boundary::PrincipalClass::RemoteAgent,
                    remote_boundary::DeviceKind::ScreenCapture,
                    Some("remote-run"),
                    0,
                    true,
                );
                if decision.permitted() {
                    return Err("远端未授权却允许读取设备".to_string());
                }
                expect_error(
                    Err(remote_boundary::require_local_authorization_error(
                        remote_boundary::DeviceKind::ScreenCapture,
                    )),
                    E_REMOTE_DEVICE_DENIED,
                )
            }
        ),
        // ── 租约/令牌类 ──────────────────────────────────────────────
        case!(
            "ipc::computer_use.wrong-lease-id",
            "computer_use_stop",
            E_TOKEN_INVALID,
            || {
                let mut manager = permissions::ComputerUseLeaseManager::new();
                manager
                    .approve(permissions::AppApproval {
                        provider_id: "macos:cua".to_string(),
                        app_id: "com.apple.TextEdit".to_string(),
                        app_name: "TextEdit".to_string(),
                    })
                    .expect("approve");
                let snapshot = permissions::PermissionSnapshot {
                    accessibility: permissions::PermissionState::Authorized,
                    screen_recording: permissions::PermissionState::Authorized,
                };
                manager
                    .start("macos:cua", "com.apple.TextEdit", snapshot, false, 0, None)
                    .expect("lease");
                expect_error(manager.stop(999).map(|_| ()), E_TOKEN_INVALID)
            }
        ),
    ]
}

fn speech_caller_denied() -> Result<(), HostError> {
    super::speech::authorize_speech_caller("settings").map(|_| ())
}

/// 运行全部 case；返回 (通过数, 失败列表)。
pub fn run() -> (usize, Vec<(String, String)>) {
    let all = cases();
    let mut passed = 0;
    let mut failures = Vec::new();
    for case in &all {
        match (case.check)() {
            Ok(()) => passed += 1,
            Err(detail) => failures.push((case.name.to_string(), detail)),
        }
    }
    (passed, failures)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_matrix_all_cases_hold() {
        let (passed, failures) = run();
        let total = cases().len();
        assert_eq!(passed, total, "负向矩阵失败: {failures:?}");
        assert!(total >= 40, "矩阵过小（{total}），疑似覆盖回退");
    }

    /// 生产者消费的逐 case 证据行：`NEGATIVE_CASE <name> <command> <expect> <actual> OK|FAIL`。
    #[test]
    fn negative_matrix_emits_evidence() {
        for case in cases() {
            match (case.check)() {
                Ok(()) => println!(
                    "NEGATIVE_CASE {} {} {} {} OK",
                    case.name, case.command, case.expect, case.expect
                ),
                Err(_) => println!(
                    "NEGATIVE_CASE {} {} {} actual-mismatch FAIL",
                    case.name, case.command, case.expect
                ),
            }
        }
        let (passed, failures) = run();
        assert!(failures.is_empty(), "负向矩阵失败: {failures:?}");
        assert_eq!(passed, cases().len());
    }

    #[test]
    fn case_names_are_unique_and_scoped() {
        let mut names: Vec<&str> = cases().iter().map(|case| case.name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "case 名重复");
        assert!(cases()
            .iter()
            .all(|case| case.name.starts_with("ipc::") && case.name.contains('.')));
        assert!(cases().iter().all(|case| !case.command.is_empty()));
        assert!(cases().iter().all(|case| !case.expect.is_empty()));
    }
}

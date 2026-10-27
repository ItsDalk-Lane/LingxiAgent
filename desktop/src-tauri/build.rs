fn main() {
    // 命令名唯一清单（与 src/lib.rs `generate_handler!` 同名同集合；T06 四集合一致）。
    let mut commands: Vec<&str> = vec![
        "open_skill_viewer",
        "skill_viewer_load",
        "list_skill_files",
        "read_skill_file",
        "skill_viewer_close",
        "select_files",
        "select_folder",
        "read_file",
        "read_file_snapshot",
        "read_file_base64",
        "read_docx_html",
        "read_xlsx_html",
        "file_info",
        "write_file",
        "write_file_binary",
        "copy_file",
        "write_file_if_unchanged",
        "open_file",
        "trash_item",
        "run_edit_command",
        "watch_file",
        "unwatch_file",
        "watch_workspace",
        "unwatch_workspace",
        "resource_subscribe",
        "resource_events",
        "resource_unsubscribe",
        "spawn_viewer",
        "viewer_request_load",
        "viewer_close",
        // R09-T01/T02 产品宿主命令。
        "get_server_connection_info",
        "get_server_port",
        "get_server_token",
        "get_service_status",
        "get_platform",
        "get_app_version",
        "app_ready",
        "window_minimize",
        "window_maximize",
        "window_close",
        "window_is_maximized",
        "window_theme_changed",
        "open_external",
        // R09-T05 浏览器控制面板宿主桥（权限按 main/browser-viewer 分组）。
        "set_browser_connection",
        "get_browser_viewer_context",
        "open_browser_viewer",
        "rust_browser_request",
        "close_browser_viewer",
        // R09-T03/T04 原生能力命令面（native::install::INVOKE_COMMANDS 唯一清单）。
        "reload_main_window",
        "restart_app",
        "open_settings",
        "settings_changed",
        "show_notification",
        "select_skill",
        "open_folder",
        "show_in_finder",
        "start_drag",
        "get_keep_awake_status",
        "set_keep_awake_enabled",
        "get_auto_launch_status",
        "set_auto_launch_enabled",
        "keybindings_reload_global",
        "quick_chat_reload_shortcut",
        "keybindings_test_register",
        "quick_chat_shortcut_status",
        "quick_chat_show",
        "quick_chat_hide",
        "quick_chat_resize",
        "quick_chat_open_session",
        "quick_chat_open_session_ack",
        "get_splash_info",
        "get_avatar_path",
        "onboarding_complete",
        "speech_permission_status",
        "speech_permission_request",
    ];
    // R09-T06 D11：测试注入命令只在 r09-test-hooks 构建注册；交付 release 不含。
    if std::env::var_os("CARGO_FEATURE_R09_TEST_HOOKS").is_some() {
        commands.extend([
            "debug_open_onboarding",
            "debug_open_onboarding_preview",
            "debug_asset_probe",
        ]);
    }
    // AppManifest::commands 需要 `&'static [&'static str]`；命令清单在 build
    // 脚本内构造一次后 leak（进程级只执行一次，无回收需求）。
    let commands: &'static [&'static str] = Box::leak(commands.into_boxed_slice());
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&commands)),
    )
    .expect("desktop command registration failed");
}

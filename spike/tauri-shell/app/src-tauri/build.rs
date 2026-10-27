fn main() {
    // 自定义命令必须显式纳入 ACL（02 §9 / W02）：一旦在 AppManifest 登记，
    // 这些命令不再是 "default allowed"，必须经 capability 逐项授权。
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "shell_public_info",
            "shell_sensitive_probe",
            "shell_secondary_probe",
            "sidecar_status",
            "sidecar_ping",
            "sidecar_restart",
            "finish_e2e",
        ])),
    )
    .expect("tauri-build failed");
}

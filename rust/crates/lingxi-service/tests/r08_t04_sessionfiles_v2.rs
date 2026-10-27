//! R08-T04 B2 切片A：sessionfiles 扩展（ImportLegacyReportV2 /
//! import_legacy_sidecar_v2 / import_legacy_session_files_dir）集成测试。
//!
//! 覆盖（A07 附件坏因面）：
//! - 好 payload：imported、无 broken、status available、字节保真复制可读；
//! - payload 缺失：broken/payload-missing，行 status=missing（不冒充可用）；
//! - 声明 size 与实字不符：broken/payload-truncated，status=broken；
//! - 声明 sha256 不符：broken/payload-truncated；
//! - 描述符 JSON 损坏：broken/sidecar-row-invalid，其余行仍导入；
//! - 行级 version≠1：broken/unsupported-version；
//! - 顶层 version≠1：UnsupportedSidecarVersion 硬错误（不降级）；
//! - 重跑幂等：skipped_existing，无重复行；
//! - Lead 条件①：V1 ImportReport serde 形状一字节不变（钉死）。

use std::path::PathBuf;
use std::sync::Arc;

use lingxi_service::sessionfiles::SessionFileService;

fn fresh_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r08-t04-sfv2-{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir home");
    dir
}

async fn service(home: PathBuf) -> SessionFileService {
    let db = lingxi_adapters::storage::RunDatabase::open(
        &home.join("runs.db"),
        lingxi_adapters::storage::StoreOptions::default(),
    )
    .await
    .expect("open db");
    SessionFileService::new(home, Arc::new(db), || 1_000)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// 造一个旧根 session-files/{hash} 目录：payload + meta.json 描述符。
fn seed_dir(
    root: &std::path::Path,
    dir: &str,
    entries: &[(&str, &str, &[u8], Option<u64>, Option<String>)], // (fileId, filename, payload, declared_size, declared_sha)
) -> PathBuf {
    let dir_path = root.join(dir);
    std::fs::create_dir_all(&dir_path).expect("mkdir dir");
    for (file_id, filename, payload, declared_size, declared_sha) in entries {
        if !payload.is_empty() || declared_size.is_none() {
            std::fs::write(dir_path.join(filename), payload).expect("write payload");
        }
        let mut meta = serde_json::json!({
            "version": 1,
            "fileId": file_id,
            "sessionId": "sess-legacy",
            "filename": filename,
            "mime": "application/octet-stream",
            "size": declared_size.unwrap_or(payload.len() as u64),
            "status": "available",
        });
        if let Some(sha) = declared_sha {
            meta["sha256"] = serde_json::json!(sha);
        } else {
            meta["sha256"] = serde_json::json!(sha256_hex(payload));
        }
        std::fs::write(
            dir_path.join(format!("{filename}.meta.json")),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .expect("write meta");
    }
    dir_path
}

#[tokio::test]
async fn v2_good_payload_imports_available_and_readable() {
    let home = fresh_home("good");
    let source_root = home.join("old/session-files");
    let dir = seed_dir(
        &source_root,
        "sfh-ab",
        &[("sf_good00000000001", "notes.md", b"hello world", None, None)],
    );
    let svc = service(home.clone()).await;
    let staging = home.join("staging");
    let report = svc
        .import_legacy_session_files_dir(&dir, &staging)
        .await
        .expect("import");
    assert_eq!(report.imported, 1);
    assert_eq!(report.skipped_existing, 0);
    assert!(report.broken.is_empty(), "broken: {:?}", report.broken);
    let row = svc
        .get_file("sess-legacy", "sf_good00000000001")
        .await
        .expect("get")
        .expect("present");
    assert_eq!(row.status, "available");
    let bytes = std::fs::read(&row.real_path).expect("read copied payload");
    assert_eq!(bytes, b"hello world", "payload 逐字节保真");
}

#[tokio::test]
async fn v2_missing_payload_reports_payload_missing() {
    let home = fresh_home("missing");
    let source_root = home.join("old/session-files");
    // 空 payload + declared size None → seed_dir 不写 payload 文件。
    let dir = seed_dir(
        &source_root,
        "sfh-miss",
        &[("sf_missing00000001", "ghost.bin", b"", None, None)],
    );
    // seed_dir 空 payload 且 size None 时不写文件 → 但描述符声明 size 0，
    // 显式删掉以模拟 payload-missing。
    let _ = std::fs::remove_file(dir.join("ghost.bin"));
    let svc = service(home.clone()).await;
    let report = svc
        .import_legacy_session_files_dir(&dir, &home.join("staging"))
        .await
        .expect("import");
    assert_eq!(report.imported, 1, "行仍登记（不整目录丢弃）");
    assert_eq!(report.broken.len(), 1);
    assert_eq!(report.broken[0].reason, "payload-missing");
    assert_eq!(report.broken[0].file_id, "sf_missing00000001");
    let row = svc
        .get_file("sess-legacy", "sf_missing00000001")
        .await
        .expect("get")
        .expect("present");
    assert_eq!(row.status, "missing");
}

#[tokio::test]
async fn v2_size_mismatch_reports_payload_truncated() {
    let home = fresh_home("truncated");
    let source_root = home.join("old/session-files");
    // payload 5 字节，声明 100 → size mismatch（fixture sfh-ef/big.bin 模式）。
    let dir = seed_dir(
        &source_root,
        "sfh-ef",
        &[(
            "sf_truncated000001",
            "big.bin",
            b"SHORT",
            Some(100),
            Some(sha256_hex(b"SHORT")),
        )],
    );
    let svc = service(home.clone()).await;
    let report = svc
        .import_legacy_session_files_dir(&dir, &home.join("staging"))
        .await
        .expect("import");
    assert_eq!(report.broken.len(), 1);
    assert_eq!(report.broken[0].reason, "payload-truncated");
    let row = svc
        .get_file("sess-legacy", "sf_truncated000001")
        .await
        .expect("get")
        .expect("present");
    assert_eq!(row.status, "broken");
}

#[tokio::test]
async fn v2_sha_mismatch_reports_payload_truncated() {
    let home = fresh_home("shabad");
    let source_root = home.join("old/session-files");
    let dir = seed_dir(
        &source_root,
        "sfh-sha",
        &[(
            "sf_shamismatch001",
            "data.bin",
            b"actual bytes",
            None,
            Some("0".repeat(64)),
        )],
    );
    let svc = service(home.clone()).await;
    let report = svc
        .import_legacy_session_files_dir(&dir, &home.join("staging"))
        .await
        .expect("import");
    assert_eq!(report.broken.len(), 1);
    assert_eq!(report.broken[0].reason, "payload-truncated");
}

#[tokio::test]
async fn v2_corrupt_descriptor_reports_sidecar_row_invalid() {
    let home = fresh_home("corrupt");
    let source_root = home.join("old/session-files");
    let dir = seed_dir(
        &source_root,
        "sfh-mixed",
        &[("sf_ok0000000000001", "ok.bin", b"ok", None, None)],
    );
    std::fs::write(dir.join("bad.bin.meta.json"), b"{not-json").expect("write corrupt meta");
    let svc = service(home.clone()).await;
    let report = svc
        .import_legacy_session_files_dir(&dir, &home.join("staging"))
        .await
        .expect("import");
    assert_eq!(report.imported, 1, "好行不受影响");
    assert_eq!(report.broken.len(), 1);
    assert_eq!(report.broken[0].reason, "sidecar-row-invalid");
}

#[tokio::test]
async fn v2_row_level_version_mismatch_reports_unsupported_version() {
    let home = fresh_home("rowver");
    let svc = service(home.clone()).await;
    let payload = home.join("p.bin");
    std::fs::write(&payload, b"x").expect("write");
    let sidecar = home.join("s.json");
    std::fs::write(
        &sidecar,
        serde_json::json!({
            "version": 1,
            "files": {
                "sf_rowver000000001": {
                    "version": 2,
                    "fileId": "sf_rowver000000001",
                    "realPath": payload.to_str().unwrap(),
                    "filename": "p.bin",
                    "mime": "application/octet-stream"
                }
            }
        })
        .to_string(),
    )
    .expect("write sidecar");
    let report = svc
        .import_legacy_sidecar_v2("sess-legacy", &sidecar)
        .await
        .expect("import");
    assert_eq!(report.imported, 0);
    assert_eq!(report.broken.len(), 1);
    assert_eq!(report.broken[0].reason, "unsupported-version");
}

#[tokio::test]
async fn v2_top_level_version_mismatch_is_hard_error() {
    let home = fresh_home("topver");
    let svc = service(home.clone()).await;
    let sidecar = home.join("s.json");
    std::fs::write(
        &sidecar,
        serde_json::json!({"version": 2, "files": {}}).to_string(),
    )
    .expect("write");
    let err = svc
        .import_legacy_sidecar_v2("sess-legacy", &sidecar)
        .await
        .expect_err("顶层版本不符必须硬拒绝");
    assert!(matches!(
        err,
        lingxi_service::sessionfiles::SessionFileError::UnsupportedSidecarVersion(2)
    ));
}

#[tokio::test]
async fn v2_rerun_is_idempotent_skipped_existing() {
    let home = fresh_home("rerun");
    let source_root = home.join("old/session-files");
    let dir = seed_dir(
        &source_root,
        "sfh-rerun",
        &[("sf_rerun0000000001", "a.bin", b"aaa", None, None)],
    );
    let svc = service(home.clone()).await;
    let staging = home.join("staging");
    let first = svc
        .import_legacy_session_files_dir(&dir, &staging)
        .await
        .expect("first");
    assert_eq!(first.imported, 1);
    let second = svc
        .import_legacy_session_files_dir(&dir, &staging)
        .await
        .expect("second");
    assert_eq!(second.imported, 0);
    assert_eq!(second.skipped_existing, 1, "重跑不重复登记");
    assert!(second.broken.is_empty());
}

#[test]
fn v1_report_shape_byte_identical() {
    // Lead 条件①：V1 响应形状钉死（imported/skipped，无新键）。
    let report = lingxi_service::sessionfiles::ImportReport {
        imported: 1,
        skipped: 2,
    };
    assert_eq!(
        serde_json::to_string(&report).expect("ser"),
        "{\"imported\":1,\"skipped\":2}"
    );
}

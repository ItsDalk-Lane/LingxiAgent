//! 独立复核：真实CSV解析子进程、processing事实/产物与七天保留联合运行。
use lingxi_adapters::storage::knowledge as ks;
use lingxi_service::knowledge::KnowledgeService;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
};
#[test]
fn actual_csv_processing_retains_until_seventh_day_then_removes_facts_outputs_not_user_file() {
    const DAY: i64 = 86_400_000;
    let home = std::env::temp_dir().join(ks::new_id("ex08-processing-independent"));
    std::fs::create_dir_all(&home).unwrap();
    let now = Arc::new(AtomicI64::new(1_790_000_000_000));
    let tick = now.clone();
    let s = KnowledgeService::new_with_clock(
        &home,
        "review-studio".into(),
        Arc::new(move || tick.load(Ordering::SeqCst)),
    )
    .unwrap();
    s.set_parser_binary(PathBuf::from(env!("CARGO_BIN_EXE_lingxi-service")))
        .unwrap();
    let notebook = s.create_notebook("CSV七天保留独立复核").unwrap();
    let book = notebook["id"].as_str().unwrap();
    let path = home.join("actual-source.csv");
    let raw = "标题,值\n真实处理,Unicode𠀀\n".as_bytes();
    std::fs::write(&path, raw).unwrap();
    let imported = s.import_file(book, path.to_str().unwrap(), None).unwrap();
    let source = imported["source"]["id"].as_str().unwrap();
    let snapshot_id = imported["snapshot"]["id"].as_str().unwrap();
    let artifact_id = imported["parseArtifact"]["id"].as_str().unwrap();
    let facts = ks::KnowledgeFactStore::open_with_clock(
        &home.join("knowledge"),
        Arc::new(|| 1_790_000_000_000),
    )
    .unwrap();
    let snapshot = facts.get_snapshot(snapshot_id).unwrap();
    let artifact = facts.get_parse_artifact(artifact_id).unwrap();
    assert_eq!(artifact.fidelity, "structural");
    let processing = artifact.processing_artifact_id.unwrap();
    let output = home
        .join("knowledge/artifacts")
        .join(snapshot_id)
        .join(format!("{processing}.txt"));
    let before = std::fs::read(&output).unwrap();
    assert!(String::from_utf8_lossy(&before).contains("Unicode𠀀"));
    let conn = rusqlite::Connection::open(home.join("knowledge/knowledge.db")).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM processing_artifacts WHERE id=?1",
            [&processing],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    let deleted = s.delete_notebook(book).unwrap();
    assert_eq!(deleted["sourceRetentionDays"], 7);
    now.fetch_add(7 * DAY - 1, Ordering::SeqCst);
    assert_eq!(
        s.run_lifecycle_maintenance().unwrap()["purgedSourceIds"],
        json!([])
    );
    assert_eq!(facts.read_snapshot_bytes(&snapshot).unwrap(), raw);
    assert_eq!(std::fs::read(&output).unwrap(), before);
    drop(facts);
    drop(s);
    drop(conn);
    let tick = now.clone();
    let s = KnowledgeService::new_with_clock(
        &home,
        "review-studio".into(),
        Arc::new(move || tick.load(Ordering::SeqCst)),
    )
    .unwrap();
    assert_eq!(std::fs::read(&output).unwrap(), before);
    now.fetch_add(1, Ordering::SeqCst);
    let report = s.run_lifecycle_maintenance().unwrap();
    assert_eq!(report["purgedSourceIds"], json!([source]));
    let conn = rusqlite::Connection::open(home.join("knowledge/knowledge.db")).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM processing_artifacts WHERE id=?1",
            [&processing],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM content_snapshots WHERE id=?1",
            [snapshot_id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert!(!output.exists());
    assert!(!home.join("knowledge/artifacts").join(snapshot_id).exists());
    assert!(!home.join("knowledge").join(&snapshot.storage_path).exists());
    assert_eq!(std::fs::read(&path).unwrap(), raw);
    let receipt = json!({"home":home,"imported":imported,"deleted":deleted,"processingId":processing,"processingOutputBefore":String::from_utf8(before).unwrap(),"day7Gc":report,"sourceFileStillExact":true,"processingFactsAndOutputsRemoved":true,"restartBeforeExpiry":true});
    std::fs::write(
        home.join("ex08-processing-proof.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    println!(
        "EX08_PROCESSING_PROOF {}",
        home.join("ex08-processing-proof.json").display()
    );
}

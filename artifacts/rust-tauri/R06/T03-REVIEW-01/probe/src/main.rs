//! R06-T03 REVIEW-01 活体探针（审查产物，非产品代码）：
//! 复现「latest 覆盖 + 文件版本」组合下 checkpoint_file_versions 的处理。
//! 场景 A：latest 覆盖同名检查点且携带相同文件 → 预期观察 INSERT PK 冲突。
//! 场景 B：latest 覆盖携带不同文件 → 预期观察旧文件版本残留（rewind 核验面污染）。

use lingxi_adapters::storage::session_tree as store;
use lingxi_adapters::storage::{RunDatabase, StoreOptions};

#[tokio::main]
async fn main() {
    let dir = std::env::temp_dir().join(format!(
        "r06t03-review-probe-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("runs.db");
    let _ = std::fs::remove_file(&db_path);

    let db = RunDatabase::open(&db_path, StoreOptions::default())
        .await
        .expect("open db");

    // 种一个会话（v8 列走默认值）。
    store::seed_session(
        &db,
        store::SeedSession {
            session_id: "s",
            agent_id: "agent",
            owner_user_id: "user_local",
            title: "t",
            now_unix_ms: 1,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .expect("seed");

    // ── 场景 A：latest 覆盖 + 相同文件 ──
    let cp = store::upsert_checkpoint(
        &db,
        store::UpsertCheckpoint {
            session_id: "s",
            name: "latest",
            target_message_id: "m1",
            turn_input_message_id: None,
            message_count: 1,
            now_unix_ms: 100,
        },
    )
    .await
    .expect("first latest");
    store::record_file_version(
        &db,
        store::RecordFileVersion {
            checkpoint_id: &cp.checkpoint_id,
            session_id: "s",
            file_path: "/tmp/f1",
            sha256: "aaaa",
            size_bytes: 1,
            now_unix_ms: 100,
        },
    )
    .await
    .expect("first file version");

    // 第二次 latest（现役语义：覆盖）。
    let cp2 = store::upsert_checkpoint(
        &db,
        store::UpsertCheckpoint {
            session_id: "s",
            name: "latest",
            target_message_id: "m2",
            turn_input_message_id: None,
            message_count: 2,
            now_unix_ms: 200,
        },
    )
    .await
    .expect("latest overwrite row");
    println!("A: overwrite ok, checkpoint_id={}（同 id? {}）", cp2.checkpoint_id, cp2.checkpoint_id == cp.checkpoint_id);

    // 现在记录同一文件的版本——production 路径 create_checkpoint 在
    // upsert 成功后逐文件 record_file_version。
    let res = store::record_file_version(
        &db,
        store::RecordFileVersion {
            checkpoint_id: &cp2.checkpoint_id,
            session_id: "s",
            file_path: "/tmp/f1",
            sha256: "bbbb",
            size_bytes: 2,
            now_unix_ms: 200,
        },
    )
    .await;
    match res {
        Ok(()) => println!("A: record_file_version on overwritten latest = OK（意外）"),
        Err(e) => println!("A: record_file_version on overwritten latest = ERR: {e:?}"),
    }

    // ── 场景 B：latest 覆盖带不同文件 → 旧文件版本残留？ ──
    let cp3 = store::upsert_checkpoint(
        &db,
        store::UpsertCheckpoint {
            session_id: "s",
            name: "latest",
            target_message_id: "m3",
            turn_input_message_id: None,
            message_count: 3,
            now_unix_ms: 300,
        },
    )
    .await
    .expect("latest overwrite 2");
    let res3 = store::record_file_version(
        &db,
        store::RecordFileVersion {
            checkpoint_id: &cp3.checkpoint_id,
            session_id: "s",
            file_path: "/tmp/f2",
            sha256: "cccc",
            size_bytes: 3,
            now_unix_ms: 300,
        },
    )
    .await;
    println!("B: record new file f2 = {:?}", res3.is_ok());
    let versions = store::list_file_versions(&db, &cp3.checkpoint_id)
        .await
        .expect("list versions");
    let paths: Vec<&str> = versions.iter().map(|v| v.file_path.as_str()).collect();
    println!("B: file versions after overwrite with f2 only: {paths:?}");
    println!(
        "B: 残留旧文件 f1? {}",
        paths.contains(&"/tmp/f1")
    );

    db.close().await.expect("close");
    let _ = std::fs::remove_dir_all(&dir);
}

//! R06-T03 REPAIR-R1 修复前复现探针（审查修复证据，非产品代码）。
//! 逐项复现 REVIEW-01 的存储层 finding 修复前状态：
//!   P1 FINDING-03：归档会话仍出现在 list_sessions 主列表
//!   P2 FINDING-06：messages.run_id 无外键——指向不存在 run 的消息插入成功
//!   P3 FINDING-08：archive_children 对已归档后代计数虚增
//!   P4 观察项：fork new_session_id 撞库映射为 Internal（生产链即 500）
//!   P5 FINDING-01 同根因扩展：检查点裁减（201 窗口）删带文件版本的检查点 → FK 失败
//!   P6 FINDING-05/07：file_checkpoints 零写入——带文件创建检查点后内容表仍为空（无恢复能力）
//! FINDING-01 本体由 R1 probe1 复跑覆盖（见 09_repair_r1_prefix_repro.txt 第一节）；
//! FINDING-02/04/05 服务层行为由 RED 测试复现（09_repair_r1_red_tests.txt）。

use lingxi_adapters::storage::session_admin as admin;
use lingxi_adapters::storage::session_tree as store;
use lingxi_adapters::storage::{RunDatabase, StoreOptions};

fn seed<'a>(session_id: &'a str, now: u64) -> store::SeedSession<'a> {
    store::SeedSession {
        session_id,
        agent_id: "agent",
        owner_user_id: "user_local",
        title: "t",
        now_unix_ms: now,
        permission_mode: None,
        authorized_folders: vec![],
    }
}

#[tokio::main]
async fn main() {
    let dir = std::env::temp_dir().join(format!("r06t03-repair-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("runs.db");
    let _ = std::fs::remove_file(&db_path);

    let db = RunDatabase::open(&db_path, StoreOptions::default())
        .await
        .expect("open db");

    // ── P1 FINDING-03：归档后主列表仍含该会话 ──
    store::seed_session(&db, seed("s-arch", 1)).await.expect("seed s-arch");
    admin::archive_session(&db, "s-arch", None, &[], 2)
        .await
        .expect("archive");
    let listed = db.list_sessions().await.expect("list_sessions");
    let ids: Vec<&str> = listed.iter().map(|s| s.session_id.as_str()).collect();
    println!(
        "P1 FINDING-03: archived session still in main list? {}",
        ids.contains(&"s-arch")
    );

    // ── P2 FINDING-06：run_id 无外键，幽灵 run 归属消息可插入 ──
    store::seed_session(&db, seed("s-msg", 10)).await.expect("seed s-msg");
    let ghost = store::append_message(
        &db,
        store::AppendMessage {
            session_id: "s-msg",
            message_id: "m-ghost",
            parent_message_id: None,
            run_id: "run-does-not-exist",
            role: "user",
            content_json: "{\"text\":\"x\"}",
            entry_type: "message",
            now_unix_ms: 11,
        },
    )
    .await;
    match &ghost {
        Ok(()) => println!("P2 FINDING-06: ghost-run message insert = OK（无外键拦截）"),
        Err(e) => println!("P2 FINDING-06: ghost-run message insert = ERR: {e:?}"),
    }

    // ── P3 FINDING-08：混合子树归档计数 ──
    // p -> c1(active), p -> c2(已归档)。archive_children 应只计真实翻转的 c1=1。
    store::seed_session(&db, seed("p", 20)).await.expect("seed p");
    store::seed_session(&db, seed("c1", 21)).await.expect("seed c1");
    store::seed_session(&db, seed("c2", 22)).await.expect("seed c2");
    // 建父子关系（fork 形状等价：直接复制谱系列的最轻路径是 fork，
    // 但 fork 需要分支消息；这里用带 parent 的 direct UPDATE 不可行
    // （queue 为 crate 内可见），改走 fork 建真子会话）。
    store::append_message(
        &db,
        store::AppendMessage {
            session_id: "p",
            message_id: "p-m1",
            parent_message_id: None,
            run_id: "r1",
            role: "user",
            content_json: "{\"text\":\"hi\"}",
            entry_type: "message",
            now_unix_ms: 23,
        },
    )
    .await
    .expect("append p-m1");
    // 真子会话：c1f / c2f 均为 p 的 fork 子代。
    store::fork_session(
        &db,
        store::ForkRequest {
            source_session_id: "p",
            new_session_id: "c1f",
            boundary_message_id: "p-m1",
            now_unix_ms: 24,
        },
    )
    .await
    .expect("fork c1f");
    store::fork_session(
        &db,
        store::ForkRequest {
            source_session_id: "p",
            new_session_id: "c2f",
            boundary_message_id: "p-m1",
            now_unix_ms: 25,
        },
    )
    .await
    .expect("fork c2f");
    // 先把 c2f 归档（无子）。
    admin::archive_session(&db, "c2f", None, &[], 26)
        .await
        .expect("archive c2f");
    let outcome = admin::archive_session(
        &db,
        "p",
        Some(admin::ChildMode::ArchiveChildren),
        &[],
        27,
    )
    .await
    .expect("archive p");
    println!(
        "P3 FINDING-08: archived_children={}（真实翻转=1：仅 c1f；c2f 已归档）",
        outcome.archived_children
    );

    // ── P4 观察项：fork 撞库 ──
    let dup = store::fork_session(
        &db,
        store::ForkRequest {
            source_session_id: "p",
            new_session_id: "c1f",
            boundary_message_id: "p-m1",
            now_unix_ms: 28,
        },
    )
    .await;
    match &dup {
        Ok(_) => println!("P4 OBS: fork duplicate id = OK（意外）"),
        Err(e) => println!("P4 OBS: fork duplicate id = ERR: {e:?}"),
    }

    // ── P5 FINDING-01 同根因扩展：201 裁减 × 文件版本 → FK ──
    store::seed_session(&db, seed("s-evict", 100)).await.expect("seed s-evict");
    let mut evict_err: Option<String> = None;
    for i in 0..=200u32 {
        let name = format!("c{i}");
        let cp = store::upsert_checkpoint(
            &db,
            store::UpsertCheckpoint {
                session_id: "s-evict",
                name: &name,
                target_message_id: "m",
                turn_input_message_id: None,
                message_count: 1,
                now_unix_ms: 1000 + u64::from(i),
            },
        )
        .await;
        let cp = match cp {
            Ok(cp) => cp,
            Err(e) => {
                evict_err = Some(format!("upsert #{i} ({name}) failed: {e:?}"));
                break;
            }
        };
        if let Err(e) = store::record_file_version(
            &db,
            store::RecordFileVersion {
                checkpoint_id: &cp.checkpoint_id,
                session_id: "s-evict",
                file_path: "/tmp/evict-f",
                sha256: "aa",
                size_bytes: 1,
                now_unix_ms: 1000 + u64::from(i),
            },
        )
        .await
        {
            evict_err = Some(format!("record #{i} failed: {e:?}"));
            break;
        }
    }
    match &evict_err {
        None => println!("P5 F01-EXT: 201 checkpoints with file versions = OK（意外）"),
        Some(e) => println!("P5 F01-EXT: eviction with file versions = ERR: {e}"),
    }

    // ── P6 FINDING-05/07：file_checkpoints 零读写 ──
    // 带文件版本创建检查点后，内容表无任何写入路径（全库 grep 仅删除会话
    // 清理触及）。这里以「无公开读 API + 写入路径缺席」佐证：创建检查点
    // 后唯一可读的只有哈希版本行。
    let cps = store::list_checkpoints(&db, "s-evict").await.expect("list cps");
    let first = &cps[0];
    let versions = store::list_file_versions(&db, &first.checkpoint_id)
        .await
        .expect("versions");
    println!(
        "P6 F05/07: checkpoint {} 的文件版本行数={}（仅存 sha256+size；\
         file_checkpoints 内容表无写路径——rewind 无内容可恢复）",
        first.checkpoint_id,
        versions.len()
    );

    db.close().await.expect("close");
    let _ = std::fs::remove_dir_all(&dir);
    println!("REPAIR_PROBE_DONE");
}

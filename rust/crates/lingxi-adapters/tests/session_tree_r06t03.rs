//! R06-T03 现役对照式测试：会话树 / 分支 / fork / 重试 / 回退 / 检查点。
//!
//! 对照纪律（RC-3）：关键断言值可追溯到现役锚点——
//! - fork 保留消息 ID：`node_modules/@earendil-works/pi-coding-agent/dist/core/
//!   session-manager.js:1113` `createBranchedSession`（复制保留 entry id，仅 parentId 重链）。
//! - 谱系深度上限 2：`server/routes/sessions.ts:1738` `MAX_FORK_LINEAGE_DEPTH = 2`。
//! - 运行中拒绝 409：`core/session-turn-actions.ts:382-384`（isSessionStreaming →
//!   session_busy）、`server/routes/sessions.ts:1790-1792`。
//! - 分支头回移 + `hana-session-branch-reset` 标记、旧历史不删：
//!   `core/session-turn-actions.ts:454-571`（commitRetryBranch 事务步序）。
//! - 具名检查点 latest 覆盖/其余冲突拒绝/上限 200：`core/session-checkpoints.ts:14-16,
//!   74-101`。
//! - 文件恢复冲突检测为对现役缺口的加固（差异 D1）：现役盲写回
//!   `core/workspace-snapshots.ts:584-620`、`lib/checkpoint-store.ts:94-108`。

use lingxi_adapters::storage::{RunDatabase, StoreOptions};

/// 独立临时数据库路径（独立临时目录语义；LINGXI_HOME 不生效——测试显式传路径）。
fn temp_db(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "lingxi-r06t03-tree-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

async fn open_db(path: &std::path::Path) -> RunDatabase {
    RunDatabase::open(path, StoreOptions::default())
        .await
        .expect("open (applies all migrations)")
}

// ---------------------------------------------------------------- A05：fork

/// A05 主场景：会话 A 中段 fork 为 B；共同历史消息 ID 稳定、各自追加互不可见、
/// 权限快照继承且不扩大。
#[tokio::test]
async fn fork_copies_shared_history_with_stable_ids_and_independent_writes() {
    const TAG: &str = "fork-share";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;

    // 种一个会话 A 并追加 3 条消息（root → m1 → m2 → m3）。
    lingxi_adapters::storage::session_tree::seed_session(
        &db,
        lingxi_adapters::storage::session_tree::SeedSession {
            session_id: "sess_a",
            agent_id: "agent",
            owner_user_id: "local-owner",
            title: "A",
            now_unix_ms: now,
            permission_mode: Some("auto"),
            authorized_folders: vec!["/work".into()],
        },
    )
    .await
    .unwrap();
    for (id, parent) in [("m1", None), ("m2", Some("m1")), ("m3", Some("m2"))] {
        lingxi_adapters::storage::session_tree::append_message(
            &db,
            lingxi_adapters::storage::session_tree::AppendMessage {
                session_id: "sess_a",
                message_id: id,
                parent_message_id: parent,
                run_id: None,
                role: "user",
                content_json: "{}",
                entry_type: "message",
                model_call_id: None,
                now_unix_ms: now,
            },
        )
        .await
        .unwrap();
    }

    // 在 m2 处 fork 出 B。
    let forked = lingxi_adapters::storage::session_tree::fork_session(
        &db,
        lingxi_adapters::storage::session_tree::ForkRequest {
            source_session_id: "sess_a",
            new_session_id: "sess_b",
            boundary_message_id: "m2",
            now_unix_ms: now + 1,
        },
    )
    .await
    .unwrap();

    // 共同历史（m1, m2）在 B 中以**相同消息 ID**可读（现役 createBranchedSession 语义）。
    let b_msgs = lingxi_adapters::storage::session_tree::list_branch_messages(&db, "sess_b")
        .await
        .unwrap();
    let b_ids: Vec<&str> = b_msgs.iter().map(|m| m.message_id.as_str()).collect();
    assert_eq!(
        b_ids,
        vec!["m1", "m2"],
        "fork 复制 root→boundary 且保留消息 ID"
    );
    assert_eq!(forked.parent_session_id, "sess_a");
    assert_eq!(forked.fork_point_message_id, "m2");
    assert_eq!(forked.lineage_depth, 1, "主对话=0，第一层 fork=1");

    // B 的权限快照继承 A（不扩大）。
    assert_eq!(forked.permission_mode.as_deref(), Some("auto"));
    assert_eq!(forked.authorized_folders, vec!["/work".to_string()]);

    // A 在 fork 后追加 m4；B 追加 n1。互不可见。
    lingxi_adapters::storage::session_tree::append_message(
        &db,
        lingxi_adapters::storage::session_tree::AppendMessage {
            session_id: "sess_a",
            message_id: "m4",
            parent_message_id: Some("m3"),
            run_id: None,
            role: "assistant",
            content_json: "{}",
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: now + 2,
        },
    )
    .await
    .unwrap();
    lingxi_adapters::storage::session_tree::append_message(
        &db,
        lingxi_adapters::storage::session_tree::AppendMessage {
            session_id: "sess_b",
            message_id: "n1",
            parent_message_id: Some("m2"),
            run_id: None,
            role: "user",
            content_json: "{}",
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: now + 2,
        },
    )
    .await
    .unwrap();

    let a_ids: Vec<String> =
        lingxi_adapters::storage::session_tree::list_branch_messages(&db, "sess_a")
            .await
            .unwrap()
            .iter()
            .map(|m| m.message_id.clone())
            .collect();
    let b_ids2: Vec<String> =
        lingxi_adapters::storage::session_tree::list_branch_messages(&db, "sess_b")
            .await
            .unwrap()
            .iter()
            .map(|m| m.message_id.clone())
            .collect();
    assert_eq!(a_ids, vec!["m1", "m2", "m3", "m4"], "A 的新消息只在 A");
    assert_eq!(
        b_ids2,
        vec!["m1", "m2", "n1"],
        "B 不含 A 的 m3/m4，共同历史仍在"
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

/// 深度闸：谱系第 3 层拒绝（现役 MAX_FORK_LINEAGE_DEPTH=2，sessions.ts:1738/1795-1801）。
#[tokio::test]
async fn fork_rejects_depth_beyond_two() {
    const TAG: &str = "fork-depth";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    lingxi_adapters::storage::session_tree::seed_session(
        &db,
        lingxi_adapters::storage::session_tree::SeedSession {
            session_id: "root",
            agent_id: "a",
            owner_user_id: "local-owner",
            title: "r",
            now_unix_ms: now,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();
    lingxi_adapters::storage::session_tree::append_message(
        &db,
        lingxi_adapters::storage::session_tree::AppendMessage {
            session_id: "root",
            message_id: "m1",
            parent_message_id: None,
            run_id: None,
            role: "user",
            content_json: "{}",
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: now,
        },
    )
    .await
    .unwrap();

    let l1 = lingxi_adapters::storage::session_tree::fork_session(
        &db,
        lingxi_adapters::storage::session_tree::ForkRequest {
            source_session_id: "root",
            new_session_id: "child",
            boundary_message_id: "m1",
            now_unix_ms: now + 1,
        },
    )
    .await
    .unwrap();
    assert_eq!(l1.lineage_depth, 1);

    let l2 = lingxi_adapters::storage::session_tree::fork_session(
        &db,
        lingxi_adapters::storage::session_tree::ForkRequest {
            source_session_id: "child",
            new_session_id: "grand",
            boundary_message_id: "m1",
            now_unix_ms: now + 2,
        },
    )
    .await
    .unwrap();
    assert_eq!(l2.lineage_depth, 2, "孙=第 2 层，是上限");

    // 第 3 层：拒绝（现役 409 session_fork_depth_limit）。
    let err = lingxi_adapters::storage::session_tree::fork_session(
        &db,
        lingxi_adapters::storage::session_tree::ForkRequest {
            source_session_id: "grand",
            new_session_id: "great",
            boundary_message_id: "m1",
            now_unix_ms: now + 3,
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(
            &err,
            lingxi_kernel::ports::StorageError::InvalidRequest { detail }
                if detail.contains("fork_depth_limit")
        ),
        "第 3 层必须拒绝（fork_depth_limit）：{err:?}"
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

/// fork 不扩大授权：子会话授权目录恰为源快照，不多一项（安全红线）。
#[tokio::test]
async fn fork_does_not_widen_authorizations() {
    const TAG: &str = "fork-auth";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    lingxi_adapters::storage::session_tree::seed_session(
        &db,
        lingxi_adapters::storage::session_tree::SeedSession {
            session_id: "s",
            agent_id: "a",
            owner_user_id: "local-owner",
            title: "s",
            now_unix_ms: now,
            permission_mode: Some("strict"),
            authorized_folders: vec!["/x".into(), "/y".into()],
        },
    )
    .await
    .unwrap();
    lingxi_adapters::storage::session_tree::append_message(
        &db,
        lingxi_adapters::storage::session_tree::AppendMessage {
            session_id: "s",
            message_id: "m1",
            parent_message_id: None,
            run_id: None,
            role: "user",
            content_json: "{}",
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: now,
        },
    )
    .await
    .unwrap();

    let child = lingxi_adapters::storage::session_tree::fork_session(
        &db,
        lingxi_adapters::storage::session_tree::ForkRequest {
            source_session_id: "s",
            new_session_id: "c",
            boundary_message_id: "m1",
            now_unix_ms: now + 1,
        },
    )
    .await
    .unwrap();
    // 恰为源集合：逐项相等（不是超集，不是并集）。
    let mut got = child.authorized_folders.clone();
    got.sort();
    assert_eq!(got, vec!["/x".to_string(), "/y".to_string()]);
    assert_eq!(child.permission_mode.as_deref(), Some("strict"));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

// ------------------------------------------------------------- 检查点

/// 具名检查点：latest 覆盖、其余重名冲突拒绝、上限 200（session-checkpoints.ts:14-16,74-101）。
#[tokio::test]
async fn named_checkpoint_latest_overwrites_others_conflict() {
    const TAG: &str = "ckpt";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    lingxi_adapters::storage::session_tree::seed_session(
        &db,
        lingxi_adapters::storage::session_tree::SeedSession {
            session_id: "s",
            agent_id: "a",
            owner_user_id: "local-owner",
            title: "s",
            now_unix_ms: now,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();

    // latest 两次：覆盖。
    lingxi_adapters::storage::session_tree::upsert_checkpoint(
        &db,
        lingxi_adapters::storage::session_tree::UpsertCheckpoint {
            session_id: "s",
            name: "latest",
            target_message_id: "m1",
            turn_input_message_id: None,
            files: vec![],
            message_count: 1,
            now_unix_ms: now,
        },
    )
    .await
    .unwrap();
    lingxi_adapters::storage::session_tree::upsert_checkpoint(
        &db,
        lingxi_adapters::storage::session_tree::UpsertCheckpoint {
            session_id: "s",
            name: "latest",
            target_message_id: "m2",
            turn_input_message_id: None,
            files: vec![],
            message_count: 2,
            now_unix_ms: now + 1,
        },
    )
    .await
    .unwrap();
    let latest = lingxi_adapters::storage::session_tree::get_checkpoint(&db, "s", "latest")
        .await
        .unwrap()
        .expect("latest");
    assert_eq!(latest.target_message_id, "m2", "latest 重复创建=覆盖");

    // 非 latest 重名：冲突拒绝。
    lingxi_adapters::storage::session_tree::upsert_checkpoint(
        &db,
        lingxi_adapters::storage::session_tree::UpsertCheckpoint {
            session_id: "s",
            name: "v1",
            target_message_id: "m1",
            turn_input_message_id: None,
            files: vec![],
            message_count: 1,
            now_unix_ms: now,
        },
    )
    .await
    .unwrap();
    let err = lingxi_adapters::storage::session_tree::upsert_checkpoint(
        &db,
        lingxi_adapters::storage::session_tree::UpsertCheckpoint {
            session_id: "s",
            name: "v1",
            target_message_id: "m9",
            turn_input_message_id: None,
            files: vec![],
            message_count: 9,
            now_unix_ms: now + 9,
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, lingxi_kernel::ports::StorageError::Conflict { .. }),
        "非 latest 重名必须冲突拒绝：{err:?}"
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

// ------------------------------------------------------------- A06：rewind

/// A06 主场景：checkpoint 后用户手动改文件 → rewind 检测冲突、保留用户修改、
/// 如实报告（不伪称全部撤销）。对照现役缺口（差异 D1）的加固。
#[tokio::test]
async fn rewind_detects_external_modification_and_refuses_to_overwrite() {
    const TAG: &str = "rewind";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    lingxi_adapters::storage::session_tree::seed_session(
        &db,
        lingxi_adapters::storage::session_tree::SeedSession {
            session_id: "s",
            agent_id: "a",
            owner_user_id: "local-owner",
            title: "s",
            now_unix_ms: now,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();

    // 先建检查点行（checkpoint_file_versions 的 FK 目标）。
    let cp = lingxi_adapters::storage::session_tree::upsert_checkpoint(
        &db,
        lingxi_adapters::storage::session_tree::UpsertCheckpoint {
            session_id: "s",
            name: "cp1",
            target_message_id: "m1",
            turn_input_message_id: None,
            files: vec![],
            message_count: 1,
            now_unix_ms: now,
        },
    )
    .await
    .unwrap();

    // 记录检查点时文件的版本（sha256）。
    let v_checkpoint = lingxi_kernel::session_tree::sha256_hex(b"agent-version");
    lingxi_adapters::storage::session_tree::record_file_version(
        &db,
        lingxi_adapters::storage::session_tree::RecordFileVersion {
            checkpoint_id: &cp.checkpoint_id,
            session_id: "s",
            file_path: "src/a.rs",
            sha256: &v_checkpoint,
            size_bytes: 13,
            now_unix_ms: now,
        },
    )
    .await
    .unwrap();

    // 冲突判定（纯逻辑，REPAIR-R1 三档）：当前文件被外部改过（hash 不在见证集）→ conflicted。
    let v_current = lingxi_kernel::session_tree::sha256_hex(b"user-edit");
    let verdict = lingxi_kernel::session_tree::judge_file_restore(
        "src/a.rs",
        &v_checkpoint,
        &v_current,
        std::slice::from_ref(&v_checkpoint),
    );
    assert_eq!(
        verdict,
        lingxi_kernel::session_tree::FileRestoreVerdict::Conflicted,
        "外部修改必须检测为冲突"
    );

    // 当前 == 检查点目标 → skipped（无需写回）。
    let verdict_same = lingxi_kernel::session_tree::judge_file_restore(
        "src/a.rs",
        &v_checkpoint,
        &v_checkpoint,
        std::slice::from_ref(&v_checkpoint),
    );
    assert_eq!(
        verdict_same,
        lingxi_kernel::session_tree::FileRestoreVerdict::Skipped
    );
    // 当前 != 目标但被系统见证过（模型后续回合改过并被检查点记录）→ restore。
    let v_model = lingxi_kernel::session_tree::sha256_hex(b"model-turn-edit");
    let verdict_restore = lingxi_kernel::session_tree::judge_file_restore(
        "src/a.rs",
        &v_checkpoint,
        &v_model,
        &[v_checkpoint.clone(), v_model.clone()],
    );
    assert_eq!(
        verdict_restore,
        lingxi_kernel::session_tree::FileRestoreVerdict::Restore,
        "被系统见证过的中间版本可安全写回存档"
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

/// 分支重置：retry/rewind 把分支头回移并追加 reset 标记，旧历史不删
/// （session-turn-actions.ts:454-571）。
#[tokio::test]
async fn branch_reset_moves_head_and_appends_marker_without_deleting_history() {
    const TAG: &str = "reset";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    lingxi_adapters::storage::session_tree::seed_session(
        &db,
        lingxi_adapters::storage::session_tree::SeedSession {
            session_id: "s",
            agent_id: "a",
            owner_user_id: "local-owner",
            title: "s",
            now_unix_ms: now,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();
    for (id, parent) in [("m1", None), ("m2", Some("m1")), ("m3", Some("m2"))] {
        lingxi_adapters::storage::session_tree::append_message(
            &db,
            lingxi_adapters::storage::session_tree::AppendMessage {
                session_id: "s",
                message_id: id,
                parent_message_id: parent,
                run_id: None,
                role: "user",
                content_json: "{}",
                entry_type: "message",
                model_call_id: None,
                now_unix_ms: now,
            },
        )
        .await
        .unwrap();
    }

    // 回移到头 m1 并追加 reset 标记（reason=retry）；标记 id 由存储层事务内铸造并返回。
    let marker_id = lingxi_adapters::storage::session_tree::reset_branch_head(
        &db,
        lingxi_adapters::storage::session_tree::ResetBranchHead {
            session_id: "s",
            new_head_message_id: Some("m1"),
            reason: "retry",
            source_message_id: Some("m3"),
            now_unix_ms: now + 1,
        },
    )
    .await
    .unwrap();
    assert!(
        marker_id.starts_with("reset:s:"),
        "标记 id 由存储层事务内铸造（reset:{{session}}:{{now}}:{{seq}}）：{marker_id}"
    );

    // 分支头 = reset 标记；重置后投影 = root→m1 + reset 标记。
    let proj = lingxi_adapters::storage::session_tree::list_branch_messages(&db, "s")
        .await
        .unwrap();
    let ids: Vec<&str> = proj.iter().map(|m| m.message_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["m1", marker_id.as_str()],
        "回移后分支=保留段+reset 标记"
    );
    assert_eq!(proj[1].entry_type, "hana-session-branch-reset");
    // reset 标记内容对照现役字段：reason/to/sourceEntryId。
    let marker: serde_json::Value = serde_json::from_str(&proj[1].content_json).unwrap();
    assert_eq!(marker["reason"], "retry");
    assert_eq!(marker["to"], "m1");
    assert_eq!(marker["sourceEntryId"], "m3");

    // 旧历史（m2, m3）仍在存储中可读（append-only，不删）。
    let all = lingxi_adapters::storage::session_tree::list_all_messages(&db, "s")
        .await
        .unwrap();
    let all_ids: Vec<&str> = all.iter().map(|m| m.message_id.as_str()).collect();
    assert!(
        all_ids.contains(&"m2") && all_ids.contains(&"m3"),
        "旧历史永不删除：{all_ids:?}"
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

/// 根回合重置：new_head=None → 分支投影只剩 reset 标记
/// （现役 retryBranchParentId = null）。
#[tokio::test]
async fn branch_reset_to_null_head_leaves_only_the_marker() {
    const TAG: &str = "reset-root";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    lingxi_adapters::storage::session_tree::seed_session(
        &db,
        lingxi_adapters::storage::session_tree::SeedSession {
            session_id: "s",
            agent_id: "a",
            owner_user_id: "local-owner",
            title: "s",
            now_unix_ms: now,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();
    lingxi_adapters::storage::session_tree::append_message(
        &db,
        lingxi_adapters::storage::session_tree::AppendMessage {
            session_id: "s",
            message_id: "m1",
            parent_message_id: None,
            run_id: None,
            role: "user",
            content_json: "{}",
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: now,
        },
    )
    .await
    .unwrap();
    let marker_id = lingxi_adapters::storage::session_tree::reset_branch_head(
        &db,
        lingxi_adapters::storage::session_tree::ResetBranchHead {
            session_id: "s",
            new_head_message_id: None,
            reason: "retry",
            source_message_id: Some("m1"),
            now_unix_ms: now + 1,
        },
    )
    .await
    .unwrap();
    let proj = lingxi_adapters::storage::session_tree::list_branch_messages(&db, "s")
        .await
        .unwrap();
    let ids: Vec<&str> = proj.iter().map(|m| m.message_id.as_str()).collect();
    assert_eq!(ids, vec![marker_id.as_str()], "根重置后分支只剩标记");
    let marker: serde_json::Value = serde_json::from_str(&proj[0].content_json).unwrap();
    assert!(
        marker["to"].is_null(),
        "to=null 对照现役 retryBranchParentId=null"
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

/// 检查点窗口：第 201 条触发裁掉最老非 latest（session-checkpoints.ts:96-101）。
#[tokio::test]
async fn checkpoint_window_evicts_oldest_non_latest_at_201() {
    const TAG: &str = "ckpt-cap";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    lingxi_adapters::storage::session_tree::seed_session(
        &db,
        lingxi_adapters::storage::session_tree::SeedSession {
            session_id: "s",
            agent_id: "a",
            owner_user_id: "local-owner",
            title: "s",
            now_unix_ms: now,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();
    // latest + 200 个具名 = 201 条；latest 永不裁。
    lingxi_adapters::storage::session_tree::upsert_checkpoint(
        &db,
        lingxi_adapters::storage::session_tree::UpsertCheckpoint {
            session_id: "s",
            name: "latest",
            target_message_id: "m0",
            turn_input_message_id: None,
            files: vec![],
            message_count: 0,
            now_unix_ms: now,
        },
    )
    .await
    .unwrap();
    for i in 0..200u32 {
        lingxi_adapters::storage::session_tree::upsert_checkpoint(
            &db,
            lingxi_adapters::storage::session_tree::UpsertCheckpoint {
                session_id: "s",
                name: &format!("cp{i:03}"),
                target_message_id: "m0",
                turn_input_message_id: None,
                files: vec![],
                message_count: 0,
                now_unix_ms: now + 1 + i as u64,
            },
        )
        .await
        .unwrap();
    }
    let rows = lingxi_adapters::storage::session_tree::list_checkpoints(&db, "s")
        .await
        .unwrap();
    assert_eq!(rows.len(), 200, "上限 200（session-checkpoints.ts:16）");
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"latest"), "latest 永不裁");
    assert!(!names.contains(&"cp000"), "最老的非 latest 被裁");
    assert!(names.contains(&"cp199"), "最新的保留");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

/// 检查点删除：FK 序（先文件版本后检查点行），不存在 → false。
#[tokio::test]
async fn delete_checkpoint_removes_file_versions_first() {
    const TAG: &str = "ckpt-del";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    lingxi_adapters::storage::session_tree::seed_session(
        &db,
        lingxi_adapters::storage::session_tree::SeedSession {
            session_id: "s",
            agent_id: "a",
            owner_user_id: "local-owner",
            title: "s",
            now_unix_ms: now,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();
    let cp = lingxi_adapters::storage::session_tree::upsert_checkpoint(
        &db,
        lingxi_adapters::storage::session_tree::UpsertCheckpoint {
            session_id: "s",
            name: "v1",
            target_message_id: "m1",
            turn_input_message_id: None,
            files: vec![],
            message_count: 1,
            now_unix_ms: now,
        },
    )
    .await
    .unwrap();
    lingxi_adapters::storage::session_tree::record_file_version(
        &db,
        lingxi_adapters::storage::session_tree::RecordFileVersion {
            checkpoint_id: &cp.checkpoint_id,
            session_id: "s",
            file_path: "a.rs",
            sha256: &"0".repeat(64),
            size_bytes: 1,
            now_unix_ms: now,
        },
    )
    .await
    .unwrap();
    assert!(
        lingxi_adapters::storage::session_tree::delete_checkpoint(&db, "s", "v1")
            .await
            .unwrap(),
        "存在 → true"
    );
    assert!(
        lingxi_adapters::storage::session_tree::list_file_versions(&db, &cp.checkpoint_id)
            .await
            .unwrap()
            .is_empty(),
        "文件版本随行删除"
    );
    assert!(
        !lingxi_adapters::storage::session_tree::delete_checkpoint(&db, "s", "v1")
            .await
            .unwrap(),
        "不存在 → false（路由映射 404，不静默成功）"
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

// ------------------------------------------------------------- 管理面存储

/// 改名/置顶/置顶重排/记忆开关：真实落库、不存在响亮 false/None。
#[tokio::test]
async fn admin_rename_pin_order_and_memory_roundtrip() {
    const TAG: &str = "admin-basic";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    for id in ["a", "b"] {
        lingxi_adapters::storage::session_tree::seed_session(
            &db,
            lingxi_adapters::storage::session_tree::SeedSession {
                session_id: id,
                agent_id: "agent",
                owner_user_id: "local-owner",
                title: id,
                now_unix_ms: now,
                permission_mode: None,
                authorized_folders: vec![],
            },
        )
        .await
        .unwrap();
    }
    use lingxi_adapters::storage::session_admin as admin;

    assert!(admin::rename_session(&db, "a", "new-title").await.unwrap());
    assert!(
        !admin::rename_session(&db, "ghost", "x").await.unwrap(),
        "不存在 → false"
    );
    let row = lingxi_adapters::storage::session_tree::get_session_row(&db, "a")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.title, "new-title");

    let (pinned_at, order) = admin::set_pinned(&db, "a", true, now + 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pinned_at, Some((now + 1) as i64));
    assert_eq!(order, Some(1));
    let (_, order_b) = admin::set_pinned(&db, "b", true, now + 2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(order_b, Some(2), "后置顶排到区尾");
    // 重排：b 在前。
    admin::set_pin_order(&db, &["b".to_string(), "a".to_string()], now + 3)
        .await
        .unwrap();
    let row_a = lingxi_adapters::storage::session_tree::get_session_row(&db, "a")
        .await
        .unwrap()
        .unwrap();
    let row_b = lingxi_adapters::storage::session_tree::get_session_row(&db, "b")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row_b.pin_order, Some(1));
    assert_eq!(row_a.pin_order, Some(2));
    // 未置顶会话出现在重排列表 → 响亮失败。
    assert!(admin::set_pin_order(&db, &["ghost".to_string()], now + 4)
        .await
        .is_err());
    // 取消置顶。
    let (pin_off, ord_off) = admin::set_pinned(&db, "a", false, now + 5)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((pin_off, ord_off), (None, None));

    // 记忆开关默认开（schema default 1），可关。
    let row = lingxi_adapters::storage::session_tree::get_session_row(&db, "b")
        .await
        .unwrap()
        .unwrap();
    assert!(row.memory_enabled);
    assert_eq!(
        admin::set_memory_enabled(&db, "b", false).await.unwrap(),
        Some(false)
    );
    assert_eq!(
        admin::set_memory_enabled(&db, "ghost", true).await.unwrap(),
        None
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

/// 归档三态：无 childMode 有子对话 → child_sessions_present；detach 摘直接
/// 子对话；archive_children 递归（skip 名单跳过并计数）；restore/delete 的
/// 生命周期闸（WrongLifecycle 响亮三态）。
#[tokio::test]
async fn admin_archive_restore_delete_lifecycle_gates() {
    const TAG: &str = "admin-arch";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    use lingxi_adapters::storage::session_admin as admin;
    use lingxi_adapters::storage::session_tree::{
        fork_session, seed_session, ForkRequest, SeedSession,
    };
    seed_session(
        &db,
        SeedSession {
            session_id: "p",
            agent_id: "a",
            owner_user_id: "local-owner",
            title: "p",
            now_unix_ms: now,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();
    lingxi_adapters::storage::session_tree::append_message(
        &db,
        lingxi_adapters::storage::session_tree::AppendMessage {
            session_id: "p",
            message_id: "m1",
            parent_message_id: None,
            run_id: None,
            role: "user",
            content_json: "{}",
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: now,
        },
    )
    .await
    .unwrap();
    fork_session(
        &db,
        ForkRequest {
            source_session_id: "p",
            new_session_id: "c1",
            boundary_message_id: "m1",
            now_unix_ms: now + 1,
        },
    )
    .await
    .unwrap();
    fork_session(
        &db,
        ForkRequest {
            source_session_id: "c1",
            new_session_id: "g1",
            boundary_message_id: "m1",
            now_unix_ms: now + 2,
        },
    )
    .await
    .unwrap();

    // 无 childMode 有子对话 → Conflict child_sessions_present。
    let err = admin::archive_session(&db, "p", None, &[], now + 3)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, lingxi_kernel::ports::StorageError::Conflict { detail } if detail.starts_with("child_sessions_present")),
        "现役 409 形状：{err:?}"
    );

    // archive_children，g1 在 skip 名单 → 跳过并计数。
    let out = admin::archive_session(
        &db,
        "p",
        Some(admin::ChildMode::ArchiveChildren),
        &["g1".to_string()],
        now + 4,
    )
    .await
    .unwrap();
    assert_eq!(out.archived_children, 1, "c1 归档");
    assert_eq!(out.skipped_streaming_children, 1, "g1 跳过");
    let g = lingxi_adapters::storage::session_tree::get_session_row(&db, "g1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(g.lifecycle, "active", "skip 名单保持 active");

    // 已归档会话再归档 → Conflict（WrongLifecycle）。
    assert!(admin::archive_session(&db, "p", None, &[], now + 5)
        .await
        .is_err());
    // restore 回 active。
    assert_eq!(
        admin::restore_session(&db, "p", now + 6).await.unwrap(),
        admin::LifecycleOutcome::Done
    );
    // active 会话不可永久删除 → WrongLifecycle。
    assert_eq!(
        admin::delete_archived_session(&db, "p").await.unwrap(),
        admin::LifecycleOutcome::WrongLifecycle
    );
    // 归档后删除 → Done；再删 → NotFound。
    admin::archive_session(
        &db,
        "p",
        Some(admin::ChildMode::DetachChildren),
        &[],
        now + 7,
    )
    .await
    .unwrap();
    let c1 = lingxi_adapters::storage::session_tree::get_session_row(&db, "c1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(c1.parent_session_id, None, "detach 摘直接子对话谱系");
    assert_eq!(
        admin::delete_archived_session(&db, "p").await.unwrap(),
        admin::LifecycleOutcome::Done
    );
    assert_eq!(
        admin::delete_archived_session(&db, "p").await.unwrap(),
        admin::LifecycleOutcome::NotFound
    );
    assert!(
        lingxi_adapters::storage::session_tree::get_session_row(&db, "p")
            .await
            .unwrap()
            .is_none()
    );
    // FK 序证据：c1 仍可读（只删了 p），messages 中 p 的行已清。
    assert!(
        lingxi_adapters::storage::session_tree::get_session_row(&db, "c1")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        lingxi_adapters::storage::session_tree::list_all_messages(&db, "p")
            .await
            .unwrap()
            .is_empty()
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

/// 搜索：title/content 两阶段、owner 过滤、LIKE 通配符注入免疫。
#[tokio::test]
async fn admin_search_title_content_owner_and_like_escape() {
    const TAG: &str = "admin-search";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    use lingxi_adapters::storage::session_admin as admin;
    use lingxi_adapters::storage::session_tree::{
        append_message, seed_session, AppendMessage, SeedSession,
    };
    seed_session(
        &db,
        SeedSession {
            session_id: "mine",
            agent_id: "a",
            owner_user_id: "user_local",
            title: "weekly report",
            now_unix_ms: now,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();
    seed_session(
        &db,
        SeedSession {
            session_id: "other",
            agent_id: "a",
            owner_user_id: "user_remote",
            title: "weekly other",
            now_unix_ms: now,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();
    append_message(
        &db,
        AppendMessage {
            session_id: "mine",
            message_id: "m1",
            parent_message_id: None,
            run_id: None,
            role: "user",
            content_json: r#"{"text":"needle in content"#,
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: now,
        },
    )
    .await
    .unwrap();

    // title 阶段 + owner 过滤：remote 看不到我的 weekly。
    let hits = admin::search_sessions(&db, Some("user_remote"), "%weekly%", "title", 10)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].session_id, "other");
    let hits = admin::search_sessions(&db, None, "%weekly%", "title", 10)
        .await
        .unwrap();
    assert_eq!(hits.len(), 2, "本地主人视野 = 全部");

    // content 阶段：命中消息内容。
    let hits = admin::search_sessions(&db, None, "%needle%", "content", 10)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].match_kind, "content");

    // LIKE 通配符免疫：查询含 % 不再通配（服务层已转义后的 pattern 直传）。
    let hits = admin::search_sessions(&db, None, "%we_kly%", "title", 10)
        .await
        .unwrap();
    assert_eq!(
        hits.len(),
        2,
        "未转义下 _ 是通配符（存储层按 pattern 直传）"
    );
    let hits = admin::search_sessions(&db, None, r"%we\_kly%", "title", 10)
        .await
        .unwrap();
    assert!(hits.is_empty(), "转义后 _ 是字面量 → 无命中");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

// ------------------------------------------------------------- REPAIR-R1 回归

/// REPAIR-R1 FINDING-01：latest 覆盖 × filePaths 组合——同文件重录成功（不再
/// PK 冲突 500）、异文件不残留、内容子行随行替换（rewind 核验面=新集合）。
#[tokio::test]
async fn latest_overwrite_with_file_paths_replaces_versions_and_contents_atomically() {
    const TAG: &str = "ckpt-latest-files";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    use lingxi_adapters::storage::session_tree as store;
    store::seed_session(
        &db,
        store::SeedSession {
            session_id: "s",
            agent_id: "a",
            owner_user_id: "local-owner",
            title: "s",
            now_unix_ms: now,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();
    let file = |p: &str, bytes: &'static [u8]| store::CheckpointFileSpec {
        file_path: p.to_string(),
        sha256: lingxi_kernel::session_tree::sha256_hex(bytes),
        size_bytes: bytes.len() as u64,
        content: bytes.to_vec(),
        encoding: "utf-8".to_string(),
    };

    // 第一次 latest 带 f1。
    let cp1 = store::upsert_checkpoint(
        &db,
        store::UpsertCheckpoint {
            session_id: "s",
            name: "latest",
            target_message_id: "m1",
            turn_input_message_id: None,
            message_count: 1,
            now_unix_ms: now,
            files: vec![file("/w/f1", b"v1")],
        },
    )
    .await
    .unwrap();
    // 第二次 latest 带同路径 f1 的新内容：必须成功（R1 probe 场景 A 修复）。
    let cp2 = store::upsert_checkpoint(
        &db,
        store::UpsertCheckpoint {
            session_id: "s",
            name: "latest",
            target_message_id: "m2",
            turn_input_message_id: None,
            message_count: 2,
            now_unix_ms: now + 1,
            files: vec![file("/w/f1", b"v2")],
        },
    )
    .await
    .unwrap();
    assert_eq!(cp1.checkpoint_id, cp2.checkpoint_id, "latest 覆盖同 id");
    let versions = store::list_file_versions(&db, &cp2.checkpoint_id)
        .await
        .unwrap();
    assert_eq!(versions.len(), 1, "同文件重录不留双行");
    assert_eq!(
        versions[0].sha256,
        lingxi_kernel::session_tree::sha256_hex(b"v2"),
        "版本行指向新内容"
    );
    // 内容子行同步替换（rewind 可恢复的是新字节）。
    let contents = store::list_file_contents(&db, &cp2.checkpoint_id)
        .await
        .unwrap();
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0].content, b"v2");

    // 第三次 latest 带不同文件 f2：旧文件 f1 不残留（R1 probe 场景 B 修复）。
    let cp3 = store::upsert_checkpoint(
        &db,
        store::UpsertCheckpoint {
            session_id: "s",
            name: "latest",
            target_message_id: "m3",
            turn_input_message_id: None,
            message_count: 3,
            now_unix_ms: now + 2,
            files: vec![file("/w/f2", b"x")],
        },
    )
    .await
    .unwrap();
    let paths: Vec<String> = store::list_file_versions(&db, &cp3.checkpoint_id)
        .await
        .unwrap()
        .into_iter()
        .map(|v| v.file_path)
        .collect();
    assert_eq!(paths, vec!["/w/f2".to_string()], "旧文件版本不残留");
    let content_paths: Vec<String> = store::list_file_contents(&db, &cp3.checkpoint_id)
        .await
        .unwrap()
        .into_iter()
        .map(|c| c.file_path)
        .collect();
    assert_eq!(content_paths, vec!["/w/f2".to_string()], "旧内容不残留");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

/// REPAIR-R1 FINDING-01 同根因扩展：201 窗口裁减携带文件版本/内容的检查点
/// 不再 FK 失败，被裁检查点的子行清零。
#[tokio::test]
async fn checkpoint_eviction_with_file_versions_is_fk_safe() {
    const TAG: &str = "ckpt-cap-fk";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    use lingxi_adapters::storage::session_tree as store;
    store::seed_session(
        &db,
        store::SeedSession {
            session_id: "s",
            agent_id: "a",
            owner_user_id: "local-owner",
            title: "s",
            now_unix_ms: now,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();
    // 200 个具名检查点，每个都带文件版本+内容；第 201 个触发裁减 cp000。
    for i in 0..=200u32 {
        store::upsert_checkpoint(
            &db,
            store::UpsertCheckpoint {
                session_id: "s",
                name: &format!("cp{i:03}"),
                target_message_id: "m0",
                turn_input_message_id: None,
                message_count: 0,
                now_unix_ms: now + u64::from(i),
                files: vec![store::CheckpointFileSpec {
                    file_path: format!("/w/f{i}"),
                    sha256: lingxi_kernel::session_tree::sha256_hex(format!("v{i}").as_bytes()),
                    size_bytes: 2,
                    content: format!("v{i}").into_bytes(),
                    encoding: "utf-8".to_string(),
                }],
            },
        )
        .await
        .unwrap_or_else(|e| panic!("upsert #{i} 必须成功（裁减 FK 安全）：{e:?}"));
    }
    let rows = store::list_checkpoints(&db, "s").await.unwrap();
    assert_eq!(rows.len(), 200, "上限 200");
    assert!(!rows.iter().any(|r| r.name == "cp000"), "最老被裁");
    // 被裁检查点的子行清零（FK 序删除生效）。
    assert!(
        store::list_file_versions(&db, "s:cp000")
            .await
            .unwrap()
            .is_empty(),
        "被裁检查点的文件版本随行清除"
    );
    assert!(
        store::list_file_contents(&db, "s:cp000")
            .await
            .unwrap()
            .is_empty(),
        "被裁检查点的内容随行清除"
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

/// REPAIR-R1 FINDING-04：fork 复制全字段保真——model_call_id、
/// committed_at_unix_ms、run 归属、条目类型原样（仅 parent/seq 重链），
/// 对照现役 session-manager.js:1113 浅拷贝。
#[tokio::test]
async fn fork_preserves_message_fields_verbatim() {
    const TAG: &str = "fork-fidelity";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    use lingxi_adapters::storage::session_tree as store;
    store::seed_session(
        &db,
        store::SeedSession {
            session_id: "src",
            agent_id: "a",
            owner_user_id: "local-owner",
            title: "s",
            now_unix_ms: now,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();
    store::append_message(
        &db,
        store::AppendMessage {
            session_id: "src",
            message_id: "m1",
            parent_message_id: None,
            run_id: None,
            role: "user",
            content_json: "{\"text\":\"hi\"}",
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: now,
        },
    )
    .await
    .unwrap();
    store::append_message(
        &db,
        store::AppendMessage {
            session_id: "src",
            message_id: "m2",
            parent_message_id: Some("m1"),
            run_id: None,
            role: "assistant",
            content_json: "{\"text\":\"ok\"}",
            entry_type: "message",
            model_call_id: Some("mc-9"),
            now_unix_ms: now + 10,
        },
    )
    .await
    .unwrap();

    store::fork_session(
        &db,
        store::ForkRequest {
            source_session_id: "src",
            new_session_id: "dst",
            boundary_message_id: "m2",
            now_unix_ms: now + 999,
        },
    )
    .await
    .unwrap();
    let src_msgs = store::list_branch_messages(&db, "src").await.unwrap();
    let dst_msgs = store::list_branch_messages(&db, "dst").await.unwrap();
    assert_eq!(src_msgs.len(), 2);
    assert_eq!(dst_msgs.len(), 2);
    for (s, d) in src_msgs.iter().zip(dst_msgs.iter()) {
        assert_eq!(s.message_id, d.message_id, "消息 ID 稳定");
        assert!(
            d.run_id.is_none(),
            "副本不持有源会话 runs 活引用（run_id 置 NULL；源 run 可追溯性 \
             由消息 id 保留）——源会话永久删除不被副本 FK 卡死"
        );
        assert_eq!(s.role, d.role);
        assert_eq!(s.content_json, d.content_json);
        assert_eq!(s.entry_type, d.entry_type, "条目类型原样");
        assert_eq!(
            s.model_call_id, d.model_call_id,
            "模型调用溯源字段保留（FINDING-04）"
        );
        assert_eq!(
            s.committed_at_unix_ms, d.committed_at_unix_ms,
            "提交时刻不改写为 fork 时刻（FINDING-04）"
        );
    }
    assert_eq!(dst_msgs[1].model_call_id.as_deref(), Some("mc-9"));
    assert_eq!(dst_msgs[1].committed_at_unix_ms, (now + 10) as i64);
    assert_eq!(dst_msgs[0].parent_message_id, None, "root parent 置 NULL");
    assert_eq!(dst_msgs[1].parent_message_id.as_deref(), Some("m1"));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

/// REPAIR-R1 FINDING-06：messages.run_id 参照完整性恢复——指向不存在 run 的
/// 消息插入响亮失败；NULL（无 run 归属条目）正常。
#[tokio::test]
async fn ghost_run_id_is_rejected_by_foreign_key() {
    const TAG: &str = "msg-fk";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    use lingxi_adapters::storage::session_tree as store;
    store::seed_session(
        &db,
        store::SeedSession {
            session_id: "s",
            agent_id: "a",
            owner_user_id: "local-owner",
            title: "s",
            now_unix_ms: now,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();
    let err = store::append_message(
        &db,
        store::AppendMessage {
            session_id: "s",
            message_id: "m-ghost",
            parent_message_id: None,
            run_id: Some("run-does-not-exist"),
            role: "user",
            content_json: "{}",
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: now,
        },
    )
    .await
    .unwrap_err();
    let detail = format!("{err:?}");
    assert!(
        detail.contains("FOREIGN KEY") || detail.contains("foreign key"),
        "幽灵 run 归属必须被 FK 响亮拒绝：{detail}"
    );
    // NULL run_id（用户输入/reset 标记形态）正常。
    store::append_message(
        &db,
        store::AppendMessage {
            session_id: "s",
            message_id: "m-ok",
            parent_message_id: None,
            run_id: None,
            role: "user",
            content_json: "{}",
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: now + 1,
        },
    )
    .await
    .unwrap();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

/// REPAIR-R1 FINDING-08：archive_children 只计真实翻转的后代——已归档的
/// 后代不虚增计数（对照现役 sessions.ts:2569-2660 逐子成功才计）。
#[tokio::test]
async fn archive_children_counts_only_actual_flips() {
    const TAG: &str = "arch-count";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    use lingxi_adapters::storage::session_admin as admin;
    use lingxi_adapters::storage::session_tree as store;
    store::seed_session(
        &db,
        store::SeedSession {
            session_id: "p",
            agent_id: "a",
            owner_user_id: "local-owner",
            title: "p",
            now_unix_ms: now,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();
    store::append_message(
        &db,
        store::AppendMessage {
            session_id: "p",
            message_id: "m1",
            parent_message_id: None,
            run_id: None,
            role: "user",
            content_json: "{}",
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: now,
        },
    )
    .await
    .unwrap();
    for child in ["c1", "c2"] {
        store::fork_session(
            &db,
            store::ForkRequest {
                source_session_id: "p",
                new_session_id: child,
                boundary_message_id: "m1",
                now_unix_ms: now + 1,
            },
        )
        .await
        .unwrap();
    }
    // c2 先归档（无子）。
    admin::archive_session(&db, "c2", None, &[], now + 2)
        .await
        .unwrap();
    let out = admin::archive_session(
        &db,
        "p",
        Some(admin::ChildMode::ArchiveChildren),
        &[],
        now + 3,
    )
    .await
    .unwrap();
    assert_eq!(
        out.archived_children, 1,
        "只计真实翻转的 c1；已归档的 c2 不虚增"
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

/// REPAIR-R1 观察项：fork new_session_id 撞库 → 响亮 Conflict(session_exists)
/// （生产链映射 409；不再落 Internal 500）。同类入口 create_session 同口径。
#[tokio::test]
async fn duplicate_session_id_is_loud_conflict_on_fork_and_create() {
    const TAG: &str = "dup-id";
    let path = temp_db(TAG);
    let db = open_db(&path).await;
    let now = 1_000u64;
    use lingxi_adapters::storage::session_admin as admin;
    use lingxi_adapters::storage::session_tree as store;
    store::seed_session(
        &db,
        store::SeedSession {
            session_id: "p",
            agent_id: "a",
            owner_user_id: "local-owner",
            title: "p",
            now_unix_ms: now,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();
    store::append_message(
        &db,
        store::AppendMessage {
            session_id: "p",
            message_id: "m1",
            parent_message_id: None,
            run_id: None,
            role: "user",
            content_json: "{}",
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: now,
        },
    )
    .await
    .unwrap();
    store::fork_session(
        &db,
        store::ForkRequest {
            source_session_id: "p",
            new_session_id: "dup",
            boundary_message_id: "m1",
            now_unix_ms: now + 1,
        },
    )
    .await
    .unwrap();
    let err = store::fork_session(
        &db,
        store::ForkRequest {
            source_session_id: "p",
            new_session_id: "dup",
            boundary_message_id: "m1",
            now_unix_ms: now + 2,
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(
            &err,
            lingxi_kernel::ports::StorageError::Conflict { detail }
                if detail.starts_with("session_exists:")
        ),
        "fork 撞库必须响亮 Conflict（session_exists）：{err:?}"
    );
    // 同类入口：create_session 撞库同口径。
    let err2 = admin::create_session(&db, "p", "a", "u", "t", None, &[], true, now + 3)
        .await
        .unwrap_err();
    assert!(
        matches!(
            &err2,
            lingxi_kernel::ports::StorageError::Conflict { detail }
                if detail.starts_with("session_exists:")
        ),
        "create_session 撞库必须响亮 Conflict（session_exists）：{err2:?}"
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lingxi_adapters::storage::wal_sidecar_path(&path));
}

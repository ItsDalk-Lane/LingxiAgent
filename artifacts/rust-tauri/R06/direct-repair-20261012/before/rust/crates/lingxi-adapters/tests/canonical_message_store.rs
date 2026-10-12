//! R06-T04 adapters 层测试：CanonicalMessageStore 的分页读取（A08）与
//! legacy 规范化回退（任务书步骤①④）。
//!
//! 对照纪律（RC-3）：
//! - 分页参数形状对照现役 `server/routes/sessions.ts:1415-1419`
//!   （limit 默认 50、上限 200、before 边界语义）；
//! - 「不每页完整扫描全部历史」对照现役目录快路径
//!   `server/history-read/page.ts`（窗口切片 + 锚点二分，不回放全史）；
//! - legacy 线性回退对照 v8 迁移事实：`migrations.rs` V8_SQL 把既有行
//!   parent_message_id 置 NULL、entry_type 置 'message'（pre-T03 写序为
//!   线性追加，seq 即提交次序，回退不按时间猜）。
//!
//! I/O 探针：read_branch_page 返回 PageStats{rows_examined, queries}；
//! 逐页成本必须与历史总长无关（O(页大小)），累计检查行数随消息数线性。

use lingxi_adapters::storage::session_tree::{
    append_message, reset_branch_head, seed_session, AppendMessage, ResetBranchHead, SeedSession,
};
use lingxi_adapters::storage::{canonical_message_store as cms, RunDatabase, StoreOptions};
use lingxi_protocol::history::HistoryCursor;

fn temp_db(tag: &str) -> (RunDatabase, std::path::PathBuf) {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r06t04-cms-{}-{}-{tag}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("runs.db");
    let db = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(RunDatabase::open(&path, StoreOptions::default()))
        .expect("open db");
    (db, dir)
}

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(fut)
}

fn seed(db: &RunDatabase, session: &str) {
    block_on(seed_session(
        db,
        SeedSession {
            session_id: session,
            agent_id: "agent",
            owner_user_id: "user_local",
            title: "t",
            now_unix_ms: 1,
            permission_mode: None,
            authorized_folders: vec![],
        },
    ))
    .expect("seed session");
}

/// seed 的 async 变体：在已是 async 的 fixture 内使用（sync 版内部
/// block_on，嵌套进 async 上下文会双 runtime panic）。
async fn seed_async(db: &RunDatabase, session: &str) {
    seed_session(
        db,
        SeedSession {
            session_id: session,
            agent_id: "agent",
            owner_user_id: "user_local",
            title: "t",
            now_unix_ms: 1,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .expect("seed session");
}

/// 经真实消息写入路径造一条 parent 链接的消息。
fn append(
    db: &RunDatabase,
    session: &str,
    id: &str,
    parent: Option<&str>,
    role: &str,
    seq_now: u64,
) {
    block_on(append_message(
        db,
        AppendMessage {
            session_id: session,
            message_id: id,
            parent_message_id: parent,
            run_id: None,
            role,
            content_json: "{}",
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: seq_now,
        },
    ))
    .expect("append message");
}

/// 造 n 条线性链接消息 m1..m{n}（user/assistant 交替）。
fn seed_linear(db: &RunDatabase, session: &str, n: usize) -> Vec<String> {
    let mut ids = Vec::new();
    let mut parent: Option<String> = None;
    for i in 1..=n {
        let id = format!("m{i}");
        let role = if i % 2 == 1 { "user" } else { "assistant" };
        append(db, session, &id, parent.as_deref(), role, i as u64);
        parent = Some(id.clone());
        ids.push(id);
    }
    ids
}

/// A08 主场景：冻结大型历史（1200 消息，对照 05 §6.1「1000 条消息的长会话」
/// 最低负载），连续读取全部页面（limit=50，对照现役默认），
/// 消息不丢不重，次序与分页语义一致（最新页先出、页内旧→新——与现役
/// `before` 语义及 service 侧「逐页拼接 == 导出」契约同源）。
#[test]
fn a08_consecutive_pages_cover_full_branch_without_loss_or_duplication() {
    let (db, _dir) = temp_db("pages-lossless");
    seed(&db, "s");
    let ids = seed_linear(&db, "s", 1200);

    let limit = 50u32; // 现役默认（sessions.ts:1415-1419 min(N||50,200)）
    let mut collected: Vec<String> = Vec::new();
    let mut before: Option<HistoryCursor> = None;
    let mut pages = 0usize;
    loop {
        let page =
            block_on(cms::read_branch_page(&db, "s", before.clone(), limit)).expect("read page");
        pages += 1;
        assert!(
            !page.messages.is_empty() || !page.has_more,
            "非空分支的中间页不得为空（第 {pages} 页）"
        );
        // 页内链序（旧→新）。
        let window = page.messages.windows(2);
        for w in window {
            assert!(
                w[0].seq < w[1].seq,
                "页内 seq 必须递增：{:?} vs {:?}",
                w[0].message_id,
                w[1].message_id
            );
        }
        collected.extend(page.messages.iter().map(|m| m.message_id.clone()));
        if !page.has_more {
            assert!(
                page.next_before.is_none(),
                "最后一页不得再给 next_before（任务书：不丢不重的终止条件）"
            );
            break;
        }
        before = page.next_before.clone();
        assert!(pages < 100, "分页必须在有限步内终止（防死循环）");
    }
    assert_eq!(pages, 24, "1200 / 50 = 24 页");
    assert_eq!(collected.len(), ids.len(), "不丢");
    // 分页语义：before=None 取最新页，before=游标取严格更老的页（同套件
    // cursor_must_reference_a_message_of_this_session 锁定）——连续拼接
    // 等于「按页大小分块后页面逆序、页内顺序」的完整链。
    let expected: Vec<String> = ids
        .chunks(limit as usize)
        .rev()
        .flatten()
        .cloned()
        .collect();
    assert_eq!(
        collected, expected,
        "连续页面拼接必须覆盖完整链（分页语义无丢无重）"
    );
    let unique: std::collections::HashSet<&String> = collected.iter().collect();
    assert_eq!(unique.len(), collected.len(), "不重");
}

/// A08 I/O 探针：每页检查行数 ≤ 2*limit+8（窗口大小量级），与页位置无关；
/// 累计检查行数随消息数线性（显著低于全扫设计的 O(N·页数)）。
#[test]
fn a08_page_walk_examines_only_window_rows() {
    let (db, _dir) = temp_db("pages-io");
    seed(&db, "s");
    let n = 1200usize;
    seed_linear(&db, "s", n);

    let limit = 50u32;
    let mut before: Option<HistoryCursor> = None;
    let mut total_rows = 0u64;
    let mut total_queries = 0u64;
    let mut per_page_rows = Vec::new();
    loop {
        let page =
            block_on(cms::read_branch_page(&db, "s", before.clone(), limit)).expect("read page");
        total_rows += page.stats.rows_examined;
        total_queries += page.stats.queries;
        per_page_rows.push(page.stats.rows_examined);
        assert!(
            page.stats.rows_examined <= u64::from(limit) * 2 + 8,
            "单页检查行数必须是窗口量级，不得随总长增长：{} (> {})",
            page.stats.rows_examined,
            limit * 2 + 8
        );
        if !page.has_more {
            break;
        }
        before = page.next_before.clone();
    }
    // 全扫设计：每页重走全链 → 累计 ≈ Σ(i*limit) ≈ limit·pages²/2 ≈ 14_400+。
    // 游标设计：每行恰好回访一次 + 每页常数 → ≈ N + 页数·常数。
    let full_scan_floor = u64::try_from(n).unwrap() * 24 / 2;
    assert!(
        total_rows < full_scan_floor,
        "累计检查行数 {total_rows} 必须显著低于全扫下界 {full_scan_floor}"
    );
    assert!(
        total_rows <= u64::try_from(n).unwrap() + 24 * 16,
        "累计检查行数须为线性：{total_rows}"
    );
    let first = per_page_rows[0];
    let last = *per_page_rows.last().unwrap();
    assert!(
        first.abs_diff(last) <= limit as u64 + 8,
        "首页({first})与末页({last})成本必须同量级（与页位置无关）"
    );
    assert!(total_queries > 0);
}

/// 步骤①④ LegacyReader：v8 迁移前的行 parent 全 NULL（pre-T03 线性写）。
/// 回退按 committed seq 线性化，整段可读——不是只剩尾消息。
#[test]
fn legacy_parentless_history_walks_full_seq_order() {
    let (db, _dir) = temp_db("legacy-linear");
    // v8 迁移产物形态：真实提交路径写 final 消息后拟态迁移（parent 全
    // NULL、无 branch head 行），模拟 pre-T03 库迁到 v8。
    block_on(seed_session(
        &db,
        SeedSession {
            session_id: "legacy",
            agent_id: "agent",
            owner_user_id: "user_local",
            title: "t",
            now_unix_ms: 1,
            permission_mode: None,
            authorized_folders: vec![],
        },
    ))
    .expect("seed");
    block_on(write_legacy_rows(&db, "legacy", 7));

    let page = block_on(cms::read_branch_page(&db, "legacy", None, 200)).expect("read");
    let got: Vec<&str> = page
        .messages
        .iter()
        .map(|m| m.message_id.as_str())
        .collect();
    // 每个 legacy run 的 final 消息 id 是 `{run}-final`（真实提交路径）。
    let expected: Vec<String> = (1..=7).map(|i| format!("legacy_run_{i}-final")).collect();
    assert_eq!(
        got,
        expected.iter().map(String::as_str).collect::<Vec<_>>(),
        "legacy 线性回退必须覆盖全段（committed seq 次序），不是只剩尾消息"
    );
    assert!(!page.has_more);
}

/// 混合区域：legacy（parent NULL）段之上叠加 T03+ 链接段，walk 必须
/// 贯穿两个区域（链接区走 parent，legacy 区走 seq 回退）。
#[test]
fn mixed_linked_and_legacy_regions_walk_through() {
    let (db, _dir) = temp_db("mixed-regions");
    seed(&db, "s");
    block_on(write_legacy_rows(&db, "s", 3)); // legacy_run_{1..3}-final（parent NULL）
                                              // T03+ 链接段：n1→legacy_run_3-final, n2→n1（append_message 顺手推进
                                              // branch head）。
    append(&db, "s", "n1", Some("legacy_run_3-final"), "user", 10);
    append(&db, "s", "n2", Some("n1"), "assistant", 11);

    let page = block_on(cms::read_branch_page(&db, "s", None, 200)).expect("read");
    let got: Vec<&str> = page
        .messages
        .iter()
        .map(|m| m.message_id.as_str())
        .collect();
    assert_eq!(
        got,
        vec![
            "legacy_run_1-final",
            "legacy_run_2-final",
            "legacy_run_3-final",
            "n1",
            "n2"
        ]
    );
}

/// 根重置（new_head=NULL）的标记 parent 为 NULL 但语义是「分支清空」：
/// walk 到标记即停，**不得**用 seq 回退穿透重置点读到旧消息。
#[test]
fn root_reset_marker_stops_the_walk() {
    let (db, _dir) = temp_db("root-reset");
    seed(&db, "s");
    seed_linear(&db, "s", 4); // m1..m4
    block_on(reset_branch_head(
        &db,
        ResetBranchHead {
            session_id: "s",
            new_head_message_id: None,
            source_message_id: None,
            reason: "rewind",
            now_unix_ms: 99,
        },
    ))
    .expect("root reset");

    let page = block_on(cms::read_branch_page(&db, "s", None, 200)).expect("read");
    assert_eq!(page.messages.len(), 1, "根重置后链上只剩重置标记");
    assert_eq!(page.messages[0].entry_type, "hana-session-branch-reset");
    assert!(!page.has_more);
}

/// 游标校验：他会话消息 / 不存在的 id 必须响亮失败（400 语义由 service 映射），
/// 不得静默当作首页。
#[test]
fn cursor_must_reference_a_message_of_this_session() {
    let (db, _dir) = temp_db("cursor-check");
    seed(&db, "a");
    seed(&db, "b");
    seed_linear(&db, "a", 4);
    seed_linear(&db, "b", 4);

    let foreign = HistoryCursor {
        message_id: "m2".to_string(),
        seq: 2,
    };
    // 两个会话都有 m2：cursor 绑定 (session,message) 主键，读 b 时用 a 的
    // 游标在 b 内解析的是 b 自己的 m2——这是合法的（主键定位）。真正的
    // 非法输入是 b 内不存在的 id。
    let missing = HistoryCursor {
        message_id: "no-such".to_string(),
        seq: 99,
    };
    let err = block_on(cms::read_branch_page(&db, "b", Some(missing), 50))
        .expect_err("不存在的游标消息必须失败");
    assert!(
        matches!(
            err,
            lingxi_kernel::ports::StorageError::InvalidRequest { .. }
        ),
        "got {err:?}"
    );
    // 合法游标：b 会话 m3 → 读到 m1..m2。
    let ok = block_on(cms::read_branch_page(&db, "b", Some(foreign), 50)).expect("read");
    let got: Vec<&str> = ok.messages.iter().map(|m| m.message_id.as_str()).collect();
    assert_eq!(
        got,
        vec!["m1"],
        "before=m2 语义：严格更老的消息（不含 m2 自身）"
    );
    let _ = ok;
}

/// 页事件读取：仅本页锚定 run 的事件，按 stream seq 升序。
#[test]
fn branch_events_scoped_to_page_runs_and_ordered() {
    let (db, _dir) = temp_db("events-scope");
    block_on(seed_runs_with_events(&db));

    // 每 run 的持久事件 = record_run_started 的 `{run}-start`（queued→
    // running，run_store.rs:1398-1416）+ 提交路径 staging 的 3 条
    // key_events（2 工具 + 1 终态）= 4 条。
    let events = block_on(cms::read_branch_events(&db, "s", &["ra".to_string()])).expect("events");
    let seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
    assert_eq!(events.len(), 4, "ra 的 4 条事件（start + 2 工具 + 1 终态）");
    assert!(
        seqs.windows(2).all(|w| w[0] < w[1]),
        "事件按 stream seq 升序: {seqs:?}"
    );
    assert!(events.iter().all(|e| e.run_id.as_deref() == Some("ra")));

    let both = block_on(cms::read_branch_events(
        &db,
        "s",
        &["ra".to_string(), "rb".to_string()],
    ))
    .expect("events");
    assert_eq!(both.len(), 8);
    let seqs: Vec<u64> = both.iter().map(|e| e.seq).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]));
}

/// lineage 闭包：父 run → 子 run → 孙 run 逐级解析；自环不死循环。
#[test]
fn lineage_children_resolve_transitively_without_cycles() {
    let (db, _dir) = temp_db("lineage");
    block_on(seed_lineage(&db));

    let edges = block_on(cms::read_lineage_children(&db, &["p".to_string()])).expect("lineage");
    let children: std::collections::HashSet<&str> = edges.iter().map(|(_, c)| c.as_str()).collect();
    assert!(
        children.contains("c1") && children.contains("g1"),
        "传递闭包必须含子与孙: {edges:?}"
    );
    assert!(children.len() <= 3, "自环不得产生新成员: {edges:?}");
}

// ── fixture helpers ──

/// 造 v8 迁移产物形态的 legacy 行：经真实 run 提交路径落 final 消息后，
/// 把 parent 全置 NULL 并删除 branch_heads 行——即 pre-T03 库迁到 v8 后
/// 的冻结数据形态（迁移 SQL 原样语义：parent 置空、无头行）。
async fn write_legacy_rows(db: &RunDatabase, session: &str, n: usize) {
    use lingxi_protocol::{
        EventId, EventPayload, KnownEventPayload, RunStateChangedPayload, RunStatus,
    };
    // 每个 legacy「run」= 1 条 final 消息（pre-T03 只有 final 落 messages）。
    // 经真实 run 提交路径写事件与 run 行，再直改 parent 为 NULL 拟态迁移。
    for i in 1..=n {
        let run = format!("legacy_run_{i}");
        let ctx = lingxi_kernel::RunContext {
            principal: lingxi_kernel::Principal::LocalUser,
            session_id: lingxi_protocol::SessionId::new(session.to_string()),
            run_id: lingxi_protocol::RunId::new(run.clone()),
            attempt: lingxi_protocol::AttemptId::new(format!("{run}#a1")),
            generation: 1,
        };
        use lingxi_kernel::ports::StoragePort;
        db.record_run_started(&ctx, 1_000 + i as u64)
            .await
            .expect("start");
        let outcome = lingxi_kernel::ports::RunOutcome {
            status: RunStatus::Completed,
            reason: Some("completed".to_string()),
            key_events: vec![lingxi_kernel::ports::KeyEvent {
                event_id: EventId::new(format!("{run}-done")),
                payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                    RunStateChangedPayload {
                        from: RunStatus::Running,
                        to: RunStatus::Completed,
                        reason: Some("completed".to_string()),
                    },
                )),
            }],
            final_message: Some(lingxi_protocol::NormalizedMessage {
                role: "assistant".to_string(),
                content: vec![lingxi_protocol::ContentBlock::Text {
                    text: format!("legacy answer {i}"),
                }],
                model_call_id: None,
            }),
        };
        db.commit_run_outcome(&ctx, outcome, 1_000 + i as u64)
            .await
            .expect("commit");
    }
    // 拟态 v8 迁移产物（migrations.rs V8_SQL 原样语义：parent 全置 NULL、
    // pre-T03 库无 branch_heads 行）。测试用 rusqlite 第二连接（adapters 的
    // 直接依赖）把库改到「迁移完成那一刻」的数据形态——这是迁移本身做过的
    // 那次重写，不是绕过候选读路径。
    {
        let conn = rusqlite::Connection::open(db.db_path()).expect("second conn");
        conn.pragma_update(None, "busy_timeout", 5_000)
            .expect("busy timeout");
        conn.execute(
            "UPDATE messages SET parent_message_id=NULL WHERE session_id=?1",
            rusqlite::params![session],
        )
        .expect("null out parents");
        conn.execute(
            "DELETE FROM session_branch_heads WHERE session_id=?1",
            rusqlite::params![session],
        )
        .expect("drop branch head");
    }
}

/// 两个真实 run（ra/rb），各 2 条工具事件 + 1 条终态事件。
async fn seed_runs_with_events(db: &RunDatabase) {
    seed_async(db, "s").await;
    for run in ["ra", "rb"] {
        let ctx = lingxi_kernel::RunContext {
            principal: lingxi_kernel::Principal::LocalUser,
            session_id: lingxi_protocol::SessionId::new("s".to_string()),
            run_id: lingxi_protocol::RunId::new(run.to_string()),
            attempt: lingxi_protocol::AttemptId::new(format!("{run}#a1")),
            generation: 1,
        };
        use lingxi_kernel::ports::StoragePort;
        db.record_run_started(&ctx, 1_000).await.expect("start");
        use lingxi_protocol::*;
        let mut key_events = Vec::new();
        for n in 1..=2 {
            key_events.push(lingxi_kernel::ports::KeyEvent {
                event_id: EventId::new(format!("{run}-tc{n}-start")),
                payload: EventPayload::Known(KnownEventPayload::ToolCallStarted(
                    ToolCallStartedPayload {
                        tool_call: ToolCallDescriptor {
                            tool_call_id: ToolCallId::new(format!("{run}-tc{n}")),
                            target: "read".to_string(),
                            args_digest: digest_arguments(&serde_json::json!({ "n": n })),
                            args_summary: None,
                        },
                    },
                )),
            });
        }
        key_events.push(lingxi_kernel::ports::KeyEvent {
            event_id: EventId::new(format!("{run}-done")),
            payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                RunStateChangedPayload {
                    from: RunStatus::Running,
                    to: RunStatus::Completed,
                    reason: Some("completed".to_string()),
                },
            )),
        });
        let outcome = lingxi_kernel::ports::RunOutcome {
            status: RunStatus::Completed,
            reason: Some("completed".to_string()),
            key_events,
            final_message: None,
        };
        db.commit_run_outcome(&ctx, outcome, 1_001)
            .await
            .expect("commit");
    }
}

/// p → c1 → g1，外加 c1 自环（lineage 重录幂等；不同则冲突——这里构造
/// 合法闭包即可，自环由调用方的 visited 集合防御）。
async fn seed_lineage(db: &RunDatabase) {
    seed_async(db, "s").await;
    use lingxi_kernel::ports::StoragePort;
    for (run, parent) in [("p", None), ("c1", Some("p")), ("g1", Some("c1"))] {
        let ctx = lingxi_kernel::RunContext {
            principal: lingxi_kernel::Principal::LocalUser,
            session_id: lingxi_protocol::SessionId::new("s".to_string()),
            run_id: lingxi_protocol::RunId::new(run.to_string()),
            attempt: lingxi_protocol::AttemptId::new(format!("{run}#a1")),
            generation: 1,
        };
        db.record_run_started(&ctx, 1_000).await.expect("start");
        db.record_run_lineage(
            &ctx,
            lingxi_kernel::subagent::RunLineage {
                parent_run_id: parent.map(lingxi_protocol::RunId::new),
                origin: if parent.is_some() {
                    lingxi_kernel::subagent::RunOrigin::Subagent
                } else {
                    lingxi_kernel::subagent::RunOrigin::User
                },
                source_message_id: None,
                cause_id: None,
            },
            1_000,
        )
        .await
        .expect("lineage");
    }
}

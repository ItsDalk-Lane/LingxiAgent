//! 正式运行表、谱系和统一数据库队列下的手机投递恢复测试。
use lingxi_adapters::storage::{conversations as phone, RunDatabase, SessionRow, StoreOptions};
use lingxi_kernel::{
    ports::{RunOutcome, StoragePort},
    subagent::{RunLineage, RunOrigin},
    Principal, RunContext,
};
use lingxi_protocol::{AttemptId, RunId, RunStatus, SessionId};
use serde_json::json;
use std::path::PathBuf;

async fn fixture(tag: &str) -> (PathBuf, RunDatabase) {
    let root = std::env::temp_dir().join(format!(
        "lingxi-r07-phone-{tag}-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let db = RunDatabase::open(&root.join("runs.db"), StoreOptions::default())
        .await
        .unwrap();
    db.ensure_session_seed(
        [("sa", "a"), ("sa2", "a"), ("sb", "b"), ("sc", "c")]
            .into_iter()
            .map(|(sid, actor)| SessionRow {
                session_id: sid.into(),
                agent_id: actor.into(),
                owner_user_id: "user_local".into(),
                title: "合成会话".into(),
                created_at_unix_ms: 1,
            })
            .collect(),
    )
    .await
    .unwrap();
    (root, db)
}
async fn done(root: PathBuf, db: RunDatabase) {
    db.close().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
fn channel(id: &str, actor: &str, chain: &str, limit: u32) -> phone::PhoneDeliveryInput {
    phone::PhoneDeliveryInput {
        delivery_id: id.into(),
        cause_id: format!("cause-{id}"),
        actor_id: actor.into(),
        conversation_id: "ch_room".into(),
        conversation_type: "channel".into(),
        batch_digest: format!("batch-{id}"),
        root_chain_id: chain.into(),
        budget_limit: limit,
        dm_direction: None,
        source_run_id: None,
        source_message_id: format!("message-{id}"),
        membership_digest: "members-a-b".into(),
        owner_subject: "user_local".into(),
        input_json: json!({"messages":[{"body":"合成消息"}]}),
    }
}
fn dm(id: &str, from: &str, to: &str, chain: &str) -> phone::PhoneDeliveryInput {
    let mut input = channel(id, to, chain, 3);
    input.conversation_id = format!("dm:{from}");
    input.conversation_type = "dm".into();
    input.dm_direction = Some((from.into(), to.into()));
    input
}
async fn start(db: &RunDatabase, input: &phone::PhoneDeliveryInput, sid: &str) -> RunContext {
    let ctx = RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new(sid),
        run_id: RunId::new(format!("run-{}", input.delivery_id)),
        attempt: AttemptId::new(format!("run-{}#a1", input.delivery_id)),
        generation: 1,
    };
    db.record_run_started(&ctx, 10).await.unwrap();
    db.record_run_lineage(
        &ctx,
        RunLineage {
            parent_run_id: input.source_run_id.clone().map(RunId::new),
            origin: if input.conversation_type == "dm" {
                RunOrigin::Dm
            } else {
                RunOrigin::Channel
            },
            source_message_id: Some(input.source_message_id.clone()),
            cause_id: Some(input.delivery_id.clone()),
        },
        10,
    )
    .await
    .unwrap();
    ctx
}
async fn terminal(db: &RunDatabase, ctx: &RunContext, status: RunStatus) {
    db.commit_run_outcome(
        ctx,
        RunOutcome {
            status,
            reason: None,
            key_events: vec![],
            final_message: None,
        },
        20,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn concurrent_claims_spend_one_frozen_budget_across_restart() {
    let (root, db) = fixture("budget").await;
    let inputs = (0..5)
        .map(|i| channel(&format!("d{i}"), "a", "root", 2))
        .collect::<Vec<_>>();
    let (a, b, c, d, e) = tokio::join!(
        phone::claim_delivery(&db, &inputs[0], 100),
        phone::claim_delivery(&db, &inputs[1], 100),
        phone::claim_delivery(&db, &inputs[2], 100),
        phone::claim_delivery(&db, &inputs[3], 100),
        phone::claim_delivery(&db, &inputs[4], 100)
    );
    assert_eq!(
        [a, b, c, d, e]
            .into_iter()
            .map(|r| r.unwrap())
            .filter(|r| r.claimed)
            .count(),
        2
    );
    assert_eq!(phone::list_pending_deliveries(&db).await.unwrap().len(), 2);
    let existing = phone::list_pending_deliveries(&db)
        .await
        .unwrap()
        .remove(0)
        .input;
    assert!(
        !phone::claim_delivery(&db, &existing, 101)
            .await
            .unwrap()
            .claimed
    );
    let mut duplicate = existing.clone();
    duplicate.delivery_id = "different-id".into();
    assert_eq!(
        phone::claim_delivery(&db, &duplicate, 102)
            .await
            .unwrap()
            .delivery
            .unwrap()
            .input
            .delivery_id,
        existing.delivery_id
    );
    let mut changed = existing.clone();
    changed.input_json = json!({"changed":true});
    assert!(phone::claim_delivery(&db, &changed, 103).await.is_err());
    db.close().await.unwrap();
    let db = RunDatabase::open(&root.join("runs.db"), StoreOptions::default())
        .await
        .unwrap();
    assert_eq!(
        phone::claim_delivery(&db, &channel("later", "b", "root", 2), 200)
            .await
            .unwrap()
            .reason
            .as_deref(),
        Some("root_chain_budget_exhausted")
    );
    assert!(
        phone::claim_delivery(&db, &channel("raise", "b", "root", 3), 201)
            .await
            .is_err(),
        "重启或重投不能提高冻结预算"
    );
    done(root, db).await;
}

#[tokio::test]
async fn dm_three_rounds_directional_cooldown_and_five_minute_lock_are_persistent() {
    let (root, db) = fixture("dm").await;
    let first = dm("first", "a", "b", "pair");
    assert!(phone::claim_delivery(&db, &first, 0).await.unwrap().claimed);
    let blocked = phone::claim_delivery(&db, &dm("blocked", "a", "b", "pair"), 1)
        .await
        .unwrap();
    assert_eq!(blocked.retry_at, Some(300_000));
    phone::settle_delivery(
        &db,
        "first",
        phone::PhoneDeliveryStatus::Failed,
        Some("接受前失败".into()),
        1_000,
    )
    .await
    .unwrap();
    let blocked = phone::claim_delivery(&db, &dm("cooldown", "a", "b", "pair"), 10_999)
        .await
        .unwrap();
    assert_eq!(blocked.reason.as_deref(), Some("dm_direction_cooldown"));
    assert_eq!(blocked.retry_at, Some(11_000));
    assert!(
        phone::claim_delivery(&db, &dm("reverse", "b", "a", "pair"), 2_000)
            .await
            .unwrap()
            .claimed,
        "反方向使用独立冷却"
    );
    assert!(
        phone::claim_delivery(&db, &dm("third", "a", "b", "pair"), 11_000)
            .await
            .unwrap()
            .claimed
    );
    assert_eq!(
        phone::claim_delivery(&db, &dm("fourth", "b", "a", "pair"), 400_000)
            .await
            .unwrap()
            .reason
            .as_deref(),
        Some("root_chain_budget_exhausted")
    );
    db.close().await.unwrap();
    let db = RunDatabase::open(&root.join("runs.db"), StoreOptions::default())
        .await
        .unwrap();
    assert_eq!(
        phone::claim_delivery(&db, &dm("fifth", "a", "b", "pair"), 500_000)
            .await
            .unwrap()
            .reason
            .as_deref(),
        Some("root_chain_budget_exhausted")
    );
    let other = dm("other", "a", "b", "new-pair");
    assert!(
        phone::claim_delivery(&db, &other, 500_000)
            .await
            .unwrap()
            .claimed
    );
    assert!(
        !phone::claim_delivery(&db, &dm("held", "a", "b", "new-pair"), 799_999)
            .await
            .unwrap()
            .claimed
    );
    assert!(
        phone::claim_delivery(&db, &dm("expired", "a", "b", "new-pair"), 800_000)
            .await
            .unwrap()
            .claimed
    );
    let mut excessive = dm("excessive", "a", "b", "bad");
    excessive.budget_limit = 4;
    assert!(phone::claim_delivery(&db, &excessive, 900_000)
        .await
        .is_err());
    done(root, db).await;
}

#[tokio::test]
async fn only_actual_causal_run_can_bind_or_decide_and_each_run_decides_once() {
    let (root, db) = fixture("decision").await;
    let input = channel("real", "a", "root", 4);
    phone::claim_delivery(&db, &input, 1).await.unwrap();
    assert!(phone::bind_delivery_run(&db, "real", "sa", "fabricated", 2)
        .await
        .is_err());
    assert!(phone::record_decision(
        &db,
        "real",
        "fabricated",
        phone::PhoneDecision::Pass,
        None,
        2
    )
    .await
    .is_err());
    let ctx = start(&db, &input, "sa").await;
    let one = phone::record_decision(
        &db,
        "real",
        ctx.run_id.as_str(),
        phone::PhoneDecision::Reply,
        Some("实际回复".into()),
        11,
    )
    .await
    .unwrap();
    assert!(one.claimed);
    assert_eq!(one.delivery.run_id.as_deref(), Some(ctx.run_id.as_str()));
    assert_eq!(one.delivery.decision_at, Some(11));
    assert!(
        !phone::record_decision(
            &db,
            "real",
            ctx.run_id.as_str(),
            phone::PhoneDecision::Reply,
            Some("实际回复".into()),
            12
        )
        .await
        .unwrap()
        .claimed
    );
    assert!(phone::record_decision(
        &db,
        "real",
        ctx.run_id.as_str(),
        phone::PhoneDecision::Pass,
        None,
        13
    )
    .await
    .is_err());
    assert!(
        phone::settle_delivery(&db, "real", phone::PhoneDeliveryStatus::Completed, None, 14)
            .await
            .is_err()
    );
    terminal(&db, &ctx, RunStatus::Completed).await;
    phone::settle_delivery(&db, "real", phone::PhoneDeliveryStatus::Completed, None, 21)
        .await
        .unwrap();
    assert_eq!(
        phone::get_delivery(&db, "real")
            .await
            .unwrap()
            .unwrap()
            .decision_at,
        Some(11),
        "重放和最终结算均不改变首次决策时刻"
    );
    assert!(
        phone::settle_delivery(&db, "real", phone::PhoneDeliveryStatus::Unknown, None, 22)
            .await
            .is_err()
    );
    let other = channel("wrong", "b", "root", 4);
    phone::claim_delivery(&db, &other, 22).await.unwrap();
    let wrong = start(&db, &other, "sa2").await;
    assert!(
        phone::bind_delivery_run(&db, "wrong", "sa2", wrong.run_id.as_str(), 23)
            .await
            .is_err(),
        "真实运行的助手身份也必须一致"
    );
    done(root, db).await;
}

#[tokio::test]
async fn accepted_unbound_run_recovers_from_lineage_and_never_creates_another() {
    let (root, db) = fixture("recovery").await;
    let input = channel("accepted", "a", "root", 3);
    phone::claim_delivery(&db, &input, 1).await.unwrap();
    let ctx = start(&db, &input, "sa").await;
    terminal(&db, &ctx, RunStatus::Completed).await;
    let live = channel("live", "b", "root", 3);
    phone::claim_delivery(&db, &live, 2).await.unwrap();
    start(&db, &live, "sb").await;
    let no_run = channel("not-started", "c", "root", 3);
    phone::claim_delivery(&db, &no_run, 3).await.unwrap();
    db.close().await.unwrap();
    let db = RunDatabase::open(&root.join("runs.db"), StoreOptions::default())
        .await
        .unwrap();
    assert_eq!(phone::recover_deliveries(&db, 30).await.unwrap(), 2);
    let recovered = phone::get_delivery(&db, "accepted").await.unwrap().unwrap();
    assert_eq!(recovered.status, phone::PhoneDeliveryStatus::Completed);
    assert_eq!(recovered.run_id.as_deref(), Some(ctx.run_id.as_str()));
    assert_eq!(recovered.decision, None, "普通最终文本不是假造的显式pass");
    assert_eq!(
        phone::get_delivery(&db, "live")
            .await
            .unwrap()
            .unwrap()
            .status,
        phone::PhoneDeliveryStatus::Unknown
    );
    assert_eq!(
        phone::get_delivery(&db, "not-started")
            .await
            .unwrap()
            .unwrap()
            .status,
        phone::PhoneDeliveryStatus::Claimed
    );
    assert!(
        !phone::claim_delivery(&db, &input, 31)
            .await
            .unwrap()
            .claimed
    );
    assert_eq!(db.run_fact_summary().await.unwrap().0, 2);
    assert_eq!(
        phone::list_active_deliveries(&db, Some("root"), Some("b"))
            .await
            .unwrap()
            .len(),
        1,
        "未知但仍在运行的真实任务必须能被清理"
    );
    done(root, db).await;
}

#[tokio::test]
async fn descendants_share_root_budget_and_cancellation_scope_includes_child_runs() {
    let (root, db) = fixture("descendants").await;
    let input = channel("parent", "a", "root", 3);
    phone::claim_delivery(&db, &input, 1).await.unwrap();
    let parent = start(&db, &input, "sa").await;
    let child = RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("sa2"),
        run_id: RunId::new("child"),
        attempt: AttemptId::new("child#a1"),
        generation: 1,
    };
    db.record_run_started(&child, 12).await.unwrap();
    db.record_run_lineage(
        &child,
        RunLineage {
            parent_run_id: Some(parent.run_id.clone()),
            origin: RunOrigin::Subagent,
            source_message_id: Some("actual-tool".into()),
            cause_id: Some("actual-delegation".into()),
        },
        12,
    )
    .await
    .unwrap();
    let runs = phone::active_runs_for_chain(&db, "root").await.unwrap();
    assert_eq!(runs.len(), 2);
    assert!(runs.iter().any(|r| r.0 == "child"));
    phone::bind_delivery_run(&db, "parent", "sa", parent.run_id.as_str(), 13)
        .await
        .unwrap();
    terminal(&db, &parent, RunStatus::Completed).await;
    phone::settle_delivery(
        &db,
        "parent",
        phone::PhoneDeliveryStatus::Completed,
        None,
        21,
    )
    .await
    .unwrap();
    let active = phone::list_active_deliveries(&db, Some("root"), Some("a"))
        .await
        .unwrap();
    assert_eq!(
        active.len(),
        1,
        "根运行已结束，仍须找到有活后代的原投递供撤权清理"
    );
    assert_eq!(active[0].status, phone::PhoneDeliveryStatus::Completed);
    assert_eq!(
        phone::active_runs_for_chain(&db, "root")
            .await
            .unwrap()
            .len(),
        1
    );
    let mut escaped = channel("escape", "b", "new-root", 3);
    escaped.source_run_id = Some(child.run_id.to_string());
    assert!(phone::claim_delivery(&db, &escaped, 13).await.is_err());
    escaped.root_chain_id = "root".into();
    assert!(
        phone::claim_delivery(&db, &escaped, 14)
            .await
            .unwrap()
            .claimed
    );
    let mut nonexistent = channel("fake-source", "b", "other", 3);
    nonexistent.source_run_id = Some("missing-run".into());
    assert!(phone::claim_delivery(&db, &nonexistent, 15).await.is_err());
    done(root, db).await;
}

#[tokio::test]
async fn binding_uses_actual_owned_session_cas_expiry_and_persistent_epoch_after_reset() {
    let (root, db) = fixture("binding").await;
    assert_eq!(
        phone::phone_binding_epoch(&db, "a", "ch_room", "user_local")
            .await
            .unwrap(),
        0
    );
    let mut input = phone::PhoneBindingInput {
        actor_id: "a".into(),
        conversation_id: "ch_room".into(),
        conversation_type: "channel".into(),
        owner_subject: "user_local".into(),
        session_id: "sa".into(),
        mode: "read_only".into(),
        expected_epoch: None,
    };
    let one = phone::put_phone_binding(&db, &input, 100).await.unwrap();
    assert_eq!(one.epoch, 1);
    assert_eq!(one.expires_at, 1_800_100);
    assert!(phone::put_phone_binding(&db, &input, 101).await.is_err());
    input.expected_epoch = Some(1);
    let touched = phone::put_phone_binding(&db, &input, 200).await.unwrap();
    assert_eq!(touched.epoch, 1);
    assert_eq!(touched.last_used, 200);
    let mut wrong = input.clone();
    wrong.session_id = "sb".into();
    assert!(phone::put_phone_binding(&db, &wrong, 201).await.is_err());
    input.session_id = "sa2".into();
    let changed = phone::put_phone_binding(&db, &input, 202).await.unwrap();
    assert_eq!(changed.epoch, 2);
    assert!(
        phone::reset_phone_binding(&db, "a", "ch_room", "user_local", 1)
            .await
            .is_err()
    );
    assert!(
        phone::reset_phone_binding(&db, "a", "ch_room", "user_local", 2)
            .await
            .unwrap()
    );
    db.close().await.unwrap();
    let db = RunDatabase::open(&root.join("runs.db"), StoreOptions::default())
        .await
        .unwrap();
    assert!(phone::get_phone_binding(&db, "a", "ch_room", "user_local")
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        phone::phone_binding_epoch(&db, "a", "ch_room", "user_local")
            .await
            .unwrap(),
        2
    );
    input.expected_epoch = None;
    let new = phone::put_phone_binding(&db, &input, 300).await.unwrap();
    assert_eq!(new.epoch, 3, "reset后旧代次不能再次命中新会话");
    assert!(
        phone::reset_phone_binding(&db, "a", "ch_room", "user_local", 2)
            .await
            .is_err()
    );
    done(root, db).await;
}

#[tokio::test]
async fn message_source_precedes_file_receipt_and_is_an_immutable_real_started_call() {
    use lingxi_kernel::ports::{InvocationIntent, InvocationPhase};
    use lingxi_protocol::ToolCallId;
    let (root, db) = fixture("message-source").await;
    let source = channel("source", "a", "root", 3);
    let ctx = start(&db, &source, "sa").await;
    let call = ToolCallId::new("source-call");
    assert!(phone::bind_message_operation_source(
        &db,
        "tool-op",
        ctx.run_id.as_str(),
        call.as_str()
    )
    .await
    .is_err());
    db.record_invocation_intent(
        &ctx,
        InvocationIntent {
            journal_id: call.clone(),
            target: "tool:first-party:channel".into(),
            args_digest: "frozen-args".into(),
            args_summary: None,
            idempotency_key: None,
        },
        11,
    )
    .await
    .unwrap();
    assert!(
        phone::bind_message_operation_source(&db, "tool-op", ctx.run_id.as_str(), call.as_str())
            .await
            .is_err(),
        "prepared不是已获准派发的工具"
    );
    db.advance_invocation(&ctx, &call, InvocationPhase::Authorized, 12)
        .await
        .unwrap();
    db.advance_invocation(&ctx, &call, InvocationPhase::Started, 13)
        .await
        .unwrap();
    phone::bind_message_operation_source(&db, "tool-op", ctx.run_id.as_str(), call.as_str())
        .await
        .unwrap();
    assert!(
        phone::get_operation(&db, "tool-op")
            .await
            .unwrap()
            .is_none(),
        "来源先于文件计划，断电也不会丢因果"
    );
    assert!(
        phone::bind_message_operation_source(
            &db,
            "different-op",
            ctx.run_id.as_str(),
            call.as_str()
        )
        .await
        .is_err(),
        "同一工具不能换操作ID重建预算根"
    );
    assert!(
        phone::bind_message_operation_source(&db, "tool-op", "foreign-run", call.as_str())
            .await
            .is_err()
    );
    phone::bind_message_operation_source(&db, "tool-op", ctx.run_id.as_str(), call.as_str())
        .await
        .unwrap();
    terminal(&db, &ctx, RunStatus::Completed).await;
    assert!(phone::bind_message_operation_source(
        &db,
        "tool-op",
        ctx.run_id.as_str(),
        call.as_str()
    )
    .await
    .is_err());
    db.close().await.unwrap();
    let db = RunDatabase::open(&root.join("runs.db"), StoreOptions::default())
        .await
        .unwrap();
    assert_eq!(
        phone::message_operation_source(&db, "tool-op")
            .await
            .unwrap()
            .as_deref(),
        Some(ctx.run_id.as_str())
    );
    done(root, db).await;
}

#[tokio::test]
async fn completed_real_reply_remains_publishable_until_same_file_operation_is_applied() {
    use lingxi_adapters::conversation_files::{ConversationFiles, ConversationMutation};
    let (root, db) = fixture("unpublished").await;
    for actor in ["a", "b"] {
        std::fs::create_dir_all(root.join("agents").join(actor)).unwrap();
    }
    let input = dm("reply", "b", "a", "chain");
    phone::claim_delivery(&db, &input, 1).await.unwrap();
    let ctx = start(&db, &input, "sa").await;
    phone::record_decision(
        &db,
        "reply",
        ctx.run_id.as_str(),
        phone::PhoneDecision::Reply,
        Some("持久回复".into()),
        11,
    )
    .await
    .unwrap();
    terminal(&db, &ctx, RunStatus::Completed).await;
    phone::recover_deliveries(&db, 30).await.unwrap();
    assert_eq!(
        phone::unpublished_decisions(&db).await.unwrap().len(),
        1,
        "已完成但未写文件的决策不能被pending过滤漏掉"
    );
    let files = ConversationFiles::new(root.clone()).unwrap();
    let mutation = ConversationMutation::AppendDm {
        from: "a".into(),
        to: "b".into(),
        body: "持久回复".into(),
        timestamp: "2026-10-16 08:00:00".into(),
    };
    files
        .prepare(&db, "phone-reply-reply", &mutation, 31)
        .await
        .unwrap();
    assert_eq!(phone::unpublished_decisions(&db).await.unwrap().len(), 1);
    files.resume(&db, "phone-reply-reply", 32).await.unwrap();
    assert!(phone::unpublished_decisions(&db).await.unwrap().is_empty());
    assert_eq!(files.read_dm("b", "a").unwrap().messages.len(), 1);
    done(root, db).await;
}

#[tokio::test]
async fn delivery_binding_is_atomic_across_first_claim_reset_and_actual_unbound_run() {
    let (root, db) = fixture("atomic-binding").await;
    db.ensure_session_seed(vec![SessionRow {
        session_id: "phone-session-user-choice".into(),
        agent_id: "a".into(),
        owner_user_id: "user_local".into(),
        title: "用户自选名称".into(),
        created_at_unix_ms: 2,
    }])
    .await
    .unwrap();
    let mut first = channel("first", "a", "chain", 4);
    first.input_json =
        json!({"sessionId":"sa","bindingBaseEpoch":0,"settings":{"mode":"read_only"}});
    let mut second = first.clone();
    second.delivery_id = "second".into();
    second.cause_id = "second-cause".into();
    second.batch_digest = "second-batch".into();
    phone::claim_delivery(&db, &first, 1).await.unwrap();
    phone::claim_delivery(&db, &second, 1).await.unwrap();
    let input = phone::PhoneBindingInput {
        actor_id: "a".into(),
        conversation_id: "ch_room".into(),
        conversation_type: "channel".into(),
        owner_subject: "user_local".into(),
        session_id: "sa".into(),
        mode: "read_only".into(),
        expected_epoch: None,
    };
    let (one, two) = tokio::join!(
        phone::put_phone_binding_for_delivery(&db, &input, "first", 2),
        phone::put_phone_binding_for_delivery(&db, &input, "second", 2)
    );
    assert_eq!(one.unwrap().epoch, 1);
    assert_eq!(
        two.unwrap().epoch,
        1,
        "同时首投递复用同一期，不互相换会话或撤权"
    );
    let visible = db.list_sessions().await.unwrap();
    assert!(
        !visible.iter().any(|s| s.session_id == "sa"),
        "真实手机绑定/预留身份隐藏内部会话"
    );
    assert!(
        visible
            .iter()
            .any(|s| s.session_id == "phone-session-user-choice"),
        "用户自取相似名字不能被前缀误隐藏"
    );
    // 运行已实际受理、尚未来得及绑定投递收据；重置仍能识别这条因果事实。
    let ctx = start(&db, &first, "sa").await;
    db.record_run_state_change(
        &ctx,
        RunStatus::Running,
        RunStatus::Cancelling,
        Some("测试真实取消中间态".into()),
        19,
    )
    .await
    .unwrap();
    terminal(&db, &ctx, RunStatus::Cancelled).await;
    phone::reset_phone_binding(&db, "a", "ch_room", "user_local", 1)
        .await
        .unwrap();
    assert!(
        phone::put_phone_binding_for_delivery(&db, &input, "first", 3)
            .await
            .is_err()
    );
    assert!(
        phone::put_phone_binding_for_delivery(&db, &input, "second", 3)
            .await
            .is_err(),
        "已重置的墓碑不能让旧投递重新绑定旧会话"
    );
    assert!(phone::get_phone_binding(&db, "a", "ch_room", "user_local")
        .await
        .unwrap()
        .is_none());
    let mut fresh = channel("fresh", "a", "new-chain", 4);
    fresh.input_json =
        json!({"sessionId":"sa2","bindingBaseEpoch":1,"settings":{"mode":"read_only"}});
    phone::claim_delivery(&db, &fresh, 4).await.unwrap();
    let mut next = input.clone();
    next.session_id = "sa2".into();
    assert_eq!(
        phone::put_phone_binding_for_delivery(&db, &next, "fresh", 5)
            .await
            .unwrap()
            .epoch,
        2
    );
    let mut bad = next;
    bad.session_id = "sa".into();
    assert!(phone::put_phone_binding_for_delivery(&db, &bad, "fresh", 6)
        .await
        .is_err());
    let collab = RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("sc"),
        run_id: RunId::new("collab-user-session"),
        attempt: AttemptId::new("collab-user-session#a1"),
        generation: 1,
    };
    db.record_run_started(&collab, 6).await.unwrap();
    db.record_run_lineage(
        &collab,
        RunLineage {
            parent_run_id: Some(ctx.run_id),
            origin: RunOrigin::SessionCollab,
            source_message_id: Some("collab-tool-call".into()),
            cause_id: Some("approved-draft".into()),
        },
        6,
    )
    .await
    .unwrap();
    assert!(
        db.list_sessions()
            .await
            .unwrap()
            .iter()
            .any(|s| s.session_id == "sc"),
        "用户确认的协作会话仍是普通会话"
    );
    done(root, db).await;
}

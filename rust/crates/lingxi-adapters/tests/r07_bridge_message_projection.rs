//! 消息公开投影使用真实收件、Run 和发件回执，不将排队或未知外发伪装成送达。
use lingxi_adapters::storage::{bridge::*, bridge_reads, RunDatabase, SessionRow, StoreOptions};
use lingxi_kernel::{
    ports::{RunOutcome, StoragePort},
    subagent::{RunLineage, RunOrigin},
    Principal, RunContext,
};
use lingxi_protocol::{AttemptId, RunId, RunStatus, SessionId};
use serde_json::json;

#[tokio::test]
async fn messages_share_canonical_identity_and_preserve_inbound_and_all_delivery_states() {
    let root = std::env::temp_dir().join(format!(
        "lingxi-r07-message-projection-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let db = RunDatabase::open(&root.join("runs.db"), StoreOptions::default())
        .await
        .unwrap();
    put_connector(
        &db,
        &BridgeConnectorInput {
            id: "connector".into(),
            platform: "telegram".into(),
            studio_id: "studio".into(),
            actor_id: "agent".into(),
            owner_subject: "user_local".into(),
            owner_sender_id: Some("native-sender".into()),
            enabled: true,
            config_json: json!({"token":"PRIVATE-CREDENTIAL"}).to_string(),
        },
        None,
        1,
    )
    .await
    .unwrap();
    let input = BridgeInboxInput {
        platform: "telegram".into(),
        connector_id: "connector".into(),
        conversation_id: "raw-conversation".into(),
        conversation_kind: ConversationKind::Dm,
        message_id: "native-message".into(),
        sender_id: "native-sender".into(),
        payload_json: json!({"text":"真实入站","replyContext":{"contextToken":"PRIVATE-CONTEXT"}})
            .to_string(),
        attachments: vec![],
    };
    let inbox = claim_inbox(&db, &input, 1, 2).await.unwrap().inbox;
    let sid = inbox.session_id.as_deref().unwrap();
    let subject =
        lingxi_protocol::bridge_subject("telegram", "connector", "native-sender").unwrap();
    db.ensure_session_seed(vec![SessionRow {
        session_id: sid.into(),
        agent_id: "agent".into(),
        owner_user_id: subject.clone(),
        title: "真实会话".into(),
        created_at_unix_ms: 2,
    }])
    .await
    .unwrap();
    prepare_inbox_session(&db, &inbox.id, 1, sid, &[], 3)
        .await
        .unwrap();
    let run = RunContext {
        principal: Principal::Bridge { subject },
        session_id: SessionId::new(sid),
        run_id: RunId::new("projection-run"),
        attempt: AttemptId::new("projection-run#a1"),
        generation: 1,
    };
    db.record_run_started(&run, 4).await.unwrap();
    db.record_run_lineage(
        &run,
        RunLineage {
            parent_run_id: None,
            origin: RunOrigin::Bridge,
            source_message_id: Some(input.message_id.clone()),
            cause_id: Some(inbox.id.clone()),
        },
        4,
    )
    .await
    .unwrap();
    db.commit_run_outcome(
        &run,
        RunOutcome {
            status: RunStatus::Completed,
            reason: None,
            key_events: vec![],
            final_message: None,
        },
        5,
    )
    .await
    .unwrap();
    bind_inbox(&db, &inbox.id, 1, sid, "projection-run", 6)
        .await
        .unwrap();
    let group = BridgeInboxInput {
        conversation_kind: ConversationKind::Group,
        message_id: "native-group-message".into(),
        sender_id: "group-user".into(),
        payload_json: json!({"text":"真实群消息"}).to_string(),
        ..input.clone()
    };
    claim_inbox(&db, &group, 1, 7).await.unwrap();
    for (index, state) in ["queued", "started", "unknown", "failed", "confirmed"]
        .iter()
        .enumerate()
    {
        let now = 10 + index as u64 * 3;
        enqueue_outbox(
            &db,
            &BridgeOutboxInput {
                delivery_id: (*state).into(),
                connector_id: "connector".into(),
                connector_generation: 1,
                inbox_id: Some(inbox.id.clone()),
                manual_action_id: None,
                run_id: Some("projection-run".into()),
                loop_delivery_id: None,
                payload_json:
                    json!({"text":format!("外发{state}"),"credentials":"PRIVATE-PAYLOAD"})
                        .to_string(),
                attachments: vec![],
            },
            now,
        )
        .await
        .unwrap();
        if *state != "queued" {
            claim_outbox(&db, state, 1, now + 1).await.unwrap();
        }
        let terminal = match *state {
            "unknown" => Some(BridgeOutboxStatus::Unknown),
            "failed" => Some(BridgeOutboxStatus::Failed),
            "confirmed" => Some(BridgeOutboxStatus::Confirmed),
            _ => None,
        };
        if let Some(terminal) = terminal {
            let receipt = (*state == "confirmed").then(|| BridgePlatformReceipt {
                platform: "telegram".into(),
                message_ids: vec!["real-platform-1".into(), "real-platform-2".into()],
                raw_json: Some(json!({"contextToken":"PRIVATE-RECEIPT"}).to_string()),
            });
            settle_outbox(&db, state, terminal, receipt, None, now + 2)
                .await
                .unwrap();
        }
    }
    let rows = bridge_reads::messages(&db, "agent", 200).await.unwrap();
    assert_eq!(rows.len(), 7);
    let conversations = list_conversations(&db, "connector", 20).await.unwrap();
    let dm_key = conversation_key(
        conversations
            .iter()
            .find(|row| row.sender_id == "native-sender")
            .unwrap(),
    );
    let session_rows = bridge_reads::sessions(&db, "agent", Some("telegram"))
        .await
        .unwrap();
    assert_eq!(session_rows.len(), 2);
    assert_eq!(session_rows[0].user_id, "group-user");
    assert!(session_rows
        .windows(2)
        .all(|pair| pair[0].last_active >= pair[1].last_active));
    assert!(session_rows.iter().any(|row| row.session_key == dm_key));
    let incoming = rows.iter().find(|row| row.id == inbox.id).unwrap();
    assert_eq!(
        (&*incoming.direction, &*incoming.sender, &*incoming.agent_id),
        ("in", "native-sender", "agent")
    );
    assert_eq!(incoming.session_key, dm_key);
    assert_eq!(incoming.message_id, "native-message");
    assert!(!incoming.is_group);
    assert_eq!(incoming.ts, incoming.timestamp);
    assert!(rows
        .iter()
        .any(|row| row.is_group && row.sender == "group-user"));
    for state in ["queued", "started", "unknown", "failed", "confirmed"] {
        let row = rows.iter().find(|row| row.id == state).unwrap();
        assert_eq!(row.direction, "out");
        assert_eq!(row.sender, "agent");
        assert_eq!(row.user_id, "native-sender");
        assert_eq!(row.session_key, dm_key);
        assert_eq!(row.status, state);
        assert_eq!(row.run_id.as_deref(), Some("projection-run"));
        if state == "confirmed" {
            assert_eq!(row.message_id, "real-platform-1");
            assert_eq!(
                row.platform_message_ids,
                ["real-platform-1", "real-platform-2"]
            );
        } else {
            assert!(row.message_id.is_empty());
            assert!(row.platform_message_ids.is_empty());
        }
    }
    assert!(rows.windows(2).all(|pair| pair[0].ts >= pair[1].ts));
    assert_eq!(
        bridge_reads::messages(&db, "agent", 2).await.unwrap().len(),
        2
    );
    assert!(bridge_reads::messages(&db, "other-agent", 200)
        .await
        .unwrap()
        .is_empty());
    let public = serde_json::to_string(&rows).unwrap();
    assert!(!public.contains("PRIVATE"));
    assert!(!public.contains("contextToken"));
    assert!(!public.contains("credentials"));
    db.close().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

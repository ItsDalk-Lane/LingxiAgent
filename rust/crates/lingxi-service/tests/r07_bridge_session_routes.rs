//! 三个管理叶沿真实HTTP、身份、会话历史与文件登记；不启动平台连接或模型运行。
use lingxi_adapters::storage::bridge as store;
use lingxi_kernel::{
    ports::{RunOutcome, StoragePort},
    subagent::{RunLineage, RunOrigin},
    Principal, RunContext,
};
use lingxi_protocol::{AttemptId, ContentBlock, NormalizedMessage, RunId, RunStatus, SessionId};
use lingxi_service::runs::UserMessageRecorder;
use lingxi_service::{prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceState};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::path::PathBuf;
struct Harness {
    home: PathBuf,
    state: ServiceState,
    url: String,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn boot() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-sessionroutes-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(home.join("agents/lingxi")).unwrap();
        std::fs::write(
            home.join("agents/lingxi/config.yaml"),
            "agent:\n  name: 合成\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        let home = home.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        let state = ServiceState::bootstrap(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 1000,
            },
            &layout,
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let router = lingxi_service::build_router(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
        });
        Self {
            home,
            state,
            url,
            token,
            stop,
            server,
        }
    }
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        auth: bool,
    ) -> (reqwest::StatusCode, Value) {
        let mut request = reqwest::Client::new().request(method, format!("{}{path}", self.url));
        if auth {
            request = request.bearer_auth(&self.token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let body = response.json().await.unwrap();
        (status, body)
    }
    async fn target(
        &self,
        platform: &str,
        group: bool,
    ) -> (store::BridgeConnector, store::BridgeInbox, String) {
        let credentials = if platform == "qq" {
            json!({"appID":"synthetic","appSecret":"synthetic-secret"})
        } else {
            json!({"token":"synthetic-token"})
        };
        let (status, body) = self
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/bridge/config?agentId=lingxi",
                Some(json!({"platform":platform,"credentials":credentials,"enabled":true})),
                true,
            )
            .await;
        assert!(status.is_success(), "{body}");
        let cfg = store::list_connectors(self.state.storage())
            .await
            .unwrap()
            .into_iter()
            .find(|row| row.input.platform == platform)
            .unwrap();
        let input=store::BridgeInboxInput{platform:platform.into(),connector_id:cfg.input.id.clone(),conversation_id:format!("{platform}-known-chat"),conversation_kind:if group{store::ConversationKind::Group}else{store::ConversationKind::Dm},message_id:format!("{platform}-known-message"),sender_id:"actual-platform-sender".into(),payload_json:json!({"text":"actual inbound","displayName":"合成昵称","replyContext":{"contextToken":"synthetic-private-token"}}).to_string(),attachments:vec![]};
        let inbox = store::claim_inbox(self.state.storage(), &input, cfg.generation, 1)
            .await
            .unwrap()
            .inbox;
        let conversation = store::list_conversations(self.state.storage(), &cfg.input.id, 20)
            .await
            .unwrap()
            .remove(0);
        (cfg, inbox, store::conversation_key(&conversation))
    }
    async fn local_file(&self, bytes: &[u8]) -> String {
        if self
            .state
            .storage()
            .get_session("local-source")
            .await
            .unwrap()
            .is_none()
        {
            self.state
                .session_admin()
                .create_session(
                    "local-source",
                    "lingxi",
                    "user_local",
                    "文件来源",
                    Some("read_only"),
                    &[],
                    false,
                    1,
                )
                .await
                .unwrap();
        }
        self.state
            .session_files()
            .register_bridge_blob(
                "local-source",
                Some("actual.txt"),
                "text/plain",
                bytes,
                "user_upload",
            )
            .await
            .unwrap()
            .file_id
    }
    async fn close(self) {
        let _ = self.stop.send(());
        self.server.await.unwrap();
        self.state.storage().close().await.unwrap();
        std::fs::remove_dir_all(self.home).unwrap();
    }
}
#[tokio::test]
#[cfg(unix)]
async fn manual_send_queues_real_file_without_fake_run_and_http_retries_return_same_fact() {
    let h = Harness::boot().await;
    let (_, _, key) = h.target("telegram", false).await;
    let file = h.local_file(b"actual local file").await;
    let body = json!({"platform":"telegram","sessionKey":key,"sessionId":"local-source","fileId":file,"requestId":"stable-user-operation"});
    let (status, first) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/bridge/send-media?agentId=lingxi",
            Some(body.clone()),
            true,
        )
        .await;
    assert!(status.is_success(), "{first}");
    assert_eq!(first["status"], "queued");
    assert_eq!(first["queued"], true);
    let mut conflicting_target = body.clone();
    conflicting_target["chatId"] = json!("different-conversation");
    assert_eq!(
        h.request(
            reqwest::Method::POST,
            "/lingxi/v1/bridge/send-media?agentId=lingxi",
            Some(conflicting_target),
            true
        )
        .await
        .0,
        reqwest::StatusCode::BAD_REQUEST
    );
    let (status, replay) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/bridge/send-media?agentId=lingxi",
            Some(body.clone()),
            true,
        )
        .await;
    assert!(status.is_success(), "{replay}");
    assert_eq!(replay["deliveryId"], first["deliveryId"]);
    assert_eq!(replay["replayed"], true);
    assert_eq!(h.state.storage().total_runs().await.unwrap(), 0);
    let count = h
        .state
        .storage()
        .query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM bridge_inbox_v1", vec![])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(count, "1");
    let registered = h
        .state
        .session_files()
        .get_file("local-source", &file)
        .await
        .unwrap()
        .unwrap();
    let protected = h.home.join("private-sentinel");
    std::fs::write(&protected, b"private bytes must not be sent").unwrap();
    std::fs::remove_file(&registered.real_path).unwrap();
    std::os::unix::fs::symlink(&protected, &registered.real_path).unwrap();
    let mut swapped = body.clone();
    swapped["requestId"] = json!("swapped-file-operation");
    let (status, result) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/bridge/send-media?agentId=lingxi",
            Some(swapped),
            true,
        )
        .await;
    assert_eq!(
        status,
        reqwest::StatusCode::UNPROCESSABLE_ENTITY,
        "{result}"
    );
    assert!(
        store::get_manual_action(h.state.storage(), "bridge-manual:swapped-file-operation")
            .await
            .unwrap()
            .is_none()
    );
    std::fs::remove_file(&registered.real_path).unwrap();
    std::fs::write(&registered.real_path, b"actual local file").unwrap();
    let mut changed = body.clone();
    changed["label"] = json!("changed operation");
    assert_eq!(
        h.request(
            reqwest::Method::POST,
            "/lingxi/v1/bridge/send-media?agentId=lingxi",
            Some(changed),
            true
        )
        .await
        .0,
        reqwest::StatusCode::CONFLICT
    );
    let mut path = body.clone();
    path["filePath"] = json!("/etc/passwd");
    assert_eq!(
        h.request(
            reqwest::Method::POST,
            "/lingxi/v1/bridge/send-media?agentId=lingxi",
            Some(path),
            true
        )
        .await
        .0,
        reqwest::StatusCode::BAD_REQUEST
    );
    let delivery = first["deliveryId"].as_str().unwrap();
    store::claim_outbox(h.state.storage(), delivery, 1, 2)
        .await
        .unwrap();
    store::recover_started_outbox(h.state.storage(), 3)
        .await
        .unwrap();
    let (status, receipt) = h
        .request(
            reqwest::Method::GET,
            &format!("/lingxi/v1/bridge/deliveries/{delivery}?agentId=lingxi"),
            None,
            true,
        )
        .await;
    assert!(status.is_success());
    assert_eq!(receipt["status"], "unknown");
    assert!(receipt["receipt"].is_null());
    assert_eq!(
        h.request(
            reqwest::Method::POST,
            "/lingxi/v1/bridge/send-media?agentId=lingxi",
            Some(body),
            false
        )
        .await
        .0,
        reqwest::StatusCode::UNAUTHORIZED
    );
    h.close().await;
}
#[tokio::test]
async fn history_uses_canonical_final_and_reset_obeys_live_lease_then_starts_new_context() {
    let h = Harness::boot().await;
    let (cfg, inbox, key) = h.target("telegram", false).await;
    let sid = inbox.session_id.as_deref().unwrap();
    let subject =
        lingxi_protocol::bridge_subject("telegram", &cfg.input.id, &inbox.input.sender_id).unwrap();
    h.state
        .session_admin()
        .create_session(
            sid,
            "lingxi",
            &subject,
            "Bridge",
            Some("read_only"),
            &[],
            false,
            1,
        )
        .await
        .unwrap();
    store::prepare_inbox_session(h.state.storage(), &inbox.id, cfg.generation, sid, &[], 2)
        .await
        .unwrap();
    let ctx = RunContext {
        principal: Principal::Bridge { subject },
        session_id: SessionId::new(sid),
        run_id: RunId::new("real-history-run"),
        attempt: AttemptId::new("real-history-run#a1"),
        generation: 1,
    };
    h.state.storage().record_run_started(&ctx, 3).await.unwrap();
    let file = h
        .state
        .session_files()
        .register_bridge_blob(
            sid,
            Some("actual.txt"),
            "text/plain",
            b"history attachment",
            "user_upload",
        )
        .await
        .unwrap();
    let chat = lingxi_service::runs::ChatRunInput {
        images: vec![],
        files: vec![json!({"fileId":file.file_id,"filename":"actual.txt"})],
        client_message_id: None,
        snapshot_version: None,
        display_text: None,
        quoted_text: None,
        session_refs: vec![],
        options: Default::default(),
    };
    lingxi_service::session_tree::DbUserMessageRecorder::new(h.state.storage().clone())
        .record_user_message_with_chat(sid, "real-history-run", "", Some(&chat), 3)
        .await
        .unwrap();
    h.state
        .storage()
        .record_run_lineage(
            &ctx,
            RunLineage {
                parent_run_id: None,
                origin: RunOrigin::Bridge,
                source_message_id: Some(inbox.input.message_id.clone()),
                cause_id: Some(inbox.id.clone()),
            },
            3,
        )
        .await
        .unwrap();
    h.state
        .storage()
        .commit_run_outcome(
            &ctx,
            RunOutcome {
                status: RunStatus::Completed,
                reason: None,
                key_events: vec![],
                final_message: Some(NormalizedMessage {
                    role: "assistant".into(),
                    content: vec![ContentBlock::Text {
                        text: "真实历史回复".into(),
                    }],
                    model_call_id: None,
                }),
            },
            4,
        )
        .await
        .unwrap();
    store::bind_inbox(
        h.state.storage(),
        &inbox.id,
        cfg.generation,
        sid,
        "real-history-run",
        5,
    )
    .await
    .unwrap();
    let url = format!("/lingxi/v1/bridge/sessions/{key}/messages?agentId=lingxi");
    let (status, history) = h.request(reqwest::Method::GET, &url, None, true).await;
    assert!(status.is_success(), "{history}");
    assert!(history["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["content"] == "真实历史回复"));
    assert!(history["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["role"] == "user"
            && row["content"] == ""
            && row["hasMedia"] == true
            && row["mediaCount"] == 1));
    assert!(!history.to_string().contains("synthetic-private-token"));
    let lease = h
        .state
        .storage()
        .session_admission()
        .try_acquire(&[sid], 512)
        .unwrap();
    let reset = format!("/lingxi/v1/bridge/sessions/{key}/reset?agentId=lingxi");
    assert_eq!(
        h.request(reqwest::Method::POST, &reset, Some(json!({})), true)
            .await
            .0,
        reqwest::StatusCode::CONFLICT
    );
    drop(lease);
    let (status, result) = h
        .request(reqwest::Method::POST, &reset, Some(json!({})), true)
        .await;
    assert!(status.is_success(), "{result}");
    assert_eq!(result["epoch"], 2);
    let (_, history) = h.request(reqwest::Method::GET, &url, None, true).await;
    assert_eq!(history["messages"], json!([]));
    assert_eq!(h.state.storage().total_runs().await.unwrap(), 1);
    h.close().await;
}
#[tokio::test]
#[cfg(unix)]
async fn manual_media_accepts_safe_bytes_above_old_snapshot_cap_and_refuses_qq_group_documents() {
    let h = Harness::boot().await;
    let (_, _, key) = h.target("telegram", false).await;
    let large = vec![b'x'; 21 * 1024 * 1024];
    let file = h.local_file(&large).await;
    let body = json!({"platform":"telegram","sessionKey":key,"sessionId":"local-source","fileId":file,"requestId":"large-real-file"});
    let (status, result) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/bridge/send-media?agentId=lingxi",
            Some(body),
            true,
        )
        .await;
    assert!(status.is_success(), "{result}");
    let (_, _, qq) = h.target("qq", true).await;
    let file = h.local_file(b"QQ document").await;
    let(status,_)=h.request(reqwest::Method::POST,"/lingxi/v1/bridge/send-media?agentId=lingxi",Some(json!({"platform":"qq","sessionKey":qq,"sessionId":"local-source","fileId":file,"requestId":"qq-group-document"})),true).await;
    assert_eq!(status, reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(h.state.storage().total_runs().await.unwrap(), 0);
    h.close().await;
}

#[tokio::test]
async fn bridge_document_bytes_are_owned_and_invalid_names_mimes_or_sessions_create_no_file_fact() {
    let h = Harness::boot().await;
    h.local_file(b"existing document").await;
    let service = h.state.session_files();
    assert!(service
        .register_blob(
            "local-source",
            Some("plain.txt"),
            "text/plain",
            b"document",
            "user_upload"
        )
        .await
        .is_err());
    let registered = service
        .register_bridge_blob(
            "local-source",
            Some("plain.txt"),
            "text/plain",
            b"document",
            "user_upload",
        )
        .await
        .unwrap();
    assert_eq!(registered.kind, "document");
    assert_eq!(std::fs::read(&registered.real_path).unwrap(), b"document");
    let duplicate = service
        .register_bridge_blob(
            "local-source",
            Some("plain.txt"),
            "text/plain",
            b"document",
            "user_upload",
        )
        .await
        .unwrap();
    assert_eq!(registered.file_id, duplicate.file_id);
    for name in [
        "../escape.txt",
        "a/b.txt",
        "a\\b.txt",
        "bad\0name",
        "C:drive.txt",
    ] {
        assert!(service
            .register_bridge_blob(
                "local-source",
                Some(name),
                "text/plain",
                b"document",
                "user_upload"
            )
            .await
            .is_err());
    }
    for mime in [
        "text/plain\r\nInjected:true",
        "text",
        "text/",
        "*/plain",
        "text/plain; charset=utf-8",
    ] {
        assert!(service
            .register_bridge_blob(
                "local-source",
                Some("valid.txt"),
                mime,
                b"document",
                "user_upload"
            )
            .await
            .is_err());
    }
    assert!(service
        .register_bridge_blob(
            "missing-session",
            Some("valid.txt"),
            "text/plain",
            b"document",
            "user_upload"
        )
        .await
        .is_err());
    assert!(h
        .state
        .storage()
        .get_session("missing-session")
        .await
        .unwrap()
        .is_none());
    let oversize = vec![0; 50 * 1024 * 1024 + 1];
    assert!(service
        .register_bridge_blob(
            "local-source",
            Some("valid.txt"),
            "application/octet-stream",
            &oversize,
            "user_upload"
        )
        .await
        .is_err());
    let count = h
        .state
        .storage()
        .query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM session_files", vec![])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(count, "2");
    h.close().await;
}

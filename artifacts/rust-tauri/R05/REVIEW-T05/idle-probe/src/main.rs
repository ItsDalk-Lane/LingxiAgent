//! REVIEW-T05 C01 idle-stream leg proof (reviewer-authored, isolated):
//! the PRODUCTION default stream idle bound (60 s, R05_BASELINE §8
//! `http_idle_stream_timeout_ms`, never overridden — bootstrap does not
//! touch `with_stream_idle_timeout`) must fire on a stream that delivered
//! one delta and then went silent. The idle trip settles the CALL as a
//! retryable `upstream_unavailable` failure, so at the production defaults
//! (max_attempts = 3, backoff 500 ms/1 s, shared 300 s call budget) the RUN
//! settles `failed` after the third trip at ≈ 181.5 s — the probe asserts
//! exactly that: 3 attempts, `upstream_unavailable`, elapsed in 170..230 s.
//! Without the driver's idle arm this double would hang the run forever.
//!
//! Boundary: the provider double only produces an external response shape
//! (one text delta, then silence); every state decision is the real
//! service composition's (real storage, events, quotas, cancel tree).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    ModelTurnDelta, ProviderDescriptor, ProviderTurn, ProviderTurnResult, TurnDeltaSink,
    TurnProviderPort,
};
use lingxi_protocol::ModelCallId;
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};

/// Emits exactly one text delta, then parks forever (a stalled stream).
struct StallAfterFirstDelta;

impl TurnProviderPort for StallAfterFirstDelta {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stall.provider".to_string(),
            model: "stall.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a ModelCallId,
        _input: &'a ModelTurnInput,
        deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            let _ = deltas.emit(ModelTurnDelta::Text("first-and-only-".to_string())).await;
            // The stream goes silent forever from here.
            std::future::pending::<()>().await;
            ProviderTurnResult::of_ctx(
                &ctx_at_issue,
                ProviderTurn::Failed {
                    error: lingxi_protocol::ProtocolError::new(
                        lingxi_protocol::ErrorCode::Internal,
                        "unreachable: the pending() arm resolved".to_string(),
                        false,
                    ),
                    retryable: false,
                },
            )
        })
    }
}

fn owner_principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".to_string(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
        studio_id: None,
        server_node_id: None,
        device_id: None,
        credential_id: None,
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".to_string()],
    }
}

#[tokio::main]
async fn main() {
    let home = std::env::temp_dir().join(format!(
        "lingxi-review-t05-idle-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&home);
    let layout = prepare_layout(&home).expect("layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let state = ServiceState::bootstrap_with_deps(
        config,
        &layout,
        ServiceDeps {
            turn_provider: Some(Arc::new(StallAfterFirstDelta)),
            ..ServiceDeps::default()
        },
    )
    .await
    .expect("bootstrap");

    let started = std::time::Instant::now();
    let storage = Arc::clone(state.storage());
    // Expected settle at the PRODUCTION defaults: the 60 s idle bound trips
    // once per attempt (the call failure is retryable) — 60 + 0.5 backoff +
    // 60 + 1.0 backoff + 60 ≈ 181.5 s, then attempts (3) exhaust and the run
    // settles failed with `upstream_unavailable`. The 240 s guard only trips
    // when the idle bound did NOT fire (the double ignores the call deadline,
    // so without the driver's idle arm the run would hang forever).
    let driven = tokio::time::timeout(
        Duration::from_secs(240),
        state.sessions().execute_for(
            storage.as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            "sess_local_alpha",
            "trigger a stalled stream",
            1_790_409_600_000,
        ),
    )
    .await
    .expect("the run must settle inside the 240 s probe guard (an unsettled run means the idle bound did NOT fire)")
    .expect("execute accepted");
    let run_id = driven.run_id;
    let elapsed = started.elapsed();

    let status = state
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec![run_id.clone()],
        )
        .await
        .expect("status query")
        .expect("run row");
    let reason = state
        .storage()
        .query_one_text(
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            vec![run_id.clone()],
        )
        .await
        .expect("reason query");

    println!("RUN_ID={run_id}");
    println!("STATUS={status}");
    println!("REASON={reason:?}");
    println!("ELAPSED_MS={}", elapsed.as_millis());
    // The partial delta must have been recorded before each idle trip
    // (one text delta per attempt → 3 attempts each persist their delta).
    let delta_events = state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1",
            vec![run_id.clone()],
        )
        .await
        .expect("event count query");
    println!("DELTA_EVENTS={delta_events:?}");
    let attempts = state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM run_attempts WHERE run_id = ?1",
            vec![run_id.clone()],
        )
        .await
        .expect("attempt count query");
    println!("ATTEMPTS={attempts:?}");

    let idle_fired = status == "failed"
        && reason.as_deref() == Some("failed.provider_error")
        && attempts.as_deref() == Some("3");
    let window_ok = elapsed >= Duration::from_secs(170) && elapsed <= Duration::from_secs(230);
    println!("IDLE_BOUND_FIRED={idle_fired} WINDOW_170_230S={window_ok}");
    if idle_fired && window_ok {
        println!("VERDICT=idle_stream_bound_enforced_at_production_default");
    } else {
        println!("VERDICT=idle_stream_bound_NOT_proven");
        std::process::exit(2);
    }
    let _: PathBuf = home; // retained for evidence inspection
}

//! R04-T01 acceptance: the tool catalog and parameter contract
//! (R04-A01 / R04-A02 + the adversarial additions).
//!
//! Everything here runs against the REAL kernel
//! [`lingxi_kernel::toolcatalog::ToolRegistry`] and — for the driver-level
//! forgery case — the REAL service composition root
//! (`ServiceState::bootstrap_with_deps`) with the real run driver. The
//! only doubles are external-system stand-ins: a counting
//! [`ToolExecutorPort`] that records which TARGET it executed (never the
//! registry/policy/decision under test) and a scripted turn provider.
//!
//! Scenarios:
//! - `r04_a01_catalog_and_execution_share_one_source_of_truth`: two
//!   same-display-name tools from two different sources; discovery,
//!   description and execution each hit exactly the intended target;
//!   registration/catalog/execution association is recorded.
//! - `r04_a02_stale_catalog_generation_is_refused_until_refresh`: a
//!   generation-1 pinned view is refused with an explicit
//!   stale-catalog error after the tool advances to generation 2; zero
//!   executions under the old description; a refreshed view executes.
//! - adversarial: alias/primary permission parity; malformed schema /
//!   over-depth arguments / illegal required types never reach the
//!   executor; a FORGED args_digest through the REAL run driver is
//!   refused with zero dispatch.

use std::pin::Pin;
use std::sync::Arc;

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolExecutionResult,
    ToolExecutorPort, ToolOutcome, ToolRequest, TurnDeltaSink, TurnProviderPort,
};
use lingxi_kernel::toolcatalog::{
    Availability, CatalogPin, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
    ToolManifest, ToolOrigin, ToolRegistry, ToolTargetRef,
};
use lingxi_protocol::{
    ContentBlock, ModelCallId, NormalizedMessage, ToolCallId, ToolSchemaDocument,
};
use lingxi_service::{
    prepare_layout, ServiceConfig, ServiceDeps, ServiceState, LOCAL_OWNER_USER_ID,
};
use lingxi_service::{HomeSource, NetworkMode};
use serde_json::json;

fn budget() -> SchemaBudget {
    SchemaBudget::default()
}

fn schema_doc(schema: serde_json::Value) -> ToolSchemaDocument {
    ToolSchemaDocument {
        dialect: "json-schema/2020-12".to_string(),
        schema,
    }
}

/// A first-party "read"-shaped manifest: {path!} required, mode defaulted,
/// bounded limit. Used for BOTH sources of the contested name so the
/// description payloads are comparable.
fn read_like_manifest(origin: ToolOrigin, version: &str) -> ToolManifest {
    ToolManifest {
        origin,
        local_name: "read".to_string(),
        display_name: "Read".to_string(),
        aliases: Vec::new(),
        version: version.to_string(),
        description: "read a file (test manifest)".to_string(),
        input_schema: schema_doc(json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1},
                "mode": {"type": "string", "enum": ["stat", "text"], "default": "text"},
            },
            "required": ["path"],
            "additionalProperties": false,
        })),
        output_schema: None,
        permission: PermissionContract {
            kind: PermissionKind::Read,
            capability_base: "read.read".to_string(),
        },
        availability: Availability::Available,
        timeout_ms: Some(30_000),
        max_concurrency: Some(2),
        declared_permission: DeclaredPermission::ReadOnly,
        recovery: lingxi_kernel::invocation::ToolRecoveryCapability {
            read_only: true,
            ..lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE
        },
    }
}

/// Counting executor double (external-system stand-in only): records WHICH
/// registry target each execution served and what argument payload it
/// actually received, returning a success whose content names the target.
#[derive(Default)]
struct CountingExecutor {
    executed: std::sync::Mutex<Vec<(String, String)>>, // (target, canonical args)
}

impl CountingExecutor {
    fn targets_executed(&self) -> Vec<String> {
        self.executed
            .lock()
            .unwrap()
            .iter()
            .map(|(target, _)| target.clone())
            .collect()
    }

    fn count(&self) -> usize {
        self.executed.lock().unwrap().len()
    }

    fn payloads_of(&self, target: &str) -> Vec<String> {
        self.executed
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, _)| t == target)
            .map(|(_, args)| args.clone())
            .collect()
    }
}

impl ToolExecutorPort for CountingExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        // The double consumes the COMPLETE effective arguments — exactly
        // what the R03 digest-only shape could not give it.
        let canonical = String::from_utf8_lossy(request.arguments.canonical_bytes()).to_string();
        self.executed
            .lock()
            .unwrap()
            .push((request.target.clone(), canonical));
        let ctx_at_issue = ctx.clone();
        let outcome = ToolOutcome::success_text(format!("executed:{}", request.target));
        Box::pin(async move { ToolExecutionResult::of_ctx(&ctx_at_issue, outcome) })
    }
}

async fn run_prepared(
    executor: &CountingExecutor,
    prepared: lingxi_kernel::toolcatalog::PreparedToolCall,
) -> ToolOutcome {
    // The minimal T01 wiring: a prepared catalog call becomes the driver's
    // ToolRequest and goes through the SAME ToolExecutorPort the run
    // driver uses (T02's gateway replaces the direct call, not the shape).
    let ctx = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess_local_alpha".to_string()),
        run_id: lingxi_protocol::RunId::new("run_r04t01".to_string()),
        attempt: lingxi_protocol::AttemptId::new("run_r04t01#a1".to_string()),
        generation: prepared.catalog_generation,
    };
    let call = ToolCallId::new("run_r04t01_tc0001".to_string());
    let request = prepared.into_tool_request();
    let result = executor.execute(&ctx, &call, &request).await;
    assert!(result.fence.matches_ctx(&ctx));
    result.outcome
}

// ── R04-A01: catalog and execution share one source of truth ───────────────

#[tokio::test]
async fn r04_a01_catalog_and_execution_share_one_source_of_truth() {
    let registry = ToolRegistry::new();
    // Two sources, one visible tool name ("read"), like a first-party
    // reader next to an MCP server that also ships a "read".
    let fp = registry
        .register(
            read_like_manifest(ToolOrigin::FirstParty, "1.2.0"),
            &budget(),
        )
        .expect("first-party read registers");
    let mcp = registry
        .register(
            read_like_manifest(
                ToolOrigin::Mcp {
                    server_id: "acme".to_string(),
                },
                "0.3.1",
            ),
            &budget(),
        )
        .expect("mcp read registers (no overwrite of the first-party tool)");
    assert_ne!(fp.target_id, mcp.target_id);
    assert_eq!(fp.target_id.as_str(), "tool:first-party:read");
    assert_eq!(mcp.target_id.as_str(), "tool:mcp:acme:read");

    // Discovery: one search, TWO results, equal display names, distinct
    // target ids and sources.
    let found = registry.search("read");
    assert_eq!(found.len(), 2, "both same-name tools are discoverable");
    assert_eq!(found[0].display_name, found[1].display_name);
    assert_ne!(found[0].target_id, found[1].target_id);
    assert_eq!(found[0].source_id, "first-party");
    assert_eq!(found[1].source_id, "acme");

    // Description: each target's full description carries ITS version and
    // ITS input schema (verified by digest).
    let fp_desc = registry.describe_full(&fp.target_id).expect("describe fp");
    let mcp_desc = registry
        .describe_full(&mcp.target_id)
        .expect("describe mcp");
    assert_eq!(fp_desc.listing.version, "1.2.0");
    assert_eq!(mcp_desc.listing.version, "0.3.1");
    assert_eq!(
        fp_desc.listing.input_schema_digest, mcp_desc.listing.input_schema_digest,
        "the two manifests ship the same schema payload here; identity is the discriminator"
    );

    // Name-only call is explicitly ambiguous — never a silent pick of one
    // of the two same-named targets.
    let err = registry
        .resolve(&ToolTargetRef::ByName {
            name: "read".to_string(),
        })
        .expect_err("contested name must not resolve silently");
    assert!(matches!(
        err,
        lingxi_kernel::toolcatalog::ToolCatalogError::TargetAmbiguous { ref target_ids, .. }
            if target_ids.len() == 2
    ));

    let executor = CountingExecutor::default();
    // Execution 1: the FIRST-PARTY target, addressed by source+name.
    let prepared_fp = registry
        .prepare_invocation(
            &ToolTargetRef::BySourceAndName {
                source_id: "first-party".to_string(),
                name: "read".to_string(),
            },
            None,
            &json!({"path": "/data/alpha.txt"}),
            &budget(),
        )
        .expect("prepare first-party read");
    assert_eq!(prepared_fp.target_id, fp.target_id);
    // Default applied by the trusted boundary ("mode" defaulted).
    assert_eq!(
        prepared_fp.effective.as_value().get("mode"),
        Some(&json!("text"))
    );
    let outcome = run_prepared(&executor, prepared_fp).await;
    match &outcome {
        ToolOutcome::Success { result } => {
            let text = match result.content.first() {
                Some(ContentBlock::Text { text }) => text.clone(),
                other => panic!("expected text content, got {other:?}"),
            };
            assert_eq!(text, "executed:tool:first-party:read");
            // The structured result is CONSUMABLE content (R04-T01), not
            // a digest placeholder.
            assert!(!result.content_digest.is_empty());
        }
        other => panic!("expected success, got {other:?}"),
    }

    // Execution 2: the MCP target under the SAME visible name.
    let prepared_mcp = registry
        .prepare_invocation(
            &ToolTargetRef::BySourceAndName {
                source_id: "acme".to_string(),
                name: "read".to_string(),
            },
            None,
            &json!({"path": "/data/beta.txt"}),
            &budget(),
        )
        .expect("prepare mcp read");
    assert_eq!(prepared_mcp.target_id, mcp.target_id);
    run_prepared(&executor, prepared_mcp).await;

    // Association record: exactly two executions, each hitting exactly its
    // own target; the equal display names caused ZERO cross-routing.
    let executed = executor.targets_executed();
    assert_eq!(
        executed,
        vec![
            "tool:first-party:read".to_string(),
            "tool:mcp:acme:read".to_string()
        ],
        "discovery, description and execution are the same authority"
    );
    // Each execution received the effective arguments prepared for ITS
    // target (payload association).
    assert_eq!(
        executor.payloads_of("tool:first-party:read"),
        vec![r#"{"mode":"text","path":"/data/alpha.txt"}"#.to_string()]
    );
    assert_eq!(
        executor.payloads_of("tool:mcp:acme:read"),
        vec![r#"{"mode":"text","path":"/data/beta.txt"}"#.to_string()]
    );
}

// ── R04-A02: a stale catalog view is refused until refreshed ───────────────

#[tokio::test]
async fn r04_a02_stale_catalog_generation_is_refused_until_refresh() {
    let registry = ToolRegistry::new();
    let receipt = registry
        .register(
            read_like_manifest(ToolOrigin::FirstParty, "1.0.0"),
            &budget(),
        )
        .expect("registers at generation 1");
    // The client caches the generation-1 catalog snapshot.
    let snapshot = registry.snapshot();
    assert_eq!(snapshot.catalog_generation, receipt.catalog_generation);
    let pin = CatalogPin {
        catalog_generation: snapshot.catalog_generation,
    };

    let executor = CountingExecutor::default();
    // The pinned view prepares fine at generation 1.
    let prepared = registry
        .prepare_invocation(
            &ToolTargetRef::ByName {
                name: "read".to_string(),
            },
            Some(pin),
            &json!({"path": "/data/old-shape.txt"}),
            &budget(),
        )
        .expect("generation-1 pin prepares");
    run_prepared(&executor, prepared).await;
    assert_eq!(executor.count(), 1);

    // The tool updates to a NEW SEMANTIC: `path` becomes `uri` and the
    // mode enum changes — generation 2. The old description must never
    // silently point at this new meaning.
    let mut v2 = read_like_manifest(ToolOrigin::FirstParty, "2.0.0");
    v2.input_schema = schema_doc(json!({
        "type": "object",
        "properties": {
            "uri": {"type": "string", "minLength": 1},
            "mode": {"type": "string", "enum": ["raw", "text"], "default": "text"},
        },
        "required": ["uri"],
        "additionalProperties": false,
    }));
    let update = registry
        .update(&receipt.target_id, v2, &budget())
        .expect("updates");
    assert_eq!(update.target_generation, 2);
    assert!(update.catalog_generation > snapshot.catalog_generation);

    // The client submits its OLD request under its OLD pin: explicit
    // refusal demanding a refresh (never an execution of the new object
    // under the old description).
    let err = registry
        .prepare_invocation(
            &ToolTargetRef::ByName {
                name: "read".to_string(),
            },
            Some(pin),
            &json!({"path": "/data/old-shape.txt"}),
            &budget(),
        )
        .expect_err("stale pin must be refused");
    match &err {
        lingxi_kernel::toolcatalog::ToolCatalogError::StaleCatalog {
            held_generation,
            current_generation,
            ..
        } => {
            assert_eq!(*held_generation, snapshot.catalog_generation);
            assert_eq!(*current_generation, update.catalog_generation);
            assert!(
                err.to_string().contains("re-describe"),
                "the error demands a refresh: {err}"
            );
        }
        other => panic!("expected StaleCatalog, got {other:?}"),
    }
    // Zero executions were added by the refused submission.
    assert_eq!(
        executor.count(),
        1,
        "the refused (stale) submission executed nothing"
    );

    // The refreshed view re-describes, re-prepares the NEW shape and
    // executes it.
    let fresh = registry.snapshot();
    let prepared_v2 = registry
        .prepare_invocation(
            &ToolTargetRef::ByName {
                name: "read".to_string(),
            },
            Some(CatalogPin {
                catalog_generation: fresh.catalog_generation,
            }),
            &json!({"uri": "file:/data/new-shape.txt"}),
            &budget(),
        )
        .expect("refreshed pin prepares the new shape");
    assert_eq!(prepared_v2.version, "2.0.0");
    run_prepared(&executor, prepared_v2).await;
    assert_eq!(executor.count(), 2);
    assert_eq!(
        executor.payloads_of(receipt.target_id.as_str()).len(),
        2,
        "the execution association record covers both generations"
    );
}

// ── adversarial additions (R04-T01 dispatch) ────────────────────────────────

/// Aliases and the primary name must produce the SAME permission contract
/// (an alias is another route to ONE authority, not a second identity).
#[tokio::test]
async fn adversarial_alias_and_primary_name_share_one_permission_contract() {
    let registry = ToolRegistry::new();
    let mut manifest = read_like_manifest(ToolOrigin::FirstParty, "1.0.0");
    manifest.local_name = "web_search".to_string();
    manifest.aliases = vec!["search_web".to_string()];
    manifest.permission = PermissionContract {
        kind: PermissionKind::Execute,
        capability_base: "web_search.execute".to_string(),
    };
    let receipt = registry.register(manifest, &budget()).expect("registers");

    let by_primary = registry
        .prepare_invocation(
            &ToolTargetRef::ByName {
                name: "web_search".to_string(),
            },
            None,
            &json!({"path": "q"}),
            &budget(),
        )
        .expect("primary prepares");
    let by_alias = registry
        .prepare_invocation(
            &ToolTargetRef::ByName {
                name: "search_web".to_string(),
            },
            None,
            &json!({"path": "q"}),
            &budget(),
        )
        .expect("alias prepares");
    // Same authority: identity, permission contract AND digest inputs
    // agree; an alias can never widen permissions.
    assert_eq!(by_primary.target_id, receipt.target_id);
    assert_eq!(by_alias.target_id, receipt.target_id);
    assert_eq!(by_primary.permission, by_alias.permission);
    assert_eq!(by_primary.args_digest.hex, by_alias.args_digest.hex);
    assert_eq!(by_primary.recovery, by_alias.recovery);
}

/// Malformed schemas, over-depth arguments and illegal required types are
/// refused BEFORE any execution exists to trigger — the counting executor
/// stays at zero across the whole battery.
#[tokio::test]
async fn adversarial_malformed_inputs_never_reach_the_executor() {
    let registry = ToolRegistry::new();
    registry
        .register(
            read_like_manifest(ToolOrigin::FirstParty, "1.0.0"),
            &budget(),
        )
        .expect("registers");
    let executor = CountingExecutor::default();

    // (a) malformed schema at REGISTRATION time: $ref must be refused and
    // the tool must NOT become discoverable afterwards (no half-registered
    // state with side effects).
    let mut ref_manifest = read_like_manifest(ToolOrigin::FirstParty, "9.9.9");
    ref_manifest.local_name = "ref_reader".to_string();
    ref_manifest.input_schema = schema_doc(json!({
        "type": "object",
        "properties": {"a": {"$ref": "https://evil.example/schema.json"}}
    }));
    let err = registry
        .register(ref_manifest, &budget())
        .expect_err("$ref refused");
    assert_eq!(
        err.code(),
        "schema_reference_unsupported",
        "remote reference resolution is never implicit: {err}"
    );
    assert!(
        registry.search("ref_reader").is_empty(),
        "a refused registration leaves no discoverable trace"
    );

    // (b) overly deep arguments: refused at prepare; nothing to execute.
    let mut deep = json!({"leaf": true});
    for level in 0..24 {
        deep = json!({format!("l{level}"): deep});
    }
    let err = registry
        .prepare_invocation(
            &ToolTargetRef::ByName {
                name: "read".to_string(),
            },
            None,
            &deep,
            &budget(),
        )
        .expect_err("deep args refused");
    assert_eq!(err.code(), "arguments_budget_exceeded");

    // (c) illegal type for a required property.
    let err = registry
        .prepare_invocation(
            &ToolTargetRef::ByName {
                name: "read".to_string(),
            },
            None,
            &json!({"path": 42}),
            &budget(),
        )
        .expect_err("illegal required type refused");
    assert_eq!(err.code(), "arguments_invalid");
    assert!(
        err.to_string().contains("expected type"),
        "the violation is named: {err}"
    );

    // (d) a float payload: rejected for cross-language canonical parity
    // (RR-T02-F2 consumption-point ruling).
    let err = registry
        .prepare_invocation(
            &ToolTargetRef::ByName {
                name: "read".to_string(),
            },
            None,
            &json!({"path": "x", "mode": "text", "extra_float": serde_json::json!(0.5)}),
            &budget(),
        )
        // additionalProperties:false catches the unknown key first — either
        // refusal is a zero-dispatch outcome; assert one of them fired.
        .map(|_| panic!("float payload must not prepare"))
        .unwrap_err();
    assert!(matches!(
        err.code(),
        "arguments_invalid" | "arguments_not_safe_integer"
    ));

    // The whole battery executed NOTHING.
    assert_eq!(executor.count(), 0);

    // Control (the legal counterpart — refusing everything is not success):
    // a VALID preparation executes exactly once.
    let prepared = registry
        .prepare_invocation(
            &ToolTargetRef::ByName {
                name: "read".to_string(),
            },
            None,
            &json!({"path": "/data/ok.txt"}),
            &budget(),
        )
        .expect("valid args prepare");
    run_prepared(&executor, prepared).await;
    assert_eq!(executor.count(), 1);
}

/// A FORGED args_digest (digest of arguments A, effective payload B)
/// through the REAL run driver and composition root: refused with zero
/// dispatch, journaled as a never-dispatched failure under the TRUSTED
/// digest, and the run completes with the tool-failure vocabulary.
#[tokio::test]
async fn adversarial_forged_digest_is_refused_with_zero_dispatch_on_real_chain() {
    // Control executor: counts real dispatches.
    #[derive(Default)]
    struct Gate {
        calls: std::sync::Mutex<Vec<String>>,
    }
    impl ToolExecutorPort for Gate {
        fn execute<'a>(
            &'a self,
            ctx: &'a lingxi_kernel::RunContext,
            _call: &'a ToolCallId,
            request: &'a ToolRequest,
        ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
            self.calls.lock().unwrap().push(request.target.clone());
            let ctx_at_issue = ctx.clone();
            let outcome = ToolOutcome::success_text("real execution");
            Box::pin(async move { ToolExecutionResult::of_ctx(&ctx_at_issue, outcome) })
        }
    }

    struct ForgedDigestProvider {
        forged: std::sync::Mutex<Option<ToolRequest>>,
        observed_targets: std::sync::Mutex<Vec<String>>,
    }
    impl TurnProviderPort for ForgedDigestProvider {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                provider: "stub.provider".to_string(),
                model: "stub.model".to_string(),
                operation: "chat".to_string(),
            }
        }
        fn next_turn<'a>(
            &'a self,
            ctx: &'a lingxi_kernel::RunContext,
            _call: &'a ModelCallId,
            _input: &'a ModelTurnInput,

            _deltas: &'a dyn TurnDeltaSink,
        ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
            let ctx_at_issue = ctx.clone();
            let next = match self.forged.lock().unwrap().take() {
                Some(request) => {
                    self.observed_targets
                        .lock()
                        .unwrap()
                        .push(request.target.clone());
                    ProviderTurn::ToolRequests {
                        content: Vec::new(),
                        requests: vec![request],
                    }
                }
                None => ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".to_string(),
                        content: vec![ContentBlock::Text {
                            text: "done after tools".to_string(),
                        }],
                        model_call_id: None,
                    },
                },
            };
            Box::pin(async move { ProviderTurnResult::of_ctx(&ctx_at_issue, next) })
        }
    }

    fn forged_request(forged: bool) -> ToolRequest {
        let mut request = ToolRequest::from_effective_arguments(
            "read",
            json!({"path": "/tmp/effective-payload.txt"}),
            &budget(),
        )
        .expect("effective request");
        if forged {
            // Digest of DIFFERENT arguments smuggled in (the model-side
            // forgery shape: "approved digest" + real payload B).
            request.args_digest = lingxi_protocol::digest_arguments(&json!({
                "path": "/tmp/approved-looking-payload.txt"
            }));
            assert!(
                !request.digest_matches_arguments(),
                "fixture must be forged"
            );
        }
        request
    }

    let home = std::env::temp_dir().join(format!(
        "lingxi-r04t01-forged-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&home);
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let layout = prepare_layout(&home).expect("layout");

    // ── Case 1: the FORGED request ──
    let gate = Arc::new(Gate::default());
    let provider = Arc::new(ForgedDigestProvider {
        forged: std::sync::Mutex::new(Some(forged_request(true))),
        observed_targets: std::sync::Mutex::new(Vec::new()),
    });
    let deps = ServiceDeps {
        turn_provider: Some(provider.clone()),
        tool_executor: Some(gate.clone()),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config.clone(), &layout, deps)
        .await
        .expect("bootstrap");
    let run_id = state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            "sess_local_alpha",
            "run with a forged digest",
            1_790_409_600_000,
        )
        .await
        .expect("execute accepted")
        .run_id;
    // ZERO dispatches: the executor never saw the call.
    assert_eq!(
        gate.calls.lock().unwrap().len(),
        0,
        "a forged args_digest must produce zero dispatches"
    );
    // The journal closed the invocation as a never-dispatched failure and
    // bound the TRUSTED digest (computed from the effective arguments).
    let journal = state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.clone()))
        .await
        .expect("journal read");
    assert_eq!(journal.len(), 1, "the refused call is journaled");
    let entry = &journal[0];
    assert_eq!(entry.target, "read");
    let trusted = lingxi_protocol::digest_arguments(&json!({
        "path": "/tmp/effective-payload.txt"
    }));
    assert_eq!(
        entry.args_digest, trusted.hex,
        "the journal binds the digest computed from the arguments, not the forged one"
    );
    assert_eq!(
        entry.phase.wire_name(),
        "failed",
        "the refusal is a closed failure receipt"
    );
    let receipt = entry.receipt.as_ref().expect("receipt present");
    assert!(!receipt.dispatched, "the forged call was never dispatched");
    assert!(
        receipt.detail.contains("args_digest mismatch"),
        "the refusal reason is diagnosable: {}",
        receipt.detail
    );
    // The run itself COMPLETED (the scripted final turn follows the
    // refusal — the tool failure is recorded as a durable fact, and the
    // no-final vocabulary would apply only if no final arrived).
    let status = state
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec![run_id.clone()],
        )
        .await
        .expect("status query")
        .expect("run row");
    assert_eq!(status, "completed");
    let reason = state
        .storage()
        .query_one_text(
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            vec![run_id.clone()],
        )
        .await
        .expect("reason query")
        .expect("terminal reason");
    assert_eq!(reason, "completed.with_final");
    state.storage().close().await.expect("close");

    // ── Case 2: the honest control on the SAME chain ──
    let gate = Arc::new(Gate::default());
    let provider = Arc::new(ForgedDigestProvider {
        forged: std::sync::Mutex::new(Some(forged_request(false))),
        observed_targets: std::sync::Mutex::new(Vec::new()),
    });
    let deps = ServiceDeps {
        turn_provider: Some(provider.clone()),
        tool_executor: Some(gate.clone()),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    let run_id = state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            "sess_local_alpha",
            "run with an honest digest",
            1_790_409_600_000,
        )
        .await
        .expect("execute accepted")
        .run_id;
    assert_eq!(
        gate.calls.lock().unwrap().len(),
        1,
        "the control (consistent digest) executes exactly once"
    );
    let journal = state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.clone()))
        .await
        .expect("journal read");
    assert_eq!(journal[0].phase.wire_name(), "succeeded");
    assert!(journal[0].receipt.as_ref().expect("receipt").dispatched);
    state.storage().close().await.expect("close");

    // The provider double observed exactly the tool turn it emitted.
    assert_eq!(
        provider.observed_targets.lock().unwrap().clone(),
        vec!["read".to_string()]
    );
    let _ = std::fs::remove_dir_all(&home);
}

fn owner_principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".to_string(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(LOCAL_OWNER_USER_ID.to_string()),
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

/// Registry-backed recovery capabilities (the R03 handoff's deferred
/// resolution): a verified read-only target recovers automatically, an
/// UNKNOWN target stays conservative.
#[tokio::test]
async fn registry_capabilities_resolve_recovery_classification() {
    use lingxi_service::invocations::{RecoveryCapabilitySource, RegistryCapabilities};

    let registry = Arc::new(ToolRegistry::new());
    registry
        .register(
            read_like_manifest(ToolOrigin::FirstParty, "1.0.0"),
            &budget(),
        )
        .expect("registers");
    let source = RegistryCapabilities::new(Arc::clone(&registry));
    let read_cap = source.capability_of("read");
    assert!(
        read_cap.read_only,
        "the manifest's VERIFIED recovery fact resolves"
    );
    let unknown = source.capability_of("send_message");
    assert_eq!(
        unknown,
        lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
        "an unregistered target stays conservative (never optimistic)"
    );
    // A source that DECLARES read-only without a verified grant stays
    // conservative — declared_permission is not a grant.
    let mut claimer = read_like_manifest(
        ToolOrigin::Mcp {
            server_id: "acme".to_string(),
        },
        "1.0.0",
    );
    claimer.local_name = "claims_readonly".to_string();
    claimer.declared_permission = DeclaredPermission::ReadOnly;
    claimer.recovery = lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE;
    registry.register(claimer, &budget()).expect("registers");
    assert!(!source.capability_of("claims_readonly").read_only);
}

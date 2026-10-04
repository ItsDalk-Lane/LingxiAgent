//! R05-T02 independent-review probes (REVIEWER-R05-T02, run R01).
//!
//! These probes live OUTSIDE the product tree and exercise the production
//! credential/redaction code through its public seams:
//! - C09: novel synthetic secret forms the implementer's fixtures never used
//!   (standard base64 with literal `/` and `=` padding, split-escape shapes).
//! - C02/A03: a 6-way concurrent 401 merge (implementer tested 2).
//! - C04: ALL waiters of a shared refresh cancelled (implementer tested
//!   one-of-two) — the spawned flight must still land its minted token.
//! - C05/A04: revoke mid-flight, late token released, restart recovers the
//!   revocation.
//! - C06: injected persist failure — honest `persisted:false`, old state
//!   recoverable after a restart.
//! - resolve-loop guard: a token endpoint that keeps minting ALREADY-EXPIRED
//!   tokens — does `resolve` bound its refresh loop? (bounded observation:
//!   3s window, count driver calls.)

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_adapters::models::config::{ModelPlaneConfig, OAuthFlowConfig};
use lingxi_adapters::models::credentials::{
    scrub_materials, ApplicableAuth, CredentialError, ProviderCredentialPort as _, RefreshVerdict,
};
use lingxi_adapters::models::oauth::{OAuthError, OAuthTokens};
use lingxi_kernel::model_exchange::{
    CredentialAuthKind, CredentialReference, ModelOperation, ProtocolFamily, ResolvedModelRoute,
};
use lingxi_service::credentials::store::{CredentialStore, StoreIo};
use lingxi_service::credentials::{CredentialService, RefreshDriver};
use lingxi_service::inject::{ManualClock, ServiceClock};
use lingxi_service::redaction::redact_line;

// ── shared fixtures ─────────────────────────────────────────────────────────

struct ScriptedRefresh {
    calls: Mutex<Vec<String>>,
    outcome: Mutex<Result<OAuthTokens, OAuthError>>,
    gate: Option<Arc<tokio::sync::Notify>>,
}

impl ScriptedRefresh {
    fn ok(access: &str, expires_at_unix_ms: u64) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            outcome: Mutex::new(Ok(OAuthTokens {
                access_token: access.to_string(),
                refresh_token: "rt-next".to_string(),
                expires_at_unix_ms,
            })),
            gate: None,
        }
    }
    fn gated(access: &str, gate: Arc<tokio::sync::Notify>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            outcome: Mutex::new(Ok(OAuthTokens {
                access_token: access.to_string(),
                refresh_token: "rt-late".to_string(),
                expires_at_unix_ms: u64::MAX,
            })),
            gate: Some(gate),
        }
    }
    fn count(&self) -> usize {
        self.calls.lock().expect("calls").len()
    }
}

impl RefreshDriver for ScriptedRefresh {
    fn refresh<'a>(
        &'a self,
        provider: &'a str,
        _flow: &'a OAuthFlowConfig,
        refresh_token: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<OAuthTokens, OAuthError>> + Send + 'a>,
    > {
        self.calls
            .lock()
            .expect("calls")
            .push(format!("{provider}:{refresh_token}"));
        let gate = self.gate.clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.notified().await;
            }
            self.outcome.lock().expect("outcome").clone()
        })
    }
}

struct MemoryStoreIo {
    content: Mutex<Option<String>>,
    fail_writes: std::sync::atomic::AtomicBool,
}

impl MemoryStoreIo {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            content: Mutex::new(None),
            fail_writes: std::sync::atomic::AtomicBool::new(false),
        })
    }
    fn content(&self) -> Option<String> {
        self.content.lock().expect("mem").clone()
    }
}

impl StoreIo for MemoryStoreIo {
    fn read(&self) -> Result<Option<String>, String> {
        Ok(self.content())
    }
    fn write(&self, content: &str) -> Result<(), String> {
        if self.fail_writes.load(Ordering::SeqCst) {
            return Err("REVIEW-INJECTED write failure".to_string());
        }
        *self.content.lock().expect("mem") = Some(content.to_string());
        Ok(())
    }
}

fn oauth_plane() -> ModelPlaneConfig {
    ModelPlaneConfig::parse_and_validate(
        r#"{
            "providers": {
                "main": {
                    "protocol": "openai-completions",
                    "endpoint": "http://127.0.0.1:9/v1",
                    "auth": {"kind": "oauth", "flow": "deviceCode",
                        "clientId": "client-review",
                        "tokenEndpoint": "http://127.0.0.1:9/token",
                        "deviceAuthorizationEndpoint": "http://127.0.0.1:9/device"}
                }
            },
            "models": {"chat": {"provider": "main", "model": "m-1"}}
        }"#,
    )
    .expect("valid plane")
}

fn route_for(provider: &str) -> ResolvedModelRoute {
    ResolvedModelRoute {
        provider: provider.to_string(),
        model: "m-1".to_string(),
        operation: ModelOperation::Chat,
        protocol: ProtocolFamily::OpenAiCompletions,
        endpoint: "http://127.0.0.1:9/v1".to_string(),
        credential: CredentialReference {
            provider: provider.to_string(),
            auth: CredentialAuthKind::OAuth,
        },
        config_generation: 1,
    }
}

fn seeded_store(io: Arc<MemoryStoreIo>, access: &str, refresh: &str, expiry: u64) -> CredentialStore {
    let store = CredentialStore::load(io).expect("load");
    store
        .put_tokens(
            "main",
            &OAuthTokens {
                access_token: access.to_string(),
                refresh_token: refresh.to_string(),
                expires_at_unix_ms: expiry,
            },
        )
        .expect("seed");
    store
}

fn service_with(
    driver: Arc<ScriptedRefresh>,
    store: Option<CredentialStore>,
    clock: Arc<dyn ServiceClock>,
) -> CredentialService {
    CredentialService::new(&oauth_plane(), store, driver, clock)
}

// ── C09: novel secret forms through the pattern redactor + exact scrub ─────

#[test]
fn probe_c09_novel_base64_secret_forms_never_survive_redaction() {
    // A form the implementer's fixtures NEVER used: 48-char standard base64
    // with a literal `/` and `=` padding.
    let secret = "RvwXQ7+k/3mQ2pLmN8+vPqRsTuVwXyZaBcDeFgHiJ=";
    assert_eq!(secret.len(), 42);
    assert!(secret.len() >= 40 && secret.contains('/') && secret.contains('='));

    let cases = [
        format!("provider echo leaked {secret} verbatim"),           // bare
        format!(r#"config apiKey = "{secret}" status=401"#),        // keyed, exact case
        format!(r#"cfg aPiKeY: "{secret}""#),                       // keyed, mixed case
        format!("Authorization: Bearer {secret}"),                  // header
        format!("GET https://api.example.test/v1/x?token={secret}&n=1 failed"), // query
        format!("body={secret} trailing"),                          // glued to '='
    ];
    for line in cases {
        let red = redact_line(&line);
        assert!(!red.contains(secret), "LEAK in: {line}\nredacted: {red}");
        assert_ne!(red, line, "vacuous pass (no redaction happened): {line}");
    }

    // The split-escape shape: `/` positioned so the right half is < 40
    // chars — the pre-fix is_token_char (no `/`/`=`) would have scanned two
    // sub-40 runs and let BOTH halves survive.
    let split = "abcdefghijklmnopqrstuvwxyz0123456789AB/CDEFGH=="; // 38 + 1 + 8 = 47
    assert_eq!(split.len(), 47);
    let left = &split[..38];
    let right = &split[39..];
    assert!(left.len() < 40 && right.len() < 40, "BOTH halves are sub-40");
    let red = redact_line(&format!("echo {split} done"));
    assert!(!red.contains(split), "split-escape token leaked: {red}");
    assert!(!red.contains(right), "right half survived: {red}");

    // Over-redaction control: an ordinary long sentence must pass through.
    let benign = "run completed after a perfectly ordinary long diagnostic line";
    assert_eq!(redact_line(benign), benign);

    // The exact-match scrub (adapter primary defense) with a marker form of
    // MY choosing (contains `/` and `=`).
    let marker = "mY+Mar/ker=Form==9x";
    let echo = format!("upstream says: bad grant {marker} sorry");
    let scrubbed = scrub_materials(&echo, &[marker]);
    assert!(!scrubbed.contains(marker));
    assert!(scrubbed.contains("[redacted]"));
    println!("PROBE c09 novel secret forms: PASS");
}

// ── C02/A03: six concurrent 401 reporters merge into exactly ONE refresh ───

#[tokio::test]
async fn probe_c02_six_way_concurrent_401_merge() {
    let clock = Arc::new(ManualClock::new(1_000_000));
    let gate = Arc::new(tokio::sync::Notify::new());
    let driver = Arc::new(ScriptedRefresh::gated("at-fresh", gate.clone()));
    let io = MemoryStoreIo::new();
    let store = seeded_store(io.clone(), "at-old", "rt-1", u64::MAX);
    let service = service_with(driver.clone(), Some(store), clock);
    let route = route_for("main");
    let used = ApplicableAuth::Bearer("at-old".to_string());

    let mut waiters = Vec::new();
    for _ in 0..6 {
        let service = service.clone();
        let route = route.clone();
        let used = used.clone();
        waiters.push(tokio::spawn(async move {
            service.report_unauthorized(&route, &used).await
        }));
    }
    // Wait until the flight actually started (one driver call recorded).
    tokio::time::timeout(Duration::from_secs(5), async {
        while driver.count() < 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("flight started");
    tokio::task::yield_now().await;
    gate.notify_waiters();

    for waiter in waiters {
        let verdict = waiter.await.expect("joined").expect("verdict");
        assert!(
            matches!(verdict, RefreshVerdict::Refreshed { persisted: true, .. }),
            "every waiter refreshed: {verdict:?}"
        );
    }
    assert_eq!(driver.count(), 1, "SIX concurrent 401s → exactly ONE refresh");
    // The store holds the rotated set.
    let on_disk: serde_json::Value =
        serde_json::from_str(&io.content().expect("store written")).expect("json");
    assert_eq!(on_disk["providers"]["main"]["tokens"]["accessToken"], "at-fresh");
    println!("PROBE c02 six-way merge: PASS (driver calls = 1)");
}

// ── C04: ALL waiters cancelled — the spawned flight still lands ─────────────

#[tokio::test]
async fn probe_c04_all_waiters_cancelled_flight_still_lands() {
    let clock = Arc::new(ManualClock::new(1_000_000));
    let gate = Arc::new(tokio::sync::Notify::new());
    let driver = Arc::new(ScriptedRefresh::gated("at-fresh", gate.clone()));
    let io = MemoryStoreIo::new();
    let store = seeded_store(io.clone(), "at-old", "rt-1", u64::MAX);
    let service = service_with(driver.clone(), Some(store.clone()), clock);
    let route = route_for("main");
    let used = ApplicableAuth::Bearer("at-old".to_string());

    let w1 = tokio::spawn({
        let (service, route, used) = (service.clone(), route.clone(), used.clone());
        async move { service.report_unauthorized(&route, &used).await }
    });
    let w2 = tokio::spawn({
        let (service, route, used) = (service.clone(), route.clone(), used.clone());
        async move { service.report_unauthorized(&route, &used).await }
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while driver.count() < 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("flight started");
    tokio::task::yield_now().await;
    // Cancel BOTH waiters mid-flight.
    w1.abort();
    w2.abort();
    let _ = w1.await;
    let _ = w2.await;
    // Now release the driver: NO waiter remains.
    gate.notify_waiters();
    // The flight must still complete and install the minted token.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(ApplicableAuth::Bearer(token)) = service.resolve(&route).await {
                if token == "at-fresh" {
                    break;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the flight landed its token without any surviving waiter");
    assert_eq!(
        driver.count(),
        1,
        "no SECOND refresh was started by the post-cancel resolve"
    );
    assert_eq!(
        store.tokens_for("main").expect("stored").access_token,
        "at-fresh",
        "the minted token persisted even though every waiter was cancelled"
    );
    println!("PROBE c04 all-waiters-cancelled: PASS (token landed, 1 driver call)");
}

// ── C05/A04: revoke mid-flight fences the late token; restart keeps it dead ─

#[tokio::test]
async fn probe_c05_revoke_midflight_fence_and_restart() {
    let clock = Arc::new(ManualClock::new(1_000_000));
    let gate = Arc::new(tokio::sync::Notify::new());
    let driver = Arc::new(ScriptedRefresh::gated("at-late", gate.clone()));
    let io = MemoryStoreIo::new();
    let store = seeded_store(io.clone(), "at-old", "rt-1", u64::MAX);
    let service = service_with(driver.clone(), Some(store.clone()), clock);
    let route = route_for("main");
    let used = ApplicableAuth::Bearer("at-old".to_string());

    let waiter = tokio::spawn({
        let (service, route, used) = (service.clone(), route.clone(), used.clone());
        async move { service.report_unauthorized(&route, &used).await }
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while driver.count() < 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("flight started");
    tokio::task::yield_now().await;
    service.revoke("main").await.expect("revoke");
    gate.notify_waiters();
    let verdict = waiter.await.expect("joined").expect("verdict");
    assert!(
        matches!(verdict, RefreshVerdict::Revoked),
        "the waiter learns the revocation: {verdict:?}"
    );
    // No resurrection in memory…
    assert!(matches!(
        service.resolve(&route).await,
        Err(CredentialError::Revoked { .. })
    ));
    // …and a NEW 401 report against the old token does NOT restart a refresh.
    let late = service.report_unauthorized(&route, &used).await.expect("verdict");
    assert!(matches!(late, RefreshVerdict::Revoked));
    assert_eq!(driver.count(), 1, "no refresh after the revocation");
    // …nor on disk.
    let on_disk: serde_json::Value =
        serde_json::from_str(&io.content().expect("store written")).expect("json");
    assert!(on_disk["providers"].get("main").is_none(), "row deleted: {on_disk}");
    assert!(!io.content().unwrap().contains("at-late"));
    // A restart (fresh service over the same store) is not-logged-in.
    let clock2 = Arc::new(ManualClock::new(1_000_000));
    let driver2 = Arc::new(ScriptedRefresh::ok("at-never", u64::MAX));
    let store2 = CredentialStore::load(io.clone()).expect("reload");
    let restarted = service_with(driver2.clone(), Some(store2), clock2);
    assert!(
        matches!(
            restarted.resolve(&route).await,
            Err(CredentialError::NotLoggedIn { .. })
        ),
        "the revocation survives the restart"
    );
    assert_eq!(driver2.count(), 0, "no refresh attempt after restart");
    println!("PROBE c05 revoke fence + restart: PASS");
}

// ── C06: injected persist failure — honest, old state recoverable ───────────

#[tokio::test]
async fn probe_c06_persist_failure_is_honest_and_recoverable() {
    let clock = Arc::new(ManualClock::new(1_000_000));
    let driver = Arc::new(ScriptedRefresh::ok("at-fresh", u64::MAX));
    let io = MemoryStoreIo::new();
    let store = seeded_store(io.clone(), "at-old", "rt-1", u64::MAX);
    io.fail_writes.store(true, Ordering::SeqCst);
    let service = service_with(driver.clone(), Some(store), clock);
    let route = route_for("main");
    let used = ApplicableAuth::Bearer("at-old".to_string());

    let verdict = service
        .report_unauthorized(&route, &used)
        .await
        .expect("verdict");
    let RefreshVerdict::Refreshed { persisted, .. } = verdict else {
        panic!("expected Refreshed, got {verdict:?}");
    };
    assert!(!persisted, "a failed persist is NEVER reported as persisted");
    // The status surface says so, too.
    let status = service.status().await;
    let main = status.iter().find(|p| p.provider == "main").expect("main");
    assert!(!main.persisted);
    assert!(
        main.last_persist_failure
            .as_deref()
            .unwrap_or_default()
            .contains("REVIEW-INJECTED"),
        "the failure is named: {main:?}"
    );
    // Memory serves the minted token; the DISK still holds the old set.
    assert_eq!(
        service.resolve(&route).await.expect("resolves"),
        ApplicableAuth::Bearer("at-fresh".to_string())
    );
    let on_disk: serde_json::Value =
        serde_json::from_str(&io.content().expect("store content")).expect("json");
    assert_eq!(on_disk["providers"]["main"]["tokens"]["accessToken"], "at-old");
    // A restart recovers the OLD state (and would refresh it again).
    let store2 = CredentialStore::load(io.clone()).expect("reload");
    assert_eq!(
        store2.tokens_for("main").expect("old row").access_token,
        "at-old"
    );
    println!("PROBE c06 persist-failure honesty: PASS");
}

// ── resolve-loop guard: perpetually-expired mints ───────────────────────────

#[tokio::test]
async fn probe_resolve_refreshes_again_when_mints_are_still_expired() {
    // The driver mints tokens that are ALREADY expired. Does `resolve`
    // bound its refresh loop, or does it keep burning refresh grants?
    let clock = Arc::new(ManualClock::new(1_000_000));
    let driver = Arc::new(ScriptedRefresh::ok("at-still-expired", 1)); // expiry = 1970
    let io = MemoryStoreIo::new();
    let store = seeded_store(io.clone(), "at-old", "rt-1", 1); // expired seed
    let service = service_with(driver.clone(), Some(store), clock);
    let route = route_for("main");

    let calls = Arc::new(AtomicUsize::new(0));
    let probe = tokio::spawn({
        let service = service.clone();
        let route = route.clone();
        let calls = Arc::clone(&calls);
        async move {
            let outcome =
                tokio::time::timeout(Duration::from_secs(3), service.resolve(&route)).await;
            calls.store(driver.count(), Ordering::SeqCst);
            outcome
        }
    });
    let outcome = probe.await.expect("probe task");
    let observed_calls = calls.load(Ordering::SeqCst);
    match &outcome {
        Ok(Ok(_)) => println!(
            "PROBE resolve-loop guard: resolve RETURNED ok after {observed_calls} driver calls"
        ),
        Ok(Err(err)) => println!(
            "PROBE resolve-loop guard: resolve errored after {observed_calls} driver calls: {err}"
        ),
        Err(_) => println!(
            "PROBE resolve-loop guard: resolve DID NOT TERMINATE within 3s; \
             driver calls so far: {observed_calls} (UNBOUNDED refresh loop with \
             pathological still-expired mints)"
        ),
    }
    // fix-r1 grading (was observation-only in R01): the F-01 fix must make
    // resolve terminate FAST with a loud error and exactly ONE driver call.
    let err = match outcome {
        Ok(Err(err)) => err,
        other => panic!("F-01 fix-r1: expected a fast loud error, got {other:?}"),
    };
    assert!(
        err.to_string().contains("single-flight bound")
            || err.to_string().contains("already-expired"),
        "F-01 fix-r1: the loud error names the bound: {err}"
    );
    assert_eq!(
        observed_calls, 1,
        "F-01 fix-r1: one refresh per resolve, never a storm"
    );
    println!("PROBE resolve-loop guard: fix-r1 PASS (loud error after exactly 1 call)");
}

// ── fix-r1 F-02: reload replaces the cell mid-flight (reviewer's own shape) ──

/// A refresh driver whose park-gate can be DISARMED mid-test, so the probe
/// controls flight timing in phase 1 and lets phase 2 run free.
struct SwitchableGateRefresh {
    calls: Mutex<Vec<String>>,
    gate: Mutex<Option<Arc<tokio::sync::Notify>>>,
    minted: Mutex<OAuthTokens>,
}

impl RefreshDriver for SwitchableGateRefresh {
    fn refresh<'a>(
        &'a self,
        provider: &'a str,
        _flow: &'a OAuthFlowConfig,
        refresh_token: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<OAuthTokens, OAuthError>> + Send + 'a>,
    > {
        self.calls
            .lock()
            .expect("calls")
            .push(format!("{provider}:{refresh_token}"));
        let gate = self.gate.lock().expect("gate").clone();
        let minted = self.minted.lock().expect("minted").clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.notified().await;
            }
            Ok(minted)
        })
    }
}

#[tokio::test]
async fn probe_f02_reload_swapped_cell_fences_my_late_writeback() {
    // Reviewer's independent end-state pin with REVIEW markers: a flight
    // parked in transport, a reload swapping in a NEW cell (changed
    // clientId), then the late mint released. The fix moved fence-1 INSIDE
    // the cell lock, so the late write-back must be discarded: the store
    // keeps the pre-reload row and the new cell is never polluted.
    let clock = Arc::new(ManualClock::new(1_000_000));
    let gate = Arc::new(tokio::sync::Notify::new());
    let driver = Arc::new(SwitchableGateRefresh {
        calls: Mutex::new(Vec::new()),
        gate: Mutex::new(Some(gate.clone())),
        minted: Mutex::new(OAuthTokens {
            access_token: "at-REVIEW-late".to_string(),
            refresh_token: "rt-REVIEW-late".to_string(),
            expires_at_unix_ms: u64::MAX,
        }),
    });
    let io = MemoryStoreIo::new();
    let store = seeded_store(io.clone(), "at-REVIEW-old", "rt-REVIEW-1", 1); // expired seed
    let service = CredentialService::new(&oauth_plane(), Some(store), driver.clone(), clock);

    let waiter = tokio::spawn({
        let service = service.clone();
        async move { service.resolve(&route_for("main")).await }
    });
    tokio::task::yield_now().await; // let the flight park in the gate

    // A changed seed (clientId review-1 -> review-2) swaps in a fresh cell.
    let changed = ModelPlaneConfig::parse_and_validate(
        r#"{
            "providers": {
                "main": {
                    "protocol": "openai-completions",
                    "endpoint": "http://127.0.0.1:9/v1",
                    "auth": {"kind": "oauth", "flow": "deviceCode",
                        "clientId": "client-review-2",
                        "tokenEndpoint": "http://127.0.0.1:9/token",
                        "deviceAuthorizationEndpoint": "http://127.0.0.1:9/device"}
                }
            },
            "models": {"chat": {"provider": "main", "model": "m-1"}}
        }"#,
    )
    .expect("valid changed plane");
    service.reload(&changed).await;

    gate.notify_waiters();
    let verdict = waiter.await.expect("joined");
    assert!(
        matches!(verdict, Err(CredentialError::Revoked { .. })),
        "the orphaned flight surfaces the honest terminal verdict: {verdict:?}"
    );
    assert_eq!(driver.calls.lock().expect("calls").len(), 1, "exactly one transport ran");

    // The late mint NEVER reached the store…
    let on_disk: serde_json::Value =
        serde_json::from_str(&io.content().expect("store content")).expect("json");
    assert_eq!(
        on_disk["providers"]["main"]["tokens"]["accessToken"],
        "at-REVIEW-old"
    );
    assert_eq!(
        on_disk["providers"]["main"]["tokens"]["refreshToken"],
        "rt-REVIEW-1"
    );
    // …and never touched the NEW cell: no refresh timestamp, and the new
    // cell still serves the store-seeded (expired) state — a follow-up
    // resolve refreshes THROUGH the new cell and lands cleanly.
    let status = service.status().await;
    let main = status
        .iter()
        .find(|row| row.provider == "main")
        .expect("main row");
    assert_eq!(main.last_refresh_at_unix_ms, None);

    // New cell is functional: disarm the gate, mint an immediately-valid
    // token, resolve again — one more driver call, then Ok.
    *driver.gate.lock().expect("gate") = None;
    *driver.minted.lock().expect("minted") = OAuthTokens {
        access_token: "at-REVIEW-fresh".to_string(),
        refresh_token: "rt-REVIEW-2".to_string(),
        expires_at_unix_ms: u64::MAX,
    };
    let resolved = service.resolve(&route_for("main")).await;
    assert!(
        matches!(resolved, Ok(ApplicableAuth::Bearer(ref tok)) if tok == "at-REVIEW-fresh"),
        "the post-reload cell resolves cleanly: {resolved:?}"
    );
    assert_eq!(
        driver.calls.lock().expect("calls").len(),
        2,
        "one refresh per resolve, this one landed"
    );
    println!("PROBE f02 reload fence: fix-r1 PASS (late write-back discarded, new cell clean)");
}

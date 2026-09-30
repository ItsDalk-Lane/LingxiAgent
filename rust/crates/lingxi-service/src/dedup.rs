//! Submission-surface requestId dedup (R03-T04 deliverable "请求去重",
//! acceptance R03-A08): duplicate submissions with an EXPLICIT client
//! request id are deduplicated against the recorded request DIGEST.
//!
//! Binding rules (the dispatch's 总控细化, frozen here):
//! - The dedup key is (owner kind, owner subject, session id, request id)
//!   — the id namespace belongs to ONE trusted principal IN ONE session.
//!   A GLOBAL requestId map would let different principals (or different
//!   sessions) hijack each other's submissions; that is structurally
//!   impossible here.
//! - Same key + SAME normalized-content digest → idempotent REPLAY: the
//!   original run id is returned, no execution is reused for new content,
//!   no second execution starts.
//! - Same key + DIFFERENT digest → an explicit CONFLICT: the old execution
//!   is not reused, a new one is not started. (Late duplicate SETTLEMENTS
//!   are a different surface — T01/A02's finalize idempotency.)
//!
//! R03 repair G04/F05 — the admission lifecycle. A key binding is NOT
//! "recorded forever at admission" anymore; the registry distinguishes
//! the facts the repair requires:
//!
//! | state | meaning | same-key duplicate sees |
//! |---|---|---|
//! | [`BindingState::Pending`] | RESERVED by a live submission between admission and the durable-start verdict | explicit retryable [`DedupDecision::InFlight`] — never a half-committed fake replay |
//! | [`BindingState::Committed`] | DURABLY ADMITTED: `record_run_started` committed — THE PROMISE POINT: from here the id promises the stable run identity; replays are real and NO failure path may retract the binding | [`DedupDecision::Replay`] |
//! | [`BindingState::Unverified`] | INDETERMINATE: the owning submission exited without a verdict (dropped request future, failed background drive, unverifiable storage error) — the binding is KEPT, never auto-deleted | [`DedupDecision::Unverified`] → the caller resolves it against the durable store (promote / safe re-admission / surface the error) |
//!
//! Compensation rules (红线): a failure that PROVABLY happened before any
//! durable fact and before any dispatch may retract the reservation
//! ([`AdmissionBinding::release_not_started`]); an UNKNOWN execution
//! outcome must keep the binding ([`AdmissionBinding::mark_unverified`],
//! the `Drop` backstop) and is resolved lazily against the store; a
//! committed binding is never released or un-marked. The run identity is
//! promised to the client exactly at the durable run-start commit — that
//! is also the only fact a REPLAY may report.
//!
//! Boundary honesty: the registry is process-memory, bounded by `cap`
//! (over the cap the submission is refused LOUDLY, never silently
//! undeduplicated) and does not survive a restart. The CROSS-RESTART
//! same-key contract lives at the submission surface
//! ([`crate::sessions`]): the durable per-run lineage anchor
//! (`cause_id = "request:{id}"`) makes a post-restart retry an EXPLICIT
//! refusal naming the earlier run — never a silent blind re-execution.

use std::collections::HashMap;
use std::sync::{Arc, LockResult, Mutex, MutexGuard};

/// Registry bound of remembered idempotency keys (default wiring).
pub const DEFAULT_DEDUP_CAP: usize = 4096;

/// Maximum accepted length of a client request id (longer ids are refused
/// loudly instead of being stored unbounded).
pub const MAX_REQUEST_ID_LEN: usize = 128;

/// Loud refusal when the registry is at its cap (service protection; the
/// submission was NOT executed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupRegistryFull {
    pub cap: usize,
}

impl std::fmt::Display for DedupRegistryFull {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "idempotency-key registry is at its cap ({}) — submission refused loudly, \
             never silently un-deduplicated",
            self.cap
        )
    }
}

/// Normalizes the request content for the digest: CRLF line endings fold to
/// LF (the same logical input typed on a different platform is the same
/// submission); everything else is byte-honest. The digest covers the FULL
/// input — never a truncated projection.
pub fn normalized_request_digest_hex(input: &str) -> String {
    let normalized = input.replace("\r\n", "\n");
    lingxi_protocol::digest_arguments(&serde_json::json!({ "input": normalized })).hex
}

/// Validates a client request id: trimmed non-empty and within the length
/// bound. Invalid ids are refused loudly (the caller surfaces the error),
/// never silently ignored.
pub fn validate_request_id(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("requestId must be a non-empty string".to_string());
    }
    if trimmed.len() > MAX_REQUEST_ID_LEN {
        return Err(format!(
            "requestId exceeds the maximum length of {MAX_REQUEST_ID_LEN} bytes"
        ));
    }
    Ok(trimmed.to_string())
}

/// The dedup namespace key: one trusted principal, one session, one
/// explicit request id. Derived from the AUTH chain's principal (kernel
/// storage vocabulary), never from payload claims.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DedupKey {
    pub owner_kind: String,
    pub owner_subject: String,
    pub session_id: String,
    pub request_id: String,
}

/// The lifecycle state of one remembered id binding (R03 repair G04/F05 —
/// see the module docs table).
#[derive(Debug, Clone, PartialEq, Eq)]
enum BindingState {
    /// RESERVED by a live owner (the submission is between admission and
    /// the durable-start verdict).
    Pending { run_id: String },
    /// DURABLY ADMITTED — the promise point; never retracted.
    Committed { run_id: String },
    /// INDETERMINATE — the owner exited without a verdict; kept for lazy
    /// resolution against the durable store.
    Unverified { run_id: String },
}

impl BindingState {
    fn run_id(&self) -> &str {
        match self {
            BindingState::Pending { run_id }
            | BindingState::Committed { run_id }
            | BindingState::Unverified { run_id } => run_id,
        }
    }
}

/// What one remembered submission recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DedupEntry {
    request_digest_hex: String,
    state: BindingState,
}

/// Decision of [`SubmissionDedup::admit`]. Not `Clone`/`Eq`: the `Fresh`
/// variant hands over the ONE owner-side [`AdmissionBinding`] verdict
/// handle.
#[derive(Debug)]
pub enum DedupDecision<T> {
    /// The id holds a DURABLY ADMITTED binding with the SAME digest: the
    /// run row exists — return this run id (idempotent replay; nothing is
    /// re-executed).
    Replay { run_id: String },
    /// The id was used before with a DIFFERENT digest: an explicit
    /// conflict — no reuse of the old execution, no new execution.
    Conflict {
        request_id: String,
        recorded_digest: String,
        submitted_digest: String,
    },
    /// R03 repair G04/F05: the id is RESERVED by a submission that is
    /// STILL between admission and its durable-start verdict. Nothing is
    /// replayable yet — an explicit retryable refusal; never a
    /// half-committed fake result.
    InFlight { request_id: String },
    /// R03 repair G04/F05: the previous owner of this id exited WITHOUT a
    /// durable-start verdict (dropped request future, failed background
    /// drive, unverifiable storage error). The binding is kept; the CALLER
    /// resolves it against the durable store and then either replays /
    /// conflicts (`same_digest`, after promotion) or re-admits fresh
    /// (after the provable-absent retraction).
    Unverified {
        request_id: String,
        run_id: String,
        same_digest: bool,
        recorded_digest: String,
    },
    /// The id is fresh and the admission SUCCEEDED under the registry
    /// lock: the key holds a PENDING reservation bound to the admitted
    /// run id, the admission's product (e.g. the session lease) is handed
    /// back, and the OWNER-side verdict handle ([`AdmissionBinding`])
    /// moves to the caller — it commits the binding at the durable run
    /// start and never lets an unknown outcome delete it.
    Fresh {
        run_id: String,
        admitted: T,
        binding: AdmissionBinding,
    },
}

/// The bounded registry of remembered idempotency keys.
pub struct SubmissionDedup {
    entries: Mutex<HashMap<DedupKey, DedupEntry>>,
    cap: usize,
}

impl Default for SubmissionDedup {
    fn default() -> Self {
        Self::new(DEFAULT_DEDUP_CAP)
    }
}

impl std::fmt::Debug for SubmissionDedup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubmissionDedup")
            .field("cap", &self.cap)
            .field("entries", &self.lock().map(|g| g.len()).unwrap_or(0))
            .finish()
    }
}

impl SubmissionDedup {
    pub fn new(cap: usize) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            cap,
        }
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    fn lock(&self) -> LockResult<MutexGuard<'_, HashMap<DedupKey, DedupEntry>>> {
        self.entries.lock()
    }

    /// Looks the key up (audit/observability only — decisions go through
    /// [`Self::admit`], which is the atomic path).
    pub fn lookup(&self, key: &DedupKey) -> Option<String> {
        self.lock()
            .ok()
            .and_then(|guard| guard.get(key).map(|entry| entry.state.run_id().to_string()))
    }

    /// Begins one idempotent submission ATOMICALLY: replay, conflict,
    /// in-flight refusal, unverified resolution, or a fresh (PENDING)
    /// admission executed UNDER the registry lock.
    ///
    /// `admission` runs the synchronous admission work (the session busy
    /// gate and the run-id allocation on the caller's side) while the key's
    /// registry entry is locked, so a concurrent duplicate with the same id
    /// can neither slip a second admission in between nor observe a
    /// half-admitted binding — it waits for the lock and then replays,
    /// conflicts or gets the in-flight refusal. If `admission` fails
    /// (busy / registry full / allocation error) NOTHING is recorded: a
    /// rejected or failed admission leaves no sticky id. On success the key
    /// holds a PENDING reservation returned together with the owner-side
    /// [`AdmissionBinding`] verdict handle.
    pub fn admit<T, E>(
        self: &Arc<Self>,
        key: DedupKey,
        digest_hex: String,
        admission: impl FnOnce() -> Result<(String, T), E>,
    ) -> Result<Result<DedupDecision<T>, E>, DedupRegistryFull> {
        let mut guard = self.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = guard.get(&key) {
            let same_digest = entry.request_digest_hex == digest_hex;
            let decision = match (&entry.state, same_digest) {
                (BindingState::Committed { run_id }, true) => DedupDecision::Replay {
                    run_id: run_id.clone(),
                },
                (BindingState::Pending { .. }, true) => DedupDecision::InFlight {
                    request_id: key.request_id.clone(),
                },
                (BindingState::Unverified { run_id }, true) => DedupDecision::Unverified {
                    request_id: key.request_id.clone(),
                    run_id: run_id.clone(),
                    same_digest: true,
                    recorded_digest: entry.request_digest_hex.clone(),
                },
                (_, false) => DedupDecision::Conflict {
                    request_id: key.request_id.clone(),
                    recorded_digest: entry.request_digest_hex.clone(),
                    submitted_digest: digest_hex,
                },
            };
            return Ok(Ok(decision));
        }
        if guard.len() >= self.cap {
            return Err(DedupRegistryFull { cap: self.cap });
        }
        // The admission runs under the lock: its failure records nothing,
        // its success commits the PENDING reservation in the same critical
        // section.
        let (run_id, admitted) = match admission() {
            Ok(pair) => pair,
            Err(err) => return Ok(Err(err)),
        };
        guard.insert(
            key.clone(),
            DedupEntry {
                request_digest_hex: digest_hex,
                state: BindingState::Pending {
                    run_id: run_id.clone(),
                },
            },
        );
        let binding = AdmissionBinding {
            registry: Arc::clone(self),
            key,
            run_id: run_id.clone(),
        };
        Ok(Ok(DedupDecision::Fresh {
            run_id,
            admitted,
            binding,
        }))
    }

    /// Owner-side state transition (the shared implementation of the
    /// [`AdmissionBinding`] verdicts and the unverified resolutions).
    /// Every op is a no-op unless the entry still binds THIS run id.
    fn transition(&self, key: &DedupKey, run_id: &str, op: BindingOp) -> bool {
        let mut guard = self.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = guard.get_mut(key) else {
            return false;
        };
        if entry.state.run_id() != run_id {
            // A replaced entry is never touched by a stale binding.
            return false;
        }
        match op {
            BindingOp::CommitDurable => {
                if matches!(entry.state, BindingState::Committed { .. }) {
                    return false;
                }
                entry.state = BindingState::Committed {
                    run_id: run_id.to_string(),
                };
                true
            }
            BindingOp::ReleaseNotStarted => {
                // NEVER releases a committed binding: a durable run row
                // exists — the id already promises that run.
                if matches!(entry.state, BindingState::Committed { .. }) {
                    return false;
                }
                guard.remove(key);
                true
            }
            BindingOp::MarkUnverified => {
                if matches!(entry.state, BindingState::Pending { .. }) {
                    entry.state = BindingState::Unverified {
                        run_id: run_id.to_string(),
                    };
                    return true;
                }
                false
            }
        }
    }

    /// Resolution of an UNVERIFIED binding whose durable run row EXISTS
    /// (verified by the caller through the storage port): the id now
    /// PROMISES that run — promote to Committed. Only acts on an
    /// Unverified entry binding this exact run id.
    pub fn resolve_unverified_present(&self, key: &DedupKey, run_id: &str) -> bool {
        let mut guard = self.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        match guard.get_mut(key) {
            Some(entry)
                if matches!(entry.state, BindingState::Unverified { .. })
                    && entry.state.run_id() == run_id =>
            {
                entry.state = BindingState::Committed {
                    run_id: run_id.to_string(),
                };
                true
            }
            _ => false,
        }
    }

    /// Resolution of an UNVERIFIED binding whose durable run row
    /// PROVABLY NEVER EXISTED (verified by the caller through the storage
    /// port): nothing was ever durably admitted and nothing was executed
    /// before the run row — the reservation is safely retracted and the
    /// id is fresh again. Only acts on an Unverified entry binding this
    /// exact run id.
    pub fn resolve_unverified_absent(&self, key: &DedupKey, run_id: &str) -> bool {
        let mut guard = self.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let removable = guard.get(key).is_some_and(|entry| {
            matches!(&entry.state, BindingState::Unverified { run_id: bound } if bound.as_str() == run_id)
        });
        if removable {
            guard.remove(key);
            true
        } else {
            false
        }
    }
}

/// The owner-side verdict operations of [`AdmissionBinding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BindingOp {
    CommitDurable,
    ReleaseNotStarted,
    MarkUnverified,
}

/// R03 repair G04/F05 — the OWNER-side verdict handle of one fresh
/// admission. The submission surface holds this from the PENDING
/// reservation until the durable run-start verdict; the run driver calls
/// [`AdmissionBinding::commit_durable`] the moment `record_run_started`
/// commits, and every exit path that cannot prove "never started" leaves
/// the honest [`BindingState::Unverified`] fact instead of deleting the
/// binding.
pub struct AdmissionBinding {
    registry: Arc<SubmissionDedup>,
    key: DedupKey,
    run_id: String,
}

impl std::fmt::Debug for AdmissionBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdmissionBinding")
            .field("run_id", &self.run_id)
            .field("request_id", &self.key.request_id)
            .finish()
    }
}

impl AdmissionBinding {
    /// The DURABLE-START VERDICT — the promise point: `record_run_started`
    /// committed, the run row exists. From this moment the requestId (if
    /// the client sent one) stably identifies this run: replays are real
    /// and no later failure — including a lost response — may retract the
    /// binding. Idempotent; called by the run driver right after the
    /// durable start commit.
    pub fn commit_durable(&self) {
        self.registry
            .transition(&self.key, &self.run_id, BindingOp::CommitDurable);
    }

    /// The KNOWN-NOT-STARTED verdict: the caller VERIFIED (against the
    /// durable store) that the run row never came to exist — which also
    /// proves no external action happened, because every external action
    /// of a drive happens after its durable start. The reservation is
    /// safely retracted and the id is fresh again for the retry. NEVER
    /// call this for an unverified outcome (use
    /// [`AdmissionBinding::mark_unverified`]) and never after any
    /// dispatch (a refused dispatch is the only other legal caller
    /// site — nothing was spawned, nothing was written).
    pub fn release_not_started(&self) {
        self.registry
            .transition(&self.key, &self.run_id, BindingOp::ReleaseNotStarted);
    }

    /// The INDETERMINATE verdict: the start outcome could not be verified
    /// (storage read failed / the owning future vanished). The binding is
    /// KEPT — an unknown execution outcome must never delete it — and the
    /// next same-key submission resolves it against the durable store.
    pub fn mark_unverified(&self) {
        self.registry
            .transition(&self.key, &self.run_id, BindingOp::MarkUnverified);
    }

    /// Whether the binding already holds the durable promise (the
    /// committed run row exists by construction).
    pub fn is_committed(&self) -> bool {
        self.registry.lock().ok().is_some_and(|guard| {
            matches!(
                guard.get(&self.key),
                Some(DedupEntry {
                    state: BindingState::Committed { run_id },
                    ..
                }) if run_id.as_str() == self.run_id
            )
        })
    }

    /// The reserved run id (observability).
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// A lightweight cloneable retraction handle for the DISPATCH side of
    /// a background submission: the frame holds it while the binding
    /// itself moves into the dispatched task, and uses it only for
    /// failures that PROVABLY happened before any dispatch (a refused
    /// spawn — nothing was written, nothing was executed). It can only
    /// release a still-PENDING reservation bound to the same run id; a
    /// committed or already-resolved binding is untouchable.
    pub fn retractor(&self) -> AdmissionRetractor {
        AdmissionRetractor {
            registry: Arc::clone(&self.registry),
            key: self.key.clone(),
            run_id: self.run_id.clone(),
        }
    }
}

/// The dispatch-side retraction handle ([`AdmissionBinding::retractor`]).
pub struct AdmissionRetractor {
    registry: Arc<SubmissionDedup>,
    key: DedupKey,
    run_id: String,
}

impl std::fmt::Debug for AdmissionRetractor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdmissionRetractor")
            .field("run_id", &self.run_id)
            .field("request_id", &self.key.request_id)
            .finish()
    }
}

impl AdmissionRetractor {
    /// Same contract as [`AdmissionBinding::release_not_started`]:
    /// provably known-not-dispatched only; never touches a committed
    /// binding.
    pub fn release_not_started(&self) {
        self.registry
            .transition(&self.key, &self.run_id, BindingOp::ReleaseNotStarted);
    }
}

impl Drop for AdmissionBinding {
    fn drop(&mut self) {
        // The owning submission exited WITHOUT delivering a verdict: keep
        // the binding (an unknown execution outcome is never deleted) and
        // leave the honest Unverified fact for the next same-key
        // submission to resolve. A committed or already-resolved binding
        // is untouched.
        self.mark_unverified();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Test admission shape: (run_id, product). Failable on demand.
    struct Admission {
        run_id: &'static str,
        fail: bool,
    }

    fn admission(a: Admission) -> impl FnOnce() -> Result<(String, u8), &'static str> {
        move || {
            if a.fail {
                Err("admission rejected")
            } else {
                Ok((a.run_id.to_string(), 7u8))
            }
        }
    }

    fn key(request_id: &str) -> DedupKey {
        DedupKey {
            owner_kind: "local_user".to_string(),
            owner_subject: "user_local".to_string(),
            session_id: "sess_a".to_string(),
            request_id: request_id.to_string(),
        }
    }

    #[test]
    fn same_id_same_digest_replays_only_the_committed_run() {
        let dedup = Arc::new(SubmissionDedup::new(8));
        let digest = normalized_request_digest_hex("hello");
        let binding = match dedup
            .admit(
                key("req-1"),
                digest.clone(),
                admission(Admission {
                    run_id: "run_1",
                    fail: false,
                }),
            )
            .unwrap()
            .unwrap()
        {
            DedupDecision::Fresh {
                run_id,
                admitted,
                binding,
            } => {
                assert_eq!(run_id, "run_1");
                assert_eq!(admitted, 7);
                binding
            }
            other => panic!("first admit must be fresh, got {other:?}"),
        };
        // BEFORE the durable-start verdict the reservation answers
        // in-flight, never a fake replay.
        assert!(!binding.is_committed());
        match dedup
            .admit(
                key("req-1"),
                digest.clone(),
                admission(Admission {
                    run_id: "run_2",
                    fail: false,
                }),
            )
            .unwrap()
            .unwrap()
        {
            DedupDecision::InFlight { request_id } => assert_eq!(request_id, "req-1"),
            other => panic!("pending reservation must be in-flight, got {other:?}"),
        }
        // The durable-start verdict is the promise point.
        binding.commit_durable();
        assert!(binding.is_committed());
        match dedup
            .admit(
                key("req-1"),
                digest,
                admission(Admission {
                    run_id: "run_2",
                    fail: false,
                }),
            )
            .unwrap()
            .unwrap()
        {
            DedupDecision::Replay { run_id } => assert_eq!(run_id, "run_1"),
            other => panic!("expected replay after commit, got {other:?}"),
        }
        assert_eq!(dedup.lookup(&key("req-1")), Some("run_1".to_string()));
        drop(binding);
        assert_eq!(
            dedup.lookup(&key("req-1")),
            Some("run_1".to_string()),
            "dropping a COMMITTED binding changes nothing"
        );
    }

    #[test]
    fn same_id_different_digest_is_an_explicit_conflict() {
        let dedup = Arc::new(SubmissionDedup::new(8));
        let first = dedup
            .admit(
                key("req-1"),
                normalized_request_digest_hex("hello"),
                admission(Admission {
                    run_id: "run_1",
                    fail: false,
                }),
            )
            .unwrap()
            .unwrap();
        assert!(matches!(first, DedupDecision::Fresh { .. }));
        match dedup
            .admit(
                key("req-1"),
                normalized_request_digest_hex("changed content"),
                admission(Admission {
                    run_id: "run_2",
                    fail: false,
                }),
            )
            .unwrap()
            .unwrap()
        {
            DedupDecision::Conflict {
                request_id,
                recorded_digest,
                submitted_digest,
            } => {
                assert_eq!(request_id, "req-1");
                assert_ne!(recorded_digest, submitted_digest);
            }
            other => panic!("expected conflict, got {other:?}"),
        }
        // The conflict changed nothing: the SAME digest still resolves to
        // the original reservation and the different-content admission
        // never executed.
        match dedup
            .admit(
                key("req-1"),
                normalized_request_digest_hex("hello"),
                admission(Admission {
                    run_id: "run_2",
                    fail: false,
                }),
            )
            .unwrap()
            .unwrap()
        {
            DedupDecision::InFlight { .. } => {}
            other => panic!("expected in-flight after conflict, got {other:?}"),
        }
    }

    #[test]
    fn different_principal_or_session_shares_no_namespace() {
        let dedup = Arc::new(SubmissionDedup::new(8));
        let digest = normalized_request_digest_hex("hello");
        let first = dedup
            .admit(
                key("req-1"),
                digest.clone(),
                admission(Admission {
                    run_id: "run_alpha",
                    fail: false,
                }),
            )
            .unwrap()
            .unwrap();
        assert!(matches!(first, DedupDecision::Fresh { .. }));
        // Another principal kind with the same id → its own namespace.
        let other_principal = DedupKey {
            owner_kind: "device".to_string(),
            owner_subject: "user_local".to_string(),
            session_id: "sess_a".to_string(),
            request_id: "req-1".to_string(),
        };
        assert!(
            matches!(
                dedup
                    .admit(
                        other_principal,
                        digest.clone(),
                        admission(Admission {
                            run_id: "run_b",
                            fail: false
                        })
                    )
                    .unwrap()
                    .unwrap(),
                DedupDecision::Fresh { .. }
            ),
            "a global requestId map must not let different principals cross-use ids"
        );
        // Another session with the same id → its own namespace too.
        let other_session = DedupKey {
            owner_kind: "local_user".to_string(),
            owner_subject: "user_local".to_string(),
            session_id: "sess_b".to_string(),
            request_id: "req-1".to_string(),
        };
        assert!(matches!(
            dedup
                .admit(
                    other_session,
                    digest,
                    admission(Admission {
                        run_id: "run_c",
                        fail: false
                    })
                )
                .unwrap()
                .unwrap(),
            DedupDecision::Fresh { .. }
        ));
    }

    #[test]
    fn rejected_admission_records_nothing_and_cap_is_loud() {
        let dedup = Arc::new(SubmissionDedup::new(2));
        let digest = normalized_request_digest_hex("hello");
        // An admission that FAILS (busy rejection shape) records nothing.
        match dedup
            .admit(
                key("req-1"),
                digest.clone(),
                admission(Admission {
                    run_id: "run_x",
                    fail: true,
                }),
            )
            .unwrap()
        {
            Err("admission rejected") => {}
            other => panic!("admission error must propagate, got {other:?}"),
        }
        assert_eq!(dedup.lookup(&key("req-1")), None, "no sticky id");
        // Fill the cap with successful admissions.
        for id in ["req-1", "req-2"] {
            assert!(matches!(
                dedup
                    .admit(
                        key(id),
                        digest.clone(),
                        admission(Admission {
                            run_id: "run_x",
                            fail: false
                        })
                    )
                    .unwrap()
                    .unwrap(),
                DedupDecision::Fresh { .. }
            ));
        }
        match dedup.admit(
            key("req-3"),
            digest,
            admission(Admission {
                run_id: "run_x",
                fail: false,
            }),
        ) {
            Err(DedupRegistryFull { cap }) => assert_eq!(cap, 2),
            other => panic!("expected registry-full, got {other:?}"),
        }
    }

    #[test]
    fn digest_normalizes_line_endings_but_not_content() {
        assert_eq!(
            normalized_request_digest_hex("line\r\nbreak"),
            normalized_request_digest_hex("line\nbreak"),
            "CRLF vs LF is the same logical submission"
        );
        assert_ne!(
            normalized_request_digest_hex("hello"),
            normalized_request_digest_hex("hello "),
            "content differences are never folded away"
        );
    }

    #[test]
    fn request_id_validation_is_loud() {
        assert_eq!(validate_request_id("  ok  ").unwrap(), "ok");
        assert!(validate_request_id("   ").is_err());
        assert!(validate_request_id(&"x".repeat(MAX_REQUEST_ID_LEN + 1)).is_err());
        assert!(validate_request_id(&"x".repeat(MAX_REQUEST_ID_LEN)).is_ok());
    }

    // ── R03 repair G04/F05: the admission lifecycle states ─────────────────

    #[test]
    fn dropped_owner_without_verdict_is_kept_and_resolved_lazily() {
        // The owner vanished between reservation and verdict (a dropped
        // request future / a failed background drive): the binding is
        // KEPT as Unverified — never silently deleted — and the next
        // same-key submission resolves it against the durable store.
        let dedup = Arc::new(SubmissionDedup::new(8));
        let digest = normalized_request_digest_hex("hello");
        let binding = match dedup
            .admit(
                key("req-1"),
                digest.clone(),
                admission(Admission {
                    run_id: "run_1",
                    fail: false,
                }),
            )
            .unwrap()
            .unwrap()
        {
            DedupDecision::Fresh { binding, .. } => binding,
            other => panic!("expected fresh, got {other:?}"),
        };
        drop(binding); // no verdict
        match dedup
            .admit(
                key("req-1"),
                digest.clone(),
                admission(Admission {
                    run_id: "run_2",
                    fail: false,
                }),
            )
            .unwrap()
            .unwrap()
        {
            DedupDecision::Unverified {
                run_id,
                same_digest,
                ..
            } => {
                assert_eq!(run_id, "run_1");
                assert!(same_digest);
            }
            other => panic!("expected unverified, got {other:?}"),
        }
        // Resolution A — the durable row EXISTS: promote, then the same
        // digest replays that exact run (changed content conflicts).
        assert!(dedup.resolve_unverified_present(&key("req-1"), "run_1"));
        match dedup
            .admit(
                key("req-1"),
                digest,
                admission(Admission {
                    run_id: "run_2",
                    fail: false,
                }),
            )
            .unwrap()
            .unwrap()
        {
            DedupDecision::Replay { run_id } => assert_eq!(run_id, "run_1"),
            other => panic!("promoted binding must replay, got {other:?}"),
        }
        assert!(
            !dedup.resolve_unverified_absent(&key("req-1"), "run_1"),
            "absent resolution on a committed binding is refused"
        );
        assert_eq!(
            dedup.lookup(&key("req-1")),
            Some("run_1".to_string()),
            "the committed binding survived the refused release"
        );
    }

    #[test]
    fn provably_absent_unverified_reservation_is_safely_re_admitted() {
        // Resolution B — the durable row PROVABLY NEVER EXISTED: the
        // reservation is retracted and the id is fresh again.
        let dedup = Arc::new(SubmissionDedup::new(8));
        let digest = normalized_request_digest_hex("hello");
        let binding = match dedup
            .admit(
                key("req-1"),
                digest.clone(),
                admission(Admission {
                    run_id: "run_1",
                    fail: false,
                }),
            )
            .unwrap()
            .unwrap()
        {
            DedupDecision::Fresh { binding, .. } => binding,
            other => panic!("expected fresh, got {other:?}"),
        };
        drop(binding);
        assert!(dedup.resolve_unverified_absent(&key("req-1"), "run_1"));
        assert_eq!(dedup.lookup(&key("req-1")), None, "the id is fresh again");
        let second = dedup
            .admit(
                key("req-1"),
                digest,
                admission(Admission {
                    run_id: "run_9",
                    fail: false,
                }),
            )
            .unwrap()
            .unwrap();
        assert!(matches!(second, DedupDecision::Fresh { .. }));
    }

    #[test]
    fn committed_binding_is_never_released_or_unmarked() {
        // 红线：没有任何错误路径可以撤回一个已持久受理的绑定。
        let dedup = Arc::new(SubmissionDedup::new(8));
        let digest = normalized_request_digest_hex("hello");
        let binding = match dedup
            .admit(
                key("req-1"),
                digest,
                admission(Admission {
                    run_id: "run_1",
                    fail: false,
                }),
            )
            .unwrap()
            .unwrap()
        {
            DedupDecision::Fresh { binding, .. } => binding,
            other => panic!("expected fresh, got {other:?}"),
        };
        binding.commit_durable();
        // All three "failure-shaped" ops are no-ops on a committed binding.
        binding.release_not_started();
        binding.mark_unverified();
        binding.release_not_started();
        drop(binding);
        assert_eq!(
            dedup.lookup(&key("req-1")),
            Some("run_1".to_string()),
            "a committed binding is never retracted"
        );
        match dedup
            .admit(
                key("req-1"),
                normalized_request_digest_hex("hello"),
                admission(Admission {
                    run_id: "run_2",
                    fail: false,
                }),
            )
            .unwrap()
            .unwrap()
        {
            DedupDecision::Replay { run_id } => assert_eq!(run_id, "run_1"),
            other => panic!("the committed binding still replays, got {other:?}"),
        }
    }

    #[test]
    fn a_stale_binding_cannot_touch_a_replaced_entry() {
        // Defensive identity fence: a binding whose run id no longer
        // matches the entry (the entry was resolved and re-admitted
        // concurrently) must not mutate or remove it.
        let dedup = Arc::new(SubmissionDedup::new(8));
        let digest = normalized_request_digest_hex("hello");
        let stale = match dedup
            .admit(
                key("req-1"),
                digest.clone(),
                admission(Admission {
                    run_id: "run_1",
                    fail: false,
                }),
            )
            .unwrap()
            .unwrap()
        {
            DedupDecision::Fresh { binding, .. } => binding,
            other => panic!("expected fresh, got {other:?}"),
        };
        // The owner reports an indeterminate outcome (the explicit
        // mark — the same fact its Drop would leave), and a concurrent
        // same-key submission resolves the unverified reservation away
        // and re-admits a NEW one.
        stale.mark_unverified();
        assert!(dedup.resolve_unverified_absent(&key("req-1"), "run_1"));
        let _new = dedup
            .admit(
                key("req-1"),
                digest,
                admission(Admission {
                    run_id: "run_2",
                    fail: false,
                }),
            )
            .unwrap()
            .unwrap();
        // The STALE binding's ops must be no-ops on run_2's reservation.
        stale.release_not_started();
        stale.mark_unverified();
        stale.commit_durable();
        drop(stale);
        assert_eq!(
            dedup.lookup(&key("req-1")),
            Some("run_2".to_string()),
            "the stale binding did not touch the replaced reservation"
        );
    }
}

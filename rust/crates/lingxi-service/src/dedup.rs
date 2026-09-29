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
//! Boundary honesty: the registry is process-memory, bounded by `cap`
//! (over the cap the submission is refused LOUDLY, never silently
//! undeduplicated) and does not survive a restart — cross-restart retry
//! semantics belong to the R03-T07 recovery work; after a restart a retry
//! with the same id is simply a fresh (fully validated) submission.

use std::collections::HashMap;
use std::sync::{LockResult, Mutex, MutexGuard};

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

/// What one remembered submission recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DedupEntry {
    request_digest_hex: String,
    run_id: String,
}

/// Decision of [`SubmissionDedup::admit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DedupDecision<T> {
    /// The id was used before with the SAME digest: return this run id
    /// (idempotent replay — nothing is re-executed).
    Replay { run_id: String },
    /// The id was used before with a DIFFERENT digest: an explicit
    /// conflict — no reuse of the old execution, no new execution.
    Conflict {
        request_id: String,
        recorded_digest: String,
        submitted_digest: String,
    },
    /// The id is fresh and the admission SUCCEEDED under the registry
    /// lock: the key is permanently bound to the admitted run id and the
    /// admission's product (e.g. the session lease) is handed back.
    Fresh { run_id: String, admitted: T },
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
            .and_then(|guard| guard.get(key).map(|entry| entry.run_id.clone()))
    }

    /// Begins one idempotent submission ATOMICALLY: replay, conflict, or a
    /// fresh admission executed UNDER the registry lock.
    ///
    /// `admission` runs the synchronous admission work (the session busy
    /// gate and the run-id allocation on the caller's side) while the key's
    /// registry entry is locked, so a concurrent duplicate with the same id
    /// can neither slip a second admission in between nor observe a
    /// half-admitted binding — it waits for the lock and then replays or
    /// conflicts. If `admission` fails (busy / registry full / allocation
    /// error) NOTHING is recorded: a rejected or failed admission leaves no
    /// sticky id. On success the key is permanently bound to the admitted
    /// run id.
    pub fn admit<T, E>(
        &self,
        key: DedupKey,
        digest_hex: String,
        admission: impl FnOnce() -> Result<(String, T), E>,
    ) -> Result<Result<DedupDecision<T>, E>, DedupRegistryFull> {
        let mut guard = self.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = guard.get(&key) {
            if entry.request_digest_hex == digest_hex {
                return Ok(Ok(DedupDecision::Replay {
                    run_id: entry.run_id.clone(),
                }));
            }
            return Ok(Ok(DedupDecision::Conflict {
                request_id: key.request_id.clone(),
                recorded_digest: entry.request_digest_hex.clone(),
                submitted_digest: digest_hex,
            }));
        }
        if guard.len() >= self.cap {
            return Err(DedupRegistryFull { cap: self.cap });
        }
        // The admission runs under the lock: its failure records nothing,
        // its success commits the binding in the same critical section.
        let (run_id, admitted) = match admission() {
            Ok(pair) => pair,
            Err(err) => return Ok(Err(err)),
        };
        guard.insert(
            key,
            DedupEntry {
                request_digest_hex: digest_hex,
                run_id: run_id.clone(),
            },
        );
        Ok(Ok(DedupDecision::Fresh { run_id, admitted }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn same_id_same_digest_replays_the_committed_run() {
        let dedup = SubmissionDedup::new(8);
        let digest = normalized_request_digest_hex("hello");
        match dedup
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
            DedupDecision::Fresh { run_id, admitted } => {
                assert_eq!(run_id, "run_1");
                assert_eq!(admitted, 7);
            }
            other => panic!("first admit must be fresh, got {other:?}"),
        }
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
            other => panic!("expected replay, got {other:?}"),
        }
        assert_eq!(dedup.lookup(&key("req-1")), Some("run_1".to_string()));
    }

    #[test]
    fn same_id_different_digest_is_an_explicit_conflict() {
        let dedup = SubmissionDedup::new(8);
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
        // The conflict changed nothing: the SAME digest still replays run_1
        // and the different-content admission never executed.
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
            other => panic!("expected replay after conflict, got {other:?}"),
        }
    }

    #[test]
    fn different_principal_or_session_shares_no_namespace() {
        let dedup = SubmissionDedup::new(8);
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
        let dedup = SubmissionDedup::new(2);
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
}

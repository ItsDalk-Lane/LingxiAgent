//! Side-effect recovery surface over the InvocationJournal (R03-T05).
//!
//! This is the recovery DATA PLANE plus the classification surface: it
//! loads one run's durable invocation receipts, runs the kernel's
//! [`classify_invocation_recovery`] decision over each entry under the
//! caller-supplied per-target capabilities, and persists the honest
//! `unknown` verdict for entries caught in the crash window (started
//! without a receipt — 已执行但回执未持久化).
//!
//! Boundary (R03): the STARTUP SCAN that walks every non-terminal run and
//! coordinates what actually happens next (re-drive, resume, park in
//! `interrupted/needs_attention`) is R03-T07's RecoveryCoordinator; the
//! per-target capability registry is R04's tool gateway. This module is
//! the reusable decision + journal surface both will consume — no second
//! scheduler, no second store.
//!
//! Nothing here ever re-executes a tool or re-drives a run: the decisions
//! are returned to the caller (T07) as explainable data.

use lingxi_kernel::invocation::{
    classify_invocation_recovery, invocation_recovery_class, RecoveryClass, RecoveryDecision,
    ToolRecoveryCapability,
};
use lingxi_kernel::ports::{StorageError, StoragePort};
use lingxi_protocol::{RunId, ToolCallId};

/// The per-entry recovery outcome of one run's journal scan: what the
/// durable facts prove, and what may (or must NOT) happen next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationRecoveryEntry {
    pub journal_id: String,
    pub target: String,
    pub phase: lingxi_kernel::ports::InvocationPhase,
    pub class: RecoveryClass,
    pub decision: RecoveryDecision,
    /// Whether THIS scan persisted the `unknown` verdict for the entry
    /// (true exactly once per crash-window entry; a re-scan replays
    /// idempotently and reports false).
    pub unknown_verdict_persisted: bool,
}

/// The recovery report of one run's invocation journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecoveryReport {
    pub run_id: String,
    pub entries: Vec<InvocationRecoveryEntry>,
}

impl RunRecoveryReport {
    /// How many entries landed in each decision bucket (stable order:
    /// safe_to_reexecute, confirmed_settled, unknown_read_only_reexecute,
    /// unknown_resume_with_idempotency_key, unknown_verify_externally,
    /// needs_attention).
    pub fn decision_counts(&self) -> Vec<(&'static str, usize)> {
        let names = [
            "safe_to_reexecute",
            "confirmed_settled",
            "unknown_read_only_reexecute",
            "unknown_resume_with_idempotency_key",
            "unknown_verify_externally",
            "needs_attention",
        ];
        names
            .into_iter()
            .map(|name| {
                (
                    name,
                    self.entries
                        .iter()
                        .filter(|e| e.decision.name() == name)
                        .count(),
                )
            })
            .collect()
    }
}

/// Resolves the per-target recovery capabilities for one journal scan. The
/// R03 default for any target without verified capabilities is
/// [`ToolRecoveryCapability::CONSERVATIVE`] — unknown outcomes of such
/// tools are NEVER auto-recovered (they go to needs-attention). R04's
/// registry replaces the resolution, not the conservative default.
pub trait RecoveryCapabilitySource: Send + Sync {
    fn capability_of(&self, target: &str) -> ToolRecoveryCapability;
}

/// The conservative source used when no capability registry is wired:
/// every unverified tool refuses automatic recovery of unknown outcomes.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConservativeCapabilities;

impl RecoveryCapabilitySource for ConservativeCapabilities {
    fn capability_of(&self, _target: &str) -> ToolRecoveryCapability {
        ToolRecoveryCapability::CONSERVATIVE
    }
}

/// R04-T01: the registry-backed capability source — the resolution the
/// R03 handoff deferred to "R04's tool registry". Per-target capabilities
/// come from the VERIFIED [`ToolManifest::recovery`] facts the registrar
/// supplied; a target the registry does not know stays conservative
/// (unknown tool ≠ optimistic tool). The conservative default itself is
/// never replaced by optimism — only explicitly verified manifests can.
pub struct RegistryCapabilities {
    registry: std::sync::Arc<lingxi_kernel::toolcatalog::ToolRegistry>,
}

impl RegistryCapabilities {
    pub fn new(registry: std::sync::Arc<lingxi_kernel::toolcatalog::ToolRegistry>) -> Self {
        Self { registry }
    }
}

impl RecoveryCapabilitySource for RegistryCapabilities {
    fn capability_of(&self, target: &str) -> ToolRecoveryCapability {
        use lingxi_kernel::toolcatalog::{ToolTargetId, ToolTargetRef};
        // Journal targets may be derived target ids ("tool:…") or legacy
        // plain names; resolve both, and stay conservative on ANY
        // unresolved or ambiguous reference.
        let resolved = if target.starts_with("tool:") {
            self.registry
                .resolve(&ToolTargetRef::ByTargetId {
                    target_id: ToolTargetId::parse(target),
                })
                .ok()
        } else {
            self.registry
                .resolve(&ToolTargetRef::ByName {
                    name: target.to_string(),
                })
                .ok()
        };
        match resolved {
            Some(target_id) => match self.registry.describe(&target_id) {
                Ok(listing) => listing.recovery,
                Err(_) => ToolRecoveryCapability::CONSERVATIVE,
            },
            None => ToolRecoveryCapability::CONSERVATIVE,
        }
    }
}

/// Loads one run's journal and classifies every entry (READ-ONLY — nothing
/// is persisted). This is the pure inspection half of
/// [`recover_run_invocations`].
pub async fn classify_run_invocations<P: StoragePort>(
    port: &P,
    run_id: &str,
    capabilities: &dyn RecoveryCapabilitySource,
) -> Result<RunRecoveryReport, StorageError> {
    let entries = port
        .load_invocation_journal(&RunId::new(run_id.to_string()))
        .await?;
    let classified = entries
        .into_iter()
        .map(|entry| {
            let class = invocation_recovery_class(&entry);
            let decision =
                classify_invocation_recovery(&entry, &capabilities.capability_of(&entry.target));
            InvocationRecoveryEntry {
                journal_id: entry.journal_id.clone(),
                target: entry.target,
                phase: entry.phase,
                class,
                decision,
                unknown_verdict_persisted: false,
            }
        })
        .collect();
    Ok(RunRecoveryReport {
        run_id: run_id.to_string(),
        entries: classified,
    })
}

/// The recovery pass over one run's invocation journal (R03-T05):
///
/// 1. loads the durable receipts and classifies each entry into the
///    four-state recovery vocabulary under the per-target capabilities;
/// 2. persists the honest `unknown` VERDICT for entries caught in the
///    crash window (`started`, no receipt) — the durable statement that
///    the external outcome cannot be determined from local state. The
///    verdict is idempotent: re-running the pass over an already-marked
///    entry persists nothing and reports `unknown_verdict_persisted:
///    false`.
///
/// The pass NEVER re-executes anything itself: the decisions are data for
/// the caller (the R03-T07 coordinator decides what is re-driven, resumed
/// or parked).
pub async fn recover_run_invocations<P: StoragePort>(
    port: &P,
    run_id: &str,
    capabilities: &dyn RecoveryCapabilitySource,
    now_ms: u64,
) -> Result<RunRecoveryReport, StorageError> {
    let raw = port
        .load_invocation_journal(&RunId::new(run_id.to_string()))
        .await?;
    let mut entries = Vec::with_capacity(raw.len());
    for entry in &raw {
        let class = invocation_recovery_class(entry);
        let decision =
            classify_invocation_recovery(entry, &capabilities.capability_of(&entry.target));
        // Persist the unknown verdict exactly for the crash-window shape:
        // started without a receipt. Entries already marked unknown by an
        // earlier pass (or by a fenced/unobserved result) replay
        // idempotently and report persisted=false.
        let mut persisted = false;
        if class == RecoveryClass::Unknown
            && entry.phase == lingxi_kernel::ports::InvocationPhase::Started
            && entry.receipt.is_none()
        {
            port.record_invocation_unknown(
                &ToolCallId::new(entry.journal_id.clone()),
                format!(
                    "recovery classification: crash window between the external execution and \
                     the receipt commit (target {:?}, idempotency key {:?})",
                    entry.target, entry.idempotency_key
                ),
                now_ms,
            )
            .await?;
            persisted = true;
        }
        entries.push(InvocationRecoveryEntry {
            journal_id: entry.journal_id.clone(),
            target: entry.target.clone(),
            phase: entry.phase,
            class,
            decision,
            unknown_verdict_persisted: persisted,
        });
    }
    Ok(RunRecoveryReport {
        run_id: run_id.to_string(),
        entries,
    })
}

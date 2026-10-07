//! Data-epoch startup gate (R02-T06 step 3 + PROD-DEFECT-1 closure).
//!
//! The new Rust service reads the SAME on-disk epoch metadata as the
//! incumbent Node kernel (`shared/data-epoch.cjs` formats, both at the data
//! home root):
//! - stamp `data-epoch.json` — schema v2 (`schemaVersion`/`epoch`/
//!   `minimumReaderEpoch`/`committedDataEpoch`/`lastVersion`/`updatedAt`)
//!   and legacy v1 (`{epoch}` only); and
//! - transition journal `data-epoch-transition.json` — schema v1 with the
//!   seven phases prepared → committed.
//!
//! ## Fail-closed semantics (the PROD-DEFECT-1 fix; ADR-004 §5)
//!
//! The incumbent Node gate has a registered production defect
//! (RR-T07-PROD-DEFECT-1): corrupt-class failures (corrupt-stamp /
//! corrupt-journal / corrupt-transition) DROP the readable journal's
//! from/toEpoch, so the server's baseline softener mis-judges `mustBlock`
//! and fails OPEN — probe variants R7/R9/R10 each really wrote 55 new
//! entries — and the warning text asserts "no higher-epoch evidence was
//! found" even when a readable higher-epoch transition journal sits in the
//! directory (R7/R10, i.e. the text contradicts the facts).
//!
//! This module closes that defect in the new stack:
//! 1. **Corrupt never passes.** Every failure of this gate refuses startup
//!    (exit 2 in the binary). There is NO baseline softening at all — the
//!    DATA_EPOCH=1 "damaged metadata is only diagnostic" relaxation of the
//!    incumbent gate does not exist here, so there is no fail-open edge to
//!    mis-judge.
//! 2. **Evidence is carried and reported truthfully.** Whenever the
//!    transition journal itself is READABLE (parsed and validated), the
//!    failure carries it as `journal_evidence` (fromEpoch/toEpoch/phase/
//!    transitionId) and the rendered diagnostic states the concrete
//!    higher-epoch target (e.g. "epochs 1→2, phase=barrier_raised").
//! 3. **The lie is structurally impossible.** The string
//!    "no higher-epoch evidence was found" appears nowhere in this crate
//!    and a unit test freezes that: when evidence exists it is printed;
//!    when it does not, the text says exactly what was unreadable instead
//!    of claiming an absence.
//!
//! ## Ordering in the startup chain
//!
//! Same-home mutex first (instance lock, R02-T02 — mirrors the incumbent
//! `server/index.ts` order where the mutex precedes the epoch gate), then
//! THIS gate, then any store is opened or auth bootstrap runs. The gate is
//! therefore the first consumer of the data home's contents and the only
//! writer of the stamp (fresh homes only). A gate refusal leaves every
//! pre-existing data file byte-identical (acceptance R02-A12).
//!
//! ## Unstamped homes
//!
//! A home with NO stamp and NO journal is started only when it is
//! "provably new" (empty or contains exclusively this service's own
//! runtime scaffolding `lingxi-service/`); the gate then writes the v2
//! epoch-1 stamp. Any OTHER content makes the home refuse with
//! `unstamped-home-with-data`: adopting an existing (possibly legacy) home
//! is an explicit R08 cutover action, never an automatic startup decision
//! — the Rust service must not silently claim authority over data it does
//! not own yet. Symbolic links and interrupted `data-epoch*.json.tmp-*`
//! writes make the home ambiguous and are refused (mirroring the incumbent
//! coordinator).

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::Deserialize;

use crate::paths::atomic_write;

/// Fixed metadata file names at the data home root (same as
/// `shared/data-epoch.cjs`; cross-compatible with the incumbent kernel).
pub const STAMP_FILE_NAME: &str = "data-epoch.json";
pub const JOURNAL_FILE_NAME: &str = "data-epoch-transition.json";

/// This service's own runtime scaffolding directory (R02-T02 `paths.rs`):
/// the ONLY content an unstamped home may contain to count as provably new.
const RUNTIME_DIR_NAME: &str = crate::paths::RUNTIME_DIR_NAME;

/// Machine-readable stderr markers (same vocabulary as the incumbent gate
/// so a future desktop host can reuse the recognition logic).
pub const EPOCH_BLOCKED_MARKER: &str = "LINGXI_DATA_EPOCH_BLOCKED";
pub const EPOCH_TRANSITION_INCOMPLETE_MARKER: &str = "LINGXI_DATA_EPOCH_TRANSITION_INCOMPLETE";

pub const STAMP_SCHEMA_VERSION_V2: u64 = 2;
pub const JOURNAL_SCHEMA_VERSION: u64 = 1;
/// The seven forward-transition phases (membership mirrors
/// `shared/data-epoch.cjs`).
pub const JOURNAL_PHASES: [&str; 7] = [
    "prepared",
    "checkpoint_complete",
    "barrier_raised",
    "migrating",
    "migrated",
    "validated",
    "committed",
];

/// Failure reasons (superset of the incumbent coordinator's vocabulary;
/// `unstamped-home-with-data` and `stamp-write-failed` are new-stack
/// refusals documented in the module header).
pub const REASON_EPOCH_DOWNGRADE_BLOCKED: &str = "epoch-downgrade-blocked";
pub const REASON_CORRUPT_JOURNAL: &str = "corrupt-journal";
pub const REASON_CORRUPT_STAMP: &str = "corrupt-stamp";
pub const REASON_CORRUPT_TRANSITION: &str = "corrupt-transition";
pub const REASON_INCONSISTENT_TRANSITION_STATE: &str = "inconsistent-transition-state";
pub const REASON_INCOMPLETE_TRANSITION: &str = "incomplete-transition";
pub const REASON_AMBIGUOUS_UNSTAMPED_HOME: &str = "ambiguous-unstamped-home";
pub const REASON_UNSTAMPED_HOME_WITH_DATA: &str = "unstamped-home-with-data";
pub const REASON_STAMP_WRITE_FAILED: &str = "stamp-write-failed";
pub const REASON_TRANSITION_FAILED: &str = "transition-failed";
pub const REASON_MIGRATION_PATH_UNAVAILABLE: &str = "migration-path-unavailable";

// ── Timestamps (zero dependencies) ───────────────────────────────────────────

/// RFC-3339 timestamp check (the shape every writer in this repo produces:
/// ISO-8601 UTC from `toISOString`). Deliberately strict:
/// `YYYY-MM-DDTHH:MM:SS(.fff…)?(Z|z|±HH:MM)`.
pub fn is_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20 {
        return false;
    }
    let digits = |slice: &[u8]| slice.iter().all(|b| b.is_ascii_digit());
    if !digits(&bytes[0..4]) || bytes[4] != b'-' || !digits(&bytes[5..7]) || bytes[7] != b'-' {
        return false;
    }
    if !digits(&bytes[8..10]) || bytes[10] != b'T' || !digits(&bytes[11..13]) || bytes[13] != b':' {
        return false;
    }
    if !digits(&bytes[14..16]) || bytes[16] != b':' || !digits(&bytes[17..19]) {
        return false;
    }
    let mut rest = &bytes[19..];
    if rest.first() == Some(&b'.') {
        // Fractional seconds: at least one digit must follow the dot.
        let frac_len = rest[1..]
            .iter()
            .position(|b| !b.is_ascii_digit())
            .unwrap_or(rest.len() - 1);
        if frac_len == 0 {
            return false; // "." with no fractional digits
        }
        rest = &rest[frac_len + 1..];
    }
    match rest {
        [b'Z'] | [b'z'] => true,
        [sign, h1, h2, b':', m1, m2]
            if (*sign == b'+' || *sign == b'-')
                && h1.is_ascii_digit()
                && h2.is_ascii_digit()
                && m1.is_ascii_digit()
                && m2.is_ascii_digit() =>
        {
            true
        }
        _ => false,
    }
}

/// Formats `unix_ms` as an ISO-8601 UTC timestamp (`toISOString` shape,
/// millisecond precision) — the cross-compatible form both kernels write.
pub fn iso8601_utc(unix_ms: i64) -> String {
    let days = unix_ms.div_euclid(86_400_000);
    let millis_of_day = unix_ms.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    let hour = millis_of_day / 3_600_000;
    let minute = (millis_of_day % 3_600_000) / 60_000;
    let second = (millis_of_day % 60_000) / 1_000;
    let millis = millis_of_day % 1_000;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Howard Hinnant's `civil_from_days` (days since 1970-01-01 → y/m/d).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // year of era
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year
    let mp = (5 * doy + 2) / 153; // month index starting March
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

// ── On-disk formats ──────────────────────────────────────────────────────────

/// A validated epoch stamp (either format, normalized).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpochStamp {
    /// v1 stamps report `V1` (the on-disk value has no schemaVersion).
    pub format: StampFormat,
    pub minimum_reader_epoch: u64,
    pub committed_data_epoch: u64,
    pub last_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StampFormat {
    V1,
    V2,
}

impl fmt::Display for StampFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StampFormat::V1 => f.write_str("legacy-v1"),
            StampFormat::V2 => f.write_str("v2"),
        }
    }
}

impl EpochStamp {
    /// `minimum/committed` compact form used in diagnostics.
    pub fn state(&self) -> String {
        format!(
            "{}/{}",
            self.minimum_reader_epoch, self.committed_data_epoch
        )
    }
}

/// A validated forward-transition journal (the concrete higher-epoch
/// evidence that the incumbent gate drops on corrupt failures).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionJournal {
    pub transition_id: String,
    pub from_epoch: u64,
    pub to_epoch: u64,
    pub phase: String,
}

#[derive(Debug, Clone)]
pub enum StampRead {
    Missing,
    Ok(EpochStamp),
    Corrupt { detail: String },
}

#[derive(Debug, Clone)]
pub enum JournalRead {
    Missing,
    Ok(TransitionJournal),
    Corrupt { detail: String },
}

fn read_json_file(path: &Path) -> Result<Option<serde_json::Value>, String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.to_string()),
    };
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|err| err.to_string())
}

/// Reads + validates `data-epoch.json` (v2 first, legacy v1 fallback) with
/// the same validation matrix as `shared/data-epoch.cjs::readDataEpochStamp`.
pub fn read_stamp(home: &Path) -> StampRead {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Raw {
        schema_version: Option<serde_json::Value>,
        epoch: Option<serde_json::Value>,
        minimum_reader_epoch: Option<serde_json::Value>,
        committed_data_epoch: Option<serde_json::Value>,
        last_version: Option<serde_json::Value>,
        updated_at: Option<serde_json::Value>,
    }
    let path = home.join(STAMP_FILE_NAME);
    let value = match read_json_file(&path) {
        Ok(None) => return StampRead::Missing,
        Ok(Some(value)) => value,
        Err(detail) => return StampRead::Corrupt { detail },
    };
    let corrupt = |detail: &str| StampRead::Corrupt {
        detail: detail.to_string(),
    };
    if !value.is_object() {
        return corrupt("stamp must be a JSON object");
    }
    let raw: Raw = match serde_json::from_value(value.clone()) {
        Ok(raw) => raw,
        Err(_) => return corrupt("stamp must be a JSON object"),
    };
    let is_positive = |v: &Option<serde_json::Value>| {
        v.as_ref()
            .and_then(|x| x.as_u64())
            .map(|x| x >= 1)
            .unwrap_or(false)
    };
    if raw.schema_version.is_none() {
        // Legacy v1: `{epoch}` only.
        if !is_positive(&raw.epoch) {
            return corrupt("legacy stamp is missing a positive integer `epoch`");
        }
        if let Some(last_version) = &raw.last_version {
            if !last_version.is_null() && !last_version.is_string() {
                return corrupt("legacy stamp has an invalid `lastVersion`");
            }
        }
        if let Some(updated_at) = &raw.updated_at {
            if !updated_at.is_null() && !updated_at.as_str().map(is_timestamp).unwrap_or(false) {
                return corrupt("legacy stamp has an invalid `updatedAt`");
            }
        }
        let epoch = raw
            .epoch
            .as_ref()
            .and_then(|v| v.as_u64())
            .expect("checked");
        return StampRead::Ok(EpochStamp {
            format: StampFormat::V1,
            minimum_reader_epoch: epoch,
            committed_data_epoch: epoch,
            last_version: raw
                .last_version
                .as_ref()
                .and_then(|v| v.as_str())
                .map(str::to_string),
        });
    }
    if raw.schema_version.as_ref().and_then(|v| v.as_u64()) != Some(STAMP_SCHEMA_VERSION_V2) {
        return StampRead::Corrupt {
            detail: format!(
                "unsupported stamp schemaVersion: {}",
                raw.schema_version
                    .as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "null".into())
            ),
        };
    }
    if !is_positive(&raw.epoch) || !is_positive(&raw.minimum_reader_epoch) {
        return corrupt("v2 stamp requires positive integer `epoch` and `minimumReaderEpoch`");
    }
    let epoch = raw
        .epoch
        .as_ref()
        .and_then(|v| v.as_u64())
        .expect("checked");
    let minimum = raw
        .minimum_reader_epoch
        .as_ref()
        .and_then(|v| v.as_u64())
        .expect("checked");
    if epoch != minimum {
        return corrupt("v2 stamp requires `epoch` to equal `minimumReaderEpoch`");
    }
    if !is_positive(&raw.committed_data_epoch) {
        return corrupt("v2 stamp requires a positive integer `committedDataEpoch`");
    }
    let committed = raw
        .committed_data_epoch
        .as_ref()
        .and_then(|v| v.as_u64())
        .expect("checked");
    if committed > minimum {
        return corrupt("v2 stamp cannot commit a higher epoch than its minimum reader barrier");
    }
    let Some(last_version) = raw.last_version.as_ref().and_then(|v| v.as_str()) else {
        return corrupt("v2 stamp requires a non-empty `lastVersion`");
    };
    if last_version.is_empty() {
        return corrupt("v2 stamp requires a non-empty `lastVersion`");
    }
    if !raw
        .updated_at
        .as_ref()
        .and_then(|v| v.as_str())
        .map(is_timestamp)
        .unwrap_or(false)
    {
        return corrupt("v2 stamp requires a valid `updatedAt`");
    }
    StampRead::Ok(EpochStamp {
        format: StampFormat::V2,
        minimum_reader_epoch: minimum,
        committed_data_epoch: committed,
        last_version: Some(last_version.to_string()),
    })
}

/// Reads + validates `data-epoch-transition.json` (forward-transition
/// shape only — a restore-kind journal, like ANY other unrecognized shape,
/// reads back as `Corrupt` and fails closed, mirroring the incumbent
/// forward reader).
pub fn read_journal(home: &Path) -> JournalRead {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Raw {
        schema_version: Option<serde_json::Value>,
        transition_id: Option<String>,
        from_epoch: Option<serde_json::Value>,
        to_epoch: Option<serde_json::Value>,
        phase: Option<String>,
        migration_ids: Option<Vec<String>>,
        recovery_modes: Option<BTreeMap<String, String>>,
        affected_store_ids: Option<Vec<String>>,
        last_version: Option<serde_json::Value>,
        created_at: Option<serde_json::Value>,
        updated_at: Option<serde_json::Value>,
        checkpoint_id: Option<serde_json::Value>,
        checkpoint_receipt: Option<serde_json::Value>,
    }
    let path = home.join(JOURNAL_FILE_NAME);
    let value = match read_json_file(&path) {
        Ok(None) => return JournalRead::Missing,
        Ok(Some(value)) => value,
        Err(detail) => return JournalRead::Corrupt { detail },
    };
    let corrupt = |detail: String| JournalRead::Corrupt { detail };
    if !value.is_object() {
        return corrupt("transition journal must be a JSON object".to_string());
    }
    let raw: Raw = match serde_json::from_value(value) {
        Ok(raw) => raw,
        Err(_) => return corrupt("transition journal must be a JSON object".to_string()),
    };
    if raw.schema_version.as_ref().and_then(|v| v.as_u64()) != Some(JOURNAL_SCHEMA_VERSION) {
        return corrupt(format!(
            "unsupported transition journal schemaVersion: {}",
            raw.schema_version
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".into())
        ));
    }
    let Some(transition_id) = raw.transition_id.filter(|id| !id.is_empty()) else {
        return corrupt("transition journal requires a non-empty transitionId".to_string());
    };
    let is_positive = |v: &Option<serde_json::Value>| {
        v.as_ref()
            .and_then(|x| x.as_u64())
            .map(|x| x >= 1)
            .unwrap_or(false)
    };
    if !is_positive(&raw.from_epoch) || !is_positive(&raw.to_epoch) {
        return corrupt(
            "transition journal requires positive integer fromEpoch < toEpoch".to_string(),
        );
    }
    let from_epoch = raw
        .from_epoch
        .as_ref()
        .and_then(|v| v.as_u64())
        .expect("checked");
    let to_epoch = raw
        .to_epoch
        .as_ref()
        .and_then(|v| v.as_u64())
        .expect("checked");
    if from_epoch >= to_epoch {
        return corrupt("transition journal requires fromEpoch < toEpoch".to_string());
    }
    let Some(phase) = raw.phase else {
        return corrupt("transition journal requires a phase".to_string());
    };
    if !JOURNAL_PHASES.contains(&phase.as_str()) {
        return corrupt(format!("transition journal has an invalid phase: {phase}"));
    }
    let Some(migration_ids) = raw.migration_ids else {
        return corrupt("transition journal requires unique migrationIds".to_string());
    };
    let unique = {
        let mut sorted = migration_ids.clone();
        sorted.sort();
        sorted.dedup();
        sorted.len() == migration_ids.len()
    };
    if migration_ids.is_empty() || migration_ids.iter().any(String::is_empty) || !unique {
        return corrupt("transition journal requires unique migrationIds".to_string());
    }
    let Some(recovery_modes) = raw.recovery_modes else {
        return corrupt(
            "transition journal recoveryModes must exactly cover migrationIds".to_string(),
        );
    };
    let modes_valid = recovery_modes.len() == migration_ids.len()
        && migration_ids.iter().all(|id| {
            matches!(
                recovery_modes.get(id).map(String::as_str),
                Some("resume-idempotent") | Some("restore-only")
            )
        });
    if !modes_valid {
        return corrupt(
            "transition journal recoveryModes must exactly cover migrationIds".to_string(),
        );
    }
    let Some(store_ids) = raw.affected_store_ids else {
        return corrupt("transition journal requires unique affectedStoreIds".to_string());
    };
    if store_ids.is_empty() || store_ids.iter().any(String::is_empty) {
        return corrupt("transition journal requires unique affectedStoreIds".to_string());
    }
    if !raw
        .last_version
        .as_ref()
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false)
    {
        return corrupt("transition journal requires a non-empty lastVersion".to_string());
    }
    let valid_ts = |v: &Option<serde_json::Value>| {
        v.as_ref()
            .and_then(|x| x.as_str())
            .map(is_timestamp)
            .unwrap_or(false)
    };
    if !valid_ts(&raw.created_at) || !valid_ts(&raw.updated_at) {
        return corrupt("transition journal requires valid timestamps".to_string());
    }
    let checkpoint_required = phase != "prepared";
    if checkpoint_required {
        let receipt_ok = raw
            .checkpoint_id
            .as_ref()
            .and_then(|v| v.as_str())
            .map(|id| !id.is_empty())
            .unwrap_or(false)
            && raw
                .checkpoint_receipt
                .as_ref()
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_str())
                .zip(raw.checkpoint_id.as_ref().and_then(|v| v.as_str()))
                .map(|(receipt_id, id)| receipt_id == id)
                .unwrap_or(false);
        if !receipt_ok {
            return corrupt(format!(
                "transition journal phase {phase} requires a checkpoint receipt"
            ));
        }
    } else if !raw
        .checkpoint_id
        .as_ref()
        .map(|v| v.is_null())
        .unwrap_or(true)
        || !raw
            .checkpoint_receipt
            .as_ref()
            .map(|v| v.is_null())
            .unwrap_or(true)
    {
        return corrupt(
            "prepared transition journal must not claim a completed checkpoint".to_string(),
        );
    }
    JournalRead::Ok(TransitionJournal {
        transition_id,
        from_epoch,
        to_epoch,
        phase,
    })
}

// ── Gate decision ────────────────────────────────────────────────────────────

/// What the gate decided. `Proceed` is the ONLY path that reaches store
/// opening; every other outcome is a refusal (exit 2 in the binary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    /// Fresh (or freshly stamped) home: the gate wrote the v2 stamp itself.
    ProceedStampedNew(EpochStamp),
    /// A legacy-v1 stamp was refreshed to the v2 form (`adopted-legacy`
    /// equivalent at epoch 1) or the `lastVersion` moved.
    ProceedRefreshed(EpochStamp),
    /// The stamp already agrees with this build.
    ProceedSteady(EpochStamp),
    /// A completed transition journal tail was cleaned (mirrors the
    /// incumbent `committed-tail-cleaned`).
    ProceedCommittedTailCleaned(EpochStamp),
}

/// A refusal. Carries the machine marker, the reason and — the
/// PROD-DEFECT-1 fix — the readable journal as concrete evidence whenever
/// the journal itself parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpochGateBlock {
    pub marker: &'static str,
    pub reason: &'static str,
    pub detail: String,
    pub journal_evidence: Option<TransitionJournal>,
}

impl EpochGateBlock {
    fn blocked(reason: &'static str, detail: String) -> Self {
        Self {
            marker: EPOCH_BLOCKED_MARKER,
            reason,
            detail,
            journal_evidence: None,
        }
    }

    fn transition_incomplete(reason: &'static str, detail: String) -> Self {
        Self {
            marker: EPOCH_TRANSITION_INCOMPLETE_MARKER,
            reason,
            detail,
            journal_evidence: None,
        }
    }

    fn with_journal_evidence(mut self, journal: &TransitionJournal) -> Self {
        self.journal_evidence = Some(journal.clone());
        self
    }
}

impl fmt::Display for EpochGateBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} reason={}", self.marker, self.reason)?;
        if let Some(journal) = &self.journal_evidence {
            write!(
                f,
                " evidence=transitionJournal(fromEpoch={},toEpoch={},phase={},transitionId={})",
                journal.from_epoch, journal.to_epoch, journal.phase, journal.transition_id
            )?;
        }
        Ok(())
    }
}

/// Renders the full human-readable refusal (marker line first, then the
/// bilingual explanation, evidence quoted verbatim when present). The
/// rendered text is what the binary prints to stderr before exiting 2.
pub fn render_block(block: &EpochGateBlock) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}\n", block));
    out.push_str(&format!(
        "[data-epoch] The data safety gate refused startup ({}): {}\n",
        block.reason, block.detail
    ));
    if let Some(journal) = &block.journal_evidence {
        out.push_str(&format!(
            "[data-epoch] A READABLE transition journal is present in this data \
             home: epochs {}→{}, phase={}, transitionId={}. It is concrete \
             higher-epoch evidence, so startup is refused instead of guessing.\n",
            journal.from_epoch, journal.to_epoch, journal.phase, journal.transition_id
        ));
    }
    out.push_str(&format!(
        "[data-epoch] 数据安全闸拒绝启动（{}）：{}\n",
        block.reason, block.detail
    ));
    out.push_str(
        "[data-epoch] 请保留现场文件，使用维护/恢复流程处理；不要删除未知状态后强行启动。\n",
    );
    out
}

/// Classification of an unstamped home (mirror of the incumbent
/// `classifyUnstampedDataHome`, scoped to what the Rust service may own).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnstampedClassification {
    /// Empty or only this service's own runtime scaffolding.
    ProvablyNew,
    /// Anything else: refusing (the Rust service does not adopt foreign or
    /// legacy data automatically — that is an explicit R08 cutover action).
    WithData { detail: String },
    /// Symlinks or interrupted epoch-metadata temp writes: refusing.
    Ambiguous { detail: String },
}

fn classify_unstamped_home(home: &Path) -> Result<UnstampedClassification, String> {
    let entries = match std::fs::read_dir(home) {
        Ok(entries) => entries,
        Err(err) => {
            return Ok(UnstampedClassification::Ambiguous {
                detail: format!("cannot inspect data home: {err}"),
            })
        }
    };
    for entry in entries {
        let entry = entry.map_err(|err| format!("cannot inspect data home: {err}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("cannot inspect {name}: {err}"))?;
        if file_type.is_symlink() {
            return Ok(UnstampedClassification::Ambiguous {
                detail: format!("symbolic link found in unstamped home: {name}"),
            });
        }
        if name.starts_with("data-epoch") && name.contains(".tmp-") {
            return Ok(UnstampedClassification::Ambiguous {
                detail: format!("interrupted epoch metadata write found: {name}"),
            });
        }
        if name == STAMP_FILE_NAME || name == JOURNAL_FILE_NAME {
            // Covered by the readers above (both reported Missing to get
            // here); tolerate defensively.
            continue;
        }
        if name == RUNTIME_DIR_NAME {
            if file_type.is_dir() {
                continue;
            }
            return Ok(UnstampedClassification::Ambiguous {
                detail: format!("{name} exists but is not a directory"),
            });
        }
        return Ok(UnstampedClassification::WithData {
            detail: format!("unrecognized existing data found: {name}"),
        });
    }
    Ok(UnstampedClassification::ProvablyNew)
}

/// Writes the v2 stamp (the only file this gate ever creates). Atomic
/// write + fsync; the stamp is a state file, so a failed write is a loud
/// refusal, never a partial stamp under the real name.
fn write_stamp(
    home: &Path,
    minimum_reader_epoch: u64,
    committed_data_epoch: u64,
    own_version: &str,
    now_unix_ms: i64,
) -> Result<(), String> {
    let stamp = serde_json::json!({
        "schemaVersion": STAMP_SCHEMA_VERSION_V2,
        "epoch": minimum_reader_epoch,
        "minimumReaderEpoch": minimum_reader_epoch,
        "committedDataEpoch": committed_data_epoch,
        "lastVersion": own_version,
        "updatedAt": iso8601_utc(now_unix_ms),
    });
    let mut body = serde_json::to_string_pretty(&stamp).map_err(|err| err.to_string())?;
    body.push('\n');
    atomic_write(&home.join(STAMP_FILE_NAME), body.as_bytes()).map_err(|err| {
        format!(
            "cannot write {}: {err}",
            home.join(STAMP_FILE_NAME).display()
        )
    })
}

/// Removes the journal (committed-tail cleanup), fsyncing the directory so
/// the removal is durable (mirrors `removeDataEpochJournal`).
fn remove_journal(home: &Path) -> Result<(), String> {
    let path = home.join(JOURNAL_FILE_NAME);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.to_string()),
    }
    #[cfg(unix)]
    {
        if let Some(parent) = path.parent() {
            if let Ok(dir) = std::fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
    }
    Ok(())
}

/// The consistency matrix of journal phase vs stamp state (mirrors the
/// incumbent `transitionConsistency`).
fn transition_consistency(
    journal: &TransitionJournal,
    stamp: Option<&EpochStamp>,
) -> Result<bool, String> {
    // Returns Ok(target_committed) when the combination is valid, Err with
    // the detail otherwise.
    let source_steady = |s: &EpochStamp| {
        s.minimum_reader_epoch == journal.from_epoch && s.committed_data_epoch == journal.from_epoch
    };
    let barrier_raised = |s: &EpochStamp| {
        s.minimum_reader_epoch == journal.to_epoch && s.committed_data_epoch == journal.from_epoch
    };
    let target_committed = |s: &EpochStamp| {
        s.minimum_reader_epoch == journal.to_epoch && s.committed_data_epoch == journal.to_epoch
    };
    let valid = match stamp {
        None => matches!(journal.phase.as_str(), "prepared" | "checkpoint_complete"),
        Some(s) => match journal.phase.as_str() {
            "prepared" => source_steady(s),
            "checkpoint_complete" => source_steady(s) || barrier_raised(s),
            "committed" => target_committed(s),
            "validated" => barrier_raised(s) || target_committed(s),
            _ => barrier_raised(s),
        },
    };
    if !valid {
        let actual = stamp
            .map(|s| s.state())
            .unwrap_or_else(|| "missing".to_string());
        return Err(format!(
            "journal phase {} contradicts stamp state {}",
            journal.phase, actual
        ));
    }
    Ok(matches!(stamp, Some(s) if target_committed(s)))
}

/// Runs the data-epoch startup gate for `home` at `own_epoch`.
///
/// Journal-first (an interrupted transition may never be bypassed by any
/// override), fail-closed on every failure, journal evidence carried on
/// every corrupt-class refusal whose journal is readable. `now_unix_ms`
/// is injected so tests can drive stamp writes deterministically.
pub fn coordinate_data_epoch_startup(
    home: &Path,
    own_epoch: u64,
    own_version: &str,
    now_unix_ms: i64,
) -> Result<GateDecision, EpochGateBlock> {
    if own_epoch == 0 {
        return Err(EpochGateBlock::blocked(
            REASON_MIGRATION_PATH_UNAVAILABLE,
            "own_epoch must be a positive integer".to_string(),
        ));
    }
    if own_version.is_empty() {
        return Err(EpochGateBlock::blocked(
            REASON_MIGRATION_PATH_UNAVAILABLE,
            "own_version is required".to_string(),
        ));
    }

    // 1. Journal first: an interrupted transition is never bypassed.
    match read_journal(home) {
        JournalRead::Corrupt { detail } => {
            // The journal file cannot be trusted, so its epochs CANNOT be
            // claimed — the refusal says exactly that instead of inventing
            // either an absence or a presence of evidence.
            return Err(EpochGateBlock::transition_incomplete(
                REASON_CORRUPT_JOURNAL,
                format!(
                    "the transition journal file is unreadable ({detail}); its \
                     epochs cannot be trusted, so startup is refused instead of \
                     guessing"
                ),
            ));
        }
        JournalRead::Ok(journal) => {
            let stamp_read = read_stamp(home);
            let stamp_opt = match &stamp_read {
                StampRead::Ok(stamp) => Some(stamp.clone()),
                StampRead::Corrupt { detail } => {
                    // PROD-DEFECT-1 fix: the journal is READABLE — carry it
                    // as concrete higher-epoch evidence on the refusal.
                    return Err(EpochGateBlock::transition_incomplete(
                        REASON_CORRUPT_STAMP,
                        format!(
                            "the epoch stamp is corrupt ({detail}); a readable \
                             transition journal targeting epochs {}→{} is present, \
                             so startup is refused instead of guessing",
                            journal.from_epoch, journal.to_epoch
                        ),
                    )
                    .with_journal_evidence(&journal));
                }
                StampRead::Missing => None,
            };
            match transition_consistency(&journal, stamp_opt.as_ref()) {
                Err(detail) => {
                    // Journal readable, stamp state contradicts it: carry
                    // the evidence (this is the incumbent R10 fail-open
                    // shape — barrier_raised journal + missing stamp —
                    // which the new stack refuses).
                    return Err(EpochGateBlock::transition_incomplete(
                        REASON_CORRUPT_TRANSITION,
                        format!("{detail}; startup is refused instead of guessing"),
                    )
                    .with_journal_evidence(&journal));
                }
                Ok(true) => {
                    // phase=committed with the target stamp committed:
                    // ordinary startup cleans the tail (mirrors the
                    // incumbent `finalize-committed-tail`) and continues.
                    if own_epoch >= journal.to_epoch {
                        if let Err(err) = remove_journal(home) {
                            return Err(EpochGateBlock::transition_incomplete(
                                REASON_TRANSITION_FAILED,
                                format!("cannot remove the committed transition journal: {err}"),
                            )
                            .with_journal_evidence(&journal));
                        }
                        // Recurse once (journal now missing): the result is
                        // normalized to CommittedTailCleaned so the caller
                        // can see a tail cleanup happened.
                        return match coordinate_data_epoch_startup(
                            home,
                            own_epoch,
                            own_version,
                            now_unix_ms,
                        ) {
                            Ok(GateDecision::ProceedStampedNew(stamp))
                            | Ok(GateDecision::ProceedRefreshed(stamp))
                            | Ok(GateDecision::ProceedSteady(stamp))
                            | Ok(GateDecision::ProceedCommittedTailCleaned(stamp)) => {
                                Ok(GateDecision::ProceedCommittedTailCleaned(stamp))
                            }
                            Err(block) => Err(block),
                        };
                    }
                    return Err(EpochGateBlock::transition_incomplete(
                        REASON_INCOMPLETE_TRANSITION,
                        format!(
                            "transition is stopped at phase {}; this kernel is \
                             below the transition target",
                            journal.phase
                        ),
                    )
                    .with_journal_evidence(&journal));
                }
                Ok(false) => {
                    return Err(EpochGateBlock::transition_incomplete(
                        REASON_INCOMPLETE_TRANSITION,
                        format!("transition is stopped at phase {}", journal.phase),
                    )
                    .with_journal_evidence(&journal));
                }
            }
        }
        JournalRead::Missing => {}
    }

    // 2. No journal: the stamp decides.
    match read_stamp(home) {
        StampRead::Corrupt { detail } => {
            // No journal, unreadable stamp: the data version cannot be
            // established. The refusal states what is unreadable; it does
            // NOT claim that higher-epoch evidence is absent.
            Err(EpochGateBlock::transition_incomplete(
                REASON_CORRUPT_STAMP,
                format!(
                    "the epoch stamp is unreadable ({detail}) and no transition \
                     journal is present; the data version cannot be established, \
                     so startup is refused instead of guessing"
                ),
            ))
        }
        StampRead::Missing => match classify_unstamped_home(home) {
            Err(detail) => Err(EpochGateBlock::blocked(
                REASON_MIGRATION_PATH_UNAVAILABLE,
                format!("cannot inspect the data home: {detail}"),
            )),
            Ok(UnstampedClassification::Ambiguous { detail }) => Err(EpochGateBlock::blocked(
                REASON_AMBIGUOUS_UNSTAMPED_HOME,
                detail,
            )),
            Ok(UnstampedClassification::WithData { detail }) => Err(EpochGateBlock::blocked(
                REASON_UNSTAMPED_HOME_WITH_DATA,
                format!(
                    "{detail}; this data home carries no epoch stamp, and \
                             adopting existing data is an explicit cutover action \
                             (R08), never an automatic startup decision"
                ),
            )),
            Ok(UnstampedClassification::ProvablyNew) => {
                match write_stamp(home, own_epoch, own_epoch, own_version, now_unix_ms) {
                    Ok(()) => Ok(GateDecision::ProceedStampedNew(EpochStamp {
                        format: StampFormat::V2,
                        minimum_reader_epoch: own_epoch,
                        committed_data_epoch: own_epoch,
                        last_version: Some(own_version.to_string()),
                    })),
                    Err(detail) => Err(EpochGateBlock::blocked(
                        REASON_STAMP_WRITE_FAILED,
                        format!(
                            "cannot write the epoch stamp for the fresh data \
                                 home ({detail}); startup is refused (no partial \
                                 stamp is left under the real name)"
                        ),
                    )),
                }
            }
        },
        StampRead::Ok(stamp) => {
            if stamp.committed_data_epoch < stamp.minimum_reader_epoch {
                // An uncommitted reader barrier with NO journal is a
                // contradictory state (the journal is what authorizes a
                // barrier).
                return Err(EpochGateBlock::transition_incomplete(
                    REASON_INCONSISTENT_TRANSITION_STATE,
                    "stamp has an uncommitted reader barrier but no transition \
                     journal"
                        .to_string(),
                ));
            }
            if stamp.minimum_reader_epoch > own_epoch {
                return Err(EpochGateBlock::blocked(
                    REASON_EPOCH_DOWNGRADE_BLOCKED,
                    format!(
                        "this data home requires a kernel at data epoch {} or \
                         newer{}; this kernel is epoch {own_epoch}. Upgrade the \
                         service — the new stack refuses instead of opening a \
                         possibly newer data format with an older kernel",
                        stamp.minimum_reader_epoch,
                        stamp
                            .last_version
                            .as_ref()
                            .map(|v| format!(" (last opened by version {v})"))
                            .unwrap_or_default(),
                    ),
                ));
            }
            if stamp.committed_data_epoch < own_epoch {
                // This build would have to run a transition; R02 has no
                // migration registry (DATA_EPOCH=1), so the honest answer
                // is refusal, not a guessed upgrade.
                return Err(EpochGateBlock::blocked(
                    REASON_MIGRATION_PATH_UNAVAILABLE,
                    format!(
                        "the stamp commits epoch {} but this build is epoch \
                         {own_epoch}; no epoch migrations are registered in \
                         this build, so the transition cannot be performed",
                        stamp.committed_data_epoch
                    ),
                ));
            }
            let should_refresh = stamp.format == StampFormat::V1
                || stamp.last_version.as_deref() != Some(own_version);
            if should_refresh {
                write_stamp(home, own_epoch, own_epoch, own_version, now_unix_ms).map_err(
                    |detail| {
                        EpochGateBlock::blocked(
                            REASON_STAMP_WRITE_FAILED,
                            format!("cannot refresh the epoch stamp ({detail})"),
                        )
                    },
                )?;
            }
            Ok(if should_refresh {
                GateDecision::ProceedRefreshed(EpochStamp {
                    format: StampFormat::V2,
                    minimum_reader_epoch: own_epoch,
                    committed_data_epoch: own_epoch,
                    last_version: Some(own_version.to_string()),
                })
            } else {
                GateDecision::ProceedSteady(stamp)
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lingxi-r02t06-gate-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp home");
        dir
    }

    fn write_raw(path: &std::path::Path, body: &str) {
        std::fs::write(path, body).expect("write raw file");
    }

    fn barrier_journal_json(from: u64, to: u64, phase: &str) -> String {
        format!(
            r#"{{
  "schemaVersion": 1,
  "transitionId": "transition-{from}-{to}",
  "fromEpoch": {from},
  "toEpoch": {to},
  "migrationIds": ["preferences-{from}-to-{to}"],
  "affectedStoreIds": ["user-preferences"],
  "recoveryModes": {{ "preferences-{from}-to-{to}": "restore-only" }},
  "phase": "{phase}",
  "checkpointId": "checkpoint-{from}-{to}",
  "checkpointReceipt": {{ "id": "checkpoint-{from}-{to}" }},
  "createdAt": "2026-09-26T08:00:00.000Z",
  "updatedAt": "2026-09-26T08:00:00.000Z",
  "lastVersion": "2.0.0"
}}"#
        )
    }

    fn stamp_v2_json(minimum: u64, committed: u64) -> String {
        format!(
            r#"{{
  "schemaVersion": 2,
  "epoch": {minimum},
  "minimumReaderEpoch": {minimum},
  "committedDataEpoch": {committed},
  "lastVersion": "9.9.9",
  "updatedAt": "2026-09-26T08:00:00.000Z"
}}"#
        )
    }

    #[test]
    fn timestamps_round_trip_and_validate() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00.000Z");
        // 2026-09-26T08:00:00Z == 1790409600000 ms (verified against UTC).
        assert_eq!(iso8601_utc(1_790_409_600_000), "2026-09-26T08:00:00.000Z");
        assert!(is_timestamp("2026-09-26T08:00:00.000Z"));
        assert!(is_timestamp("2026-09-26T08:00:00Z"));
        assert!(is_timestamp("2026-09-26T08:00:00.123456Z"));
        assert!(is_timestamp("2026-09-26T08:00:00+02:00"));
        assert!(!is_timestamp("2026-09-26 08:00:00"));
        assert!(!is_timestamp("not a date"));
        assert!(!is_timestamp(""));
    }

    #[test]
    fn fresh_home_is_stamped_and_subsequent_run_is_steady() {
        let home = temp_home("fresh");
        let decision =
            coordinate_data_epoch_startup(&home, 1, "0.4.0", 1_790_409_600_000).expect("proceed");
        assert!(matches!(decision, GateDecision::ProceedStampedNew(_)));
        let stamp = read_stamp(&home);
        let StampRead::Ok(stamp) = stamp else {
            panic!("stamp must be readable now");
        };
        assert_eq!(stamp.minimum_reader_epoch, 1);
        assert_eq!(stamp.committed_data_epoch, 1);
        assert_eq!(stamp.format, StampFormat::V2);
        // Idempotent: same version → steady.
        let again =
            coordinate_data_epoch_startup(&home, 1, "0.4.0", 1_790_409_600_000).expect("proceed");
        assert!(matches!(again, GateDecision::ProceedSteady(_)));
        // Different version → refreshed.
        let refreshed =
            coordinate_data_epoch_startup(&home, 1, "0.5.0", 1_790_409_600_000).expect("proceed");
        assert!(matches!(refreshed, GateDecision::ProceedRefreshed(_)));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn corrupt_stamp_with_readable_higher_epoch_journal_refuses_with_truthful_evidence() {
        // PROD-DEFECT-1 variant R7 (torn stamp + readable barrier_raised
        // journal 1→2): the incumbent gate fails open and lies about the
        // evidence; this gate refuses AND names the evidence.
        let home = temp_home("r7");
        write_raw(&home.join(STAMP_FILE_NAME), "{ \"epoch\": 1, \"dat"); // torn
        write_raw(
            &home.join(JOURNAL_FILE_NAME),
            &barrier_journal_json(1, 2, "barrier_raised"),
        );
        let before_stamp = std::fs::read(home.join(STAMP_FILE_NAME)).unwrap();
        let before_journal = std::fs::read(home.join(JOURNAL_FILE_NAME)).unwrap();

        let block = coordinate_data_epoch_startup(&home, 1, "0.4.0", 0).expect_err("must refuse");
        assert_eq!(block.marker, EPOCH_TRANSITION_INCOMPLETE_MARKER);
        assert_eq!(block.reason, REASON_CORRUPT_STAMP);
        let journal = block
            .journal_evidence
            .as_ref()
            .expect("readable journal must be carried as evidence");
        assert_eq!(journal.from_epoch, 1);
        assert_eq!(journal.to_epoch, 2);
        assert_eq!(journal.phase, "barrier_raised");

        let text = render_block(&block);
        assert!(
            !text.contains("no higher-epoch evidence was found"),
            "the incumbent lie must never appear: {text}"
        );
        assert!(text.contains("1→2"), "evidence must be stated: {text}");
        assert!(text.contains("barrier_raised"), "{text}");

        // Refusal leaves every pre-existing file byte-identical.
        assert_eq!(
            std::fs::read(home.join(STAMP_FILE_NAME)).unwrap(),
            before_stamp
        );
        assert_eq!(
            std::fs::read(home.join(JOURNAL_FILE_NAME)).unwrap(),
            before_journal
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn corrupt_stamp_without_journal_refuses_without_claiming_absence() {
        let home = temp_home("r8");
        write_raw(&home.join(STAMP_FILE_NAME), "GARBAGE");
        let block = coordinate_data_epoch_startup(&home, 1, "0.4.0", 0).expect_err("must refuse");
        assert_eq!(block.reason, REASON_CORRUPT_STAMP);
        assert!(block.journal_evidence.is_none());
        let text = render_block(&block);
        assert!(
            !text.contains("no higher-epoch evidence was found"),
            "{text}"
        );
        assert!(
            text.contains("the epoch stamp is unreadable"),
            "text must state what is unreadable: {text}"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn corrupt_journal_refuses() {
        // PROD-DEFECT-1 variant R9 (torn journal).
        let home = temp_home("r9");
        write_raw(&home.join(STAMP_FILE_NAME), &stamp_v2_json(1, 1));
        write_raw(
            &home.join(JOURNAL_FILE_NAME),
            "{ \"schemaVersion\": 1, \"fro",
        );
        let before = std::fs::read(home.join(JOURNAL_FILE_NAME)).unwrap();
        let block = coordinate_data_epoch_startup(&home, 1, "0.4.0", 0).expect_err("must refuse");
        assert_eq!(block.reason, REASON_CORRUPT_JOURNAL);
        assert!(block.journal_evidence.is_none());
        assert_eq!(std::fs::read(home.join(JOURNAL_FILE_NAME)).unwrap(), before);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn missing_stamp_with_readable_higher_epoch_journal_refuses_with_truthful_evidence() {
        // PROD-DEFECT-1 variant R10 — THE incumbent fail-open shape
        // (barrier_raised journal, no stamp): Node started and really
        // wrote; this gate refuses and prints the evidence.
        let home = temp_home("r10");
        write_raw(
            &home.join(JOURNAL_FILE_NAME),
            &barrier_journal_json(1, 2, "barrier_raised"),
        );
        let before = std::fs::read(home.join(JOURNAL_FILE_NAME)).unwrap();
        let block = coordinate_data_epoch_startup(&home, 1, "0.4.0", 0).expect_err("must refuse");
        assert_eq!(block.reason, REASON_CORRUPT_TRANSITION);
        let text = render_block(&block);
        assert_eq!(
            block.journal_evidence.expect("evidence carried").to_epoch,
            2
        );
        assert!(!text.contains("no higher-epoch evidence was found"));
        assert!(text.contains("epochs 1→2"));
        assert!(!home.join(STAMP_FILE_NAME).exists(), "no stamp invented");
        assert_eq!(std::fs::read(home.join(JOURNAL_FILE_NAME)).unwrap(), before);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn missing_stamp_with_prepared_higher_epoch_journal_refuses() {
        // Variant R11 (incumbent refused too; here for completeness).
        let home = temp_home("r11");
        let prepared = barrier_journal_json(1, 2, "prepared").replace(
            "  \"checkpointId\": \"checkpoint-1-2\",\n  \"checkpointReceipt\": { \"id\": \"checkpoint-1-2\" },\n",
            "  \"checkpointId\": null,\n  \"checkpointReceipt\": null,\n",
        );
        write_raw(&home.join(JOURNAL_FILE_NAME), &prepared);
        let block = coordinate_data_epoch_startup(&home, 1, "0.4.0", 0).expect_err("must refuse");
        assert_eq!(block.reason, REASON_INCOMPLETE_TRANSITION);
        assert_eq!(block.journal_evidence.expect("readable").phase, "prepared");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn higher_stamp_refuses_startup() {
        let home = temp_home("downgrade");
        write_raw(&home.join(STAMP_FILE_NAME), &stamp_v2_json(2, 2));
        let block = coordinate_data_epoch_startup(&home, 1, "0.4.0", 0).expect_err("must refuse");
        assert_eq!(block.marker, EPOCH_BLOCKED_MARKER);
        assert_eq!(block.reason, REASON_EPOCH_DOWNGRADE_BLOCKED);
        let text = render_block(&block);
        assert!(text.contains("epoch 2 or newer"), "{text}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn unstamped_home_with_foreign_data_refuses_without_stamping() {
        let home = temp_home("unstamped-data");
        std::fs::create_dir_all(home.join("session-jsonl")).expect("legacy dir");
        std::fs::write(home.join("session-jsonl/a.jsonl"), b"x").unwrap();
        let block = coordinate_data_epoch_startup(&home, 1, "0.4.0", 0).expect_err("must refuse");
        assert_eq!(block.reason, REASON_UNSTAMPED_HOME_WITH_DATA);
        assert!(!home.join(STAMP_FILE_NAME).exists(), "no stamp written");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn unstamped_home_with_own_runtime_scaffolding_is_provably_new() {
        let home = temp_home("unstamped-own");
        std::fs::create_dir_all(home.join("lingxi-service")).expect("runtime dir");
        let decision = coordinate_data_epoch_startup(&home, 1, "0.4.0", 1_790_409_600_000)
            .expect("own scaffolding only");
        assert!(matches!(decision, GateDecision::ProceedStampedNew(_)));
        assert!(home.join(STAMP_FILE_NAME).exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn symlink_in_unstamped_home_is_ambiguous() {
        #[cfg(unix)]
        {
            let home = temp_home("symlink");
            std::os::unix::fs::symlink("/tmp/elsewhere", home.join("link")).unwrap();
            let block =
                coordinate_data_epoch_startup(&home, 1, "0.4.0", 0).expect_err("must refuse");
            assert_eq!(block.reason, REASON_AMBIGUOUS_UNSTAMPED_HOME);
            let _ = std::fs::remove_dir_all(&home);
        }
    }

    #[test]
    fn interrupted_epoch_metadata_tmp_write_is_ambiguous() {
        let home = temp_home("tmp-write");
        write_raw(&home.join("data-epoch.json.tmp-4242-abc"), "{");
        let block = coordinate_data_epoch_startup(&home, 1, "0.4.0", 0).expect_err("must refuse");
        assert_eq!(block.reason, REASON_AMBIGUOUS_UNSTAMPED_HOME);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn committed_tail_is_cleaned_and_gate_continues() {
        let home = temp_home("committed-tail");
        write_raw(&home.join(STAMP_FILE_NAME), &stamp_v2_json(2, 2));
        write_raw(
            &home.join(JOURNAL_FILE_NAME),
            &barrier_journal_json(1, 2, "committed"),
        );
        let decision = coordinate_data_epoch_startup(&home, 2, "2.0.0", 1_790_409_600_000)
            .expect("committed tail cleans");
        assert!(matches!(
            decision,
            GateDecision::ProceedCommittedTailCleaned(_)
        ));
        assert!(!home.join(JOURNAL_FILE_NAME).exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn barrier_stamp_without_journal_is_inconsistent() {
        let home = temp_home("barrier-no-journal");
        write_raw(&home.join(STAMP_FILE_NAME), &stamp_v2_json(2, 1));
        let block = coordinate_data_epoch_startup(&home, 2, "2.0.0", 0).expect_err("must refuse");
        assert_eq!(block.reason, REASON_INCONSISTENT_TRANSITION_STATE);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[cfg(unix)]
    #[test]
    fn stamp_write_failure_is_a_loud_refusal_without_partial_stamp() {
        use std::os::unix::fs::PermissionsExt as _;
        let home = temp_home("stamp-write-fail");
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o555))
            .expect("chmod home read-only");
        let block = coordinate_data_epoch_startup(&home, 1, "0.4.0", 0).expect_err("must refuse");
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o755))
            .expect("restore perms");
        assert_eq!(block.reason, REASON_STAMP_WRITE_FAILED);
        assert!(!home.join(STAMP_FILE_NAME).exists(), "no partial stamp");
        let leftovers: Vec<String> = std::fs::read_dir(&home)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            leftovers.iter().all(|n| !n.contains("data-epoch.json")),
            "no stamp artifacts at all: {leftovers:?}"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_incumbent_lie_string_never_appears_in_rendered_refusals() {
        // Freeze the truthfulness contract over a matrix of refusals.
        let mut homes = Vec::new();
        let mut mk = |tag: &str| {
            let home = temp_home(tag);
            homes.push(home.clone());
            home
        };
        let cases: Vec<(String, EpochGateBlock)> = vec![
            {
                let home = mk("lie-r7");
                write_raw(&home.join(STAMP_FILE_NAME), "{ torn");
                write_raw(
                    &home.join(JOURNAL_FILE_NAME),
                    &barrier_journal_json(1, 2, "barrier_raised"),
                );
                let b = coordinate_data_epoch_startup(&home, 1, "0.4.0", 0).unwrap_err();
                (render_block(&b), b)
            },
            {
                let home = mk("lie-r10");
                write_raw(
                    &home.join(JOURNAL_FILE_NAME),
                    &barrier_journal_json(1, 2, "barrier_raised"),
                );
                let b = coordinate_data_epoch_startup(&home, 1, "0.4.0", 0).unwrap_err();
                (render_block(&b), b)
            },
            {
                let home = mk("lie-r9");
                write_raw(&home.join(STAMP_FILE_NAME), &stamp_v2_json(1, 1));
                write_raw(&home.join(JOURNAL_FILE_NAME), "{ torn");
                let b = coordinate_data_epoch_startup(&home, 1, "0.4.0", 0).unwrap_err();
                (render_block(&b), b)
            },
        ];
        for (text, block) in cases {
            assert!(
                !text.contains("no higher-epoch evidence was found"),
                "reason {}: the incumbent lie leaked: {text}",
                block.reason
            );
            if block.journal_evidence.is_some() {
                assert!(
                    text.contains("→"),
                    "evidence-bearing refusal must name the epochs: {text}"
                );
            }
        }
        for home in homes {
            let _ = std::fs::remove_dir_all(&home);
        }
    }
}

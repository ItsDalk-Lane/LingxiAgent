//! The normalized usage/trace vocabulary of ONE model call (R05-T07).
//!
//! The wire type [`lingxi_protocol::UsageRecord`] stays the frozen
//! `{inputTokens, outputTokens}` projection the `model_call_completed`
//! event carries; THIS module owns the richer fact the usage ledger
//! persists — the per-protocol component tokens (cache read/write,
//! reasoning), their inclusion semantics (documented per family in
//! `docs/rust-tauri/R05/MODEL_USAGE_SEMANTICS.md`, never guessed), and
//! the provenance of every number:
//!
//! - `Reported` — the provider's own usage object, complete on both
//!   totals;
//! - `Partial` — the provider reported PART of the usage (e.g. the input
//!   half of an interrupted stream): the known half is kept, the missing
//!   half is `None` — never zero, never an estimate;
//! - `Estimated` — a host-derived estimate with its basis named (never
//!   presented as a provider number);
//! - absent/`Invalid` — no usable usage fact at all, or a usage object
//!   whose numbers violate the contract (negative, non-integer,
//!   overflow): the REQUEST fact is still recorded (a possibly-billable
//!   request must never vanish because its usage was garbage), the
//!   numbers are not.
//!
//! Aggregation (taskbook T07-C04): protocols differ in whether a usage
//! frame is the request's FINAL snapshot, a RUNNING cumulative total, or
//! an input-half that a later frame completes. [`UsageFolder`] implements
//! the two streaming modes with one rule each — an IDENTICAL repeat is an
//! idempotent no-op (a re-transmitted usage never double-counts), a
//! CONFLICT (a different final snapshot, or a running total that goes
//! backwards) is loud. Nothing here ever sums a cumulative snapshot.

use lingxi_protocol::UsageRecord;

/// Provenance of the numbers of one [`ModelCallUsage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageProvenance {
    /// The provider's usage object for the request, complete on both
    /// totals.
    Reported,
    /// The provider reported part of the usage; `missing` names the
    /// fields that are NOT known (their slots stay `None`).
    Partial { missing: Vec<&'static str> },
    /// Host-derived estimate; `basis` names how it was derived. Never
    /// presented as a provider number.
    Estimated { basis: String },
}

impl UsageProvenance {
    /// Stable ledger vocabulary of the provenance.
    pub fn state_name(&self) -> &'static str {
        match self {
            UsageProvenance::Reported => "reported",
            UsageProvenance::Partial { .. } => "partial",
            UsageProvenance::Estimated { .. } => "estimated",
        }
    }
}

/// The normalized usage of ONE model call: the totals the wire record
/// projects, plus the component tokens and their per-family semantics.
/// Every field is optional EXCEPT via provenance: a `None` component
/// means "this protocol response did not report it", never zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCallUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// Prompt tokens served from a provider cache (Anthropic
    /// `cache_read_input_tokens`, OpenAI `prompt_tokens_details.cached_tokens`,
    /// Gemini `cachedContentTokenCount`).
    pub cache_read_tokens: Option<u64>,
    /// Prompt tokens written INTO a provider cache (Anthropic
    /// `cache_creation_input_tokens`; the OpenAI/Gemini families have no
    /// such field).
    pub cache_write_tokens: Option<u64>,
    /// Reasoning/thought tokens (OpenAI
    /// `completion_tokens_details.reasoning_tokens` /
    /// `output_tokens_details.reasoning_tokens`, Gemini
    /// `thoughtsTokenCount`).
    pub reasoning_tokens: Option<u64>,
    pub provenance: UsageProvenance,
}

impl ModelCallUsage {
    /// A fully reported usage without component detail (the shape a
    /// protocol double or a legacy `UsageRecord` projects).
    pub fn reported(input_tokens: u64, output_tokens: u64) -> Self {
        Self {
            input_tokens: Some(input_tokens),
            output_tokens: Some(output_tokens),
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            provenance: UsageProvenance::Reported,
        }
    }

    /// The frozen wire projection: `Some` only when BOTH totals are
    /// known (a half-known usage never projects a number that could be
    /// read as the request's real usage).
    pub fn wire_record(&self) -> Option<UsageRecord> {
        match (self.input_tokens, self.output_tokens) {
            (Some(input_tokens), Some(output_tokens)) => Some(UsageRecord {
                input_tokens,
                output_tokens,
            }),
            _ => None,
        }
    }
}

impl From<UsageRecord> for ModelCallUsage {
    /// The legacy projection is a REPORTED complete usage without
    /// component detail (that is exactly what the type asserted).
    fn from(record: UsageRecord) -> Self {
        Self::reported(record.input_tokens, record.output_tokens)
    }
}

/// What ONE provider turn resolved with, usage-wise (R05-T07): the three
/// honest states of "did the provider account this request".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportedUsage {
    /// No usable usage fact arrived (missing usage, or a request that
    /// failed before any usage could exist). NOT zero.
    Unknown,
    /// A usable (complete or partial) usage fact.
    Known(ModelCallUsage),
    /// A usage object arrived but its numbers violate the contract
    /// (negative token counts, non-integers, out-of-`u64` magnitudes).
    /// The violation is named; the numbers are NOT trusted — no
    /// truncation, no saturation, no zero.
    Invalid { detail: String },
}

impl ReportedUsage {
    /// The ledger vocabulary of the state.
    pub fn state_name(&self) -> &'static str {
        match self {
            ReportedUsage::Unknown => "unknown",
            ReportedUsage::Known(usage) => usage.provenance.state_name(),
            ReportedUsage::Invalid { .. } => "invalid",
        }
    }
}

impl From<ModelCallUsage> for ReportedUsage {
    fn from(usage: ModelCallUsage) -> Self {
        ReportedUsage::Known(usage)
    }
}

/// How a protocol's usage frames aggregate within ONE request
/// (taskbook T07-C04). The mode is a fixed fact of the family, declared
/// in the family's usage mapping — never inferred from the values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageAggregationMode {
    /// Each usage frame is the request's FINAL snapshot (the OpenAI
    /// final chunk's `usage`, the Responses `response.completed` usage,
    /// every buffered response). One frame replaces nothing; an
    /// IDENTICAL repeat is an idempotent no-op; a DIFFERENT second
    /// snapshot is a loud conflict.
    FinalSnapshot,
    /// Frames carry a RUNNING cumulative total (Anthropic
    /// `message_delta.usage.output_tokens`, Gemini streaming
    /// `usageMetadata`): a later frame REPLACES an earlier one when it is
    /// consistent-and-not-smaller per known field; going backwards is a
    /// loud conflict. Never summed.
    RunningTotal,
}

/// A usage fold conflict (loud, never merged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageConflict(pub String);

impl std::fmt::Display for UsageConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for UsageConflict {}

/// Folds the usage frames of ONE request per the family's aggregation
/// mode. Pure; the protocol accumulators own it so buffered and streamed
/// modes share one contract.
///
/// Rules (identical in both modes unless stated):
/// - an IDENTICAL repeat of the current state is an idempotent no-op
///   (a re-transmitted usage never double-counts — T07-C03/C04);
/// - `FinalSnapshot`: a DIFFERENT second snapshot is a loud conflict;
/// - `RunningTotal`: a later frame replaces an earlier one when every
///   known field is `>=` the current value (a running total may grow);
///   any field going backwards (or a set field changing to a smaller
///   value) is a loud conflict;
/// - merging an input-half frame (totals `None` on the output side) with
///   a later output frame keeps both halves (an interrupted stream then
///   holds a `Partial` fact, never a fabricated zero).
#[derive(Debug, Clone)]
pub struct UsageFolder {
    mode: UsageAggregationMode,
    current: Option<ModelCallUsage>,
}

impl UsageFolder {
    pub fn new(mode: UsageAggregationMode) -> Self {
        Self {
            mode,
            current: None,
        }
    }

    /// Folds one usage fact. The fact's provenance is IGNORED here — the
    /// folder merges NUMBERS; provenance of the folded result is
    /// recomputed by [`UsageFolder::finish`] from what is actually known.
    pub fn fold(&mut self, fact: ModelCallUsage) -> Result<(), UsageConflict> {
        let Some(current) = self.current.clone() else {
            self.current = Some(fact);
            return Ok(());
        };
        if current == fact {
            return Ok(());
        }
        match self.mode {
            UsageAggregationMode::FinalSnapshot => Err(UsageConflict(format!(
                "two different final usage snapshots for one request ({:?} then {:?}): \
                 a conflict is never merged",
                wire_of(&current),
                wire_of(&fact),
            ))),
            UsageAggregationMode::RunningTotal => {
                // A later running frame may grow known fields and may FILL
                // previously-unknown ones; nothing may shrink and no known
                // field may change to a different value while staying
                // equal-or-larger is impossible (fields are only replaced
                // by growth).
                let shrunk = shrinks(&current.input_tokens, &fact.input_tokens)
                    || shrinks(&current.output_tokens, &fact.output_tokens)
                    || shrinks(&current.cache_read_tokens, &fact.cache_read_tokens)
                    || shrinks(&current.cache_write_tokens, &fact.cache_write_tokens)
                    || shrinks(&current.reasoning_tokens, &fact.reasoning_tokens);
                if shrunk {
                    return Err(UsageConflict(format!(
                        "a running usage total went backwards ({:?} then {:?}): \
                         a conflict is never merged",
                        wire_of(&current),
                        wire_of(&fact),
                    )));
                }
                self.current = Some(merge_later(current, fact));
                Ok(())
            }
        }
    }

    /// The folded usage with provenance recomputed from what is actually
    /// known: both totals known → `Reported`; exactly one total known →
    /// `Partial` naming the missing half; none → `Partial` naming both.
    /// Component-only knowledge stays `Partial` too (a component without
    /// its totals is not a complete account).
    pub fn finish(self) -> Option<ModelCallUsage> {
        let mut usage = self.current?;
        let mut missing: Vec<&'static str> = Vec::new();
        if usage.input_tokens.is_none() {
            missing.push("input_tokens");
        }
        if usage.output_tokens.is_none() {
            missing.push("output_tokens");
        }
        usage.provenance = if missing.is_empty() {
            UsageProvenance::Reported
        } else {
            UsageProvenance::Partial { missing }
        };
        Some(usage)
    }
}

/// `true` when a known value got smaller (`None` on the later fact keeps
/// the earlier knowledge — a later frame may simply not repeat it).
fn shrinks(earlier: &Option<u64>, later: &Option<u64>) -> bool {
    match (earlier, later) {
        (Some(earlier), Some(later)) => later < earlier,
        _ => false,
    }
}

/// Merges a later running frame into the current knowledge: later values
/// win where present (the folder already refused any shrink), earlier
/// knowledge survives where the later frame is silent.
fn merge_later(mut current: ModelCallUsage, later: ModelCallUsage) -> ModelCallUsage {
    if later.input_tokens.is_some() {
        current.input_tokens = later.input_tokens;
    }
    if later.output_tokens.is_some() {
        current.output_tokens = later.output_tokens;
    }
    if later.cache_read_tokens.is_some() {
        current.cache_read_tokens = later.cache_read_tokens;
    }
    if later.cache_write_tokens.is_some() {
        current.cache_write_tokens = later.cache_write_tokens;
    }
    if later.reasoning_tokens.is_some() {
        current.reasoning_tokens = later.reasoning_tokens;
    }
    current
}

fn wire_of(usage: &ModelCallUsage) -> (Option<u64>, Option<u64>) {
    (usage.input_tokens, usage.output_tokens)
}

/// The settlement outcome of the CALL a ledger row accounts (R05 RR1
/// F21). The usage fact and the call outcome are SEPARATE facts: a call
/// can fail with a perfectly reported usage object (the provider
/// answered 500 WITH usage), and a succeeded call can carry an unknown
/// usage. `Unknown` is the honest state of a row written for a call
/// whose settlement was never observed (crash-window recovery rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome {
    Succeeded,
    Failed,
    /// The call was cancelled before its settlement was observed. A
    /// cancellation fence never erases the accounting row of a request
    /// that may already have left the process.
    Cancelled,
    Unknown,
}

impl CallOutcome {
    /// Stable ledger vocabulary (the `outcome` column).
    pub fn wire_name(self) -> &'static str {
        match self {
            CallOutcome::Succeeded => "succeeded",
            CallOutcome::Failed => "failed",
            CallOutcome::Cancelled => "cancelled",
            CallOutcome::Unknown => "unknown",
        }
    }

    /// Parses the storage vocabulary (unknown values are corruption,
    /// never a guess).
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "succeeded" => Some(CallOutcome::Succeeded),
            "failed" => Some(CallOutcome::Failed),
            "cancelled" => Some(CallOutcome::Cancelled),
            "unknown" => Some(CallOutcome::Unknown),
            _ => None,
        }
    }
}

/// One row of the durable usage ledger (R05-T07): the per-request
/// correlation of a model call — identity (session/run/attempt/call),
/// route (provider/model/protocol), purpose, causal parentage (origin +
/// parent run + cause ref) and the usage fact itself. Written BEFORE the
/// call's completion event is published (a DB failure never publishes
/// success).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCallUsageRecord {
    /// `None` for plane-originated calls (embedding/media/… operations
    /// run outside any session; such rows are internal accounting — an
    /// owner-scoped query never returns them).
    pub session_id: Option<String>,
    /// `None` for plane-originated calls (see `session_id`).
    pub run_id: Option<String>,
    pub attempt: Option<String>,
    /// Host-minted model call identity — unique per row.
    pub model_call_id: String,
    /// What the call was for: `chat`, `auxiliary.{slot}`, `worker.callback`,
    /// or an operation name (`embedding`, `rerank`, …).
    pub purpose: String,
    /// The entry-surface vocabulary of the causal root (the run lineage
    /// vocabulary plus the plane origins: `user`, `subagent`, `cron`,
    /// `heartbeat`, `bridge`, `worker-callback`, `operation`).
    pub origin: String,
    /// The run whose delegation caused this call, when one exists.
    pub parent_run_id: Option<String>,
    /// The precise causal anchor inside the parent: a subagent run's
    /// parent tool call, a worker callback's invocation id.
    pub cause_ref: Option<String>,
    /// R05 RR1 F21: the REAL host-minted [`ToolCallId`]
    /// (`lingxi_protocol::ToolCallId`) of the parent tool invocation this
    /// call rode on — the durable JOIN key from a worker-callback row to
    /// its parent tool. The parent MODEL call is discoverable through the
    /// parent row's [`Self::emitted_tool_calls`] on the same run.
    pub parent_tool_call_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub protocol: String,
    /// The usage fact. `None` = no usable usage (never zero).
    pub usage: Option<ModelCallUsage>,
    /// A usage object arrived but violated the contract; the detail names
    /// the violation. Mutually exclusive with `usage: Some(_)`.
    pub invalid_detail: Option<String>,
    /// Physical provider requests this logical call sent. R05 RR1 F38:
    /// the column is NULLABLE —
    /// - `Some(0)` = the call was refused BEFORE anything left the
    ///   process (R05 RR1 F21: not-sent is never recorded as 1);
    /// - `Some(n)` n≥1 = n physical requests were observed (a
    ///   credential-refresh retry is a SECOND physical request and is
    ///   counted — a possibly-billable request never vanishes into one
    ///   row, T07-C03);
    /// - `None` = the attempts count is UNKNOWN — the call's future was
    ///   dropped before settling (a cancellation fence dropping an
    ///   in-flight call), so how many physical requests actually left
    ///   the process cannot be observed. Never `0`, never `1`: both
    ///   would fabricate a fact the dropper does not have.
    pub transport_attempts: Option<u32>,
    /// R05 RR1 F21: the settlement outcome of the call (succeeded /
    /// failed / cancelled / unknown) — the query/export "status" field.
    pub outcome: CallOutcome,
    /// R05 RR1 F21: when the logical call STARTED (host-observed wall
    /// clock; `None` when the writer could not observe it).
    pub started_at_unix_ms: Option<u64>,
    /// R05 RR1 F21: when the call SETTLED into the outcome this row
    /// records. `None` = not observed (crash-window rows).
    pub settled_at_unix_ms: Option<u64>,
    /// R05 RR1 F21: for a parent model call that emitted a tool batch,
    /// the host-minted tool call ids of that batch — the durable JOIN
    /// from a child (`parent_tool_call_id`) back to the parent MODEL call
    /// row of the same run. Empty for calls that emitted no tools.
    pub emitted_tool_calls: Vec<String>,
    /// Cost basis of the row. `None` = cost unknown — no price source is
    /// configured and NO cost is ever invented (T07-C08). A future priced
    /// basis names its source here; token facts are recorded either way.
    pub cost_basis: Option<String>,
}

/// Read scope of the usage ledger query (R05-T07-C09). Authorization is
/// part of the read: `owner_user_id` scopes rows through the session
/// owner when set (a query for another principal's session returns
/// nothing, not filtered content). Session-less plane rows are visible
/// only to the unscoped internal query.
///
/// R05 RR1 F21: the query surface carries the taskbook's
/// date/category/model filters (`recorded_from/to_unix_ms`,
/// `purpose`, `model`) alongside session/run/owner scoping.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelUsageQuery {
    /// Owner scoping (`None` = internal diagnostics over all rows).
    pub owner_user_id: Option<String>,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    /// Category filter — exact match on the row's `purpose`
    /// (`chat`, `auxiliary.{slot}`, an operation name, …).
    pub purpose: Option<String>,
    /// Model filter — exact match on the row's `model`.
    pub model: Option<String>,
    /// Date-window filter (inclusive bounds on `recorded_at_unix_ms`).
    pub recorded_from_unix_ms: Option<u64>,
    pub recorded_to_unix_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: Option<u64>, output: Option<u64>) -> ModelCallUsage {
        ModelCallUsage {
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            provenance: UsageProvenance::Reported,
        }
    }

    #[test]
    fn identical_repeats_are_idempotent_in_both_modes() {
        for mode in [
            UsageAggregationMode::FinalSnapshot,
            UsageAggregationMode::RunningTotal,
        ] {
            let mut folder = UsageFolder::new(mode);
            folder.fold(usage(Some(10), Some(3))).expect("first");
            folder
                .fold(usage(Some(10), Some(3)))
                .expect("identical repeat is a no-op");
            let folded = folder.finish().expect("folded");
            assert_eq!(folded.input_tokens, Some(10));
            assert_eq!(folded.output_tokens, Some(3));
            assert_eq!(folded.provenance, UsageProvenance::Reported);
        }
    }

    #[test]
    fn final_mode_conflicts_on_a_different_snapshot() {
        let mut folder = UsageFolder::new(UsageAggregationMode::FinalSnapshot);
        folder.fold(usage(Some(10), Some(3))).expect("first");
        let conflict = folder
            .fold(usage(Some(10), Some(4)))
            .expect_err("different final snapshot is loud");
        assert!(conflict.0.contains("conflict"), "{conflict}");
    }

    #[test]
    fn running_mode_replaces_growth_and_refuses_shrink() {
        let mut folder = UsageFolder::new(UsageAggregationMode::RunningTotal);
        folder.fold(usage(Some(10), Some(3))).expect("first");
        folder
            .fold(usage(Some(10), Some(9)))
            .expect("a running output total grows");
        let later = folder.clone().finish().expect("folded");
        assert_eq!(
            later.output_tokens,
            Some(9),
            "the later total replaces, never sums"
        );
        let shrunk = folder
            .fold(usage(Some(10), Some(4)))
            .expect_err("a running total never goes backwards");
        assert!(shrunk.0.contains("backwards"), "{shrunk}");
    }

    #[test]
    fn an_input_half_then_output_half_is_partial_never_zero() {
        let mut folder = UsageFolder::new(UsageAggregationMode::RunningTotal);
        folder
            .fold(usage(Some(7), None))
            .expect("input half (message_start shape)");
        folder
            .fold(usage(None, Some(5)))
            .expect("output half completes the fact");
        let folded = folder.finish().expect("folded");
        assert_eq!(folded.input_tokens, Some(7));
        assert_eq!(folded.output_tokens, Some(5));
        assert_eq!(
            folded.provenance,
            UsageProvenance::Reported,
            "the halves complete each other"
        );
        // Interrupted before the output half: PARTIAL, the missing half
        // is named — never written as zero.
        let mut folder = UsageFolder::new(UsageAggregationMode::RunningTotal);
        folder.fold(usage(Some(7), None)).expect("input half");
        let partial = folder
            .finish()
            .expect("partial fact")
            .finish_partial_check();
        assert_eq!(
            partial,
            Some(vec!["output_tokens"]),
            "the missing half is named, not zeroed"
        );
    }

    #[test]
    fn wire_projection_requires_both_totals() {
        assert!(usage(Some(1), Some(2)).wire_record().is_some());
        assert!(usage(Some(1), None).wire_record().is_none());
        assert!(usage(None, None).wire_record().is_none());
    }

    /// Test helper: the missing-field names of a folded partial fact.
    trait PartialCheck {
        fn finish_partial_check(&self) -> Option<Vec<&'static str>>;
    }
    impl PartialCheck for ModelCallUsage {
        fn finish_partial_check(&self) -> Option<Vec<&'static str>> {
            match &self.provenance {
                UsageProvenance::Partial { missing } => Some(missing.clone()),
                _ => None,
            }
        }
    }
}

//! R05 RR1 F11: the whole-batch provider-call-id admission shared by the
//! four chat parser families (openai-completions, anthropic-messages,
//! openai-responses + codex, google-generative-ai).
//!
//! The per-request shape checks (parseable arguments, known wire name,
//! argument invariants) run while each request is built; what was MISSING
//! on the frozen candidate was the BATCH-level identity check: a model turn
//! that carries the SAME provider call id twice is ambiguous — the next
//! request cannot correlate two results to one id. Admission rule:
//!
//! - same id, same target and same arguments digest → an identical
//!   completed re-send: it carries no new information and COLLAPSES to the
//!   first occurrence (one execution, one result — never a double side
//!   effect, never two same-id results);
//! - same id with ANY differing component → a CONFLICT: the WHOLE batch is
//!   rejected as a loud protocol violation (zero requests admitted, zero
//!   side effects — the turn never reaches the tool gateway);
//! - absent id → no identity to conflict on (positional families); calls
//!   without ids never merge.
//!
//! Distinct ids carrying IDENTICAL arguments stay distinct calls — the
//! dedup key is the provider id, never the content.

use std::collections::HashMap;

use lingxi_kernel::ports::ToolRequest;
use lingxi_protocol::{ErrorCode, ProtocolError};

/// Admits one model turn's built tool requests as a whole batch. The input
/// order is preserved (minting order == execution order).
pub fn admit_provider_call_ids(
    family: &str,
    requests: Vec<ToolRequest>,
) -> Result<Vec<ToolRequest>, ProtocolError> {
    let mut seen: HashMap<String, (String, String)> = HashMap::new();
    let mut admitted = Vec::with_capacity(requests.len());
    for request in requests {
        let Some(id) = request.provider_call_id.clone() else {
            admitted.push(request);
            continue;
        };
        let shape = (request.target.clone(), request.args_digest.hex.clone());
        match seen.get(&id) {
            None => {
                seen.insert(id, shape);
                admitted.push(request);
            }
            Some(kept) if *kept == shape => {
                // Identical completed re-send under the same id: collapse.
                // (The digest pins the arguments; the target pins the tool.)
                continue;
            }
            Some(kept) => {
                return Err(ProtocolError::new(
                    ErrorCode::InvalidMessage,
                    format!(
                        "{family}: the model turn carries the provider call id {id:?} twice with \
                         DIFFERENT shapes (first {:?}, then {:?}): the batch is ambiguous and is \
                         rejected whole — nothing is admitted or dispatched",
                        kept.0, shape.0
                    ),
                    false,
                ));
            }
        }
    }
    Ok(admitted)
}

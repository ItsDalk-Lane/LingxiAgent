//! The shared honest rendering of a REAL [`ToolOutcome`] into protocol
//! tool-result content (R05-T03 C07/C08): every adapter family renders the
//! SAME four outcome states with the SAME text vocabulary, so no family
//! ever mislabels a failure as a success, drops the truncation fact, or
//! loses a resource reference / process status.
//!
//! The base four-state texts are the R05-T01 openai-completions rendering
//! (pinned by its golden evidence); T03 extends the Success arm with the
//! full `ToolSuccess` surface (resource refs, run status) that T01 left
//! unrendered.

use lingxi_kernel::ports::{ToolOutcome, ToolRunStatus};
use lingxi_protocol::ContentBlock;

/// The machine-checkable state word of one outcome (the structured
/// envelope half — e.g. Google's `functionResponse.response.status`).
pub fn outcome_status_word(outcome: &ToolOutcome) -> &'static str {
    match outcome {
        ToolOutcome::Success { .. } => "succeeded",
        ToolOutcome::Failed { .. } => "failed",
        ToolOutcome::Cancelled => "cancelled",
        ToolOutcome::Unknown { .. } => "unknown",
    }
}

/// Renders one REAL tool outcome into text content. All four states are
/// honest: a failure says the error, a cancellation says it was cancelled,
/// an unknown says the outcome could not be determined — none is flattened
/// into a fake success. A Success carries its full surface: content blocks
/// (non-text blocks as their JSON), every resource reference, the process
/// run/exit status, and the truncation marker LAST.
pub fn render_tool_outcome_text(outcome: &ToolOutcome) -> String {
    match outcome {
        ToolOutcome::Success { result } => {
            let mut text = String::new();
            for block in &result.content {
                if !text.is_empty() {
                    text.push('\n');
                }
                match block {
                    ContentBlock::Text { text: block_text } => text.push_str(block_text),
                    other => {
                        let mut value = serde_json::to_value(other).unwrap_or_else(|_| {
                            serde_json::Value::String("<unserializable content block>".to_string())
                        });
                        // N-02 (§29.12): the JSON projection of a non-text
                        // block gets the same file:// narrowing as the
                        // resource-refs loop — the model never sees a host
                        // filesystem path, whatever block shape carries it.
                        let carries_file_uri = value
                            .pointer("/resource/uri")
                            .and_then(|v| v.as_str())
                            .map(|uri| uri.starts_with("file://"))
                            .unwrap_or(false);
                        if carries_file_uri {
                            if let Some(resource) =
                                value.get_mut("resource").and_then(|r| r.as_object_mut())
                            {
                                resource.remove("uri");
                            }
                        }
                        text.push_str(&value.to_string());
                    }
                }
            }
            for reference in &result.resource_refs {
                if !text.is_empty() {
                    text.push('\n');
                }
                let name = reference
                    .display_name
                    .as_deref()
                    .unwrap_or(reference.resource_id.as_str());
                // N-02 (§29.12): a `file://` URI is a host-filesystem
                // layout fact — it never crosses to the model. The model
                // sees the citable resource identity; only a genuinely
                // remote URI renders inline. The host side (ToolOutcome /
                // journal) keeps the full URI — audit fidelity is not
                // lost, only the model-facing projection narrows.
                match &reference.uri {
                    Some(uri) if !uri.starts_with("file://") => {
                        text.push_str(&format!("[resource: {name} <{uri}>]"));
                    }
                    _ => text.push_str(&format!("[resource: {name}]")),
                }
            }
            if let Some(status) = &result.status {
                if !text.is_empty() {
                    text.push('\n');
                }
                match status.as_ref() {
                    ToolRunStatus::Exited { code } => {
                        text.push_str(&format!("[process exited with code {code}]"));
                    }
                    ToolRunStatus::Running { handle } => {
                        text.push_str(&format!("[process still running (handle: {handle})]"));
                    }
                    ToolRunStatus::StopUnconfirmed { handle, detail } => {
                        text.push_str(&format!(
                            "[process stop unconfirmed (handle: {handle}): {detail}]"
                        ));
                    }
                }
            }
            if result.truncated {
                text.push_str("\n[result truncated at the tool's bound]");
            }
            text
        }
        ToolOutcome::Failed { error } => {
            format!("tool error [{}]: {}", error.code.wire_name(), error.message)
        }
        ToolOutcome::Cancelled => "tool call cancelled by the host before completion".to_string(),
        ToolOutcome::Unknown { reason } => {
            format!("tool outcome could not be determined: {reason}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lingxi_kernel::ports::{ToolOutcome, ToolRunStatus, ToolSuccess};
    use lingxi_protocol::{ContentBlock, ProtocolError, ResourceId, ResourceRef};

    fn resource(name: Option<&str>, uri: Option<&str>) -> ResourceRef {
        ResourceRef {
            resource_id: ResourceId::new("res-1".to_string()),
            kind: lingxi_protocol::ResourceKind::Attachment,
            display_name: name.map(|s| s.to_string()),
            uri: uri.map(|s| s.to_string()),
            digest: None,
            size_bytes: None,
        }
    }

    #[test]
    fn the_four_base_states_keep_the_t01_vocabulary() {
        assert_eq!(
            render_tool_outcome_text(&ToolOutcome::success_text("ok body")),
            "ok body"
        );
        let failed = render_tool_outcome_text(&ToolOutcome::Failed {
            error: ProtocolError::new(
                lingxi_protocol::ErrorCode::Forbidden,
                "denied by policy",
                false,
            ),
        });
        assert!(failed.contains("forbidden") && failed.contains("denied by policy"));
        assert!(render_tool_outcome_text(&ToolOutcome::Cancelled).contains("cancelled"));
        let unknown = render_tool_outcome_text(&ToolOutcome::Unknown {
            reason: "receipt lost".to_string(),
        });
        assert!(unknown.contains("receipt lost"));
        for (outcome, word) in [
            (ToolOutcome::success_text("x"), "succeeded"),
            (
                ToolOutcome::Failed {
                    error: ProtocolError::new(lingxi_protocol::ErrorCode::Internal, "x", false),
                },
                "failed",
            ),
            (ToolOutcome::Cancelled, "cancelled"),
            (
                ToolOutcome::Unknown {
                    reason: "x".to_string(),
                },
                "unknown",
            ),
        ] {
            assert_eq!(outcome_status_word(&outcome), word);
        }
    }

    #[test]
    fn success_renders_resources_status_and_truncation_last() {
        let mut success = ToolSuccess::from_content(vec![
            ContentBlock::Text {
                text: "partial body".to_string(),
            },
            ContentBlock::ResourceRef {
                resource: resource(Some("out.txt"), Some("file:///out.txt")),
            },
        ]);
        success.resource_refs = vec![
            resource(Some("out.txt"), Some("file:///out.txt")),
            // A remote URI renders inline; a file:// URI renders as the
            // citable identity only (N-02, §29.12).
            resource(Some("cover"), Some("https://cdn.example.test/a.png")),
            resource(Some("no-uri"), None),
        ];
        success.truncated = true;
        success.status = Some(Box::new(ToolRunStatus::Exited { code: 3 }));
        let text = render_tool_outcome_text(&ToolOutcome::Success { result: success });
        assert!(text.contains("partial body"), "{text}");
        assert!(
            text.contains("[resource: out.txt]"),
            "file:// renders as the identity only: {text}"
        );
        assert!(
            !text.contains("file:///out.txt"),
            "the host path never crosses to the model: {text}"
        );
        assert!(
            text.contains("[resource: cover <https://cdn.example.test/a.png>]"),
            "{text}"
        );
        assert!(text.contains("[resource: no-uri]"), "{text}");
        assert!(text.contains("[process exited with code 3]"), "{text}");
        assert!(
            text.ends_with("\n[result truncated at the tool's bound]"),
            "truncation marker is LAST: {text}"
        );
        // A non-text content block renders as its JSON (never dropped).
        assert!(text.contains("resource_ref"), "{text}");
    }

    #[test]
    fn running_and_stop_unconfirmed_statuses_are_honest() {
        let mut running = ToolSuccess::text("so far");
        running.status = Some(Box::new(ToolRunStatus::Running {
            handle: "proc-9".to_string(),
        }));
        let text = render_tool_outcome_text(&ToolOutcome::Success { result: running });
        assert!(
            text.contains("[process still running (handle: proc-9)]"),
            "{text}"
        );
        let mut unconfirmed = ToolSuccess::text("partial");
        unconfirmed.status = Some(Box::new(ToolRunStatus::StopUnconfirmed {
            handle: "proc-10".to_string(),
            detail: "wait timed out".to_string(),
        }));
        let text = render_tool_outcome_text(&ToolOutcome::Success {
            result: unconfirmed,
        });
        assert!(
            text.contains("[process stop unconfirmed (handle: proc-10): wait timed out]"),
            "{text}"
        );
    }
}

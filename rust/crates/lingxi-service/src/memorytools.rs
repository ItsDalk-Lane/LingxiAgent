//! R06-T06 memory/skill model-facing tools registered on the R04 unified
//! gateway (search_memory / pin_memory / unpin_memory / tenet_propose /
//! context_notes / install_skill).
//!
//! Production entries: the same construction point as the R04 file/process
//! tools (lib.rs bootstrap tool plane), the SAME gateway discipline
//! (prepare → policy/approval → execute), and each executor is BOUND to one
//! agent at registration (`MemoryService::resolve_primary_agent`) — there is
//! no cross-agent handle, which is the A11 tool-surface isolation contract
//! (incumbent `core/agent.ts:1045-1047`: "同一 FactStore，不复制数据归属").
//!
//! Incumbent anchors: `lib/memory/memory-search.ts:108-298`,
//! `lib/tools/pinned-memory.ts:32-158`, `lib/tools/tenet-propose-tool.ts:25-108`,
//! `lib/tools/context-notes-tool.ts:43-104`, `lib/tools/install-skill.ts:287-589`.

use std::path::PathBuf;
use std::sync::Arc;

use lingxi_kernel::invocation::ToolRecoveryCapability;
use lingxi_kernel::ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome};
use lingxi_kernel::toolcatalog::{
    Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
    ToolManifest, ToolOrigin, ToolRegistry,
};
use lingxi_protocol::{ContentBlock, ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};

use lingxi_adapters::storage::agent_memory as am;

use crate::memory::MemoryService;
use crate::skills::SkillService;
use crate::toolgateway::ToolInvocationGateway;

fn manifest(
    local_name: &str,
    description: &str,
    kind: PermissionKind,
    capability_base: &str,
    declared: DeclaredPermission,
    schema: Value,
) -> ToolManifest {
    ToolManifest {
        origin: ToolOrigin::FirstParty,
        local_name: local_name.to_string(),
        display_name: local_name.to_string(),
        aliases: Vec::new(),
        version: "1.0.0".to_string(),
        description: description.to_string(),
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".to_string(),
            schema,
        },
        output_schema: None,
        permission: PermissionContract {
            kind,
            capability_base: capability_base.to_string(),
        },
        availability: Availability::Available,
        timeout_ms: if local_name == "install_skill" {
            Some(120_000)
        } else {
            Some(60_000)
        },
        max_concurrency: None,
        declared_permission: declared,
        recovery: ToolRecoveryCapability::CONSERVATIVE,
    }
}

fn failed(code: ErrorCode, message: String) -> ToolOutcome {
    ToolOutcome::Failed {
        error: ProtocolError::new(code, message, false),
    }
}

fn text_success(text: String) -> ToolOutcome {
    ToolOutcome::Success {
        result: lingxi_kernel::ports::ToolSuccess::from_content(vec![ContentBlock::Text { text }]),
    }
}

fn args_of(request: &lingxi_kernel::ports::ToolRequest) -> Value {
    request.arguments.as_value().clone()
}

/// The bound (single-agent) memory tool executor family.
pub struct MemoryTools {
    memory: Arc<MemoryService>,
    skills: Arc<SkillService>,
    agent_id: String,
    /// Injectable clock (context-notes timestamps / diagnostics).
    #[allow(dead_code)]
    clock: Arc<dyn crate::inject::ServiceClock>,
}

enum ToolKind {
    SearchMemory,
    PinMemory,
    UnpinMemory,
    TenetPropose,
    ContextNotes,
    InstallSkill,
}

impl MemoryTools {
    fn bound_agent(&self) -> Result<String, ToolOutcome> {
        if self.agent_id.is_empty() {
            Err(failed(
                ErrorCode::InvalidMessage,
                "no agent is configured; memory tools are unavailable".into(),
            ))
        } else {
            Ok(self.agent_id.clone())
        }
    }

    fn run_search_memory(&self, args: &Value) -> ToolOutcome {
        let agent = match self.bound_agent() {
            Ok(agent) => agent,
            Err(outcome) => return outcome,
        };
        let store = match self.memory.fact_store(&agent) {
            Ok(store) => store,
            Err(err) => return failed(ErrorCode::Internal, err.to_string()),
        };
        let size = match store.size() {
            Ok(size) => size,
            Err(err) => return failed(ErrorCode::Internal, err.to_string()),
        };
        if size == 0 {
            return text_success(
                "Memory is empty — no facts are stored for this agent yet.".into(),
            );
        }
        let query = args.get("query").and_then(Value::as_str).unwrap_or("");
        let tags: Vec<String> = args
            .get("tags")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let date_from = args.get("date_from").and_then(Value::as_str);
        let date_to = args
            .get("date_to")
            .and_then(Value::as_str)
            .map(|v| format!("{v}T23:59"));
        let mut results: Vec<(am::FactRow, &'static str)> = Vec::new();
        let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
        // Strategy 1: tag match (memory-search.ts:157-168)
        if !tags.is_empty() {
            if let Ok(rows) = store.search_by_tags(&tags, date_from, date_to.as_deref(), 15) {
                for row in rows {
                    if seen.insert(row.id) {
                        results.push((row, "tag"));
                    }
                }
            }
        }
        // Strategy 2: FTS (+ semantic path when wired — status reported honestly)
        let semantic_status = "not_configured";
        if results.len() < 3 && !query.is_empty() {
            match store.search_full_text(query, 10) {
                Ok(rows) => {
                    for row in rows {
                        if seen.insert(row.id) {
                            results.push((row, "fts"));
                        }
                    }
                }
                Err(err) => return failed(ErrorCode::Internal, err.to_string()),
            }
        }
        // date filter on all results (memory-search.ts:234-242)
        if date_from.is_some() || date_to.is_some() {
            results.retain(|(row, _)| match &row.time {
                None => true,
                Some(time) => {
                    if let Some(from) = date_from {
                        if time.as_str() < from {
                            return false;
                        }
                    }
                    if let Some(to) = &date_to {
                        if time.as_str() > to.as_str() {
                            return false;
                        }
                    }
                    true
                }
            });
        }
        if results.is_empty() {
            let mut parts = vec!["No memory matches this search.".to_string()];
            if let Ok(Some(diagnostics)) = store.zero_result_diagnostics(query) {
                let counts: Vec<String> = diagnostics
                    .as_array()
                    .map(|entries| {
                        entries
                            .iter()
                            .map(|e| {
                                format!(
                                    "『{}』{} 条",
                                    e.get("term").and_then(Value::as_str).unwrap_or(""),
                                    e.get("count").and_then(Value::as_i64).unwrap_or(0)
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if !counts.is_empty() {
                    parts.push(format!("Per-term hit counts: {}", counts.join("、")));
                    if diagnostics
                        .as_array()
                        .map(|entries| {
                            entries
                                .iter()
                                .any(|e| e.get("count").and_then(Value::as_i64).unwrap_or(1) == 0)
                        })
                        .unwrap_or(false)
                    {
                        parts.push("Try rewriting the query without the zero-hit terms.".into());
                    }
                }
            }
            if semantic_status != "used" && semantic_status != "not_configured" {
                parts.push(format!("semantic search status: {semantic_status}"));
            }
            return text_success(parts.join("\n"));
        }
        let lines: Vec<String> = results
            .iter()
            .enumerate()
            .map(|(i, (row, _))| {
                let tags_str = if row.tags.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", row.tags.join(", "))
                };
                let time_str = row
                    .time
                    .as_deref()
                    .map(|t| format!(" — {t}"))
                    .unwrap_or_default();
                format!("{}. {}{}{}", i + 1, row.fact, tags_str, time_str)
            })
            .collect();
        text_success(lines.join("\n"))
    }

    fn run_pin_memory(&self, args: &Value) -> ToolOutcome {
        let agent = match self.bound_agent() {
            Ok(agent) => agent,
            Err(outcome) => return outcome,
        };
        let Some(content) = args.get("content").and_then(Value::as_str) else {
            return failed(ErrorCode::InvalidMessage, "content is required".into());
        };
        let (cleaned, detected) = am::scrub_pii(content);
        let _ = detected;
        let store = match self.memory.tenets_store(&agent) {
            Ok(store) => store,
            Err(err) => return failed(ErrorCode::Internal, err.to_string()),
        };
        match store.add_direct(&cleaned, None, format!("pin-{}", am::random_token_suffix())) {
            Ok((_tenet, duplicate)) => {
                if duplicate {
                    text_success("This item is already pinned.".into())
                } else {
                    text_success(format!("Pinned: {cleaned}"))
                }
            }
            Err(err) => failed(
                ErrorCode::InvalidMessage,
                match err.tenet_code() {
                    Some(code) => format!("{code}: {err}"),
                    None => err.to_string(),
                },
            ),
        }
    }

    fn run_unpin_memory(&self, args: &Value) -> ToolOutcome {
        let agent = match self.bound_agent() {
            Ok(agent) => agent,
            Err(outcome) => return outcome,
        };
        let store = match self.memory.tenets_store(&agent) {
            Ok(store) => store,
            Err(err) => return failed(ErrorCode::Internal, err.to_string()),
        };
        let existing = store.active();
        if existing.is_empty() {
            return text_success("Pinned memory is empty; nothing to remove.".into());
        }
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        let keyword = args
            .get("keyword")
            .and_then(Value::as_str)
            .map(|k| k.replace("\r\n", "\n").trim().to_lowercase())
            .unwrap_or_default();
        if id.is_empty() && keyword.is_empty() {
            return text_success("Nothing matched: provide an id or keyword.".into());
        }
        let mut removed = Vec::new();
        for tenet in existing {
            let matches_id = !id.is_empty() && tenet.id == id;
            let matches_keyword =
                !keyword.is_empty() && tenet.content.to_lowercase().contains(&keyword);
            if matches_id || matches_keyword {
                match store.remove(&tenet.id) {
                    Ok(true) => removed.push((tenet.id.clone(), tenet.content.clone())),
                    Ok(false) => {}
                    Err(err) => return failed(ErrorCode::Internal, err.to_string()),
                }
            }
        }
        if removed.is_empty() {
            let label = if id.is_empty() { keyword.as_str() } else { id };
            return text_success(format!("No pinned item matched \"{label}\"."));
        }
        let items: Vec<String> = removed.iter().map(|(_, c)| c.clone()).collect();
        text_success(format!(
            "Removed {} pinned item(s): {}",
            removed.len(),
            items.join(", ")
        ))
    }

    fn run_tenet_propose(&self, args: &Value) -> ToolOutcome {
        let agent = match self.bound_agent() {
            Ok(agent) => agent,
            Err(outcome) => return outcome,
        };
        // Memory master switch (tenet-propose-tool.ts:62-67)
        match self.memory.read_flags(&agent) {
            Ok(flags) if flags.memory_enabled.unwrap_or(true) => {}
            Ok(_) => {
                return text_success(
                    "Memory is paused for this agent; proposals are not accepted right now.".into(),
                );
            }
            Err(err) => return failed(ErrorCode::Internal, err.to_string()),
        }
        let Some(content) = args
            .get("content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|c| !c.is_empty())
        else {
            return text_success("A proposal needs non-empty content.".into());
        };
        let priority = args.get("priority").and_then(Value::as_str);
        let store = match self.memory.tenets_store(&agent) {
            Ok(store) => store,
            Err(err) => return failed(ErrorCode::Internal, err.to_string()),
        };
        match store.add_proposal(
            content,
            priority,
            None,
            format!("tenet-{}", am::random_token_suffix()),
        ) {
            Ok((tenet, duplicate, existing_status)) => {
                if duplicate {
                    text_success(format!(
                        "This proposal already exists (status: {}).",
                        existing_status.map(|s| s.as_str()).unwrap_or("pending")
                    ))
                } else {
                    text_success(format!(
                        "Proposal submitted as pending (id: {}). The user will approve or reject it.",
                        tenet.id
                    ))
                }
            }
            Err(err) => failed(
                ErrorCode::InvalidMessage,
                match err.tenet_code() {
                    Some("TENET_PENDING_FULL") => {
                        "Pending proposals are full; ask the user to review them first.".into()
                    }
                    _ => err.to_string(),
                },
            ),
        }
    }

    fn run_context_notes(&self, ctx: &lingxi_kernel::RunContext, args: &Value) -> ToolOutcome {
        // 没有会话时不能把失败包装成空笔记，也不能创建无归属文件。
        if ctx.session_id.as_str().trim().is_empty() {
            return failed(
                ErrorCode::InvalidMessage,
                "no active session — context notes are session-scoped".into(),
            );
        }
        let action = args.get("action").and_then(Value::as_str).unwrap_or("read");
        let notes = self.memory.context_notes();
        match action {
            "read" => match notes.read(ctx.session_id.as_str()) {
                Ok(text) => {
                    if text.is_empty() {
                        text_success("(no context notes yet)".into())
                    } else {
                        text_success(format!("context notes:\n{text}"))
                    }
                }
                Err(err) => failed(ErrorCode::Internal, err.to_string()),
            },
            "write" | "append" => {
                let Some(text) = args.get("text").and_then(Value::as_str) else {
                    return failed(
                        ErrorCode::InvalidMessage,
                        "text is required for write/append".into(),
                    );
                };
                if text.trim().is_empty() {
                    return failed(
                        ErrorCode::InvalidMessage,
                        "refusing to write empty notes (use them or drop them)".into(),
                    );
                }
                let next = if action == "append" {
                    match notes.read(ctx.session_id.as_str()) {
                        Ok(current) if !current.is_empty() => {
                            format!("{}\n{}", current.trim_end(), text)
                        }
                        Ok(_) => text.to_string(),
                        Err(err) => return failed(ErrorCode::Internal, err.to_string()),
                    }
                } else {
                    text.to_string()
                };
                match notes.write(ctx.session_id.as_str(), &next) {
                    Ok(bytes) => text_success(format!(
                        "context notes {action} ({bytes}/{} bytes used). They survive compaction.",
                        am::CONTEXT_NOTES_MAX_BYTES
                    )),
                    Err(am::MemoryError::Conflict { code, message }) => {
                        failed(ErrorCode::InvalidMessage, format!("{code}: {message}"))
                    }
                    Err(err) => failed(ErrorCode::Internal, err.to_string()),
                }
            }
            other => failed(
                ErrorCode::InvalidMessage,
                format!("unknown action: {other}"),
            ),
        }
    }

    async fn run_install_skill(&self, args: &Value) -> ToolOutcome {
        // local_path branch of the incumbent tool (install-skill.ts:470-574);
        // the GitHub branch reports its flag gate honestly (:357-372).
        let github = args
            .get("github_url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if github.is_some() {
            return text_success(
                "GitHub fetching for install_skill is not enabled in this deployment \
                 (capabilities.learn_skills.allow_github_fetch). Provide a local package path \
                 visible to the server instead."
                    .into(),
            );
        }
        let local_path = args
            .get("local_path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let source = match local_path {
            Some(path) => PathBuf::from(path),
            None => {
                if args
                    .get("skill_content")
                    .and_then(Value::as_str)
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false)
                    || args
                        .get("skill_name")
                        .and_then(Value::as_str)
                        .map(|s| !s.trim().is_empty())
                        .unwrap_or(false)
                {
                    return text_success(
                        "install_skill only installs complete skill packages (folder with \
                         SKILL.md). skill_content is rejected — provide a local package path."
                            .into(),
                    );
                }
                return text_success(
                    "Provide exactly one source: a local package path visible to the server."
                        .into(),
                );
            }
        };
        // 安装前验证所属助手配置，不能先落盘再吞掉启用错误。
        let agent = match self.bound_agent() {
            Ok(agent) => agent,
            Err(outcome) => return outcome,
        };
        if let Err(err) = self.memory.read_flags(&agent) {
            return failed(ErrorCode::Internal, err.to_string());
        }
        let source_key = format!("local_path:{}", source.to_string_lossy());
        let result = self
            .skills
            .install_with_review(&source, "", args, &source_key)
            .await;
        match result {
            Ok(value) => {
                let ok = value.get("ok").and_then(Value::as_bool).unwrap_or(false);
                if ok {
                    // onInstalled parity: add to the bound agent's enabled list
                    if let Some(name) = value.get("skillName").and_then(Value::as_str) {
                        if let Err(err) = self.enable_for_agent(name) {
                            // 安装已经发生时如实报告，不能伪称已启用或删除安装成果。
                            return failed(
                                ErrorCode::Internal,
                                format!("Skill installed: {name}, but enabling failed: {err}"),
                            );
                        }
                    }
                    let mut text = format!(
                        "Skill installed: {} (enabled for this agent).",
                        value.get("skillName").and_then(Value::as_str).unwrap_or("")
                    );
                    if value.get("riskOverride").and_then(Value::as_bool) == Some(true) {
                        let reason = value
                            .get("riskReason")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        text.push_str(&format!(
                            "\nSafety review did NOT pass; installed after explicit user confirmation (reason: {reason})."
                        ));
                    } else {
                        text.push_str("\nSafety review passed.");
                    }
                    text_success(text)
                } else {
                    let reason = value
                        .get("riskReason")
                        .or_else(|| value.get("error"))
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    if value
                        .get("requiresRiskConfirmation")
                        .and_then(Value::as_bool)
                        == Some(true)
                    {
                        text_success(format!(
                            "Safety review FAILED: {reason}\nExplain this risk to the user. Only after the user \
                             explicitly accepts, retry with risk_accepted=true plus risk_confirmation_token={}.",
                            value
                                .get("riskConfirmationToken")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                        ))
                    } else {
                        failed(
                            ErrorCode::InvalidMessage,
                            value
                                .get("error")
                                .and_then(Value::as_str)
                                .unwrap_or("skill install failed")
                                .to_string(),
                        )
                    }
                }
            }
            Err(err) => failed(ErrorCode::InvalidMessage, err.to_string()),
        }
    }

    fn enable_for_agent(&self, skill_name: &str) -> Result<(), am::MemoryError> {
        if self.agent_id.is_empty() {
            return Ok(());
        }
        let config_path = self.memory.config_path(&self.agent_id)?;
        let text = am::config::read_config_text(&config_path)
            .map_err(|e| am::MemoryError::Io(e.to_string()))?;
        let (mut enabled, workspace_disabled) =
            lingxi_adapters::storage::agent_memory::AgentConfigPatch::read_skill_lists(&text)?;
        if !enabled.iter().any(|n| n == skill_name) {
            enabled.push(skill_name.to_string());
            let patched =
                lingxi_adapters::storage::agent_memory::AgentConfigPatch::write_skill_lists(
                    &text,
                    &enabled,
                    &workspace_disabled,
                )?;
            am::atomic_write(&config_path, &patched).map_err(am::MemoryError::from)?;
        }
        Ok(())
    }
}

impl ToolExecutorPort for MemoryTools {
    fn execute<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a ToolCallId,
        request: &'a lingxi_kernel::ports::ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        // The wire target is the registry target id ("tool:first-party:<name>",
        // the shape every R05 protocol adapter emits — see
        // openai_completions.rs:1259); the local name is its last ':' segment.
        let local_name = request
            .target
            .rsplit(':')
            .next()
            .unwrap_or(request.target.as_str());
        let kind = match local_name {
            "search_memory" => ToolKind::SearchMemory,
            "pin_memory" => ToolKind::PinMemory,
            "unpin_memory" => ToolKind::UnpinMemory,
            "tenet_propose" => ToolKind::TenetPropose,
            "context_notes" => ToolKind::ContextNotes,
            "install_skill" => ToolKind::InstallSkill,
            other => {
                return Box::pin(async move {
                    ToolExecutionResult::of_ctx(
                        ctx,
                        failed(
                            ErrorCode::InvalidMessage,
                            format!("memory tools bound to unknown target {other}"),
                        ),
                    )
                });
            }
        };
        let args = args_of(request);
        let ctx = ctx.clone();
        Box::pin(async move {
            let outcome = match kind {
                ToolKind::SearchMemory => self.run_search_memory(&args),
                ToolKind::PinMemory => self.run_pin_memory(&args),
                ToolKind::UnpinMemory => self.run_unpin_memory(&args),
                ToolKind::TenetPropose => self.run_tenet_propose(&args),
                ToolKind::ContextNotes => self.run_context_notes(&ctx, &args),
                ToolKind::InstallSkill => self.run_install_skill(&args).await,
            };
            ToolExecutionResult::of_ctx(&ctx, outcome)
        })
    }
}

/// Registers the memory/skill tools on the registry + gateway. Called from
/// the bootstrap tool plane (same construction point as the R04 basic tools).
pub fn register_memory_tools(
    registry: &ToolRegistry,
    gateway: &ToolInvocationGateway,
    memory: Arc<MemoryService>,
    skills: Arc<SkillService>,
    agent_id: String,
    clock: Arc<dyn crate::inject::ServiceClock>,
    budget: &SchemaBudget,
) {
    let tools = Arc::new(MemoryTools {
        memory,
        skills,
        agent_id,
        clock,
    });
    let search_manifest = manifest(
        "search_memory",
        "Search this agent's long-term fact store. Prefer tags; combine with a free-text query. Only the CURRENT agent's memory is visible.",
        PermissionKind::Read,
        "search_memory.read",
        DeclaredPermission::ReadOnly,
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "tags": {"type": "array", "items": {"type": "string"}},
                "date_from": {"type": "string"},
                "date_to": {"type": "string"},
            },
            "additionalProperties": false,
        }),
    );
    let pin_manifest = manifest(
        "pin_memory",
        "Save an item to pinned memory (immediately active for this agent). Use when the user says 'remember this'.",
        PermissionKind::Execute,
        "pin_memory.pin",
        DeclaredPermission::Execute,
        json!({
            "type": "object",
            "properties": {"content": {"type": "string", "minLength": 1}},
            "required": ["content"],
            "additionalProperties": false,
        }),
    );
    let unpin_manifest = manifest(
        "unpin_memory",
        "Remove pinned items by id or keyword (all matches are removed).",
        PermissionKind::Execute,
        "unpin_memory.unpin",
        DeclaredPermission::Execute,
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string"},
                "keyword": {"type": "string"},
            },
            "additionalProperties": false,
        }),
    );
    let tenet_manifest = manifest(
        "tenet_propose",
        "Propose a durable behavioral principle for this agent. It becomes PENDING and only takes effect after the user approves it.",
        PermissionKind::Execute,
        "tenet_propose.propose",
        DeclaredPermission::Execute,
        json!({
            "type": "object",
            "properties": {
                "content": {"type": "string", "minLength": 1},
                "priority": {"type": "string", "enum": ["critical", "high", "medium", "low"]},
            },
            "required": ["content"],
            "additionalProperties": false,
        }),
    );
    let notes_manifest = manifest(
        "context_notes",
        "Notes to your future self that survive context compaction (16KiB cap, session-scoped). action=read|write|append.",
        PermissionKind::Read,
        "context_notes.notes",
        DeclaredPermission::Execute,
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["read", "write", "append"]},
                "text": {"type": "string"},
            },
            "additionalProperties": false,
        }),
    );
    let install_manifest = manifest(
        "install_skill",
        "Install a complete skill package (folder with SKILL.md) into the shared pool, enabled for this agent. A failed safety review returns requiresRiskConfirmation; only retry with risk_accepted=true plus the returned token after explicit user confirmation.",
        PermissionKind::Execute,
        "install_skill.install",
        DeclaredPermission::Execute,
        json!({
            "type": "object",
            "properties": {
                "local_path": {"type": "string"},
                "github_url": {"type": "string"},
                "risk_accepted": {"type": "boolean"},
                "risk_confirmation_token": {"type": "string"},
                "reason": {"type": "string"},
            },
            "additionalProperties": false,
        }),
    );
    for (_name, m) in [
        ("search_memory", search_manifest),
        ("pin_memory", pin_manifest),
        ("unpin_memory", unpin_manifest),
        ("tenet_propose", tenet_manifest),
        ("context_notes", notes_manifest),
        ("install_skill", install_manifest),
    ] {
        // Registration failure is a LOUD startup error, never a silent skip —
        // a quietly missing tool is exactly the failure mode the A11 tool
        // surface must not have (R06-T06 R1 F-03; filetools.rs `.expect`
        // precedent).
        let receipt = registry
            .register(m, budget)
            .expect("the resident memory/skill tools register");
        gateway.bind_executor(
            receipt.target_id,
            Arc::new(MemoryToolExecutor {
                tools: Arc::clone(&tools),
            }),
            "R06-T06 memory/skill tool executor (per-agent memory surface)",
        );
    }
}

/// Thin per-target dispatcher so each registered target gets its own executor
/// binding (the gateway requires one executor per target).
struct MemoryToolExecutor {
    tools: Arc<MemoryTools>,
}

impl ToolExecutorPort for MemoryToolExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        call: &'a ToolCallId,
        request: &'a lingxi_kernel::ports::ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        ToolExecutorPort::execute(&*self.tools, ctx, call, request)
    }
}

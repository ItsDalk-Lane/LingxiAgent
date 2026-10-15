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

fn success_details(text: String, details: Value) -> ToolOutcome {
    let mut result = lingxi_kernel::ports::ToolSuccess::text(text);
    result.details = Some(details.as_object().expect("业务字段必须是对象").clone());
    ToolOutcome::Success { result }
}

fn matched_pins(
    store: &am::TenetsStore,
    args: &Value,
) -> Result<Vec<(String, String)>, am::MemoryError> {
    let id = args.get("id").and_then(Value::as_str).unwrap_or("").trim();
    let keyword = args
        .get("keyword")
        .and_then(Value::as_str)
        .unwrap_or("")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_lowercase();
    Ok(store
        .active_checked()?
        .into_iter()
        .filter(|t| {
            (!id.is_empty() && t.id == id)
                || (!keyword.is_empty() && t.content.to_lowercase().contains(&keyword))
        })
        .map(|t| (t.id, t.content))
        .collect())
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
    LearnLesson,
    RecordExperience,
    RecallExperience,
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

    async fn run_search_memory(
        &self,
        ctx: &lingxi_kernel::RunContext,
        call: &ToolCallId,
        args: &Value,
    ) -> ToolOutcome {
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
            return success_details(
                "Memory is empty — no facts are stored for this agent yet.".into(),
                json!({"retrieval":"empty_store", "semantic":"not_searched", "facts":[]}),
            );
        }
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
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
            .map(|d| format!("{d}T23:59"));
        let cross_channel = args
            .get("cross_channel")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let visible = |row: &am::FactRow| {
            !ctx.session_id.as_str().starts_with("channel-")
                || cross_channel
                || !row
                    .session_id
                    .as_deref()
                    .unwrap_or("")
                    .starts_with("channel-")
                || row.session_id.as_deref() == Some(ctx.session_id.as_str())
        };
        let mut results: Vec<(am::FactRow, &'static str)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        if !tags.is_empty() {
            let rows = match store.search_by_tags(&tags, date_from, date_to.as_deref(), 15) {
                Ok(rows) => rows,
                Err(err) => return failed(ErrorCode::Internal, err.to_string()),
            };
            for row in rows {
                if visible(&row) && seen.insert(row.id) {
                    results.push((row, "tag"));
                }
            }
        }
        let mut searched = !tags.is_empty();
        let mut semantic = if results.len() >= 3 {
            "skipped_tag_sufficient"
        } else {
            "not_searched"
        };
        let mut coverage = None;
        if results.len() < 3 && !query.is_empty() {
            searched = true;
            let fts = match store.search_full_text(query, 10) {
                Ok(rows) => rows.into_iter().filter(visible).collect::<Vec<_>>(),
                Err(err) => return failed(ErrorCode::Internal, err.to_string()),
            };
            let mut semantic_rows = Vec::new();
            semantic = "not_configured";
            if let Some(embedder) = self.memory.search_embedder() {
                match embedder.identity() {
                    Ok(None) => semantic = "unavailable_no_model",
                    Err(_) => semantic = "unavailable_unresolvable",
                    Ok(Some(_)) => {
                        let mut runtime = match crate::toolgateway::knowledge_runtime(ctx, call) {
                            Ok(runtime) => runtime,
                            Err(err) => return ToolOutcome::Failed { error: err },
                        };
                        runtime.cancel = runtime.cancel.child(
                            format!("memory-query:{}", call),
                            crate::cancel::ScopeKind::ModelCall,
                        );
                        let attempt = tokio::time::timeout(
                            std::time::Duration::from_secs(2),
                            embedder.embed_owned(
                                vec![query.into()],
                                crate::knowledge::EmbeddingInputType::Query,
                                &runtime,
                                None,
                            ),
                        )
                        .await;
                        match attempt {
                            Err(_) => {
                                runtime.cancel.cancel("memory query embedding timeout");
                                semantic = "skipped_timeout";
                            }
                            Ok(Err(_)) => semantic = "unavailable_unresolvable",
                            Ok(Ok(result)) => {
                                if let Some(error) = result.accounting_error {
                                    return failed(ErrorCode::Internal, error.to_string());
                                }
                                if result.vectors.len() != 1
                                    || result.vectors[0].len()
                                        != result.identity.dimensions as usize
                                    || result.vectors[0].is_empty()
                                    || result.vectors[0].iter().any(|v| !v.is_finite())
                                {
                                    semantic = "unavailable_unresolvable";
                                } else {
                                    let count = match store.embedding_coverage(
                                        &result.identity.key,
                                        result.vectors[0].len(),
                                    ) {
                                        Ok(count) => count,
                                        Err(err) => {
                                            return failed(ErrorCode::Internal, err.to_string())
                                        }
                                    };
                                    coverage = Some(count);
                                    if count == 0 {
                                        semantic = "skipped_no_coverage";
                                    } else {
                                        semantic = "used";
                                        semantic_rows = match store.semantic_search(
                                            &result.identity.key,
                                            &result.vectors[0],
                                            15,
                                        ) {
                                            Ok(rows) => rows.into_iter().filter(visible).collect(),
                                            Err(err) => {
                                                return failed(ErrorCode::Internal, err.to_string())
                                            }
                                        };
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let rankings = [
                fts.iter().map(|row| row.id.to_string()).collect(),
                semantic_rows
                    .iter()
                    .map(|row: &am::FactRow| row.id.to_string())
                    .collect(),
            ];
            for (id, _) in lingxi_kernel::knowledge::fuse_rankings(&rankings, 25) {
                let ft = fts.iter().find(|row| row.id.to_string() == id);
                let sm = semantic_rows.iter().find(|row| row.id.to_string() == id);
                if let Some(row) = sm.or(ft) {
                    if seen.insert(row.id) {
                        results.push((
                            row.clone(),
                            match (ft.is_some(), sm.is_some()) {
                                (true, true) => "semantic+fts",
                                (_, true) => "semantic",
                                _ => "fts",
                            },
                        ));
                    }
                }
            }
        }
        results.retain(|(row, _)| {
            row.time
                .as_deref()
                .map(|time| {
                    date_from.is_none_or(|from| time >= from)
                        && date_to.as_deref().is_none_or(|to| time <= to)
                })
                .unwrap_or(true)
        });
        let retrieval = if !searched {
            "not_searched"
        } else if results.is_empty() {
            "no_matches"
        } else {
            "matched"
        };
        let facts: Vec<Value> = results.iter().map(|(row,source)| json!({"id":row.id,"fact":row.fact,"time":row.time,"tags":row.tags,"source":source})).collect();
        let mut text = if results.is_empty() {
            if searched {
                "No memory matches this search.".into()
            } else {
                "Memory was not searched: provide a query or tags.".into()
            }
        } else {
            results
                .iter()
                .enumerate()
                .map(|(i, (row, _))| {
                    format!(
                        "{}. {} ({}){}",
                        i + 1,
                        row.fact,
                        row.tags.join(", "),
                        row.time
                            .as_deref()
                            .map(|t| format!(" — {t}"))
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let diagnostics = if searched && results.is_empty() {
            match store.zero_result_diagnostics(query) {
                Ok(value) => value,
                Err(err) => return failed(ErrorCode::Internal, err.to_string()),
            }
        } else {
            None
        };
        if let Some(terms) = &diagnostics {
            let entries = terms.as_array().expect("逐词诊断列表");
            text.push_str(&format!(
                "\nPer-term hit counts: {}",
                entries
                    .iter()
                    .map(|e| format!("『{}』{} 条", e["term"].as_str().unwrap_or(""), e["count"]))
                    .collect::<Vec<_>>()
                    .join("、")
            ));
            if entries.iter().any(|e| e["count"].as_i64() == Some(0)) {
                text.push_str("\nTry rewriting the query without the zero-hit terms.");
            }
        }
        text.push_str(&format!(
            "\nretrieval status: {retrieval}; semantic search status: {semantic}"
        ));
        success_details(
            text,
            json!({"retrieval":retrieval,"semantic":semantic,"coverage":coverage,"facts":facts,"diagnostics":diagnostics}),
        )
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
        let Some(scope) = crate::toolgateway::review_scope() else {
            return failed(
                ErrorCode::Unauthorized,
                "pinned deletion requires a trusted reviewed scope".into(),
            );
        };
        if scope["agentId"].as_str() != Some(agent.as_str()) {
            return failed(
                ErrorCode::Unauthorized,
                "pinned deletion scope belongs to another agent".into(),
            );
        }
        let expected: Vec<(String, String)> = scope["items"]
            .as_array()
            .expect("确认范围中的条目列表")
            .iter()
            .map(|item| {
                (
                    item["id"].as_str().expect("条目ID").into(),
                    item["content"].as_str().expect("条目正文").into(),
                )
            })
            .collect();
        let current = match matched_pins(&store, args) {
            Ok(items) => items,
            Err(err) => return failed(ErrorCode::Internal, err.to_string()),
        };
        if current != expected {
            return failed(
                ErrorCode::Conflict,
                "pinned deletion scope changed; review again".into(),
            );
        }
        let removed = match store.remove_reviewed(&expected) {
            Ok(items) => items,
            Err(err) => return failed(ErrorCode::Internal, err.to_string()),
        };
        let text = if removed.is_empty() {
            "Nothing matched: pinned memory is empty or no item matched.".into()
        } else {
            format!(
                "Removed {} pinned item(s): {}",
                removed.len(),
                removed
                    .iter()
                    .map(|(_, c)| c.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        success_details(
            text,
            json!({"removedCount":removed.len(),"removedItems":removed.iter().map(|(id,content)|json!({"id":id,"content":content})).collect::<Vec<_>>()}),
        )
    }

    fn run_experience(&self, args: &Value, record: bool) -> ToolOutcome {
        let agent = match self.bound_agent() {
            Ok(agent) => agent,
            Err(outcome) => return outcome,
        };
        match self.memory.read_flags(&agent) {
            Ok(flags) if flags.experience_enabled.unwrap_or(false) => {}
            Ok(_) => {
                return success_details(
                    "Experience is paused for this agent.".into(),
                    json!({"paused":true}),
                )
            }
            Err(err) => return failed(ErrorCode::Internal, err.to_string()),
        }
        let store = match self.memory.experience_store(&agent) {
            Ok(store) => store,
            Err(err) => return failed(ErrorCode::Internal, err.to_string()),
        };
        let category = args
            .get("category")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim_start_matches('#')
            .trim();
        if record {
            let content = args
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if category.is_empty() || content.is_empty() {
                return failed(
                    ErrorCode::InvalidMessage,
                    "category and content must be non-empty".into(),
                );
            }
            match store.record_entry(category, content) {
                Ok((added, reason)) => success_details(
                    if added {
                        format!("Recorded in {category}: {content}")
                    } else {
                        "This experience already exists.".into()
                    },
                    json!({"category":category,"content":content,"added":added,"reason":reason}),
                ),
                Err(err @ am::MemoryError::Conflict { .. }) => ToolOutcome::Failed {
                    error: ProtocolError::new(ErrorCode::Conflict, err.to_string(), true),
                },
                Err(err) => failed(
                    if matches!(err, am::MemoryError::InvalidRequest(_)) {
                        ErrorCode::InvalidMessage
                    } else {
                        ErrorCode::Internal
                    },
                    err.to_string(),
                ),
            }
        } else if category.is_empty() {
            match store.index_text_checked() {
                Ok(text) => success_details(
                    if text.trim().is_empty() {
                        "Experience library is empty.".into()
                    } else {
                        text
                    },
                    json!({}),
                ),
                Err(err) => failed(ErrorCode::Internal, err.to_string()),
            }
        } else {
            let Some(category) = lingxi_kernel::memory::normalize_experience_category(category)
            else {
                return failed(
                    ErrorCode::InvalidMessage,
                    "invalid experience category".into(),
                );
            };
            match store.list_documents_checked() {
                Ok(docs) => match docs.into_iter().find(|doc| doc.title == category) {
                    Some(doc) => success_details(
                        format!("# {}\n\n{}", doc.title, doc.body),
                        json!({"category":doc.title}),
                    ),
                    None => success_details(
                        format!("Experience category not found: {category}"),
                        json!({"category":category,"found":false}),
                    ),
                },
                Err(err) => failed(ErrorCode::Internal, err.to_string()),
            }
        }
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
        call: &'a ToolCallId,
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
            "learn_lesson" => ToolKind::LearnLesson,
            "record_experience" => ToolKind::RecordExperience,
            "recall_experience" => ToolKind::RecallExperience,
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
            // 助手材料必须与网关铸造的运行归属相同；上下文笔记仅以当前会话为归属。
            let runtime = match crate::toolgateway::knowledge_runtime(&ctx, call) {
                Ok(runtime) => runtime,
                Err(error) => {
                    return ToolExecutionResult::of_ctx(&ctx, ToolOutcome::Failed { error })
                }
            };
            if !matches!(kind, ToolKind::ContextNotes)
                && runtime.ownership.agent_id.as_deref() != Some(self.agent_id.as_str())
            {
                return ToolExecutionResult::of_ctx(
                    &ctx,
                    failed(
                        ErrorCode::Unauthorized,
                        "memory tool belongs to another agent; the action was not run".into(),
                    ),
                );
            }
            let outcome = match kind {
                ToolKind::SearchMemory => self.run_search_memory(&ctx, call, &args).await,
                ToolKind::RecordExperience => self.run_experience(&args, true),
                ToolKind::RecallExperience => self.run_experience(&args, false),
                ToolKind::PinMemory => self.run_pin_memory(&args),
                ToolKind::UnpinMemory => self.run_unpin_memory(&args),
                ToolKind::TenetPropose => self.run_tenet_propose(&args),
                ToolKind::ContextNotes => self.run_context_notes(&ctx, &args),
                ToolKind::InstallSkill => self.run_install_skill(&args).await,
                ToolKind::LearnLesson => {
                    match self.skills.learn_lesson(&self.agent_id, &args).await {
                        Ok(value) => success_details(
                            if value["ok"].as_bool() == Some(true) {
                                "Lesson saved and enabled for this agent.".into()
                            } else {
                                "Safety review requires explicit user confirmation.".into()
                            },
                            value,
                        ),
                        Err(error) => failed(ErrorCode::InvalidMessage, error.to_string()),
                    }
                }
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
                "cross_channel": {"type": "boolean"},
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
        "Remove pinned items by id or keyword after reviewing the exact matched items.",
        PermissionKind::Review,
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
    let lesson_manifest = manifest(
        "learn_lesson",
        "Save a reusable lesson as a new shared skill enabled for this agent and index it in experience. A failed safety review requires explicit user acceptance and the returned one-shot token. Existing names are never overwritten.",
        PermissionKind::Execute,
        "learn_lesson.learn",
        DeclaredPermission::Execute,
        json!({"type":"object","properties":{"name":{"type":"string","minLength":1},"description":{"type":"string","minLength":1},"lesson":{"type":"string","minLength":1},"risk_accepted":{"type":"boolean"},"risk_confirmation_token":{"type":"string"}},"required":["name","description","lesson"],"additionalProperties":false}),
    );
    let record_manifest = manifest(
        "record_experience",
        "Record a concise lesson in this agent's enabled experience library.",
        PermissionKind::Execute,
        "record_experience.record",
        DeclaredPermission::Execute,
        json!({"type":"object","properties":{"category":{"type":"string","minLength":1},"content":{"type":"string","minLength":1}},"required":["category","content"],"additionalProperties":false}),
    );
    let recall_manifest = manifest(
        "recall_experience",
        "Read this agent's experience index, or a specific category.",
        PermissionKind::Read,
        "recall_experience.read",
        DeclaredPermission::ReadOnly,
        json!({"type":"object","properties":{"category":{"type":"string"}},"additionalProperties":false}),
    );
    for (name, m) in [
        ("search_memory", search_manifest),
        ("pin_memory", pin_manifest),
        ("unpin_memory", unpin_manifest),
        ("tenet_propose", tenet_manifest),
        ("context_notes", notes_manifest),
        ("install_skill", install_manifest),
        ("learn_lesson", lesson_manifest),
        ("record_experience", record_manifest),
        ("recall_experience", recall_manifest),
    ] {
        // Registration failure is a LOUD startup error, never a silent skip —
        // a quietly missing tool is exactly the failure mode the A11 tool
        // surface must not have (R06-T06 R1 F-03; filetools.rs `.expect`
        // precedent).
        let receipt = registry
            .register(m, budget)
            .expect("the resident memory/skill tools register");
        let executor = Arc::new(MemoryToolExecutor {
            tools: Arc::clone(&tools),
        });
        if name == "unpin_memory" {
            let scoped = Arc::clone(&tools);
            gateway.bind_executor_with_review(receipt.target_id, executor, Arc::new(move |input, args| {
                if input.agent_id != scoped.agent_id {
                    return Err("pinned-memory review belongs to another agent; no items were read".into());
                }
                let agent = scoped.bound_agent().map_err(|_| "agent is unavailable".to_string())?;
                let store = scoped.memory.tenets_store(&agent).map_err(|err| err.to_string())?;
                let items = matched_pins(&store, args.as_value()).map_err(|err| err.to_string())?;
                Ok(json!({"agentId":agent,"items":items.iter().map(|(id,content)|json!({"id":id,"content":content})).collect::<Vec<_>>()}))
            }), "R06 pinned-memory deletion review scope");
        } else {
            gateway.bind_executor(
                receipt.target_id,
                executor,
                "R06-T06 memory/skill tool executor (per-agent memory surface)",
            );
        }
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

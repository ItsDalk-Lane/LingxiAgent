//! 模型历史是独立于展示正文的持久档案。专用树节点沿有效分支保留，
//! fork复制完整内容而不持有源Run外键；恢复只读，永不重新执行工具。
use lingxi_adapters::storage::{
    canonical_message_store as cms, session_tree as store, RunDatabase,
};
use lingxi_kernel::{
    model_exchange::{ExchangeItem, RequestedToolCall, TurnOrigin},
    ports::{StorageError, StoragePort, ToolOutcome, ToolRunStatus, ToolSuccess},
    toolcatalog::{EffectiveArguments, SchemaBudget},
    Principal, RunContext,
};
use lingxi_protocol::{
    canon, ArgsDigest, ContentBlock, ModelCallId, NormalizedMessage, ToolCallId, ToolResultStatus,
    ToolResultWire,
};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub const ENTRY_TYPE: &str = "lingxi-model-exchange-v1";
pub async fn record_delivered_bridge_notice(
    db: &RunDatabase,
    delivery_id: &str,
    now: u64,
) -> Result<Option<lingxi_adapters::storage::bridge_notice::BridgeNoticeFact>, StorageError> {
    lingxi_adapters::storage::bridge_notice::record_delivered_bridge_notice(db, delivery_id, now)
        .await
}

fn automation_input_run(message_id: &str) -> Option<&str> {
    message_id
        .strip_prefix("model-history:")?
        .strip_suffix(":input")
        .filter(|id| !id.is_empty())
}
fn corrupt(detail: impl Into<String>) -> StorageError {
    StorageError::Corrupted {
        detail: format!("model_history: {}", detail.into()),
    }
}
fn str_field<'a>(v: &'a Value, key: &str) -> Result<&'a str, StorageError> {
    v[key]
        .as_str()
        .ok_or_else(|| corrupt(format!("missing {key}")))
}
fn decode<T: serde::de::DeserializeOwned>(v: &Value) -> Result<T, StorageError> {
    serde_json::from_value(v.clone()).map_err(|e| corrupt(e.to_string()))
}

// 当前读取权限和原始执行身份分别验证：本地主人可读所有会话，其余主体精确归属。
fn can_read(principal: &Principal, owner: &str, session: &str) -> bool {
    principal.can_access_session(owner, session)
}

fn archive_actor(body: &Value) -> Result<Principal, StorageError> {
    let actor = lingxi_kernel::recovery::principal_from_storage_facts(
        str_field(body, "actorKind")?,
        str_field(body, "actorSubject")?,
    )?;
    if !can_read(
        &actor,
        str_field(body, "owner")?,
        str_field(body, "sourceSession")?,
    ) {
        return Err(corrupt(
            "archive original actor is not authorized for session owner",
        ));
    }
    Ok(actor)
}

/// 真正手动操作的提交坐标；与 Run 来源严格分开。
pub type ManualContextCommit = Arc<
    dyn Fn(&mut dyn FnMut() -> Result<(), StorageError>) -> Result<(), StorageError> + Send + Sync,
>;
struct ManualSaveGuard {
    cancel: crate::cancel::CancelScope,
    phase: Arc<std::sync::atomic::AtomicU8>,
    context: ManualContextCommit,
}
impl store::ManualCompactionCommit for ManualSaveGuard {
    fn allowed(&self) -> bool {
        !self.cancel.is_cancelled() && self.phase.load(std::sync::atomic::Ordering::SeqCst) == 0
    }
    fn commit(
        &self,
        write: &mut dyn FnMut() -> Result<(), StorageError>,
    ) -> Result<(), StorageError> {
        (self.context)(&mut || {
            use std::sync::atomic::Ordering::SeqCst;
            // 材料/目录栅栏内与取消共用最终裁决，接着直接执行实际COMMIT。
            if self.cancel.is_cancelled()
                || self.phase.compare_exchange(0, 2, SeqCst, SeqCst).is_err()
            {
                return Err(StorageError::Conflict {
                    detail: "manual compaction cancelled before commit".into(),
                });
            }
            write()
        })
    }
}
pub struct ManualCompactionAction<'a> {
    pub principal: &'a Principal,
    pub agent: &'a str,
    pub session: &'a str,
    pub action_id: &'a str,
    pub pre_head: Option<String>,
    pub pre_revision: u64,
    pub before: &'a [ExchangeItem],
    pub after: &'a [ExchangeItem],
    pub refreshed_context: Value,
    pub cancel: crate::cancel::CancelScope,
    pub commit_phase: Arc<std::sync::atomic::AtomicU8>,
    pub now: u64,
    pub context_commit: ManualContextCommit,
}
fn encoded_items(items: &[ExchangeItem]) -> Result<Vec<Value>, StorageError> {
    items.iter().map(encode_item).collect()
}
fn manual_source(body: &Value) -> Result<&str, StorageError> {
    let action = str_field(body, "sourceAction")?;
    if body["sourceKind"] != "manual_compaction"
        || !action.starts_with("compact-action-")
        || action.len() != "compact-action-".len() + 64
        || !action["compact-action-".len()..]
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
        || body.get("sourceRun").is_some()
        || body.get("attempt").is_some()
    {
        return Err(corrupt(
            "invalid manual action identity; never a Run archive",
        ));
    }
    Ok(action)
}
fn manual_replacement(
    body: &Value,
    parent: Option<&str>,
    before: &[ExchangeItem],
) -> Result<Vec<ExchangeItem>, StorageError> {
    manual_source(body)?;
    let payload = &body["payload"];
    let context = &body["refreshedContext"];
    let method = match context["fresh"].as_bool() {
        Some(true) => "fresh-compact",
        Some(false)
            if context["runtimeMode"] == "lossy_local" && context["zeroModelRequest"] == true =>
        {
            "instant_simple"
        }
        Some(false) => "compact",
        None => return Err(corrupt("manual action fresh/method binding missing")),
    };
    if context["request"]["method"] != method
        || (!context["request"]["customInstructions"].is_null()
            && context["request"]["customInstructions"]
                .as_str()
                .is_none_or(|text| text.len() > 8192))
    {
        return Err(corrupt("manual action request binding invalid"));
    }
    let previous = encoded_items(before)?;
    let expected_digest = canon::sha256_hex(&canon::canonical_json_bytes(&json!(previous)));
    if payload["mode"] != "replace"
        || payload.get("projection").is_some()
        || body["preHead"].as_str() != parent
        || body["preRevision"].as_u64().is_none()
        || body["preHistoryDigest"] != expected_digest
        || !body["refreshedContext"].is_object()
        || !body["refreshedContext"]["tools"].is_object()
        || body["refreshedContext"]["render"].as_str().is_none()
    {
        return Err(corrupt(
            "manual action branch/history/context binding mismatch",
        ));
    }
    let wire = payload["items"]
        .as_array()
        .ok_or_else(|| corrupt("manual archive items missing"))?;
    let cut = wire
        .first()
        .and_then(|item| item["covered"].as_u64())
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| corrupt("manual summary cut missing"))?;
    if cut == 0
        || cut >= previous.len()
        || wire[0]["kind"] != "summary"
        || wire[0]["midRun"] != false
        || wire[1..] != previous[cut..]
    {
        return Err(corrupt(
            "manual replacement differs from retained original history",
        ));
    }
    let items = wire
        .iter()
        .map(decode_item)
        .collect::<Result<Vec<_>, _>>()?;
    validate_pairs(&items)?;
    Ok(items)
}

pub struct ModelHistory {
    storage: Arc<RunDatabase>,
}
impl ModelHistory {
    pub fn new(storage: Arc<RunDatabase>) -> Self {
        Self { storage }
    }
    pub async fn gate(
        &self,
        principal: &Principal,
        agent: &str,
        session: &str,
    ) -> Result<(), StorageError> {
        let s = self
            .storage
            .get_session(session)
            .await?
            .ok_or_else(|| corrupt("missing session"))?;
        if !can_read(principal, &s.owner_user_id, session) || s.agent_id != agent {
            return Err(StorageError::InvalidRequest {
                detail: "model_history: principal/agent/session mismatch".into(),
            });
        }
        Ok(())
    }
    /// 同进程前台等待者丢弃时，原guard明确Abandoned；恢复准确源Run的未知收据与终态。
    pub async fn recover_abandoned(
        &self,
        principal: &Principal,
        agent: &str,
        session: &str,
        registry: &crate::cancel::CancelRegistry,
        events: &crate::events::EventService,
        now: u64,
    ) -> Result<(), StorageError> {
        self.gate(principal, agent, session).await?;
        let session_owner = store::get_session_row(&self.storage, session)
            .await?
            .ok_or_else(|| corrupt("missing session"))?
            .owner_user_id;
        let rows = store::list_branch_messages(&self.storage, session).await?;
        let mut seen = HashSet::new();
        for row in rows.into_iter().filter(|r| r.entry_type == ENTRY_TYPE) {
            let v: Value =
                serde_json::from_str(&row.content_json).map_err(|e| corrupt(e.to_string()))?;
            let mut binding = v.clone();
            binding
                .as_object_mut()
                .ok_or_else(|| corrupt("archive object"))?
                .remove("digest");
            if str_field(&v, "owner")? != session_owner
                || str_field(&v, "agent")? != agent
                || str_field(&v, "boundSession")? != session
                || str_field(&v, "schema")? != ENTRY_TYPE
                || str_field(&v, "digest")?
                    != canon::sha256_hex(&canon::canonical_json_bytes(&binding))
            {
                return Err(corrupt("abandoned archive binding mismatch"));
            }
            let actor = archive_actor(&v)?;
            if v.get("sourceKind").is_some() {
                manual_source(&v)?;
                continue;
            }
            if str_field(&v, "sourceSession")? == session {
                let run = str_field(&v, "sourceRun")?;
                if seen.insert(run.to_string()) {
                    crate::recovery::RecoveryCoordinator::recover_abandoned_run(
                        &self.storage,
                        events,
                        registry,
                        &actor,
                        session,
                        run,
                        now,
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }
    /// 只恢复当前有效链；当前输入已写入，按明确本Run标识排除一次。
    pub async fn load(
        &self,
        principal: &Principal,
        agent: &str,
        session: &str,
        current_run: &str,
    ) -> Result<Vec<ExchangeItem>, StorageError> {
        self.load_at_boundary(principal, agent, session, current_run, None)
            .await
    }
    /// 分叉只预检实际保留前缀；旧无专档UI分叉仍保留原复制合同，恢复时再诊断缺史。
    pub(crate) async fn validate_fork_prefix(
        &self,
        principal: &Principal,
        agent: &str,
        session: &str,
        boundary: &str,
    ) -> Result<(), StorageError> {
        self.load_at_boundary(principal, agent, session, "", Some(boundary))
            .await?;
        Ok(())
    }
    async fn load_at_boundary(
        &self,
        principal: &Principal,
        agent: &str,
        session: &str,
        current_run: &str,
        boundary: Option<&str>,
    ) -> Result<Vec<ExchangeItem>, StorageError> {
        self.gate(principal, agent, session).await?;
        let session_owner = store::get_session_row(&self.storage, session)
            .await?
            .ok_or_else(|| corrupt("missing session"))?
            .owner_user_id;
        let mut rows = store::list_branch_messages(&self.storage, session).await?;
        if let Some(boundary) = boundary {
            let index = rows
                .iter()
                .position(|row| row.message_id == boundary)
                .ok_or_else(|| StorageError::InvalidRequest {
                    detail: "fork target is not on the current branch".into(),
                })?;
            rows.truncate(index + 1);
            if !rows.iter().any(|row| {
                row.entry_type == ENTRY_TYPE
                    || row.entry_type == lingxi_adapters::storage::bridge_notice::ENTRY_TYPE
                    || row.entry_type
                        == lingxi_adapters::storage::conversations_collab::DECISION_ENTRY_TYPE
            }) {
                return Ok(Vec::new());
            }
        }
        let branch_run_inputs: HashSet<String> = rows
            .iter()
            .filter_map(|r| {
                r.message_id
                    .strip_prefix("user:")
                    .or_else(|| {
                        (r.entry_type == ENTRY_TYPE)
                            .then(|| automation_input_run(&r.message_id))
                            .flatten()
                    })
                    .map(str::to_string)
            })
            .collect();
        let mut lineage_sessions = HashSet::new();
        let mut ancestor = Some(session.to_string());
        while let Some(id) = ancestor.take() {
            if lineage_sessions.len() > lingxi_kernel::session_tree::MAX_FORK_LINEAGE_DEPTH as usize
                || !lineage_sessions.insert(id.clone())
            {
                return Err(corrupt("session fork lineage cycle/depth corruption"));
            }
            let row = store::get_session_row(&self.storage, &id)
                .await?
                .ok_or_else(|| corrupt("missing fork ancestor"))?;
            if row.owner_user_id != session_owner || row.agent_id != agent {
                return Err(corrupt("fork ancestor owner/agent mismatch"));
            }
            ancestor = row.parent_session_id;
        }
        let mut exchange = Vec::new();
        let mut archived_calls = HashSet::new();
        let mut archived_turns = HashMap::<String, Value>::new();
        let mut tool_sources = HashMap::<String, (String, String, Principal)>::new();
        let mut verified_argument_refusals = HashSet::new();
        // 旧工具没有完整assistant/签名档案，禁止将UI正文或raw卡伪装为完整历史。
        let old_runs: Vec<String> = rows
            .iter()
            .filter(|r| boundary.is_none() || r.role == "assistant")
            .filter_map(|r| r.run_id.clone())
            .collect();
        let archived_runs: HashSet<String> = rows
            .iter()
            .filter(|r| r.entry_type == ENTRY_TYPE)
            .filter_map(|r| {
                serde_json::from_str::<Value>(&r.content_json)
                    .ok()?
                    .get("sourceRun")?
                    .as_str()
                    .map(str::to_string)
            })
            .collect();
        let mut source_runs = old_runs.clone();
        source_runs.extend(archived_runs.iter().cloned());
        let mut child_ids: HashSet<String> =
            cms::read_lineage_children(&self.storage, &source_runs)
                .await?
                .into_iter()
                .map(|(_, child)| child)
                .collect();
        for row in rows.iter().filter(|r| r.entry_type == ENTRY_TYPE) {
            let v: Value =
                serde_json::from_str(&row.content_json).map_err(|e| corrupt(e.to_string()))?;
            if let Some(excluded) = v.get("excludedChildRuns") {
                child_ids.extend(decode::<Vec<String>>(excluded)?);
            }
        }
        // fork清空Run外键，但命名模型调用仍须由精确usage与原谱系证明，不能按时间/后缀猜测。
        let archive_call_ids: HashSet<String> = rows
            .iter()
            .filter(|r| r.entry_type == ENTRY_TYPE)
            .filter_map(|r| serde_json::from_str::<Value>(&r.content_json).ok())
            .flat_map(|v| {
                v["payload"]["items"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            })
            .filter_map(|v| v["call"].as_str().map(str::to_string))
            .collect();
        let mut fork_facts = HashMap::<String, Value>::new();
        for row in rows
            .iter()
            .filter(|r| r.entry_type == "message" && r.role == "assistant" && r.run_id.is_none())
        {
            if child_ids
                .iter()
                .any(|id| row.message_id == format!("{id}-final"))
            {
                continue;
            }
            let Some(call) = &row.model_call_id else {
                continue;
            };
            if archive_call_ids.contains(call) {
                continue;
            }
            let facts = self.storage.query_one_text("SELECT json_object('run',r.run_id,'session',r.session_id,'owner',s.owner_user_id,'runOwner',r.owner_subject,'runKind',r.owner_kind,'agent',s.agent_id,'purpose',m.purpose,'parent',l.parent_run_id,'origin',l.origin,'parentOwner',ps.owner_user_id,'parentRunOwner',pr.owner_subject,'parentRunKind',pr.owner_kind,'parentAgent',ps.agent_id,'parentSession',pr.session_id,'toolCount',(SELECT COUNT(*) FROM key_events e WHERE e.run_id=r.run_id AND e.event_type IN ('tool_call_started','tool_call_completed'))) FROM model_call_usage m JOIN runs r ON r.run_id=m.run_id AND r.session_id=m.session_id JOIN sessions s ON s.session_id=r.session_id LEFT JOIN run_lineage l ON l.run_id=r.run_id LEFT JOIN runs pr ON pr.run_id=l.parent_run_id LEFT JOIN sessions ps ON ps.session_id=pr.session_id WHERE m.model_call_id=?1",vec![call.clone()]).await?
                .ok_or_else(|| corrupt("legacy_fork_source_unavailable: named model call lacks authoritative source; originals retained"))?;
            let facts: Value = serde_json::from_str(&facts).map_err(|e| corrupt(e.to_string()))?;
            let original_actor = lingxi_kernel::recovery::principal_from_storage_facts(
                str_field(&facts, "runKind")?,
                str_field(&facts, "runOwner")?,
            )?;
            if facts["owner"] != session_owner
                || !can_read(
                    &original_actor,
                    &session_owner,
                    facts["session"]
                        .as_str()
                        .ok_or_else(|| corrupt("legacy source session missing"))?,
                )
                || facts["agent"] != agent
                || facts["purpose"] != "chat"
                || !lineage_sessions.contains(str_field(&facts, "session")?)
            {
                return Err(corrupt("legacy fork model call scope/purpose disagreement"));
            }
            if let Some(parent) = facts["parent"].as_str() {
                if facts["origin"] != "subagent"
                    || facts["parentOwner"] != session_owner
                    || facts["parentRunOwner"] != facts["runOwner"]
                    || facts["parentRunKind"] != facts["runKind"]
                    || facts["parentAgent"] != agent
                    || facts["parentSession"] != facts["session"]
                    || !rows.iter().any(|r| {
                        r.message_id == format!("user:{parent}")
                            || r.message_id == format!("{parent}-final")
                    })
                {
                    return Err(corrupt(
                        "legacy fork child lineage is not bound to retained parent",
                    ));
                }
                child_ids.insert(str_field(&facts, "run")?.to_string());
            } else if !branch_run_inputs.contains(str_field(&facts, "run")?) {
                // 子Run原合同没有独立用户消息；普通Run必须绑定其精确保留输入。
                return Err(corrupt("legacy fork model call lacks retained input"));
            }
            fork_facts.insert(call.clone(), facts);
        }
        // 完整新档案无需加载raw工具大卡，只对明确缺档案的旧Run查兼容边界。
        let legacy_runs: Vec<String> = old_runs
            .into_iter()
            .filter(|r| !archived_runs.contains(r) && !child_ids.contains(r))
            .collect();
        let old_events = cms::read_branch_events(&self.storage, session, &legacy_runs).await?;
        for event in old_events {
            if let Some(run) = &event.run_id {
                if !child_ids.contains(run)
                    && !archived_runs.contains(run)
                    && matches!(
                        &event.payload,
                        lingxi_protocol::EventPayload::Known(
                            lingxi_protocol::KnownEventPayload::ToolCallStarted(_)
                                | lingxi_protocol::KnownEventPayload::ToolCallCompleted(_)
                        )
                    )
                {
                    return Err(corrupt(
                        "legacy_tool_archive_unavailable: bounded compatibility refuses raw-only/incomplete tool history; original archive retained",
                    ));
                }
            }
        }
        for row in rows {
            if row.run_id.as_ref().is_some_and(|id| child_ids.contains(id))
                || child_ids.iter().any(|id| {
                    row.message_id == format!("user:{id}")
                        || row.message_id == format!("{id}-final")
                })
            {
                continue;
            }
            if row.message_id == format!("user:{current_run}") {
                continue;
            }
            // 决策是可核对的界面收据，不是新的用户指令或普通聊天内容。
            if row.entry_type == lingxi_adapters::storage::conversations_collab::DECISION_ENTRY_TYPE
            {
                continue;
            }
            if row.entry_type == lingxi_adapters::storage::bridge_notice::ENTRY_TYPE {
                let fact: lingxi_adapters::storage::bridge_notice::BridgeNoticeFact =
                    serde_json::from_str(&row.content_json)
                        .map_err(|_| corrupt("通知事实快照损坏"))?;
                let saved = lingxi_adapters::storage::bridge_notice::load_notice_fact(
                    &self.storage,
                    &fact.delivery_id,
                )
                .await?
                .ok_or_else(|| corrupt("通知事实缺少真实送达收据"))?;
                if saved != fact
                    || fact.actor_subject != session_owner
                    || fact.agent_id != agent
                    || row.role != "host_context"
                    || row.run_id.is_some()
                    || row.message_id
                        != format!(
                            "bridge-notice:{}",
                            canon::sha256_hex(fact.delivery_id.as_bytes())
                        )
                {
                    return Err(corrupt("通知事实身份或来源不匹配"));
                }
                if let Some(source) =
                    store::get_session_row(&self.storage, &fact.session_id).await?
                {
                    if source.owner_user_id != session_owner
                        || source.agent_id != agent
                        || !lineage_sessions.contains(&fact.session_id)
                    {
                        return Err(corrupt("通知事实不属于当前保留分支"));
                    }
                }
                exchange.push(ExchangeItem::HostNotice {
                    delivery_id: fact.delivery_id,
                    text: fact.text,
                });
                continue;
            }
            if row.entry_type == ENTRY_TYPE {
                let v: Value =
                    serde_json::from_str(&row.content_json).map_err(|e| corrupt(e.to_string()))?;
                if str_field(&v, "owner")? != session_owner
                    || str_field(&v, "agent")? != agent
                    || str_field(&v, "boundSession")? != session
                    || str_field(&v, "schema")? != ENTRY_TYPE
                {
                    return Err(corrupt("archive scope/schema mismatch"));
                }
                str_field(&v, "sourceSession")?;
                let payload = &v["payload"];
                let mut binding = v.clone();
                binding
                    .as_object_mut()
                    .ok_or_else(|| corrupt("archive object"))?
                    .remove("digest");
                if str_field(&v, "digest")?
                    != canon::sha256_hex(&canon::canonical_json_bytes(&binding))
                {
                    return Err(corrupt("archive digest mismatch"));
                }
                let actor = archive_actor(&v)?;
                if v.get("sourceKind").is_some() {
                    let action = manual_source(&v)?;
                    let source_session = str_field(&v, "sourceSession")?;
                    if row.message_id != format!("model-history:manual:{action}")
                        || source_session.is_empty()
                    {
                        return Err(corrupt("manual action row/source lineage disagreement"));
                    }
                    // fork复制的是完整自足档案，源删除后不持有假Run外键；源仍在时必须核准谱系。
                    if let Some(source) =
                        store::get_session_row(&self.storage, source_session).await?
                    {
                        if source.owner_user_id != session_owner
                            || source.agent_id != agent
                            || !lineage_sessions.contains(source_session)
                        {
                            return Err(corrupt(
                                "manual action source owner/agent/lineage disagreement",
                            ));
                        }
                    }
                    exchange = manual_replacement(&v, row.parent_message_id.as_deref(), &exchange)?;
                    continue;
                }
                let source_run = str_field(&v, "sourceRun")?;
                let source_session = str_field(&v, "sourceSession")?;
                if !branch_run_inputs.contains(source_run) {
                    return Err(corrupt("archive source Run lacks retained input anchor"));
                }
                if source_run.is_empty() || source_session.is_empty() {
                    return Err(corrupt("empty archive source identity"));
                }
                let source = self
                    .storage
                    .load_run(&lingxi_protocol::RunId::new(source_run))
                    .await?;
                let source_exists = source.is_some();
                if let Some(source) = source {
                    if source.session_id.as_str() != source_session {
                        return Err(corrupt("archive source Run/session disagreement"));
                    }
                    if !lineage_sessions.contains(source_session) {
                        return Err(corrupt(
                            "archive source session is not a retained fork ancestor",
                        ));
                    }
                    self.gate(&actor, agent, source_session).await?;
                    let source_owner = store::get_session_row(&self.storage, source_session)
                        .await?
                        .ok_or_else(|| corrupt("archive source session missing"))?
                        .owner_user_id;
                    let facts = self.storage.query_one_text(
                        "SELECT json_object('kind',owner_kind,'subject',owner_subject) FROM runs WHERE run_id=?1",
                        vec![source_run.to_string()]).await?
                        .ok_or_else(|| corrupt("archive source Run actor missing"))?;
                    let facts: Value =
                        serde_json::from_str(&facts).map_err(|e| corrupt(e.to_string()))?;
                    if source_owner != session_owner
                        || facts["kind"] != actor.storage_kind()
                        || facts["subject"] != actor.storage_subject()
                    {
                        return Err(corrupt(
                            "archive source Run actor/session owner disagreement",
                        ));
                    }
                }
                if let Some(origin) = v.get("inputOrigin") {
                    let origin = origin
                        .as_str()
                        .ok_or_else(|| corrupt("automation input origin missing"))?;
                    let input_items = payload["items"]
                        .as_array()
                        .ok_or_else(|| corrupt("automation input items missing"))?;
                    if !matches!(
                        origin,
                        "cron" | "heartbeat" | "channel" | "dm" | "session_collab" | "loop"
                    ) || row.message_id != format!("model-history:{source_run}:input")
                        || row.role != "model_history"
                        || payload["mode"] != "append"
                        || payload.get("projection").is_some()
                        || input_items.len() != 1
                        || input_items[0]["kind"] != "user"
                        || input_items[0]["text"].as_str().is_none()
                    {
                        return Err(corrupt("automation input archive binding mismatch"));
                    }
                    let source_lineage = self
                        .storage
                        .load_run_lineage(&lingxi_protocol::RunId::new(source_run))
                        .await?;
                    if let Some(lineage) = source_lineage {
                        if lineage.origin.wire_name() != origin
                            || lineage.cause_id.as_deref().is_none_or(str::is_empty)
                        {
                            return Err(corrupt("automation input source lineage mismatch"));
                        }
                    } else if source_exists {
                        return Err(corrupt("automation input source lineage missing"));
                    }
                    // 本轮提交另由驱动传给模型；历史读取才恢复已完成后台任务的输入。
                    if source_run != current_run {
                        exchange.push(decode_item(&input_items[0])?);
                    }
                    continue;
                } else if automation_input_run(&row.message_id).is_some() {
                    return Err(corrupt("automation input anchor lacks source origin"));
                }
                if let Some(projection) = payload.get("projection") {
                    let host_id = str_field(projection, "toolCallId")?;
                    if str_field(projection, "rawEventId")?
                        != format!("{}-{host_id}-done", str_field(&v, "sourceRun")?)
                    {
                        return Err(corrupt("projection raw event/source identity disagreement"));
                    }
                    let requested = exchange
                        .iter()
                        .rev()
                        .find_map(|item| match item {
                            ExchangeItem::AssistantTurn { tool_calls, .. } => tool_calls
                                .iter()
                                .find(|call| call.tool_call_id.as_str() == host_id),
                            _ => None,
                        })
                        .ok_or_else(|| corrupt("projection lacks authoritative request"))?;
                    let expected_target = requested
                        .declaration_binding
                        .as_ref()
                        .and_then(|b| b.execution.as_ref())
                        .map(|e| e.target.as_str())
                        .unwrap_or(requested.target.as_str());
                    if str_field(projection, "target")? != expected_target {
                        return Err(corrupt("projection target/request disagreement"));
                    }
                    if let Some(execution) = requested
                        .declaration_binding
                        .as_ref()
                        .and_then(|b| b.execution.as_ref())
                    {
                        if self
                            .storage
                            .load_run(&lingxi_protocol::RunId::new(source_run))
                            .await?
                            .is_some()
                        {
                            let journal = self
                                .storage
                                .load_invocation_journal(&lingxi_protocol::RunId::new(source_run))
                                .await?;
                            let actual = journal
                                .iter()
                                .find(|entry| entry.journal_id == host_id)
                                .ok_or_else(|| corrupt("目录执行原始journal缺失"))?;
                            if actual.target != execution.target
                                || actual.args_digest != execution.args_digest.hex
                            {
                                return Err(corrupt("目录执行身份与journal不一致"));
                            }
                        }
                    }
                    let digest_mismatch =
                        !requested.arguments.digest_matches(&requested.args_digest);
                    if digest_mismatch {
                        let refusal = payload.get("argumentRefusal").ok_or_else(|| {
                            corrupt("bad argument digest lacks trusted refusal archive")
                        })?;
                        if refusal["run"] != source_run
                            || refusal["session"] != source_session
                            || refusal["owner"] != actor.storage_subject()
                            || refusal["ownerKind"] != actor.storage_kind()
                            || refusal["tool"] != host_id
                            || refusal["target"] != requested.target
                            || refusal["argsDigest"] != requested.arguments.digest().hex
                            || refusal["phase"] != "failed"
                            || refusal["outcome"] != "failed"
                            || refusal["dispatched"] != false
                            || !str_field(refusal, "detail")?
                                .starts_with("not dispatched: args_digest mismatch ")
                        {
                            return Err(corrupt("argument refusal/source/request disagreement"));
                        }
                        if self
                            .storage
                            .load_run(&lingxi_protocol::RunId::new(source_run))
                            .await?
                            .is_some()
                        {
                            let entries = self
                                .storage
                                .load_invocation_journal(&lingxi_protocol::RunId::new(source_run))
                                .await?;
                            let actual = entries
                                .iter()
                                .find(|entry| entry.journal_id == host_id)
                                .ok_or_else(|| {
                                corrupt("argument refusal original journal missing")
                            })?;
                            if argument_refusal_binding(actual)? != *refusal {
                                return Err(corrupt(
                                    "argument refusal original journal disagreement",
                                ));
                            }
                        }
                        verified_argument_refusals.insert(host_id.to_string());
                    } else if payload.get("argumentRefusal").is_some() {
                        return Err(corrupt("unexpected argument refusal archive"));
                    }
                    // 拒绝凭据先核对真实执行账，随后仍严格验证投影及容量，不能让伪造拒绝蒙混过关。
                    let projected = projected_tool_item(projection)?;
                    if digest_mismatch && !is_argument_refusal(&projected, requested) {
                        return Err(corrupt("bad argument digest lacks exact rejection outcome"));
                    }
                    if payload["items"] != json!([encode_item(&projected)?]) {
                        return Err(corrupt("projection/archive disagreement"));
                    }
                }
                let items = payload["items"]
                    .as_array()
                    .ok_or_else(|| corrupt("archive items missing"))?;
                if str_field(payload, "mode")? == "replace" {
                    exchange.clear();
                } else if str_field(payload, "mode")? != "append" {
                    return Err(corrupt("unknown archive mode"));
                }
                for wire_item in items {
                    let item = decode_item(wire_item)?;
                    if let ExchangeItem::AssistantTurn {
                        call,
                        tool_calls,
                        content,
                        origin,
                    } = &item
                    {
                        if content
                            .iter()
                            .any(|block| matches!(block, ContentBlock::Opaque { .. }))
                        {
                            let origin = origin
                                .as_ref()
                                .ok_or_else(|| corrupt("opaque origin unavailable"))?;
                            if origin.provider.is_empty() || origin.model.is_empty() {
                                return Err(corrupt("opaque origin empty"));
                            }
                            // 源仍存在时以真实调用账核准；源删除后的合法fork仅依赖已校验完整专档。
                            if self
                                .storage
                                .load_run(&lingxi_protocol::RunId::new(source_run))
                                .await?
                                .is_some()
                                && payload["mode"] != "replace"
                            {
                                let facts = self.storage.query_one_text(
                                    "SELECT json_object('provider',provider,'model',model,'protocol',protocol) FROM model_call_usage WHERE model_call_id=?1 AND run_id=?2 AND session_id=?3 AND purpose='chat'",
                                    vec![call.as_str().into(), source_run.into(), source_session.into()]).await?
                                    .ok_or_else(|| corrupt("opaque source model call unavailable"))?;
                                let facts: Value = serde_json::from_str(&facts)
                                    .map_err(|e| corrupt(e.to_string()))?;
                                if facts["provider"] != origin.provider || facts["model"] != origin.model
                                    || content.iter().any(|block| matches!(block, ContentBlock::Opaque { provider, .. } if facts["protocol"] != *provider)) {
                                    return Err(corrupt("opaque origin/source model call disagreement"));
                                }
                            }
                        }
                        if payload["mode"] == "replace" {
                            if archived_turns.get(call.as_str()) != Some(wire_item) {
                                return Err(corrupt(
                                    "snapshot assistant differs from original archive",
                                ));
                            }
                        } else if !call.as_str().starts_with(&format!("{source_run}-mc"))
                            || tool_calls.iter().any(|c| {
                                !c.tool_call_id
                                    .as_str()
                                    .starts_with(&format!("{source_run}-tc"))
                            })
                        {
                            return Err(corrupt(
                                "assistant/tool host identity differs from source Run",
                            ));
                        }
                        archived_turns.insert(call.as_str().to_string(), wire_item.clone());
                        archived_calls.insert(call.as_str().to_string());
                        for requested in tool_calls {
                            tool_sources
                                .entry(requested.tool_call_id.as_str().to_string())
                                .or_insert_with(|| {
                                    (
                                        v["sourceSession"].as_str().unwrap_or_default().to_string(),
                                        v["sourceRun"].as_str().unwrap_or_default().to_string(),
                                        actor.clone(),
                                    )
                                });
                        }
                    }
                    exchange.push(item);
                }
                continue;
            }
            if row.entry_type == "hana-session-branch-reset" {
                continue;
            }
            // R07-T07-δ6：loop-user-prompt 是循环任务的展示型气泡
            // （recordUserPrompt 平移），模型上下文恒定排除——展示层投影由
            // history_projection 的 LoopUserPrompt 臂承担。本臂漏配时未知
            // 类型闸会把它误判成未知遗留行（δ 集成测试抓获）。
            if row.entry_type == crate::loopx::LOOP_USER_PROMPT_MESSAGE_TYPE {
                continue;
            }
            if matches!(
                row.entry_type.as_str(),
                "todo-state-v1"
                    | "interactive-question-v1"
                    | "interactive-answer-v1"
                    | "interactive-card-v1"
            ) {
                let source_run = row
                    .run_id
                    .as_deref()
                    .ok_or_else(|| corrupt("interactive UI entry lacks a source Run"))?;
                let source = self
                    .storage
                    .load_run(&lingxi_protocol::RunId::new(source_run))
                    .await?
                    .ok_or_else(|| corrupt("interactive UI source Run is missing"))?;
                let source_session = self
                    .storage
                    .get_session(source.session_id.as_str())
                    .await?
                    .ok_or_else(|| corrupt("interactive UI source session is missing"))?;
                if source_session.owner_user_id != session_owner
                    || source_session.agent_id != agent
                    || !lineage_sessions.contains(source.session_id.as_str())
                    || !branch_run_inputs.contains(source_run)
                {
                    return Err(corrupt("interactive UI source owner/agent/branch mismatch"));
                }
                // 这里只跳过已绑定的页面快照；模型结果仍从真实工具档案恢复。
                continue;
            }
            if row.entry_type != "message" {
                return Err(corrupt(
                    "unknown legacy tree entry; not converted to normal conversation",
                ));
            }
            let value: Value =
                serde_json::from_str(&row.content_json).map_err(|e| corrupt(e.to_string()))?;
            match row.role.as_str() {
                "user" => {
                    let text = if let Some(text) = value
                        .get("modelText")
                        .or_else(|| value.get("text"))
                        .and_then(Value::as_str)
                    {
                        text.to_string()
                    } else {
                        let msg: NormalizedMessage = decode(&value)?;
                        if msg.role != "user" {
                            return Err(corrupt("legacy user role disagreement"));
                        }
                        plain_user_text(&msg.content)?
                    };
                    if let Some(images) = value.get("images") {
                        let images = decode_images(images)?;
                        if images.is_empty() {
                            return Err(corrupt("empty historical media"));
                        }
                        exchange.push(ExchangeItem::UserMediaTurn { text, images });
                    } else {
                        exchange.push(ExchangeItem::UserTurn { text });
                    }
                }
                "assistant" => {
                    if row
                        .model_call_id
                        .as_ref()
                        .is_some_and(|id| archived_calls.contains(id))
                    {
                        continue;
                    }
                    if row.run_id.is_none() {
                        if let Some(call) = &row.model_call_id {
                            let facts = fork_facts
                                .get(call)
                                .ok_or_else(|| corrupt("legacy fork missing exact source facts"))?;
                            if facts["toolCount"].as_u64() != Some(0) {
                                return Err(corrupt(
                                    "legacy_fork_tool_archive_unavailable: raw-only tools cannot be recovered from UI final; originals retained",
                                ));
                            }
                        }
                    }
                    let message: NormalizedMessage = decode(&value)?;
                    if message.role != "assistant"
                        || message.model_call_id.as_ref().map(|id| id.as_str())
                            != row.model_call_id.as_deref()
                    {
                        return Err(corrupt("legacy assistant role/call disagreement"));
                    }
                    if message
                        .content
                        .iter()
                        .any(|b| matches!(b, ContentBlock::Opaque { .. }))
                    {
                        return Err(corrupt("legacy opaque origin unavailable"));
                    }
                    // 明示的纯文本旧记录兼容：没有工具/签名，不猜Run关联，不生成宿主调用身份。
                    tracing::warn!(session_id=session,message_id=%row.message_id,"model_history legacy plain assistant compatibility (origin unknown)");
                    let legacy_call = message
                        .model_call_id
                        .clone()
                        .unwrap_or_else(|| ModelCallId::new(format!("legacy:{}", row.message_id)));
                    let legacy = ExchangeItem::AssistantTurn {
                        call: legacy_call.clone(),
                        content: message.content,
                        tool_calls: Vec::new(),
                        origin: None,
                    };
                    archived_turns.insert(legacy_call.as_str().to_string(), encode_item(&legacy)?);
                    exchange.push(legacy);
                }
                _ => return Err(corrupt("unsupported legacy role")),
            }
        }
        for item in &exchange {
            if let ExchangeItem::AssistantTurn { tool_calls, .. } = item {
                for call in tool_calls {
                    if !call.arguments.digest_matches(&call.args_digest)
                        && !verified_argument_refusals.contains(call.tool_call_id.as_str())
                    {
                        return Err(corrupt(
                            "tool arguments digest mismatch without authoritative refusal",
                        ));
                    }
                }
            }
        }
        for (tool, provider) in pending_pairs(&exchange)? {
            let (source_session, source, actor) = tool_sources
                .get(&tool)
                .ok_or_else(|| corrupt("unconfirmed tool has no explicit source Run"))?;
            self.gate(actor, agent, source_session).await?;
            let record = self
                .storage
                .load_run(&lingxi_protocol::RunId::new(source))
                .await?
                .ok_or_else(|| {
                    corrupt("unconfirmed tool source Run unavailable; explicit rewind required")
                })?;
            let actor_facts = self.storage.query_one_text(
                "SELECT json_object('kind',owner_kind,'subject',owner_subject) FROM runs WHERE run_id=?1",
                vec![source.clone()]).await?.ok_or_else(|| corrupt("unconfirmed source actor missing"))?;
            let actor_facts: Value =
                serde_json::from_str(&actor_facts).map_err(|e| corrupt(e.to_string()))?;
            if actor_facts["kind"] != actor.storage_kind()
                || actor_facts["subject"] != actor.storage_subject()
            {
                return Err(corrupt("unconfirmed tool source Run actor disagreement"));
            }
            if record.session_id.as_str() != source_session {
                return Err(corrupt("unconfirmed tool source Run/session disagreement"));
            }
            if !record.status.is_terminal() {
                return Err(corrupt("pending tool belongs to an active Run"));
            }
            if record.status == lingxi_protocol::RunStatus::Completed {
                return Err(corrupt(
                    "completed source Run has a missing tool archive; corrupted history retained",
                ));
            }
            // 这只是缺少完成档案的恢复诊断，绝非ToolCallCompleted或成功收据；不执行旧工具。
            exchange.push(ExchangeItem::ToolResult{tool_call_id:ToolCallId::new(tool),provider_call_id:provider,outcome:ToolOutcome::Unknown{reason:format!("历史调用所属Run {source} 已终止/中断，但没有完整模型结果档案；本条只描述模型历史未知，真实执行/副作用状态以原收据为准，不能从本诊断判断成功或失败。恢复仅提供Unknown诊断，不重放工具，不视为成功；检查既有结果后再决定新的任务。")}});
        }
        for item in &exchange {
            if let ExchangeItem::HostNotice { delivery_id, text } = item {
                let fact = lingxi_adapters::storage::bridge_notice::load_notice_fact(
                    &self.storage,
                    delivery_id,
                )
                .await?
                .ok_or_else(|| corrupt("历史通知缺少真实送达收据"))?;
                if fact.actor_subject != session_owner
                    || fact.agent_id != agent
                    || fact.text != *text
                {
                    return Err(corrupt("历史通知正文或接收身份不等于送达事实"));
                }
            }
        }
        validate_pairs(&exchange)?;
        Ok(exchange)
    }
    pub async fn record_manual_compaction(
        &self,
        action: ManualCompactionAction<'_>,
    ) -> Result<(), StorageError> {
        self.gate(action.principal, action.agent, action.session)
            .await?;
        let owner = store::get_session_row(&self.storage, action.session)
            .await?
            .ok_or_else(|| corrupt("manual session missing"))?
            .owner_user_id;
        let chain = store::list_branch_messages(&self.storage, action.session).await?;
        let mut source_runs = Vec::new();
        let mut excluded = HashSet::new();
        for row in &chain {
            if let Some(run) = row
                .message_id
                .strip_prefix("user:")
                .or_else(|| automation_input_run(&row.message_id))
            {
                source_runs.push(run.to_string());
            }
            if row.entry_type == ENTRY_TYPE {
                let previous: Value =
                    serde_json::from_str(&row.content_json).map_err(|e| corrupt(e.to_string()))?;
                if let Some(ids) = previous.get("excludedChildRuns") {
                    excluded.extend(decode::<Vec<String>>(ids)?);
                }
            }
        }
        excluded.extend(
            cms::read_lineage_children(&self.storage, &source_runs)
                .await?
                .into_iter()
                .map(|(_, child)| child),
        );
        let mut excluded: Vec<_> = excluded.into_iter().collect();
        excluded.sort();
        let mut body = json!({"schema":ENTRY_TYPE,"sourceKind":"manual_compaction",
            "sourceAction":action.action_id,"owner":owner,"agent":action.agent,
            "actorKind":action.principal.storage_kind(),"actorSubject":action.principal.storage_subject(),
            "sourceSession":action.session,"boundSession":action.session,
            "preHead":action.pre_head,"preRevision":action.pre_revision,
            "preHistoryDigest":canon::sha256_hex(&canon::canonical_json_bytes(&json!(encoded_items(action.before)?))),
            "excludedChildRuns":excluded,"refreshedContext":action.refreshed_context,
            "payload":{"mode":"replace","items":encoded_items(action.after)?}});
        manual_replacement(&body, action.pre_head.as_deref(), action.before)?;
        body["digest"] = json!(canon::sha256_hex(&canon::canonical_json_bytes(&body)));
        let message_id = format!("model-history:manual:{}", action.action_id);
        let allowed = Arc::new(ManualSaveGuard {
            cancel: action.cancel,
            phase: action.commit_phase,
            context: action.context_commit,
        });
        store::append_manual_compaction(
            &self.storage,
            store::AppendMessage {
                session_id: action.session,
                message_id: &message_id,
                parent_message_id: action.pre_head.as_deref(),
                run_id: None,
                role: "model_history",
                content_json: &body.to_string(),
                entry_type: ENTRY_TYPE,
                model_call_id: None,
                now_unix_ms: action.now,
            },
            allowed,
        )
        .await
    }

    /// 后台输入进入专用模型档案，不能伪装成用户消息或省略真实来源。
    pub(crate) async fn record_automation_input(
        &self,
        ctx: &RunContext,
        agent: &str,
        input: &str,
        now: u64,
    ) -> Result<(), StorageError> {
        self.gate(&ctx.principal, agent, ctx.session_id.as_str())
            .await?;
        let run = self
            .storage
            .load_run(&ctx.run_id)
            .await?
            .ok_or_else(|| corrupt("automation input Run missing"))?;
        let lineage = self
            .storage
            .load_run_lineage(&ctx.run_id)
            .await?
            .ok_or_else(|| corrupt("automation input lineage missing"))?;
        let owner = store::get_session_row(&self.storage, ctx.session_id.as_str())
            .await?
            .ok_or_else(|| corrupt("automation input session missing"))?
            .owner_user_id;
        let owned = self.storage.query_one_text(
            "SELECT r.run_id FROM runs r JOIN run_attempts a ON a.run_id=r.run_id WHERE r.run_id=?1 AND r.session_id=?2 AND r.owner_kind=?3 AND r.owner_subject=?4 AND r.generation=?5 AND a.attempt=?6 AND a.generation=r.generation AND r.status='running'",
            vec![ctx.run_id.to_string(),ctx.session_id.to_string(),ctx.principal.storage_kind().into(),ctx.principal.storage_subject(),ctx.generation.to_string(),ctx.attempt.to_string()],
        ).await?;
        use lingxi_kernel::subagent::RunOrigin as Origin;
        let causal_match = match lineage.origin {
            Origin::Channel | Origin::Dm => {
                let id = lineage
                    .cause_id
                    .as_deref()
                    .ok_or_else(|| corrupt("phone input cause missing"))?;
                let d = lingxi_adapters::storage::conversations::get_delivery(&self.storage, id)
                    .await?
                    .ok_or_else(|| corrupt("phone input delivery missing"))?;
                let actual =
                    lingxi_adapters::storage::conversations::get_delivery_run(&self.storage, id)
                        .await?
                        .ok_or_else(|| corrupt("phone input actual Run missing"))?;
                actual.0 == ctx.run_id.as_str()
                    && actual.1 == ctx.session_id.as_str()
                    && d.input.actor_id == agent
                    && d.input.input_json["prompt"] == input
            }
            Origin::SessionCollab => {
                let id = lineage
                    .cause_id
                    .as_deref()
                    .ok_or_else(|| corrupt("collaboration input cause missing"))?;
                let d = lingxi_adapters::storage::conversations_collab::get_collab_draft(
                    &self.storage,
                    id,
                )
                .await?
                .ok_or_else(|| corrupt("collaboration draft missing"))?;
                let actual = lingxi_adapters::storage::conversations_collab::get_collab_run(
                    &self.storage,
                    id,
                )
                .await?
                .ok_or_else(|| corrupt("collaboration actual Run missing"))?;
                let payload: Value = serde_json::from_str(
                    d.frozen_apply_json
                        .as_deref()
                        .ok_or_else(|| corrupt("collaboration frozen input missing"))?,
                )
                .map_err(|e| corrupt(e.to_string()))?;
                let source = store::get_session_row(&self.storage, &d.input.source_session_id)
                    .await?
                    .ok_or_else(|| corrupt("collaboration source session missing"))?;
                let body = payload["message"]
                    .as_str()
                    .or_else(|| payload["firstMessage"].as_str())
                    .ok_or_else(|| corrupt("collaboration message missing"))?;
                actual.0 == ctx.run_id.as_str()
                    && actual.1 == ctx.session_id.as_str()
                    && crate::session_collab::collaboration_input(&source.agent_id, body) == input
            }
            Origin::Cron | Origin::Heartbeat => true,
            // R07-T07-δ6：Loop 唤醒轮的因果锚是循环控制器自身的派发闸
            // （execute_scheduled_for 只受理 Cron/Heartbeat/Loop 且
            // trigger_id=循环键非空），本层无可回读的第二来源，与
            // Cron/Heartbeat 同口径。
            Origin::Loop => true,
            _ => false,
        };
        let actor_allowed = ctx.principal == Principal::LocalUser
            || (lineage.origin == Origin::SessionCollab
                && matches!(ctx.principal, Principal::Device { .. }));
        if !actor_allowed
            || !causal_match
            || owner != ctx.principal.storage_subject()
            || run.session_id != ctx.session_id
            || owned.is_none()
            || !matches!(
                lineage.origin,
                Origin::Cron
                    | Origin::Heartbeat
                    | Origin::Channel
                    | Origin::Dm
                    | Origin::SessionCollab
                    | Origin::Loop
            )
            || lineage.cause_id.as_deref().is_none_or(str::is_empty)
        {
            return Err(corrupt("automation input Run/principal/lineage mismatch"));
        }
        let mut body = json!({"schema":ENTRY_TYPE,"owner":owner,"actorKind":ctx.principal.storage_kind(),
            "actorSubject":ctx.principal.storage_subject(),"agent":agent,"sourceSession":ctx.session_id,
            "boundSession":ctx.session_id,"sourceRun":ctx.run_id,"inputOrigin":lineage.origin.wire_name(),
            "payload":{"mode":"append","items":[encode_item(&ExchangeItem::UserTurn{text:input.to_string()})?]}});
        body["digest"] = json!(canon::sha256_hex(&canon::canonical_json_bytes(&body)));
        let chain = store::list_branch_messages(&self.storage, ctx.session_id.as_str()).await?;
        let message_id = format!("model-history:{}:input", ctx.run_id);
        let encoded = body.to_string();
        if let Some(existing) = chain.iter().find(|row| row.message_id == message_id) {
            return if existing.role == "model_history"
                && existing.entry_type == ENTRY_TYPE
                && existing.content_json == encoded
            {
                Ok(())
            } else {
                Err(corrupt("automation input replay changed"))
            };
        }
        if !chain.is_empty() && matches!(lineage.origin, Origin::Cron | Origin::Heartbeat) {
            return Err(corrupt("automation input requires its new activity branch"));
        }
        let parent = chain.last().map(|row| row.message_id.as_str());
        store::append_message(
            &self.storage,
            store::AppendMessage {
                session_id: ctx.session_id.as_str(),
                message_id: &message_id,
                parent_message_id: parent,
                run_id: None,
                role: "model_history",
                content_json: &encoded,
                entry_type: ENTRY_TYPE,
                model_call_id: None,
                now_unix_ms: now,
            },
        )
        .await
    }

    /// 每一步先保存到同一消息树，再进入下一请求；摘要替换只覆盖模型投影。
    #[allow(clippy::too_many_arguments)]
    pub async fn record(
        &self,
        ctx: &RunContext,
        agent: &str,
        items: &[ExchangeItem],
        replace: bool,
        sequence: u32,
        now: u64,
        projection: Option<&Value>,
    ) -> Result<(), StorageError> {
        self.gate(&ctx.principal, agent, ctx.session_id.as_str())
            .await?;
        let mut payload = json!({"mode":if replace{"replace"}else{"append"},"items":items.iter().map(encode_item).collect::<Result<Vec<_>,_>>()?});
        if let Some(projection) = projection {
            payload["projection"] = projection.clone();
        }
        let owner = store::get_session_row(&self.storage, ctx.session_id.as_str())
            .await?
            .ok_or_else(|| corrupt("record session missing"))?
            .owner_user_id;
        let mut body = json!({"schema":ENTRY_TYPE,"owner":owner,"actorKind":ctx.principal.storage_kind(),"actorSubject":ctx.principal.storage_subject(),"agent":agent,"sourceSession":ctx.session_id.as_str(),"boundSession":ctx.session_id.as_str(),"sourceRun":ctx.run_id.as_str(),"payload":payload});
        body["digest"] = json!(canon::sha256_hex(&canon::canonical_json_bytes(&body)));
        let chain = store::list_branch_messages(&self.storage, ctx.session_id.as_str()).await?;
        if let Some(projection) = projection {
            let host = str_field(projection, "toolCallId")?;
            let mut bad_request = None;
            for row in chain.iter().filter(|row| row.entry_type == ENTRY_TYPE) {
                let previous: Value =
                    serde_json::from_str(&row.content_json).map_err(|e| corrupt(e.to_string()))?;
                if let Some(items) = previous["payload"]["items"].as_array() {
                    for item in items {
                        if item["kind"] == "assistant" {
                            if let ExchangeItem::AssistantTurn { tool_calls, .. } =
                                decode_item(item)?
                            {
                                bad_request = tool_calls
                                    .into_iter()
                                    .find(|call| {
                                        call.tool_call_id.as_str() == host
                                            && !call.arguments.digest_matches(&call.args_digest)
                                    })
                                    .or(bad_request);
                            }
                        }
                    }
                }
            }
            if let Some(request) = bad_request {
                if items.len() != 1 || !is_argument_refusal(&items[0], &request) {
                    return Err(corrupt("bad request result is not exact refusal"));
                }
                let journal = self.storage.load_invocation_journal(&ctx.run_id).await?;
                let entry = journal
                    .iter()
                    .find(|entry| entry.journal_id == host)
                    .ok_or_else(|| corrupt("refusal journal missing before archive"))?;
                if entry.run_id != ctx.run_id.as_str()
                    || entry.session_id != ctx.session_id.as_str()
                    || entry.owner_subject != ctx.principal.storage_subject()
                    || entry.owner_kind != ctx.principal.storage_kind()
                    || entry.target != request.target
                    || entry.args_digest != request.arguments.digest().hex
                {
                    return Err(corrupt("refusal journal binding mismatch"));
                }
                body["payload"]["argumentRefusal"] = argument_refusal_binding(entry)?;
            }
        }
        let input_index = chain
            .iter()
            .position(|r| {
                r.message_id == format!("user:{}", ctx.run_id)
                    || (r.entry_type == ENTRY_TYPE
                        && r.message_id == format!("model-history:{}:input", ctx.run_id))
            })
            .ok_or_else(|| corrupt("current Run input left active branch"))?;
        let children: HashSet<String> =
            cms::read_lineage_children(&self.storage, &[ctx.run_id.as_str().to_string()])
                .await?
                .into_iter()
                .map(|(_, child)| child)
                .collect();
        // 本次输入之后只能有本Run档案/正文和绑定child正文；新reset或其它writer不能混入。
        if chain[input_index + 1..].iter().any(|r| {
            !(r.entry_type == ENTRY_TYPE
                && r.message_id
                    .starts_with(&format!("model-history:{}:", ctx.run_id)))
                && !(matches!(
                    r.entry_type.as_str(),
                    "todo-state-v1"
                        | "interactive-question-v1"
                        | "interactive-answer-v1"
                        | "interactive-card-v1"
                ) && r.run_id.as_deref() == Some(ctx.run_id.as_str()))
                && !(r.entry_type == "message"
                    && (r.run_id.as_deref() == Some(ctx.run_id.as_str())
                        || r.run_id.as_ref().is_some_and(|id| children.contains(id))
                        || children
                            .iter()
                            .any(|id| r.message_id == format!("user:{id}"))))
        }) {
            return Err(corrupt(
                "current Run branch changed by reset/non-child writer",
            ));
        }
        let mut excluded = children.clone();
        let mut sources = vec![ctx.run_id.as_str().to_string()];
        for row in chain.iter().filter(|r| r.entry_type == ENTRY_TYPE) {
            let old: Value =
                serde_json::from_str(&row.content_json).map_err(|e| corrupt(e.to_string()))?;
            if old.get("sourceKind").is_some() {
                manual_source(&old)?;
            } else {
                sources.push(str_field(&old, "sourceRun")?.to_string());
            }
            if let Some(ids) = old.get("excludedChildRuns") {
                excluded.extend(decode::<Vec<String>>(ids)?);
            }
        }
        excluded.extend(
            cms::read_lineage_children(&self.storage, &sources)
                .await?
                .into_iter()
                .map(|(_, child)| child),
        );
        let mut excluded: Vec<String> = excluded.into_iter().collect();
        excluded.sort();
        body["excludedChildRuns"] = json!(excluded);
        body.as_object_mut().expect("档案为对象").remove("digest");
        body["digest"] = json!(canon::sha256_hex(&canon::canonical_json_bytes(&body)));
        let mut parent = chain.last().map(|r| r.message_id.clone());
        loop {
            let result = store::append_message(
                &self.storage,
                store::AppendMessage {
                    session_id: ctx.session_id.as_str(),
                    message_id: &format!("model-history:{}:{sequence}", ctx.run_id),
                    parent_message_id: parent.as_deref(),
                    run_id: None,
                    role: "model_history",
                    content_json: &body.to_string(),
                    entry_type: ENTRY_TYPE,
                    model_call_id: None,
                    now_unix_ms: now,
                },
            )
            .await;
            if !matches!(&result, Err(StorageError::InvalidRequest{detail}) if detail == "model_history: branch head changed before archive commit")
            {
                return result;
            }
            // 只对真实child的并发正文追加重新接头；reset、异枝、其它writer均不能重试。
            let chain = store::list_branch_messages(&self.storage, ctx.session_id.as_str()).await?;
            let from = match &parent {
                Some(id) => chain
                    .iter()
                    .position(|r| &r.message_id == id)
                    .map(|i| i + 1)
                    .ok_or_else(|| corrupt("archive parent left active branch"))?,
                None => 0,
            };
            let children: HashSet<String> =
                cms::read_lineage_children(&self.storage, &[ctx.run_id.as_str().to_string()])
                    .await?
                    .into_iter()
                    .map(|(_, child)| child)
                    .collect();
            let added = &chain[from..];
            if added.is_empty()
                || added.len() > 1024
                || added.iter().any(|r| {
                    r.entry_type != "message"
                        || !r.run_id.as_ref().is_some_and(|id| children.contains(id))
                            && !children
                                .iter()
                                .any(|id| r.message_id == format!("user:{id}"))
                })
            {
                return Err(corrupt("archive head advanced by non-child writer/reset"));
            }
            parent = chain.last().map(|r| r.message_id.clone());
        }
    }
}
fn plain_user_text(blocks: &[ContentBlock]) -> Result<String, StorageError> {
    blocks
        .iter()
        .map(|b| match b {
            ContentBlock::Text { text } => Ok(text.clone()),
            _ => Err(corrupt("legacy user nontext not safely replayable")),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|x| x.join("\n"))
}

pub fn encode_declaration_binding(
    binding: &lingxi_kernel::model_exchange::HistoricalToolDeclaration,
) -> Value {
    let d = &binding.declaration;
    json!({"catalogGeneration":binding.catalog_generation,"declaration":{"target":d.target,"wireName":d.wire_name,"description":d.description,"inputSchema":d.input_schema},"execution":binding.execution.as_ref().map(|e|json!({"target":e.target,"catalogGeneration":e.catalog_generation,"arguments":e.arguments,"argsDigest":e.args_digest}))})
}
fn decode_declaration_binding(
    v: &Value,
) -> Result<lingxi_kernel::model_exchange::HistoricalToolDeclaration, StorageError> {
    use lingxi_kernel::model_exchange::{
        HistoricalToolDeclaration, HistoricalToolExecution, ToolDeclaration,
    };
    let generation = v["catalogGeneration"]
        .as_u64()
        .filter(|g| *g > 0)
        .ok_or_else(|| corrupt("可信历史声明代次缺失"))?;
    let d = &v["declaration"];
    let declaration = ToolDeclaration {
        target: str_field(d, "target")?.into(),
        wire_name: str_field(d, "wireName")?.into(),
        description: str_field(d, "description")?.into(),
        input_schema: decode(&d["inputSchema"])?,
    };
    if declaration.wire_name.is_empty()
        || declaration.wire_name.len() > 64
        || !declaration
            .wire_name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(corrupt("历史声明名称非法"));
    }
    let execution = if v["execution"].is_null() {
        None
    } else {
        let e = &v["execution"];
        let arguments =
            EffectiveArguments::from_value(e["arguments"].clone(), &SchemaBudget::default())
                .map_err(|error| corrupt(error.to_string()))?;
        let args_digest = decode(&e["argsDigest"])?;
        if declaration.target != "tool:first-party:mcp_call"
            || !arguments.digest_matches(&args_digest)
        {
            return Err(corrupt("目录历史执行绑定非法"));
        }
        Some(HistoricalToolExecution {
            target: str_field(e, "target")?.into(),
            catalog_generation: e["catalogGeneration"]
                .as_u64()
                .filter(|g| *g > 0)
                .ok_or_else(|| corrupt("目录历史执行代次缺失"))?,
            arguments: arguments.as_value().clone(),
            args_digest,
        })
    };
    Ok(HistoricalToolDeclaration {
        catalog_generation: generation,
        declaration,
        execution,
    })
}

pub(crate) fn encode_item(item: &ExchangeItem) -> Result<Value, StorageError> {
    Ok(match item {
        ExchangeItem::UserMediaTurn { text, images } => {
            json!({"kind":"user_media","text":text,"images":encode_images(images)})
        }
        ExchangeItem::UserTurn { text } => json!({"kind":"user","text":text}),
        ExchangeItem::HostNotice { delivery_id, text } => {
            json!({"kind":"host_notice","deliveryId":delivery_id,"text":text})
        }
        ExchangeItem::AssistantTurn {
            call,
            content,
            tool_calls,
            origin,
        } => {
            json!({"kind":"assistant","call":call.as_str(),"content":content,"calls":tool_calls.iter().map(|c|json!({"id":c.tool_call_id.as_str(),"providerId":c.provider_call_id,"target":c.target,"arguments":c.arguments.as_value(),"digest":c.args_digest,"summary":c.args_summary,"declarationBinding":c.declaration_binding.as_ref().map(encode_declaration_binding)})).collect::<Vec<_>>(),"origin":origin.as_ref().map(|o|json!({"provider":o.provider,"model":o.model}))})
        }
        ExchangeItem::ToolResult {
            tool_call_id,
            provider_call_id,
            outcome,
        } => {
            json!({"kind":"tool","id":tool_call_id.as_str(),"providerId":provider_call_id,"outcome":encode_outcome(outcome)})
        }
        ExchangeItem::CompactionSummary {
            summary,
            covered_items,
            mid_run,
        } => json!({"kind":"summary","summary":summary,"covered":covered_items,"midRun":mid_run}),
        ExchangeItem::CurrentSubmission | ExchangeItem::CompactionInstruction { .. } => {
            return Err(corrupt("transient instruction cannot be archived"));
        }
    })
}
fn encode_outcome(outcome: &ToolOutcome) -> Value {
    match outcome {
        ToolOutcome::Success { result } => {
            let mut value = json!({"status":"success","content":result.content,"refs":result.resource_refs,"truncated":result.truncated,"digest":result.content_digest,"process":result.status.as_deref().map(|s|match s{ToolRunStatus::Exited{code}=>json!({"kind":"exited","code":code}),ToolRunStatus::Running{handle}=>json!({"kind":"running","handle":handle}),ToolRunStatus::StopUnconfirmed{handle,detail}=>json!({"kind":"unconfirmed","handle":handle,"detail":detail})})});
            if let Some(details) = &result.details {
                value["details"] = json!(details);
            }
            value
        }
        ToolOutcome::Failed { error } => json!({"status":"failed","error":error}),
        ToolOutcome::Cancelled => json!({"status":"cancelled"}),
        ToolOutcome::Unknown { reason } => json!({"status":"unknown","reason":reason}),
    }
}
fn decode_outcome(v: &Value) -> Result<ToolOutcome, StorageError> {
    Ok(match str_field(v, "status")? {
        "success" => {
            let content: Vec<ContentBlock> = decode(&v["content"])?;
            let mut result = ToolSuccess::from_content(content);
            if str_field(v, "digest")? != result.content_digest {
                return Err(corrupt("projection content digest mismatch"));
            }
            result.details = match v.get("details") {
                None | Some(Value::Null) => None,
                Some(value) => Some(decode(value)?),
            };
            result.resource_refs = decode(&v["refs"])?;
            result.truncated = decode(&v["truncated"])?;
            let p = &v["process"];
            result.status = if p.is_null() {
                None
            } else {
                Some(Box::new(match str_field(p, "kind")? {
                    "exited" => ToolRunStatus::Exited {
                        code: decode(&p["code"])?,
                    },
                    "running" => ToolRunStatus::Running {
                        handle: str_field(p, "handle")?.into(),
                    },
                    "unconfirmed" => ToolRunStatus::StopUnconfirmed {
                        handle: str_field(p, "handle")?.into(),
                        detail: str_field(p, "detail")?.into(),
                    },
                    _ => return Err(corrupt("unknown process status")),
                }))
            };
            if result
                .details
                .as_ref()
                .and_then(|fields| fields.get("_modelProjection"))
                .and_then(|metadata| metadata.get("detailsTruncated"))
                .and_then(Value::as_bool)
                == Some(true)
                && !result.truncated
            {
                return Err(corrupt("structured details truncation flag mismatch"));
            }
            let outcome = ToolOutcome::Success { result };
            if matches!(&outcome, ToolOutcome::Success { result } if result.details.is_some())
                && crate::hooks::tool_outcome_model_bytes(&outcome)
                    > crate::hooks::MAX_TOOL_SUCCESS_PROJECTION_BYTES
            {
                return Err(corrupt("structured details exceed model projection budget"));
            }
            outcome
        }
        "failed" => ToolOutcome::Failed {
            error: decode(&v["error"])?,
        },
        "cancelled" => ToolOutcome::Cancelled,
        "unknown" => ToolOutcome::Unknown {
            reason: str_field(v, "reason")?.into(),
        },
        _ => return Err(corrupt("unknown tool outcome")),
    })
}
pub(crate) fn decode_item(v: &Value) -> Result<ExchangeItem, StorageError> {
    Ok(match str_field(v, "kind")? {
        "user_media" => ExchangeItem::UserMediaTurn {
            text: str_field(v, "text")?.into(),
            images: decode_images(&v["images"])?,
        },
        "host_notice" => ExchangeItem::HostNotice {
            delivery_id: str_field(v, "deliveryId")?.into(),
            text: str_field(v, "text")?.into(),
        },
        "user" => ExchangeItem::UserTurn {
            text: str_field(v, "text")?.into(),
        },
        "assistant" => {
            let calls = v["calls"]
                .as_array()
                .ok_or_else(|| corrupt("missing calls"))?
                .iter()
                .map(|c| {
                    let arguments = EffectiveArguments::from_value(
                        c["arguments"].clone(),
                        &SchemaBudget::default(),
                    )
                    .map_err(|e| corrupt(e.to_string()))?;
                    let digest: ArgsDigest = decode(&c["digest"])?;
                    // 原声明摘要保留；拒绝记录须在完整请求/结果及真实journal配对后核准。
                    // 编码侧对无目录绑定的调用（工具直发/快照缺目标）写 null；解码对称
                    // 还原为 None（消费方 historical_wire_name/投影核验均按 Option 容忍），
                    // 仅对存在的绑定保持严格校验。
                    let declaration_binding = if c["declarationBinding"].is_null() {
                        None
                    } else {
                        let binding = decode_declaration_binding(&c["declarationBinding"])?;
                        if binding.catalog_generation == 0
                            || binding.declaration.target != str_field(c, "target")?
                        {
                            return Err(corrupt("历史声明来源与调用目标不一致"));
                        }
                        Some(binding)
                    };
                    Ok(RequestedToolCall {
                        declaration_binding,
                        tool_call_id: ToolCallId::new(str_field(c, "id")?),
                        provider_call_id: decode(&c["providerId"])?,
                        target: str_field(c, "target")?.into(),
                        arguments,
                        args_digest: digest,
                        args_summary: decode(&c["summary"])?,
                    })
                })
                .collect::<Result<Vec<_>, StorageError>>()?;
            let origin = if v["origin"].is_null() {
                None
            } else {
                Some(TurnOrigin {
                    provider: str_field(&v["origin"], "provider")?.into(),
                    model: str_field(&v["origin"], "model")?.into(),
                })
            };
            ExchangeItem::AssistantTurn {
                call: ModelCallId::new(str_field(v, "call")?),
                content: decode(&v["content"])?,
                tool_calls: calls,
                origin,
            }
        }
        "tool" => ExchangeItem::ToolResult {
            tool_call_id: ToolCallId::new(str_field(v, "id")?),
            provider_call_id: decode(&v["providerId"])?,
            outcome: decode_outcome(&v["outcome"])?,
        },
        "summary" => ExchangeItem::CompactionSummary {
            summary: str_field(v, "summary")?.into(),
            covered_items: decode(&v["covered"])?,
            mid_run: decode(&v["midRun"])?,
        },
        _ => return Err(corrupt("unknown exchange kind")),
    })
}
fn is_argument_refusal(item: &ExchangeItem, request: &RequestedToolCall) -> bool {
    let batch_message = format!(
        "args_digest mismatch (declared {}, computed {}): forged or adulterated request; not dispatched",
        request.args_digest.hex,
        request.arguments.digest().hex
    );
    matches!(item,ExchangeItem::ToolResult{outcome:ToolOutcome::Failed{error},..}
        if error.code==lingxi_protocol::ErrorCode::InvalidMessage && !error.retryable
            && (error.message==batch_message || error.message=="tool request args_digest does not match its arguments (forged or adulterated request); not dispatched"))
}
fn argument_refusal_binding(
    entry: &lingxi_kernel::ports::InvocationJournalEntry,
) -> Result<Value, StorageError> {
    let receipt = entry
        .receipt
        .as_ref()
        .ok_or_else(|| corrupt("argument refusal journal not settled"))?;
    if entry.phase.wire_name() != "failed"
        || receipt.outcome != lingxi_kernel::ports::ReceiptOutcome::Failed
        || receipt.dispatched
        || !receipt
            .detail
            .starts_with("not dispatched: args_digest mismatch ")
    {
        return Err(corrupt(
            "argument refusal journal is not a never-dispatched failure",
        ));
    }
    Ok(
        json!({"run":entry.run_id,"session":entry.session_id,"owner":entry.owner_subject,"ownerKind":entry.owner_kind,
        "tool":entry.journal_id,"target":entry.target,"argsDigest":entry.args_digest,"phase":entry.phase.wire_name(),
        "outcome":"failed","dispatched":false,"detail":receipt.detail}),
    )
}
/// 完整档案的调用/结果配对验证。取消留下的待完成调用不被伪补成功。
fn pending_pairs(items: &[ExchangeItem]) -> Result<Vec<(String, Option<String>)>, StorageError> {
    let mut calls = HashMap::new();
    let mut seen_tools = HashSet::new();
    let mut seen_models = HashSet::new();
    for item in items {
        match item {
            ExchangeItem::AssistantTurn {
                call, tool_calls, ..
            } => {
                if !seen_models.insert(call.as_str()) {
                    return Err(corrupt("duplicate model call"));
                }
                for c in tool_calls {
                    if !seen_tools.insert(c.tool_call_id.as_str())
                        || calls
                            .insert(c.tool_call_id.as_str(), c.provider_call_id.as_deref())
                            .is_some()
                    {
                        return Err(corrupt("duplicate host tool identity"));
                    }
                }
            }
            ExchangeItem::ToolResult {
                tool_call_id,
                provider_call_id,
                ..
            } => {
                let id = calls
                    .remove(tool_call_id.as_str())
                    .ok_or_else(|| corrupt("orphan/duplicate tool result"))?;
                if id != provider_call_id.as_deref() {
                    return Err(corrupt("provider/host tool pairing mismatch"));
                }
            }
            _ => {}
        }
    }
    Ok(items
        .iter()
        .flat_map(|item| match item {
            ExchangeItem::AssistantTurn { tool_calls, .. } => tool_calls
                .iter()
                .filter(|c| calls.contains_key(c.tool_call_id.as_str()))
                .map(|c| {
                    (
                        c.tool_call_id.as_str().to_string(),
                        c.provider_call_id.clone(),
                    )
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect())
}
pub(crate) fn validate_pairs(items: &[ExchangeItem]) -> Result<(), StorageError> {
    if !pending_pairs(items)?.is_empty() {
        return Err(corrupt(
            "incomplete_tool_exchange: missing authoritative outcomes",
        ));
    }
    Ok(())
}

/// 从已提交F08投影恢复真实结构，绝不读raw.content作模型输入。
pub fn projected_tool_item(projection: &Value) -> Result<ExchangeItem, StorageError> {
    if str_field(projection, "schema")? != "lingxi.tool-result-model-projection.v1" {
        return Err(corrupt("projection schema"));
    }
    for key in [
        "rawContentDigest",
        "projectionContentDigest",
        "processStatus",
        "providerCallId",
        "result",
    ] {
        if projection.get(key).is_none() {
            return Err(corrupt(format!("missing projection {key}")));
        }
    }
    if projection["successBudgetBytes"].as_u64()
        != Some(crate::hooks::MAX_TOOL_SUCCESS_PROJECTION_BYTES as u64)
        || projection["errorBudgetPolicy"] != "untruncated"
        || projection["scope"] != "model_context_projection_not_raw_tool_archive"
    {
        return Err(corrupt("F08 projection policy metadata disagreement"));
    }
    str_field(projection, "rawEventId")?;
    str_field(projection, "target")?;
    let wire: ToolResultWire = decode(&projection["result"])?;
    if wire.status != ToolResultStatus::Success
        && (!projection["rawContentDigest"].is_null()
            || !projection["projectionContentDigest"].is_null()
            || !projection["processStatus"].is_null())
    {
        return Err(corrupt("non-success projection contains success metadata"));
    }
    let outcome = match wire.status {
        ToolResultStatus::Success => {
            str_field(projection, "rawContentDigest")?;
            let details = wire.details;
            let content = wire.content;
            let mut result = ToolSuccess::from_content(content);
            result.details = details;
            if str_field(projection, "projectionContentDigest")? != result.content_digest {
                return Err(corrupt("F08 projection digest"));
            }
            result.resource_refs = wire.resource_refs;
            result.truncated = wire.truncated;
            let p = &projection["processStatus"];
            result.status = if p.is_null() {
                None
            } else {
                Some(Box::new(match str_field(p, "kind")? {
                    "exited" => ToolRunStatus::Exited {
                        code: decode(&p["code"])?,
                    },
                    "running" => ToolRunStatus::Running {
                        handle: str_field(p, "handle")?.into(),
                    },
                    "stop_unconfirmed" => ToolRunStatus::StopUnconfirmed {
                        handle: str_field(p, "handle")?.into(),
                        detail: str_field(p, "detail")?.into(),
                    },
                    _ => return Err(corrupt("F08 process status")),
                }))
            };
            ToolOutcome::Success { result }
        }
        ToolResultStatus::Failed => ToolOutcome::Failed {
            error: wire.error.ok_or_else(|| corrupt("missing failed error"))?,
        },
        ToolResultStatus::Cancelled => ToolOutcome::Cancelled,
        ToolResultStatus::Unknown => ToolOutcome::Unknown {
            reason: wire
                .error
                .ok_or_else(|| corrupt("missing unknown reason"))?
                .message
                .strip_prefix("unknown tool outcome: ")
                .ok_or_else(|| corrupt("unknown projection error prefix disagreement"))?
                .to_string(),
        },
    };
    let rendered = lingxi_adapters::models::tool_render::render_tool_outcome_text(&outcome).len();
    let model_bytes = crate::hooks::tool_outcome_model_bytes(&outcome);
    let details_flag_mismatch = matches!(&outcome, ToolOutcome::Success { result } if
        result.details.as_ref().and_then(|fields|fields.get("_modelProjection")).and_then(|metadata|metadata.get("detailsTruncated")).and_then(Value::as_bool) == Some(true) && !result.truncated);
    if projection["renderedBytes"].as_u64() != Some(rendered as u64)
        || projection
            .get("modelProjectionBytes")
            .is_some_and(|bytes| bytes.as_u64() != Some(model_bytes as u64))
        || details_flag_mismatch
        || (matches!(outcome, ToolOutcome::Success { .. })
            && model_bytes > crate::hooks::MAX_TOOL_SUCCESS_PROJECTION_BYTES)
    {
        return Err(corrupt("F08 projection rendered budget mismatch"));
    }
    Ok(ExchangeItem::ToolResult {
        tool_call_id: ToolCallId::new(str_field(projection, "toolCallId")?),
        provider_call_id: decode(&projection["providerCallId"])?,
        outcome,
    })
}

/// 摘要版本与保留后缀在一个树节点原子保存，原树档案均保留。
pub fn archive_projection(items: &[ExchangeItem], submission: &str) -> Vec<ExchangeItem> {
    let mut out = Vec::new();
    if !items
        .iter()
        .any(|i| matches!(i, ExchangeItem::CurrentSubmission))
    {
        out.push(ExchangeItem::UserTurn {
            text: submission.to_string(),
        });
    }
    out.extend(items.iter().map(|i| match i {
        ExchangeItem::CurrentSubmission => ExchangeItem::UserTurn {
            text: submission.to_string(),
        },
        other => other.clone(),
    }));
    out
}

/// 图片事实只由服务适配层编解码，内核保持不依赖序列化框架。
pub(crate) fn encode_images(images: &[lingxi_kernel::model_exchange::InputImage]) -> Value {
    {
        use base64::Engine as _;
        json!(images.iter().map(|image|json!({"mime":image.mime,"data":base64::engine::general_purpose::STANDARD.encode(&image.bytes)})).collect::<Vec<_>>())
    }
}
pub(crate) fn decode_images(
    value: &Value,
) -> Result<Vec<lingxi_kernel::model_exchange::InputImage>, StorageError> {
    let rows = value
        .as_array()
        .ok_or_else(|| corrupt("historical images array"))?;
    if rows.is_empty() || rows.len() > 10 {
        return Err(corrupt("historical image count"));
    }
    rows.iter()
        .map(|row| {
            let mime = str_field(row, "mime")?.to_string();
            if !matches!(
                mime.as_str(),
                "image/png" | "image/jpeg" | "image/gif" | "image/webp"
            ) {
                return Err(corrupt("historical image mime"));
            }
            let bytes: Vec<u8> = if let Some(data) = row.get("data").and_then(Value::as_str) {
                use base64::Engine as _;
                if data.len() > 20 * 1024 * 1024 {
                    return Err(corrupt("historical image encoded size"));
                }
                base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(|_| corrupt("historical image encoding"))?
            } else {
                decode(&row["bytes"])?
            };
            if bytes.is_empty() || bytes.len() > 20 * 1024 * 1024 {
                return Err(corrupt("historical image bytes"));
            }
            Ok(lingxi_kernel::model_exchange::InputImage { bytes, mime })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_preserves_resource_process_unknown_and_errors() {
        let resource=serde_json::from_value(json!({"resourceId":"res_synthetic","kind":"session_file","displayName":"合成材料","digest":{"algorithm":"sha256","canonicalization":"test-only","hex":"abc"},"sizeBytes":"3"})).unwrap();
        let mut success = ToolSuccess::from_content(vec![ContentBlock::Text {
            text: "投影正文".into(),
        }]);
        success.resource_refs = vec![resource];
        success.truncated = true;
        for status in [
            ToolRunStatus::Exited { code: 7 },
            ToolRunStatus::Running {
                handle: "synthetic-process".into(),
            },
            ToolRunStatus::StopUnconfirmed {
                handle: "synthetic-process".into(),
                detail: "未确认停止".into(),
            },
        ] {
            success.status = Some(Box::new(status));
            let item = ExchangeItem::ToolResult {
                tool_call_id: ToolCallId::new("host-tool"),
                provider_call_id: Some("provider-tool".into()),
                outcome: ToolOutcome::Success {
                    result: success.clone(),
                },
            };
            let encoded = encode_item(&item).unwrap();
            assert_eq!(
                encode_item(&decode_item(&encoded).unwrap()).unwrap(),
                encoded
            );
        }
        for outcome in [
            ToolOutcome::Cancelled,
            ToolOutcome::Unknown {
                reason: "副作用未知，不补造成功".into(),
            },
            ToolOutcome::Failed {
                error: lingxi_protocol::ProtocolError::new(
                    lingxi_protocol::ErrorCode::UpstreamUnavailable,
                    "完整错误".repeat(20000),
                    false,
                ),
            },
        ] {
            let item = ExchangeItem::ToolResult {
                tool_call_id: ToolCallId::new("host-tool"),
                provider_call_id: None,
                outcome,
            };
            let encoded = encode_item(&item).unwrap();
            assert_eq!(
                encode_item(&decode_item(&encoded).unwrap()).unwrap(),
                encoded
            );
        }
    }
    #[test]
    fn result_without_authoritative_request_is_rejected() {
        let item = ExchangeItem::ToolResult {
            tool_call_id: ToolCallId::new("unknown-host"),
            provider_call_id: Some("unknown-provider".into()),
            outcome: ToolOutcome::Unknown {
                reason: "未确认副作用".into(),
            },
        };
        assert!(validate_pairs(&[item])
            .unwrap_err()
            .to_string()
            .contains("orphan"));
    }
    #[test]
    fn current_submission_is_archived_exactly_once_at_its_position() {
        let items = vec![
            ExchangeItem::UserTurn {
                text: "旧输入".into(),
            },
            ExchangeItem::CurrentSubmission,
        ];
        let archived = archive_projection(&items, "当前输入");
        assert_eq!(
            archived
                .iter()
                .filter(|x| matches!(x,ExchangeItem::UserTurn{text} if text=="当前输入"))
                .count(),
            1
        );
        assert!(encode_item(&ExchangeItem::CurrentSubmission).is_err());
    }
    #[test]
    fn first_layer_f07_historical_declaration_and_dispatch_codec_preserves_both_identities() {
        use lingxi_kernel::model_exchange::{
            HistoricalToolDeclaration, HistoricalToolExecution, ToolDeclaration,
        };
        let arguments = EffectiveArguments::from_value(
            json!({"tool":"future_tool","arguments":{"path":"synthetic"}}),
            &SchemaBudget::default(),
        )
        .unwrap();
        let effective =
            EffectiveArguments::from_value(json!({"path":"synthetic"}), &SchemaBudget::default())
                .unwrap();
        let item = ExchangeItem::AssistantTurn {
            call: ModelCallId::new("run-mc1"),
            content: vec![],
            origin: None,
            tool_calls: vec![RequestedToolCall {
                tool_call_id: ToolCallId::new("run-tc1"),
                provider_call_id: Some("provider-call".into()),
                target: "tool:first-party:mcp_call".into(),
                args_digest: arguments.digest(),
                arguments,
                args_summary: None,
                declaration_binding: Some(HistoricalToolDeclaration {
                    catalog_generation: 8,
                    declaration: ToolDeclaration {
                        target: "tool:first-party:mcp_call".into(),
                        wire_name: "mcp_call".into(),
                        description: "当时真实说明".into(),
                        input_schema: lingxi_protocol::ToolSchemaDocument {
                            dialect: "json-schema/2020-12".into(),
                            schema: json!({"type":"object"}),
                        },
                    },
                    execution: Some(HistoricalToolExecution {
                        target: "tool:first-party:future_tool".into(),
                        catalog_generation: 8,
                        args_digest: effective.digest(),
                        arguments: effective.as_value().clone(),
                    }),
                }),
            }],
        };
        let wire = encode_item(&item).unwrap();
        assert_eq!(decode_item(&wire).unwrap(), item);
        assert_eq!(wire["calls"][0]["target"], "tool:first-party:mcp_call");
        assert_eq!(
            wire["calls"][0]["declarationBinding"]["execution"]["target"],
            "tool:first-party:future_tool"
        );
    }
    #[test]
    fn adversarial_f07_missing_or_forged_historical_binding_is_loudly_rejected() {
        // 调和契约：无目录定义的 run（无模型/无网关的宿主直发）合法归档无绑定，
        // 解码对称还原为 None（消费方按 Option 容忍，wire 名缺席）；伪造绑定
        // （存在但目标/代次不匹配）仍响亮拒绝。
        let base = json!({"kind":"assistant","call":"run-mc1","content":[],"origin":null,"calls":[{"id":"run-tc1","providerId":"p1","target":"tool:first-party:read","arguments":{"path":"synthetic"},"digest":EffectiveArguments::from_value(json!({"path":"synthetic"}),&SchemaBudget::default()).unwrap().digest(),"summary":null}]});
        let decoded = decode_item(&base).expect("missing binding decodes as unbound");
        match decoded {
            ExchangeItem::AssistantTurn { tool_calls, .. } => {
                assert!(tool_calls[0].declaration_binding.is_none());
            }
            _ => panic!("assistant turn"),
        }
        let mut forged = base.clone();
        forged["calls"][0]["declarationBinding"] = json!({"catalogGeneration":8,"declaration":{"target":"tool:first-party:other","wireName":"read","description":"伪造目标","inputSchema":{"dialect":"json-schema/2020-12","schema":{"type":"object"}}},"execution":null});
        assert!(decode_item(&forged).is_err());
    }
}

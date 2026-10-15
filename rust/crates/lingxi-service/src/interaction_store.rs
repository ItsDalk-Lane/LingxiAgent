//! 批量问答与会话内容卡的事实存储；答案只能由所属会话确认或真实超时产生。
use crate::inject::ServiceClock;
use lingxi_adapters::storage::{session_tree as store, RunDatabase};
use lingxi_kernel::{ports::StoragePort, RunContext};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AskOption {
    pub value: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Question {
    pub key: String,
    pub question: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub options: Vec<AskOption>,
    pub recommended: Vec<String>,
    pub required: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingQuestion {
    pub id: String,
    pub session_id: String,
    pub run_id: String,
    pub questions: Vec<Question>,
    pub deadline_unix_ms: u64,
}
struct Pending {
    ready: bool,
    view: PendingQuestion,
    sender: tokio::sync::oneshot::Sender<Value>,
}
pub struct InteractionStore {
    pub storage: Arc<RunDatabase>,
    pub events: Arc<crate::events::EventService>,
    pub clock: Arc<dyn ServiceClock>,
    pending: Mutex<HashMap<String, Pending>>,
    available: AtomicBool,
}
fn trim(value: &Value, key: &str, max: usize) -> Result<String, String> {
    let s = value.get(key).and_then(Value::as_str).unwrap_or("").trim();
    if s.is_empty() || s.chars().count() > max {
        return Err(format!("{key} is empty or oversized"));
    }
    Ok(s.into())
}
pub fn normalize_questions(value: &Value) -> Result<Vec<Question>, String> {
    let raw = value
        .as_array()
        .filter(|x| !x.is_empty() && x.len() <= 8)
        .ok_or("questions must contain 1..8 items")?;
    let mut out = Vec::new();
    let mut keys = std::collections::HashSet::new();
    for (i, q) in raw.iter().enumerate() {
        let question = trim(q, "question", 2000)?;
        let key = q["key"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("q{}", i + 1));
        if key.len() > 32
            || !key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            || !keys.insert(key.clone())
        {
            return Err("invalid or duplicate question key".into());
        }
        let kind = q["type"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(if q["options"].is_array() {
                "single"
            } else {
                "text"
            })
            .to_string();
        if !matches!(kind.as_str(), "text" | "single" | "multi") {
            return Err("unknown question type".into());
        }
        let mut options = Vec::new();
        let mut recommended = Vec::new();
        if kind == "text" {
            if q["options"].is_array() || q.get("recommended").is_some() {
                return Err("text question cannot carry options or recommended".into());
            }
        } else {
            let list = q["options"]
                .as_array()
                .filter(|x| (2..=12).contains(&x.len()))
                .ok_or("choice needs 2..12 options")?;
            let mut values = std::collections::HashSet::new();
            let mut labels = std::collections::HashSet::new();
            for o in list {
                let label = trim(o, "label", 200)?;
                let value = o["value"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&label)
                    .to_string();
                if value.chars().count() > 200
                    || !values.insert(value.clone())
                    || !labels.insert(label.clone())
                {
                    return Err("duplicate or oversized option value/label".into());
                }
                let description = o["description"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.chars().take(500).collect());
                options.push(AskOption {
                    value,
                    label,
                    description,
                });
            }
            let rec = match q.get("recommended") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(xs)) => xs.clone(),
                Some(x) => vec![x.clone()],
            };
            for item in rec {
                let item = item.as_str().ok_or("recommended must be an option value")?;
                if !values.contains(item) {
                    return Err("recommended option is unknown".into());
                }
                if !recommended.iter().any(|v| v == item) {
                    recommended.push(item.to_string());
                }
            }
            if kind == "single" && recommended.len() > 1 {
                return Err("single question has multiple recommendations".into());
            }
        }
        out.push(Question {
            key,
            question,
            kind,
            options,
            recommended,
            required: q["required"] != false,
        });
    }
    Ok(out)
}
pub fn mapped_result(questions: &[Question], action: &str, values: &Value) -> Value {
    if !matches!(action, "confirmed" | "timeout") {
        return if action == "aborted" {
            json!({"answered":false,"aborted":true,"answers":[]})
        } else {
            json!({"answered":false,"dismissed":true,"answers":[]})
        };
    }
    let answers:Vec<Value>=questions.iter().map(|q|{
        let mut picked=Vec::<String>::new();
        if action=="timeout" {picked=q.recommended.clone();}
        else if q.kind=="text"{if let Some(s)=values[&q.key].as_str(){let s:String=s.trim().chars().take(4000).collect();if !s.is_empty(){picked.push(s);}}}
        else {let raw=if q.kind=="multi"{values[&q.key].as_array().cloned().unwrap_or_default()}else{vec![values[&q.key].clone()]};for v in raw{if let Some(s)=v.as_str(){if q.options.iter().any(|o|o.value==s)&&!picked.iter().any(|v|v==s){picked.push(s.to_string());if q.kind=="single"{break;}}}}}
        let labels:Vec<String>=picked.iter().map(|v|q.options.iter().find(|o|o.value==*v).map(|o|o.label.clone()).unwrap_or_else(||v.clone())).collect();
        json!({"key":q.key,"question":q.question,"unanswered":picked.is_empty(),"autoSelected":action=="timeout"&&!picked.is_empty(),"values":picked,"labels":labels})
    }).collect();
    json!({"answered":true,"timedOut":action=="timeout","answers":answers})
}
impl InteractionStore {
    pub fn new(
        storage: Arc<RunDatabase>,
        events: Arc<crate::events::EventService>,
        clock: Arc<dyn ServiceClock>,
    ) -> Self {
        Self {
            storage,
            events,
            clock,
            pending: Mutex::new(HashMap::new()),
            available: AtomicBool::new(true),
        }
    }
    /// 无确认服务时明确拒绝；测试可验证相同故障路径。
    pub fn set_available(&self, value: bool) {
        self.available.store(value, Ordering::Release);
    }
    pub async fn validate(&self, ctx: &RunContext) -> Result<(), String> {
        let row = store::get_session_row(&self.storage, ctx.session_id.as_str())
            .await
            .map_err(|e| e.to_string())?
            .ok_or("no active session")?;
        if row.archived_at_unix_ms.is_some()
            || (ctx.principal.storage_kind() != "local_user"
                && row.owner_user_id != ctx.principal.storage_subject())
        {
            return Err("session is unavailable or belongs to another principal".into());
        }
        let run = self
            .storage
            .load_run(&ctx.run_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("no active run")?;
        if run.session_id != ctx.session_id || run.status.is_terminal() {
            return Err("run context is stale".into());
        }
        Ok(())
    }
    pub async fn commit(
        &self,
        ctx: &RunContext,
        entry_type: &str,
        value: &Value,
        event_type: &str,
    ) -> Result<(), String> {
        let content = serde_json::to_string(value).map_err(|e| e.to_string())?;
        let mut payload = value
            .as_object()
            .cloned()
            .ok_or("event must be an object")?;
        payload.insert("type".into(), json!(event_type));
        payload.insert("sessionId".into(), json!(ctx.session_id));
        payload.insert("runId".into(), json!(ctx.run_id));
        let event = lingxi_protocol::EventPayload::Unknown(lingxi_protocol::UnknownEventPayload {
            event_type: event_type.into(),
            raw: payload,
        });
        let (_, event) = store::append_tool_branch_entry(
            &self.storage,
            ctx,
            store::AppendBranchEntry {
                session_id: ctx.session_id.as_str(),
                message_id: None,
                entry_type,
                role: "custom",
                content_json: &content,
                now_unix_ms: self.clock.now_unix_ms(),
            },
            event,
        )
        .await
        .map_err(|e| e.to_string())?;
        if let Some(event) = event {
            self.events.publish_committed(&[event]);
        }
        Ok(())
    }
    pub fn list_pending(&self, sid: &str) -> Vec<PendingQuestion> {
        let mut list: Vec<_> = self
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .filter(|p| p.ready && p.view.session_id == sid)
            .map(|p| p.view.clone())
            .collect();
        list.sort_by(|a, b| a.id.cmp(&b.id));
        list
    }
    pub fn answer(&self, sid: &str, id: &str, action: &str, values: &Value) -> Result<(), String> {
        if !matches!(action, "confirmed" | "dismissed") {
            return Err("answer action must be confirmed or dismissed".into());
        }
        let mut map = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        let p = map
            .get(id)
            .filter(|p| p.ready && p.view.session_id == sid)
            .ok_or("question is missing, settled or belongs to another session")?;
        let timed_out = self.clock.now_unix_ms() >= p.view.deadline_unix_ms;
        let result = mapped_result(
            &p.view.questions,
            if timed_out { "timeout" } else { action },
            values,
        );
        let p = map.remove(id).unwrap();
        p.sender
            .send(result)
            .map_err(|_| "question was aborted".into())
    }
    pub fn abort(&self, id: &str) {
        if let Some(p) = self
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(id)
        {
            let _ = p
                .sender
                .send(mapped_result(&p.view.questions, "aborted", &Value::Null));
        }
    }
    pub async fn ask(self: &Arc<Self>, ctx: &RunContext, args: &Value) -> Result<Value, String> {
        let questions = normalize_questions(&args["questions"])?;
        if !self.available.load(Ordering::Acquire) {
            return Err("question confirmation store is unavailable".into());
        }
        self.validate(ctx).await?;
        let seconds = args["timeout_seconds"]
            .as_f64()
            .filter(|x| x.is_finite() && *x > 0.0)
            .map(|x| x.round().clamp(30.0, 1800.0) as u64)
            .unwrap_or(300);
        let id = crate::auth::hex_random_public(16).map_err(|e| e.to_string())?;
        let view = PendingQuestion {
            id: id.clone(),
            session_id: ctx.session_id.to_string(),
            run_id: ctx.run_id.to_string(),
            questions,
            deadline_unix_ms: self.clock.now_unix_ms().saturating_add(seconds * 1000),
        };
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        {
            let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
            if pending.len() >= 1024 {
                return Err("too many pending questions".into());
            }
            pending.insert(
                id.clone(),
                Pending {
                    ready: false,
                    view: view.clone(),
                    sender: tx,
                },
            );
        }
        struct AbortOnDrop {
            store: Arc<InteractionStore>,
            id: String,
        }
        impl Drop for AbortOnDrop {
            fn drop(&mut self) {
                self.store.abort(&self.id);
            }
        }
        let _guard = AbortOnDrop {
            store: self.clone(),
            id: id.clone(),
        };
        self.commit(
            ctx,
            "interactive-question-v1",
            &json!({"request":view}),
            "session_confirmation",
        )
        .await?;
        if let Some(p) = self
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_mut(&id)
        {
            p.ready = true;
        }
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            tokio::select! {
                value=&mut rx=>return value.map_err(|_|"question confirmation channel unavailable".into()),
                _=tick.tick()=>{
                    if self.clock.now_unix_ms()>=view.deadline_unix_ms{
                        let mut pending=self.pending.lock().unwrap_or_else(|p|p.into_inner());
                        if let Some(p)=pending.remove(&id){let _=p.sender.send(mapped_result(&view.questions,"timeout",&Value::Null));}
                    }
                    if !self.available.load(Ordering::Acquire){return Err("question confirmation store became unavailable".into());}
                }
            }
        }
    }
    pub async fn cards(&self, sid: &str) -> Result<Vec<Value>, String> {
        let rows = lingxi_adapters::storage::canonical_message_store::read_branch_entries_by_type(
            &self.storage,
            sid,
            "interactive-card-v1",
        )
        .await
        .map_err(|e| e.to_string())?;
        let mut values = rows
            .into_iter()
            .map(|r| serde_json::from_str(&r.content_json).map_err(|e| e.to_string()))
            .collect::<Result<Vec<Value>, _>>()?;
        values.reverse();
        Ok(values)
    }
}

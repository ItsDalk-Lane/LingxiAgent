//! 正式浏览器服务：真实会话、授权代次与命令回执共同约束受控 Chromium。
use crate::{Principal, ServiceClock, ServiceState};
use lingxi_adapters::storage::{browser as store, session_tree, RunDatabase};
use lingxi_kernel::{
    browser::{BrowserError, BrowserFuture, BrowserOutput, BrowserPort, BrowserScope},
    ports::StoragePort,
    subagent::RunOrigin,
    RunContext,
};
use lingxi_protocol::{BrowserAction, BrowserCommand, BrowserSettings, BrowserWorkspace};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
};
fn failure(code: &str, message: impl ToString) -> BrowserError {
    BrowserError {
        code: code.into(),
        message: message.to_string(),
        effect_unknown: false,
    }
}
fn storage(error: lingxi_kernel::ports::StorageError) -> BrowserError {
    failure("BROWSER_STORAGE", error)
}
fn uncertain_receipt(error: lingxi_kernel::ports::StorageError) -> BrowserError {
    BrowserError {
        code: "BROWSER_RECEIPT_UNKNOWN".into(),
        message: format!("浏览器动作已受理，但结果保存未确认：{error}"),
        effect_unknown: true,
    }
}
pub struct UnavailableBrowser {
    pub reason: String,
}
impl BrowserPort for UnavailableBrowser {
    fn execute<'a>(
        &'a self,
        _: BrowserScope,
        _: BrowserCommand,
        _: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, BrowserOutput> {
        Box::pin(async { Err(failure("BROWSER_UNAVAILABLE", &self.reason)) })
    }
    fn workspaces(&self) -> Result<Vec<BrowserWorkspace>, BrowserError> {
        Ok(vec![])
    }
    fn restore(&self, _: BrowserScope, _: BrowserWorkspace) -> Result<(), BrowserError> {
        Ok(())
    }
    fn revoke<'a>(&'a self, _: BrowserScope) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn configure<'a>(&'a self, _: BrowserSettings) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn clear_site_data<'a>(&'a self) -> BrowserFuture<'a, ()> {
        Box::pin(async { Err(failure("BROWSER_UNAVAILABLE", &self.reason)) })
    }
    fn read_auxiliary_page<'a>(
        &'a self,
        _: String,
        _: String,
        _: Option<String>,
        _: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, Value> {
        Box::pin(async { Err(failure("BROWSER_UNAVAILABLE", &self.reason)) })
    }
    fn reap_idle<'a>(&'a self, _: u64) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn shutdown<'a>(&'a self) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}
type ActiveCommands = Arc<Mutex<BTreeMap<String, (String, Arc<AtomicBool>)>>>;
struct CommandGuard {
    key: String,
    active: ActiveCommands,
    cancel: Arc<AtomicBool>,
}
impl Drop for CommandGuard {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.active.lock().expect("浏览器操作锁").remove(&self.key);
    }
}
pub enum HumanSteerClaim {
    New {
        row: Box<store::BrowserAuthority>,
        operation: String,
    },
    Replayed,
}
pub struct BrowserService {
    pub(crate) db: Arc<RunDatabase>,
    port: Arc<dyn BrowserPort>,
    node: String,
    memory: Arc<crate::memory::MemoryService>,
    files: Arc<crate::sessionfiles::SessionFileService>,
    events: Arc<crate::appevents::AppEventLog>,
    clock: Arc<dyn ServiceClock>,
    runs: Weak<crate::runs::RunSupervisor>,
    active: ActiveCommands,
    unavailable: Option<String>,
    settings_lock: tokio::sync::Mutex<()>,
    state_lock: tokio::sync::Mutex<()>,
    authority_lock: tokio::sync::Mutex<()>,
    known: Mutex<BTreeMap<String, BrowserScope>>,
    stop: tokio::sync::watch::Sender<bool>,
    worker: Mutex<Option<tokio::task::JoinHandle<Result<(), String>>>>,
    worker_result: Mutex<Option<Result<(), String>>>,
    join_lock: tokio::sync::Mutex<()>,
}
impl BrowserService {
    pub async fn new(
        state: &ServiceState,
        port: Arc<dyn BrowserPort>,
        unavailable: Option<String>,
    ) -> Result<Arc<Self>, String> {
        let this = Arc::new(Self {
            db: state.storage().clone(),
            port,
            node: state.management.identity_ids().0,
            memory: state.memory().clone(),
            files: state.session_files().clone(),
            events: state.app_events().clone(),
            clock: state.clock().clone(),
            runs: Arc::downgrade(state.runs()),
            active: Arc::new(Mutex::new(BTreeMap::new())),
            unavailable,
            settings_lock: tokio::sync::Mutex::new(()),
            state_lock: tokio::sync::Mutex::new(()),
            authority_lock: tokio::sync::Mutex::new(()),
            known: Mutex::new(BTreeMap::new()),
            stop: tokio::sync::watch::channel(false).0,
            worker: Mutex::new(None),
            worker_result: Mutex::new(None),
            join_lock: tokio::sync::Mutex::new(()),
        });
        store::recover_commands(&this.db, this.clock.now_unix_ms())
            .await
            .map_err(|e| e.to_string())?;
        this.port
            .configure(this.settings()?)
            .await
            .map_err(|e| e.to_string())?;
        for row in store::list(&this.db).await.map_err(|e| e.to_string())? {
            if row.execution_node != this.node {
                continue;
            }
            this.known
                .lock()
                .expect("浏览器边界锁")
                .insert(row.session_id.clone(), this.scope(&row, false));
            if session_tree::get_session_row(&this.db, &row.session_id)
                .await
                .map_err(|e| e.to_string())?
                .is_none_or(|s| s.lifecycle != "active" || s.owner_user_id != row.owner_subject)
            {
                continue;
            }
            let Some(mut cold) = row.state.clone() else {
                continue;
            };
            cold.running = false;
            cold.user_takeover = false;
            cold.revoked = row.revoked;
            cold.resumable = !cold.tabs.is_empty();
            this.port
                .restore(this.scope(&row, false), cold)
                .map_err(|e| e.to_string())?;
        }
        state.runs().attach_browser(&this);
        Ok(this)
    }
    pub fn review_epoch(&self, sid: &str) -> u64 {
        self.known
            .lock()
            .expect("浏览器边界锁")
            .get(sid)
            .map_or(0, |scope| scope.epoch)
    }
    pub async fn refresh_authority(&self, sid: &str, cold: bool) -> Result<(), BrowserError> {
        let _authority = self.authority_lock.lock().await;
        self.refresh_authority_locked(sid, cold).await
    }
    pub async fn release_for_user_run(
        &self,
        sid: &str,
        run: &str,
        now: u64,
    ) -> Result<(), BrowserError> {
        let _authority = self.authority_lock.lock().await;
        let changed = store::release_for_user_run(&self.db, sid, run, now)
            .await
            .map_err(storage)?;
        if changed {
            self.refresh_authority_locked(sid, true).await?;
        }
        Ok(())
    }
    async fn refresh_authority_locked(&self, sid: &str, cold: bool) -> Result<(), BrowserError> {
        let session = session_tree::get_session_row(&self.db, sid)
            .await
            .map_err(storage)?
            .ok_or_else(|| failure("BROWSER_NOT_FOUND", "session missing"))?;
        let row = store::ensure(
            &self.db,
            sid,
            &session.owner_user_id,
            &self.node,
            self.clock.now_unix_ms(),
        )
        .await
        .map_err(storage)?;
        self.known
            .lock()
            .expect("浏览器边界锁")
            .insert(sid.into(), self.scope(&row, false));
        if cold {
            if let Some(mut state) = row.state.clone() {
                state.running = false;
                state.user_takeover = false;
                state.revoked = row.revoked;
                state.resumable = !state.tabs.is_empty();
                self.port.restore(self.scope(&row, false), state)?;
            }
        }
        Ok(())
    }
    pub async fn replay_human_steer(
        &self,
        p: &Principal,
        sid: &str,
        text: &str,
        request: Option<&str>,
    ) -> Result<bool, BrowserError> {
        let Some(id) = request else { return Ok(false) };
        if id.is_empty() || id.len() > 1024 || id.chars().any(char::is_control) {
            return Err(failure("BROWSER_INPUT", "invalid steering requestId"));
        }
        let operation = format!(
            "human-steer-{}",
            lingxi_protocol::canon::sha256_hex(json!([p.user_id, sid, id]).to_string().as_bytes())
        );
        let prior=self.db.query_one_text("SELECT json_object('input',json(input_json),'status',status) FROM browser_commands_v1 WHERE operation_id=?1 AND session_id=?2",vec![operation,sid.into()]).await.map_err(storage)?;
        let Some(prior) = prior else { return Ok(false) };
        let prior: Value = serde_json::from_str(&prior)
            .map_err(|_| failure("BROWSER_STORAGE", "steering receipt damaged"))?;
        if prior["input"]["textDigest"] != lingxi_protocol::canon::sha256_hex(text.as_bytes()) {
            return Err(failure(
                "BROWSER_INPUT",
                "steering requestId belongs to different input",
            ));
        }
        match prior["status"].as_str() {
            Some("confirmed") => Ok(true),
            Some("failed") => Err(failure(
                "BROWSER_STEER_FAILED",
                "previous steering request did not complete",
            )),
            _ => Err(BrowserError {
                code: "BROWSER_STEER_UNKNOWN".into(),
                message: "previous steering acceptance is unconfirmed; request was not repeated"
                    .into(),
                effect_unknown: true,
            }),
        }
    }
    pub async fn begin_human_steer(
        &self,
        p: &Principal,
        sid: &str,
        run: &str,
        text: &str,
        request: Option<&str>,
    ) -> Result<HumanSteerClaim, BrowserError> {
        let _authority = self.authority_lock.lock().await;
        let row = self.authorize(p, sid).await?;
        let actual=self.db.query_one_text("SELECT r.run_id FROM runs r JOIN run_lineage l ON l.run_id=r.run_id WHERE r.run_id=?1 AND r.session_id=?2 AND r.status IN ('running','waiting_approval') AND l.origin='user' AND l.parent_run_id IS NULL",vec![run.into(),sid.into()]).await.map_err(storage)?;
        if actual.is_none() {
            return Err(failure(
                "BROWSER_FORBIDDEN",
                "human steering must name the actual running user root",
            ));
        }
        let text_digest = lingxi_protocol::canon::sha256_hex(text.as_bytes());
        let id = request
            .map(String::from)
            .unwrap_or_else(|| format!("{run}:{text_digest}"));
        if id.is_empty() || id.len() > 1024 || id.chars().any(char::is_control) {
            return Err(failure("BROWSER_INPUT", "invalid steering requestId"));
        }
        let operation = format!(
            "human-steer-{}",
            lingxi_protocol::canon::sha256_hex(json!([p.user_id, sid, id]).to_string().as_bytes())
        );
        let input = json!({"action":"human_steer","runId":run,"textDigest":text_digest});
        match store::claim_command(
            &self.db,
            &self.scope(&row, true),
            &operation,
            &input,
            true,
            self.clock.now_unix_ms(),
        )
        .await
        .map_err(storage)?
        {
            store::CommandClaim::Started => Ok(HumanSteerClaim::New {
                row: Box::new(row),
                operation,
            }),
            store::CommandClaim::Confirmed(_) => Ok(HumanSteerClaim::Replayed),
            store::CommandClaim::Failed(message) => Err(failure("BROWSER_STEER_FAILED", message)),
            store::CommandClaim::Unknown(message) => Err(BrowserError {
                code: "BROWSER_STEER_UNKNOWN".into(),
                message,
                effect_unknown: true,
            }),
        }
    }
    pub async fn finish_human_steer(
        &self,
        row: store::BrowserAuthority,
        run: &str,
        operation: &str,
        accepted: bool,
    ) -> Result<(), BrowserError> {
        if !accepted {
            return store::finish_command(
                &self.db,
                operation,
                None,
                Some("steering was not accepted".into()),
                false,
                self.clock.now_unix_ms(),
            )
            .await
            .map_err(storage);
        }
        let _authority = self.authority_lock.lock().await;
        let result = store::accept_human_steer(
            &self.db,
            &self.scope(&row, true),
            run,
            operation,
            self.clock.now_unix_ms(),
        )
        .await;
        match result {
            Ok(current) => {
                self.refresh_authority_locked(
                    &row.session_id,
                    current.epoch != row.epoch && !current.revoked,
                )
                .await
            }
            Err(error) => {
                store::finish_command(
                    &self.db,
                    operation,
                    None,
                    Some(error.to_string()),
                    true,
                    self.clock.now_unix_ms(),
                )
                .await
                .map_err(storage)?;
                Err(BrowserError {
                    code: "BROWSER_STEER_UNKNOWN".into(),
                    message: error.to_string(),
                    effect_unknown: true,
                })
            }
        }
    }
    pub fn port(&self) -> Arc<dyn BrowserPort> {
        self.port.clone()
    }
    pub fn unavailable_reason(&self) -> Option<&str> {
        self.unavailable.as_deref()
    }
    fn scope(&self, row: &store::BrowserAuthority, human: bool) -> BrowserScope {
        BrowserScope {
            session_id: row.session_id.clone(),
            owner_subject: row.owner_subject.clone(),
            epoch: row.epoch,
            execution_node: self.node.clone(),
            human,
        }
    }
    pub fn settings(&self) -> Result<BrowserSettings, String> {
        let preferences = self.memory.read_preferences_root()?;
        let b = &preferences["browser"];
        if b.is_null() {
            return Ok(BrowserSettings::default());
        }
        if !b.is_object() {
            return Err("invalid browser preferences".into());
        }
        let mut selected = json!({"acceptCookies":true,"agentOpenBehavior":"smart"});
        for key in ["acceptCookies", "agentOpenBehavior"] {
            if let Some(v) = b.get(key) {
                selected[key] = v.clone();
            }
        }
        serde_json::from_value(selected).map_err(|e| format!("invalid browser preferences: {e}"))
    }
    pub async fn configure(&self, patch: &Value) -> Result<BrowserSettings, BrowserError> {
        let _serial = self.settings_lock.lock().await;
        let patch = patch.get("browser").unwrap_or(patch);
        let map = patch
            .as_object()
            .ok_or_else(|| failure("BROWSER_SETTINGS", "browser patch must be object"))?;
        if map
            .keys()
            .any(|key| !matches!(key.as_str(), "acceptCookies" | "agentOpenBehavior"))
        {
            return Err(failure("BROWSER_SETTINGS", "unknown browser setting"));
        }
        let old = self
            .settings()
            .map_err(|e| failure("BROWSER_SETTINGS", e))?;
        let mut selected = serde_json::to_value(&old).expect("浏览器设置");
        for (k, v) in map {
            selected[k] = v.clone();
        }
        let next: BrowserSettings =
            serde_json::from_value(selected).map_err(|e| failure("BROWSER_SETTINGS", e))?;
        if let Err(error) = self.port.configure(next.clone()).await {
            if let Err(rollback) = self.port.configure(old.clone()).await {
                return Err(BrowserError {
                    code: "BROWSER_SETTINGS_UNKNOWN".into(),
                    message: format!(
                        "浏览器设置失败，恢复原设置的结果也未确认：{error}; {rollback}"
                    ),
                    effect_unknown: true,
                });
            }
            return Err(error);
        }
        let persisted = (|| -> Result<(), BrowserError> {
            let _guard = crate::agents_admin::preferences_locked();
            let mut prefs = self
                .memory
                .read_preferences_root()
                .map_err(|e| failure("BROWSER_SETTINGS", e))?;
            if prefs["browser"].is_null() {
                prefs["browser"] = json!({});
            }
            let b = prefs["browser"]
                .as_object_mut()
                .ok_or_else(|| failure("BROWSER_SETTINGS", "browser preferences damaged"))?;
            for (k, v) in map {
                b.insert(k.clone(), v.clone());
            }
            self.memory
                .write_preferences_root(&prefs)
                .map_err(|e| failure("BROWSER_SETTINGS", e))?;
            Ok(())
        })();
        if let Err(error) = persisted {
            if let Err(rollback) = self.port.configure(old).await {
                return Err(BrowserError {
                    code: "BROWSER_SETTINGS_UNKNOWN".into(),
                    message: format!("{error}; rollback unconfirmed: {rollback}"),
                    effect_unknown: true,
                });
            }
            return Err(error);
        }
        self.sync_workspaces().await.map_err(|mut error| {
            error.effect_unknown = true;
            error.message = format!("设置已保存，页面状态同步未确认：{}", error.message);
            error
        })?;
        Ok(next)
    }
    pub async fn clear_site_data(&self) -> Result<(), BrowserError> {
        let _serial = self.settings_lock.lock().await;
        self.port.clear_site_data().await?;
        self.sync_workspaces().await.map_err(|mut error| {
            error.effect_unknown = true;
            error.message = format!("站点资料已清理，页面状态同步未确认：{}", error.message);
            error
        })
    }
    pub async fn authorize(
        &self,
        principal: &Principal,
        sid: &str,
    ) -> Result<store::BrowserAuthority, BrowserError> {
        if matches!(
            principal.kind,
            crate::auth::PrincipalKind::Bridge | crate::auth::PrincipalKind::BridgeTakeover
        ) {
            return Err(failure(
                "BROWSER_FORBIDDEN",
                "Bridge cannot operate a browser",
            ));
        }
        let session = session_tree::get_session_row(&self.db, sid)
            .await
            .map_err(storage)?
            .ok_or_else(|| failure("BROWSER_NOT_FOUND", "session missing"))?;
        if session.lifecycle != "active"
            || !crate::sessions::SessionStore::can_access_session(
                principal,
                &session.owner_user_id,
                sid,
            )
        {
            return Err(failure(
                "BROWSER_FORBIDDEN",
                "session unavailable or not owned",
            ));
        }
        let row = store::ensure(
            &self.db,
            sid,
            &session.owner_user_id,
            &self.node,
            self.clock.now_unix_ms(),
        )
        .await
        .map_err(storage)?;
        self.known
            .lock()
            .expect("浏览器边界锁")
            .insert(sid.into(), self.scope(&row, false));
        Ok(row)
    }
    pub async fn tool_scope(
        &self,
        ctx: &RunContext,
        call: &lingxi_protocol::ToolCallId,
        request: &lingxi_kernel::ports::ToolRequest,
    ) -> Result<store::BrowserAuthority, BrowserError> {
        if request.target != "tool:first-party:browser"
            || request.arguments.digest() != request.args_digest
        {
            return Err(failure(
                "BROWSER_FORBIDDEN",
                "browser invocation target or arguments changed",
            ));
        }
        let actual=self.db.query_one_text("SELECT r.run_id FROM runs r JOIN sessions s ON s.session_id=r.session_id JOIN run_attempts a ON a.run_id=r.run_id JOIN invocation_journal j ON j.run_id=r.run_id AND j.journal_id=?7 WHERE r.run_id=?1 AND r.session_id=?2 AND r.owner_kind=?3 AND r.owner_subject=?4 AND r.generation=?5 AND a.attempt=?6 AND a.generation=r.generation AND r.status='running' AND s.lifecycle='active' AND j.phase='started' AND j.session_id=r.session_id AND j.owner_kind=r.owner_kind AND j.owner_subject=r.owner_subject AND j.target=?8 AND j.args_digest=?9 AND j.generation=r.generation AND j.attempt=a.attempt AND a.attempt=(SELECT attempt FROM run_attempts WHERE run_id=r.run_id ORDER BY started_at_unix_ms DESC,rowid DESC LIMIT 1)",vec![ctx.run_id.to_string(),ctx.session_id.to_string(),ctx.principal.storage_kind().into(),ctx.principal.storage_subject(),ctx.generation.to_string(),ctx.attempt.to_string(),call.to_string(),request.target.clone(),request.args_digest.hex.clone()]).await.map_err(storage)?;
        if actual.is_none() {
            return Err(failure(
                "BROWSER_FORBIDDEN",
                "current actual Run and started invocation required",
            ));
        }
        let mut cursor = ctx.run_id.clone();
        let mut seen = std::collections::BTreeSet::new();
        loop {
            if !seen.insert(cursor.clone()) || seen.len() > 64 {
                return Err(failure("BROWSER_FORBIDDEN", "browser source chain invalid"));
            }
            let allowed = self.db.query_one_text("SELECT r.run_id FROM runs r LEFT JOIN browser_workspaces_v1 b ON b.session_id=r.session_id WHERE r.run_id=?1 AND (b.session_id IS NULL OR r.rowid>b.revoked_run_watermark OR (b.revoked=0 AND EXISTS(SELECT 1 FROM browser_commands_v1 c WHERE c.session_id=r.session_id AND c.owner_subject=b.owner_subject AND c.status='confirmed' AND json_extract(c.input_json,'$.action')='human_steer' AND json_extract(c.result_json,'$.runId')=r.run_id AND json_extract(c.result_json,'$.epoch')=b.epoch AND json_extract(c.result_json,'$.authorityGranted')=1)))", vec![cursor.to_string()]).await.map_err(storage)?;
            if allowed.is_none() {
                return Err(failure("BROWSER_REVOKED", "this Run predates the browser emergency stop; a new user submission is required"));
            }
            let lineage = self
                .db
                .load_run_lineage(&cursor)
                .await
                .map_err(storage)?
                .ok_or_else(|| failure("BROWSER_FORBIDDEN", "browser source lineage missing"))?;
            if matches!(
                lineage.origin,
                RunOrigin::Bridge | RunOrigin::Channel | RunOrigin::Dm
            ) {
                return Err(failure(
                    "BROWSER_FORBIDDEN",
                    "external or phone source cannot operate browser",
                ));
            }
            match lineage.parent_run_id {
                Some(parent) => cursor = parent,
                None => break,
            }
        }
        let session = session_tree::get_session_row(&self.db, ctx.session_id.as_str())
            .await
            .map_err(storage)?
            .ok_or_else(|| failure("BROWSER_NOT_FOUND", "session missing"))?;
        if !ctx
            .principal
            .can_access_session(&session.owner_user_id, ctx.session_id.as_str())
        {
            return Err(failure(
                "BROWSER_FORBIDDEN",
                "browser session ownership changed",
            ));
        }
        let row = store::ensure(
            &self.db,
            ctx.session_id.as_str(),
            &session.owner_user_id,
            &self.node,
            self.clock.now_unix_ms(),
        )
        .await
        .map_err(storage)?;
        let bound = crate::toolgateway::review_scope()
            .and_then(|scope| scope["authorizationEpoch"].as_u64());
        if bound != Some(row.epoch) && !(bound == Some(0) && row.epoch == 1 && !row.revoked) {
            return Err(failure(
                "BROWSER_REVOKED",
                "browser authorization changed since confirmation",
            ));
        }
        Ok(row)
    }
    pub async fn command(
        &self,
        row: store::BrowserAuthority,
        command: BrowserCommand,
        human: bool,
        operation: String,
        run: Option<&RunContext>,
    ) -> Result<Value, BrowserError> {
        command
            .validate()
            .map_err(|e| failure("BROWSER_INPUT", e))?;
        if !human && !command.action.is_tool_action() {
            return Err(failure(
                "BROWSER_FORBIDDEN",
                "human browser control required",
            ));
        }
        if row.revoked {
            return Err(failure(
                "BROWSER_REVOKED",
                "browser emergency stop remains active until a new user submission",
            ));
        }
        if let Some(reason) = &self.unavailable {
            return Err(failure("BROWSER_UNAVAILABLE", reason));
        }
        let scope = self.scope(&row, human);
        self.known
            .lock()
            .expect("浏览器边界锁")
            .insert(row.session_id.clone(), scope.clone());
        let input = serde_json::to_value(&command).expect("浏览器命令");
        match store::claim_command(
            &self.db,
            &scope,
            &operation,
            &input,
            false,
            self.clock.now_unix_ms(),
        )
        .await
        .map_err(storage)?
        {
            store::CommandClaim::Started => {}
            store::CommandClaim::Confirmed(value) => return Ok(value),
            store::CommandClaim::Failed(message) => return Err(failure("BROWSER_FAILED", message)),
            store::CommandClaim::Unknown(message) => {
                return Err(BrowserError {
                    code: "BROWSER_UNKNOWN".into(),
                    message,
                    effect_unknown: true,
                })
            }
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let admitted = {
            let mut active = self.active.lock().expect("浏览器操作锁");
            if active.len() >= 64 {
                false
            } else {
                active.insert(operation.clone(), (row.session_id.clone(), cancel.clone()));
                true
            }
        };
        if !admitted {
            store::finish_command(
                &self.db,
                &operation,
                None,
                Some("browser operations at capacity; not dispatched".into()),
                false,
                self.clock.now_unix_ms(),
            )
            .await
            .map_err(storage)?;
            return Err(failure("BROWSER_BUSY", "browser operations at capacity"));
        }
        let _guard = CommandGuard {
            key: operation.clone(),
            active: self.active.clone(),
            cancel: cancel.clone(),
        };
        let scope_cancel = run.and_then(|ctx| {
            self.runs
                .upgrade()
                .and_then(|runs| runs.run_scope(ctx.run_id.as_str()))
        });
        let mut future = self.port.execute(scope.clone(), command, cancel.clone());
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(45);
        let mut cancel_at: Option<tokio::time::Instant> = None;
        let output = loop {
            tokio::select! {result=&mut future=>break result,_=tokio::time::sleep(std::time::Duration::from_millis(20))=>{if scope_cancel.as_ref().is_some_and(|scope|scope.is_cancelled())||tokio::time::Instant::now()>=deadline{cancel.store(true,Ordering::Release);let since=cancel_at.get_or_insert_with(tokio::time::Instant::now);if tokio::time::Instant::now()>=*since+std::time::Duration::from_secs(5){break Err(BrowserError{code:"BROWSER_CLEANUP_UNCONFIRMED".into(),message:"browser cancellation cleanup remains unconfirmed".into(),effect_unknown:true});}}}}
        };
        let result = match output {
            Ok(output) => self
                .accept_output(&scope, output, run)
                .await
                .map_err(|mut error| {
                    error.effect_unknown = true;
                    error
                }),
            Err(error) => Err(error),
        };
        match &result {
            Ok(value) => store::finish_command(
                &self.db,
                &operation,
                Some(value.clone()),
                None,
                false,
                self.clock.now_unix_ms(),
            )
            .await
            .map_err(uncertain_receipt)?,
            Err(error) => store::finish_command(
                &self.db,
                &operation,
                None,
                Some(error.message.clone()),
                error.effect_unknown,
                self.clock.now_unix_ms(),
            )
            .await
            .map_err(uncertain_receipt)?,
        }
        result
    }
    async fn accept_output(
        &self,
        scope: &BrowserScope,
        mut output: BrowserOutput,
        _run: Option<&RunContext>,
    ) -> Result<Value, BrowserError> {
        let _state = self.state_lock.lock().await;
        store::save(
            &self.db,
            scope,
            &output.reply.state,
            self.clock.now_unix_ms(),
        )
        .await
        .map_err(|e| BrowserError {
            code: "BROWSER_RECEIPT_UNKNOWN".into(),
            message: e.to_string(),
            effect_unknown: true,
        })?;
        let mut files = Vec::new();
        let mut acknowledged = Vec::new();
        let mut downloads = Vec::new();
        if let Some(bytes) = output.screenshot.take() {
            let file = self
                .files
                .register_browser_artifact(
                    &scope.session_id,
                    "browser-screenshot.png",
                    "image/png",
                    &bytes,
                    true,
                )
                .await
                .map_err(|e| BrowserError {
                    code: "BROWSER_ARTIFACT_UNKNOWN".into(),
                    message: e.to_string(),
                    effect_unknown: true,
                })?;
            files.push(serde_json::to_value(file).expect("会话文件"));
        }
        for download in output.downloads {
            let mut file_id = None;
            let mut error = None;
            if download.completed {
                if let Some(bytes) = download.bytes {
                    let name = download
                        .suggested_filename
                        .rsplit(['/', '\\'])
                        .next()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("download.bin");
                    match self
                        .files
                        .register_browser_artifact(
                            &scope.session_id,
                            name,
                            "application/octet-stream",
                            &bytes,
                            false,
                        )
                        .await
                    {
                        Ok(file) => {
                            file_id = Some(file.file_id.clone());
                            files.push(serde_json::to_value(file).expect("下载文件"));
                        }
                        Err(e) => error = Some(e.to_string()),
                    }
                } else {
                    error = Some("completed download bytes unavailable".into());
                }
            } else {
                error = Some("download completion unconfirmed".into());
            }
            downloads.push(json!({"id":download.id,"status":if file_id.is_some(){"completed"}else{"unknown"},"fileId":file_id,"error":error}));
            if file_id.is_some() {
                acknowledged.push(download.id.clone());
            }
            store::record_download(
                &self.db,
                scope,
                &download.id,
                None,
                file_id.as_deref(),
                error.as_deref(),
                self.clock.now_unix_ms(),
            )
            .await
            .map_err(storage)?;
        }
        self.port
            .acknowledge_downloads(scope.clone(), acknowledged)
            .await?;
        let current = store::get(&self.db, &scope.session_id)
            .await
            .map_err(uncertain_receipt)?;
        if current.as_ref().is_none_or(|row| {
            row.revoked || row.epoch != scope.epoch || row.owner_subject != scope.owner_subject
        }) {
            return Err(BrowserError {
                code: "BROWSER_RECEIPT_UNKNOWN".into(),
                message: "浏览器动作后授权发生变化，旧状态不再发布。".into(),
                effect_unknown: true,
            });
        }
        output.reply.files = files;
        output.reply.downloads = downloads;
        let value = serde_json::to_value(&output.reply).expect("浏览器回执");
        self.events.publish("browser_status",json!({"sessionPath":output.reply.state.session_path,"running":output.reply.state.running,"url":output.reply.state.url,"executionNode":self.node,"workspace":output.reply.state}));
        Ok(value)
    }
    async fn record_revoke_state(
        &self,
        row: &store::BrowserAuthority,
        confirmed: bool,
    ) -> Result<(), BrowserError> {
        let mut state = row.state.clone().unwrap_or_else(|| BrowserWorkspace {
            session_id: row.session_id.clone(),
            session_path: crate::session_locator::native(&row.session_id),
            execution_node: row.execution_node.clone(),
            running: false,
            resumable: false,
            unavailable_reason: None,
            url: None,
            title: None,
            active_tab_id: None,
            tabs: vec![],
            suspended: false,
            user_takeover: false,
            revoked: true,
            visible_capable: false,
        });
        state.revoked = true;
        if confirmed {
            state.running = false;
            state.resumable = false;
            state.user_takeover = false;
            state.suspended = false;
            state.tabs.clear();
            state.active_tab_id = None;
            state.url = None;
            state.title = None;
            state.unavailable_reason = None;
        } else {
            state.unavailable_reason = Some(
                "BROWSER_CLEANUP_UNCONFIRMED：权限已撤销，浏览器进程与页面回收尚未确认".into(),
            );
        }
        store::save_revoked_state(
            &self.db,
            &self.scope(row, true),
            &state,
            self.clock.now_unix_ms(),
        )
        .await
        .map_err(uncertain_receipt)
    }
    pub async fn close(
        &self,
        row: store::BrowserAuthority,
        revoke: bool,
        operation: String,
    ) -> Result<(), BrowserError> {
        if !revoke {
            self.command(
                row,
                BrowserCommand::new(BrowserAction::Stop),
                true,
                operation,
                None,
            )
            .await?;
            return Ok(());
        }
        match store::claim_command(
            &self.db,
            &self.scope(&row, true),
            &operation,
            &json!({"action":"emergency_stop"}),
            true,
            self.clock.now_unix_ms(),
        )
        .await
        .map_err(storage)?
        {
            store::CommandClaim::Confirmed(_) => return Ok(()),
            store::CommandClaim::Failed(message) => return Err(failure("BROWSER_FAILED", message)),
            store::CommandClaim::Unknown(message) => {
                return Err(BrowserError {
                    code: "BROWSER_UNKNOWN".into(),
                    message,
                    effect_unknown: true,
                })
            }
            store::CommandClaim::Started => {}
        }
        for (sid, cancel) in self.active.lock().expect("浏览器操作锁").values() {
            if sid == &row.session_id {
                cancel.store(true, Ordering::Release);
            }
        }
        let _authority = self.authority_lock.lock().await;
        let result = async {
            let revoked = store::revoke_human_intent(
                &self.db,
                &row.session_id,
                &row.owner_subject,
                &operation,
                self.clock.now_unix_ms(),
            )
            .await
            .map_err(storage)?;
            self.known
                .lock()
                .expect("浏览器边界锁")
                .insert(revoked.session_id.clone(), self.scope(&revoked, false));
            match self.port.revoke(self.scope(&revoked, true)).await {
                Ok(()) => self.record_revoke_state(&revoked, true).await?,
                Err(error) => {
                    self.record_revoke_state(&revoked, false).await?;
                    return Err(BrowserError {
                        code: "BROWSER_CLEANUP_UNCONFIRMED".into(),
                        message: format!("浏览器授权已撤销，关闭清理结果未确认：{error}"),
                        effect_unknown: true,
                    });
                }
            }
            Ok::<(), BrowserError>(())
        }
        .await;
        store::finish_command(
            &self.db,
            &operation,
            result.as_ref().ok().map(|_| json!({"revoked":true})),
            result.as_ref().err().map(ToString::to_string),
            result.is_err(),
            self.clock.now_unix_ms(),
        )
        .await
        .map_err(uncertain_receipt)?;
        if result.is_ok() {
            let _state = self.state_lock.lock().await;
            self.events.publish("browser_status",json!({"sessionPath":crate::session_locator::native(&row.session_id),"running":false,"url":null,"executionNode":self.node,"revoked":true}));
        }
        result
    }
    pub async fn workspaces(
        &self,
        principal: &Principal,
    ) -> Result<Vec<BrowserWorkspace>, BrowserError> {
        let live = self.port.workspaces()?;
        let mut result = vec![];
        for row in store::list(&self.db).await.map_err(storage)? {
            if row.execution_node != self.node {
                continue;
            }
            let Some(session) = session_tree::get_session_row(&self.db, &row.session_id)
                .await
                .map_err(storage)?
            else {
                continue;
            };
            if session.lifecycle != "active"
                || session.owner_user_id != row.owner_subject
                || !crate::sessions::SessionStore::can_access_session(
                    principal,
                    &session.owner_user_id,
                    &row.session_id,
                )
            {
                continue;
            }
            let is_live = live
                .iter()
                .any(|s| s.session_id == row.session_id && s.execution_node == self.node);
            let state = live
                .iter()
                .find(|s| s.session_id == row.session_id && s.execution_node == self.node)
                .cloned()
                .or(row.state.clone());
            if let Some(mut state) = state {
                if !is_live {
                    state.running = false;
                    state.user_takeover = false;
                    state.resumable = !state.tabs.is_empty();
                }
                state.revoked = row.revoked;
                if row.revoked {
                    state.user_takeover = false;
                    let unknown = row
                        .state
                        .as_ref()
                        .and_then(|state| state.unavailable_reason.as_deref())
                        .is_some_and(|reason| reason.starts_with("BROWSER_CLEANUP_UNCONFIRMED"));
                    if state.running || unknown {
                        state.unavailable_reason = Some(
                            "BROWSER_CLEANUP_UNCONFIRMED：权限已撤销，浏览器页面回收尚未确认"
                                .into(),
                        );
                    }
                }
                if let Some(reason) = &self.unavailable {
                    state.running = false;
                    state.unavailable_reason = Some(reason.clone());
                }
                result.push(state);
            }
        }
        Ok(result)
    }
    pub async fn maintain(&self) -> Result<(), BrowserError> {
        // 打印可能从未建立浏览器工作区。独立核真实会话归属并回收其临时租约，
        // 不借known/R20补造页面事实，也不把Run generation当作浏览器epoch。
        for scope in self.port.pdf_scopes()? {
            let session = session_tree::get_session_row(&self.db, &scope.session_id)
                .await
                .map_err(storage)?;
            if session.as_ref().is_none_or(|session| {
                session.lifecycle != "active" || session.owner_user_id != scope.owner_subject
            }) {
                self.port.cancel_pdf_scope(scope).await?;
            }
        }
        let known = self
            .known
            .lock()
            .expect("浏览器边界锁")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for scope in known {
            let session = session_tree::get_session_row(&self.db, &scope.session_id)
                .await
                .map_err(storage)?;
            if session
                .as_ref()
                .is_none_or(|s| s.lifecycle != "active" || s.owner_user_id != scope.owner_subject)
            {
                for (sid, cancel) in self.active.lock().expect("浏览器操作锁").values() {
                    if sid == &scope.session_id {
                        cancel.store(true, Ordering::Release);
                    }
                }
                let _authority = self.authority_lock.lock().await;
                let cleanup_scope = match store::revoke(
                    &self.db,
                    &scope.session_id,
                    &scope.owner_subject,
                    self.clock.now_unix_ms(),
                )
                .await
                .map_err(storage)?
                {
                    Some(row) => self.scope(&row, true),
                    None => BrowserScope {
                        epoch: scope.epoch.checked_add(1).ok_or_else(|| {
                            failure("BROWSER_AUTHORITY", "browser epoch exhausted")
                        })?,
                        human: true,
                        ..scope.clone()
                    },
                };
                if session
                    .as_ref()
                    .is_none_or(|s| s.owner_user_id != scope.owner_subject)
                {
                    self.port.forget(cleanup_scope).await?;
                    self.known
                        .lock()
                        .expect("浏览器边界锁")
                        .remove(&scope.session_id);
                } else {
                    let cleanup = self.port.revoke(cleanup_scope.clone()).await;
                    if let Some(row) = store::get(&self.db, &scope.session_id)
                        .await
                        .map_err(storage)?
                    {
                        self.record_revoke_state(&row, cleanup.is_ok()).await?;
                    }
                    cleanup?;
                    self.known
                        .lock()
                        .expect("浏览器边界锁")
                        .insert(scope.session_id.clone(), cleanup_scope);
                }
                let _state = self.state_lock.lock().await;
                self.events.publish("browser_status", json!({"sessionPath":crate::session_locator::native(&scope.session_id),"executionNode":self.node,"running":false,"url":null}));
            }
        }
        self.port.reap_idle(self.clock.now_unix_ms()).await?;
        self.sync_workspaces().await
    }
    async fn sync_workspaces(&self) -> Result<(), BrowserError> {
        let _state = self.state_lock.lock().await;
        for workspace in self.port.workspaces()? {
            let Some(row) = store::get(&self.db, &workspace.session_id)
                .await
                .map_err(storage)?
            else {
                continue;
            };
            if row.revoked || row.execution_node != self.node {
                continue;
            }
            let session = session_tree::get_session_row(&self.db, &workspace.session_id)
                .await
                .map_err(storage)?;
            if session
                .is_none_or(|s| s.lifecycle != "active" || s.owner_user_id != row.owner_subject)
            {
                continue;
            }
            if row.state.as_ref() != Some(&workspace) {
                store::save(
                    &self.db,
                    &self.scope(&row, false),
                    &workspace,
                    self.clock.now_unix_ms(),
                )
                .await
                .map_err(storage)?;
                self.events.publish("browser_status", json!({"sessionPath":workspace.session_path,"executionNode":self.node,"running":workspace.running,"url":workspace.url,"workspace":workspace}));
            }
        }
        Ok(())
    }
    pub fn start(self: &Arc<Self>) {
        let mut worker = self.worker.lock().expect("浏览器维护锁");
        if worker.is_some()
            || self
                .worker_result
                .lock()
                .expect("浏览器退出记录锁")
                .is_some()
        {
            return;
        }
        self.stop.send_replace(false);
        let this = self.clone();
        *worker = Some(tokio::spawn(async move {
            let mut stop = this.stop.subscribe();
            let mut ticks = tokio::time::interval(std::time::Duration::from_secs(30));
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! { biased; _ = stop.wait_for(|v| *v) => break, _ = ticks.tick() => {} }
                if let Err(e) = this.maintain().await {
                    tracing::error!(error=%e,"浏览器维护失败");
                }
            }
            this.shutdown().await.map_err(|e| e.to_string())
        }));
    }
    pub fn stop(&self) {
        self.stop.send_replace(true);
    }
    pub async fn wait_stopped(&self) -> Result<(), String> {
        let _join = self.join_lock.lock().await;
        std::future::poll_fn(|cx| {
            let mut task = self.worker.lock().expect("浏览器维护锁");
            let Some(handle) = task.as_mut() else {
                return std::task::Poll::Ready(
                    self.worker_result
                        .lock()
                        .expect("浏览器退出记录锁")
                        .clone()
                        .unwrap_or(Ok(())),
                );
            };
            match std::future::Future::poll(std::pin::Pin::new(handle), cx) {
                std::task::Poll::Ready(result) => {
                    let result = result.map_err(|e| e.to_string()).and_then(|result| result);
                    // 同一宿主的重复等待共享真实退出结果，不能把未知清理改写成成功。
                    *self.worker_result.lock().expect("浏览器退出记录锁") = Some(result.clone());
                    *task = None;
                    std::task::Poll::Ready(result)
                }
                std::task::Poll::Pending => std::task::Poll::Pending,
            }
        })
        .await
    }
    pub async fn resume_for(
        &self,
        principal: &Principal,
        sid: &str,
        operation: String,
    ) -> Result<Value, BrowserError> {
        let row = self.authorize(principal, sid).await?;
        let live = self.port.workspaces()?;
        let existing = live.iter().any(|w| w.session_id == sid && w.running);
        if !existing && row.state.as_ref().is_none_or(|s| s.tabs.is_empty()) {
            return Ok(
                json!({"status":"skipped","reason":"no_browser_state","canResume":false,"executionNode":self.node}),
            );
        }
        if !principal.is_local_owner() && !principal.has_scope("admin") {
            return Ok(
                json!({"status":"skipped","reason":"human_browser_control_required","executionNode":self.node}),
            );
        }
        let reply = self
            .command(
                row,
                BrowserCommand::new(BrowserAction::Resume),
                true,
                operation,
                None,
            )
            .await?;
        let running = reply["state"]["running"].as_bool().unwrap_or(false);
        Ok(
            json!({"status":if running {"resumed"}else{"attempted"},"found":running,"running":running,"url":reply["state"]["url"],"executionNode":self.node}),
        )
    }
    pub async fn shutdown(&self) -> Result<(), BrowserError> {
        for (_, cancel) in self.active.lock().expect("浏览器操作锁").values() {
            cancel.store(true, Ordering::Release);
        }
        self.port.shutdown().await
    }
}

/// 浏览器始终运行在本服务节点；缺少可执行文件时显式报告不可用。
pub fn production_port(
    state: &ServiceState,
    injected: Option<Arc<dyn BrowserPort>>,
) -> Result<(Arc<dyn BrowserPort>, Option<String>), crate::ServiceStartupError> {
    if let Some(port) = injected {
        return Ok((port, None));
    }
    #[cfg(unix)]
    {
        let configured = std::env::var_os("LINGXI_CHROMIUM_PATH").map(std::path::PathBuf::from);
        let binary = if let Some(path) = configured {
            if !path.is_absolute() || !path.is_file() {
                return Err(crate::ServiceStartupError::Storage(lingxi_kernel::ports::StorageError::InvalidRequest{detail:"LINGXI_CHROMIUM_PATH must name an existing absolute Chromium executable".into()}));
            }
            Some(path)
        } else {
            [
                state.config().data_home.join("bundled-bin/chromium/chrome"),
                std::path::PathBuf::from(
                    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                ),
                std::path::PathBuf::from("/usr/bin/chromium"),
                std::path::PathBuf::from("/usr/bin/chromium-browser"),
                std::path::PathBuf::from("/usr/bin/google-chrome"),
            ]
            .into_iter()
            .find(|p| p.is_file())
        };
        if let Some(executable) = binary {
            let config = lingxi_adapters::browser::ChromiumConfig {
                executable,
                root: state.config().data_home.join("lingxi-service/browser"),
                execution_node: state.management.identity_ids().0,
                headless: std::env::var("LINGXI_BROWSER_HEADLESS").is_ok_and(|v| v == "1"),
                network: state.wired_network_plane().cloned().unwrap_or_else(
                    lingxi_adapters::models::network::NetworkPlane::from_environment,
                ),
            };
            let port = lingxi_adapters::browser::ChromiumHost::new(config).map_err(|e| {
                crate::ServiceStartupError::Storage(
                    lingxi_kernel::ports::StorageError::InvalidRequest {
                        detail: e.to_string(),
                    },
                )
            })?;
            return Ok((port, None));
        }
    }
    let reason = if cfg!(unix) {
        "本服务节点没有配置受控Chromium；不会使用客户端浏览器代替。"
    } else {
        "本系统的受控Chromium管道尚不可用；不会降级为客户端浏览器。"
    }
    .to_owned();
    Ok((
        Arc::new(UnavailableBrowser {
            reason: reason.clone(),
        }),
        Some(reason),
    ))
}

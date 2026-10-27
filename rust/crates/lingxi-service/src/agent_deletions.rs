//! 助手删除：同盘真实原子暂存是逻辑提交点，物理清理单独记录并可恢复。
use super::{preferences_locked, AgentLifecycleError};
use crate::{memory::MemoryService, ServiceState};
use axum::{
    extract::{Path as AxumPath, State},
    response::{IntoResponse, Response},
    Json,
};
use lingxi_adapters::storage::agent_memory as am;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};
static DELETIONS: Mutex<()> = Mutex::new(());
pub(super) fn lock_deletions() -> MutexGuard<'static, ()> {
    DELETIONS.lock().unwrap_or_else(|e| e.into_inner())
}
fn io_error(error: impl std::fmt::Display) -> AgentLifecycleError {
    AgentLifecycleError::internal(format!("agent deletion storage failure: {error}"))
}
fn valid_id(id: &str) -> Result<(), AgentLifecycleError> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(AgentLifecycleError::bad_request(
            "invalid deletion receipt id",
        ));
    }
    Ok(())
}
fn real_dir(path: &Path) -> Result<(), AgentLifecycleError> {
    let meta = std::fs::symlink_metadata(path).map_err(io_error)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(io_error("deletion directory must be a real directory"));
    }
    Ok(())
}
fn directory_present(path: &Path) -> Result<bool, AgentLifecycleError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(io_error("deletion data path must be a real directory")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_error(error)),
    }
}
fn create_private_dir(path: &Path) -> Result<(), AgentLifecycleError> {
    if !path.exists() {
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path).map_err(io_error)?;
    }
    real_dir(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(io_error)?;
    }
    Ok(())
}
fn sync_dir(path: &Path) -> Result<(), AgentLifecycleError> {
    #[cfg(unix)]
    {
        std::fs::File::open(path)
            .and_then(|file| file.sync_all())
            .map_err(io_error)?;
    }
    // Windows标准文件接口无目录同步；文件本身sync_all，范围为进程重启恢复。
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DeletionPhase {
    AfterPrepared,
    AfterMoved,
}
type Hook = Arc<dyn Fn(DeletionPhase, &Path) + Send + Sync>;
struct RegisteredHook {
    token: Arc<()>,
    hook: Hook,
}
static HOOKS: Mutex<BTreeMap<PathBuf, Vec<RegisteredHook>>> = Mutex::new(BTreeMap::new());
/// 可信时序探针只在真实持久准备/真实rename之后调用，HTTP没有注入入口。
#[doc(hidden)]
pub struct DeletionTestHookGuard {
    home: PathBuf,
    token: Arc<()>,
}
impl Drop for DeletionTestHookGuard {
    fn drop(&mut self) {
        let removed = {
            let mut hooks = HOOKS
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(entries) = hooks.get_mut(&self.home) else {
                return;
            };
            let removed = entries
                .iter()
                .position(|entry| Arc::ptr_eq(&entry.token, &self.token))
                .map(|index| entries.remove(index));
            if entries.is_empty() {
                hooks.remove(&self.home);
            }
            removed
        };
        // 捕获对象的释放也留在锁外，不能因其持有其它探针而重入登记锁。
        drop(removed);
    }
}
#[doc(hidden)]
pub fn deletion_test_hook(home: &Path, hook: Hook) -> DeletionTestHookGuard {
    let token = Arc::new(());
    HOOKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entry(home.to_path_buf())
        .or_default()
        .push(RegisteredHook {
            token: Arc::clone(&token),
            hook,
        });
    DeletionTestHookGuard {
        home: home.to_path_buf(),
        token,
    }
}
fn probe(home: &Path, phase: DeletionPhase, transaction: &Path) {
    let hook = HOOKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(home)
        .and_then(|entries| entries.last())
        .map(|entry| Arc::clone(&entry.hook));
    if let Some(hook) = hook {
        hook(phase, transaction);
    }
}
impl MemoryService {
    fn deletion_root(&self) -> PathBuf {
        self.deletion_runtime_dir().join("agent-deletions")
    }
    fn deletion_transaction(&self, id: &str) -> Result<PathBuf, AgentLifecycleError> {
        valid_id(id)?;
        let root = self.deletion_root();
        real_dir(&root)?;
        let transaction = root.join(id);
        real_dir(&transaction)?;
        Ok(transaction)
    }
    fn save_deletion_receipt(&self, receipt: &mut Value) -> Result<(), AgentLifecycleError> {
        receipt["updatedAt"] = json!(am::now_iso());
        let id = receipt["deletionId"]
            .as_str()
            .ok_or_else(|| io_error("receipt omitted id"))?;
        let dir = self.deletion_transaction(id)?;
        let path = dir.join("receipt.json");
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err(io_error("receipt must be a real file"));
            }
        }
        let temp = dir.join(format!(
            ".receipt-{}.tmp",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(io_error)?
                .as_nanos()
        ));
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(io_error)?;
        file.write_all(
            serde_json::to_string_pretty(receipt)
                .map_err(io_error)?
                .as_bytes(),
        )
        .map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        std::fs::rename(&temp, &path).map_err(io_error)?;
        sync_dir(&dir)
    }
    fn read_deletion_receipt(&self, id: &str) -> Result<Value, AgentLifecycleError> {
        let dir = self.deletion_transaction(id)?;
        let path = dir.join("receipt.json");
        let meta = std::fs::symlink_metadata(&path).map_err(io_error)?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(io_error("receipt must be a real file"));
        }
        let value: Value =
            serde_json::from_slice(&std::fs::read(path).map_err(io_error)?).map_err(io_error)?;
        if value["schemaVersion"] != 1
            || value["deletionId"] != id
            || !value["logicalDeleted"].is_boolean()
            || !value["cleanupPending"].is_boolean()
            || !value["referencesPending"].is_boolean()
            || !matches!(
                value["status"].as_str(),
                Some("prepared" | "committed" | "completed" | "aborted")
            )
            || value["logicalDeleted"].as_bool()
                != Some(matches!(
                    value["status"].as_str(),
                    Some("committed" | "completed")
                ))
        {
            return Err(io_error("invalid deletion receipt"));
        }
        am::validate_agent_id(
            value["agentId"]
                .as_str()
                .ok_or_else(|| io_error("receipt agent id missing"))?,
        )?;
        am::validate_agent_id(
            value["targetReplacementAgentId"]
                .as_str()
                .ok_or_else(|| io_error("receipt replacement id missing"))?,
        )?;
        self.validate_deletion_materials(&value)?;
        Ok(value)
    }
    // 所有入口使用同一实物判定；坏收据不能停用现役助手、改引用或清掉暂存。
    fn validate_deletion_materials(&self, receipt: &Value) -> Result<(), AgentLifecycleError> {
        let agent = receipt["agentId"].as_str().unwrap();
        let id = receipt["deletionId"].as_str().unwrap();
        let public_present = self.public_agent_data_present(agent)?;
        let staged_present = directory_present(&self.deletion_transaction(id)?.join("data"))?;
        let consistent = match receipt["status"].as_str().unwrap() {
            "prepared" => public_present != staged_present,
            "aborted" => !staged_present,
            "committed" => !public_present,
            "completed" => !public_present && !staged_present,
            _ => false,
        };
        if !consistent {
            return Err(io_error(
                "deletion receipt contradicts actual public/staged agent data; recovery refused",
            ));
        }
        Ok(())
    }
    /// 公共路径只剩 `.deleted-agent.json` 墓碑（R08 收尾续接接缝）视为已无
    /// agent 数据——墓碑是删除事实的载体，不是残留 agent。
    fn public_agent_data_present(&self, agent: &str) -> Result<bool, AgentLifecycleError> {
        let dir = self.agents_root().join(agent);
        if !directory_present(&dir)? {
            return Ok(false);
        }
        let mut entries = std::fs::read_dir(&dir).map_err(io_error)?;
        loop {
            match entries.next() {
                Some(Ok(entry)) => {
                    if entry.file_name().to_string_lossy() != ".deleted-agent.json" {
                        return Ok(true);
                    }
                }
                Some(Err(error)) => return Err(io_error(error)),
                None => return Ok(false),
            }
        }
    }
    // 首份收据之前崩溃只能留下空目录或未发布收据；不碰任何带data/未知文件的目录。
    fn unpublished_intent(&self, dir: &Path) -> Result<bool, AgentLifecycleError> {
        match std::fs::symlink_metadata(dir.join("receipt.json")) {
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
        for entry in std::fs::read_dir(dir).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = entry.path().symlink_metadata().map_err(io_error)?;
            let temporary = name
                .strip_prefix(".receipt-")
                .and_then(|s| s.strip_suffix(".tmp"))
                .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()));
            if !temporary || !meta.is_file() || meta.file_type().is_symlink() {
                return Err(io_error(
                    "receiptless deletion directory contains data; recovery refused",
                ));
            }
        }
        Ok(true)
    }
    fn prune_unpublished_intents(&self) -> Result<(), AgentLifecycleError> {
        let root = self.deletion_root();
        if !root.exists() {
            return Ok(());
        }
        real_dir(&root)?;
        for entry in std::fs::read_dir(&root).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            valid_id(&entry.file_name().to_string_lossy())?;
            real_dir(&entry.path())?;
            if self.unpublished_intent(&entry.path())? {
                std::fs::remove_dir_all(entry.path()).map_err(io_error)?;
                sync_dir(&root)?;
            }
        }
        Ok(())
    }
    fn deletion_ids(&self) -> Result<Vec<String>, AgentLifecycleError> {
        let root = self.deletion_root();
        let entries = match std::fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(io_error(e)),
        };
        real_dir(&root)?;
        let mut out = Vec::new();
        for entry in entries {
            let entry = entry.map_err(io_error)?;
            let id = entry.file_name().to_string_lossy().to_string();
            valid_id(&id)?;
            real_dir(&entry.path())?;
            if !self.unpublished_intent(&entry.path())? {
                out.push(id);
            }
        }
        out.sort();
        Ok(out)
    }
    pub(super) fn reserved_deleted_agent_ids(&self) -> Result<Vec<String>, AgentLifecycleError> {
        self.deletion_ids()?
            .into_iter()
            .map(|id| self.read_deletion_receipt(&id))
            .collect::<Result<Vec<_>, _>>()
            .map(|receipts| {
                receipts
                    .into_iter()
                    .filter(|r| r["status"] != "aborted")
                    .filter_map(|r| r["agentId"].as_str().map(str::to_owned))
                    .collect()
            })
    }
    fn prepare_deletion(&self, agent: &str) -> Result<Value, AgentLifecycleError> {
        let dir = self.agent_dir_checked(agent)?;
        real_dir(&dir)?;
        let live = self.ordered_live_agents()?;
        let other = live
            .iter()
            .find(|(id, _)| id != agent)
            .ok_or_else(|| AgentLifecycleError::bad_request("cannot delete the last agent"))?;
        let preferences = self.read_preferences_strict()?;
        let referenced =
            preferences["current_agent_id"] == agent || preferences["primary_agent_id"] == agent;
        real_dir(self.deletion_runtime_dir())?;
        create_private_dir(&self.deletion_root())?;
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = format!(
            "delete-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(io_error)?
                .as_nanos(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        create_private_dir(&self.deletion_root().join(&id))?;
        sync_dir(&self.deletion_root())?;
        let mut receipt = json!({"schemaVersion":1,"deletionId":id,"agentId":agent,"targetReplacementAgentId":other.0,"wasReferenced":referenced,"logicalDeleted":false,"cleanupPending":false,"referencesPending":false,"replacementAgentId":null,"status":"prepared","createdAt":am::now_iso()});
        if let Err(error) = self.save_deletion_receipt(&mut receipt) {
            let cleanup = std::fs::remove_dir_all(self.deletion_root().join(&id));
            return Err(AgentLifecycleError::internal(format!(
                "{error}; preparation cleanup: {cleanup:?}"
            )));
        }
        probe(
            self.home(),
            DeletionPhase::AfterPrepared,
            &self.deletion_transaction(&id)?,
        );
        Ok(receipt)
    }
    fn repair_deletion_references(&self, receipt: &mut Value) -> Result<(), AgentLifecycleError> {
        let agent = receipt["agentId"].as_str().unwrap().to_owned();
        let _preferences = preferences_locked();
        let mut prefs = self.read_preferences_strict()?;
        let needs_switch = ["current_agent_id", "primary_agent_id"]
            .iter()
            .any(|key| prefs[*key] == agent);
        let mut replacement = receipt["targetReplacementAgentId"]
            .as_str()
            .unwrap()
            .to_owned();
        if needs_switch && self.agent_dir_checked(&replacement).is_err() {
            replacement = self
                .ordered_live_agents()?
                .into_iter()
                .find(|(id, _)| id != &agent)
                .map(|(id, _)| id)
                .ok_or_else(|| io_error("no live replacement for pending deletion references"))?;
            receipt["referenceRepairReplacement"] = json!(replacement);
            receipt["targetReplacementAgentId"] = json!(replacement);
        }
        let mut changed = false;
        for key in ["current_agent_id", "primary_agent_id"] {
            if prefs[key] == agent {
                prefs[key] = json!(replacement);
                changed = true;
            }
        }
        if let Some(order) = prefs["agent_order"].as_array_mut() {
            let old = order.len();
            order.retain(|id| id != &agent);
            changed |= old != order.len();
        }
        if changed {
            self.write_preferences(&prefs)?;
        }
        receipt["referencesPending"] = json!(false);
        receipt["referenceError"] = Value::Null;
        if receipt["wasReferenced"] == true {
            receipt["replacementAgentId"] = json!(replacement);
        }
        Ok(())
    }
    fn settle_deletion(&self, receipt: &mut Value) -> Result<(), AgentLifecycleError> {
        let id = receipt["deletionId"].as_str().unwrap().to_owned();
        let transaction = self.deletion_transaction(&id)?;
        self.validate_deletion_materials(receipt)?;
        if receipt["logicalDeleted"] != true {
            return Ok(());
        }
        receipt["status"] = json!("committed");
        receipt["cleanupPending"] = json!(true);
        // 提交收据未能持久化前不清理暂存；重启可从真实prepared+data恢复。
        if let Err(error) = self.save_deletion_receipt(receipt) {
            receipt["journalError"] = json!(error.to_string());
            return Ok(());
        }
        match self.repair_deletion_references(receipt) {
            Ok(()) => {}
            Err(error) => {
                receipt["referencesPending"] = json!(true);
                receipt["referenceError"] = json!(error.to_string());
                self.save_deletion_receipt(receipt)?;
                return Ok(());
            }
        }
        self.save_deletion_receipt(receipt)?;
        let staged = transaction.join("data");
        let result = match std::fs::symlink_metadata(&staged) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {
                std::fs::remove_dir_all(&staged)
            }
            Ok(_) => return Err(io_error("staged agent data must be a real directory")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        };
        match result {
            Ok(()) => {
                receipt["cleanupPending"] = json!(false);
                receipt["cleanupError"] = Value::Null;
                receipt["status"] = json!("completed");
                sync_dir(&transaction)?;
            }
            Err(error) => {
                receipt["cleanupPending"] = json!(true);
                receipt["cleanupError"] = json!(error.to_string());
            }
        }
        self.save_deletion_receipt(receipt)
    }
    pub fn delete_agent(&self, agent: &str) -> Result<Value, AgentLifecycleError> {
        let _deletions = lock_deletions();
        let mut receipt = self.dream().delete_agent_files(
            agent,
            || self.prepare_deletion(agent),
            |mut receipt| {
                let id = receipt["deletionId"].as_str().unwrap().to_owned();
                let transaction = self.deletion_transaction(&id)?;
                let dir = self.agents_root().join(agent);
                let _publication = am::ContextMaterialMutation::for_path(&dir);
                if let Err(error) = std::fs::rename(&dir, transaction.join("data")) {
                    receipt["status"] = json!("aborted");
                    receipt["moveError"] = json!(error.to_string());
                    self.save_deletion_receipt(&mut receipt).map_err(|save| {
                        AgentLifecycleError::internal(format!(
                            "cannot stage agent: {error}; abort receipt failed: {save}"
                        ))
                    })?;
                    return Err(AgentLifecycleError::internal(format!(
                        "cannot stage agent for deletion: {error}"
                    )));
                }
                receipt["logicalDeleted"] = json!(true);
                receipt["cleanupPending"] = json!(true);
                receipt["referencesPending"] = json!(true);
                receipt["status"] = json!("committed");
                // R08 收尾（continue-deleted-agent 接缝）：现役删除语义在原位留
                // `.deleted-agent.json` 墓碑（旧栈 _deletedAgentTombstonePath），
                // is_agent_deleted_checked / scan_live_agent_dirs / 续接路由都以它
                // 判定"已删"。目录移入事务后公共路径补写墓碑，删除事实经同一
                // 约定对全路由可见；写失败按 journalError 如实记录不静默。
                let tombstone = dir.join(".deleted-agent.json");
                let tombstone_body = serde_json::json!({
                    "schemaVersion": 1,
                    "agentId": agent,
                    "deletionId": receipt["deletionId"],
                    "logicalDeleted": true,
                });
                if let Err(error) = std::fs::create_dir_all(&dir)
                    .and_then(|_| std::fs::write(&tombstone, format!("{tombstone_body}\n")))
                {
                    receipt["tombstoneError"] = json!(error.to_string());
                }
                if let Err(error) =
                    sync_dir(&self.agents_root()).and_then(|_| sync_dir(&transaction))
                {
                    receipt["journalError"] = json!(error.to_string());
                }
                probe(self.home(), DeletionPhase::AfterMoved, &transaction);
                self.validate_deletion_materials(&receipt)?;
                Ok(receipt)
            },
        )?;
        if let Err(error) = self.settle_deletion(&mut receipt) {
            receipt["journalError"] = json!(error.to_string());
        }
        Ok(public_receipt(&receipt))
    }
    /// prepared收据按真实两处目录判断；不把可能半清的data搬回公共助手目录。
    fn recover_one_deletion(&self, id: &str) -> Result<Value, AgentLifecycleError> {
        let mut receipt = self.read_deletion_receipt(id)?;
        if receipt["status"] == "aborted" {
            return Ok(public_receipt(&receipt));
        }
        let agent = receipt["agentId"].as_str().unwrap().to_owned();
        let staged = self.deletion_transaction(id)?.join("data");
        if receipt["status"] == "prepared" {
            let public_present = directory_present(&self.agents_root().join(&agent))?;
            if directory_present(&staged)? {
                if public_present {
                    return Err(io_error(
                        "prepared deletion has both public and staged agent; recovery refused",
                    ));
                }
                receipt["logicalDeleted"] = json!(true);
                receipt["status"] = json!("committed");
                receipt["referencesPending"] = json!(true);
            } else if public_present {
                receipt["status"] = json!("aborted");
                self.save_deletion_receipt(&mut receipt)?;
                return Ok(public_receipt(&receipt));
            } else {
                return Err(io_error("prepared receipt lost both original and staged agent; cannot invent deletion outcome"));
            }
        }
        self.dream().stop_agent(&agent);
        self.settle_deletion(&mut receipt)?;
        Ok(public_receipt(&receipt))
    }
    pub fn recover_agent_deletions(&self) -> Result<(), AgentLifecycleError> {
        let _lock = lock_deletions();
        self.prune_unpublished_intents()?;
        let ids = self.deletion_ids()?;
        // 先验证整批收据，避免后面的坏记录让启动恢复只执行一半业务动作。
        for id in &ids {
            self.read_deletion_receipt(id)?;
        }
        for id in ids {
            self.recover_one_deletion(&id)?;
        }
        Ok(())
    }
    pub fn retry_agent_deletion_cleanup(&self, id: &str) -> Result<Value, AgentLifecycleError> {
        let _lock = lock_deletions();
        self.recover_one_deletion(id)
    }
    pub fn agent_deletion_receipts(&self) -> Result<Vec<Value>, AgentLifecycleError> {
        let _lock = lock_deletions();
        self.deletion_ids()?
            .into_iter()
            .map(|id| {
                let mut receipt = self.read_deletion_receipt(&id)?;
                if receipt["status"] == "prepared" {
                    let staged = self.deletion_transaction(&id)?.join("data");
                    match std::fs::symlink_metadata(&staged) {
                        Ok(_) => {
                            real_dir(&staged)?;
                            let agent = receipt["agentId"].as_str().unwrap();
                            if directory_present(&self.agents_root().join(agent))? {
                                return Err(io_error("prepared deletion has both public and staged agent; query refused"));
                            }
                            receipt["logicalDeleted"] = json!(true);
                            receipt["cleanupPending"] = json!(true);
                            receipt["referencesPending"] = json!(true);
                            receipt["status"] = json!("committed");
                            receipt["journalCheckpointPending"] = json!(true);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(io_error(error)),
                    }
                }
                Ok(public_receipt(&receipt))
            })
            .collect()
    }
}
fn public_receipt(receipt: &Value) -> Value {
    let mut value = receipt.clone();
    let object = value.as_object_mut().unwrap();
    object.remove("schemaVersion");
    object.remove("targetReplacementAgentId");
    object.remove("wasReferenced");
    object.insert("ok".into(), json!(receipt["logicalDeleted"] == true));
    value
}
pub async fn deletion_receipts_route(State(state): State<ServiceState>) -> Response {
    match state.memory().agent_deletion_receipts() {
        Ok(receipts) => Json(json!({"deletions":receipts})).into_response(),
        Err(error) => error.response(),
    }
}
pub async fn deletion_cleanup_route(
    State(state): State<ServiceState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match state.memory().retry_agent_deletion_cleanup(&id) {
        Ok(value) => Json(value).into_response(),
        Err(error) => error.response(),
    }
}

#[cfg(test)]
mod hook_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Barrier,
    };

    fn home(label: &str) -> PathBuf {
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "lingxi-deletion-hook-{}-{}-{label}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn concurrent_homes_keep_their_own_registered_probe() {
        let homes = [home("parallel-a"), home("parallel-b")];
        let calls = [AtomicUsize::new(0), AtomicUsize::new(0)];
        let registered = Barrier::new(2);
        let probed = Barrier::new(2);
        std::thread::scope(|threads| {
            for (root, calls) in homes.iter().zip(calls.iter()) {
                let registered = &registered;
                let probed = &probed;
                threads.spawn(move || {
                    let transaction = root.join("transaction");
                    let expected = transaction.clone();
                    let count = Arc::new(AtomicUsize::new(0));
                    let hook_count = Arc::clone(&count);
                    let _guard = deletion_test_hook(
                        root,
                        Arc::new(move |_, actual| {
                            assert_eq!(actual, expected.as_path());
                            hook_count.fetch_add(1, Ordering::SeqCst);
                        }),
                    );
                    // 两个目录的注册必须同时存活，不能靠调度顺序避开原覆盖问题。
                    registered.wait();
                    probe(root, DeletionPhase::AfterPrepared, &transaction);
                    probe(root, DeletionPhase::AfterMoved, &transaction);
                    calls.store(count.load(Ordering::SeqCst), Ordering::SeqCst);
                    probed.wait();
                });
            }
        });
        assert_eq!(calls[0].load(Ordering::SeqCst), 2);
        assert_eq!(calls[1].load(Ordering::SeqCst), 2);
        let hooks = HOOKS.lock().unwrap();
        assert!(homes.iter().all(|root| !hooks.contains_key(root)));
    }

    #[test]
    fn dropping_older_same_home_guard_never_deletes_newer_or_revives_old_hook() {
        let root = home("drop-older-first");
        let old_calls = Arc::new(AtomicUsize::new(0));
        let newer_calls = Arc::new(AtomicUsize::new(0));
        let old_count = Arc::clone(&old_calls);
        let older = deletion_test_hook(
            &root,
            Arc::new(move |_, _| {
                old_count.fetch_add(1, Ordering::SeqCst);
            }),
        );
        let newer_count = Arc::clone(&newer_calls);
        let newer = deletion_test_hook(
            &root,
            Arc::new(move |_, _| {
                newer_count.fetch_add(1, Ordering::SeqCst);
            }),
        );
        drop(older);
        probe(&root, DeletionPhase::AfterMoved, &root);
        assert_eq!(old_calls.load(Ordering::SeqCst), 0);
        assert_eq!(newer_calls.load(Ordering::SeqCst), 1);
        drop(newer);
        probe(&root, DeletionPhase::AfterMoved, &root);
        assert_eq!(old_calls.load(Ordering::SeqCst), 0);
        assert_eq!(newer_calls.load(Ordering::SeqCst), 1);
        assert!(!HOOKS.lock().unwrap().contains_key(&root));
    }

    #[test]
    fn dropping_newer_same_home_guard_restores_only_still_live_older_hook() {
        let root = home("drop-newer-first");
        let old_calls = Arc::new(AtomicUsize::new(0));
        let old_count = Arc::clone(&old_calls);
        let older = deletion_test_hook(
            &root,
            Arc::new(move |_, _| {
                old_count.fetch_add(1, Ordering::SeqCst);
            }),
        );
        let newer = deletion_test_hook(&root, Arc::new(|_, _| {}));
        probe(&root, DeletionPhase::AfterPrepared, &root);
        assert_eq!(old_calls.load(Ordering::SeqCst), 0);
        drop(newer);
        probe(&root, DeletionPhase::AfterMoved, &root);
        assert_eq!(old_calls.load(Ordering::SeqCst), 1);
        drop(older);
        probe(&root, DeletionPhase::AfterMoved, &root);
        assert_eq!(old_calls.load(Ordering::SeqCst), 1);
        assert!(!HOOKS.lock().unwrap().contains_key(&root));
    }

    #[test]
    fn probe_callback_can_register_and_release_another_hook_outside_registry_lock() {
        let root = home("callback");
        let nested_root = home("nested-callback");
        let calls = Arc::new(AtomicUsize::new(0));
        let hook_calls = Arc::clone(&calls);
        let guard = deletion_test_hook(
            &root,
            Arc::new(move |_, _| {
                let nested_calls = Arc::clone(&hook_calls);
                let nested = deletion_test_hook(
                    &nested_root,
                    Arc::new(move |_, _| {
                        nested_calls.fetch_add(1, Ordering::SeqCst);
                    }),
                );
                probe(&nested_root, DeletionPhase::AfterMoved, &nested_root);
                drop(nested);
            }),
        );
        probe(&root, DeletionPhase::AfterMoved, &root);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        drop(guard);
        assert!(!HOOKS.lock().unwrap().contains_key(&root));
    }
}

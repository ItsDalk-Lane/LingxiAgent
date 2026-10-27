//! 只读旧定时任务文件；身份由真实服务确认，旧历史原文归档而不伪造新 Run。
use crate::automation::{local_owner_principal, prepare_job, JobApi};
use lingxi_adapters::storage::{
    automation::AutomationJobInput, automation_import as store, session_tree,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

#[cfg(unix)]
const MAX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportStatus {
    Imported,
    AlreadyPresent,
    SkippedLegacyCovered,
    Blocked,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRecord {
    pub source: String,
    pub legacy_job_id: Option<String>,
    pub job_id: Option<String>,
    pub status: ImportStatus,
    pub detail: Option<String>,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub imported: usize,
    pub already_present: usize,
    pub blocked: usize,
    pub entries: Vec<ImportRecord>,
}
impl ImportReport {
    fn record(
        &mut self,
        source: &Path,
        legacy: Option<String>,
        job: Option<String>,
        status: ImportStatus,
        detail: Option<String>,
    ) {
        match status {
            ImportStatus::Imported => self.imported += 1,
            ImportStatus::AlreadyPresent => self.already_present += 1,
            ImportStatus::Blocked => self.blocked += 1,
            ImportStatus::SkippedLegacyCovered => {}
        }
        self.entries.push(ImportRecord {
            source: source.display().to_string(),
            legacy_job_id: legacy,
            job_id: job,
            status,
            detail,
        });
    }
}

fn segment(value: &str) -> Result<&str, String> {
    if value.trim().is_empty()
        || value.contains('/')
        || value.contains('\\')
        || value.contains("..")
    {
        Err("unsafe automation path identity".into())
    } else {
        Ok(value)
    }
}
fn digest(bytes: &[u8]) -> String {
    lingxi_kernel::session_tree::sha256_hex(bytes)
}
fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// 固定受控根下的文件，不跟随旧记录或目录链接读取其他材料。
fn read_safe(root: &Path, path: &Path) -> Result<Option<String>, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "legacy automation source outside service home")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err("unsafe legacy automation component".into());
        };
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err("legacy automation source contains symlink".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("legacy automation metadata unavailable: {error}")),
        }
    }
    #[cfg(unix)]
    {
        // 复用现行父目录句柄与逐段无跟随打开；先固定句柄，再读正文和核对身份。
        let snapshot = crate::file_safety::Snapshot::read_bounded(path, MAX_SOURCE_BYTES)
            .map_err(|error| format!("legacy automation safe snapshot failed: {error}"))?;
        if !snapshot.path_is_current(path) {
            return Err("legacy automation source changed during safe snapshot".into());
        }
        String::from_utf8(snapshot.bytes)
            .map(Some)
            .map_err(|_| "legacy automation source is not UTF-8".into())
    }
    #[cfg(not(unix))]
    {
        // 现有 Windows 文件层未关闭打开竞态，不能用路径回退读取旧敏感历史。
        Err("legacy automation import requires a verified no-follow directory handle on this platform".into())
    }
}

struct Source {
    path: PathBuf,
    raw: String,
    document: Value,
    agent: Option<String>,
}
fn read_source(home: &Path, path: &Path, agent: Option<String>) -> Result<Option<Source>, String> {
    let main = read_safe(home, path)?;
    let parse = |raw: &str| -> Result<Value, String> {
        let value: Value = serde_json::from_str(raw)
            .map_err(|_| "legacy cron store JSON is corrupt".to_string())?;
        if !value.is_object() || !value.get("jobs").is_some_and(Value::is_array) {
            return Err("legacy cron store must contain a jobs array".into());
        }
        Ok(value)
    };
    let mut primary_error = None;
    if let Some(raw) = main {
        match parse(&raw) {
            Ok(document) => {
                return Ok(Some(Source {
                    path: path.into(),
                    raw,
                    document,
                    agent,
                }))
            }
            Err(error) => primary_error = Some(error),
        }
    }
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    if let Some(raw) = read_safe(home, &tmp)? {
        let document = parse(&raw)?;
        return Ok(Some(Source {
            path: tmp,
            raw,
            document,
            agent,
        }));
    }
    match primary_error {
        Some(error) => Err(error),
        None => Ok(None),
    }
}

fn legacy_ref(job: &Value) -> Option<(String, String)> {
    Some((
        text(job.get("legacyRef")?, "agentId")?.into(),
        text(job.get("legacyRef")?, "jobId")?.into(),
    ))
}
fn timestamp(value: &Value, key: &str) -> Result<Option<i64>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => chrono::DateTime::parse_from_rfc3339(value)
            .map(|value| Some(value.timestamp_millis()))
            .map_err(|_| format!("legacy {key} is not an unambiguous ISO timestamp")),
        _ => Err(format!("legacy {key} must be an ISO timestamp or null")),
    }
}

fn normalize_executor(config: &mut Value) -> Result<(), String> {
    let old = config.get("executor").cloned().unwrap_or(Value::Null);
    let kind = old
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("agent_session");
    let prompt = match kind {
        "agent_session" => text(config, "prompt")
            .or_else(|| text(&old, "prompt"))
            .unwrap_or("")
            .to_string(),
        "direct_action" if old["action"] == "notify" => {
            let params = old.get("params").cloned().unwrap_or_else(|| json!({}));
            let mut payload = json!({"title":params["title"].as_str().unwrap_or(""),"body":params["body"].as_str().unwrap_or("")});
            for key in ["channels", "bridgePlatforms", "contextPolicy", "audience"] {
                if let Some(value) = params.get(key) {
                    payload[key] = value.clone();
                }
            }
            format!("这是一个自动化 Agent Run。触发后只完成下面的固定通知动作，不要扩展成其他任务。\n\n请调用 notify 工具发送通知，参数如下：\n{payload}\n\n发送完成后，用一句话说明发送结果；如果 notify 工具不可用或发送失败，明确说明失败原因。")
        }
        "plugin_action" => {
            let plugin = text(&old, "pluginId").ok_or("legacy plugin action has no pluginId")?;
            let action = text(&old, "actionId").ok_or("legacy plugin action has no actionId")?;
            let params = old
                .get("params")
                .filter(|value| value.is_object())
                .cloned()
                .unwrap_or_else(|| json!({}));
            format!("这是一个自动化 Agent Run。触发后只完成下面的插件动作，不要扩展成其他任务。\n\n插件动作：{plugin}/{action}\n参数：\n{params}\n\n请调用对应插件工具执行这个动作。若工具不可用，明确说明失败原因。")
        }
        _ => return Err("unsupported legacy automation executor".into()),
    };
    config["prompt"] = json!(prompt);
    if config.get("model").is_none() {
        config["model"] = old.get("model").cloned().unwrap_or_else(|| json!(""));
    }
    if config.get("executionContext").is_none() {
        config["executionContext"] = old
            .get("executionContext")
            .cloned()
            .unwrap_or_else(|| json!({}));
    }
    let mut normalized = if kind == "agent_session" && old.is_object() {
        old.clone()
    } else {
        json!({})
    };
    normalized["kind"] = json!("agent_session");
    for (target, source) in [
        ("agentId", "actorAgentId"),
        ("prompt", "prompt"),
        ("model", "model"),
        ("executionContext", "executionContext"),
    ] {
        normalized[target] = config[source].clone();
    }
    config["executor"] = normalized;
    Ok(())
}

async fn prepare_import(
    api: &JobApi,
    home: &Path,
    source: &Source,
    raw: &Value,
) -> Result<(AutomationJobInput, u64, i64, i64, String), String> {
    let old_id = text(raw, "id").ok_or("legacy job id missing")?;
    segment(old_id)?;
    let actor = source
        .agent
        .as_deref()
        .or_else(|| text(raw, "actorAgentId"))
        .or_else(|| {
            raw.get("legacyRef")
                .and_then(|value| text(value, "agentId"))
        })
        .or_else(|| raw.get("executor").and_then(|value| text(value, "agentId")))
        .ok_or("legacy actor cannot be resolved")?
        .to_string();
    segment(&actor)?;
    let roots = api
        .memory
        .roots(&actor)
        .map_err(|error| error.to_string())?;
    if !roots.agent_dir.is_dir()
        || api
            .memory
            .is_agent_deleted_checked(&actor)
            .map_err(|error| error.to_string())?
    {
        return Err("legacy actor no longer exists".into());
    }
    let mut config = raw.clone();
    if !config.is_object() {
        return Err("legacy job must be object".into());
    }
    if let Some(version) = config.get("schemaVersion") {
        if version
            .as_u64()
            .is_none_or(|version| version > 4 || version == 0)
        {
            return Err("unsupported legacy automation schema".into());
        }
    }
    config["schemaVersion"] = json!(4);
    config["actorAgentId"] = json!(actor);
    if let Some(agent) = &source.agent {
        config["legacyRef"] = json!({"agentId":agent,"jobId":old_id});
    }
    normalize_executor(&mut config)?;
    let mut context = config["executionContext"].clone();
    if context.is_null() {
        context = json!({});
    }
    if !context.is_object() {
        return Err("legacy execution context is malformed".into());
    }
    if let Some(id) = text(&context, "sourceSessionId") {
        let session = session_tree::get_session_row(&api.storage, id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or("legacy source session is unavailable")?;
        if session.agent_id != actor
            || session.owner_user_id != lingxi_kernel::LOCAL_OWNER_SUBJECT
            || session.lifecycle != "active"
        {
            return Err("legacy source session identity is no longer authorized".into());
        }
    } else if text(&context, "sourceSessionPath").is_some()
        || text(&context, "sourceBridgeSessionKey").is_some()
    {
        return Err("legacy source has only an unauthenticated path or Bridge locator".into());
    }
    // 保存的路径只是旧定位信息，不能成为新内核的文件授权。
    for key in ["cwd", "sourceSessionPath"] {
        context[key] = Value::Null;
    }
    for key in ["workspaceFolders", "authorizedFolders"] {
        context[key] = json!([]);
    }
    config["executionContext"] = context.clone();
    config["executor"]["executionContext"] = context;
    for key in [
        "authorization",
        "requestedGrants",
        "permissionMode",
        "approvalPolicy",
        "allowHumanApproval",
    ] {
        config.as_object_mut().unwrap().remove(key);
    }
    let enabled = match config.get("enabled") {
        None => true,
        Some(Value::Bool(value)) => *value,
        _ => return Err("legacy enabled value is malformed".into()),
    };
    if enabled && text(&config, "prompt").is_none() {
        return Err("enabled legacy automation has no prompt".into());
    }
    let mut repaired_every = false;
    if config["type"] == "every" {
        let interval = config["schedule"]
            .as_u64()
            .or_else(|| {
                config["schedule"]
                    .as_str()
                    .and_then(|value| value.parse().ok())
            })
            .ok_or("legacy every interval is malformed")?;
        let decoded = interval / 60_000;
        let repair = interval >= 3_600_000_000
            && interval.is_multiple_of(3_600_000_000)
            && (60_000..=366 * 24 * 60 * 60 * 1000).contains(&decoded);
        config["schedule"] = json!(if repair {
            repaired_every = true;
            decoded
        } else {
            interval.max(60_000)
        });
    }
    let now = i64::try_from(api.clock.now_unix_ms())
        .map_err(|_| "automation clock outside SQLite range")?;
    let next = if !enabled {
        None
    } else if repaired_every {
        crate::automation_schedule::next_run(
            text(&config, "type").ok_or("legacy type missing")?,
            &config["schedule"],
            api.clock.now_unix_ms(),
        )?
        .map(|value| value as i64)
    } else {
        match timestamp(raw, "nextRunAt")? {
            Some(value) => Some(value),
            None => crate::automation_schedule::next_run(
                text(&config, "type").ok_or("legacy type missing")?,
                &config["schedule"],
                api.clock.now_unix_ms(),
            )?
            .map(|value| value as i64),
        }
    };
    let history_path = source
        .path
        .parent()
        .ok_or("legacy source has no parent")?
        .join("cron-runs")
        .join(format!("{old_id}.jsonl"));
    let history = read_safe(home, &history_path)?.unwrap_or_default();
    for (index, line) in history.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let item: Value = serde_json::from_str(line)
            .map_err(|_| format!("legacy history line {} is corrupt", index + 1))?;
        if !item.is_object() {
            return Err(format!(
                "legacy history line {} is not an object",
                index + 1
            ));
        }
        if matches!(
            text(&item, "status"),
            Some("running" | "started" | "unknown")
        ) {
            return Err(
                "legacy history contains an unconfirmed external result; automatic replay blocked"
                    .into(),
            );
        }
    }
    let source_key =
        serde_json::to_string(&(api.studio_id.as_str(), source.agent.as_deref(), old_id))
            .map_err(|error| error.to_string())?;
    let id = if source.agent.is_some() {
        format!("legacy-job-{}", digest(source_key.as_bytes()))
    } else {
        old_id.into()
    };
    let metadata = json!({"sourceKey":source_key,"sourceDigest":digest(source.raw.as_bytes()),"sourceJob":raw,"historyJsonl":history,"historyDigest":digest(history.as_bytes()),"documentMetadata":source.document.as_object().unwrap().iter().filter(|(key,_)|key.as_str()!="jobs").map(|(key,value)|(key.clone(),value.clone())).collect::<serde_json::Map<_,_>>()});
    if config
        .get("legacyImport")
        .is_some_and(|value| !value.is_object())
        || config
            .get("legacyImport")
            .and_then(|value| value.get("rustR07"))
            .is_some()
    {
        return Err("legacy import metadata name is already occupied".into());
    }
    if config.get("legacyImport").is_none() {
        config["legacyImport"] = json!({});
    }
    config["legacyImport"]["rustR07"] = metadata;
    // 旧 at 游标可以在过去；只绕过“新建必须未来”的规则，不恢复旧目录授权。
    config["enabled"] = json!(false);
    let mut prepared = prepare_job(api, &local_owner_principal(), config, None).await?;
    let mut normalized: Value =
        serde_json::from_str(&prepared.config_json).map_err(|error| error.to_string())?;
    normalized["enabled"] = json!(enabled);
    prepared.id = id;
    prepared.config_json = normalized.to_string();
    prepared.next_run_at = next;
    let revision = raw
        .get("configRevision")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0 && *value <= i64::MAX as u64)
        .unwrap_or(1);
    let created = timestamp(raw, "createdAt")?.unwrap_or(now);
    let updated = timestamp(raw, "lastRunAt")?.unwrap_or(created);
    Ok((prepared, revision, created, updated, source_key))
}

async fn import_source(
    api: &JobApi,
    home: &Path,
    source: Source,
    covered: &mut BTreeSet<(String, String)>,
    report: &mut ImportReport,
) {
    if source.agent.is_some()
        && source
            .document
            .get("studioCronMigration")
            .is_some_and(|marker| marker["version"] != 1 || marker["status"] != "imported")
    {
        report.record(
            &source.path,
            None,
            None,
            ImportStatus::Blocked,
            Some("legacy Studio migration marker has an unknown or incomplete state".into()),
        );
        return;
    }
    if source.agent.is_some()
        && source
            .document
            .get("studioCronMigration")
            .is_some_and(|marker| marker["version"] == 1 && marker["status"] == "imported")
    {
        report.record(
            &source.path,
            None,
            None,
            ImportStatus::SkippedLegacyCovered,
            Some("legacy store already transferred by the prior Studio migration".into()),
        );
        return;
    }
    let mut counts = BTreeMap::new();
    for raw in source.document["jobs"].as_array().unwrap() {
        if let Some(id) = text(raw, "id") {
            *counts.entry(id.to_string()).or_insert(0_usize) += 1;
        }
    }
    for raw in source.document["jobs"].as_array().unwrap() {
        let old_id = text(raw, "id").map(str::to_owned);
        if let (Some(agent), Some(id)) = (&source.agent, &old_id) {
            if covered.contains(&(agent.clone(), id.clone())) {
                report.record(
                    &source.path,
                    old_id,
                    None,
                    ImportStatus::SkippedLegacyCovered,
                    None,
                );
                continue;
            }
        }
        if old_id
            .as_ref()
            .is_some_and(|id| counts.get(id).copied().unwrap_or(0) > 1)
        {
            report.record(
                &source.path,
                old_id,
                None,
                ImportStatus::Blocked,
                Some("duplicate legacy job id in one source".into()),
            );
            continue;
        }
        match prepare_import(api, home, &source, raw).await {
            Ok((prepared, revision, created, updated, key)) => {
                match store::import_job(&api.storage, &prepared, revision, created, updated).await {
                    Ok(result) => {
                        let existing: Value = match serde_json::from_str(&result.job.config_json) {
                            Ok(value) => value,
                            Err(error) => {
                                report.record(
                                    &source.path,
                                    old_id,
                                    Some(prepared.id),
                                    ImportStatus::Blocked,
                                    Some(format!("existing imported config is corrupt: {error}")),
                                );
                                continue;
                            }
                        };
                        if !result.imported
                            && (result.job.studio_id != api.studio_id
                                || result.job.owner_kind != "local_user"
                                || result.job.owner_subject != lingxi_kernel::LOCAL_OWNER_SUBJECT
                                || existing["legacyImport"]["rustR07"]["sourceKey"].as_str()
                                    != Some(&key))
                        {
                            report.record(&source.path,old_id,Some(prepared.id),ImportStatus::Blocked,Some("stable job id is occupied by a different record; nothing overwritten".into()));
                            continue;
                        }
                        if let Some(reference) = legacy_ref(raw) {
                            covered.insert(reference);
                        }
                        let detail = if !result.imported
                            && existing["legacyImport"]["rustR07"]["sourceDigest"]
                                != json!(digest(source.raw.as_bytes()))
                        {
                            Some("legacy source changed after initial import; existing Rust record and archived history kept without overwrite".into())
                        } else if source.path.extension().is_some_and(|value| value == "tmp") {
                            Some(
                                "read legacy recovery .tmp without modifying either source file"
                                    .into(),
                            )
                        } else {
                            None
                        };
                        report.record(
                            &source.path,
                            old_id,
                            Some(prepared.id),
                            if result.imported {
                                ImportStatus::Imported
                            } else {
                                ImportStatus::AlreadyPresent
                            },
                            detail,
                        );
                    }
                    Err(error) => report.record(
                        &source.path,
                        old_id,
                        Some(prepared.id),
                        ImportStatus::Blocked,
                        Some(error.to_string()),
                    ),
                }
            }
            Err(error) => report.record(
                &source.path,
                old_id,
                None,
                ImportStatus::Blocked,
                Some(error),
            ),
        }
    }
}

/// 调用方在调度器启动前消费逐条报告；有阻塞时不能把该记录声称已迁移。
pub async fn import_current_studio(api: &JobApi) -> ImportReport {
    let mut report = ImportReport::default();
    let home = match api.memory.home().canonicalize() {
        Ok(value) => value,
        Err(error) => {
            report.record(
                api.memory.home(),
                None,
                None,
                ImportStatus::Blocked,
                Some(error.to_string()),
            );
            return report;
        }
    };
    if let Err(error) = segment(&api.studio_id) {
        report.record(&home, None, None, ImportStatus::Blocked, Some(error));
        return report;
    }
    let mut covered = BTreeSet::new();
    let path = home
        .join("studios")
        .join(&api.studio_id)
        .join("desk/cron-jobs.json");
    match read_source(&home, &path, None) {
        Ok(Some(source)) => import_source(api, &home, source, &mut covered, &mut report).await,
        Ok(None) => {}
        Err(error) => report.record(&path, None, None, ImportStatus::Blocked, Some(error)),
    }
    let agents = home.join("agents");
    let entries = match std::fs::read_dir(&agents) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return report,
        Err(error) => {
            report.record(
                &agents,
                None,
                None,
                ImportStatus::Blocked,
                Some(error.to_string()),
            );
            return report;
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => paths.push(entry.path()),
            Err(error) => report.record(
                &agents,
                None,
                None,
                ImportStatus::Blocked,
                Some(error.to_string()),
            ),
        }
    }
    paths.sort();
    for agent_path in paths {
        let agent = match agent_path.file_name().and_then(|value| value.to_str()) {
            Some(agent) => agent.to_owned(),
            None => {
                report.record(
                    &agent_path,
                    None,
                    None,
                    ImportStatus::Blocked,
                    Some("legacy agent identity is not UTF-8".into()),
                );
                continue;
            }
        };
        let metadata = match std::fs::symlink_metadata(&agent_path) {
            Ok(value) => value,
            Err(error) => {
                report.record(
                    &agent_path,
                    None,
                    None,
                    ImportStatus::Blocked,
                    Some(error.to_string()),
                );
                continue;
            }
        };
        if metadata.file_type().is_symlink() {
            report.record(
                &agent_path,
                None,
                None,
                ImportStatus::Blocked,
                Some("legacy agent directory is a symlink".into()),
            );
            continue;
        }
        if !metadata.is_dir() {
            continue;
        }
        let path = agent_path.join("desk/cron-jobs.json");
        match read_source(&home, &path, Some(agent)) {
            Ok(Some(source)) => import_source(api, &home, source, &mut covered, &mut report).await,
            Ok(None) => {}
            Err(error) => report.record(&path, None, None, ImportStatus::Blocked, Some(error)),
        }
    }
    report
}

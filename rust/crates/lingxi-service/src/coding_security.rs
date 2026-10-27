//! 安全扫描的权限、快照和审计由 Rust 持有，规则沿用现有纯计算模块。
use crate::{
    inject::ServiceClock,
    resourceaccess::{ResourceAccess, ResourceOp},
    resourceio::{OpContext, ResourceIoService},
    sandbox::SandboxPort,
    toolgateway::{self, ToolInvocationGateway},
};
use lingxi_kernel::{
    invocation::ToolRecoveryCapability,
    ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess},
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin, ToolRegistry,
    },
    RunContext,
};
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "dist",
    "dist-sandbox",
    "build",
    "out",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    "target",
];
const SKIP_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "ico", "pdf", "zip", "gz", "tgz", "tar", "mp4", "mp3",
    "wav", "woff", "woff2", "ttf", "eot", "dylib", "so", "dll", "exe", "bin", "wasm",
];
fn executable(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|p| p.join(name))
        .find(|p| p.is_file())
}
struct SnapshotRoot(PathBuf);
impl SnapshotRoot {
    fn create() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "lingxi-security-{}",
            crate::auth::hex_random_public(16).map_err(|e| e.to_string())?
        ));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path).map_err(|e| e.to_string())?;
        Ok(Self(path.canonicalize().map_err(|e| e.to_string())?))
    }
}
impl Drop for SnapshotRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Executor {
    access: Arc<ResourceAccess>,
    cwd: PathBuf,
    home: PathBuf,
    io: Arc<ResourceIoService>,
    sandbox: Option<Arc<dyn SandboxPort>>,
    clock: Arc<dyn ServiceClock>,
}
impl Executor {
    fn authorize(&self, ctx: &RunContext, raw: &Path, cwd: &Path) -> Result<PathBuf, String> {
        self.access
            .authorize(
                ctx.principal.storage_kind(),
                &ctx.principal.storage_subject(),
                ctx.session_id.as_str(),
                raw,
                cwd,
                ResourceOp::Read,
            )
            .map(|s| s.path)
            .map_err(|e| format!("SECURITY_SCAN_OUT_OF_SCOPE: {e}"))
    }
    async fn helper(&self, input: Value, cwd: &Path, ctx: &RunContext) -> Result<Value, String> {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../scripts/rust-tauri/r08-security-worker.mjs");
        crate::coding_ast_lsp::run_snapshot_worker(&script, input, cwd, &self.sandbox, ctx).await
    }
    async fn run(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        args: &Value,
    ) -> Result<ToolOutcome, String> {
        toolgateway::trusted_settings_context(ctx, call).map_err(|e| e.to_string())?;
        let cwd = toolgateway::workspace().unwrap_or_else(|| self.cwd.clone());
        let mode = args["mode"].as_str().unwrap_or("changes");
        let cap = args["max_files"].as_u64().unwrap_or(400).min(2000) as usize;
        let mut note = None;
        let mut paths = Vec::new();
        let target = self.authorize(
            ctx,
            Path::new(if mode == "path" {
                args["path"].as_str().unwrap_or(".")
            } else {
                "."
            }),
            &cwd,
        )?;
        toolgateway::require_bound_resource(&target, ResourceOp::Read)
            .map_err(|e| e.to_string())?;
        let mut capped = false;
        if mode == "changes" {
            // Git 向上发现仓库、gitdir 外指和可执行的配置会超出已授权根。
            // 仅接受当前根内的实体 .git；其余明确建议 path，不枚举父仓库。
            let git = target.join(".git");
            let git_kind = std::fs::symlink_metadata(&git);
            if !matches!(git_kind, Ok(ref metadata) if metadata.is_dir() && !metadata.file_type().is_symlink())
            {
                return Err(
                    "SECURITY_GIT_SCOPE_UNAVAILABLE: 当前授权根不是独立 Git 根；请使用 mode=path"
                        .into(),
                );
            }
            self.authorize(ctx, &git, &cwd)?;
            let git_binary = executable("git").ok_or("SECURITY_GIT_MISSING: 请使用 mode=path")?;
            let list = self
                .helper(
                    json!({"op":"changes","cwd":target,"git":git_binary}),
                    &cwd,
                    ctx,
                )
                .await?;
            note = list["note"].as_str().map(str::to_owned);
            for raw in list["files"]
                .as_array()
                .ok_or("SECURITY_CHANGE_LIST_INVALID")?
            {
                let path = Path::new(raw.as_str().ok_or("SECURITY_CHANGE_PATH_INVALID")?);
                if path.is_absolute()
                    || path
                        .components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    return Err("SECURITY_CHANGE_PATH_INVALID".into());
                }
                if paths.len() == cap {
                    capped = true;
                    break;
                }
                paths.push(cwd.join(path));
            }
        } else if target.is_file() {
            paths.push(target.clone());
        } else {
            let mut pending = vec![target.clone()];
            let mut visited = 0;
            while let Some(directory) = pending.pop() {
                let authorized = self.authorize(ctx, &directory, &cwd)?;
                let mut entries = Vec::new();
                for entry in std::fs::read_dir(&authorized)
                    .map_err(|e| format!("SECURITY_DIRECTORY_UNREADABLE: {e}"))?
                {
                    visited += 1;
                    if visited > 20000 {
                        return Err("SECURITY_ENUMERATION_LIMIT: 请缩小扫描目录".into());
                    }
                    if toolgateway::TOOL_LIFETIME
                        .try_with(|life| life.cancel.is_cancelled())
                        .unwrap_or(true)
                    {
                        return Err("SECURITY_CANCELLED".into());
                    }
                    entries.push(entry.map_err(|e| e.to_string())?);
                }
                entries.sort_by_key(|entry| entry.file_name());
                for entry in entries {
                    let kind = entry.file_type().map_err(|e| e.to_string())?;
                    if kind.is_symlink() {
                        continue;
                    }
                    if kind.is_dir() {
                        let name = entry.file_name();
                        let name = name.to_string_lossy();
                        if !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_ref()) {
                            pending.push(entry.path());
                        }
                    } else if kind.is_file() {
                        let ext = entry
                            .path()
                            .extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_lowercase();
                        if SKIP_EXTS.contains(&ext.as_str()) {
                            continue;
                        }
                        if paths.len() == cap {
                            capped = true;
                            break;
                        }
                        paths.push(entry.path());
                    }
                }
                if capped {
                    break;
                }
            }
        }
        paths.sort();
        paths.dedup();
        let root = SnapshotRoot::create()?;
        let sources = root.0.join("sources");
        let reports = root.0.join("reports");
        std::fs::create_dir(&sources).map_err(|e| e.to_string())?;
        std::fs::create_dir(&reports).map_err(|e| e.to_string())?;
        let op = OpContext {
            principal_kind: ctx.principal.storage_kind(),
            principal_subject: ctx.principal.storage_subject(),
            session_id: ctx.session_id.to_string(),
            source: "security_scan".into(),
            reason: Some("只读代码扫描".into()),
            session_path: None,
            emit: false,
            capture_origin: None,
            engine_plane: false,
        };
        let mut files = Vec::new();
        let mut skipped = Vec::new();
        let mut total_bytes = 0usize;
        for path in paths {
            if toolgateway::TOOL_LIFETIME
                .try_with(|life| life.cancel.is_cancelled())
                .unwrap_or(true)
            {
                return Err("SECURITY_CANCELLED".into());
            }
            if !path.exists() {
                skipped.push(json!({"path":path.strip_prefix(&cwd).unwrap_or(&path),"reason":"deleted_or_missing"}));
                continue;
            }
            let canonical = self.authorize(ctx, &path, &cwd)?;
            let metadata = std::fs::metadata(&canonical).map_err(|e| e.to_string())?;
            let label = canonical
                .strip_prefix(&cwd)
                .unwrap_or(&canonical)
                .to_string_lossy()
                .replace('\\', "/");
            if metadata.len() > 4 * 1024 * 1024 {
                skipped.push(json!({"path":label,"reason":"oversized"}));
                continue;
            }
            let snapshot = self
                .io
                .read_artifact_snapshot(&json!({"kind":"local-file","path":canonical}), &op)
                .await
                .map_err(|e| format!("{}: {}", e.code, e.message))?;
            total_bytes += snapshot.bytes.len();
            if total_bytes > 32 * 1024 * 1024 {
                return Err("SECURITY_SNAPSHOT_LIMIT: 请缩小扫描范围".into());
            }
            let text = match std::str::from_utf8(&snapshot.bytes) {
                Ok(text) => text,
                Err(_) => {
                    skipped.push(json!({"path":label,"reason":"binary"}));
                    continue;
                }
            };
            // 外部扫描器只见编号副本；原始绝对路径不会作为临时文件名。
            let name = format!(
                "{:05}/{}",
                files.len(),
                canonical
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("source")
            );
            let staged = sources.join(&name);
            std::fs::create_dir_all(staged.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(staged, &snapshot.bytes).map_err(|e| e.to_string())?;
            files.push(json!({"path":name,"displayPath":label,"content":text}));
        }
        let engines = json!({"semgrep":executable("semgrep"),"gitleaks":executable("gitleaks")});
        let mut result = self.helper(json!({"op":"scan","files":files,"engines":engines,"snapshotRoot":sources,"reportRoot":reports}), &cwd, ctx).await?;
        if let Some(findings) = result["findings"].as_array_mut() {
            for finding in findings {
                let name = finding["file"].as_str().ok_or("SECURITY_FINDING_INVALID")?;
                let file = files
                    .iter()
                    .find(|file| file["path"] == name)
                    .ok_or("SECURITY_FINDING_OUT_OF_SCOPE")?;
                finding["file"] = file["displayPath"].clone();
            }
        }
        result["mode"] = json!(mode);
        result["skippedFiles"] = json!(skipped);
        result["fileCapReached"] = json!(capped);
        result["note"] = json!(note);
        let audit = json!({"action":"security_scan","target":ctx.session_id.as_str(),"atUnixMs":self.clock.now_unix_ms(),"metadata":{"mode":mode,"engines":result["engines"],"counts":result["counts"],"filesScanned":result["filesScanned"],"fileCapReached":capped},"actor":{"kind":ctx.principal.storage_kind(),"principalId":ctx.principal.storage_subject()}});
        match crate::security_audit::project(
            &self.home,
            &format!("{}:{}", ctx.run_id, call),
            &[audit],
        ) {
            Ok(()) => result["auditStatus"] = json!("committed"),
            Err(_) => result["auditStatus"] = json!("failed"),
        }
        let mut lines = vec![format!(
            "security scan — {} critical, {} warning, {} info",
            result["counts"]["critical"], result["counts"]["warning"], result["counts"]["info"]
        )];
        if let Some(note) = note {
            lines.push(note);
        }
        if capped {
            lines.push(format!("file cap {cap} reached — narrow the path"));
        }
        lines.push(format!(
            "{} file(s) scanned; {} baseline skip(s), {} unreadable/deleted/oversized skip(s)",
            result["filesScanned"],
            result["filesSkipped"],
            skipped.len()
        ));
        if result["findingCount"].as_u64().unwrap_or(0) > 50 {
            lines.push(format!(
                "{} more findings — narrow the scan scope",
                result["findingCount"].as_u64().unwrap() - 50
            ));
        }
        for engine in ["semgrep", "gitleaks"] {
            lines.push(format!(
                "{engine}: {}",
                result["external"][engine]["status"]
                    .as_str()
                    .unwrap_or("unavailable")
            ));
        }
        for finding in result["findings"]
            .as_array()
            .ok_or("SECURITY_FINDINGS_INVALID")?
        {
            lines.push(format!(
                "[{}] {}:{} — {}",
                finding["severity"].as_str().unwrap_or("warning"),
                finding["file"].as_str().unwrap_or(""),
                finding["line"],
                finding["ruleId"].as_str().unwrap_or("")
            ));
        }
        if result["auditStatus"] == "failed" {
            lines.push("审计写入失败，扫描结果保留；本次不能记为审计完整通过".into());
        }
        let mut success = ToolSuccess::text(lines.join("\n"));
        success.details = result.as_object().cloned();
        Ok(ToolOutcome::Success { result: success })
    }
}
impl ToolExecutorPort for Executor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let outcome = self
                .run(ctx, call, request.arguments.as_value())
                .await
                .unwrap_or_else(|message| ToolOutcome::Failed {
                    error: ProtocolError::new(ErrorCode::InvalidMessage, message, false),
                });
            ToolExecutionResult::of_ctx(ctx, outcome)
        })
    }
}
#[allow(clippy::too_many_arguments)]
pub fn register(
    registry: &ToolRegistry,
    gateway: &ToolInvocationGateway,
    access: Arc<ResourceAccess>,
    cwd: PathBuf,
    home: PathBuf,
    io: Arc<ResourceIoService>,
    sandbox: Option<Arc<dyn SandboxPort>>,
    clock: Arc<dyn ServiceClock>,
    budget: &SchemaBudget,
) -> Result<(), String> {
    let receipt=registry.register(ToolManifest { origin:ToolOrigin::FirstParty,local_name:"security_scan".into(),display_name:"security_scan".into(),aliases:vec![],version:"1.0.0".into(),description:"Read-only security scan of git changes or an authorized file/directory. Baseline rules always run; installed semgrep/gitleaks scan isolated authorized snapshots. Findings contain paths, lines and rule IDs, never secret bodies. Missing or failed scanners and skipped files are explicit.".into(),input_schema:ToolSchemaDocument{dialect:"json-schema/2020-12".into(),schema:json!({"type":"object","properties":{"mode":{"type":"string","enum":["changes","path"]},"path":{"type":"string","minLength":1},"max_files":{"type":"integer","minimum":1,"maximum":2000}},"additionalProperties":false})},output_schema:None,permission:PermissionContract{kind:PermissionKind::Read,capability_base:"security_scan.scan".into()},availability:Availability::Available,timeout_ms:Some(60_000),max_concurrency:Some(2),declared_permission:DeclaredPermission::ReadOnly,recovery:ToolRecoveryCapability::CONSERVATIVE },budget).map_err(|e|e.to_string())?;
    let roots = access.clone();
    let base = cwd.clone();
    let extractor: toolgateway::ResourceExtractor = Arc::new(move |input, args| {
        let cwd = toolgateway::workspace().unwrap_or_else(|| base.clone());
        let raw = if args.as_value()["mode"] == "path" {
            args.as_value()["path"].as_str().unwrap_or(".")
        } else {
            "."
        };
        roots
            .authorize(
                input.principal_kind,
                &input.principal_subject,
                &input.session_id,
                Path::new(raw),
                &cwd,
                ResourceOp::Read,
            )
            .map(|s| vec![s])
            .map_err(|e| e.to_string())
    });
    gateway.bind_executor_with_resources(
        receipt.target_id,
        Arc::new(Executor {
            access,
            cwd,
            home,
            io,
            sandbox,
            clock,
        }),
        Some(extractor),
        "真实授权快照、同源基线规则、隔离外部扫描与无正文审计",
    );
    Ok(())
}

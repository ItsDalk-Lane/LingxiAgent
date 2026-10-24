//! AST/LSP 真解析器适配。外围只处理 Rust 授权快照，写回复用 ResourceIO 版本事实与收据。
use crate::{
    resourceaccess::{ResourceAccess, ResourceOp},
    resourceio::{ArtifactSnapshot, OpContext, ResourceIoService},
    sandbox::{SandboxCommandRequest, SandboxNetworkRequest, SandboxPort},
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
    collections::BTreeMap,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
const SNAPSHOT_LIMIT: usize = 32 * 1024 * 1024;
fn failure(message: impl Into<String>) -> ToolOutcome {
    ToolOutcome::Failed {
        error: ProtocolError::new(ErrorCode::InvalidMessage, message.into(), false),
    }
}
fn success(text: String, details: Value) -> ToolOutcome {
    let mut result = ToolSuccess::text(text);
    result.truncated = details["truncated"].as_bool().unwrap_or(false);
    result.details = details.as_object().cloned();
    ToolOutcome::Success { result }
}
fn executable(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|p| p.join(name))
        .find(|p| p.is_file())
}
fn op(ctx: &RunContext) -> OpContext {
    OpContext {
        principal_kind: ctx.principal.storage_kind(),
        principal_subject: ctx.principal.storage_subject(),
        session_id: ctx.session_id.to_string(),
        source: "lsp".into(),
        reason: Some("LSP rename".into()),
        session_path: None,
        emit: true,
        capture_origin: None,
        engine_plane: false,
    }
}
fn glob(pattern: &str) -> Result<regex::Regex, String> {
    let mut out = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    if chars.peek() == Some(&'/') {
                        chars.next();
                        out.push_str("(?:.*/)?");
                    } else {
                        out.push_str(".*");
                    }
                } else {
                    out.push_str("[^/]*");
                }
            }
            '?' => out.push_str("[^/]"),
            _ => out.push_str(&regex::escape(&c.to_string())),
        }
    }
    out.push('$');
    regex::Regex::new(&out).map_err(|e| e.to_string())
}
struct OwnedChild {
    child: Option<tokio::process::Child>,
    pid: Option<u32>,
    permit: Option<Arc<lingxi_kernel::session_admission::SessionPermit>>,
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            unsafe {
                libc::killpg(pid as i32, libc::SIGKILL);
            }
        }
        let _ = child.start_kill();
        // 即使上层直接丢弃执行 future，也保留真实会话许可直到进程回收。
        let permit = self.permit.take();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _permit = permit;
                if !matches!(tokio::time::timeout(Duration::from_secs(2),child.wait()).await,Ok(Ok(_))){
                    tracing::error!("CODING_CLEANUP_UNCONFIRMED: dropped worker did not reap in time; retaining session permit");
                    loop {match child.wait().await{Ok(_)=>break,Err(error)=>{tracing::error!("CODING_CLEANUP_UNCONFIRMED: {error}");tokio::time::sleep(Duration::from_millis(100)).await;}}}
                }
            });
        }
    }
}
// 临时副本归 Rust 所有：取消杀树后也会清理，不依赖被杀 worker 的 finally。
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("coding scratch cleanup failed: {error}");
            }
        }
    }
}
pub(crate) async fn run_snapshot_worker(
    script: &Path,
    mut input: Value,
    cwd: &Path,
    sandbox: &Option<Arc<dyn SandboxPort>>,
    ctx: &RunContext,
) -> Result<Value, String> {
    let permit = crate::task_supervisor::retain_blocking_run_permit(ctx)
        .map_err(|_| "CODING_PERMIT_MISMATCH")?;
    let cancel = crate::task_supervisor::blocking_run_cancel_scope(ctx)
        .map_err(|_| "CODING_CANCEL_SCOPE_MISMATCH")?;
    let node = executable("node").ok_or("NODE_MISSING: 请安装 Node.js 24")?;
    let scratch = Scratch(std::env::temp_dir().join(format!(
        "lingxi-coding-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    )));
    std::fs::create_dir(&scratch.0).map_err(|e| format!("CODING_SCRATCH_FAILED: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&scratch.0, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    input
        .as_object_mut()
        .ok_or("CODING_INPUT_INVALID")?
        .insert("_scratch".into(), json!(scratch.0));

    // 领头在脚本结束后仍保持存活。Host 先杀自有整组再 wait；既清理孙进程，
    // 又不会在回收领头之后使用可能已复用的 PID。
    const SUPERVISOR: &str = r#"const {spawn}=require('node:child_process');
const child=spawn(process.execPath,[process.argv[1]],{stdio:['pipe','pipe','ignore']});
const hold=setInterval(()=>{},1000);let finished=false;
const finish=(code)=>{if(finished)return;finished=true;process.stdout.write('\nLX_SNAPSHOT_WORKER_EXIT:'+code+'\n');};
process.stdin.pipe(child.stdin);child.stdin.on('error',()=>{});
child.stdout.on('data',b=>process.stdout.write(b));
child.on('error',e=>{process.stdout.write(JSON.stringify({error:'worker spawn failed: '+e.message}));finish(70);});
child.on('exit',(code,signal)=>finish(code===null?128:code));"#;
    let mut argv = vec![
        node.to_string_lossy().into_owned(),
        "-e".into(),
        SUPERVISOR.into(),
        script.to_string_lossy().into_owned(),
    ];
    if let Some(port) = sandbox {
        argv = port
            .wrap(SandboxCommandRequest {
                argv,
                network: SandboxNetworkRequest::Contained,
                cwd: Some(cwd.into()),
            })
            .map_err(|e| format!("CODING_SANDBOX_REFUSED: {e}"))?
            .argv;
    }
    let mut command = tokio::process::Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(cwd)
        .env_clear()
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    for name in ["PATH", "LANG", "LC_ALL", "LC_CTYPE", "TMPDIR", "TZ"] {
        if let Some(v) = std::env::var_os(name) {
            command.env(name, v);
        }
    }
    // 不继承服务令牌、供应商密钥和 Node 注入参数。
    command.env("HOME", std::env::temp_dir());
    #[cfg(unix)]
    command.process_group(0);
    let child = command
        .spawn()
        .map_err(|e| format!("CODING_SPAWN_FAILED: {e}"))?;
    let pid = child.id();
    let mut owned = OwnedChild {
        child: Some(child),
        pid,
        permit,
    };
    let mut stdin = owned
        .child
        .as_mut()
        .unwrap()
        .stdin
        .take()
        .ok_or("CODING_STDIN_MISSING")?;
    let mut stdout = owned
        .child
        .as_mut()
        .unwrap()
        .stdout
        .take()
        .ok_or("CODING_STDOUT_MISSING")?;
    let payload = serde_json::to_vec(&input).map_err(|e| e.to_string())?;
    let work = async {
        stdin.write_all(&payload).await.map_err(|e| e.to_string())?;
        drop(stdin);
        let mut bytes = Vec::new();
        const MARKER: &[u8] = b"\nLX_SNAPSHOT_WORKER_EXIT:";
        let (body, exit) = loop {
            let mut buffer = [0u8; 8192];
            let count = stdout.read(&mut buffer).await.map_err(|e| e.to_string())?;
            if count == 0 {
                return Err("CODING_SUPERVISOR_EXITED_WITHOUT_RECEIPT".into());
            }
            bytes.extend_from_slice(&buffer[..count]);
            if bytes.len() > 8 * 1024 * 1024 {
                return Err("CODING_OUTPUT_LIMIT".into());
            }
            if let Some(start) = bytes.windows(MARKER.len()).position(|w| w == MARKER) {
                if let Some(end) = bytes[start + MARKER.len()..]
                    .iter()
                    .position(|b| *b == b'\n')
                {
                    let exit = std::str::from_utf8(
                        &bytes[start + MARKER.len()..start + MARKER.len() + end],
                    )
                    .map_err(|_| "CODING_EXIT_RECEIPT_INVALID")?
                    .parse::<i32>()
                    .map_err(|_| "CODING_EXIT_RECEIPT_INVALID")?;
                    break (bytes[..start].to_vec(), exit);
                }
            }
        };
        let value: Value =
            serde_json::from_slice(&body).map_err(|e| format!("CODING_INVALID_RESPONSE: {e}"))?;
        if exit != 0 || value.get("error").is_some() {
            return Err(format!("CODING_WORKER_FAILED: {}", value["error"]));
        }
        Ok(value)
    };
    let result = tokio::select! {value=tokio::time::timeout(Duration::from_secs(30),work)=>value.map_err(|_|"CODING_TIMEOUT".to_string()).and_then(|v|v),_=async {if let Some(scope)=&cancel{scope.cancelled().await}else{std::future::pending::<()>().await}}=>Err("CODING_CANCELLED".into())};
    if owned.pid.is_some() {
        #[cfg(unix)]
        if let Some(pid) = owned.pid {
            unsafe {
                libc::killpg(pid as i32, libc::SIGKILL);
            }
        }
        let _ = owned.child.as_mut().unwrap().start_kill();
        match tokio::time::timeout(Duration::from_secs(2), owned.child.as_mut().unwrap().wait())
            .await
        {
            Ok(Ok(_)) => {
                owned.pid = None;
                owned.child.take();
            }
            _ => return Err("CODING_CLEANUP_UNCONFIRMED".into()),
        }
    }
    result
}
async fn worker(
    input: Value,
    cwd: &Path,
    sandbox: &Option<Arc<dyn SandboxPort>>,
    ctx: &RunContext,
) -> Result<Value, String> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts/rust-tauri/r08-lsp-worker.mjs");
    run_snapshot_worker(&script, input, cwd, sandbox, ctx).await
}

struct Executor {
    ast: bool,
    access: Arc<ResourceAccess>,
    cwd: PathBuf,
    io: Arc<ResourceIoService>,
    sandbox: Option<Arc<dyn SandboxPort>>,
}
impl Executor {
    fn authorize(
        &self,
        ctx: &RunContext,
        path: &Path,
        cwd: &Path,
        write: bool,
    ) -> Result<(), String> {
        self.access
            .authorize(
                ctx.principal.storage_kind(),
                &ctx.principal.storage_subject(),
                ctx.session_id.as_str(),
                path,
                cwd,
                if write {
                    ResourceOp::Write
                } else {
                    ResourceOp::Read
                },
            )
            .map(|_| ())
            .map_err(|e| format!("CODING_PATH_NOT_AUTHORIZED: {}", e.message))
    }
    async fn snapshots(
        &self,
        ctx: &RunContext,
        args: &Value,
        cwd: &Path,
    ) -> Result<BTreeMap<String, ArtifactSnapshot>, String> {
        let explicit = args["files"].as_array();
        let mut paths = Vec::new();
        if let Some(files) = explicit {
            for f in files {
                let raw = Path::new(f.as_str().ok_or("AST_FILES_INVALID")?);
                self.authorize(ctx, raw, cwd, false)?;
                paths.push(if raw.is_absolute() {
                    raw.into()
                } else {
                    cwd.join(raw)
                });
            }
        } else {
            let mut pending = vec![cwd.to_path_buf()];
            let mut entries = 0;
            while let Some(dir) = pending.pop() {
                self.authorize(ctx, &dir, cwd, false)?;
                for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
                    let entry = entry.map_err(|e| e.to_string())?;
                    entries += 1;
                    if entries > 10000 {
                        return Err("CODING_SCAN_LIMIT: 请指定更小的 files 范围".into());
                    }
                    let file = entry.file_type().map_err(|e| e.to_string())?;
                    let path = entry.path();
                    if file.is_symlink() {
                        continue;
                    }
                    if file.is_dir() {
                        if ![".git", "node_modules", "dist", "build", "coverage"]
                            .contains(&entry.file_name().to_string_lossy().as_ref())
                        {
                            pending.push(path);
                        }
                    } else if file.is_file() {
                        paths.push(path);
                    }
                }
            }
        }
        let mut ignores = Vec::new();
        if explicit.is_none() && cwd.join(".gitignore").is_file() {
            let ignore = self
                .io
                .read_artifact_snapshot(
                    &json!({"kind":"local-file","path":cwd.join(".gitignore")}),
                    &op(ctx),
                )
                .await
                .map_err(|e| e.message)?;
            for line in std::str::from_utf8(&ignore.bytes)
                .map_err(|_| "GITIGNORE_NOT_UTF8")?
                .lines()
            {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let negative = line.starts_with('!');
                let pattern = line
                    .trim_start_matches('!')
                    .trim_start_matches('/')
                    .trim_end_matches('/');
                if !pattern.is_empty() {
                    ignores.push((negative, glob(pattern)?));
                }
            }
        }
        paths.sort();
        let patterns: Vec<_> = args["globs"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| {
                p.as_str().ok_or("AST_GLOB_INVALID").and_then(|s| {
                    glob(s.strip_prefix('!').unwrap_or(s))
                        .map(|r| (s.starts_with('!'), r))
                        .map_err(|_| "AST_GLOB_INVALID")
                })
            })
            .collect::<Result<_, _>>()?;
        let language = args["language"].as_str().unwrap_or("");
        let extensions: Option<&[&str]> = if self.ast {
            if explicit.is_some() {
                None
            } else {
                Some(match language {
                    "javascript" | "js" => &["js", "jsx"],
                    "typescript" | "ts" => &["ts"],
                    "tsx" => &["tsx"],
                    "python" | "py" => &["py"],
                    "rust" | "rs" => &["rs"],
                    "go" => &["go"],
                    "c" => &["c", "h"],
                    "cpp" => &["cpp", "hpp", "cc", "cxx"],
                    "java" => &["java"],
                    "csharp" | "cs" => &["cs"],
                    "ruby" => &["rb"],
                    "swift" => &["swift"],
                    "kotlin" => &["kt", "kts"],
                    "scala" => &["scala"],
                    "html" => &["html", "htm"],
                    "css" => &["css"],
                    _ => return Err("AST_LANGUAGE_SCAN_UNSUPPORTED: 请用 files 明确范围".into()),
                })
            }
        } else {
            Some(match language {
                "typescript" => &["ts", "tsx", "js", "jsx"],
                "python" => &["py"],
                "go" => &["go"],
                "rust" => &["rs"],
                "cpp" => &["c", "h", "cpp", "hpp", "cc"],
                _ => return Err("LSP_LANGUAGE_INVALID".into()),
            })
        };
        let mut result = BTreeMap::new();
        let mut bytes = 0;
        for path in paths {
            let relative = path
                .strip_prefix(cwd)
                .map_err(|_| "CODING_FILE_OUTSIDE_WORKSPACE")?
                .to_str()
                .ok_or("CODING_PATH_NOT_UTF8")?
                .replace('\\', "/");
            if explicit.is_none() {
                let mut ignored = false;
                for (negative, re) in &ignores {
                    if re.is_match(&relative) || relative.split('/').any(|p| re.is_match(p)) {
                        ignored = !*negative;
                    }
                }
                if ignored {
                    continue;
                }
            }
            if let Some(ext) = extensions {
                if !ext.contains(&path.extension().and_then(|s| s.to_str()).unwrap_or("")) {
                    continue;
                }
            }
            if !patterns.is_empty() {
                let positive = patterns.iter().any(|(neg, _)| !neg);
                let mut included = !positive;
                for (negative, re) in &patterns {
                    if re.is_match(&relative)
                        || (!relative.contains('/')
                            && re.is_match(path.file_name().unwrap().to_str().unwrap_or("")))
                    {
                        included = !*negative;
                    }
                }
                if !included {
                    continue;
                }
            }
            self.authorize(ctx, &path, cwd, false)?;
            let snapshot = self
                .io
                .read_artifact_snapshot(&json!({"kind":"local-file","path":path}), &op(ctx))
                .await
                .map_err(|e| format!("{}: {}", e.code, e.message))?;
            bytes += snapshot.bytes.len();
            if bytes > SNAPSHOT_LIMIT {
                return Err("CODING_SNAPSHOT_LIMIT: 请缩小工作范围".into());
            }
            if std::str::from_utf8(&snapshot.bytes).is_err() {
                if explicit.is_some() {
                    return Err("CODING_INPUT_NOT_UTF8".into());
                }
                continue;
            }
            result.insert(relative, snapshot);
        }
        Ok(result)
    }
    async fn ast(&self, ctx: &RunContext, args: &Value, cwd: &Path) -> Result<ToolOutcome, String> {
        let binary = executable("ast-grep")
            .ok_or("AST_GREP_MISSING: 请安装官方 @ast-grep/cli；环境依赖设置页可查看安装指导")?;
        worker(json!({"op":"ast","binary":binary,"pattern":args["pattern"],"language":args["language"],"text":""}),cwd,&self.sandbox,ctx).await?;
        let snapshots = self.snapshots(ctx, args, cwd).await?;
        let mut matches = Vec::new();
        let mut truncated = false;
        for (relative, snapshot) in &snapshots {
            let response=worker(json!({"op":"ast","binary":binary,"pattern":args["pattern"],"language":args["language"],"text":std::str::from_utf8(&snapshot.bytes).map_err(|_|"AST_INPUT_NOT_UTF8")?}),cwd,&self.sandbox,ctx).await?;
            let found = response["matches"]
                .as_array()
                .ok_or("AST_GREP_INVALID_MATCHES")?;
            for value in found {
                if value["range"]["start"]["line"].as_u64().is_none() || !value["text"].is_string()
                {
                    return Err("AST_GREP_INVALID_MATCH".into());
                }
                if matches.len() == 1000 {
                    truncated = true;
                    break;
                }
                let mut value = value.clone();
                value["file"] = json!(relative);
                matches.push(value);
            }
            if truncated {
                break;
            }
        }
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = (args["limit"].as_u64().unwrap_or(100) as usize).min(1000);
        let mode = args["output_mode"].as_str().unwrap_or("content");
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for m in &matches {
            *counts
                .entry(m["file"].as_str().unwrap().into())
                .or_default() += 1;
        }
        let mut files: Vec<_> = counts.iter().collect();
        files.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let text = match mode {
            "count" => format!(
                "{}{} match(es)\n{}",
                matches.len(),
                if truncated { "+" } else { "" },
                files
                    .iter()
                    .map(|(f, n)| format!("{f}: {n}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            "files" => {
                files.sort_by_key(|(f, _)| {
                    std::cmp::Reverse(
                        std::fs::metadata(&snapshots[*f].file_path)
                            .and_then(|m| m.modified())
                            .ok(),
                    )
                });
                files
                    .iter()
                    .take(100)
                    .map(|(f, n)| format!("{f}: {n}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            _ => matches
                .iter()
                .skip(offset)
                .take(limit)
                .map(|m| {
                    format!(
                        "{}:{}: {}",
                        m["file"].as_str().unwrap(),
                        m["range"]["start"]["line"],
                        m["text"]
                            .as_str()
                            .unwrap()
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .chars()
                            .take(200)
                            .collect::<String>()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        };
        let returned = matches.len().saturating_sub(offset).min(limit);
        let mut text = if matches.is_empty() {
            "no matches".into()
        } else {
            text
        };
        if mode == "content" && offset + returned < matches.len() {
            text.push_str(&format!(
                "\n... {} more matches — use offset={}",
                matches.len() - offset - returned,
                offset + returned
            ));
        }
        Ok(success(
            text,
            json!({"matchCount":matches.len(),"pageReturned":returned,"offset":offset,"limit":limit,"truncated":truncated,"outputMode":mode,"matches":matches.iter().skip(offset).take(limit).collect::<Vec<_>>()}),
        ))
    }
    async fn lsp(&self, ctx: &RunContext, args: &Value, cwd: &Path) -> Result<ToolOutcome, String> {
        let action = args["action"].as_str().unwrap_or("status");
        let servers = [
            ("typescript", "typescript-language-server"),
            ("python", "pyright-langserver"),
            ("go", "gopls"),
            ("rust", "rust-analyzer"),
            ("cpp", "clangd"),
        ];
        if action == "status" {
            let response = worker(json!({"op":"status"}), cwd, &self.sandbox, ctx).await?;
            let availability = response["availability"]
                .as_array()
                .ok_or("LSP_STATUS_INVALID")?
                .clone();
            return Ok(success(
                availability
                    .iter()
                    .map(|a| {
                        format!(
                            "{}: {} ({}){}",
                            a["language"],
                            if a["available"] == true {
                                "ready"
                            } else {
                                "server missing"
                            },
                            a["server"],
                            if a["available"] == true {
                                ""
                            } else {
                                " — 请安装后检查环境依赖设置页"
                            }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                json!({"availability":availability}),
            ));
        }
        let language = args["language"].as_str().ok_or("LSP_LANGUAGE_REQUIRED")?;
        let server = servers
            .iter()
            .find(|(l, _)| *l == language)
            .ok_or("LSP_LANGUAGE_INVALID")?
            .1;
        if executable(server).is_none() {
            return Err(format!(
                "LSP_SERVER_MISSING: {server} 未安装，请检查环境依赖设置页"
            ));
        }
        if action == "rename"
            && args["new_name"]
                .as_str()
                .is_none_or(|s| s.trim().is_empty())
        {
            return Err("LSP_NEW_NAME_REQUIRED".into());
        }
        let raw = Path::new(args["file"].as_str().ok_or("LSP_FILE_REQUIRED")?);
        self.authorize(ctx, raw, cwd, false)?;
        let file = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            cwd.join(raw)
        }
        .canonicalize()
        .map_err(|e| e.to_string())?;
        let relative = file
            .strip_prefix(cwd)
            .map_err(|_| "LSP_FILE_OUTSIDE_WORKSPACE")?
            .to_str()
            .ok_or("LSP_PATH_NOT_UTF8")?
            .replace('\\', "/");
        let snapshots = self.snapshots(ctx, args, cwd).await?;
        if !snapshots.contains_key(&relative) {
            return Err("LSP_FILE_NOT_IN_SNAPSHOT".into());
        }
        let response=worker(json!({"op":"lsp","action":action,"language":language,"file":relative,"line":args["line"].as_u64().unwrap_or(1),"column":args["column"].as_u64().unwrap_or(1),"new_name":args["new_name"],"documents":snapshots.iter().map(|(r,s)|json!({"relative":r,"text":std::str::from_utf8(&s.bytes).unwrap()})).collect::<Vec<_>>()}),cwd,&self.sandbox,ctx).await?;
        let result = &response["result"];
        if action != "rename" {
            validate_locations(result, &snapshots)?;
            return Ok(read_result(action, result));
        }
        let edits = workspace_edits(result)?;
        let mut prepared = Vec::new();
        let mut preview = Vec::new();
        let mut write_denied = None;
        for (relative, changes) in edits {
            let snapshot = snapshots.get(&relative).ok_or("LSP_EDIT_NOT_IN_SNAPSHOT")?;
            let before = std::str::from_utf8(&snapshot.bytes).map_err(|_| "LSP_EDIT_NOT_UTF8")?;
            let updated = apply_edits(before, &changes)?;
            preview.push(json!({"file":relative,"edits":changes,"sourceStamp":snapshot.meta["artifactStamp"]}));
            if let Err(error) = self.authorize(ctx, &snapshot.file_path, cwd, true) {
                write_denied = Some(error);
                continue;
            }
            match self
                .io
                .prepare_artifact_write(
                    &json!({"kind":"local-file","path":snapshot.file_path}),
                    true,
                    &op(ctx),
                )
                .await
            {
                Ok(preflight) => {
                    preflight
                        .require_snapshot(&snapshot.meta["artifactStamp"])
                        .map_err(|e| format!("{}: {}", e.code, e.message))?;
                    prepared.push((relative, preflight, updated.into_bytes()));
                }
                Err(error) => {
                    write_denied = Some(format!("{}: {}", error.code, error.message));
                }
            }
        }
        if args["preview"].as_bool() == Some(true) || write_denied.is_some() {
            return Ok(success(
                format!(
                    "rename preview; no files written{}",
                    write_denied
                        .as_deref()
                        .map(|e| format!(": {e}"))
                        .unwrap_or_default()
                ),
                json!({"applied":false,"preview":preview,"errorCode":write_denied.map(|_|"LSP_WRITE_UNAVAILABLE")}),
            ));
        }
        let mut receipts = Vec::new();
        for (relative, preflight, bytes) in prepared {
            match self.io.publish_artifact(&preflight, bytes, &op(ctx)).await {
                Ok(receipt) => receipts.push(json!({"file":relative,"receipt":receipt})),
                Err(error) => {
                    return Ok(success(
                        format!(
                            "LSP_WRITE_FAILED: {}；已写入 {} 个文件，收据可用于恢复",
                            error.message,
                            receipts.len()
                        ),
                        json!({"applied":false,"partial":!receipts.is_empty(),"errorCode":error.code,"receipts":receipts,"preview":preview}),
                    ))
                }
            }
        }
        Ok(success(
            if receipts.is_empty() {
                "rename returned no edits — nothing changed".into()
            } else {
                format!("renamed — {} file(s) updated", receipts.len())
            },
            json!({"applied":!receipts.is_empty(),"changedFiles":receipts.iter().map(|r|&r["file"]).collect::<Vec<_>>(),"receipts":receipts}),
        ))
    }
}
// 保留原消费者所需的 1-based 位置与摘要字段，同时保留服务器原始结果便于核验。
fn read_result(action: &str, result: &Value) -> ToolOutcome {
    let mut details = json!({"action":action,"result":result});
    let text = match action {
        "definition" | "references" => {
            let items: Vec<&Value> = if let Some(a) = result.as_array() {
                a.iter().collect()
            } else if result.is_object() {
                vec![result]
            } else {
                vec![]
            };
            let locations:Vec<_>=items.iter().map(|item|{let range=item.get("range").or_else(||item.get("targetSelectionRange")).or_else(||item.get("targetRange")).unwrap_or(&Value::Null);json!({"file":item.get("uri").or_else(||item.get("targetUri")),"line":range["start"]["line"].as_u64().unwrap_or(0)+1,"column":range["start"]["character"].as_u64().unwrap_or(0)+1})}).collect();
            details["count"] = json!(locations.len());
            details["locations"] = json!(locations.iter().take(50).collect::<Vec<_>>());
            if locations.is_empty() {
                format!("no {action} found")
            } else {
                locations
                    .iter()
                    .take(50)
                    .map(|l| {
                        format!(
                            "{}:{}:{}",
                            l["file"].as_str().unwrap_or(""),
                            l["line"],
                            l["column"]
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
        "diagnostics" => {
            let diagnostics:Vec<_>=result.as_array().into_iter().flatten().take(100).map(|d|json!({"severity":match d["severity"].as_u64(){Some(1)=>"error",Some(2)=>"warning",Some(3)=>"info",Some(4)=>"hint",_=>"?"},"line":d["range"]["start"]["line"].as_u64().unwrap_or(0)+1,"message":d["message"].as_str().unwrap_or("").lines().next().unwrap_or("").chars().take(300).collect::<String>()})).collect();
            details["count"] = json!(diagnostics.len());
            details["diagnostics"] = json!(diagnostics);
            if diagnostics.is_empty() {
                "no diagnostics".into()
            } else {
                diagnostics
                    .iter()
                    .map(|d| {
                        format!(
                            "[{}] :{} — {}",
                            d["severity"].as_str().unwrap(),
                            d["line"],
                            d["message"].as_str().unwrap()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
        "hover" => {
            let contents = &result["contents"];
            let hover = if let Some(text) = contents.as_str() {
                text.to_owned()
            } else if let Some(value) = contents["value"].as_str() {
                value.into()
            } else if let Some(items) = contents.as_array() {
                items
                    .iter()
                    .map(|i| i.as_str().or_else(|| i["value"].as_str()).unwrap_or(""))
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                String::new()
            };
            details["hover"] = if hover.is_empty() {
                Value::Null
            } else {
                json!(hover.chars().take(2000).collect::<String>())
            };
            if hover.is_empty() {
                "no hover info at this position".into()
            } else {
                hover.chars().take(4000).collect()
            }
        }
        "symbols" => {
            fn walk(v: &Value, out: &mut Vec<Value>) {
                for item in v.as_array().into_iter().flatten() {
                    out.push(json!({"name":item["name"],"kind":item["kind"],"line":item["range"]["start"]["line"].as_u64().or_else(||item["location"]["range"]["start"]["line"].as_u64()).unwrap_or(0)+1}));
                    walk(&item["children"], out);
                }
            }
            let mut symbols = Vec::new();
            walk(result, &mut symbols);
            details["count"] = json!(symbols.len());
            details["symbols"] = json!(symbols.iter().take(200).collect::<Vec<_>>());
            if symbols.is_empty() {
                "no symbols".into()
            } else {
                symbols
                    .iter()
                    .take(200)
                    .map(|s| {
                        format!(
                            "{} (kind {}) :{}",
                            s["name"].as_str().unwrap_or(""),
                            s["kind"],
                            s["line"]
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
        _ => result.to_string(),
    };
    success(text, details)
}
fn validate_locations(
    v: &Value,
    snapshots: &BTreeMap<String, ArtifactSnapshot>,
) -> Result<(), String> {
    match v {
        Value::Array(a) => {
            for v in a {
                validate_locations(v, snapshots)?;
            }
        }
        Value::Object(o) => {
            for (k, v) in o {
                if k == "uri" || k == "targetUri" {
                    if !snapshots.contains_key(v.as_str().ok_or("LSP_LOCATION_INVALID")?) {
                        return Err("LSP_LOCATION_NOT_IN_SNAPSHOT".into());
                    }
                } else {
                    validate_locations(v, snapshots)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
fn workspace_edits(v: &Value) -> Result<BTreeMap<String, Vec<Value>>, String> {
    let mut out = BTreeMap::new();
    if v.is_null() {
        return Ok(out);
    }
    if let Some(changes) = v.get("changes") {
        for (path, edits) in changes.as_object().ok_or("LSP_CHANGES_INVALID")? {
            out.insert(
                path.clone(),
                edits.as_array().ok_or("LSP_EDITS_INVALID")?.clone(),
            );
        }
    }
    if let Some(changes) = v.get("documentChanges") {
        for change in changes.as_array().ok_or("LSP_DOCUMENT_CHANGES_INVALID")? {
            if change.get("kind").is_some() {
                return Err("LSP_FILE_OPERATIONS_UNSUPPORTED".into());
            }
            let doc = &change["textDocument"];
            if doc["version"].as_u64().is_some_and(|v| v != 1) {
                return Err("LSP_DOCUMENT_VERSION_CONFLICT".into());
            }
            let path = doc["uri"].as_str().ok_or("LSP_DOCUMENT_URI_INVALID")?;
            if out.contains_key(path) {
                return Err("LSP_DUPLICATE_DOCUMENT_EDIT".into());
            }
            out.insert(
                path.into(),
                change["edits"]
                    .as_array()
                    .ok_or("LSP_EDITS_INVALID")?
                    .clone(),
            );
        }
    }
    Ok(out)
}
fn utf16_offset(text: &str, position: &Value) -> Result<usize, String> {
    let line = position["line"].as_u64().ok_or("LSP_RANGE_INVALID")? as usize;
    let column = position["character"].as_u64().ok_or("LSP_RANGE_INVALID")? as usize;
    let mut start = 0;
    for _ in 0..line {
        start += text[start..].find('\n').ok_or("LSP_RANGE_LINE_INVALID")? + 1;
    }
    let tail = &text[start..];
    let end = tail.find('\n').unwrap_or(tail.len());
    let mut units = 0;
    for (i, c) in tail[..end].char_indices() {
        if units == column {
            return Ok(start + i);
        }
        units += c.len_utf16();
        if units > column {
            return Err("LSP_RANGE_SURROGATE_SPLIT".into());
        }
    }
    if units == column {
        Ok(start + end)
    } else {
        Err("LSP_RANGE_COLUMN_INVALID".into())
    }
}
fn apply_edits(text: &str, edits: &[Value]) -> Result<String, String> {
    let mut positions = Vec::new();
    for edit in edits {
        let start = utf16_offset(text, &edit["range"]["start"])?;
        let end = utf16_offset(text, &edit["range"]["end"])?;
        if end < start {
            return Err("LSP_RANGE_REVERSED".into());
        }
        positions.push((
            start,
            end,
            edit["newText"].as_str().ok_or("LSP_NEW_TEXT_INVALID")?,
        ));
    }
    positions.sort_by_key(|(s, e, _)| (*s, *e));
    for pair in positions.windows(2) {
        if pair[0].1 > pair[1].0 || pair[0].0 == pair[1].0 {
            return Err("LSP_OVERLAPPING_EDITS".into());
        }
    }
    let mut out = text.to_owned();
    for (start, end, replacement) in positions.into_iter().rev() {
        out.replace_range(start..end, replacement);
    }
    Ok(out)
}
impl ToolExecutorPort for Executor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let cwd = toolgateway::workspace().unwrap_or_else(|| self.cwd.clone());
            let result = if self.ast {
                self.ast(ctx, request.arguments.as_value(), &cwd).await
            } else {
                self.lsp(ctx, request.arguments.as_value(), &cwd).await
            };
            ToolExecutionResult::of_ctx(ctx, result.unwrap_or_else(failure))
        })
    }
}
pub fn register(
    registry: &ToolRegistry,
    gateway: &ToolInvocationGateway,
    access: Arc<ResourceAccess>,
    cwd: PathBuf,
    io: Arc<ResourceIoService>,
    sandbox: Option<Arc<dyn SandboxPort>>,
    budget: &SchemaBudget,
) -> Result<(), String> {
    for ast in [true, false] {
        let name = if ast { "ast_grep" } else { "lsp" };
        let schema = if ast {
            json!({"type":"object","properties":{"pattern":{"type":"string","minLength":1},"language":{"type":"string","minLength":1},"globs":{"type":"array","items":{"type":"string"}},"files":{"type":"array","items":{"type":"string","minLength":1}},"output_mode":{"enum":["content","files","count"]},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":1000}},"required":["pattern","language"],"additionalProperties":false})
        } else {
            json!({"type":"object","properties":{"action":{"enum":["status","diagnostics","definition","references","hover","symbols","rename"]},"language":{"enum":["typescript","python","go","rust","cpp"]},"file":{"type":"string","minLength":1},"line":{"type":"integer","minimum":1},"column":{"type":"integer","minimum":1},"new_name":{"type":"string","minLength":1},"preview":{"type":"boolean"}},"additionalProperties":false})
        };
        let target=registry.register(ToolManifest{origin:ToolOrigin::FirstParty,local_name:name.into(),display_name:name.into(),aliases:vec![],version:"1.0.0".into(),description:if ast{"Search syntax structures in authorized snapshots with the official ast-grep parser; content/files/count and pagination; read-only."}else{"Real LSP diagnostics, definition, references, hover, symbols and prepared rename. Rust validates all ranges and writes against original snapshots; preview never writes."}.into(),input_schema:ToolSchemaDocument{dialect:"json-schema/2020-12".into(),schema},output_schema:None,permission:PermissionContract{kind:if ast{PermissionKind::Read}else{PermissionKind::ArgumentAwareLsp},capability_base:if ast{"ast_grep.search"}else{"lsp"}.into()},availability:Availability::Available,timeout_ms:Some(60_000),max_concurrency:Some(4),declared_permission:if ast{DeclaredPermission::ReadOnly}else{DeclaredPermission::Execute},recovery:ToolRecoveryCapability{read_only:ast,honors_idempotency_key:false,externally_verifiable:false}},budget).map_err(|e|format!("{e:?}"))?.target_id;
        let extraction_access = access.clone();
        let extraction_cwd = cwd.clone();
        let extractor = Arc::new(
            move |input: &toolgateway::ResourceExtractionInput,
                  args: &lingxi_kernel::toolcatalog::EffectiveArguments| {
                if !ast && args.as_value()["action"].as_str().unwrap_or("status") == "status" {
                    return Ok(vec![]);
                }
                let cwd = toolgateway::workspace().unwrap_or_else(|| extraction_cwd.clone());
                let files: Vec<&str> = if ast {
                    args.as_value()["files"]
                        .as_array()
                        .map(|v| v.iter().filter_map(Value::as_str).collect())
                        .unwrap_or_else(|| vec!["."])
                } else {
                    vec![args.as_value()["file"]
                        .as_str()
                        .ok_or("LSP_FILE_REQUIRED")?]
                };
                files
                    .iter()
                    .map(|f| {
                        extraction_access
                            .authorize(
                                input.principal_kind,
                                &input.principal_subject,
                                &input.session_id,
                                Path::new(f),
                                &cwd,
                                ResourceOp::Read,
                            )
                            .map_err(|_| "CODING_PATH_NOT_AUTHORIZED".to_string())
                    })
                    .collect::<Result<Vec<_>, String>>()
            },
        );
        gateway.bind_executor_with_resources(
            target,
            Arc::new(Executor {
                ast,
                access: access.clone(),
                cwd: cwd.clone(),
                io: io.clone(),
                sandbox: sandbox.clone(),
            }),
            Some(extractor),
            "Rust-authorized parser snapshots and versioned ResourceIO rename",
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf16_edit_offsets_and_overlap() {
        let text = "😀 int old;\nold;";
        let edit = json!({"range":{"start":{"line":0,"character":7},"end":{"line":0,"character":10}},"newText":"new"});
        assert_eq!(
            apply_edits(text, &[edit.clone()]).unwrap(),
            "😀 int new;\nold;"
        );
        assert!(apply_edits(text, &[edit.clone(), edit]).is_err());
        assert!(utf16_offset(text, &json!({"line":0,"character":1})).is_err());
    }
}

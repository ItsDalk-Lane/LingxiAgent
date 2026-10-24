//! PTC 外围只运行纯程序；每个子调用必须回到真实父 driver 重新裁决。
use crate::{
    events::EventService, inject::ServiceClock, runs::RunSupervisor,
    toolgateway::ToolInvocationGateway,
};
use futures::{stream::FuturesUnordered, FutureExt, StreamExt};
use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::{
    invocation::ToolRecoveryCapability,
    ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess},
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin, ToolRegistry,
    },
    RunContext,
};
use lingxi_protocol::{ContentBlock, ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::{Duration, Instant},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
const MAX_FRAME: usize = 8 * 1024 * 1024;
const WALL: Duration = Duration::from_secs(602);
fn clip(text: &str, cap: usize) -> (String, bool) {
    if text.len() <= cap {
        return (text.into(), false);
    }
    let mut head = cap / 2;
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = text.len() - cap / 2;
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    (
        format!(
            "{}\n[... {} bytes omitted ...]\n{}",
            &text[..head],
            tail - head,
            &text[tail..]
        ),
        true,
    )
}
fn summary(text: &str, cap: usize) -> String {
    let mut chars = text.chars();
    let mut result: String = chars.by_ref().take(cap).collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}
fn error(message: impl Into<String>, details: Value) -> ToolOutcome {
    ToolOutcome::Failed {
        error: ProtocolError::new(ErrorCode::InvalidMessage, message.into(), false)
            .with_details(details.as_object().cloned().unwrap_or_default()),
    }
}
fn content_text(outcome: &ToolOutcome) -> String {
    match outcome {
        ToolOutcome::Success { result } => result
            .content
            .iter()
            .filter_map(|b| {
                if let ContentBlock::Text { text } = b {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        ToolOutcome::Failed { error } => error.message.clone(),
        ToolOutcome::Cancelled => "子调用已取消".into(),
        ToolOutcome::Unknown { reason } => format!("子调用结果未确认：{reason}"),
    }
}
fn envelope(name: &str, outcome: &ToolOutcome) -> Value {
    match outcome {
        ToolOutcome::Success { result } => {
            let values: Vec<Value> = result
                .content
                .iter()
                .map(|b| {
                    if let ContentBlock::Text { text } = b {
                        json!(text)
                    } else {
                        serde_json::to_value(b).unwrap_or(Value::Null)
                    }
                })
                .collect();
            json!({"ok":true,"value":match values.len(){0=>Value::Null,1=>values[0].clone(),_=>Value::Array(values)}})
        }
        _ => json!({"ok":false,"error":{"toolName":name,"message":content_text(outcome)}}),
    }
}
struct OwnedWorker {
    child: Option<tokio::process::Child>,
    pid: Option<u32>,
    permit: Option<Arc<lingxi_kernel::session_admission::SessionPermit>>,
}
impl Drop for OwnedWorker {
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
        let permit = self.permit.take();
        // 执行 future 被取消也不能先释放会话停写许可；回收者持有到实际 wait。
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move{
                let _permit=permit;
                if !matches!(tokio::time::timeout(Duration::from_secs(2),child.wait()).await,Ok(Ok(_))){
                    tracing::error!("PTC_STOP_UNCONFIRMED: cleanup continues while retaining the session permit");
                    loop{if child.wait().await.is_ok(){break;}tokio::time::sleep(Duration::from_millis(100)).await;}
                }
            });
        } else {
            std::thread::spawn(move || {
                let _permit = permit;
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => break,
                        Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                        Err(error) => {
                            tracing::error!("PTC_REAP_FAILED: {error}");
                            std::thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
            });
        }
    }
}
impl OwnedWorker {
    async fn stop(&mut self) -> Result<(), String> {
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            unsafe {
                libc::killpg(pid as i32, libc::SIGKILL);
            }
        }
        let _ = child.start_kill();
        match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
            Ok(Ok(_)) => {
                self.pid = None;
                self.child.take();
                self.permit.take();
                Ok(())
            }
            other => Err(format!("RUN_TOOLS_STOP_UNCONFIRMED: {other:?}")),
        }
    }
}

// 程序即使逃出 vm 也不能取得宿主数据或网络；不是把 vm 当作授权边界。
fn contained_command(node: &Path, script: &Path) -> Result<tokio::process::Command, String> {
    let node = node
        .canonicalize()
        .map_err(|e| format!("RUN_TOOLS_NODE_INVALID: {e}"))?;
    let script = script
        .canonicalize()
        .map_err(|e| format!("RUN_TOOLS_WORKER_MISSING: {e}"))?;
    let runtime = script_root()
        .join("lib/ptc/ptc-runtime.ts")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let package = script_root()
        .join("package.json")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    {
        let sandbox = Path::new("/usr/bin/sandbox-exec");
        if !sandbox.is_file() {
            return Err("RUN_TOOLS_CONTAINMENT_UNAVAILABLE: sandbox-exec 不存在".into());
        }
        let mut profile=String::from("(version 1)(deny default)(allow process-exec process-fork signal sysctl-read mach* ipc-posix* file-read-metadata)(allow file-read* (literal \"/\")");
        for path in [
            "/usr",
            "/System",
            "/Library/Apple",
            "/opt/homebrew",
            "/bin",
            "/sbin",
        ] {
            profile.push_str(&format!(
                " (subpath {})",
                serde_json::to_string(path).unwrap()
            ));
        }
        for path in [&node, &script, &runtime, &package] {
            profile.push_str(&format!(
                " (literal {})",
                serde_json::to_string(&path.to_string_lossy()).unwrap()
            ));
        }
        profile.push_str(")(allow file-write* (literal \"/dev/null\"))(deny network*)");
        let mut command = tokio::process::Command::new(sandbox);
        command.args(["-p", &profile]).arg(node).arg(script);
        return Ok(command);
    }
    #[cfg(target_os = "linux")]
    {
        let sandbox = ["/usr/bin/bwrap", "/bin/bwrap"]
            .iter()
            .map(Path::new)
            .find(|p| p.is_file())
            .ok_or("RUN_TOOLS_CONTAINMENT_UNAVAILABLE: 缺少 bwrap，不能无隔离执行")?;
        let mut command = tokio::process::Command::new(sandbox);
        command.args([
            "--unshare-all",
            "--die-with-parent",
            "--new-session",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--tmpfs",
            "/tmp",
        ]);
        let system: Vec<&Path> = ["/usr", "/bin", "/sbin", "/lib", "/lib64"]
            .iter()
            .map(Path::new)
            .filter(|p| p.exists())
            .collect();
        for path in &system {
            command.arg("--ro-bind").arg(path).arg(path);
        }
        let mut directories = std::collections::BTreeSet::new();
        for file in [&node, &script, &runtime, &package] {
            for path in file.ancestors().skip(1) {
                if path != Path::new("/") && !system.iter().any(|base| path.starts_with(base)) {
                    directories.insert(path.to_path_buf());
                }
            }
        }
        for path in directories {
            command.arg("--dir").arg(path);
        }
        for file in [&node, &script, &runtime, &package] {
            if !system.iter().any(|base| file.starts_with(base)) {
                command.arg("--ro-bind").arg(file).arg(file);
            }
        }
        command
            .args(["--chdir", "/tmp", "--"])
            .arg(node)
            .arg(script);
        return Ok(command);
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (node, script, runtime, package);
        Err("RUN_TOOLS_PLATFORM_UNSUPPORTED: 本平台尚无已验证的严格程序隔离，拒绝执行".into())
    }
}

struct Executor {
    runs: Weak<RunSupervisor>,
    db: Arc<RunDatabase>,
    events: Arc<EventService>,
    clock: Arc<dyn ServiceClock>,
}
impl Executor {
    async fn program(
        &self,
        ctx: &RunContext,
        parent: &ToolCallId,
        code: &str,
        records: &mut Vec<Value>,
    ) -> Result<Value, String> {
        let runs = self.runs.upgrade().ok_or("RUN_TOOLS_DRIVER_GONE")?;
        let permit = crate::task_supervisor::retain_blocking_run_permit(ctx)
            .map_err(|_| "RUN_TOOLS_PERMIT_MISMATCH")?;
        let cancel = crate::task_supervisor::blocking_run_cancel_scope(ctx)
            .map_err(|_| "RUN_TOOLS_CANCEL_SCOPE_MISMATCH")?;
        let node = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|p| p.join("node"))
            .find(|p| p.is_file())
            .ok_or("RUN_TOOLS_NODE_MISSING: 请在环境依赖设置中安装 Node.js 24")?;
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../scripts/rust-tauri/r08-ptc-worker.mjs");
        let mut command = contained_command(&node, &script)?;
        command
            .current_dir("/")
            .env_clear()
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        for key in ["PATH", "LANG", "LC_ALL", "LC_CTYPE", "TMPDIR", "TZ"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        // 程序无法直接访问 Node 宿主能力；外围也不继承服务凭据或注入参数。
        command.env("HOME", std::env::temp_dir());
        #[cfg(unix)]
        command.process_group(0);
        let child = command
            .spawn()
            .map_err(|e| format!("RUN_TOOLS_SPAWN_FAILED: {e}"))?;
        let pid = child.id();
        let mut owned = OwnedWorker {
            child: Some(child),
            pid,
            permit,
        };
        let mut stdin = owned
            .child
            .as_mut()
            .expect("自有外围子进程")
            .stdin
            .take()
            .ok_or("RUN_TOOLS_STDIN_MISSING")?;
        let stdout = owned
            .child
            .as_mut()
            .expect("自有外围子进程")
            .stdout
            .take()
            .ok_or("RUN_TOOLS_STDOUT_MISSING")?;
        stdin
            .write_all(format!("{}\n", json!({"type":"run","code":code})).as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stdout);
        let mut frame = Vec::new();
        let mut sequence = 0u64;
        let mut children = FuturesUnordered::new();
        let mut done = None;
        let mut pipe_closed = false;
        let mut total_output = 0usize;
        let work = async {
            loop {
                if done.is_some() && children.is_empty() {
                    break;
                }
                tokio::select! {
                    read=reader.read_until(b'\n',&mut frame),if done.is_none()=>{
                        let count=read.map_err(|e|format!("RUN_TOOLS_READ_FAILED: {e}"))?;
                        if count==0{return Err("RUN_TOOLS_WORKER_EXIT: 程序未提供完成结果".into())}
                        if frame.len()>MAX_FRAME{return Err("RUN_TOOLS_FRAME_LIMIT".into())}
                        let message:Value=serde_json::from_slice(&frame).map_err(|e|format!("RUN_TOOLS_PROTOCOL_INVALID: {e}"))?;frame.clear();
                        match message["type"].as_str(){
                            Some("done")=>{done=Some(message["result"].clone());},
                            Some("call")=>{
                                if children.len()>=8{return Err("RUN_TOOLS_CONCURRENCY_LIMIT".into())}
                                sequence+=1;if message["id"].as_u64()!=Some(sequence){return Err("RUN_TOOLS_SEQUENCE_INVALID".into())}
                                let name=message["name"].as_str().ok_or("RUN_TOOLS_NAME_INVALID")?.to_string();
                                let args=message.get("args").cloned().unwrap_or_else(||json!({}));
                                let args_text=serde_json::to_string(&args).map_err(|e|e.to_string())?;
                                let mut record=json!({"seq":sequence,"name":name,"argsSummary":summary(&args_text,120),"ok":false,"ms":0,"error":"RUN_TOOLS_SUBCALL_UNCONFIRMED: 子调用尚未确认完成"});
                                if args_text.len()<=4000{record["args"]=args.clone();}else{record["argsOmitted"]=json!(true);}
                                records.push(record);let index=records.len()-1;let seq=sequence;let driver=runs.clone();
                                children.push(async move{
                                    let started=Instant::now();
                                    let outcome=driver.execute_program_child(&self.db,&self.events,ctx,parent,seq,&name,args,self.clock.now_unix_ms()).await;
                                    (index,seq,name,started.elapsed().as_millis() as u64,outcome)
                                }.boxed());
                            },_=>return Err("RUN_TOOLS_PROTOCOL_INVALID".into())
                        }
                    },
                    Some((index,seq,name,ms,outcome))=children.next(),if !children.is_empty()=>{
                        let record=&mut records[index];record["ms"]=json!(ms);
                        let (response,output)=match outcome{
                            Ok(outcome)=>{record["ok"]=json!(matches!(outcome,ToolOutcome::Success{..}));record.as_object_mut().expect("子调用记录").remove("error");let text=content_text(&outcome);if !matches!(outcome,ToolOutcome::Success{..}){record["error"]=json!(summary(&text,200));}(envelope(&name,&outcome),Some(text))},
                            Err(message)=>{record["error"]=json!(summary(&message,200));(json!({"ok":false,"error":{"toolName":name,"message":message}}),None)}
                        };
                        if let Some(text)=output.filter(|text|!text.is_empty()){
                            if total_output>=64000{record["outputOmitted"]=json!(true);}else{let (clipped,truncated)=clip(&text,8000);total_output+=clipped.len();record["output"]=json!(clipped);if truncated{record["outputTruncated"]=json!(true);}}
                        }
                        if done.is_none() && !pipe_closed {
                            // 程序可不等待已授权子调用便返回；先读完其完成帧，再判断管道关闭。
                            // 子动作仍必须收齐与记账，不能因外围先退出而重发或丢弃。
                            if stdin.write_all(format!("{}\n",json!({"type":"result","id":seq,"envelope":response})).as_bytes()).await.is_err(){pipe_closed=true;}
                        }
                    }
                }
            }
            // stdin 保持打开，使可信外围领头活到 Rust 主动停止并回收整组。
            done.ok_or_else(|| "RUN_TOOLS_RESULT_MISSING".into())
        };
        let result = tokio::select! {
            result=tokio::time::timeout(WALL,work)=>result.map_err(|_|"RUN_TOOLS_TIMEOUT: 程序超过10分钟墙钟预算".to_string()).and_then(|result|result),
            _=async{if let Some(cancel)=cancel{cancel.cancelled().await}else{std::future::pending::<()>().await}}=>Err("RUN_TOOLS_ABORT: 实际父任务已取消".into())
        };
        // 成功也必须实际回收外围进程；不能用 kill 请求代替停止事实。
        drop(children);
        owned.stop().await?;
        result
    }
    async fn run(&self, ctx: &RunContext, parent: &ToolCallId, args: &Value) -> ToolOutcome {
        let started = Instant::now();
        let code = args["code"].as_str().unwrap_or("");
        let mut records = Vec::new();
        let description = args["description"].as_str().unwrap_or("").trim();
        if code.trim().is_empty() {
            return error(
                "RUN_TOOLS_CODE_REQUIRED: code 必须为非空异步程序体",
                json!({"errorCode":"RUN_TOOLS_CODE_REQUIRED","subcalls":[],"description":description}),
            );
        }
        let outcome = self.program(ctx, parent, code, &mut records).await;
        let mut details = json!({"description":if description.is_empty(){Value::Null}else{json!(description)},"subcalls":records,"durationMs":started.elapsed().as_millis() as u64});
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(message) => {
                details["errorCode"] =
                    json!(message.split(':').next().unwrap_or("RUN_TOOLS_WORKER_EXIT"));
                return error(message, details);
            }
        };
        let logs = outcome["logs"]
            .as_array()
            .map(|logs| {
                logs.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        if outcome["ok"].as_bool() != Some(true) {
            let kind = outcome["kind"].as_str().unwrap_or("worker-exit");
            details["errorCode"] = json!(format!(
                "RUN_TOOLS_{}",
                kind.replace('-', "_").to_uppercase()
            ));
            if let Some(stack) = outcome["stack"].as_str() {
                details["stack"] = json!(stack);
            }
            let captured = if logs.is_empty() {
                String::new()
            } else {
                format!(
                    "\n\n--- captured output before the failure ---\n{}",
                    clip(&logs, 10000).0
                )
            };
            return error(
                format!(
                    "run_tools {kind}: {}{captured}",
                    outcome["message"].as_str().unwrap_or("unknown error")
                ),
                details,
            );
        }
        let returned = if outcome["hasReturn"].as_bool() == Some(true) {
            outcome["returnText"].as_str().unwrap_or("undefined")
        } else {
            ""
        };
        let body=match(logs.is_empty(),returned.is_empty()){(false,false)=>format!("{logs}\n\n--- returned ---\n{returned}"),(false,true)=>logs,(true,false)=>returned.into(),(true,true)=>"(the program finished without printing or returning anything — only console output and the return value leave the sandbox)".into()};
        let (body, truncated) = clip(body.trim(), 50000);
        let mut result = ToolSuccess::text(body);
        result.details = details.as_object().cloned();
        result.truncated = truncated;
        ToolOutcome::Success { result }
    }
}
fn script_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}
impl ToolExecutorPort for Executor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            ToolExecutionResult::of_ctx(
                ctx,
                self.run(ctx, call, request.arguments.as_value()).await,
            )
        })
    }
}
#[allow(clippy::too_many_arguments)]
pub fn register(
    registry: &ToolRegistry,
    gateway: &ToolInvocationGateway,
    runs: Weak<RunSupervisor>,
    db: Arc<RunDatabase>,
    events: Arc<EventService>,
    clock: Arc<dyn ServiceClock>,
    budget: &SchemaBudget,
) -> Result<(), String> {
    let receipt=registry.register(ToolManifest{origin:ToolOrigin::FirstParty,local_name:"run_tools".into(),display_name:"Run tools (PTC)".into(),aliases:vec![],version:"1.0.0".into(),description:"Run one async JS or erasable TS function body. Every tools.name(args) child uses the original target's Rust authorization and durable invocation path. Up to 8 children run concurrently. Only console logs and return leave in content; details.subcalls preserve bounded UI receipts. No recursion or Node builtins. Budgets: 10 minutes, 256 MB worker, 128 KB logs.".into(),input_schema:ToolSchemaDocument{dialect:"json-schema/2020-12".into(),schema:json!({"type":"object","properties":{"code":{"type":"string","maxLength":524288},"description":{"type":"string"}},"required":["code","description"],"additionalProperties":false})},output_schema:None,permission:PermissionContract{kind:PermissionKind::Execute,capability_base:"run_tools.execute".into()},availability:Availability::Available,timeout_ms:Some(605000),max_concurrency:None,declared_permission:DeclaredPermission::Execute,recovery:ToolRecoveryCapability::CONSERVATIVE},budget).map_err(|e|format!("run_tools注册失败：{e}"))?;
    gateway.bind_executor(
        receipt.target_id,
        Arc::new(Executor {
            runs,
            db,
            events,
            clock,
        }),
        "纯PTC运行时；每个子调用必须经过真实父driver的权限与持久事实链",
    );
    Ok(())
}

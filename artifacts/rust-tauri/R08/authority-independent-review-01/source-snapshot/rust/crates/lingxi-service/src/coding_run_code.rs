//! 会话专属的真实代码内核；持久解释器由统一进程监督器拥有。
use crate::{
    exectools::{ProcessOwnershipGuard, ProcessTools},
    inject::ServiceClock,
    procsupervisor::{
        ProcessHandleId, ProcessKind, ProcessOwner, ProcessSupervisor, RecordPhase, SpawnSpec,
        TerminationOutcome, TerminationReason,
    },
    resourceaccess::{ResourceAccess, ResourceOp},
    toolgateway::{self, ToolInvocationGateway},
};
use lingxi_kernel::{
    invocation::ToolRecoveryCapability,
    ports::{
        ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolRunStatus, ToolSuccess,
    },
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin, ToolRegistry,
    },
    RunContext,
};
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

const WAIT_CAP: Duration = Duration::from_secs(30);
const OUTPUT_CAP: usize = 50_000;
const KERNEL_CAP: usize = 64;

// 这里只编解码单次解释器请求；任务、批准、会话和恢复均不在 worker 内裁决。
// Python 的用户 globals 与协议局部变量分开，变量、import、函数跨调用保留。
const PYTHON_WORKER: &str = r#"
import sys,json,traceback,termios
_fd=sys.stdin.fileno()
_attrs=termios.tcgetattr(_fd)
_attrs[3] &= ~(termios.ECHO|termios.ICANON)
_attrs[6][termios.VMIN]=1
_attrs[6][termios.VTIME]=0
termios.tcsetattr(_fd,termios.TCSANOW,_attrs)
_out=sys.stdout
_err=sys.stderr
_globals={'__name__':'__main__','__builtins__':__builtins__}
def _marker(request,status):
    _out.write('\n\x1eLXRC:'+request+':'+status+'\x1f\n');_out.flush()
_marker(sys.argv[1],'ready')
for _line in sys.stdin:
    _request=json.loads(_line)
    try:
        exec(compile(_request['code'],'<run_code>','exec'),_globals,_globals)
    except BaseException:
        traceback.print_exc(file=_err);_err.flush();_marker(_request['id'],'error')
    else:
        _marker(_request['id'],'ok')
"#;

// 复用 Node 官方 REPL evaluator，保留 const/let/函数、require 和顶层 await。
// REPL 的输入流不连接协议 stdin，用户代码不会被回显成工具输出。
// Node 默认 evaluator 的运行异常走自身 domain；同时监听它，避免误报超时。
const NODE_WORKER: &str = r#"
const repl=require('node:repl'),stream=require('node:stream'),readline=require('node:readline'),util=require('node:util');
const out=process.stdout.write.bind(process.stdout),err=process.stderr.write.bind(process.stderr);
if(process.stdin.isTTY)process.stdin.setRawMode(true);
const input=new stream.PassThrough();
const server=repl.start({prompt:'',input,output:process.stdout,terminal:false,useGlobal:false,ignoreUndefined:true});
const marker=(id,state)=>out('\n\x1eLXRC:'+id+':'+state+'\x1f\n');
const lines=readline.createInterface({input:process.stdin,terminal:false,crlfDelay:Infinity});
let queue=Promise.resolve(),active=null;
if(!server._domain||typeof server._domain.on!=='function')throw new Error('Node REPL error channel unavailable');
server._domain.on('error',error=>{if(active)active(error);});
lines.on('line',line=>{queue=queue.then(()=>new Promise(resolve=>{
  const request=JSON.parse(line);let finished=false;
  const finish=(error,result)=>{
    if(finished)return;finished=true;active=null;
    if(error){err(String(error.stack||error)+'\n');marker(request.id,'error');}
    else {if(result!==undefined)out(util.inspect(result,{depth:4,maxArrayLength:100})+'\n');marker(request.id,'ok');}
    resolve();
  };
  active=finish;
  server.eval(request.code+'\n',server.context,'<run_code>',finish);
})).catch(error=>{err(String(error)+'\n');process.exit(70);});});
marker(process.argv[1],'ready');
"#;

fn fail(code: &str, message: impl AsRef<str>) -> ToolOutcome {
    ToolOutcome::Failed {
        error: ProtocolError::new(
            ErrorCode::InvalidMessage,
            format!("{code}: {}", message.as_ref()),
            false,
        ),
    }
}

#[derive(Default)]
struct Capture {
    head: Vec<u8>,
    tail: Vec<u8>,
    bytes: u64,
    dropped: u64,
}
impl Capture {
    fn append(&mut self, bytes: &[u8]) {
        self.bytes = self.bytes.saturating_add(bytes.len() as u64);
        let take = bytes
            .len()
            .min((OUTPUT_CAP / 2).saturating_sub(self.head.len()));
        self.head.extend_from_slice(&bytes[..take]);
        self.tail.extend_from_slice(&bytes[take..]);
        if self.tail.len() > OUTPUT_CAP / 2 {
            self.tail.drain(..self.tail.len() - OUTPUT_CAP / 2);
        }
    }
    fn truncated(&self) -> bool {
        self.dropped > 0 || self.bytes > (self.head.len() + self.tail.len()) as u64
    }
    fn text(&self) -> String {
        if self.truncated() {
            let head_end = std::str::from_utf8(&self.head)
                .map(|_| self.head.len())
                .unwrap_or_else(|error| error.valid_up_to());
            let tail_start = self
                .tail
                .iter()
                .position(|byte| byte & 0xc0 != 0x80)
                .unwrap_or(self.tail.len());
            let head = String::from_utf8_lossy(&self.head[..head_end]);
            let tail = String::from_utf8_lossy(&self.tail[tail_start..]);
            format!(
                "{head}\n[输出已截断：接收 {} 字节，另有 {} 字节未能从终端环缓冲读取]\n{tail}",
                self.bytes, self.dropped
            )
        } else {
            String::from_utf8_lossy(&[self.head.as_slice(), self.tail.as_slice()].concat())
                .into_owned()
        }
    }
}

struct Kernel {
    id: ProcessHandleId,
    cwd: PathBuf,
    awaiting: Option<String>,
    pending: String,
    output: Capture,
    completed_error: bool,
}
struct Slot {
    kernel: tokio::sync::Mutex<Option<Kernel>>,
    handle: Mutex<Option<ProcessHandleId>>,
}
impl Slot {
    fn new() -> Self {
        Self {
            kernel: tokio::sync::Mutex::new(None),
            handle: Mutex::new(None),
        }
    }
}

pub struct RunCodeExecutor {
    supervisor: Arc<ProcessSupervisor>,
    access: Arc<ResourceAccess>,
    cwd: PathBuf,
    kernels: Mutex<HashMap<String, Arc<Slot>>>,
}

impl RunCodeExecutor {
    fn executable(language: &str) -> Option<PathBuf> {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|dir| dir.join(language))
            .find(|file| file.is_file())
    }
    fn env(cwd: &Path) -> BTreeMap<String, String> {
        let mut env: BTreeMap<_, _> =
            ["PATH", "LANG", "LC_ALL", "LC_CTYPE", "TERM", "TZ", "TMPDIR"]
                .into_iter()
                .filter_map(|key| std::env::var(key).ok().map(|value| (key.into(), value)))
                .collect();
        // 不继承服务令牌、供应商密钥、NODE_OPTIONS 或 PYTHONPATH。
        env.insert("HOME".into(), cwd.to_string_lossy().into_owned());
        env.insert("PYTHONUNBUFFERED".into(), "1".into());
        env
    }
    fn success(&self, language: &str, kernel: &Kernel) -> ToolOutcome {
        let phase = self.supervisor.phase_of(&kernel.id);
        let running = matches!(phase, Some(RecordPhase::Running));
        let status = match &phase {
            Some(RecordPhase::Running) => "running",
            Some(RecordPhase::Terminating { .. }) => "terminating",
            Some(RecordPhase::CleanupTimedOut { .. }) => "stop_unconfirmed",
            Some(RecordPhase::Exited { .. } | RecordPhase::Terminated { .. }) => "exited",
            None => "stale",
        };
        let mut result = ToolSuccess::text(if kernel.output.bytes == 0 {
            "内核没有新输出。".into()
        } else {
            kernel.output.text()
        });
        result.truncated = kernel.output.truncated();
        result.details = json!({"language":language,"started":true,"status":status,"kernelId":kernel.id.as_str(),"stillBusy":kernel.awaiting.is_some(),"codeCompleted":kernel.awaiting.is_none(),"bytes":kernel.output.bytes,"droppedBytes":kernel.output.dropped,"phase":format!("{phase:?}"),"pid":self.supervisor.record(&kernel.id).map(|record|record.pid)}).as_object().cloned();
        if running {
            result.status = Some(Box::new(ToolRunStatus::Running {
                handle: kernel.id.to_string(),
            }));
        }
        ToolOutcome::Success { result }
    }
    async fn collect(&self, kernel: &mut Kernel) -> Result<(), String> {
        let Some(delivery) = self.supervisor.pty_deliver(&kernel.id).await else {
            return Err("内核进程记录已失效".into());
        };
        kernel.output.dropped = kernel
            .output
            .dropped
            .saturating_add(delivery.dropped_undelivered_bytes);
        kernel.pending.push_str(&delivery.text);
        let Some(id) = kernel.awaiting.clone() else {
            kernel.output.append(kernel.pending.as_bytes());
            kernel.pending.clear();
            return Ok(());
        };
        let marker = format!("\u{1e}LXRC:{id}:");
        if let Some(start) = kernel.pending.find(&marker) {
            if let Some(end) = kernel.pending[start + marker.len()..].find('\u{1f}') {
                let end = start + marker.len() + end;
                let state = &kernel.pending[start + marker.len()..end];
                if !["ok", "error", "ready"].contains(&state) {
                    return Err("解释器完成标记无效".into());
                }
                kernel.completed_error = state == "error";
                kernel.output.append(kernel.pending[..start].as_bytes());
                kernel.pending.drain(..end + 1);
                kernel.awaiting = None;
                return Ok(());
            }
        }
        // 标记可能跨多次终端读取；仅暂存短尾，正文始终按真实头尾边界计数。
        let mut split = kernel.pending.len().saturating_sub(marker.len() + 16);
        while split > 0 && !kernel.pending.is_char_boundary(split) {
            split -= 1;
        }
        kernel.output.append(kernel.pending[..split].as_bytes());
        kernel.pending.drain(..split);
        Ok(())
    }
    async fn wait(&self, kernel: &mut Kernel, limit: Duration) -> Result<bool, String> {
        let started = tokio::time::Instant::now();
        loop {
            self.collect(kernel).await?;
            if kernel.awaiting.is_none() {
                return Ok(true);
            }
            if !matches!(
                self.supervisor.phase_of(&kernel.id),
                Some(RecordPhase::Running)
            ) {
                kernel.output.append(kernel.pending.as_bytes());
                kernel.pending.clear();
                return Err(format!(
                    "RUN_CODE_KERNEL_EXITED: {:?}; 部分输出：{}；请 restart（状态已丢失）",
                    self.supervisor.phase_of(&kernel.id),
                    kernel.output.text()
                ));
            }
            if started.elapsed() >= limit {
                return Ok(false);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    async fn run(&self, ctx: &RunContext, call: &ToolCallId, args: &Value) -> ToolOutcome {
        if ctx.session_id.as_str().trim().is_empty() {
            return fail("RUN_CODE_NO_SESSION", "没有当前会话");
        }
        let action = args["action"].as_str().unwrap_or("run");
        if !["run", "status", "restart"].contains(&action) {
            return fail(
                "RUN_CODE_INVALID_ACTION",
                "action 必须为 run/status/restart",
            );
        }
        if action != "status" && toolgateway::workflow_scope().is_some() {
            return fail(
                "RUN_CODE_WORKFLOW_UNSUPPORTED",
                "工作流受限节点不能启动或重启持久解释器；请使用已授权且具备沙盒的执行入口。",
            );
        }
        let language = args["language"].as_str().unwrap_or("").trim();
        if !["python3", "node"].contains(&language) {
            return fail(
                "RUN_CODE_INVALID_LANGUAGE",
                "language 必须为 python3 或 node",
            );
        }
        let Some(executable) = Self::executable(language) else {
            return fail(
                "RUN_CODE_LANGUAGE_MISSING",
                format!("{language} 尚未安装；请在环境依赖设置中安装并检查。"),
            );
        };
        let scope = match toolgateway::trusted_settings_context(ctx, call) {
            Ok(scope) => scope,
            Err(error) => return fail("RUN_CODE_UNTRUSTED_CONTEXT", error),
        };
        let actual_cwd = toolgateway::workspace().unwrap_or_else(|| self.cwd.clone());
        let cwd = match self.access.authorize(
            ctx.principal.storage_kind(),
            &ctx.principal.storage_subject(),
            ctx.session_id.as_str(),
            &actual_cwd,
            &actual_cwd,
            ResourceOp::Read,
        ) {
            Ok(scope) => scope.path,
            Err(error) => return fail("RUN_CODE_RESOURCE_DENIED", error.to_string()),
        };
        let key = serde_json::to_string(&(
            ctx.principal.storage_kind(),
            ctx.principal.storage_subject(),
            scope.agent_id(),
            ctx.session_id.as_str(),
            language,
        ))
        .expect("内核归属可编码");
        let slot = {
            let mut map = self
                .kernels
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if action == "status" && !map.contains_key(&key) {
                let mut result = ToolSuccess::text("内核尚未启动；第一次 run 会启动它。");
                result.details = json!({"started":false,"language":language})
                    .as_object()
                    .cloned();
                return ToolOutcome::Success { result };
            }
            if !map.contains_key(&key) && map.len() >= KERNEL_CAP {
                return fail(
                    "RUN_CODE_KERNEL_CAPACITY",
                    "持久内核数量达到上限，请关闭已有内核或重启服务。",
                );
            }
            map.entry(key)
                .or_insert_with(|| Arc::new(Slot::new()))
                .clone()
        };
        if action == "restart" {
            let previous = slot
                .handle
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .clone();
            if let Some(id) = previous {
                let receipt = self
                    .supervisor
                    .terminate(&id, TerminationReason::Close)
                    .await;
                if matches!(
                    receipt.outcome,
                    TerminationOutcome::CleanupTimedOut
                        | TerminationOutcome::AlreadyTerminal(RecordPhase::CleanupTimedOut { .. })
                        | TerminationOutcome::Terminated {
                            reclaimed: false,
                            ..
                        }
                ) {
                    return fail(
                        "RUN_CODE_STOP_UNCONFIRMED",
                        "旧内核尚未确认停止，不启动第二内核。",
                    );
                }
            }
        }
        let mut holder = match slot.kernel.try_lock() {
            Ok(holder) => holder,
            Err(_) => {
                let id = slot
                    .handle
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .clone();
                if action == "status" {
                    let mut result = ToolSuccess::text("内核仍在执行；尚未确认代码完成。");
                    result.details = json!({"started":id.is_some(),"status":"running","stillBusy":true,"codeCompleted":false,"kernelId":id.as_ref().map(ProcessHandleId::as_str)}).as_object().cloned();
                    if let Some(id) = id {
                        result.status = Some(Box::new(ToolRunStatus::Running {
                            handle: id.to_string(),
                        }));
                    }
                    return ToolOutcome::Success { result };
                }
                if action != "restart" {
                    return fail(
                        "RUN_CODE_KERNEL_BUSY",
                        "同一会话内核仍在执行；请查询 status，不能重复提交。",
                    );
                }
                slot.kernel.lock().await
            }
        };
        if action == "restart" {
            *holder = None;
            *slot
                .handle
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()) = None;
        }
        if action == "status" {
            let Some(kernel) = holder.as_mut() else {
                return fail("RUN_CODE_KERNEL_MISSING", "内核记录不完整");
            };
            if let Err(error) = self.collect(kernel).await {
                return fail("RUN_CODE_KERNEL_IO", error);
            }
            return self.success(language, kernel);
        }
        let code = args["code"].as_str().unwrap_or("");
        if action == "run" && (code.trim().is_empty() || code.len() > 512 * 1024) {
            return fail(
                "RUN_CODE_CODE_REQUIRED",
                "run 必须提供非空且不超过512KiB的 code。",
            );
        }
        let mut startup_guard = None;
        if holder.is_none() {
            let ready = match crate::auth::hex_random_public(16) {
                Ok(id) => id,
                Err(error) => return fail("RUN_CODE_RANDOM_FAILED", error.to_string()),
            };
            let script = if language == "python3" {
                PYTHON_WORKER
            } else {
                NODE_WORKER
            };
            let argv = if language == "python3" {
                vec![
                    executable.to_string_lossy().into_owned(),
                    "-u".into(),
                    "-c".into(),
                    script.into(),
                    ready.clone(),
                ]
            } else {
                vec![
                    executable.to_string_lossy().into_owned(),
                    "-e".into(),
                    script.into(),
                    ready.clone(),
                ]
            };
            let spawned = match self
                .supervisor
                .spawn(SpawnSpec {
                    argv,
                    cwd: cwd.clone(),
                    env: Self::env(&cwd),
                    owner: ProcessOwner {
                        principal_kind: ctx.principal.storage_kind().into(),
                        principal_subject: ctx.principal.storage_subject(),
                        session_id: ctx.session_id.to_string(),
                        run_id: ctx.run_id.to_string(),
                        tool_call_id: call.clone(),
                    },
                    kind: ProcessKind::PersistentTerminal,
                    cols: 200,
                    rows: 50,
                })
                .await
            {
                Ok(process) => process,
                Err(error) => return fail("RUN_CODE_SPAWN_FAILED", error.to_string()),
            };
            startup_guard = Some(ProcessOwnershipGuard::new(
                self.supervisor.clone(),
                spawned.id.clone(),
            ));
            *slot
                .handle
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()) = Some(spawned.id.clone());
            *holder = Some(Kernel {
                id: spawned.id,
                cwd: cwd.clone(),
                awaiting: Some(ready),
                pending: String::new(),
                output: Capture::default(),
                completed_error: false,
            });
            match self
                .wait(
                    holder.as_mut().expect("刚登记的真实内核"),
                    Duration::from_secs(10),
                )
                .await
            {
                Ok(true) => {}
                Ok(false) => {
                    return fail(
                        "RUN_CODE_START_TIMEOUT",
                        "解释器未就绪，正在停止；不声称启动成功。",
                    )
                }
                Err(error) => return fail("RUN_CODE_KERNEL_DEAD", error),
            }
        }
        let kernel = holder.as_mut().expect("内核已创建");
        if let Err(error) = self.access.authorize(
            ctx.principal.storage_kind(),
            &ctx.principal.storage_subject(),
            ctx.session_id.as_str(),
            &kernel.cwd,
            &actual_cwd,
            ResourceOp::Read,
        ) {
            return fail("RUN_CODE_RESOURCE_DENIED", error.to_string());
        }
        if !matches!(
            self.supervisor.phase_of(&kernel.id),
            Some(RecordPhase::Running)
        ) {
            return fail(
                "RUN_CODE_KERNEL_DEAD",
                "内核已经死亡；请 restart 重建，旧变量不会恢复。",
            );
        }
        if let Err(error) = self.collect(kernel).await {
            return fail("RUN_CODE_KERNEL_IO", error);
        }
        if kernel.awaiting.is_some() {
            return fail(
                "RUN_CODE_KERNEL_BUSY",
                "上次代码仍未结束；请 status 或 restart，不重复提交。",
            );
        }
        let mut guard = startup_guard.unwrap_or_else(|| {
            ProcessOwnershipGuard::new(self.supervisor.clone(), kernel.id.clone())
        });
        kernel.output = Capture::default();
        kernel.pending.clear();
        kernel.completed_error = false;
        if action == "restart" {
            guard.disarm();
            let mut result = self.success(language, kernel);
            if let ToolOutcome::Success { result } = &mut result {
                result
                    .details
                    .as_mut()
                    .expect("结构化内核状态")
                    .insert("restarted".into(), json!(true));
            }
            return result;
        }
        let request_id = match crate::auth::hex_random_public(16) {
            Ok(id) => id,
            Err(error) => return fail("RUN_CODE_RANDOM_FAILED", error.to_string()),
        };
        kernel.awaiting = Some(request_id.clone());
        let request = format!("{}\n", json!({"id":request_id,"code":code}));
        if let Err(error) = self
            .supervisor
            .pty_write(&kernel.id, request.as_bytes())
            .await
        {
            return fail("RUN_CODE_KERNEL_IO", error);
        }
        match self.wait(kernel, WAIT_CAP).await {
            Ok(true) => {
                guard.disarm();
                if kernel.completed_error {
                    fail("RUN_CODE_EXECUTION_FAILED", kernel.output.text())
                } else {
                    self.success(language, kernel)
                }
            }
            Ok(false) => {
                guard.disarm();
                fail("RUN_CODE_TIMEOUT", format!("30秒等待已到，内核仍在执行，未确认完成；kernelId={}；请 status/restart，不重发代码。部分输出：{}",kernel.id,kernel.output.text()))
            }
            Err(error) => fail("RUN_CODE_KERNEL_DEAD", error),
        }
    }
}

impl ToolExecutorPort for RunCodeExecutor {
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
    access: Arc<ResourceAccess>,
    cwd: PathBuf,
    supervisor: Arc<ProcessSupervisor>,
    _clock: Arc<dyn ServiceClock>,
    budget: &SchemaBudget,
) -> Result<(), String> {
    let receipt=registry.register(ToolManifest {
        origin:ToolOrigin::FirstParty, local_name:"run_code".into(), display_name:"run_code".into(), aliases:vec![], version:"1.0.0".into(),
        description:"Run Python3 or Node.js code in a persistent per-agent/session interpreter. Variables, imports and functions survive calls. run executes code; status reports actual live/busy/dead state; restart stops the old kernel and creates fresh state (old variables are lost). Missing languages and timeout are explicit errors. A cancelled invocation stops its real interpreter process group. Output is bounded head/tail; use print()/console.log().".into(),
        input_schema:ToolSchemaDocument {dialect:"json-schema/2020-12".into(),schema:json!({"type":"object","properties":{"action":{"type":"string","enum":["run","status","restart"]},"language":{"type":"string","minLength":1},"code":{"type":"string","maxLength":524288}},"required":["language","code"],"additionalProperties":false})},
        output_schema:None, permission:PermissionContract{kind:PermissionKind::ArgumentAwareRunCode,capability_base:"run_code.run".into()},availability:Availability::Available,timeout_ms:Some(45_000),max_concurrency:None,declared_permission:DeclaredPermission::Execute,recovery:ToolRecoveryCapability::CONSERVATIVE,
    },budget).map_err(|error|format!("run_code注册失败：{error}"))?;
    gateway.bind_executor_with_resources(
        receipt.target_id,
        Arc::new(RunCodeExecutor {
            supervisor,
            access: access.clone(),
            cwd: cwd.clone(),
            kernels: Mutex::new(HashMap::new()),
        }),
        Some(ProcessTools::resource_extractor(access, cwd)),
        "真实持久Python/Node解释器，统一网关授权、会话隔离与进程组取消",
    );
    Ok(())
}

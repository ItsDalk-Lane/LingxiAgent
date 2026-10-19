//! R07-T07-δ8 workflow JS 沙箱（现役 `lib/workflow/sandbox.ts` + `meta.ts` 的端口）。
//!
//! 引擎 = boa_engine 0.20（纯 Rust，选型与探针验证见 T07_STATUS.md δ8 设计节）。
//! 每 run 一个专用 OS 线程（`tokio::task::spawn_blocking`）+ current_thread tokio
//! runtime——boa Context !Send，与主运行时经 tokio 通道通信（HostRequest/
//! oneshot 回包）。定制 TokioJobQueue：promise job 同步排空；FutureJob(!Send) 入
//! FuturesUnordered 在当前任务驱动（boa 默认 SimpleJobQueue 的 pollster 会在
//! current_thread runtime 上死锁——探针已复现）。
//!
//! 与现役的对应与登记：
//! - meta 提取：正则定位 export const meta + matchBrace（三种字符串字面量内花括号
//!   跳过）+ 纯净 context 求值字面量，全平移。现役 vm 的 50ms 求值超时在 boa 无
//!   对应参数；以有限循环/递归/栈额度拒绝失控求值，避免占住服务工作线程。
//! - normalizeExports：export default→const __wf_default + 尾调 __wf_default(__wf_api)；
//!   其余 export const/let/var/function/class/async 剥前缀。regex crate 无 lookahead，
//!   以捕获组等价改写（语义逐点对齐）。
//! - 沙箱语义：只注入 __native_* 基元与 __wf_* 数据（boa realm 固有内建对象在，
//!   与现役 vm context 固有内建同位；禁网/禁 fs 靠不注入 require/process/fs/net，
//!   同现役机制）；Math.random/Date.now 覆写抛错（逐字文案）；脚本包 async IIFE。
//! - 同步 eval 阶段的 deadline：现役 vm runInContext 的 timeout 参数只覆盖同步阶段，
//!   boa 使用有限循环/递归/栈额度；同步死循环和异步续体同样退出并回收专用线程。
//! - 宿主 API 组装（agent/parallel/pipeline/workflow/phase/log/budget/args、await 强制
//!   的 Proxy 陷阱、unawaited 记账）在 JS_BOOTSTRAP 里逐行平移现役 host-api.ts 语义；
//!   原生侧只暴露 __native_* 基元（JSON 字符串信封进出）。
//! - raceDeadline → 驱动循环 tokio::select!（scope.cancelled / sleep(deadline) /
//!   run_jobs_async 静默收尾），文案逐字。

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use boa_engine::builtins::promise::PromiseState;
use boa_engine::job::{FutureJob, JobQueue, NativeJob};
use boa_engine::object::builtins::JsPromise;
use boa_engine::object::FunctionObjectBuilder;
use boa_engine::{Context, JsArgs, JsResult, JsValue, NativeFunction, Source};
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use serde_json::Value;

use crate::cancel::CancelScope;

/// Tokio-aware job queue（探针验证形态）：promise job 同步排空；原生异步
/// future 在当前任务上并发驱动直至队列静默。
#[derive(Default)]
struct TokioJobQueue {
    promise_jobs: RefCell<VecDeque<NativeJob>>,
    futures: RefCell<FuturesUnordered<FutureJob>>,
    root_promise: RefCell<Option<JsPromise>>,
}

impl JobQueue for TokioJobQueue {
    fn enqueue_promise_job(&self, job: NativeJob, _: &mut Context) {
        self.promise_jobs.borrow_mut().push_back(job);
    }

    fn enqueue_future_job(&self, future: FutureJob, _: &mut Context) {
        self.futures.borrow_mut().push(future);
    }

    fn run_jobs(&self, context: &mut Context) {
        loop {
            let job = self.promise_jobs.borrow_mut().pop_front();
            match job {
                Some(job) => {
                    if job.call(context).is_err() {
                        self.promise_jobs.borrow_mut().clear();
                        return;
                    }
                }
                None => return,
            }
        }
    }

    fn run_jobs_async<'a, 'ctx, 'fut>(
        &'a self,
        context: &'ctx mut Context,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + 'fut>>
    where
        'a: 'fut,
        'ctx: 'fut,
    {
        Box::pin(async move {
            loop {
                loop {
                    let job = self.promise_jobs.borrow_mut().pop_front();
                    match job {
                        Some(job) => {
                            if job.call(context).is_err() {
                                self.promise_jobs.borrow_mut().clear();
                                return;
                            }
                        }
                        None => break,
                    }
                }
                // 根 Promise 已失败时，不能继续等另一个永不结束的节点。
                // 退出后编排器会取消并等待这些节点的真实清理。
                if self
                    .root_promise
                    .borrow()
                    .as_ref()
                    .is_some_and(|promise| !matches!(promise.state(), PromiseState::Pending))
                {
                    return;
                }
                let completed = {
                    // 不跨 await 持 RefCell borrow：整个 FuturesUnordered
                    // 换出驱动、完成后换回。单线程队列——await 期间不存在
                    // 入队路径（入队只发生在同步 JS 执行里），换回不丢任务。
                    let mut driving = std::mem::take(&mut *self.futures.borrow_mut());
                    if driving.is_empty() {
                        return;
                    }
                    let completed = driving.next().await;
                    let mut slot = self.futures.borrow_mut();
                    debug_assert!(slot.is_empty());
                    *slot = driving;
                    drop(slot);
                    completed
                };
                if let Some(native_job) = completed {
                    self.enqueue_promise_job(native_job, context);
                }
            }
        })
    }
}

/// 沙箱没有文件或网络能力；同步计算也必须有界，否则异步取消无从接管。
/// 同一限制应用于 meta、脚本正文和 Promise 续体，超限由 Boa 返回错误。
fn bound_runtime(ctx: &mut Context) {
    let limits = ctx.runtime_limits_mut();
    limits.set_loop_iteration_limit(100_000);
    limits.set_recursion_limit(128);
    limits.set_stack_size_limit(4096);
}

/// 提取出的 workflow meta（name/description 已校验必填——description 的
/// 「读取」即构造时的必填校验本身；现役 extractMeta 同样只校验不消费）。
#[derive(Debug, Clone)]
pub(crate) struct WorkflowMeta {
    pub name: String,
    #[allow(dead_code)]
    pub description: String,
    /// 完整字面量求值结果（declarativeNodesUnsupported 等字段检查用）。
    pub raw: Value,
}

/// 从 start 处的 `{` 找配对 `}`（跳过三种字符串字面量内的花括号）。
/// 未闭合返回 None（现役 -1）。
fn match_brace(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0u32;
    let mut in_str: Option<u8> = None;
    let mut i = start;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(quote) = in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == quote {
                in_str = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => in_str = Some(c),
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// 归一化 meta 之外的 export（现役 normalizeExports 逐点平移）。
fn normalize_exports(body: &str) -> String {
    // 只改声明，不改字符串内携带的子脚本和注释；原文偏移保持不变。
    let bytes = body.as_bytes();
    let mut code = bytes.to_vec();
    let mut quote = None;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(delimiter) = quote {
            code[index] = b' ';
            if byte == b'\\' {
                if index + 1 < bytes.len() {
                    code[index + 1] = b' ';
                }
                index += 2;
                continue;
            }
            if byte == delimiter {
                quote = None;
            }
        } else if line_comment {
            if matches!(byte, b'\n' | b'\r') {
                line_comment = false;
            } else {
                code[index] = b' ';
            }
        } else if block_comment {
            code[index] = b' ';
            if byte == b'*' && bytes.get(index + 1) == Some(&b'/') {
                code[index + 1] = b' ';
                block_comment = false;
                index += 2;
                continue;
            }
        } else if matches!(byte, b'"' | b'\'' | b'`') {
            quote = Some(byte);
            code[index] = b' ';
        } else if byte == b'/' && matches!(bytes.get(index + 1), Some(b'/' | b'*')) {
            line_comment = bytes[index + 1] == b'/';
            block_comment = !line_comment;
            code[index] = b' ';
            code[index + 1] = b' ';
            index += 2;
            continue;
        }
        index += 1;
    }
    let code = std::str::from_utf8(&code).expect("只将完整字符串与注释替换为空格");
    let default_re = regex::Regex::new(r"\bexport\s+default\s+").expect("static regex");
    let named_re = regex::Regex::new(r"\bexport\s+((?:const|let|var|function|class|async)\b)")
        .expect("static regex");
    let mut edits = Vec::new();
    if let Some(found) = default_re.find(code) {
        edits.push((found.start(), found.end(), "const __wf_default = "));
    }
    let has_default = !edits.is_empty();
    for captures in named_re.captures_iter(code) {
        let declaration = captures.get(1).expect("固定声明分组");
        edits.push((captures.get(0).unwrap().start(), declaration.start(), ""));
    }
    edits.sort_by_key(|edit| edit.0);
    let mut out = String::new();
    let mut copied = 0;
    for (start, end, replacement) in edits {
        out.push_str(&body[copied..start]);
        out.push_str(replacement);
        copied = end;
    }
    out.push_str(&body[copied..]);
    if has_default {
        out.push_str(
            "\n;return await (typeof __wf_default === 'function' ? __wf_default(__wf_api) : __wf_default);",
        );
    }
    out
}

/// 静态提取并校验脚本开头的 `export const meta = {...}` 字面量
/// （现役 meta.ts extractMeta 平移；错误文案逐字，boa 求值错误的消息后缀
/// 与 node:vm 不同——登记说明，前缀逐字）。
pub(crate) fn extract_meta(script: &str) -> Result<(WorkflowMeta, String), String> {
    if script.trim().is_empty() {
        return Err("workflow script 不能为空".to_string());
    }
    let marker = regex::Regex::new(r"export\s+const\s+meta\s*=").expect("static regex");
    let Some(found) = marker.find(script) else {
        return Err("workflow script 必须以 export const meta = {...} 开头".to_string());
    };
    let after = &script[found.end()..];
    let Some(brace_rel) = after.find('{') else {
        return Err("workflow meta 必须是对象字面量".to_string());
    };
    let brace_start = found.end() + brace_rel;
    let Some(brace_end) = match_brace(script, brace_start) else {
        return Err("workflow meta 对象字面量未闭合".to_string());
    };

    let literal = &script[brace_start..=brace_end];
    // 在纯净 context 求值字面量：不暴露任何宿主能力。
    let mut meta_ctx = Context::default();
    bound_runtime(&mut meta_ctx);
    let evaluated = meta_ctx
        .eval(Source::from_bytes(&format!("({literal})")))
        .map_err(|err| format!("workflow meta 不是合法对象字面量: {err}"))?;
    let raw = evaluated
        .to_json(&mut meta_ctx)
        .map_err(|err| format!("workflow meta 不是合法对象字面量: {err}"))?;
    let name = raw.get("name").and_then(Value::as_str);
    let description = raw.get("description").and_then(Value::as_str);
    let (Some(name), Some(description)) = (name, description) else {
        return Err("workflow meta 必须含 name 和 description 字符串".to_string());
    };
    let meta = WorkflowMeta {
        name: name.to_string(),
        description: description.to_string(),
        raw,
    };

    // 现役：marker 之前的原文 + marker 起把 export const meta 改 const meta
    // （replace(/export\s+const\s+meta/, ...) 第一处）。
    let const_meta_re = regex::Regex::new(r"export\s+const\s+meta").expect("static regex");
    let stripped = format!(
        "{}{}",
        &script[..found.start()],
        const_meta_re.replace(&script[found.start()..], "const meta")
    );
    Ok((meta, normalize_exports(&stripped)))
}

/// 原生侧收到的一次 agent() 节点请求（bootstrap 已做过同步形状校验；
/// 原生侧仍全量复验——脚本可直接触达 __native_*，校验以原生为准）。
pub(crate) struct AgentRequest {
    pub seq: u64,
    pub prompt: String,
    pub opts_json: String,
    /// computeKey 的 sha256 输入串：bootstrap 用真 JSON.stringify 构造
    /// （{p, o: sanitized}），与现役逐字节同形；原生只负责哈希。
    pub key_payload: String,
    pub reply: tokio::sync::oneshot::Sender<String>,
}

/// 沙箱 → 编排侧的宿主请求（通道协议）。
pub(crate) enum HostRequest {
    Agent(AgentRequest),
    /// 嵌套 workflow（限一层；子层不再注入 workflow 全局）。
    Nested {
        script: String,
        args_json: String,
        reply: tokio::sync::oneshot::Sender<String>,
    },
    /// log(message)：seq 分配 + step start/done 事件 + progress 日志。
    Log {
        message: String,
    },
    /// phase(title)：仅 progress 事件（现役 phase 不分配 nodeSeq）。
    Phase {
        title: String,
    },
    /// parallel/pipeline 的 step 事件（start/done/fail）。
    Step {
        kind: String,
        phase: String,
        seq: u64,
    },
}

/// 专用线程上的宿主状态（thread_local 持有；原生函数是 fn 指针不能捕获）。
struct HostThreadState {
    tx: tokio::sync::mpsc::UnboundedSender<HostRequest>,
    seq: Arc<AtomicU64>,
    budget_spent: Arc<AtomicU64>,
}

thread_local! {
    static WORKFLOW_HOST: RefCell<Option<HostThreadState>> = const { RefCell::new(None) };
}

fn host_state<R>(f: impl FnOnce(&HostThreadState) -> R) -> JsResult<R> {
    WORKFLOW_HOST.with(|slot| {
        let borrowed = slot.borrow();
        borrowed.as_ref().map(f).ok_or_else(|| {
            boa_engine::JsNativeError::typ()
                .with_message("workflow host is not bound on this thread")
                .into()
        })
    })
}

fn js_arg_string(args: &[JsValue], index: usize, ctx: &mut Context) -> String {
    args.get_or_undefined(index)
        .to_string(ctx)
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_default()
}

/// __native_next_seq() → number（agent 节点与 step 共享同一计数器，
/// 与现役 host-api 闭包内的 nodeSeq 等位）。
fn native_next_seq(_this: &JsValue, _args: &[JsValue], _ctx: &mut Context) -> JsResult<JsValue> {
    let seq = host_state(|state| state.seq.fetch_add(1, Ordering::Relaxed) + 1)?;
    Ok(JsValue::from(seq as f64))
}

/// __native_budget_spent() → number（快照值；每节点完成后由编排侧刷新。
/// 预算硬闸在节点派发点用实时查询，脚本内 budget 对象只是规划参考——登记注记）。
fn native_budget_spent(
    _this: &JsValue,
    _args: &[JsValue],
    _ctx: &mut Context,
) -> JsResult<JsValue> {
    let spent = host_state(|state| state.budget_spent.load(Ordering::SeqCst))?;
    Ok(JsValue::from(spent as f64))
}

fn native_log(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let message = js_arg_string(args, 0, ctx);
    host_state(|state| {
        let _ = state.tx.send(HostRequest::Log { message });
    })?;
    Ok(JsValue::undefined())
}

fn native_phase(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let title = js_arg_string(args, 0, ctx);
    host_state(|state| {
        let _ = state.tx.send(HostRequest::Phase { title });
    })?;
    Ok(JsValue::undefined())
}

fn native_step(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let kind = js_arg_string(args, 0, ctx);
    let phase = js_arg_string(args, 1, ctx);
    let seq = js_arg_string(args, 2, ctx).parse::<u64>().unwrap_or(0);
    host_state(|state| {
        let _ = state.tx.send(HostRequest::Step { kind, phase, seq });
    })?;
    Ok(JsValue::undefined())
}

/// __native_agent(seq, prompt, optsJson, keyPayload) → Promise<envelopeJson>。
fn native_agent(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> impl std::future::Future<Output = JsResult<JsValue>> + 'static {
    let seq = args
        .get_or_undefined(0)
        .to_number(ctx)
        .map(|n| n as u64)
        .unwrap_or(0);
    let prompt = js_arg_string(args, 1, ctx);
    let opts_json = js_arg_string(args, 2, ctx);
    let key_payload = js_arg_string(args, 3, ctx);
    let _ = this;
    let send = WORKFLOW_HOST.with(|slot| {
        slot.borrow().as_ref().map(|state| {
            let (reply, rx) = tokio::sync::oneshot::channel();
            let sent = state.tx.send(HostRequest::Agent(AgentRequest {
                seq,
                prompt,
                opts_json,
                key_payload,
                reply,
            }));
            (sent.is_ok(), rx)
        })
    });
    async move {
        let Some((sent, rx)) = send else {
            return Err(boa_engine::JsNativeError::typ()
                .with_message("workflow host is not bound on this thread")
                .into());
        };
        if !sent {
            return Err(boa_engine::JsNativeError::typ()
                .with_message("workflow host channel closed")
                .into());
        }
        match rx.await {
            Ok(envelope) => Ok(JsValue::from(boa_engine::js_string!(envelope.as_str()))),
            Err(_) => Err(boa_engine::JsNativeError::typ()
                .with_message("workflow host dropped the agent reply")
                .into()),
        }
    }
}

/// __native_workflow(script, argsJson) → Promise<envelopeJson>（限一层）。
fn native_workflow(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> impl std::future::Future<Output = JsResult<JsValue>> + 'static {
    let script = js_arg_string(args, 0, ctx);
    let args_json = js_arg_string(args, 1, ctx);
    let _ = this;
    let send = WORKFLOW_HOST.with(|slot| {
        slot.borrow().as_ref().map(|state| {
            let (reply, rx) = tokio::sync::oneshot::channel();
            let sent = state.tx.send(HostRequest::Nested {
                script,
                args_json,
                reply,
            });
            (sent.is_ok(), rx)
        })
    });
    async move {
        let Some((sent, rx)) = send else {
            return Err(boa_engine::JsNativeError::typ()
                .with_message("workflow host is not bound on this thread")
                .into());
        };
        if !sent {
            return Err(boa_engine::JsNativeError::typ()
                .with_message("workflow host channel closed")
                .into());
        }
        match rx.await {
            Ok(envelope) => Ok(JsValue::from(boa_engine::js_string!(envelope.as_str()))),
            Err(_) => Err(boa_engine::JsNativeError::typ()
                .with_message("workflow host dropped the nested reply")
                .into()),
        }
    }
}

/// 注入沙箱的全局数据（bootstrap 组装宿主 API 的原料）。
pub(crate) struct SandboxGlobals {
    /// 同一任务的父层与嵌套层共享编号，避免日志及活动互相覆盖。
    pub node_seq: Arc<AtomicU64>,
    pub args: Value,
    pub budget_total: Option<u64>,
    /// 父会话权限模式 wire 值（"operate"/"ask"/"read_only"），写声明闸用。
    pub parent_mode: Option<String>,
    /// false = 嵌套层：workflow() 全局抛「当前环境不支持 workflow 嵌套调用」。
    pub nesting: bool,
}

/// 现役 host-api.ts 的 JS 侧组装：agent/parallel/pipeline/workflow/log/phase/
/// budget/args + runtime contract（await 强制、result 属性陷阱、unawaited 记账）。
/// 文案逐字平移。
const JS_BOOTSTRAP: &str = r#"
(function () {
  'use strict';
  const AGENT_OPTION_KEYS = new Set(["label", "model", "agentType", "toolFilter", "access", "schema", "writeFolders", "retries"]);
  const __wf_calls = [];

  function normalizeAgentPrompt(prompt) {
    if (typeof prompt !== "string" || !prompt.trim()) {
      throw new Error("workflow agent(prompt, opts) requires a non-empty prompt string as the first argument.");
    }
    return prompt;
  }

  function scopeError(message, code) {
    const err = new Error(message);
    err.code = code;
    return err;
  }

  function assertNodeWriteFoldersShape(writeFolders, access) {
    if (writeFolders == null) return;
    if (access === "read") {
      throw scopeError('workflow agent() writeFolders declares a write scope and cannot be combined with access:"read".', "WRITE_FOLDERS_CONFLICT_WITH_READ");
    }
    if (!Array.isArray(writeFolders) || writeFolders.length === 0) {
      throw scopeError("workflow agent() writeFolders must be a non-empty array of absolute folder paths.", "WRITE_FOLDERS_INVALID");
    }
    for (const raw of writeFolders) {
      if (typeof raw !== "string" || !raw.trim() || !raw.trim().startsWith("/")) {
        throw scopeError(`workflow agent() writeFolders entries must be absolute paths; got: ${JSON.stringify(raw)}`, "WRITE_FOLDERS_INVALID");
      }
    }
  }

  function normalizeAgentOptions(rawOpts) {
    if (rawOpts == null) return {};
    if (typeof rawOpts !== "object" || Array.isArray(rawOpts)) {
      throw new Error("workflow agent(prompt, opts) 的 opts 必须是对象。");
    }
    for (const key of Object.keys(rawOpts)) {
      if (!AGENT_OPTION_KEYS.has(key)) {
        throw new Error(
          `workflow agent() unsupported option "${key}". ` +
          "Use agent(prompt, { label?, model?, agentType?, access?, writeFolders?, schema?, toolFilter?, retries? }); " +
          "put the task instructions in the first prompt argument."
        );
      }
    }
    if (rawOpts.access != null && rawOpts.access !== "read" && rawOpts.access !== "write") {
      throw new Error('workflow agent() access must be "read" or "write".');
    }
    if (rawOpts.retries != null && (!Number.isInteger(rawOpts.retries) || rawOpts.retries < 0)) {
      throw new Error("workflow agent() retries must be a non-negative integer.");
    }
    assertNodeWriteFoldersShape(rawOpts.writeFolders, rawOpts.access);
    return rawOpts;
  }

  function assertNodeWriteScopeDeclared(opts, nodeId) {
    const writeFolders = opts.writeFolders ?? null;
    const access = opts.access ?? null;
    const parentPermissionMode = __wf_parent_mode ?? null;
    if (writeFolders != null) return;
    if (access === "read") return;
    if (parentPermissionMode === "read_only") return;
    const label = typeof opts.label === "string" && opts.label ? opts.label : null;
    const nodeName = label || nodeId;
    throw scopeError(
      `workflow node "${nodeName}" is write-capable but declares no writeFolders. ` +
      'Every node must either run read-only (access:"read") or declare writeFolders: ["/abs/folder", ...] — ' +
      "the narrowest existing folders the task writes to (the first entry becomes the node cwd). " +
      "Re-dispatch the workflow with the fixed script and pass resumeFromRunId to reuse completed nodes.",
      "WRITE_FOLDERS_REQUIRED"
    );
  }

  function trackAgentCall(meta, start) {
    const record = Object.assign({}, meta, { consumed: false });
    __wf_calls.push(record);
    let promise = null;
    const consume = () => {
      record.consumed = true;
      if (!promise) {
        promise = Promise.resolve().then(start);
      }
      return promise;
    };
    return new Proxy(Object.create(null), {
      get(_target, prop) {
        if (prop === "then") return (onFulfilled, onRejected) => consume().then(onFulfilled, onRejected);
        if (prop === "catch") return (onRejected) => consume().catch(onRejected);
        if (prop === "finally") return (onFinally) => consume().finally(onFinally);
        if (prop === Symbol.toStringTag) return "Promise";
        if (prop === "result") {
          throw new Error(
            'workflow agent() returns a Promise. Use "const result = await agent(...)" before reading the result; ' +
            "do not use agent(...).result."
          );
        }
        throw new Error(
          `workflow agent() returns a Promise. Await agent("${record.promptPreview}") before reading property "${String(prop)}".`
        );
      },
    });
  }

  globalThis.__wf_assert_no_unawaited = function () {
    const unawaited = __wf_calls.filter((call) => !call.consumed);
    if (!unawaited.length) return;
    const first = unawaited[0];
    throw new Error(
      `workflow agent() call ${first.nodeId} was not awaited. ` +
      'Use `const result = await agent("task prompt", { access, agentType })`; ' +
      "agent() does not return { result }."
    );
  };

  function agent(prompt, rawOpts) {
    const normalizedPrompt = normalizeAgentPrompt(prompt);
    const opts = normalizeAgentOptions(rawOpts === undefined ? {} : rawOpts);
    const seq = __native_next_seq();
    const nodeId = `node-${seq}`;
    assertNodeWriteScopeDeclared(opts, nodeId);
    const sanitized = {};
    for (const key of Object.keys(opts)) {
      const v = opts[key];
      if (typeof v === "function") continue;
      if (key === "signal" || key === "onSessionReady") continue;
      sanitized[key] = v;
    }
    const keyPayload = JSON.stringify({ p: normalizedPrompt, o: sanitized });
    return trackAgentCall(
      { nodeId, promptPreview: normalizedPrompt.slice(0, 80) },
      async () => {
        const envelope = JSON.parse(await __native_agent(seq, normalizedPrompt, JSON.stringify(opts), keyPayload));
        if (!envelope.ok) {
          const err = new Error(envelope.error || "agent failed");
          if (envelope.code) err.code = envelope.code;
          throw err;
        }
        return envelope.result;
      }
    );
  }

  async function parallel(thunks) {
    const seq = __native_next_seq();
    __native_step("parallel", "start", seq);
    try {
      const result = await Promise.all((thunks || []).map((thunk) =>
        Promise.resolve().then(thunk).catch(() => null)
      ));
      __native_step("parallel", "done", seq);
      return result;
    } catch (err) {
      __native_step("parallel", "fail", seq);
      throw err;
    }
  }

  async function pipeline(items, ...stages) {
    const seq = __native_next_seq();
    __native_step("pipeline", "start", seq);
    try {
      const result = await Promise.all((items || []).map(async (item, index) => {
        let cur = item;
        for (const stage of stages) {
          try { cur = await stage(cur, item, index); }
          catch { return null; }
        }
        return cur;
      }));
      __native_step("pipeline", "done", seq);
      return result;
    } catch (err) {
      __native_step("pipeline", "fail", seq);
      throw err;
    }
  }

  async function workflow(script, childArgs) {
    if (!__wf_nesting) {
      throw new Error("当前环境不支持 workflow 嵌套调用");
    }
    const envelope = JSON.parse(await __native_workflow(script, JSON.stringify(childArgs ?? null)));
    if (!envelope.ok) throw new Error(envelope.error || "nested workflow failed");
    return envelope.result;
  }

  function log(message) {
    // 现役 log() 同样消耗 nodeSeq（step 条目）——序号与 agent 节点共享同一
    // 计数器是 journal 跨引擎 resume 的对齐前提，不能只发事件不占号。
    __native_next_seq();
    __native_log(String(message));
  }

  function phase(title) {
    __native_phase(String(title));
  }

  const budget = {
    total: __wf_budget_total,
    spent: () => __native_budget_spent(),
    remaining: () => (__wf_budget_total == null ? Infinity : Math.max(0, __wf_budget_total - __native_budget_spent())),
  };

  const args = __wf_args;

  const api = { agent, parallel, pipeline, workflow, log, phase, budget, args };
  globalThis.agent = agent;
  globalThis.parallel = parallel;
  globalThis.pipeline = pipeline;
  globalThis.workflow = workflow;
  globalThis.log = log;
  globalThis.phase = phase;
  globalThis.budget = budget;
  globalThis.args = args;
  globalThis.__wf_api = api;
})();
"#;

/// 脚本运行结果：meta + 兑现值（reject/中止/超时经 Err 上抛，文案逐字）。
#[derive(Debug)]
pub(crate) struct ScriptRun {
    /// 调用方在派单时已提取过 meta（编排侧只用 result）；保留以完整表达
    /// 现役 runWorkflowScript 的 {meta, result} 产物形状。
    #[allow(dead_code)]
    pub meta: WorkflowMeta,
    pub result: Value,
}

/// 在专用线程上跑脚本（spawn_blocking 的工作体）。发送侧通道由编排任务持有。
///
/// `deadline_ms` 是兜底（现役语义：正常路径由 abort 收场，deadline 只防脚本
/// promise 连 abort 都唤不醒的极端情况）。
pub(crate) fn run_workflow_script(
    script: &str,
    globals: SandboxGlobals,
    tx: tokio::sync::mpsc::UnboundedSender<HostRequest>,
    budget_spent: Arc<AtomicU64>,
    scope: CancelScope,
    deadline_ms: u64,
) -> Result<ScriptRun, String> {
    let (meta, body) = extract_meta(script)?;
    if scope.is_cancelled() {
        return Err(format!("workflow \"{}\" 被中止", meta.name));
    }
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => return Err(format!("workflow 线程运行时创建失败: {err}")),
    };
    let body = body.to_string();
    let name = meta.name.clone();
    let result = rt.block_on(async move {
        drive_context(&name, &body, globals, tx, budget_spent, &scope, deadline_ms).await
    });
    match result {
        Ok(value) => Ok(ScriptRun {
            meta,
            result: value,
        }),
        Err(message) => Err(message),
    }
}

async fn drive_context(
    name: &str,
    body: &str,
    globals: SandboxGlobals,
    tx: tokio::sync::mpsc::UnboundedSender<HostRequest>,
    budget_spent: Arc<AtomicU64>,
    scope: &CancelScope,
    deadline_ms: u64,
) -> Result<Value, String> {
    let queue = Rc::new(TokioJobQueue::default());
    let mut ctx = boa_engine::context::ContextBuilder::new()
        .job_queue(queue.clone())
        .build()
        .map_err(|err| format!("workflow 沙箱创建失败: {err}"))?;
    bound_runtime(&mut ctx);

    WORKFLOW_HOST.with(|slot| {
        *slot.borrow_mut() = Some(HostThreadState {
            tx,
            seq: Arc::clone(&globals.node_seq),
            budget_spent,
        });
    });
    let _host_guard = scopeguard_guard();

    // 注入 __wf_* 数据全局。
    let args_js = JsValue::from_json(&globals.args, &mut ctx)
        .map_err(|err| format!("workflow args 注入失败: {err}"))?;
    ctx.register_global_property(
        boa_engine::js_string!("__wf_args"),
        args_js,
        boa_engine::property::Attribute::all(),
    )
    .map_err(|err| format!("workflow 沙箱注入失败: {err}"))?;
    let budget_total_js = match globals.budget_total {
        Some(total) => JsValue::from(total as f64),
        None => JsValue::null(),
    };
    ctx.register_global_property(
        boa_engine::js_string!("__wf_budget_total"),
        budget_total_js,
        boa_engine::property::Attribute::all(),
    )
    .map_err(|err| format!("workflow 沙箱注入失败: {err}"))?;
    let parent_mode_js = match &globals.parent_mode {
        Some(mode) => JsValue::from(boa_engine::js_string!(mode.as_str())),
        None => JsValue::null(),
    };
    ctx.register_global_property(
        boa_engine::js_string!("__wf_parent_mode"),
        parent_mode_js,
        boa_engine::property::Attribute::all(),
    )
    .map_err(|err| format!("workflow 沙箱注入失败: {err}"))?;
    ctx.register_global_property(
        boa_engine::js_string!("__wf_nesting"),
        JsValue::from(globals.nesting),
        boa_engine::property::Attribute::all(),
    )
    .map_err(|err| format!("workflow 沙箱注入失败: {err}"))?;

    // __native_* 基元。
    register_sync_fn(&mut ctx, "__native_next_seq", 0, native_next_seq)?;
    register_sync_fn(&mut ctx, "__native_budget_spent", 0, native_budget_spent)?;
    register_sync_fn(&mut ctx, "__native_log", 1, native_log)?;
    register_sync_fn(&mut ctx, "__native_phase", 1, native_phase)?;
    register_sync_fn(&mut ctx, "__native_step", 3, native_step)?;
    register_async_fn(&mut ctx, "__native_agent", 4, native_agent)?;
    if globals.nesting {
        register_async_fn(&mut ctx, "__native_workflow", 2, native_workflow)?;
    }

    // bootstrap 组装宿主 API。
    if let Err(err) = ctx.eval(Source::from_bytes(JS_BOOTSTRAP)) {
        return Err(format!("workflow 沙箱 bootstrap 失败: {err}"));
    }

    // 包成 async IIFE：脚本内 await/return 合法；前缀禁非确定性 API（逐字）。
    let wrapped = format!(
        "(async () => {{\n'use strict';\nconst __nd = () => {{ throw new Error('workflow 脚本内禁止非确定性 API（Math.random/Date.now）'); }};\nMath.random = __nd; Date.now = __nd;\n{body}\n}})()"
    );

    let value = match ctx.eval(Source::from_bytes(&wrapped)) {
        Ok(value) => value,
        Err(err) => {
            return Err(format!(
                "workflow \"{name}\" 脚本错误: {}",
                js_error_message(&err, &mut ctx)
            ));
        }
    };
    let Some(obj) = value.as_object().cloned() else {
        return Err(format!(
            "workflow \"{name}\" 脚本错误: script did not return a promise"
        ));
    };
    let promise = JsPromise::from_object(obj)
        .map_err(|err| format!("workflow \"{name}\" 脚本错误: {err}"))?;
    *queue.root_promise.borrow_mut() = Some(promise.clone());

    // raceDeadline：abort / 兜底 deadline / 队列静默三者赛跑。
    // 静默路径继续用 ctx——把 pin 作用域收进块内，出块即放下可变借用。
    {
        let drive = queue.run_jobs_async(&mut ctx);
        tokio::pin!(drive);
        tokio::select! {
            _ = scope.cancelled() => {
                return Err(format!("workflow \"{name}\" 被中止"));
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(deadline_ms)) => {
                return Err(format!("workflow \"{name}\" 超时（{deadline_ms}ms）"));
            }
            _ = &mut drive => {}
        }
    }

    match promise.state() {
        PromiseState::Fulfilled(value) => {
            // 兑现后查 unawaited（现役只在成功路径断言；断言抛错即整体失败）。
            if let Err(err) = ctx.eval(Source::from_bytes("__wf_assert_no_unawaited()")) {
                return Err(js_error_message(&err, &mut ctx));
            }
            value
                .to_json(&mut ctx)
                .map_err(|err| format!("workflow result is not JSON-serializable: {err}"))
        }
        PromiseState::Rejected(reason) => Err(js_value_message(&reason, &mut ctx)),
        PromiseState::Pending => Err(format!(
            "workflow \"{name}\" 脚本错误: promise pending after job queue quiescence"
        )),
    }
}

/// thread_local 清场守卫（线程归还 blocking 池前摘除宿主绑定）。
fn scopeguard_guard() -> impl Drop {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            WORKFLOW_HOST.with(|slot| {
                *slot.borrow_mut() = None;
            });
        }
    }
    Guard
}

fn register_sync_fn(
    ctx: &mut Context,
    name: &'static str,
    length: usize,
    f: fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>,
) -> Result<(), String> {
    let obj = FunctionObjectBuilder::new(ctx.realm(), NativeFunction::from_fn_ptr(f))
        .name(boa_engine::js_string!(name))
        .length(length)
        .build();
    ctx.register_global_property(
        boa_engine::js_string!(name),
        obj,
        boa_engine::property::Attribute::all(),
    )
    .map_err(|err| format!("workflow 沙箱注入失败: {err}"))
}

fn register_async_fn<Fut>(
    ctx: &mut Context,
    name: &'static str,
    length: usize,
    f: fn(&JsValue, &[JsValue], &mut Context) -> Fut,
) -> Result<(), String>
where
    Fut: std::future::Future<Output = JsResult<JsValue>> + 'static,
{
    let obj = FunctionObjectBuilder::new(ctx.realm(), NativeFunction::from_async_fn(f))
        .name(boa_engine::js_string!(name))
        .length(length)
        .build();
    ctx.register_global_property(
        boa_engine::js_string!(name),
        obj,
        boa_engine::property::Attribute::all(),
    )
    .map_err(|err| format!("workflow 沙箱注入失败: {err}"))
}

/// 拒绝值 → 错误文本：Error 对象取 .message，其余取字符串化
/// （现役 err?.message || String(err) 语义）。
fn js_value_message(value: &JsValue, ctx: &mut Context) -> String {
    if let Some(obj) = value.as_object() {
        if let Ok(message) = obj.get(boa_engine::js_string!("message"), ctx) {
            if let Some(text) = message.as_string() {
                return text.to_std_string_escaped();
            }
        }
    }
    value
        .to_string(ctx)
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_else(|_| "unknown script error".to_string())
}

fn js_error_message(err: &boa_engine::JsError, ctx: &mut Context) -> String {
    // 原生错误（语法/类型）取 message 字段；opaque 值退回 Display。
    match err.try_native(ctx) {
        Ok(native) => native.message().to_string(),
        Err(_) => err.to_string(),
    }
}

// ── 测试（沙箱 + bootstrap + 宿主桥的端到端烟测，不依赖服务栈） ──────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn globals(args: Value, nesting: bool) -> SandboxGlobals {
        SandboxGlobals {
            node_seq: Arc::new(AtomicU64::new(0)),
            args,
            budget_total: Some(10),
            parent_mode: Some("operate".to_string()),
            nesting,
        }
    }

    /// 在 blocking 线程起脚本，主侧用现成 runtime 消费宿主请求。
    async fn drive(
        script: &str,
        globals: SandboxGlobals,
        answer: impl Fn(HostRequest) + Send + 'static,
    ) -> Result<ScriptRun, String> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<HostRequest>();
        let scope = crate::cancel::CancelScope::background_root("test");
        let script = script.to_string();
        let spent = Arc::new(AtomicU64::new(0));
        let join = tokio::task::spawn_blocking(move || {
            run_workflow_script(&script, globals, tx, spent, scope, 10_000)
        });
        while let Some(request) = rx.recv().await {
            answer(request);
        }
        join.await.map_err(|err| err.to_string())?
    }

    #[tokio::test]
    async fn root_failure_returns_without_waiting_for_unfinished_sibling_future() {
        let pending = Arc::new(std::sync::Mutex::new(Vec::new()));
        let held = pending.clone();
        let script = "export const meta={name:'fail-fast',description:'d'}; return await Promise.all([agent('hanging',{access:'read'}),agent('failing',{access:'read'})]);";
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            drive(script, globals(Value::Null, false), move |request| {
                if let HostRequest::Agent(request) = request {
                    if request.prompt == "hanging" {
                        held.lock().unwrap().push(request.reply);
                    } else {
                        let _ = request.reply.send(
                            serde_json::json!({"ok":false,"error":"fixture failure"}).to_string(),
                        );
                    }
                }
            }),
        )
        .await
        .expect("根Promise失败后线程必须回收，不等挂起兄弟节点");
        assert!(result.unwrap_err().contains("fixture failure"));
        assert_eq!(
            pending.lock().unwrap().len(),
            1,
            "挂起回执仍保留，证明没有靠丢弃发送者提前结束"
        );
    }

    #[tokio::test]
    async fn runaway_sync_and_promise_loops_join_their_worker() {
        for body in [
            "while (true) {}",
            "await Promise.resolve(); while (true) {}",
        ] {
            let script =
                format!("export const meta = {{ name: \"bounded\", description: \"d\" }}; {body}");
            // drive 必须等到 JoinHandle 真正退出才返回；超时不算回收成功。
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                drive(&script, globals(Value::Null, false), |_| {}),
            )
            .await
            .expect("失控脚本的工作线程必须退出");
            assert!(result.is_err(), "死循环不能报告成功");
        }
        let meta = "export const meta = {name: (() => { while (true) {} })(), description: 'd'};";
        let worker = tokio::task::spawn_blocking(move || extract_meta(meta));
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("meta 求值的工作线程必须退出")
            .expect("线程没有 panic");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn script_requires_meta_marker() {
        let err = drive("const x = 1;", globals(Value::Null, false), |_| {})
            .await
            .unwrap_err();
        assert_eq!(err, "workflow script 必须以 export const meta = {...} 开头");
    }

    #[tokio::test]
    async fn return_value_and_args_round_trip() {
        let run = drive(
            "export const meta = { name: \"t\", description: \"d\" };\n\
             export default async function ({ args, budget }) {\n\
               return { got: args.seed * 2, budget: budget.total, spent: budget.spent() };\n\
             }",
            globals(json!({ "seed": 21 }), false),
            |_| panic!("no host calls expected"),
        )
        .await
        .expect("script ok");
        // JS 数字的 serde_json 表示依赖注入路径（boa to_json 对整值 f64
        // 的映射）；数值比较即可，repr 不是契约。
        let obj = run.result.as_object().expect("object result");
        assert_eq!(obj["got"].as_f64(), Some(42.0));
        assert_eq!(obj["budget"].as_f64(), Some(10.0));
        assert_eq!(obj["spent"].as_f64(), Some(0.0));
        assert_eq!(run.meta.name, "t");
    }

    #[tokio::test]
    async fn agent_call_round_trip_and_log() {
        let saw_log = Arc::new(AtomicU64::new(0));
        let saw_log2 = Arc::clone(&saw_log);
        let run = drive(
            "export const meta = { name: \"t\", description: \"d\" };\n\
             export default async function ({ agent, log }) {\n\
               log(\"hi\");\n\
               const r = await agent(\"do thing\", { access: \"read\" });\n\
               return { got: r };\n\
             }",
            globals(Value::Null, false),
            move |request| match request {
                HostRequest::Agent(agent) => {
                    assert_eq!(agent.seq, 2); // log() 先占 1 号（与现役同计数器）
                    assert_eq!(agent.prompt, "do thing");
                    assert!(agent.key_payload.contains("\"p\":\"do thing\""));
                    let _ = agent
                        .reply
                        .send("{\"ok\":true,\"result\":\"R42\"}".to_string());
                }
                HostRequest::Log { message } => {
                    assert_eq!(message, "hi");
                    saw_log2.fetch_add(1, Ordering::Relaxed);
                }
                _ => panic!("unexpected request"),
            },
        )
        .await
        .expect("script ok");
        assert_eq!(run.result, json!({ "got": "R42" }));
        assert_eq!(saw_log.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn parallel_fan_out_consumes_replies_in_order() {
        let run = drive(
            "export const meta = { name: \"t\", description: \"d\" };\n\
             export default async function ({ agent, parallel }) {\n\
               return await parallel([\n\
                 () => agent(\"a\", { access: \"read\" }),\n\
                 () => agent(\"b\", { access: \"read\" }),\n\
               ]);\n\
             }",
            globals(Value::Null, false),
            |request| {
                if let HostRequest::Agent(agent) = request {
                    let result = format!("R{}", agent.seq);
                    let _ = agent.reply.send(
                        serde_json::to_string(&json!({ "ok": true, "result": result })).unwrap(),
                    );
                }
            },
        )
        .await
        .expect("script ok");
        // parallel 占 1 号 step，两个 agent 依次 2/3。
        assert_eq!(run.result, json!(["R2", "R3"]));
    }

    #[tokio::test]
    async fn export_normalization_preserves_quoted_nested_source_and_comments() {
        let child = "export const meta={name:'子',description:'d'}; export default async function(){return 'child';}";
        let script = format!("export const meta={{name:'root',description:'d'}};\n// export default must stay a comment\nexport default async function(){{const child={}; return child;}}", serde_json::to_string(child).unwrap());
        let run = drive(&script, globals(Value::Null, false), |_| {})
            .await
            .unwrap();
        assert_eq!(run.result, json!(child));
        let normalized = normalize_exports("/* export default ignored */\nconst source=`export const meta={name:'x'}`; export const value=1;");
        assert!(normalized.contains("/* export default ignored */"));
        assert!(normalized.contains("`export const meta={name:'x'}`"));
        assert!(normalized.contains("const value=1"));
        assert!(!normalized.contains("__wf_default"));
    }

    #[tokio::test]
    async fn nondeterministic_apis_are_forbidden() {
        let err = drive(
            "export const meta = { name: \"t\", description: \"d\" };\n\
             export default async function () { return Math.random(); }",
            globals(Value::Null, false),
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(
            err.contains("workflow 脚本内禁止非确定性 API（Math.random/Date.now）"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn unawaited_agent_call_fails_with_trap_text() {
        let err = drive(
            "export const meta = { name: \"t\", description: \"d\" };\n\
             export default async function ({ agent }) {\n\
               agent(\"x\", { access: \"read\" });\n\
               return 1;\n\
             }",
            globals(Value::Null, false),
            |_| panic!("unawaited call must never reach the host"),
        )
        .await
        .unwrap_err();
        assert!(
            err.contains("workflow agent() call node-1 was not awaited"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn nested_workflow_rejected_when_not_nesting() {
        let err = drive(
            "export const meta = { name: \"t\", description: \"d\" };\n\
             export default async function ({ workflow }) {\n\
               return await workflow(\"export const meta = { name: \\\"c\\\", description: \\\"d\\\" };\", null);\n\
             }",
            globals(Value::Null, false),
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(err.contains("当前环境不支持 workflow 嵌套调用"), "{err}");
    }

    #[tokio::test]
    async fn agent_error_envelope_propagates_message_and_code() {
        let err = drive(
            "export const meta = { name: \"t\", description: \"d\" };\n\
             export default async function ({ agent }) {\n\
               return await agent(\"x\", { access: \"read\" });\n\
             }",
            globals(Value::Null, false),
            |request| {
                if let HostRequest::Agent(agent) = request {
                    let _ = agent.reply.send(
                        "{\"ok\":false,\"error\":\"boom\",\"code\":\"WRITE_FOLDERS_REQUIRED\"}"
                            .to_string(),
                    );
                }
            },
        )
        .await
        .unwrap_err();
        assert_eq!(err, "boom");
    }

    #[tokio::test]
    async fn scope_cancellation_aborts_waiting_script() {
        let scope = crate::cancel::CancelScope::background_root("test-cancel");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<HostRequest>();
        let script_scope = scope.clone();
        let join = tokio::task::spawn_blocking(move || {
            run_workflow_script(
                "export const meta = { name: \"w\", description: \"d\" };\n\
                 export default async function ({ agent }) {\n\
                   return await agent(\"hang\", { access: \"read\" });\n\
                 }",
                globals(Value::Null, false),
                tx,
                Arc::new(AtomicU64::new(0)),
                script_scope,
                60_000,
            )
        });
        // 收到 agent 请求但不答复；取消 scope → 脚本以「被中止」收场。
        let Some(HostRequest::Agent(_agent)) = rx.recv().await else {
            panic!("expected agent request");
        };
        scope.cancel("test abort");
        let err = join.await.unwrap().unwrap_err();
        assert_eq!(err, "workflow \"w\" 被中止");
    }
}

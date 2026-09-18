/**
 * ptc-runtime.ts — PTC（程序化工具调用）沙箱运行时。
 *
 * 模型写的程序在「worker 线程 + node:vm 上下文」双层收容里执行：
 * - worker 线程负责可杀性（死循环/爆内存由 resourceLimits + terminate 兜住，
 *   主进程的事件循环永远不被程序阻塞）；
 * - vm 上下文负责可达性：程序的全局里只有 tools/console/定时器等显式安装
 *   的桥，没有 require/process/fs/net/fetch，也没有动态 import——一切 I/O
 *   必须走 tools.*，也就是走宿主那条逐个审批的工具管线。
 *
 * 这是收容而非绝对安全边界（与 deepseek-harness 同一姿态）：它挡住的是
 * 「代码顺手拿到宿主能力」，不是针对 vm 逃逸的对抗级防线。
 *
 * 通信协议（结构化克隆）：
 *   worker → host: {type:"call",id,name,args} | {type:"logs",lines[]} | {type:"done",...}
 *   host → worker: {type:"result",id,envelope}
 * 子调用结果以 envelope 回流：{ok:true,value} 在程序里 resolve 成值；
 * {ok:false,error:{toolName,message}} 在程序里抛成 vm 域内的 ToolCallError
 * （instanceof 可用——类是在 vm 域里定义的）。
 */
import { Worker } from "node:worker_threads";
import { stripTypeScriptTypes } from "node:module";

export interface PtcBudgets {
  /** 墙钟上限：子调用可能很慢（检索/网络），给足 10 分钟。 */
  wallMs: number;
  /** worker 老生代内存上限。 */
  memoryMb: number;
  /** console 输出账本（字节），超限整轮判 output-limit。 */
  maxLogBytes: number;
  /** 同时在飞的 tools.* 子调用上限。 */
  maxConcurrent: number;
}

export const PTC_DEFAULT_BUDGETS: PtcBudgets = Object.freeze({
  wallMs: 600_000,
  memoryMb: 256,
  maxLogBytes: 128_000,
  maxConcurrent: 8,
});

export type PtcFailureKind =
  | "exception"
  | "timeout"
  | "output-limit"
  | "worker-exit"
  | "invalid-output"
  | "abort";

/** 宿主侧工具派发契约：工具错误也必须 resolve（不 reject），走 error 字段。 */
export interface PtcCallEnvelopeOk { ok: true; value: unknown }
export interface PtcCallEnvelopeFail { ok: false; error: { toolName: string; message: string } }
export type PtcCallEnvelope = PtcCallEnvelopeOk | PtcCallEnvelopeFail;

export interface PtcRunRequest {
  /** 异步函数体源码；可擦除 TypeScript 语法允许（宿主侧先脱型）。 */
  code: string;
  call: (name: string, args: unknown) => Promise<PtcCallEnvelope>;
  signal?: AbortSignal;
  budgets?: Partial<PtcBudgets>;
}

export interface PtcRunSuccess { ok: true; value: unknown; logs: string[] }
export interface PtcRunFailure {
  ok: false;
  kind: PtcFailureKind;
  message: string;
  /** 程序异常时的截断栈（行号对应脱型后的源码，位置保持）。 */
  stack?: string;
  logs: string[];
}
export type PtcRunResult = PtcRunSuccess | PtcRunFailure;

/**
 * worker 引导代码（纯 JS，eval worker；不经过 TS 编译链，打包无文件路径问题）。
 * 注意：模板串里禁止出现反引号与 ${。
 */
const WORKER_BOOTSTRAP = String.raw`
"use strict";
const { parentPort, workerData } = require("node:worker_threads");
const vm = require("node:vm");
const util = require("node:util");

const port = parentPort;

// ---- console 捕获：逐行直推宿主。postMessage 出站不依赖 worker 事件循环，
// 因此同步死循环被打死之前已打印的行不会丢（25ms 批推在那种程序下永远轮不到）。
let logOverflow = false;
function fmt(value) {
  if (typeof value === "string") return value;
  try { return util.inspect(value, { depth: 4, maxArrayLength: 100, breakLength: 160 }); }
  catch (_err) { try { return String(value); } catch (_e2) { return "[unprintable value]"; } }
}
function hostLog(level, args) {
  if (logOverflow) return;
  const body = args.map(fmt).join(" ");
  try { port.postMessage({ type: "logs", lines: [level === "log" ? body : level + ": " + body] }); }
  catch (_err) { logOverflow = true; }
}
function flushLogs() { /* 逐行直推后无缓冲可冲；保留函数位防误删语义 */ }

// ---- tools.* RPC 桥 ----
let nextCallId = 1;
const pendingCalls = new Map();
port.on("message", (msg) => {
  if (!msg || msg.type !== "result") return;
  const resolve = pendingCalls.get(msg.id);
  if (!resolve) return;
  pendingCalls.delete(msg.id);
  resolve(msg.envelope);
});
function hostCall(name, args) {
  const id = nextCallId++;
  return new Promise((resolve) => {
    pendingCalls.set(id, resolve);
    try {
      port.postMessage({ type: "call", id: id, name: name, args: args });
    } catch (err) {
      pendingCalls.delete(id);
      resolve({ ok: false, error: { toolName: name, message: "arguments could not cross into the host: " + (err && err.message ? err.message : String(err)) } });
    }
  });
}

// 宿主函数只以参数身份进入 vm 域的闭包，从不挂到程序可达的全局上；
// 程序能摸到的函数全部是 vm 域内编译出来的（Function 构造器逃逸面因此只剩 vm 域自身）。
const installSource = "(function install(hostCall, hostLog, timers, structuredCloneHost, TextEncoderHost, TextDecoderHost, queueMicrotaskHost) {"
  + "  'use strict';"
  + "  class ToolCallError extends Error {"
  + "    constructor(message, toolName) {"
  + "      super(message || 'tool call failed');"
  + "      this.name = 'ToolCallError';"
  + "      this.toolName = toolName || null;"
  + "    }"
  + "  }"
  + "  Object.defineProperty(globalThis, 'ToolCallError', { value: ToolCallError, writable: true, configurable: true });"
  + "  const toolsProxy = new Proxy(Object.create(null), {"
  + "    get(_target, prop) {"
  + "      if (typeof prop !== 'string') return undefined;"
  + "      return function callTool(args) {"
  + "        return Promise.resolve(hostCall(prop, args === undefined || args === null ? {} : args)).then((envelope) => {"
  + "          if (!envelope || envelope.ok !== true) {"
  + "            const error = (envelope && envelope.error) || {};"
  + "            throw new ToolCallError(String(error.message || ('tool ' + JSON.stringify(prop) + ' failed')), String(error.toolName || prop));"
  + "          }"
  + "          return envelope.value;"
  + "        });"
  + "      };"
  + "    },"
  + "  });"
  + "  Object.defineProperty(globalThis, 'tools', { value: toolsProxy, writable: false, configurable: false });"
  + "  const consoleFacade = Object.create(null);"
  + "  for (const level of ['log', 'info', 'warn', 'error', 'debug']) {"
  + "    consoleFacade[level] = function () { hostLog(level, Array.prototype.slice.call(arguments)); };"
  + "  }"
  + "  Object.defineProperty(globalThis, 'console', { value: consoleFacade, writable: true, configurable: true });"
  + "  Object.defineProperty(globalThis, 'setTimeout', { value: function (fn, ms) { const rest = Array.prototype.slice.call(arguments, 2); return timers.setTimeout.apply(null, [fn, ms].concat(rest)); }, writable: true, configurable: true });"
  + "  Object.defineProperty(globalThis, 'clearTimeout', { value: function (id) { timers.clearTimeout(id); }, writable: true, configurable: true });"
  + "  Object.defineProperty(globalThis, 'setInterval', { value: function (fn, ms) { const rest = Array.prototype.slice.call(arguments, 2); return timers.setInterval.apply(null, [fn, ms].concat(rest)); }, writable: true, configurable: true });"
  + "  Object.defineProperty(globalThis, 'clearInterval', { value: function (id) { timers.clearInterval(id); }, writable: true, configurable: true });"
  + "  Object.defineProperty(globalThis, 'structuredClone', { value: structuredCloneHost, writable: true, configurable: true });"
  + "  Object.defineProperty(globalThis, 'TextEncoder', { value: TextEncoderHost, writable: true, configurable: true });"
  + "  Object.defineProperty(globalThis, 'TextDecoder', { value: TextDecoderHost, writable: true, configurable: true });"
  + "  Object.defineProperty(globalThis, 'queueMicrotask', { value: queueMicrotaskHost, writable: true, configurable: true });"
  + "})";

const context = vm.createContext(Object.create(null), { name: "run_tools" });
vm.runInContext(installSource, context, { filename: "run_tools_prelude.js" })(
  hostCall,
  hostLog,
  { setTimeout: setTimeout, clearTimeout: clearTimeout, setInterval: setInterval, clearInterval: clearInterval },
  structuredClone,
  TextEncoder,
  TextDecoder,
  queueMicrotask,
);

// 悬空 Promise 拒绝不该砸掉整轮运行（fire-and-forget 是程序的合法选择），记一行日志。
process.on("unhandledRejection", (reason) => {
  hostLog("error", ["unhandled promise rejection: " + fmt(reason)]);
});

function shortStack(err) {
  if (!err || !err.stack) return "";
  return String(err.stack).split("\n").slice(0, 8).join("\n");
}

let finished = false;
function finish(ok, payload) {
  if (finished) return;
  finished = true;
  flushLogs();
  if (ok) {
    try {
      port.postMessage({ type: "done", ok: true, value: payload });
    } catch (err) {
      try {
        port.postMessage({ type: "done", ok: false, error: {
          name: "InvalidOutputError",
          message: "the returned value cannot leave the sandbox (return JSON-compatible data): " + (err && err.message ? err.message : String(err)),
        } });
      } catch (_err) { /* 宿主已走 */ }
    }
    return;
  }
  try { port.postMessage({ type: "done", ok: false, error: payload }); } catch (_err) { /* 宿主已走 */ }
}

let script = null;
try {
  // workerData.code 已是宿主侧包装+脱型完成的完整表达式；lineOffset 把行号
  // 拨回用户源码坐标（包装头占第 1 行，用户代码从第 2 行起）。
  script = new vm.Script(workerData.code, { filename: "run_tools_program.js", lineOffset: -1 });
} catch (err) {
  finish(false, { name: (err && err.name) || "SyntaxError", message: String((err && err.message) || err), stack: shortStack(err) });
}
if (script) {
  let outcome;
  try {
    outcome = script.runInContext(context);
  } catch (err) {
    finish(false, { name: (err && err.name) || "Error", message: String((err && err.message) || err), stack: shortStack(err) });
    outcome = null;
    script = null;
  }
  if (script) {
    Promise.resolve(outcome).then(
      (value) => finish(true, value),
      (err) => finish(false, { name: (err && err.name) || "Error", message: String((err && err.message) || err), stack: shortStack(err) }),
    );
  }
}
`;

function asErrorMessage(err: unknown): string {
  if (err && typeof err === "object" && typeof (err as any).message === "string") return (err as any).message;
  return String(err);
}

/**
 * 跑一次 PTC 程序。永远 resolve（运行失败是结果里的字段，不是 Promise 拒绝），
 * 调用方把 kind/message/logs 渲染给模型自我纠正。
 */
export async function runPtcProgram(request: PtcRunRequest): Promise<PtcRunResult> {
  const budgets = { ...PTC_DEFAULT_BUDGETS, ...(request.budgets || {}) };

  // 宿主侧先包装成异步函数表达式再脱型：return 因此合法（程序语义=异步函数体），
  // 位置保持（包装头与用户首行不同行，lineOffset 在 worker 里拨回行号）；
  // 非可擦除语法（enum/namespace/参数属性）在 spawn worker 之前就成为 exception。
  let code: string;
  try {
    code = stripTypeScriptTypes(`(async () => {\n${String(request.code ?? "")}\n})()`, { mode: "strip" });
  } catch (err) {
    const isTsSyntax = (err as any)?.code === "ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX";
    return {
      ok: false,
      kind: "exception",
      message: isTsSyntax
        ? `TypeScript is limited to erasable syntax (no enums/namespaces/parameter properties): ${asErrorMessage(err)}`
        : `program does not parse: ${asErrorMessage(err)}`,
      logs: [],
    };
  }

  return await new Promise<PtcRunResult>((resolve) => {
    const logs: string[] = [];
    let logBytes = 0;
    let settled = false;
    let worker: Worker | null = null;

    const finish = (result: PtcRunResult) => {
      if (settled) return;
      settled = true;
      clearTimeout(wallTimer);
      if (request.signal) request.signal.removeEventListener("abort", onAbort);
      const stale = worker;
      worker = null;
      if (stale) { try { void stale.terminate(); } catch { /* 尽力 */ } }
      resolve(result);
    };

    let workerBootError: unknown = null;
    try {
      worker = new Worker(WORKER_BOOTSTRAP, {
        eval: true,
        env: {},
        workerData: { code, maxLogBytes: budgets.maxLogBytes },
        resourceLimits: { maxOldGenerationSizeMb: budgets.memoryMb },
      });
    } catch (err) {
      workerBootError = err;
    }
    if (!worker) {
      resolve({
        ok: false,
        kind: "worker-exit",
        message: `sandbox worker could not start: ${asErrorMessage(workerBootError)}`,
        logs,
      });
      return;
    }

    // ---- 子调用并发闸（提交序放行，完成序无关） ----
    let inFlight = 0;
    const queue: Array<Record<string, any>> = [];
    const dispatch = (msg: Record<string, any>) => {
      const id = msg.id;
      const name = String(msg.name ?? "");
      const send = (envelope: PtcCallEnvelope) => {
        const target = worker;
        if (!target) return;
        try { target.postMessage({ type: "result", id, envelope }); } catch { /* worker 已死 */ }
      };
      const done = () => {
        inFlight -= 1;
        const next = queue.shift();
        if (next) runOne(next);
      };
      inFlight += 1;
      Promise.resolve()
        .then(() => request.call(name, msg.args))
        .then((envelope) => {
          send(
            envelope && typeof envelope === "object" && typeof (envelope as any).ok === "boolean"
              ? envelope
              : { ok: true, value: envelope },
          );
        })
        .catch((err) => {
          send({ ok: false, error: { toolName: name, message: asErrorMessage(err) } });
        })
        .finally(done);
    };
    const runOne = (msg: Record<string, any>) => {
      if (inFlight >= budgets.maxConcurrent) {
        queue.push(msg);
        return;
      }
      dispatch(msg);
    };

    const wallTimer = setTimeout(() => {
      finish({
        ok: false,
        kind: "timeout",
        message: `program exceeded the ${Math.round(budgets.wallMs / 1000)}s wall-clock budget — split the work or call fewer tools per run`,
        logs,
      });
    }, budgets.wallMs);
    wallTimer.unref?.();

    const onAbort = () => {
      finish({ ok: false, kind: "abort", message: "aborted by the caller", logs });
    };
    if (request.signal) {
      if (request.signal.aborted) {
        finish({ ok: false, kind: "abort", message: "aborted by the caller", logs });
        return;
      }
      request.signal.addEventListener("abort", onAbort, { once: true });
    }

    worker.on("message", (msg: any) => {
      if (!msg || typeof msg !== "object") return;
      if (msg.type === "logs" && Array.isArray(msg.lines)) {
        for (const raw of msg.lines) {
          const line = String(raw);
          logBytes += Buffer.byteLength(line, "utf8") + 1;
          if (logBytes > budgets.maxLogBytes) {
            finish({
              ok: false,
              kind: "output-limit",
              message: `printed output exceeded the ${budgets.maxLogBytes}-byte budget — print less (summarize inside the program and return the summary)`,
              logs,
            });
            return;
          }
          logs.push(line);
        }
        return;
      }
      if (msg.type === "call") {
        runOne(msg);
        return;
      }
      if (msg.type === "done") {
        if (msg.ok === true) {
          finish({ ok: true, value: msg.value, logs });
          return;
        }
        const error = (msg.error && typeof msg.error === "object" ? msg.error : {}) as Record<string, any>;
        const name = typeof error.name === "string" ? error.name : "Error";
        finish({
          ok: false,
          kind: name === "InvalidOutputError" ? "invalid-output" : "exception",
          message: `${name}: ${typeof error.message === "string" ? error.message : "unknown error"}`,
          ...(typeof error.stack === "string" && error.stack ? { stack: error.stack } : {}),
          logs,
        });
      }
    });
    worker.on("error", (err) => {
      finish({
        ok: false,
        kind: "worker-exit",
        message: `sandbox worker crashed: ${asErrorMessage(err)}`,
        logs,
      });
    });
    worker.on("exit", (exitCode) => {
      finish({
        ok: false,
        kind: "worker-exit",
        message: `sandbox worker exited (code ${exitCode}) before the program completed`,
        logs,
      });
    });
  });
}

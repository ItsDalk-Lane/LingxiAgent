/**
 * ptc-tool.ts — run_tools：PTC（程序化工具调用）入口工具。
 *
 * 模型写一个异步 JS/TS 程序，在 ptc-runtime 的 worker+vm 沙箱里执行；程序里
 * `await tools.<name>(args)` 调本会话的全部可用工具。子调用结果只是沙箱里的
 * 值，天然不进对话上下文——回模型的只有程序 console 输出与 return 值。
 *
 * 权限零新造（本工具最关键的设计）：绑定表不是工具注册表，而是 buildTools
 * 尾部「已经过完 wrapWithSessionPermission/取消包装」的最终工具面——
 * - 直挂工具的子调用 = 调包装后工具对象的 execute；
 * - 目录（deferred）工具的子调用 = 转发包装后的 mcp_call（它的 resolver 以
 *   真实目标的能力名义呈递，未知名 fail-closed）。
 * 两条路都完整经过 安全策略→resolver→classify→review/ask→gateway，
 * 每个子调用独立审批，与模型手动调用同权；子调用 ID 合成
 * `<外层toolCallId>:ptc:<n>`，审计链可追。
 *
 * 绑定表由 core/engine.ts 在包装完成后 attach（late-bind）：工具对象必须先
 * 进包装链，包装产物才是合法绑定目标。
 */
import { Type } from "../pi-sdk/index.ts";
import { truncateHeadTail } from "../exec-command/runner.ts";
import { runPtcProgram, type PtcRunFailure } from "../ptc/ptc-runtime.ts";

export const RUN_TOOLS_TOOL_NAME = "run_tools";

/** mcp_call 是目录工具的桥；PTC 借它触达 deferred 目标（常量与 tool-catalog-bridge.ts 同步）。 */
const CATALOG_CALL_TOOL_NAME = "mcp_call";

/** 回模型的正文（日志+返回值）截断预算，与 run_code 对齐。 */
const MAX_RESULT_BYTES = 50_000;
/** 失败时附带的已捕获日志预算（足够模型自我纠正，又不爆上下文）。 */
const MAX_FAILURE_LOG_BYTES = 10_000;
/** details.subcalls 里参数摘要的字符上限。 */
const MAX_ARGS_SUMMARY_CHARS = 120;
/** 子调用错误摘要的字符上限。 */
const MAX_ERROR_SUMMARY_CHARS = 200;
/** 子调用行展开用的完整实参预算（stringify 字节数）；超出则只留摘要。 */
const MAX_SUBCALL_ARGS_BYTES = 4_000;
/** 单个子调用结果文本的落盘预算（字节），保头保尾。 */
const MAX_SUBCALL_OUTPUT_BYTES = 8_000;
/** 全部子调用结果文本的总预算（字节），防止长循环程序撑爆会话文件。 */
const MAX_SUBCALL_OUTPUT_TOTAL_BYTES = 64_000;

const RUN_TOOLS_DESCRIPTION = [
  "DEFAULT for any task that needs more than one or two tool calls, and MANDATORY when the user asks for run_tools / 编排工具 / orchestration: write ONE async JS/TS program here that does all the steps, as your FIRST tool call — never run the steps individually first or instead.",
  "Inside the program every available tool is `await tools.<name>(<args>)` — both directly loaded tools (read/write/edit/exec_command/mcp_search_tools/...) and every tool listed in the tool catalog (grep, web_search, search_memory, knowledge_*, ...). Each sub-call goes through the same permission checks as a direct call; unknown names throw.",
  "Sub-call results stay INSIDE the sandbox as plain values (a tool's text resolves to a string; a failure rejects with ToolCallError carrying err.toolName — try/catch to continue). They never enter the conversation. Only console.log(...) lines and the final `return` value come back as this call's result — print or return exactly what the conversation needs, nothing more.",
  "Use it for fan-out (Promise.all — up to 8 sub-calls run concurrently), loops over files, conditional pipelines, and condensing many reads into one small summary.",
  "Rules: code is an async function body (top-level await/return work; erasable TypeScript only — no enums/namespaces/parameter properties/import statements). No Node builtins: require/process/fs/net/fetch/dynamic-import are absent — ALL I/O goes through tools.*. Budgets: 10 min wall clock, 128 KB of printed output. Arguments must match each tool's schema — check the tool catalog manifest line, or call tools.mcp_describe_tool({ name: \"tool_name\" }) from inside the program first. run_tools cannot call itself.",
].join(" ");

export interface PtcSubCallRecord {
  seq: number;
  name: string;
  argsSummary: string;
  ok: boolean;
  ms: number;
  error?: string;
  /** 完整实参（UI 子调用行展开用）；stringify 超预算时省略，只留 argsSummary。 */
  args?: unknown;
  argsOmitted?: boolean;
  /** 子调用结果文本（UI 子调用行展开用），单条截断；程序 catch 路径没有。 */
  output?: string;
  outputTruncated?: boolean;
  /** 子调用输出总量超总预算后置位，输出不再落盘。 */
  outputOmitted?: boolean;
}

/**
 * 绑定表持有者：engine 在包装链完成后 attach 最终工具面。工具 execute 时
 * 才读取，因此创建时机可以早于包装。
 */
export interface PtcBindingHolder {
  attach(input: { tools: readonly any[] }): void;
  isAttached(): boolean;
  /** 直挂面命中（run_tools 自身永远排除）。 */
  direct(name: string): any | null;
  /** 目录桥（mcp_call）——deferred 工具的入口；不可用时为 null。 */
  catalogBridge(): any | null;
}

export function createPtcBindingHolder(): PtcBindingHolder {
  let directTools: Map<string, any> | null = null;
  return {
    attach({ tools }) {
      const map = new Map<string, any>();
      for (const tool of tools || []) {
        if (!tool || typeof tool.name !== "string" || typeof tool.execute !== "function") continue;
        if (tool.name === RUN_TOOLS_TOOL_NAME) continue;
        if (!map.has(tool.name)) map.set(tool.name, tool);
      }
      directTools = map;
    },
    isAttached: () => directTools !== null,
    direct: (name) => directTools?.get(name) ?? null,
    catalogBridge: () => directTools?.get(CATALOG_CALL_TOOL_NAME) ?? null,
  };
}

function isAbortSignal(value: unknown): value is AbortSignal {
  return !!value && typeof value === "object"
    && typeof (value as any).aborted === "boolean"
    && typeof (value as any).addEventListener === "function";
}

/** 工具结果 content → 程序内的值：单文本块解包成字符串，多块给数组，空给 null。 */
function unwrapContent(content: unknown): unknown {
  if (!Array.isArray(content)) return content ?? null;
  if (content.length === 0) return null;
  const mapped = content.map((block) => (
    block && typeof block === "object" && (block as any).type === "text" && typeof (block as any).text === "string"
      ? (block as any).text
      : block
  ));
  return mapped.length === 1 ? mapped[0] : mapped;
}

function textOfContent(content: unknown): string {
  if (!Array.isArray(content)) return typeof content === "string" ? content : "";
  return content
    .map((block) => (block && typeof block === "object" && (block as any).type === "text" && typeof (block as any).text === "string" ? (block as any).text : ""))
    .filter(Boolean)
    .join("\n");
}

function summarizeArgs(args: unknown): string {
  let text: string;
  try {
    text = JSON.stringify(args) ?? "";
  } catch {
    text = String(args);
  }
  return text.length > MAX_ARGS_SUMMARY_CHARS ? `${text.slice(0, MAX_ARGS_SUMMARY_CHARS)}…` : text;
}

/**
 * 字节级保头保尾（truncateHeadTail 的行级语义在这里不适用：子调用结果常是
 * 单行大 JSON，按行预算会把整行丢掉只剩标记）。按字节切，头尾各半，永不放空。
 */
function clipSubcallOutputText(text: string, maxBytes: number): { content: string; truncated: boolean } {
  const buf = Buffer.from(text, "utf-8");
  if (buf.length <= maxBytes) return { content: text, truncated: false };
  const half = Math.floor(maxBytes / 2);
  let headEnd = half;
  while (headEnd > 0 && (buf[headEnd] & 0xc0) === 0x80) headEnd--;
  let tailStart = buf.length - half;
  while (tailStart < buf.length && (buf[tailStart] & 0xc0) === 0x80) tailStart++;
  const omittedBytes = buf.length - headEnd - (buf.length - tailStart);
  const formatSize = (bytes: number) => (bytes < 1024 ? `${bytes}B` : `${(bytes / 1024).toFixed(1)}KB`);
  const content = `${buf.subarray(0, headEnd).toString("utf-8")}\n[... ${formatSize(omittedBytes)} omitted ...]\n${buf.subarray(tailStart).toString("utf-8")}`;
  return { content, truncated: true };
}

function renderReturnValue(value: unknown): string {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2) ?? String(value);
  } catch {
    return String(value);
  }
}

function errorCodeForKind(kind: string): string {
  return `RUN_TOOLS_${kind.replace(/-/g, "_").toUpperCase()}`;
}

export function createPtcTool(deps: { binding: PtcBindingHolder; now?: () => number }) {
  const binding = deps.binding;
  const now = deps.now || (() => Date.now());

  return {
    name: RUN_TOOLS_TOOL_NAME,
    label: "Run tools (PTC)",
    description: RUN_TOOLS_DESCRIPTION,
    parameters: Type.Object({
      code: Type.String({
        description: "Async function body (JS or erasable TS). Call tools as `await tools.<name>(args)`; console.log(...) lines and the final return value are the only things sent back.",
      }),
      description: Type.String({
        description: "5-10 word active-voice summary of what this program does (shown in the UI).",
      }),
    }),
    sessionPermission: {
      resolveInvocation: (_input: any = {}) => (
        { action: "execute", kind: "routine", capability: "run_tools.execute" }
      ),
    },
    async execute(toolCallId: string, params: any = {}, signalOrCtx?: unknown, _onUpdate?: unknown, maybeCtx?: unknown) {
      const signal = isAbortSignal(signalOrCtx) ? signalOrCtx : undefined;
      const ctx = maybeCtx && typeof maybeCtx === "object"
        ? maybeCtx
        : (!signal && signalOrCtx && typeof signalOrCtx === "object" ? signalOrCtx : {});

      const code = typeof params?.code === "string" ? params.code : "";
      if (!code.trim()) {
        return {
          isError: true,
          content: [{ type: "text", text: "run_tools: params.code is required — an async JS/TS function body that calls tools.<name>(args)" }],
          details: { errorCode: "RUN_TOOLS_CODE_REQUIRED" },
        };
      }
      const description = typeof params?.description === "string" ? params.description.trim() : "";
      if (!binding.isAttached()) {
        return {
          isError: true,
          content: [{ type: "text", text: "run_tools: tool binding table is not attached (engine wiring missing) — call tools directly instead" }],
          details: { errorCode: "RUN_TOOLS_NOT_ATTACHED" },
        };
      }

      const subcalls: PtcSubCallRecord[] = [];
      let seq = 0;
      let subcallOutputBytes = 0;
      const startedAt = now();

      // 子调用行的展开材料（完整实参 + 结果文本）有独立预算：单条截断保头保尾，
      // 总量超预算后后续记录只留摘要——中间结果进不进模型上下文的承诺不变，
      // 这些只进 details（UI 通道），但会话文件体积仍要有界。
      const attachRecordIO = (record: PtcSubCallRecord, args: unknown) => {
        let argsText = "";
        try {
          argsText = JSON.stringify(args ?? {}) ?? "";
        } catch {
          argsText = "";
        }
        if (argsText && Buffer.byteLength(argsText, "utf-8") <= MAX_SUBCALL_ARGS_BYTES) {
          record.args = args;
        } else {
          record.argsOmitted = true;
        }
      };
      const attachRecordOutput = (record: PtcSubCallRecord, text: string) => {
        if (!text) return;
        if (subcallOutputBytes >= MAX_SUBCALL_OUTPUT_TOTAL_BYTES) {
          record.outputOmitted = true;
          return;
        }
        const clipped = clipSubcallOutputText(text, MAX_SUBCALL_OUTPUT_BYTES);
        record.output = clipped.content;
        if (clipped.truncated) record.outputTruncated = true;
        subcallOutputBytes += Buffer.byteLength(clipped.content, "utf-8");
      };

      const call = async (name: string, args: unknown) => {
        const subSeq = ++seq;
        const subCallId = `${toolCallId}:ptc:${subSeq}`;
        const record: PtcSubCallRecord = {
          seq: subSeq,
          name,
          argsSummary: summarizeArgs(args ?? {}),
          ok: false,
          ms: 0,
        };
        attachRecordIO(record, args);
        const callStarted = now();
        try {
          let result: any;
          if (name === RUN_TOOLS_TOOL_NAME) {
            throw new Error("run_tools cannot call itself — no recursion; finish this program and start a new one from the conversation");
          }
          const direct = binding.direct(name);
          if (direct) {
            result = await direct.execute(subCallId, args ?? {}, signal, undefined, ctx);
          } else {
            const bridge = binding.catalogBridge();
            if (!bridge) {
              throw new Error(`unknown tool "${name}" — it is not loaded and no tool catalog bridge exists in this session`);
            }
            result = await bridge.execute(subCallId, { tool: name, arguments: args ?? {} }, signal, undefined, ctx);
          }
          record.ms = Math.max(0, now() - callStarted);
          const resultText = textOfContent(result?.content);
          if (result && typeof result === "object" && result.isError === true) {
            record.error = (resultText || `tool "${name}" returned an error`).slice(0, MAX_ERROR_SUMMARY_CHARS);
            attachRecordOutput(record, resultText);
            subcalls.push(record);
            return { ok: false as const, error: { toolName: name, message: textOfContent(result.content) || `tool "${name}" returned an error` } };
          }
          record.ok = true;
          attachRecordOutput(record, resultText);
          subcalls.push(record);
          return { ok: true as const, value: unwrapContent(result?.content) };
        } catch (err) {
          record.ms = Math.max(0, now() - callStarted);
          record.error = (err && typeof err === "object" && typeof (err as any).message === "string"
            ? (err as any).message
            : String(err)).slice(0, MAX_ERROR_SUMMARY_CHARS);
          subcalls.push(record);
          return { ok: false as const, error: { toolName: name, message: record.error } };
        }
      };

      const outcome = await runPtcProgram({ code, signal, call });
      const durationMs = Math.max(0, now() - startedAt);
      const details: Record<string, unknown> = {
        description: description || null,
        subcalls,
        durationMs,
      };

      if (outcome.ok) {
        const parts: string[] = [];
        if (outcome.logs.length > 0) parts.push(outcome.logs.join("\n"));
        if (outcome.value !== undefined) {
          parts.push(parts.length > 0 ? `--- returned ---\n${renderReturnValue(outcome.value)}` : renderReturnValue(outcome.value));
        }
        const text = truncateHeadTail(
          parts.join("\n\n").trim() || "(the program finished without printing or returning anything — only console output and the return value leave the sandbox)",
          { maxBytes: MAX_RESULT_BYTES },
        ).content;
        return { content: [{ type: "text", text }], details };
      }

      // lib 的 tsconfig 未开 strictNullChecks：ok 的 true/false 字面量会被抹成
      // boolean，联合判别失效——成功分支已 return，这里显式断言失败形状。
      const failure = outcome as PtcRunFailure;
      const captured = failure.logs.length > 0
        ? `\n\n--- captured output before the failure ---\n${truncateHeadTail(failure.logs.join("\n"), { maxBytes: MAX_FAILURE_LOG_BYTES }).content}`
        : "";
      return {
        isError: true,
        content: [{ type: "text", text: `run_tools ${failure.kind}: ${failure.message}${captured}` }],
        details: {
          ...details,
          errorCode: errorCodeForKind(failure.kind),
          ...(failure.stack ? { stack: failure.stack } : {}),
        },
      };
    },
  };
}

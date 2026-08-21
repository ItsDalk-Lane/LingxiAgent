/**
 * ModelCallObserver — 模型调用生命周期观测契约。
 *
 * 定位：统一的事实层协议，位于业务调用与 Provider 路径之间的**旁路**。
 * Observer 只能观察，不能成为业务控制器（不修改 payload、不改变重试/超时/
 * 模型选择、不写 accounting）。Usage Ledger 继续独立承担 accounting 职责；
 * 两边通过 ledger entry `metadata.modelCallId` 关联，互不替代。
 *
 * 生命周期事件（语义固定，名称即契约）：
 *
 *   logical_call_start          logical call 进入生命周期（Provider 请求之前）
 *   attempt_start               一次网络 attempt 开始（Provider retry = 新 attempt）
 *   provider_request_prepared   Provider 请求已完成构造并被观测到（不含 body）
 *   provider_response_received  Provider response 到达（不含 body/全量 headers）
 *   semantic_response_completed 语义响应解析完成（assembled message / parser 之后）
 *   attempt_error               attempt 失败（可能随后有新 attempt）
 *   logical_call_error          logical call 以错误终态结束
 *   logical_call_aborted        logical call 被调用方/用户中止（与 timeout 区分）
 *   logical_call_end            logical call 离开生命周期（恰好一次，带终态 status）
 *
 * 内容安全红线：事件不得携带 prompt/消息正文/响应正文/reasoning/tool result/
 * Authorization/API Key/Cookie/OAuth token/完整 headers/二进制媒体。
 * 允许的 content 型 metadata 只有不可逆结构信息（messageCount、toolCount、
 * hasSystemPrompt、hasImages、streaming、protocol、byteEstimate 等），
 * 字段不存在时保持 null，不制造假值（§四十）。
 *
 * 非侵入红线（§九）：observer handler 抛错、序列化失败、存储不可用都不得影响
 * 模型调用。所有事件经过 safeEmitModelCallEvent 投递，异常就地吞掉。
 */

import {
  normalizeUsageContext,
} from "./usage-context.ts";

export const MODEL_CALL_EVENT_TYPES = [
  "logical_call_start",
  "attempt_start",
  "provider_request_prepared",
  "provider_response_received",
  "semantic_response_completed",
  "attempt_error",
  "logical_call_error",
  "logical_call_aborted",
  "logical_call_end",
] as const;

export type ModelCallEventType = typeof MODEL_CALL_EVENT_TYPES[number];

export type ModelCallTerminalStatus = "ok" | "error" | "aborted";

export type ModelCallModelIdentity = {
  provider: string | null;
  modelId: string | null;
  api: string | null;
};

/** 与 usage-context 的 source 对齐；未知字段保持 "unknown"，不猜。 */
export type ModelCallSource = {
  subsystem: string;
  operation: string;
  surface: string;
  trigger: string;
  parent?: Record<string, unknown>;
  actor?: Record<string, unknown>;
};

/**
 * 与 usage attribution 同一份形状（kind/sessionId/sessionPath/conversationId/
 * conversationType/agentId/childAgentId/childSessionId/childSessionPath/taskId…），
 * 自由扩展但不接受嵌套内容载体。
 */
export type ModelCallAttribution = Record<string, unknown> & { kind: string };

export type ModelCallEvent = {
  eventType: ModelCallEventType;
  /** ISO-8601。 */
  timestamp: string;
  callId: string;
  attemptId?: string | null;
  traceId?: string | null;
  parentCallId?: string | null;
  providerRequestId?: string | null;
  model?: ModelCallModelIdentity | null;
  source?: ModelCallSource | null;
  attribution?: ModelCallAttribution | null;
  /** 终态事件（logical_call_end/error/aborted）携带最终状态。 */
  status?: ModelCallTerminalStatus | null;
  /**
   * 小型安全 metadata 包：只放不可逆结构信息（messageCount、stopReason、
   * usagePresent、attemptVisibility、errorKind…）。进入事件前统一经过
   * sanitizeModelCallDetails 机器安全门（fail closed），禁止正文与秘密。
   */
  details?: Record<string, unknown> | null;
  /**
   * 安全错误事实：name + code（内部固定错误码）+ message（仅显式 safe 标记的
   * 内部固定文案）。Provider 返回的不可信错误正文（error.message fallback
   * rawText 等）一律 message=null——缺少错误正文好过泄漏进 Observer。
   */
  error?: ModelCallErrorSummary | null;
};

/**
 * Attempt 可见度统一枚举（§五十三）：一个 attemptId 的事实精确到什么程度。
 *   exact                    Lingxi 亲眼看到这次网络请求的全部边界（自有 fetch）
 *   logical_boundary         一个逻辑边界（streamFn 调用）折叠为一次 attempt，
 *                            SDK 内部 transport retry 不可见（Pi MC-01/02/03）
 *   external_process_boundary 请求发生在外部进程内（CLI），只见进程边界
 * 禁止自由字符串；未来 Query 依赖该枚举判断数据精度。
 */
export const MODEL_CALL_ATTEMPT_VISIBILITY = [
  "exact",
  "logical_boundary",
  "external_process_boundary",
] as const;
export type ModelCallAttemptVisibility = typeof MODEL_CALL_ATTEMPT_VISIBILITY[number];

/**
 * Provider wire 可见度（§五十四）：Lingxi 对该 attempt 的 Provider 请求/响应
 * 边界究竟看到多少。opaque（CLI）与「没捕获」不是同一种缺失——不捕获时不
 * 写该字段，不伪造。
 */
export const MODEL_CALL_PROVIDER_WIRE_VISIBILITY = [
  "request_response",
  "response_only",
  "opaque",
] as const;
export type ModelCallProviderWireVisibility = typeof MODEL_CALL_PROVIDER_WIRE_VISIBILITY[number];

/**
 * Safe-message contract 标记：错误对象带此 symbol 且值为 string 时，其
 * message 才允许进入 Observer 事件。只允许标记仓库内部固定文案（如
 * "LLM returned invalid JSON (status=…)"）；任何来自 Provider 响应正文、
 * 网络对端、外部 stdout 的文本都不得标记。
 */
export const MODEL_CALL_SAFE_MESSAGE: unique symbol = Symbol.for("lingxi.modelCallSafeMessage");

export type ModelCallErrorSummary = {
  name: string | null;
  message: string | null;
  code: string | null;
};

export interface ModelCallObserver {
  handleModelCallEvent(event: ModelCallEvent): void;
}

/** 默认 observer：什么都不做。生产默认即此，观测不常驻内存（§四十二）。 */
export const NOOP_MODEL_CALL_OBSERVER: ModelCallObserver = Object.freeze({
  handleModelCallEvent() { /* no-op */ },
});

let currentObserver: ModelCallObserver = NOOP_MODEL_CALL_OBSERVER;

/**
 * 进程级 observer 注册点。默认 noop；测试/调试通过 set 注入 collector。
 * 恢复默认传 null/undefined。这不是第二套 EventBus——它是 Observer 契约的
 * 进程内注入点（对应 §十 的 in-memory/test observer 形态）。
 */
export function setModelCallObserver(observer: ModelCallObserver | null | undefined): void {
  currentObserver = typeof observer === "object" && observer !== null
    && typeof observer.handleModelCallEvent === "function"
    ? observer
    : NOOP_MODEL_CALL_OBSERVER;
}

export function getModelCallObserver(): ModelCallObserver {
  return currentObserver;
}

/**
 * 唯一的投递通道。Observer 故障（throw / 序列化失败）必须不能影响模型调用
 * （§九）。与 usage-ledger emit 同一纪律：异常就地吞掉。
 */
export function safeEmitModelCallEvent(
  observer: ModelCallObserver | null | undefined,
  event: ModelCallEvent,
): void {
  const target = observer ?? currentObserver;
  if (!target || typeof target.handleModelCallEvent !== "function") return;
  try {
    target.handleModelCallEvent(event);
  } catch {
    // Observability must never break the model request path.
  }
}

const ERROR_MESSAGE_MAX_CHARS = 1_000;
const ERROR_NAME_MAX_CHARS = 128;
const PROVIDER_REQUEST_ID_MAX_CHARS = 128;

/**
 * Provider request id 抽取：只认响应头里的常见 id 字段（allowlist），
 * 绝不记录全量 headers（§八）。不存在时返回 null，不猜（§四十）。
 * 同时接受 fetch Headers 实例和 pi-ai headersToRecord 的 plain object。
 */
const PROVIDER_REQUEST_ID_HEADERS = [
  "x-request-id",
  "request-id",
  "x-trace-id",
  "trace-id",
  "anthropic-request-id",
  "openai-request-id",
];

export function extractProviderRequestId(headers: unknown): string | null {
  if (!headers) return null;
  for (const name of PROVIDER_REQUEST_ID_HEADERS) {
    let value: unknown = null;
    if (typeof (headers as any).get === "function") {
      value = (headers as any).get(name);
    } else if (typeof headers === "object" && !Array.isArray(headers)) {
      const record = headers as Record<string, unknown>;
      value = record[name] ?? record[name.toLowerCase()];
    }
    if (typeof value === "string" && value.trim()) return sanitizeProviderRequestId(value);
  }
  return null;
}

/**
 * providerRequestId 基本边界（§五十二）：string only、trim、长度上限。
 * 异常超长值（恶意 Provider 经 x-request-id 塞内容）整体丢弃为 null，
 * 不截断保留。
 */
export function sanitizeProviderRequestId(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  if (!trimmed || trimmed.length > PROVIDER_REQUEST_ID_MAX_CHARS) return null;
  return trimmed;
}

function boundedErrorText(value: unknown, max: number): string | null {
  if (typeof value !== "string" || !value.trim()) return null;
  const trimmed = value.trim();
  return trimmed.length > max ? `${trimmed.slice(0, max)}…[truncated]` : trimmed;
}

/**
 * 错误摘要 = 安全错误事实（§七）：name（截断）+ code（内部错误码）+
 * message（仅 MODEL_CALL_SAFE_MESSAGE 显式标记的内部固定文案）。
 * Provider raw error body 的唯一入口是 error.message fallback——该链在
 * 这里被切断：无标记 → message=null。
 */
export function normalizeModelCallError(error: unknown): ModelCallErrorSummary {
  if (!error) return { name: null, message: null, code: null };
  const source = (error && typeof error === "object" ? error : {}) as Record<string, unknown>;
  const safeMessage = source[MODEL_CALL_SAFE_MESSAGE as unknown as string];
  return {
    name: boundedErrorText(source.name, ERROR_NAME_MAX_CHARS),
    message: typeof safeMessage === "string" ? boundedErrorText(safeMessage, ERROR_MESSAGE_MAX_CHARS) : null,
    code: boundedErrorText(source.code, ERROR_NAME_MAX_CHARS),
  };
}

/** 给内部固定错误文案打 safe 标记（唯一合法入口）。 */
export function markModelCallSafeMessage<T extends object>(error: T, message: string): T {
  try {
    (error as any)[MODEL_CALL_SAFE_MESSAGE] = message;
  } catch {
    // frozen/sealed 对象打不上标记 → message 保持 null（fail closed）。
  }
  return error;
}

export function normalizeModelCallIdentity(model: unknown): ModelCallModelIdentity {
  const m = (model && typeof model === "object" ? model : {}) as Record<string, any>;
  return {
    provider: textOrNull(m.provider),
    modelId: textOrNull(m.modelId ?? m.id),
    api: textOrNull(m.api),
  };
}

/**
 * UsageContext → ModelCallContext 映射（§三十二）：复用现有
 * normalizeUsageContext，不新建第二套 attribution schema。未知输入归一为
 * unknown 四元组 + kind:"unknown"，与 ledger 保持同一语义。
 */
export function modelCallFieldsFromUsageContext(usageContext: unknown): {
  source: ModelCallSource;
  attribution: ModelCallAttribution;
} {
  const normalized = normalizeUsageContext(usageContext);
  return {
    source: normalized.source as ModelCallSource,
    attribution: normalized.attribution as ModelCallAttribution,
  };
}

/**
 * Provider request payload 的安全结构摘要（§二十七）：只看形状与计数，
 * 绝不读取消息正文/system 文本/工具 schema 细节/base64 数据。
 * 覆盖各 API 形状：OpenAI chat（messages）、Responses（input）、
 * Anthropic（messages+system）、Gemini（contents+systemInstruction）。
 * 不认识的形状字段留 null，不猜。
 */
export function summarizeProviderRequestPayload(payload: unknown): Record<string, unknown> {
  const p = (payload && typeof payload === "object" ? payload : {}) as Record<string, any>;
  const messageLike = firstArray(p.messages, p.input, p.contents);
  return {
    messageCount: messageLike ? messageLike.length : null,
    toolCount: firstArray(p.tools, p.toolDeclarations, p.functions)?.length ?? null,
    hasSystemPrompt: detectSystemPrompt(p, messageLike),
    hasMedia: messageLike ? detectMediaInMessages(messageLike) : null,
    streaming: typeof p.stream === "boolean" ? p.stream : null,
  };
}

function firstArray(...values: unknown[]): any[] | null {
  for (const value of values) {
    if (Array.isArray(value)) return value;
  }
  return null;
}

function detectSystemPrompt(p: Record<string, any>, messages: any[] | null): boolean | null {
  if (typeof p.system === "string" && p.system.length > 0) return true;
  if (Array.isArray(p.system) && p.system.length > 0) return true;
  if (typeof p.instructions === "string" && p.instructions.length > 0) return true;
  if (p.systemInstruction) return true;
  if (messages?.some((m) => m?.role === "system" || m?.role === "developer")) return true;
  return messages || p.system !== undefined || p.instructions !== undefined ? false : null;
}

function detectMediaInMessages(messages: any[]): boolean {
  for (const message of messages) {
    const parts = firstArray(message?.content, message?.parts);
    if (!parts) continue;
    for (const part of parts) {
      const type = typeof part?.type === "string" ? part.type : "";
      if (/image|audio|video|file|inline_data/i.test(type)) return true;
      if (part?.inline_data || part?.inlineData) return true;
      if (part?.source?.data || part?.image_url || part?.imageUrl) return true;
    }
  }
  return false;
}

function textOrNull(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

/* ── Metadata Safety Gate（§九/§十/§十一）──────────────────────────────
 *
 * details 的安全契约不能只靠注释：所有事件在投递前统一经过本 gate，
 * 机器执行、fail closed——禁入键直接丢弃，值形状不合法直接丢弃。
 *
 * 两层防线：
 *   1. 键 denylist（大小写/分隔符归一后整键匹配）：prompt/systemPrompt/
 *      messages/message/content/text/body/rawBody/… （§十全表）。
 *      hasText/messageCount 这类布尔/计数键与被禁的 text/message 是不同
 *      的归一键，不受影响（整键匹配，不做子串猜测）。
 *   2. 值形状 gate：只放行 string（截断）/finite number/boolean/null，
 *      嵌套 plain object（深度 ≤2）/array（≤32 项）递归同规则；其余
 *      （function/symbol/bigint/超深结构）一律丢弃。
 *
 * gate 是最终防线，不是脱敏管道：调用方仍应只构造结构性 metadata
 * （typed builders = summarize* helpers）。
 */

const DENIED_DETAIL_KEYS = new Set([
  "prompt", "systemprompt", "messages", "message", "content", "text", "body",
  "rawbody", "rawresponse", "responsebody", "responsetext", "rawtext",
  "reasoning", "thinking", "toolresult", "toolschema", "headers",
  "authorization", "cookie", "apikey", "accesstoken", "refreshtoken",
  "credential", "credentials", "secret", "token", "base64", "audio", "video",
  "image", "imagedata", "imageurl", "stdout", "stderr", "commandargs", "args",
  "environment", "env", "detail", "error", "errormessage", "errortext",
  "transcription", "transcript", "payload", "request", "response", "filename",
  "filepath", "signedurl",
]);

const DETAIL_STRING_MAX_CHARS = 256;
const DETAIL_ARRAY_MAX_ITEMS = 32;
const DETAIL_KEY_MAX_CHARS = 64;

function normalizeDetailKey(key: string): string {
  return key.toLowerCase().replace(/[^a-z0-9]/g, "");
}

function sanitizeDetailValue(value: unknown, depth: number): unknown {
  if (value === null) return null;
  const type = typeof value;
  if (type === "string") {
    const text = value as string;
    return text.length > DETAIL_STRING_MAX_CHARS
      ? `${text.slice(0, DETAIL_STRING_MAX_CHARS)}…[truncated]`
      : text;
  }
  if (type === "number") return Number.isFinite(value as number) ? value : null;
  if (type === "boolean") return value;
  if (type !== "object") return undefined; // function/symbol/bigint/undefined：丢弃
  if (depth <= 0) return undefined;
  if (Array.isArray(value)) {
    const items = value
      .slice(0, DETAIL_ARRAY_MAX_ITEMS)
      .map((item) => sanitizeDetailValue(item, depth - 1))
      .filter((item) => item !== undefined);
    return items;
  }
  const nested = value as Record<string, unknown>;
  if (Object.keys(nested).length > DETAIL_ARRAY_MAX_ITEMS) return undefined;
  const out: Record<string, unknown> = {};
  for (const [key, child] of Object.entries(nested)) {
    if (!key || key.length > DETAIL_KEY_MAX_CHARS) continue;
    if (DENIED_DETAIL_KEYS.has(normalizeDetailKey(key))) continue;
    const safe = sanitizeDetailValue(child, depth - 1);
    if (safe !== undefined) out[key] = safe;
  }
  return Object.keys(out).length > 0 ? out : undefined; // 空壳（被剥空的嵌套对象）不保留
}

/**
 * 事件 details 的最终安全门：禁入键丢弃、值形状过滤、空包归 null。
 * Recorder 在每次 emit 前调用——集成点无法绕过。
 */
export function sanitizeModelCallDetails(details: unknown): Record<string, unknown> | null {
  if (!details || typeof details !== "object" || Array.isArray(details)) return null;
  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(details as Record<string, unknown>)) {
    if (!key || key.length > DETAIL_KEY_MAX_CHARS) continue;
    if (DENIED_DETAIL_KEYS.has(normalizeDetailKey(key))) continue; // fail closed
    const safe = sanitizeDetailValue(value, 2);
    if (safe !== undefined) out[key] = safe;
  }
  return Object.keys(out).length > 0 ? out : null;
}

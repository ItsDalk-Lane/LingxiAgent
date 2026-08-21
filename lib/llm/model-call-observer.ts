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
   * usagePresent、attemptVisibility、errorKind…）。禁止正文与秘密。
   */
  details?: Record<string, unknown> | null;
  /** 错误摘要：仅 name + message（截断），与 Usage Ledger 的风险口径一致。 */
  error?: { name: string | null; message: string | null } | null;
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
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return null;
}

/** 错误摘要只允许 name + 截断 message（同 Usage Ledger 口径），不保存堆栈/正文。 */
export function normalizeModelCallError(error: unknown): { name: string | null; message: string | null } {
  if (!error) return { name: null, message: null };
  const name = typeof (error as any)?.name === "string" ? (error as any).name : null;
  let message: string | null;
  if (typeof (error as any)?.message === "string") message = (error as any).message;
  else message = typeof error === "string" ? error : String(error);
  if (typeof message === "string" && message.length > ERROR_MESSAGE_MAX_CHARS) {
    message = `${message.slice(0, ERROR_MESSAGE_MAX_CHARS)}…[truncated]`;
  }
  return { name, message };
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

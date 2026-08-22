/**
 * model-trace-scope.ts — 任务级 Model Trace 上下文（AsyncLocalStorage）。
 *
 * 与 model-call-scope.ts 的分层（§十八～§二十）：
 *
 *   ModelTraceScope          生命周期 = 整个用户任务 / agent orchestration
 *     ├─ Call C1（ModelCallScope，单次 Provider 调用）
 *     │    └─ Attempt A1
 *     ├─ Call C2 …
 *     └─ Call C3 …
 *
 * 第一性原理定义（任务书 Phase 4）：
 *   - traceId = 一次具有共同因果根源的完整任务执行；不是 sessionId/conversationId/
 *     taskId/callId，一个 session 可以有很多 trace，一个 trace 可以跨多个 session。
 *   - 绝对禁止 traceId = sessionId。
 *   - parentCallId = 直接造成当前 Model Call 发生的上游 Model Call；不是"最近的"、
 *     不是"同 session 的上一条"、不按时间/数组顺序猜。无事实 → null。
 *
 * 传播机制（§十九/§三十一）：
 *   - AsyncLocalStorage 保证并发隔离与异步链传播；不用 global currentTraceId /
 *     module-level mutable parent（并发 Session 会串线）。
 *   - Agent-loop 流式调用（MC-01/02 经 stream observer）推进 scope.lastCallId；
 *     工具执行边界进入时快照 causalParentCallId = scope.lastCallId 建立子 scope
 *     （冻结），并行工具各自快照、互不覆盖（Chat C1 ├─ Vision C2 / └─ Approval C3
 *     双双 parent=C1）。
 *   - 辅助调用（callText/媒体/语音/probe/direct summary）只读、不推进 lastCallId
 *     ——数据依赖不等于触发因果（§四十八）。
 *
 * 泄漏防线（§四十九/§五十）：
 *   - runWithNewModelTrace 强制新根（覆盖外层 scope）——定时/后台任务入口必用；
 *   - runWithoutModelTrace 显式脱离；
 *   - caller 不得直接操作 ALS store（本模块是唯一入口）。
 */

import { AsyncLocalStorage } from "node:async_hooks";
import { mintModelTraceId } from "./model-call-identity.ts";

/**
 * Trace origin 有限枚举（§六十八）：任务从哪里开始。closed set，禁止自由字符串。
 * prompt 类别（memory/approval/vision/summary…）继续由 subsystem/operation 表达，
 * 不进 origin（§六十九）。
 */
export const MODEL_TRACE_ORIGINS = [
  "user_turn",
  "bridge_message",
  "phone_message",
  "slash_command",
  "automation",
  "background",
  "plugin",
  "media",
  "speech",
  "provider_probe",
  "health_check",
  "diary",
  "unknown",
] as const;
export type ModelTraceOrigin = typeof MODEL_TRACE_ORIGINS[number];

export function isModelTraceOrigin(value: unknown): value is ModelTraceOrigin {
  return typeof value === "string" && (MODEL_TRACE_ORIGINS as readonly string[]).includes(value);
}

/**
 * 任务级 Trace Scope。refs 只允许小型安全业务身份（§五十五：禁止 prompt/messages/
 * response/memory/tool result——Trace Scope 不是 Payload Store）。
 */
export type ModelTraceScope = {
  readonly traceId: string;
  readonly origin: ModelTraceOrigin;
  /** scope 建立时的直接因果上游（快照，之后不变；任务根为 null）。 */
  readonly causalParentCallId: string | null;
  /** ≤8 键、string 值 ≤128 chars 的安全业务引用（sessionId/taskId/toolCallId…）。 */
  readonly refs: Record<string, string> | null;
  /**
   * 当前异步链最近一次 agent-loop 流式调用（mutable）。仅由 stream observer
   * 推进（noteAgentStreamCallStarted）；子 scope 在建立时冻结快照，不受后续
   * 推进影响——并行分支不会互相覆盖（§三十一）。
   */
  lastCallId: string | null;
};

const MODEL_TRACE_STORAGE = new AsyncLocalStorage<ModelTraceScope>();

const REFS_MAX_KEYS = 8;
const REF_VALUE_MAX_CHARS = 128;

function sanitizeTraceRefs(refs: Record<string, unknown> | null | undefined): Record<string, string> | null {
  if (!refs || typeof refs !== "object") return null;
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(refs)) {
    if (Object.keys(out).length >= REFS_MAX_KEYS) break;
    if (typeof key !== "string" || !key || key.length > 64) continue;
    if (typeof value !== "string" || !value.trim()) continue;
    const trimmed = value.trim();
    out[key] = trimmed.length > REF_VALUE_MAX_CHARS
      ? `${trimmed.slice(0, REF_VALUE_MAX_CHARS)}…`
      : trimmed;
  }
  return Object.keys(out).length > 0 ? out : null;
}

/** 以显式 scope 进入（子 scope 建立 / 测试用）。 */
export function runWithModelTrace<T>(scope: ModelTraceScope, fn: () => T): T {
  return MODEL_TRACE_STORAGE.run(scope, fn);
}

/**
 * 强制新 trace 根（detach 语义，§五十一）：无论外层是否有 scope，都铸造新
 * traceId 并覆盖。定时/后台/调度任务入口必须用它，防止沿 ALS 继承旧任务的
 * trace（§五十：30 分钟后的 timer 回调不得仍属于 T1）。
 */
export function runWithNewModelTrace<T>(
  options: {
    origin?: ModelTraceOrigin;
    refs?: Record<string, unknown> | null;
    causalParentCallId?: string | null;
  },
  fn: () => T,
): T {
  const scope: ModelTraceScope = {
    traceId: mintModelTraceId(),
    origin: options.origin && isModelTraceOrigin(options.origin) ? options.origin : "unknown",
    causalParentCallId: typeof options.causalParentCallId === "string" && options.causalParentCallId.trim()
      ? options.causalParentCallId.trim()
      : null,
    refs: sanitizeTraceRefs(options.refs),
    lastCallId: null,
  };
  return MODEL_TRACE_STORAGE.run(scope, fn);
}

/** 显式脱离 trace 上下文（控制面/不需要归属的工作）。 */
export function runWithoutModelTrace<T>(fn: () => T): T {
  return MODEL_TRACE_STORAGE.run(undefined as unknown as ModelTraceScope, fn);
}

/** 当前异步调用链上的任务级 trace scope；未标记时为 null。 */
export function currentModelTraceScope(): ModelTraceScope | null {
  return MODEL_TRACE_STORAGE.getStore() ?? null;
}

/**
 * 顶层任务入口包装（§二十五/§二十六/§五十三）：已有 scope（嵌套业务调用，如
 * chat 工具触发的媒体/子代理）→ 原样继承；没有 → 铸新 trace 根。
 * 只该出现在真正的任务入口（user turn / inbound message / 独立请求），
 * 不是通用装饰器。
 */
export function runWithModelTraceRoot<T>(
  options: { origin?: ModelTraceOrigin; refs?: Record<string, unknown> | null },
  fn: () => T,
): T {
  if (currentModelTraceScope()) return fn();
  return runWithNewModelTrace(options, fn);
}

/**
 * 工具执行边界（§三十二/§三十三）：继承当前 traceId，把 causalParentCallId
 * 冻结为"进入工具时的 scope.lastCallId"（= 产生本次 toolCall 的那次模型调用，
 * 由 agent loop 顺序性保证：工具只能在产生它的消息组装完成后开始执行）。
 * toolCallId 是 Tool Invocation Identity，与 parentCallId（Model Call Identity）
 * 语义不同，只进 refs 安全保留（§三十四），不建 Tool Trace Store。
 *
 * 无外层 scope 时原样执行——工具外的模型调用按 singleton trace 各自铸根。
 */
export function runToolExecutionWithModelTrace<T>(
  info: { toolName?: string | null; toolCallId?: string | null },
  fn: () => T,
): T {
  const parent = currentModelTraceScope();
  if (!parent) return fn();
  const child: ModelTraceScope = {
    traceId: parent.traceId,
    origin: parent.origin,
    causalParentCallId: parent.lastCallId,
    refs: sanitizeTraceRefs({
      ...(parent.refs || {}),
      ...(info.toolName ? { toolName: info.toolName } : {}),
      ...(info.toolCallId ? { toolCallId: info.toolCallId } : {}),
    }),
    lastCallId: parent.lastCallId,
  };
  return MODEL_TRACE_STORAGE.run(child, fn);
}

/**
 * Agent-loop 流式调用开始（仅 stream observer 调用）：推进当前链的 lastCallId。
 * 同一 agent loop 顺序调用 C1→C2 时，C2 解析到 parent=C1（§二十九：同一
 * runAgentLoop 内工具链之后的继续推理，运行时链证明成立）。
 */
export function noteAgentStreamCallStarted(callId: string | null | undefined): void {
  const scope = currentModelTraceScope();
  if (!scope || typeof callId !== "string" || !callId) return;
  scope.lastCallId = callId;
}

/**
 * 统一身份解析（§二十一/§四十一/§四十二/§四十三）——所有 Model Call 创建时的
 * 唯一入口，各 caller 不得自行实现：
 *
 *   explicit trace context（caller 显式传 traceId/parentCallId）
 *     → current ModelTraceScope（parent = scope.lastCallId）
 *     → new singleton trace（traceId 现铸，parentCallId = null）
 *
 * 返回的 traceId 恒非空（§二十二：本轮完成后禁止生产 Model Call 没有 traceId；
 * 独立 Health Check/Probe/后台任务也形成单 call 的 singleton trace）。
 * 自动生成 traceId 安全；自动猜 parentCallId 不安全——无事实 → null（§二十三）。
 */
export type ResolvedModelTraceContext = {
  traceId: string;
  parentCallId: string | null;
  origin: ModelTraceOrigin | null;
  /** 身份来源（仅供测试/诊断断言，不进事件）。 */
  source: "explicit" | "trace_scope" | "singleton";
};

export function resolveModelTraceContext(
  explicit?: { traceId?: string | null; parentCallId?: string | null } | null,
): ResolvedModelTraceContext {
  const explicitTraceId = typeof explicit?.traceId === "string" && explicit.traceId.trim()
    ? explicit.traceId.trim()
    : null;
  if (explicitTraceId) {
    return {
      traceId: explicitTraceId,
      parentCallId: typeof explicit?.parentCallId === "string" && explicit.parentCallId.trim()
        && explicit.parentCallId.trim() !== explicitTraceId
        ? explicit.parentCallId.trim()
        : null,
      origin: currentModelTraceScope()?.origin ?? null,
      source: "explicit",
    };
  }
  const scope = currentModelTraceScope();
  if (scope) {
    return {
      traceId: scope.traceId,
      parentCallId: scope.lastCallId && scope.lastCallId !== scope.traceId ? scope.lastCallId : null,
      origin: scope.origin,
      source: "trace_scope",
    };
  }
  return { traceId: mintModelTraceId(), parentCallId: null, origin: null, source: "singleton" };
}

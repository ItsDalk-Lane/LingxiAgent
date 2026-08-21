/**
 * model-call-stream-observer.ts — Pi streamFunction 统一观测接点（审计 Boundary A）。
 *
 * 覆盖范围（0.84.1 实证）：
 *   - MC-01 普通 Chat / Bridge / Phone / Subagent：Agent.prompt → runAgentLoop
 *     → `this.streamFunction`（pi-agent-core agent.js:272/277）。
 *   - MC-02 cache-preserving AgentRun：Lingxi 侧 `getSessionAgentRunRuntime()`
 *     读取 `agent.streamFunction` 传给 runAgentLoop（自动经过本包装）；runner
 *     另用 ALS scope 显式标记 compaction 分类并把 callId 写进 ledger metadata。
 *   - MC-03 原生 compaction/branch summarizer：Pi `compact()` /
 *     `_runAutoCompaction()` / `generateBranchSummary()` 都把
 *     `this.agent.streamFunction` 传给 `completeSummarization()`。
 *
 * 因此包装 `agent.streamFunction` 是三条路径唯一公共边界——与
 * `stream-guard.ts` / `_installCachePrefixGuard` 同一包装先例。
 *
 * 生命周期映射（streamFn 契约：返回 AssistantMessageEventStream，provider
 * 错误经 stream 的 error/done 事件传递，`result()` 总是 resolve 终态消息）：
 *   - 包装被调用（= Provider 请求之前）→ logical_call_start + attempt_start
 *   - `before_provider_request` hook（经 ALS scope 关联）→ provider_request_prepared
 *   - `after_provider_response` hook（同 scope）→ provider_response_received
 *   - result() resolve 出 assembled message → semantic_response_completed
 *     （stopReason=error/aborted 时转为对应错误/中止终态）
 *
 * 诚实性（§四十）：
 *   - attemptVisibility: "logical_boundary"——一个 streamFn 调用对 Lingxi 是一次
 *     逻辑网络 attempt；pi-ai `retryProviderRequest` 的内部 transport retry
 *     （408/409/429/5xx/网络错误）没有 attempt hook，被折叠在这一个 attempt 里，
 *     不伪造多个 attemptId。
 *   - Pi 原生 summarizer 的请求 options 不含 onPayload（0.84.1 实证），因此
 *     MC-03 不会触发 provider_request_prepared——事件缺失即真相，不补假事件。
 */
import {
  getModelCallObserver,
  modelCallFieldsFromUsageContext,
  normalizeModelCallIdentity,
  type ModelCallAttribution,
  type ModelCallSource,
} from "../llm/model-call-observer.ts";
import { createModelCallRecorder } from "../llm/model-call-recorder.ts";
import {
  currentModelCallScope,
  runWithModelCallScope,
  type ModelCallScope,
} from "../llm/model-call-scope.ts";
import { mintModelCallId } from "../llm/model-call-identity.ts";

const INSTALLED = Symbol.for("lingxi.modelCallStreamObserver.installed");

/** 每个 session 的静态归属（由创建方注册）；per-call 分类优先走 ALS scope。 */
export type ModelCallSessionContextProvider = () => {
  source?: ModelCallSource | null;
  attribution?: ModelCallAttribution | null;
  traceId?: string | null;
  parentCallId?: string | null;
} | null;

const sessionContextProviders = new WeakMap<object, ModelCallSessionContextProvider>();

/**
 * 由 session 创建方（session-coordinator / bridge-session-manager /
 * agent-executor）在 createAgentSession 之后注册该 session 的归属上下文。
 * provider 在每次模型调用时求值，保持 cheap + 无副作用。
 */
export function registerSessionModelCallContext(
  session: unknown,
  provider: ModelCallSessionContextProvider,
): void {
  if (!session || typeof session !== "object" || typeof provider !== "function") return;
  sessionContextProviders.set(session as object, provider);
}

const UNKNOWN_CHAT_CONTEXT = {
  source: Object.freeze({
    subsystem: "session",
    operation: "reply",
    surface: "unknown",
    trigger: "unknown",
  }),
  attribution: Object.freeze({ kind: "unknown" }),
} as const;

export function installModelCallStreamObserver(
  session: any,
  contextProvider: ModelCallSessionContextProvider | null = null,
): void {
  const agent = session?.agent;
  if (!agent || typeof agent.streamFunction !== "function" || agent[INSTALLED]) return;
  if (contextProvider) registerSessionModelCallContext(session, contextProvider);
  const originalStreamFn = agent.streamFunction;
  agent[INSTALLED] = true;

  agent.streamFunction = async (model: any, context: any, options: any) => {
    const observer = getModelCallObserver();
    const modelIdentity = normalizeModelCallIdentity(model);
    const explicitScope = currentModelCallScope();
    const registered = readRegisteredContext(session, sessionContextProviders.get(session));
    const nativeSummarization = !explicitScope && session?.isCompacting === true;

    // 分类优先级：显式 ALS scope（MC-02 runner）> native summarization（MC-03）
    // > session 注册归属（MC-01 各 surface）> 诚实 unknown。
    const effectiveSource = explicitScope?.source
      ?? (nativeSummarization ? NATIVE_SUMMARIZATION_SOURCE(registered) : null)
      ?? registered?.source
      ?? UNKNOWN_CHAT_CONTEXT.source;
    const effectiveAttribution = explicitScope?.attribution
      ?? registered?.attribution
      ?? unknownAttributionWithSessionIds(session);
    const traceId = explicitScope?.traceId ?? registered?.traceId ?? null;
    const parentCallId = explicitScope?.parentCallId ?? registered?.parentCallId ?? null;
    // MC-02 runner 先铸 callId 写进 ledger metadata，这里接管同一身份（单点发射）。
    const callId = explicitScope?.callId ?? mintModelCallId();

    const recorder = createModelCallRecorder({
      observer,
      context: {
        callId,
        traceId,
        parentCallId,
        model: modelIdentity,
        source: effectiveSource,
        attribution: effectiveAttribution,
      },
    });
    recorder.beginLogicalCall({
      details: {
        path: "pi_stream",
        ...(nativeSummarization ? { nativeSummarization: true } : {}),
        ...(explicitScope?.details && typeof explicitScope.details === "object"
          ? explicitScope.details
          : {}),
      },
    });
    const attemptId = recorder.beginAttempt({
      // 一个 streamFn 调用 = 一个逻辑网络 attempt；pi-ai 传输层 retry 折叠在内。
      details: { attemptVisibility: "logical_boundary" },
    });

    const scope: ModelCallScope = {
      callId,
      attemptId,
      traceId,
      parentCallId,
      model: modelIdentity,
      source: effectiveSource,
      attribution: effectiveAttribution,
    };

    let inner: any;
    try {
      inner = await runWithModelCallScope(
        scope,
        () => originalStreamFn.call(agent, model, context, options),
      );
    } catch (error) {
      // streamFn 在返回 stream 之前抛错（罕见：sdk 闭包读 settings、凭证边界等）。
      emitTerminalError(recorder, error, "pre_stream_throw");
      throw error;
    }

    observeStreamTerminal(inner, recorder);
    return inner;
  };
}

function readRegisteredContext(session: any, provider: ModelCallSessionContextProvider | undefined) {
  if (!provider) return null;
  try {
    return provider() ?? null;
  } catch {
    return null;
  }
}

function NATIVE_SUMMARIZATION_SOURCE(registered: ReturnType<typeof readRegisteredContext>) {
  return {
    subsystem: "compaction",
    // Pi 原生 summarizer：manual/auto/branch summary 共用 isCompacting 信号，
    // 这里无法进一步区分，operation 保持通用 "compact"（见实现报告 Retry Reality）。
    operation: "compact",
    surface: registered?.source?.surface ?? "unknown",
    trigger: "unknown",
  } as ModelCallSource;
}

function unknownAttributionWithSessionIds(session: any): ModelCallAttribution {
  const sessionId = safeString(() => session?.sessionManager?.getSessionId?.());
  const sessionPath = safeString(() => session?.sessionManager?.getSessionFile?.());
  return {
    kind: "unknown",
    ...(sessionId ? { sessionId } : {}),
    ...(sessionPath ? { sessionPath } : {}),
  };
}

function safeString(read: () => unknown): string | null {
  try {
    const value = read();
    return typeof value === "string" && value.trim() ? value.trim() : null;
  } catch {
    return null;
  }
}

function observeStreamTerminal(inner: any, recorder: ReturnType<typeof createModelCallRecorder>): void {
  try {
    if (!inner || typeof inner.result !== "function") {
      // 非标准 stream：终态不可见。不伪造——logical_call_start 已经留下事实。
      recorder.endLogicalCall("error", {
        details: { terminalVisibility: "stream_result_unavailable" },
      });
      return;
    }
    Promise.resolve(inner.result()).then(
      (message: any) => {
        try {
          const stopReason = typeof message?.stopReason === "string" ? message.stopReason : null;
          if (stopReason === "aborted") {
            recorder.logicalCallAborted({ details: { stopReason } });
            recorder.endLogicalCall("aborted");
            return;
          }
          if (stopReason === "error") {
            const error = new Error(
              typeof message?.errorMessage === "string" && message.errorMessage
                ? message.errorMessage
                : "provider stream error",
            );
            recorder.attemptError(error, { details: { stopReason } });
            recorder.logicalCallError(error, { details: { stopReason } });
            recorder.endLogicalCall("error");
            return;
          }
          recorder.semanticResponseCompleted({ details: summarizeAssistantMessage(message) });
          recorder.endLogicalCall("ok");
        } catch {
          // 观测代码自身失败不得影响业务（§九）
        }
      },
      (error: unknown) => {
        // result() 按 EventStream 契约不 reject；防御性兜底。
        try {
          emitTerminalError(recorder, error, "stream_result_rejected");
        } catch { /* never break */ }
      },
    );
  } catch {
    // 观测代码自身失败不得影响业务（§九）
  }
}

function emitTerminalError(
  recorder: ReturnType<typeof createModelCallRecorder>,
  error: unknown,
  errorKind: string,
): void {
  const aborted = (error as any)?.name === "AbortError" || (error as any)?.type === "aborted";
  if (aborted) {
    recorder.logicalCallAborted({ details: { errorKind } });
    recorder.endLogicalCall("aborted");
    return;
  }
  recorder.attemptError(error, { details: { errorKind } });
  recorder.logicalCallError(error, { details: { errorKind } });
  recorder.endLogicalCall("error");
}

/** Assembled assistant message 的安全结构 metadata——只看类型/计数/数值，不看正文。 */
function summarizeAssistantMessage(message: any): Record<string, unknown> {
  const content = Array.isArray(message?.content) ? message.content : [];
  let hasText = false;
  let hasReasoning = false;
  let toolCallCount = 0;
  for (const block of content) {
    const type = block?.type;
    if (type === "text" && typeof block?.text === "string" && block.text.length > 0) hasText = true;
    if (type === "thinking" || type === "redacted_thinking" || type === "reasoning") hasReasoning = true;
    if (type === "toolCall") toolCallCount += 1;
  }
  const usage = compactUsageNumbers(message?.usage);
  return {
    stopReason: typeof message?.stopReason === "string" ? message.stopReason : null,
    hasText,
    hasReasoning,
    toolCallCount,
    usagePresent: Boolean(message?.usage),
    errorPresent: typeof message?.errorMessage === "string" && message.errorMessage.length > 0,
    ...(usage ? { usage } : {}),
  };
}

function compactUsageNumbers(usage: any): Record<string, number> | null {
  if (!usage || typeof usage !== "object") return null;
  const out: Record<string, number> = {};
  for (const key of ["input", "output", "cacheRead", "cacheWrite", "totalTokens"]) {
    const value = Number(usage[key]);
    if (Number.isFinite(value)) out[key] = value;
  }
  return Object.keys(out).length > 0 ? out : null;
}

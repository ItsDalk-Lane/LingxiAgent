/**
 * ModelCallRecorder — 面向集成点的生命周期 API。
 *
 * Recorder 持有一个 logical call 的状态（callId / 当前 attemptId / 终态标志），
 * 把"组事件 + 安全投递"从集成点（callText、Pi stream wrapper…）收走：
 *
 *   const recorder = createModelCallRecorder({ context: { model, source, attribution } });
 *   recorder.callId                       // 立即可用——可写进 ledger metadata 做关联
 *   recorder.beginLogicalCall();
 *   const attemptId = recorder.beginAttempt({ details: { … } });
 *   recorder.providerRequestPrepared({ details: { messageCount, … } });
 *   recorder.providerResponseReceived({ httpStatus: 200, providerRequestId });
 *   recorder.semanticResponseCompleted({ details: { stopReason, usagePresent, … } });
 *   recorder.endLogicalCall("ok");        // 恰好一次；重复调用是 no-op
 *
 * 语义不变量（由 recorder 强制，集成点不用各自维护）：
 *   - callId 在 recorder 创建时生成（= Provider 请求之前），全生命周期不变；
 *     调用方可用 context.callId 显式接管（MC-02 需要先把 callId 写进 ledger）。
 *   - beginAttempt 每次生成新 attemptId；attemptId != callId。
 *   - endLogicalCall 幂等：恰好投递一次 logical_call_end。
 *   - 所有事件经 safeEmitModelCallEvent：observer 抛错不影响业务。
 */

import {
  mintModelAttemptId,
  mintModelCallId,
  type ModelCallIdentityFactory,
} from "./model-call-identity.ts";
import {
  getModelCallObserver,
  normalizeModelCallError,
  safeEmitModelCallEvent,
  type ModelCallAttribution,
  type ModelCallEvent,
  type ModelCallEventType,
  type ModelCallModelIdentity,
  type ModelCallObserver,
  type ModelCallSource,
  type ModelCallTerminalStatus,
} from "./model-call-observer.ts";

export type ModelCallRecorderContext = {
  /** 显式接管 callId（调用方需要先于 logical_call_start 把身份写进别处时用）。 */
  callId?: string | null;
  /**
   * 显式接管当前 attemptId（ALS scope 内新建的临时 recorder 用，
   * 如 before/after provider hooks——它们不 beginAttempt，只补充中途事件）。
   */
  attemptId?: string | null;
  traceId?: string | null;
  parentCallId?: string | null;
  model?: ModelCallModelIdentity | null;
  source?: ModelCallSource | null;
  attribution?: ModelCallAttribution | null;
};

export type ModelCallRecorderOptions = {
  observer?: ModelCallObserver | null;
  identity?: ModelCallIdentityFactory;
  context?: ModelCallRecorderContext;
  now?: () => number;
};

export type ModelCallRecorder = ReturnType<typeof createModelCallRecorder>;

export function createModelCallRecorder({
  observer = null,
  identity = null,
  context = {},
  now = () => Date.now(),
}: ModelCallRecorderOptions = {}) {
  const sink: ModelCallObserver = observer ?? getModelCallObserver();
  const callId = typeof context.callId === "string" && context.callId.trim()
    ? context.callId.trim()
    : (identity ? identity.mintCallId() : mintModelCallId());
  const traceId = context.traceId ?? null;
  const parentCallId = context.parentCallId ?? null;
  const model = context.model ?? null;
  const source = context.source ?? null;
  const attribution = context.attribution ?? null;

  let currentAttemptId: string | null = typeof context.attemptId === "string" && context.attemptId.trim()
    ? context.attemptId.trim()
    : null;
  let ended = false;

  const emit = (
    eventType: ModelCallEventType,
    extras: Partial<ModelCallEvent> = {},
  ) => {
    safeEmitModelCallEvent(sink, {
      eventType,
      timestamp: new Date(now()).toISOString(),
      callId,
      attemptId: extras.attemptId !== undefined ? extras.attemptId : currentAttemptId,
      traceId,
      parentCallId,
      model,
      source,
      attribution,
      ...extras,
    });
  };

  return {
    /** logical call 稳定身份。创建即存在（Provider 请求发生之前）。 */
    callId,
    traceId,
    parentCallId,

    get currentAttemptId() {
      return currentAttemptId;
    },

    get ended() {
      return ended;
    },

    beginLogicalCall({ details = null }: { details?: Record<string, unknown> | null } = {}) {
      emit("logical_call_start", { attemptId: null, details });
    },

    /** 每次调用生成新 attemptId 并投递 attempt_start。返回 attemptId。 */
    beginAttempt({ details = null }: { details?: Record<string, unknown> | null } = {}) {
      currentAttemptId = identity ? identity.mintAttemptId() : mintModelAttemptId();
      emit("attempt_start", { attemptId: currentAttemptId, details });
      return currentAttemptId;
    },

    /**
     * Provider 请求已完成构造并被观测到。details 只允许结构 metadata
     * （messageCount/toolCount/hasSystemPrompt/hasImages/streaming/protocol/
     * inputByteEstimate）；绝不放 body/headers 全量。
     */
    providerRequestPrepared({ details = null }: { details?: Record<string, unknown> | null } = {}) {
      emit("provider_request_prepared", { details });
    },

    /** Provider response 到达。只带 httpStatus / providerRequestId，不带 body。 */
    providerResponseReceived({
      httpStatus = null,
      providerRequestId = null,
      details = null,
    }: {
      httpStatus?: number | null;
      providerRequestId?: string | null;
      details?: Record<string, unknown> | null;
    } = {}) {
      emit("provider_response_received", {
        providerRequestId,
        details: {
          ...(httpStatus !== null ? { httpStatus } : {}),
          ...(details || {}),
        },
      });
    },

    /**
     * 语义响应解析完成（parser/assembled message 之后、业务裁剪之前）。
     * details 建议：stopReason、hasText、hasReasoning、toolCallCount、
     * usagePresent、usage（仅数值）、errorPresent。
     */
    semanticResponseCompleted({ details = null }: { details?: Record<string, unknown> | null } = {}) {
      emit("semantic_response_completed", { details });
    },

    attemptError(error: unknown, { details = null, providerRequestId = null }: { details?: Record<string, unknown> | null; providerRequestId?: string | null } = {}) {
      emit("attempt_error", { status: "error", error: normalizeModelCallError(error), details, providerRequestId });
    },

    logicalCallError(error: unknown, { details = null, providerRequestId = null }: { details?: Record<string, unknown> | null; providerRequestId?: string | null } = {}) {
      emit("logical_call_error", { status: "error", error: normalizeModelCallError(error), details, providerRequestId });
    },

    logicalCallAborted({ details = null }: { details?: Record<string, unknown> | null } = {}) {
      emit("logical_call_aborted", { status: "aborted", details });
    },

    /** 恰好投递一次 logical_call_end；之后所有调用为 no-op。 */
    endLogicalCall(status: ModelCallTerminalStatus, { details = null }: { details?: Record<string, unknown> | null } = {}) {
      if (ended) return;
      ended = true;
      emit("logical_call_end", { status, details });
    },
  };
}

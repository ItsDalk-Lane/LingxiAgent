/**
 * ModelCallRecorder — 面向集成点的生命周期 API + 状态机。
 *
 * Recorder 持有一个 logical call 的状态（callId / 当前 attemptId / 终态标志），
 * 把"组事件 + 安全投递"从集成点（callText、Pi stream wrapper、媒体/语音/probe
 * 边界…）收走：
 *
 *   const recorder = createModelCallRecorder({ context: { model, source, attribution } });
 *   recorder.callId                       // 立即可用——可写进 ledger metadata 做关联
 *   recorder.beginLogicalCall();
 *   const attemptId = recorder.beginAttempt({ details: { … } });
 *   recorder.providerRequestPrepared({ details: { messageCount, … } });
 *   recorder.providerResponseReceived({ httpStatus: 200, providerRequestId });
 *   recorder.semanticResponseCompleted({ details: { stopReason, usagePresent, … } });
 *   recorder.endLogicalCall("ok");        // 恰好一次；之后一切调用为 no-op
 *
 * 语义不变量（由 recorder 强制，集成点不用各自维护）：
 *   - callId 在 recorder 创建时生成（= Provider 请求之前），全生命周期不变；
 *     调用方可用 context.callId 显式接管（MC-02 需要先把 callId 写进 ledger）。
 *   - beginAttempt 每次生成新 attemptId；attemptId != callId；同一 call 可多次
 *     beginAttempt（Codex image 401 credential refresh = 2 attempts 1 call）。
 *   - endLogicalCall 幂等：恰好投递一次 logical_call_end。
 *   - 状态机（§十二）：logical_call_end 之后任何生命周期方法都是 silent no-op，
 *     不 throw、不影响业务；晚到事件被丢弃的事实不补假事件。
 *   - 所有事件经 safeEmitModelCallEvent：observer 抛错不影响业务。
 *   - 所有 details 经 sanitizeModelCallDetails、providerRequestId 经
 *     sanitizeProviderRequestId：安全门在唯一出口执行，集成点无法绕过。
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
  sanitizeModelCallDetails,
  sanitizeProviderRequestId,
  type ModelCallAttribution,
  type ModelCallEvent,
  type ModelCallEventType,
  type ModelCallModelIdentity,
  type ModelCallObserver,
  type ModelCallSource,
  type ModelCallTerminalStatus,
  type ModelCallUsageCorrelationState,
} from "./model-call-observer.ts";
import {
  MODEL_CALL_SEMANTIC_PROVENANCE,
  sanitizeSemanticInputProvenance,
  summarizeSemanticInputProvenance,
  type ModelSemanticInputProvenance,
} from "./semantic-input-provenance.ts";
import type { ModelCallPayloadCaptureSession } from "./model-call-payload-capture.ts";

/** duck-typed capture session 校验（fail closed：形状不对即视为未安装）。 */
function isPayloadCaptureSession(value: unknown): value is ModelCallPayloadCaptureSession {
  if (!value || typeof value !== "object") return false;
  const session = value as Record<string, unknown>;
  return typeof session.captureSemanticRequest === "function"
    && typeof session.captureProviderRequest === "function"
    && typeof session.captureProviderResponse === "function"
    && typeof session.captureSemanticResponse === "function";
}

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
  /** 只由知道 exact usage correlation 不存在的真实运行时边界设置。 */
  usageCorrelation?: ModelCallUsageCorrelationState | null;
  /**
   * Phase 6：Sensitive Payload Capture session handle（§一百二十一）。
   * 只允许 capture capability 引用（身份+计数器+sink 引用）——session 本身
   * 不持有正文，事件也不暴露它；集成点经 recorder.payloadCapture 短路快路径。
   */
  payloadCapture?: unknown;
};

export type ModelCallRecorderOptions = {
  observer?: ModelCallObserver | null;
  identity?: ModelCallIdentityFactory | null;
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
  const usageCorrelation = context.usageCorrelation === "not_correlated"
    ? context.usageCorrelation
    : null;
  /** Phase 6 capture session handle（duck-typed 校验，防错型注入；非对象一律 null）。 */
  let payloadCaptureHandle = isPayloadCaptureSession(context.payloadCapture)
    ? (context.payloadCapture as ModelCallPayloadCaptureSession)
    : null;

  let currentAttemptId: string | null = typeof context.attemptId === "string" && context.attemptId.trim()
    ? context.attemptId.trim()
    : null;
  let currentAttemptErrored = false;
  let ended = false;
  /** Phase 5：per-call Semantic Input Provenance sidecar（随 recorder GC，无全局 Map）。 */
  let semanticInputProvenance: ModelSemanticInputProvenance | null = null;

  /** 终态后返回 null：logical_call_end 已关闭该 call 的事实边界。 */
  const buildEvent = (
    eventType: ModelCallEventType,
    extras: Partial<ModelCallEvent> = {},
  ): ModelCallEvent => {
    const event: ModelCallEvent = {
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
      ...(usageCorrelation ? { usageCorrelation } : {}),
      details: extras.details !== undefined ? sanitizeModelCallDetails(extras.details) : null,
      providerRequestId: extras.providerRequestId !== undefined
        ? sanitizeProviderRequestId(extras.providerRequestId)
        : null,
    };
    if (semanticInputProvenance) {
      // 非 enumerable symbol 引用：完整 provenance map 不进 details（§三十九），
      // 仅以引用形态随事件传递；JSON.stringify / Metadata Safety Gate 均不可见。
      Object.defineProperty(event, MODEL_CALL_SEMANTIC_PROVENANCE, {
        value: semanticInputProvenance,
        enumerable: false,
        writable: false,
        configurable: true,
      });
    }
    return event;
  };

  /** 唯一非终态出口：logical_call_end 之后一切生命周期事件 silent no-op（§十二）。 */
  const emit = (
    eventType: ModelCallEventType,
    extras: Partial<ModelCallEvent> = {},
  ) => {
    if (ended) return;
    safeEmitModelCallEvent(sink, buildEvent(eventType, extras));
  };

  return {
    /** logical call 稳定身份。创建即存在（Provider 请求发生之前）。 */
    callId,
    traceId,
    parentCallId,

    get currentAttemptId() {
      return currentAttemptId;
    },

    /** 当前 attempt 是否已投递 attempt_error（避免业务层 catch 重复投递）。 */
    get attemptErrored() {
      return currentAttemptErrored;
    },

    get ended() {
      return ended;
    },

    /** 当前 provenance（未 attach 时 null；测试/诊断读取）。 */
    get semanticInputProvenance() {
      return semanticInputProvenance;
    },

    /**
     * Phase 6 capture session handle（null = sink 未安装的快路径）。
     * 集成点只经此引用调用 capture，绝不让它进入事件/ledger。
     */
    get payloadCapture() {
      return payloadCaptureHandle;
    },

    /**
     * 附着 Semantic Input Provenance（§四十/§八十）：sanitize fail closed，
     * 非法输入整体为 null 不影响业务。须在 beginLogicalCall 之前调用，
     * summary 才会并入 logical_call_start details（§三十九）。
     */
    attachSemanticInputProvenance(provenance: unknown) {
      semanticInputProvenance = sanitizeSemanticInputProvenance(provenance);
    },

    /**
     * Phase 6：附着/替换 capture session handle（beginObservedModelCall 等
     * 先铸 callId 再建 session 的场景）。形状校验 fail closed。
     */
    attachPayloadCapture(session: unknown) {
      payloadCaptureHandle = isPayloadCaptureSession(session)
        ? (session as ModelCallPayloadCaptureSession)
        : null;
    },

    beginLogicalCall({ details = null }: { details?: Record<string, unknown> | null } = {}) {
      // Provenance 安装在 recorder 上（per-call sidecar），beginLogicalCall
      // 只投递安全 summary；完整 map 经事件 symbol 引用传递，不进 details。
      const provenanceSummary = summarizeSemanticInputProvenance(semanticInputProvenance);
      emit("logical_call_start", {
        attemptId: null,
        details: { ...(details || {}), ...(provenanceSummary || {}) },
      });
    },

    /** 每次调用生成新 attemptId 并投递 attempt_start。返回 attemptId。 */
    beginAttempt({ details = null }: { details?: Record<string, unknown> | null } = {}) {
      if (ended) return currentAttemptId;
      currentAttemptId = identity ? identity.mintAttemptId() : mintModelAttemptId();
      currentAttemptErrored = false;
      // Phase 6：capture session 的 attempt 归属随 recorder 推进（§一百二十二）。
      try {
        payloadCaptureHandle?.setAttempt(currentAttemptId);
      } catch { /* capture 故障不影响观测主链路 */ }
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
      currentAttemptErrored = true;
      emit("attempt_error", { status: "error", error: normalizeModelCallError(error), details, providerRequestId });
    },

    logicalCallError(error: unknown, { details = null, providerRequestId = null }: { details?: Record<string, unknown> | null; providerRequestId?: string | null } = {}) {
      emit("logical_call_error", { status: "error", error: normalizeModelCallError(error), details, providerRequestId });
    },

    logicalCallAborted({ details = null }: { details?: Record<string, unknown> | null } = {}) {
      emit("logical_call_aborted", { status: "aborted", details });
    },

    /**
     * 恰好投递一次 logical_call_end；之后所有调用（含本方法）为 no-op。
     * 终态事件本身在置位 ended 之前投递。
     */
    endLogicalCall(status: ModelCallTerminalStatus, { details = null }: { details?: Record<string, unknown> | null } = {}) {
      if (ended) return;
      ended = true;
      safeEmitModelCallEvent(sink, buildEvent("logical_call_end", { status, details }));
    },
  };
}

/**
 * observed-pi-direct-summary.ts — Pi direct summary（generateSummary 直发）
 * 的统一观测边界（MC-10，任务书 §四/§五）。
 *
 * 背景：Pi 0.84.1 `generateSummary()` 未传 streamFn 时回落
 * `completeSimple()` 直连 Provider（compaction.js:440-449）——不经 Pi
 * AgentSession streamFunction、不经 callText，是 MC-01～09 之外的独立架构
 * 路径（diary temporary summary 生产可达，见 MODEL_CALL_CLOSURE_DELTA.md）。
 *
 * 本模块在 Lingxi Pi SDK facade（lib/pi-sdk/index.ts）内被调用，复用
 * ModelCallObserver/ModelCallRecorder/resolveModelTraceContext——不创建
 * DiaryObserver/SummaryObserver（§四）。
 *
 * Attempt 诚实性（§五）：
 *   - attemptVisibility = logical_boundary：一个 generateSummary 边界折叠为
 *     一次 attempt；pi-ai transport retry 在 SDK 内部不可见（同 MC-01/02/03）。
 *   - diary 未传 retry policy（maxRetries=0，无语义重试）；即便未来传入，
 *     retryAssistantCall 的 produce 重入对本边界不可见，仍折叠。
 *   - 无 Provider request/response Hook（summarizer options 不含 onPayload，
 *     且不在 session 扩展链内）→ 不伪造 provider_request_prepared /
 *     provider_response_received。事件缺失即真相。
 */

import { resolveModelTraceContext } from "./model-trace-scope.ts";
import {
  modelCallFieldsFromUsageContext,
  normalizeModelCallIdentity,
  type ModelCallAttribution,
  type ModelCallSource,
} from "./model-call-observer.ts";
import { createModelCallRecorder } from "./model-call-recorder.ts";

/** Ledger 的最小结构契约（usage-ledger.ts 的 start/finish/recordError 投影）。 */
export type ObservedDirectSummaryLedger = {
  start?: (request: Record<string, unknown>) => { requestId?: string | null } | null | undefined;
  finish?: (requestId: string, payload?: Record<string, unknown>) => void;
  recordError?: (requestId: string, error: unknown) => void;
} | null | undefined;

export type ObservedDirectSummaryContext = {
  /** 业务归属（diary：subsystem memory / operation diary_temporary_summary）。 */
  usageContext?: unknown;
  source?: ModelCallSource | null;
  attribution?: ModelCallAttribution | null;
  /** 可选 accounting 投影；null = 只观测不入账（测试）。 */
  usageLedger?: ObservedDirectSummaryLedger;
  /** 显式 trace 覆盖（缺省走统一解析：scope → singleton）。 */
  traceId?: string | null;
  parentCallId?: string | null;
  /**
   * Phase 5：facade 参数边界构造的 Semantic Input Provenance
   * （messages/customInstructions/previousSummary 三元组，§七十）。仅安全
   * metadata；经 recorder sanitize fail closed。
   */
  semanticInputProvenance?: unknown;
};

export type ObservedDirectSummaryRunner<T> = () => Promise<T>;

/**
 * 包住一次 Pi direct summary 调用。返回 runner 的原样结果/异常（绝不改变业务
 * 行为：不碰 payload、模型、retry、timeout）。
 */
export async function observePiDirectSummary<T>(
  model: unknown,
  context: ObservedDirectSummaryContext,
  runner: ObservedDirectSummaryRunner<T>,
): Promise<T> {
  const fields = context.usageContext !== null && context.usageContext !== undefined
    ? modelCallFieldsFromUsageContext(context.usageContext)
    : { source: context.source ?? null, attribution: context.attribution ?? null };
  const trace = resolveModelTraceContext({
    traceId: context.traceId ?? null,
    parentCallId: context.parentCallId ?? null,
  });

  const recorder = createModelCallRecorder({
    context: {
      traceId: trace.traceId,
      parentCallId: trace.parentCallId,
      model: normalizeModelCallIdentity(model),
      source: fields.source,
      attribution: fields.attribution,
    },
  });
  if (context.semanticInputProvenance) {
    recorder.attachSemanticInputProvenance(context.semanticInputProvenance);
  }
  recorder.beginLogicalCall({
    details: {
      path: "pi_direct_summary",
      ...(trace.origin ? { traceOrigin: trace.origin } : {}),
    },
  });
  recorder.beginAttempt({
    // 一个 direct summary 边界 = 一次逻辑 attempt；SDK 内部重试不可见。
    details: { attemptVisibility: "logical_boundary" },
  });

  const ledgerMetadata = {
    modelCallId: recorder.callId,
    traceId: recorder.traceId,
    parentCallId: recorder.parentCallId,
  };
  let ledgerRequestId: string | null = null;
  if (context.usageLedger?.start) {
    try {
      ledgerRequestId = context.usageLedger.start({
        model: ledgerModelIdentity(model),
        usageContext: context.usageContext ?? fields,
        metadata: ledgerMetadata,
      })?.requestId ?? null;
    } catch {
      ledgerRequestId = null; // accounting 失败不影响观测/业务
    }
  }

  let result: T;
  try {
    result = await runner();
  } catch (error) {
    const aborted = (error as any)?.name === "AbortError" || (error as any)?.type === "aborted";
    if (aborted) {
      recorder.logicalCallAborted();
      recorder.endLogicalCall("aborted");
    } else {
      if (!recorder.attemptErrored) {
        recorder.attemptError(error, { details: { errorKind: "provider_or_network" } });
      }
      recorder.logicalCallError(error, { details: { errorKind: "provider_or_network" } });
      recorder.endLogicalCall("error");
    }
    if (ledgerRequestId && context.usageLedger?.recordError) {
      try { context.usageLedger.recordError(ledgerRequestId, error); } catch { /* never break */ }
    }
    throw error;
  }

  recorder.semanticResponseCompleted({
    details: {
      hasText: typeof result === "string" ? result.trim().length > 0 : Boolean(result),
    },
  });
  recorder.endLogicalCall("ok");
  if (ledgerRequestId && context.usageLedger?.finish) {
    try { context.usageLedger.finish(ledgerRequestId); } catch { /* never break */ }
  }
  return result;
}

function ledgerModelIdentity(model: unknown): Record<string, unknown> {
  const identity = normalizeModelCallIdentity(model);
  return {
    provider: identity.provider,
    modelId: identity.modelId,
    api: identity.api,
  };
}

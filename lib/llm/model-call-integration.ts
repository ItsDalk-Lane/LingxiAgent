/**
 * model-call-integration.ts — MC-05～MC-09 共用的观测接入层（§四十三/§七十一）。
 *
 * 这不是第二套 Observer：统一复用 ModelCallRecorder/ModelCallObserver，只是把
 * 「HTTP attempt 边界」「外部进程 attempt 边界」「logical call bootstrap」三件
 * 事从十几个 Adapter 里收拢成小 helper。它只负责：
 *
 *   - attempt identity（beginAttempt → 新 attemptId，可重复调用 = 同 call 多 attempt）
 *   - 安全 provider-request-prepared metadata（调用方给结构摘要，helper 不碰 body）
 *   - HTTP response status / allowlist request id
 *   - attempt error（fetch throw 或 !res.ok）
 *
 * 它绝不负责 prompt、payload 转换、credential、业务结果、accounting。
 *
 * Recorder 经业务边界显式注入（ctx.modelCall / input.modelCall）——「用了
 * accounting wrapper」不自动推断「这是模型调用」（§十五/§十六）；没有 recorder
 * 的调用点（独立测试、控制面）是纯 passthrough。
 */
import {
  extractProviderRequestId,
  modelCallFieldsFromUsageContext,
  normalizeModelCallIdentity,
  type ModelCallAttribution,
  type ModelCallSource,
} from "./model-call-observer.ts";
import { createModelCallRecorder, type ModelCallRecorder } from "./model-call-recorder.ts";

/** 携带 in-flight recorder 的最小契约（media submitCtx / speech input 等）。 */
export type ObservedModelCallCarrier = { modelCall?: unknown } | null | undefined;

function recorderFrom(carrier: ObservedModelCallCarrier): ModelCallRecorder | null {
  const candidate = carrier?.modelCall;
  if (!candidate || typeof candidate !== "object") return null;
  const recorder = candidate as unknown as ModelCallRecorder;
  return typeof recorder.beginAttempt === "function" && typeof recorder.providerRequestPrepared === "function"
    ? recorder
    : null;
}

function transportErrorKind(error: unknown): string {
  const name = (error as any)?.name;
  if (name === "AbortError" || (error as any)?.type === "aborted") return "abort";
  if (name === "TimeoutError") return "timeout";
  return "network";
}

/**
 * 观测一次真实 HTTP attempt（MC-05/06/08/09 的每个 fetch 调用点）。
 *
 *   attempt_start（attemptVisibility=exact, providerWireVisibility=request_response）
 *   provider_request_prepared（只带调用方的结构 metadata）
 *   fetch → provider_response_received（status + allowlist id）
 *   !res.ok → attempt_error（http_error；同一 call 再调用本函数即新 attempt，
 *             覆盖 Codex image 401 credential refresh 的 1 call + 2 attempts）
 *   fetch throw → attempt_error（abort/timeout/network）+ rethrow
 *
 * logical call 的终态（semantic_response_completed / logical_call_end）由业务
 * 边界负责——本 helper 不结束 call。
 */
export async function observedProviderFetch<T extends Response>(
  carrier: ObservedModelCallCarrier,
  fetchFn: () => Promise<T>,
  { requestDetails = null }: { requestDetails?: Record<string, unknown> | null } = {},
): Promise<T> {
  const recorder = recorderFrom(carrier);
  if (!recorder || recorder.ended) return fetchFn();
  recorder.beginAttempt({
    details: {
      attemptVisibility: "exact",
      providerWireVisibility: "request_response",
    },
  });
  recorder.providerRequestPrepared({ details: requestDetails });
  let response: T;
  try {
    response = await fetchFn();
  } catch (error) {
    recorder.attemptError(error, { details: { errorKind: transportErrorKind(error) } });
    throw error;
  }
  const httpStatus = typeof (response as any)?.status === "number" ? (response as any).status : null;
  recorder.providerResponseReceived({
    httpStatus,
    providerRequestId: extractProviderRequestId((response as any)?.headers),
  });
  if (httpStatus !== null && !(httpStatus >= 200 && httpStatus < 300)) {
    recorder.attemptError(new Error(`HTTP ${httpStatus}`), {
      details: { errorKind: "http_error", httpStatus },
    });
  }
  return response;
}

/**
 * 观测一次外部进程 attempt（MC-07 Dreamina CLI 的 execFile 边界）。
 *
 * Lingxi 看不到 CLI 内部的 HTTP request/retry/response——诚实标记
 * attemptVisibility=external_process_boundary、providerWireVisibility=opaque，
 * 绝不伪造 provider_request_prepared / provider_response_received（§三十）。
 * 进程 throw → attempt_error + rethrow；stdout 解析失败属于语义层，由业务
 * 边界的 logical_call_error 表达。
 */
export async function observedExternalProcessRun<T>(
  carrier: ObservedModelCallCarrier,
  runFn: () => Promise<T>,
  { details = null }: { details?: Record<string, unknown> | null } = {},
): Promise<T> {
  const recorder = recorderFrom(carrier);
  if (!recorder || recorder.ended) return runFn();
  recorder.beginAttempt({
    details: {
      attemptVisibility: "external_process_boundary",
      providerWireVisibility: "opaque",
      ...(details || {}),
    },
  });
  try {
    return await runFn();
  } catch (error) {
    recorder.attemptError(error, { details: { errorKind: "external_process" } });
    throw error;
  }
}

/**
 * 业务 Model Call 边界的 bootstrap：铸 callId（Provider/外部执行之前）+
 * beginLogicalCall。media/video/speech/probe 的逻辑调用都从真实 caller 处
 * 显式调用本函数（§十六），不从 accounting wrapper 自动推断。
 */
export function beginObservedModelCall({
  model,
  usageContext = null,
  source = null,
  attribution = null,
  details = null,
  traceId = null,
  parentCallId = null,
}: {
  model: unknown;
  usageContext?: unknown;
  source?: ModelCallSource | null;
  attribution?: ModelCallAttribution | null;
  details?: Record<string, unknown> | null;
  traceId?: string | null;
  parentCallId?: string | null;
}): ModelCallRecorder {
  const fields = usageContext !== null && usageContext !== undefined
    ? modelCallFieldsFromUsageContext(usageContext)
    : { source: source ?? null, attribution: attribution ?? null };
  const recorder = createModelCallRecorder({
    context: {
      traceId,
      parentCallId,
      model: normalizeModelCallIdentity(model),
      source: fields.source,
      attribution: fields.attribution,
    },
  });
  recorder.beginLogicalCall({ details });
  return recorder;
}

/**
 * 失败终态：logical_call_error + logical_call_end("error")。attempt 级失败已由
 * observedProviderFetch / observedExternalProcessRun 在失败点投递，这里不重复
 * 投递 attempt_error；语义层失败（HTTP 200 但解析/校验失败）本来就不属于
 * attempt_error。
 */
export function failObservedModelCall(
  recorder: ModelCallRecorder | null,
  error: unknown,
  { errorKind = "adapter_error", details = null }: { errorKind?: string; details?: Record<string, unknown> | null } = {},
): void {
  if (!recorder || recorder.ended) return;
  recorder.logicalCallError(error, { details: { errorKind, ...(details || {}) } });
  recorder.endLogicalCall("error", { details: { errorKind } });
}

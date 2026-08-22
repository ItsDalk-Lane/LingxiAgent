/**
 * model-observability-actions.ts — Model Observatory 专用 API client（Phase 9 §十一）。
 *
 * 不复用 lingxiFetch：它把 400/403/404/413 的结构化错误体压成一句话，
 * status 与 matchedCalls/maxCalls/field 全部丢失（§十一 error contract
 * 要求 UI 按 kind 分支）。本模块的 observabilityRequest 保留完整错误载荷。
 *
 * 纪律：
 *   - 所有函数抛出 ModelObservabilityRequestError；绝不 catch → []（§十一）。
 *   - 每个函数接受 { signal }，请求竞态由调用方 AbortController/generation
 *     管理（§十二）；本模块不吞 AbortError。
 *   - export / blob 返回原始 Response（流式保存/懒加载预览由调用方负责），
 *     但错误路径仍然先解析结构化错误体再抛。
 */
import { useSettingsStore } from '../../store';
import {
  appendConnectionAuth,
  buildConnectionUrl,
  requireServerConnection,
} from '../../../services/server-connection';
import { normalizeSessionRouteError } from '../../../../../../shared/error-user-messages.ts';
import type {
  ModelObservabilityAggregateResult,
  ModelObservabilityApiErrorKind,
  ModelObservabilityCallDetail,
  ModelObservabilityCallFilterInput,
  ModelObservabilityCallPage,
  ModelObservabilityExportRequest,
  ModelObservabilityGroupByDimension,
  ModelObservabilityHealthResponse,
  ModelObservabilityPayloadRecordDetail,
  ModelObservabilitySettingsResponse,
  ModelObservabilitySettingsUpdateRequest,
  ModelObservabilitySettingsUpdateResponse,
  ModelObservabilitySortKey,
  ModelObservabilityTraceDetail,
  ModelObservabilityTracePage,
} from '../../../../../../shared/model-observability-api-contract.ts';

const DEFAULT_TIMEOUT = 30_000;
const API_BASE = '/api/model-observability';

/* ── Error contract（§十一）────────────────────────────────────────────── */

/**
 * 结构化 API 错误。字段语义：
 *   status     HTTP status（网络层失败为 0）
 *   kind       顶层 error 字符串（invalid_query / invalid_cursor / not_initialized /
 *              not_found / export_limit / query_failed / local_only_route /
 *              studio_owner_required / forbidden / invalid_json …）
 *   code       细粒度 code；403 拒绝没有 code 字段时 = kind（与
 *              errorCodeFromResponseBody 语义一致：record.code 优先，否则
 *              error 字符串匹配闭集形态时兼任 code）
 *   field      400 invalid_query 的出错字段（有则给出）
 *   matchedCalls / maxCalls  413 export_limit 的计数（有则给出）
 *   reason     403 的拒绝原因（有则给出）
 */
export class ModelObservabilityRequestError extends Error {
  status: number;
  kind: string | null;
  code: string | null;
  field: string | null;
  matchedCalls: number | null;
  maxCalls: number | null;
  reason: string | null;

  constructor(message: string, init: {
    status: number;
    kind?: string | null;
    code?: string | null;
    field?: string | null;
    matchedCalls?: number | null;
    maxCalls?: number | null;
    reason?: string | null;
  }) {
    super(message);
    this.name = 'ModelObservabilityRequestError';
    this.status = init.status;
    this.kind = init.kind ?? null;
    this.code = init.code ?? null;
    this.field = init.field ?? null;
    this.matchedCalls = init.matchedCalls ?? null;
    this.maxCalls = init.maxCalls ?? null;
    this.reason = init.reason ?? null;
  }
}

export function isObservabilityErrorKind(
  error: unknown,
  kind: ModelObservabilityApiErrorKind,
): error is ModelObservabilityRequestError {
  return error instanceof ModelObservabilityRequestError && error.kind === kind;
}

export function isObservabilityAbortError(error: unknown): boolean {
  return !!error && typeof error === 'object'
    && (error as { name?: unknown }).name === 'AbortError';
}

/* ── 请求内核 ──────────────────────────────────────────────────────────── */

type ObservabilityRequestOptions = {
  method?: string;
  body?: unknown;
  signal?: AbortSignal | null;
  /** timeout=null 关闭超时（export 流式保存）；缺省 30s，仅覆盖到响应头到达。 */
  timeout?: number | null;
};

/**
 * 保留完整错误载荷的请求内核。timeout 只覆盖「响应头到达之前」——响应体
 * 流式读取阶段由调用方 signal 控制取消（与 lingxiFetch 语义一致）。
 */
export async function observabilityRequest(
  path: string,
  opts: ObservabilityRequestOptions = {},
): Promise<Response> {
  const connection = requireServerConnection(
    useSettingsStore.getState(),
    `observability ${path}: server connection not ready`,
  );
  const headers = new Headers(appendConnectionAuth(connection, undefined));
  if (opts.body !== undefined) headers.set('content-type', 'application/json');

  const controller = new AbortController();
  const timeout = opts.timeout === undefined ? DEFAULT_TIMEOUT : opts.timeout;
  const timer = timeout === null ? null : setTimeout(() => controller.abort(), timeout);
  const callerSignal = opts.signal ?? null;
  if (callerSignal) {
    if (callerSignal.aborted) controller.abort();
    else callerSignal.addEventListener('abort', () => controller.abort(), { once: true });
  }

  let res: Response;
  try {
    res = await fetch(buildConnectionUrl(connection, path), {
      method: opts.method ?? 'GET',
      headers,
      body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
      signal: controller.signal,
    });
  } catch (error) {
    if (timer) clearTimeout(timer);
    throw error;
  }
  if (timer) clearTimeout(timer);

  if (!res.ok) {
    throw await readObservabilityError(res, path);
  }
  return res;
}

async function readObservabilityError(res: Response, path: string): Promise<ModelObservabilityRequestError> {
  let text: string;
  try {
    text = await res.text();
  } catch {
    return new ModelObservabilityRequestError(`${path}: ${res.status} ${res.statusText}`, { status: res.status });
  }
  let data: unknown = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    // 非 JSON 错误体：保留文本作为 message，不伪装成结构化错误。
  }
  const routeError = normalizeSessionRouteError(data);
  const record = data && typeof data === 'object' && !Array.isArray(data)
    ? data as Record<string, unknown>
    : null;
  const kind = typeof record?.error === 'string' ? record.error : null;
  const code = routeError.code ?? (kind && /^[a-z][a-z0-9_]*$/.test(kind) ? kind : null);
  const message = routeError.message
    ?? (typeof record?.message === 'string' ? record.message : null)
    ?? (text.trim() || `${path}: ${res.status} ${res.statusText}`);
  return new ModelObservabilityRequestError(message, {
    status: res.status,
    kind,
    code,
    field: typeof record?.field === 'string' ? record.field : null,
    matchedCalls: typeof record?.matchedCalls === 'number' ? record.matchedCalls : null,
    maxCalls: typeof record?.maxCalls === 'number' ? record.maxCalls : null,
    reason: typeof record?.reason === 'string' ? record.reason : null,
  });
}

async function observabilityJson<T>(path: string, opts: ObservabilityRequestOptions = {}): Promise<T> {
  const res = await observabilityRequest(path, opts);
  const data = await res.json() as T & { error?: unknown };
  // HTTP 200 内嵌业务错误的边界与 lingxiFetchJson 对齐。
  if (data && typeof data === 'object' && 'error' in data && data.error) {
    const message = typeof data.error === 'string' ? data.error : JSON.stringify(data.error);
    throw new ModelObservabilityRequestError(message || `${path}: request failed`, { status: res.status });
  }
  return data;
}

type SignalOptions = { signal?: AbortSignal | null };

/* ── Health / Settings（控制面）────────────────────────────────────────── */

export function loadObservabilityHealth(opts: SignalOptions = {}): Promise<ModelObservabilityHealthResponse> {
  return observabilityJson(`${API_BASE}/health`, opts);
}

export function loadObservabilitySettings(opts: SignalOptions = {}): Promise<ModelObservabilitySettingsResponse> {
  return observabilityJson(`${API_BASE}/settings`, opts);
}

export function updateObservabilitySettings(
  patch: ModelObservabilitySettingsUpdateRequest,
  opts: SignalOptions = {},
): Promise<ModelObservabilitySettingsUpdateResponse> {
  return observabilityJson(`${API_BASE}/settings`, { ...opts, method: 'PUT', body: patch });
}

/* ── Query（POST JSON body）────────────────────────────────────────────── */

export type ObservabilityCallsQuery = {
  filter?: ModelObservabilityCallFilterInput;
  sort?: ModelObservabilitySortKey;
  limit?: number;
  cursor?: string | null;
};

export function queryObservabilityCalls(
  query: ObservabilityCallsQuery,
  opts: SignalOptions = {},
): Promise<ModelObservabilityCallPage> {
  return observabilityJson(`${API_BASE}/query/calls`, { ...opts, method: 'POST', body: query });
}

export function queryObservabilityTraces(
  query: ObservabilityCallsQuery,
  opts: SignalOptions = {},
): Promise<ModelObservabilityTracePage> {
  return observabilityJson(`${API_BASE}/query/traces`, { ...opts, method: 'POST', body: query });
}

export type ObservabilityAggregateQuery = {
  filter?: ModelObservabilityCallFilterInput;
  groupBy: ModelObservabilityGroupByDimension[];
  dateBucket?: { bucket: 'day'; utcOffsetMinutes: number };
};

export function queryObservabilityAggregate(
  query: ObservabilityAggregateQuery,
  opts: SignalOptions = {},
): Promise<ModelObservabilityAggregateResult> {
  return observabilityJson(`${API_BASE}/query/aggregate`, { ...opts, method: 'POST', body: query });
}

/* ── Drill-down detail ─────────────────────────────────────────────────── */

export function loadObservabilityCallDetail(
  callId: string,
  opts: SignalOptions = {},
): Promise<ModelObservabilityCallDetail> {
  return observabilityJson(`${API_BASE}/calls/${encodeURIComponent(callId)}`, opts);
}

export function loadObservabilityTraceDetail(
  traceId: string,
  opts: SignalOptions = {},
): Promise<ModelObservabilityTraceDetail> {
  return observabilityJson(`${API_BASE}/traces/${encodeURIComponent(traceId)}`, opts);
}

/**
 * exact payload retrieval（LOCAL_ONLY；远端 owner 403 由调用方转成
 * 「仅本机可看」提示，绝不视为 call 失败，§六十三）。
 */
export function loadObservabilityPayloadRecord(
  recordId: number,
  opts: SignalOptions = {},
): Promise<ModelObservabilityPayloadRecordDetail> {
  return observabilityJson(`${API_BASE}/payloads/${encodeURIComponent(String(recordId))}`, opts);
}

/* ── Export（流式；调用方负责 save bridge，§一百一十五）─────────────────── */

/**
 * 返回原始 Response（body 为 NDJSON 流）。**禁止** res.text()/res.blob()
 * 全量缓冲——调用方必须 reader.read() 分块写入 save bridge。错误路径
 * （400/413/…）仍解析结构化错误体后抛。
 */
export function fetchObservabilityExportStream(
  request: ModelObservabilityExportRequest,
  opts: SignalOptions = {},
): Promise<Response> {
  return observabilityRequest(`${API_BASE}/export`, {
    ...opts,
    method: 'POST',
    body: request,
    timeout: null,
  });
}

/* ── Stored Blob exact retrieval（§一百一十九～一百三十一）────────────── */

/**
 * HEAD 探测：只取 content-type/content-length（决定预览/下载/拒绝），
 * 不下载字节。响应头缺失时返回 null 字段，由调用方按 unknown 处理。
 */
export async function probeObservabilityBlob(
  blobId: string,
  opts: SignalOptions = {},
): Promise<{ contentType: string | null; contentLength: number | null }> {
  const res = await observabilityRequest(`${API_BASE}/blobs/${encodeURIComponent(blobId)}`, {
    ...opts,
    method: 'HEAD',
  });
  // HEAD 无响应体，必须显式 cancel 掉可能存在的 stream。
  res.body?.cancel().catch(() => {});
  const lengthHeader = res.headers.get('content-length');
  const contentLength = lengthHeader && /^\d+$/.test(lengthHeader) ? Number(lengthHeader) : null;
  return { contentType: res.headers.get('content-type'), contentLength };
}

/**
 * GET 完整 blob 字节（用户点 Preview/Download 之后才调用）。字节大小已由
 * probe 的 contentLength 与 UI 上限把关，此处 res.blob() 是单 blob 的有界
 * 读取，不受 export 流式纪律约束。
 */
export function fetchObservabilityBlob(
  blobId: string,
  opts: SignalOptions = {},
): Promise<Response> {
  return observabilityRequest(`${API_BASE}/blobs/${encodeURIComponent(blobId)}`, opts);
}

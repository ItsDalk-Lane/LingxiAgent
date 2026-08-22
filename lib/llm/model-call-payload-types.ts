/**
 * model-call-payload-types.ts — Sensitive Payload Capture 契约（Phase 6）。
 *
 * 与 ModelCallObserver（safe metadata channel）严格分层的第二个 contract：
 * Observer 事件只承载不可逆结构 metadata；本模块定义**敏感正文**的捕获记录
 * （Semantic/Provider Request/Response 四层级），进入 Capture Sink 之前必须
 * 经过 model-call-payload-redaction.ts 的统一 Redaction/Externalization，
 * Sink 永远只能收到 sanitized detached copy（§十四）。
 *
 * 红线（与 Observer 同源，本模块进一步收紧）：
 *   - kind/visibility/fidelity/action/transformation 全部闭集，禁止自由字符串。
 *   - record 复用 ModelCallObserver 的既有 safe identity（model/source/attribution），
 *     不建第二套身份体系（§十六）。
 *   - 资源上限（§四十三）机器执行：超限 truncate/omit，绝不 OOM、绝不静默截断
 *     不标注（§四十二）。
 *   - 一个 logical call 的基数（§十八）：1 semantic_request、N provider_request、
 *     N provider_response、0..1 semantic_response；N 由 transport 边界决定，
 *     不假设等于 attempt 数。
 */

import type {
  ModelCallAttribution,
  ModelCallModelIdentity,
  ModelCallSource,
} from "./model-call-observer.ts";
import {
  MODEL_CALL_PAYLOAD_SCHEMA_VERSION,
} from "../../shared/model-observability-api-contract.ts";
import type {
  ModelSemanticInputProvenance,
  SemanticInputShape,
  ProviderRequestProvenance,
  ModelCallPayloadKind,
  ModelCallPayloadVisibility,
  ModelCallPayloadFidelity,
  ModelCallPayloadSanitization,
  ModelCallPayloadSanitizationStatus,
} from "../../shared/model-observability-api-contract.ts";

/* ── 闭集与 wire 类型：已迁移到 shared/model-observability-api-contract.ts ──
 *
 * Phase 9（§九）：renderer 需要消费 payload kind/visibility/fidelity/sanitization
 * 闭集与 Semantic Response / Provider Provenance wire 形状，但本文件链路携带
 * Node-only 依赖。因此全部闭集数组与纯 JSON wire 类型收拢到 shared 单一事实源，
 * 此处 re-export 保持全部既有 import 站点（capture sink/redactor/store/routes/
 * tests）不变；sanitizeStatusOf / NO_SANITIZATION / CAPTURE_LIMITS / Record /
 * Capture 输入形状等 server 逻辑留在本文件。
 */
export {
  MODEL_CALL_PAYLOAD_SCHEMA_VERSION,
  MODEL_CALL_PAYLOAD_KINDS,
  MODEL_CALL_PAYLOAD_VISIBILITY,
  MODEL_CALL_PAYLOAD_FIDELITY,
  MODEL_CALL_REDACTION_ACTIONS,
  MODEL_CALL_PAYLOAD_SANITIZATION_STATUS,
  PROVIDER_REQUEST_TRANSFORMATIONS,
  PROVIDER_MAPPING_PRECISION,
} from "../../shared/model-observability-api-contract.ts";
export type {
  ModelCallPayloadKind,
  ModelCallPayloadVisibility,
  ModelCallPayloadFidelity,
  ModelCallRedactionAction,
  ModelCallRedactionActionEntry,
  ModelCallPayloadSanitization,
  ModelCallPayloadSanitizationStatus,
  ModelSemanticToolCall,
  ModelSemanticMediaResult,
  ModelSemanticResponse,
  ProviderRequestTransformation,
  ProviderMappingPrecision,
  ProviderPayloadLocator,
  ProviderRequestProvenanceMapping,
  ProviderRequestProvenance,
} from "../../shared/model-observability-api-contract.ts";

export function sanitizeStatusOf(sanitization: ModelCallPayloadSanitization): ModelCallPayloadSanitizationStatus {
  const redacted = sanitization.redacted ? "redacted" : "";
  const truncated = sanitization.truncated ? "truncated" : "";
  const degraded = sanitization.degraded ? "degraded" : "";
  return [redacted, truncated, degraded].filter(Boolean).join("_") as
    ModelCallPayloadSanitizationStatus || "none";
}

/** Redactor 无输出时的标准摘要。 */
export const NO_SANITIZATION: ModelCallPayloadSanitization = {
  redacted: false,
  truncated: false,
  degraded: false,
  actions: [],
};

/* ── 资源上限（§四十三，契约值，测试锁定）────────────────────────────── */

export const MODEL_CALL_PAYLOAD_CAPTURE_LIMITS = {
  /** 对象/数组嵌套深度上限；超限子树整体 omitted（degraded）。 */
  maxDepth: 24,
  /** 单条 record 访问节点总数上限（防百万数组元素）。 */
  maxNodes: 20_000,
  /** 单个数组保留元素上限；超出折叠（truncated）。 */
  maxArrayItems: 256,
  /** 单个对象保留键上限；超出折叠（truncated）。 */
  maxObjectKeys: 128,
  /** 单个字符串保留字符上限（UTF-16）；超出截断（truncated）。 */
  maxStringChars: 131_072,
  /** 单条 record 捕获总字符预算；超出停止复制（truncated）。 */
  maxRecordChars: 1_000_000,
} as const;

/* ── Provider Request 传输描述（捕获输入形状）────────────────────────── */

export type ModelCallProviderTransport = {
  method?: string | null;
  /** 原始 URL；进入 record 前经 sanitizeCapturedUrl（query credential 剥除）。 */
  url?: string | null;
  /** 原始 headers（Headers 实例或 plain object）；键保留、secret 值替换。 */
  headers?: unknown;
  /** pre-serialization body（plain object / FormData / string）。 */
  body?: unknown;
};

/* ── Semantic Response 统一外壳（§一百零五）与 Provider-Wire Provenance 契约
 * （§五十三～§五十七）：wire 类型与闭集定义已迁移到 shared（见本文件头部
 * re-export）。语义保持：transformation 映射必须在 transformation 发生时由构造
 * 代码产生（§五十九），严禁对 provider body 做内容搜索反推（§五十八）。
 */

/* ── Blob Externalization 契约（Phase 7，§六十/§六十一）───────────────── */

/**
 * privileged Blob Externalizer contract——与 ModelCallPayloadSink 完全不同的
 * 第二通道：Redactor 在统一脱敏时把 runtime 中**真实 materialized 的二进制**
 * （Buffer/TypedArray/ArrayBuffer；Blob/base64/dataURL 因无法同步读取字节而
 * 保持 externalized，诚实 PARTIAL）交给它换取 blob descriptor。字节走
 * externalizer → Blob Store；Payload 通道只拿到 descriptor。
 *
 * 生产默认 null（维持 Phase 6 externalized 行为，§六十二）；只在显式启用
 * blob persistence 时安装。externalizer 绝不自动读取本地文件/下载 URL
 * （§六十三/§六十四）；stage 超出 size/queue cap 时返回 null（降级 externalized）。
 */
export interface ModelCallBlobExternalizer {
  stageBinary(input: { bytes: Uint8Array; mediaType: string | null }): { blobId: string } | null;
}

/**
 * external_blob descriptor 的 captureStatus 语义（Phase 6 起 + Phase 7 扩展）：
 *   externalized —— bytes 未进入任何存储（Phase 6 默认/不可同步读取的类型）
 *   staged       —— bytes 已进入 externalizer 的 bounded 暂存队列（Redactor 产物）
 *   stored       —— blob 文件已 durable 且 metadata 已 commit（持久化层归一后）
 *   store_failed —— blob 写盘失败；descriptor 不携带 blobId（无 dangling ref）
 * 「staged → stored/store_failed」是持久化层的存储态记账，不是第二次业务
 * redaction（payload-store normalizeStagedBlobDescriptors）。
 */

/* ── Capture Record（§十六）─────────────────────────────────────────── */

export type ModelCallPayloadRecord = {
  schemaVersion: typeof MODEL_CALL_PAYLOAD_SCHEMA_VERSION;
  kind: ModelCallPayloadKind;
  /** ISO-8601。 */
  capturedAt: string;
  callId: string;
  traceId: string | null;
  parentCallId: string | null;
  attemptId: string | null;
  /**
   * 同一 call 内 provider request 的单调序号（§十九）：capture 每条
   * provider_request 时由 session 分配；重试/codex refresh 各得独立 ordinal。
   * 其它 kind 为 null。
   */
  providerRequestOrdinal: number | null;
  model: ModelCallModelIdentity | null;
  source: ModelCallSource | null;
  attribution: ModelCallAttribution | null;
  visibility: ModelCallPayloadVisibility;
  fidelity: ModelCallPayloadFidelity;
  sanitization: ModelCallPayloadSanitization;
  /**
   * sanitized detached copy。形状由 kind 决定：
   *   semantic_request → { inputShape, systemPrompt?, messages?, tools?, parameters?, inputShape }
   *   provider_request → { transport: { method?, url?, headers?, body? } }
   *   provider_response → { status?, headers?, body? }
   *   semantic_response → ModelSemanticResponse
   * opaque/unavailable 时为 null。
   */
  payload: unknown;
  /** 仅 kind=semantic_request：随 redaction offset remap 后的 provenance 副本。 */
  semanticInputProvenance?: ModelSemanticInputProvenance | null;
  /** 仅 kind=provider_request：构造时产生的 mapping sidecar。 */
  providerRequestProvenance?: ProviderRequestProvenance | null;
};

/** Semantic request 捕获输入（capture session API 的参数形状）。 */
export type SemanticRequestCaptureInput = {
  inputShape: SemanticInputShape;
  systemPrompt?: string | null;
  messages?: unknown;
  tools?: unknown;
  /** media/speech/probe/summary 等参数型语义输入（prompt/references/audio/language…）。 */
  parameters?: Record<string, unknown> | null;
  provenance?: ModelSemanticInputProvenance | null;
};

/** Semantic request record 的 payload 外壳（sanitized 后）。 */
export type SemanticRequestPayloadShape = {
  inputShape: SemanticInputShape;
  systemPrompt?: string | null;
  messages?: unknown;
  tools?: unknown;
  parameters?: Record<string, unknown> | null;
};

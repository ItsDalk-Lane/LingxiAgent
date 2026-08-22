/**
 * model-observability-api-contract.ts — Model Observatory 的 browser-safe wire
 * contract（Phase 9 §九）。
 *
 * 本模块是 Server 与 Desktop renderer 之间**唯一**共享的观测 DTO/枚举事实源：
 *
 *   - 只包含纯 JSON wire 形状：`type` / `interface` / `as const` 枚举数组。
 *   - 严禁任何运行时依赖：node:* / fs / path / better-sqlite3 / server 代码
 *     （contract 测试扫描本文件源码锁定）。
 *   - lib/llm 的 server 模块（query-types / query / payload-types /
 *     semantic-input-provenance）从这里 import 并 re-export，保持既有 import
 *     站点不变；normalize / cursor / fingerprint / SQL 逻辑留在 server 侧。
 *   - renderer 只 import 本模块（或 desktop 侧的 re-export），绝不 import
 *     lib/llm/*（query-types.ts 含 node:crypto）。
 *
 * 语义冻结（Phase 9 §三）：Logical Call ≠ Attempt ≠ Provider Request；
 * Trace ≠ Conversation ≠ Session；terminalStatus ≠ usage.status；
 * Semantic ≠ Provider Request/Response；OPAQUE/UNAVAILABLE/METADATA_ONLY ≠ empty；
 * payloadAvailability=unknown ≠ not_captured。UI 不得为画图方便修改这些语义。
 */

/* ════════════════════════════════════════════════════════════════════════
 * A. Query wire contract（filter / group by / pagination）
 * ════════════════════════════════════════════════════════════════════════ */

/** 每字段最大多值数量（避免巨大 IN (...)）。 */
export const MODEL_OBSERVABILITY_FILTER_MAX_VALUES = 32;

/** 多值字段：字段内 OR，字段间 AND。 */
export type ModelObservabilityMultiValueField =
  | "provider"
  | "modelId"
  | "api"
  | "subsystem"
  | "operation"
  | "surface"
  | "trigger"
  | "callPurpose"
  | "terminalStatus"
  | "attributionKind"
  | "sessionId"
  | "sessionPath"
  | "conversationId"
  | "conversationType"
  | "agentId"
  | "childAgentId"
  | "childSessionId"
  | "taskId"
  | "inputShape"
  | "provenancePrecision"
  | "payloadAvailability";

/** filter 的原始（用户输入）wire 形状；server normalize 后才可用。 */
export type ModelObservabilityCallFilterInput = {
  since?: unknown;
  until?: unknown;
  traceId?: unknown;
  parentCallId?: unknown;
  callId?: unknown;
  provider?: unknown;
  modelId?: unknown;
  api?: unknown;
  subsystem?: unknown;
  /** category ≡ subsystem（alias，不另立语义）。 */
  category?: unknown;
  operation?: unknown;
  surface?: unknown;
  trigger?: unknown;
  callPurpose?: unknown;
  terminalStatus?: unknown;
  attributionKind?: unknown;
  sessionId?: unknown;
  sessionPath?: unknown;
  conversationId?: unknown;
  conversationType?: unknown;
  agentId?: unknown;
  childAgentId?: unknown;
  childSessionId?: unknown;
  taskId?: unknown;
  inputShape?: unknown;
  provenancePrecision?: unknown;
  payloadAvailability?: unknown;
  interruptedByRestart?: unknown;
  hasPayload?: unknown;
};

export type ModelObservabilitySortKey = "started_at_desc";

export const MODEL_OBSERVABILITY_SORT_KEYS: readonly ModelObservabilitySortKey[] = ["started_at_desc"];

export const MODEL_OBSERVABILITY_PAGE_DEFAULT_LIMIT = 50;
export const MODEL_OBSERVABILITY_PAGE_MAX_LIMIT = 200;

/* ── Group By ─────────────────────────────────────────────────────────── */

export type ModelObservabilityGroupByDimension =
  | "date"
  | "provider"
  | "model"
  | "category"
  | "operation"
  | "callPurpose"
  | "status"
  | "attributionKind"
  | "session"
  | "conversation"
  | "agent"
  | "task"
  | "inputShape"
  | "provenancePrecision";

export const MODEL_OBSERVABILITY_GROUP_BY_DIMENSIONS: readonly ModelObservabilityGroupByDimension[] = [
  "date",
  "provider",
  "model",
  "category",
  "operation",
  "callPurpose",
  "status",
  "attributionKind",
  "session",
  "conversation",
  "agent",
  "task",
  "inputShape",
  "provenancePrecision",
];

/** 多级 groupBy 上限（最多 3 维，不支持无限嵌套）。 */
export const MODEL_OBSERVABILITY_GROUP_BY_MAX_DIMENSIONS = 3;

export type ModelObservabilityDateBucket = {
  bucket: "day";
  /** 本地时区偏移（分钟，东半球为正）；server timezone 不入局。 */
  utcOffsetMinutes: number;
};

/* ── 闭集值数组（UI 与 normalizer 共享的唯一事实源）───────────────────── */

export const MODEL_OBSERVABILITY_TERMINAL_STATUSES = [
  "ok",
  "error",
  "aborted",
  "incomplete",
] as const;
export type ModelObservabilityTerminalStatus = typeof MODEL_OBSERVABILITY_TERMINAL_STATUSES[number];

export const MODEL_OBSERVABILITY_PAYLOAD_AVAILABILITIES = [
  "present",
  "expired",
  "dropped",
  "not_captured",
  "unknown",
] as const;
export type ModelObservabilityPayloadAvailability =
  typeof MODEL_OBSERVABILITY_PAYLOAD_AVAILABILITIES[number];

export const MODEL_OBSERVABILITY_USAGE_AVAILABILITIES = [
  "present",
  "not_correlated",
  "projection_unavailable",
  "unknown",
] as const;
export type ModelObservabilityUsageAvailability =
  typeof MODEL_OBSERVABILITY_USAGE_AVAILABILITIES[number];

/* ════════════════════════════════════════════════════════════════════════
 * B. Query response DTO（列表永远是轻量 metadata，无正文）
 * ════════════════════════════════════════════════════════════════════════ */

export type ModelObservabilityUsageSummary = {
  inputTokens: number | null;
  outputTokens: number | null;
  reasoningTokens: number | null;
  cacheReadTokens: number | null;
  cacheWriteTokens: number | null;
  totalTokens: number | null;
  costTotal: number | null;
};

export type ModelObservabilityCallListItem = {
  callId: string;
  traceId: string | null;
  parentCallId: string | null;
  startedAt: string | null;
  endedAt: string | null;
  durationMs: number | null;
  terminalStatus: string | null;
  persistenceCompleteness: string;
  interruptedByRestart: boolean;
  model: { provider: string | null; modelId: string | null; api: string | null };
  source: { subsystem: string | null; operation: string | null; surface: string | null; trigger: string | null };
  attribution: {
    kind: string | null;
    sessionId: string | null;
    sessionPath: string | null;
    conversationId: string | null;
    conversationType: string | null;
    agentId: string | null;
    childAgentId: string | null;
    childSessionId: string | null;
    taskId: string | null;
  };
  callPurpose: string | null;
  inputShape: string | null;
  provenancePrecision: string | null;
  provenance: { sectionCount: number | null; opaqueCount: number | null; categories: string[] };
  payloadAvailability: ModelObservabilityPayloadAvailability;
  payloadRecordCount: number;
  usage: {
    availability: ModelObservabilityUsageAvailability;
    status: string | null;
    summary: ModelObservabilityUsageSummary | null;
  };
  attemptCount: number;
  providerRequestCount: number;
};

export type ModelObservabilityDataCompleteness = {
  droppedTraceEvents: number;
  droppedPayloadRecords: number;
  droppedBlobs: number;
  interruptedByRestartCalls: number;
};

export type ModelObservabilityCallPage = {
  calls: ModelObservabilityCallListItem[];
  nextCursor: string | null;
  dataCompleteness: ModelObservabilityDataCompleteness | null;
};

export type ModelObservabilityTraceListItem = {
  traceId: string;
  origin: string | null;
  firstSeenAt: string;
  lastSeenAt: string;
  callCount: number;
  terminalOk: number;
  terminalError: number;
  terminalAborted: number;
  incomplete: number;
};

export type ModelObservabilityTracePage = {
  traces: ModelObservabilityTraceListItem[];
  nextCursor: string | null;
};

export type ModelObservabilityGroupMetrics = {
  callCount: number;
  traceCount: number;
  okCount: number;
  errorCount: number;
  abortedCount: number;
  incompleteCount: number;
  attemptCount: number;
  durationObservedCount: number;
  durationTotalMs: number;
  durationAverageMs: number | null;
  usageCoveredCalls: number;
  usageMissingCalls: number;
  inputTokens: number;
  outputTokens: number;
  reasoningTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  totalTokens: number;
  costTotal: number | null;
  cacheHitCount: number;
  cacheObservedCount: number;
};

/**
 * group 维度值：model 维度展开为 provider + modelId 两列（逻辑 key =
 * provider + modelId），其余维度单列。
 */
export type ModelObservabilityGroupValues = Partial<
  Record<Exclude<ModelObservabilityGroupByDimension, "model">, string | null>
> & {
  provider?: string | null;
  modelId?: string | null;
};

export type ModelObservabilityGroupBucket = {
  key: string;
  values: ModelObservabilityGroupValues;
  metrics: ModelObservabilityGroupMetrics;
};

export type ModelObservabilityAggregateResult = {
  groups: ModelObservabilityGroupBucket[];
  overall: ModelObservabilityGroupMetrics;
};

/* ── Drill-down DTO ───────────────────────────────────────────────────── */

export type ModelObservabilityQueryHealth = {
  queryStatus: "ready" | "absent" | "unavailable" | "degraded";
  queryStatusReason: string | null;
  schemaVersion: number | null;
  accountingProjectionAvailable: boolean;
  oldestCallAt: string | null;
  newestCallAt: string | null;
  callCount: number;
  traceCount: number;
  payloadRecordCount: number;
  usageProjectionCount: number;
  /** global cumulative drop counters（DB 全局事实，不绑具体 call）。 */
  dataCompleteness: ModelObservabilityDataCompleteness;
};

export type ModelObservabilityPayloadRecordMetadata = {
  id: number;
  callId: string;
  kind: string;
  attemptId: string | null;
  providerRequestOrdinal: number | null;
  capturedAt: string;
  visibility: string;
  fidelity: string;
  sanitizationStatus: string;
  redacted: boolean;
  truncated: boolean;
  degraded: boolean;
  recordCharCount: number | null;
  hasBody: boolean;
  hasSemanticProvenance: boolean;
  hasProviderProvenance: boolean;
  blobIds: string[];
};

export const MODEL_OBSERVABILITY_PAYLOAD_CONTENT_STATES = [
  "present",
  "null_payload",
  "opaque_or_unavailable",
  "corrupt",
] as const;
export type ModelObservabilityPayloadContentState =
  typeof MODEL_OBSERVABILITY_PAYLOAD_CONTENT_STATES[number];

export type ModelObservabilityPayloadRecordDetail = ModelObservabilityPayloadRecordMetadata & {
  contentAvailable: boolean;
  contentState: ModelObservabilityPayloadContentState;
  payload: unknown;
  semanticInputProvenance: unknown;
  providerRequestProvenance: unknown;
};

export type ModelObservabilityCallRef = {
  callId: string;
  startedAt: string | null;
  terminalStatus: string | null;
  modelId: string | null;
};

export type ModelObservabilityAttemptSummary = {
  attemptId: string;
  startedAt: string | null;
  requestPreparedAt: string | null;
  responseReceivedAt: string | null;
  errorAt: string | null;
  providerRequestId: string | null;
  httpStatus: number | null;
  attemptVisibility: string | null;
  providerWireVisibility: string | null;
  errorName: string | null;
  errorCode: string | null;
};

export type ModelObservabilityCallDetail = {
  call: ModelObservabilityCallListItem;
  trace: { traceId: string; origin: string | null; firstSeenAt: string | null; lastSeenAt: string | null } | null;
  parentCall: ModelObservabilityCallRef | null;
  childCalls: ModelObservabilityCallRef[];
  attempts: ModelObservabilityAttemptSummary[];
  payloadRecords: ModelObservabilityPayloadRecordMetadata[];
};

export type ModelObservabilityTraceDetail = {
  trace: ModelObservabilityTraceListItem;
  calls: ModelObservabilityCallListItem[];
  roots: Array<{ callId: string; orphanParent: boolean }>;
  edges: Array<{ parentCallId: string; childCallId: string }>;
  orphanEdges: Array<{ childCallId: string; missingParentCallId: string }>;
  graphIntegrity: "ok" | "degraded";
  usageAggregate: {
    availability: ModelObservabilityUsageAvailability;
    summary: {
      inputTokens: number;
      outputTokens: number;
      reasoningTokens: number;
      cacheReadTokens: number;
      cacheWriteTokens: number;
      totalTokens: number;
      costTotal: number | null;
    } | null;
  };
  payloadCompleteness: {
    present: number;
    expired: number;
    dropped: number;
    notCaptured: number;
    unknown: number;
  };
  dataCompleteness: ModelObservabilityDataCompleteness;
};

/* ════════════════════════════════════════════════════════════════════════
 * C. Payload / Provenance wire contract（closed sets + JSON 形状）
 * ════════════════════════════════════════════════════════════════════════ */

export const MODEL_CALL_PAYLOAD_SCHEMA_VERSION = 1;

/** Payload 层级闭集：不得合并成模糊的 request/response。 */
export const MODEL_CALL_PAYLOAD_KINDS = [
  "semantic_request",
  "provider_request",
  "provider_response",
  "semantic_response",
] as const;
export type ModelCallPayloadKind = typeof MODEL_CALL_PAYLOAD_KINDS[number];

/**
 * Payload 可见度闭集（观测能力，与 sanitization 正交）：
 *   full          该层级内容在 Lingxi 运行时完整可见并被捕获（副本可能 redacted/truncated）
 *   partial       部分可见（stream 只保留 aggregate 等）
 *   metadata_only 只有结构/状态 metadata，无正文
 *   opaque        边界存在但内容在外部（CLI 进程内）
 *   unavailable   边界事件结构已知但本轮不可观察（无 hook / 协议不触发）
 */
export const MODEL_CALL_PAYLOAD_VISIBILITY = [
  "full",
  "partial",
  "metadata_only",
  "opaque",
  "unavailable",
] as const;
export type ModelCallPayloadVisibility = typeof MODEL_CALL_PAYLOAD_VISIBILITY[number];

/**
 * Payload 保真度闭集（捕获副本精确到什么程度）：runtime_exact /
 * parsed_equivalent / stream_aggregate / normalized / metadata_only /
 * external_process / opaque。严禁自称 raw。
 */
export const MODEL_CALL_PAYLOAD_FIDELITY = [
  "runtime_exact",
  "parsed_equivalent",
  "stream_aggregate",
  "normalized",
  "metadata_only",
  "external_process",
  "opaque",
] as const;
export type ModelCallPayloadFidelity = typeof MODEL_CALL_PAYLOAD_FIDELITY[number];

/** Redaction action 闭集。action 条目绝不携带原值。 */
export const MODEL_CALL_REDACTION_ACTIONS = [
  "removed",
  "replaced",
  "externalized",
  "truncated",
  "unsupported",
] as const;
export type ModelCallRedactionAction = typeof MODEL_CALL_REDACTION_ACTIONS[number];

export type ModelCallRedactionActionEntry = {
  /** 捕获副本内的路径（对 sanitized payload 寻址；顶层键省略前缀）。 */
  path: Array<string | number>;
  action: ModelCallRedactionAction;
  reason: string;
};

/** Sanitization 结果摘要：可审计、无原值。 */
export type ModelCallPayloadSanitization = {
  redacted: boolean;
  truncated: boolean;
  /** 资源上限/不支持类型导致内容不完整（结构有缺口）。 */
  degraded: boolean;
  actions: ModelCallRedactionActionEntry[];
};

export const MODEL_CALL_PAYLOAD_SANITIZATION_STATUS = [
  "none",
  "redacted",
  "truncated",
  "degraded",
  "redacted_truncated",
  "redacted_degraded",
  "truncated_degraded",
  "redacted_truncated_degraded",
] as const;
export type ModelCallPayloadSanitizationStatus =
  typeof MODEL_CALL_PAYLOAD_SANITIZATION_STATUS[number];

/* ── Semantic Response 统一外壳 ────────────────────────────────────────── */

export type ModelSemanticToolCall = {
  name: string | null;
  id: string | null;
  /** 模型产出的 arguments（任意 JSON 形状；已脱敏）。 */
  arguments?: unknown;
};

export type ModelSemanticMediaResult = {
  taskId?: string | null;
  providerTaskId?: string | null;
  fileCount?: number | null;
  deferred?: boolean | null;
  files?: unknown;
};

export type ModelSemanticResponse = {
  text?: string | null;
  reasoning?: string | null;
  toolCalls?: ModelSemanticToolCall[];
  structuredOutput?: unknown;
  media?: ModelSemanticMediaResult;
  transcription?: string | null;
  finishReason?: string | null;
  usage?: unknown;
  /** complete / partial（aborted 但有已组装内容）。 */
  completeness: "complete" | "partial";
};

/* ── Provider-Wire Provenance 契约 ─────────────────────────────────────── */

/** Transformation 闭集。 */
export const PROVIDER_REQUEST_TRANSFORMATIONS = [
  "pass_through",
  "renamed",
  "moved",
  "merged",
  "split",
  "filtered",
  "injected",
  "externalized",
  "dropped",
  "opaque",
] as const;
export type ProviderRequestTransformation = typeof PROVIDER_REQUEST_TRANSFORMATIONS[number];

/** Mapping precision 闭集。 */
export const PROVIDER_MAPPING_PRECISION = [
  "exact",
  "structural",
  "opaque",
] as const;
export type ProviderMappingPrecision = typeof PROVIDER_MAPPING_PRECISION[number];

export type ProviderPayloadLocator = {
  /** 对 sanitized provider request body 的寻址路径，如 ["system"] / ["messages",0,"content"]。 */
  path: Array<string | number>;
  /** 若目标值是 string 且位置精确到 span（UTF-16 闭开区间）。 */
  span?: { start: number; end: number } | null;
};

/**
 * 单条 semantic section → provider 字段的映射。构造时产生；严禁对 provider
 * body 做内容搜索反推。
 */
export type ProviderRequestProvenanceMapping = {
  /** 对 ModelSemanticInputProvenance.sections 的下标引用。 */
  semanticSectionOrdinal: number;
  providerLocator: ProviderPayloadLocator | null;
  transformation: ProviderRequestTransformation;
  mappingPrecision: ProviderMappingPrecision;
};

export type ProviderRequestProvenance = {
  schemaVersion: typeof MODEL_CALL_PAYLOAD_SCHEMA_VERSION;
  /** api/protocol id（与 record.model.api 对齐）。 */
  protocol: string;
  mappings: ProviderRequestProvenanceMapping[];
};

/* ── Semantic Input Provenance 契约（Phase 5 wire 形状）────────────────── */

export const SEMANTIC_INPUT_PROVENANCE_SCHEMA_VERSION = 1;

/** Category 有限枚举：每个值必须有真实生产使用点。 */
export const SEMANTIC_INPUT_CATEGORIES = [
  "platform_instruction", // agent system prompt 的平台/环境/运行纪律块
  "persona", // identity + yuan + AGENTS.md 人格；样貌
  "user_profile", // user.md 用户档案
  "memory_context", // 记忆规则/置顶/长期记忆/记忆域上下文
  "skill_instruction", // SDK 注入的 skills prompt（identity-only）
  "agents_file", // SDK 注入的 agents files（identity-only）
  "session_instruction", // appendSystemPrompt / 会话时间快照等会话级指令
  "agent_roster", // 团队 agent 名单
  "conversation_history", // 历史对话消息（user/assistant/tool continuation）
  "current_user_input", // 本 turn 触发输入
  "tool_definition", // tools[i] 定义
  "tool_result", // role=toolResult 消息
  "task_instruction", // utility/compaction/summarizer 的任务指令
  "task_input", // 任务的数据输入段
  "format_constraint", // 格式约束/修复指令
  "previous_summary", // 上一次摘要/快照
  "compaction_summary", // compaction 草稿/检查点类内容
  "media_prompt", // 图片/视频生成 prompt
  "media_reference", // 参考图/参考媒体（locator 指向参数位置，不含值）
  "audio_input", // 语音识别音频
  "language_hint", // 语音识别语言提示
  "adapter_injected", // adapter 在系统为空时注入的真实文本指令
  "sdk_internal", // SDK 内部拼装、Lingxi 无法拆分的输入段
  "unknown", // 诚实兜底
] as const;
export type SemanticInputCategory = typeof SEMANTIC_INPUT_CATEGORIES[number];

/** Semantic role 有限枚举：category 与 role 是两个正交维度。 */
export const SEMANTIC_INPUT_ROLES = [
  "system",
  "developer",
  "user",
  "assistant",
  "tool",
  "input",
  "parameter",
] as const;
export type SemanticInputRole = typeof SEMANTIC_INPUT_ROLES[number];

/** Source type 有限枚举。 */
export const SEMANTIC_SOURCE_TYPES = [
  "template",
  "runtime",
  "snapshot",
  "memory",
  "skill",
  "tool",
  "sdk",
  "adapter",
  "unknown",
] as const;
export type SemanticSourceType = typeof SEMANTIC_SOURCE_TYPES[number];

/** Semantic Request 的根容器。 */
export const SEMANTIC_INPUT_ROOTS = [
  "systemPrompt",
  "messages",
  "tools",
  "input",
  "parameters",
] as const;
export type SemanticInputRoot = typeof SEMANTIC_INPUT_ROOTS[number];

/** Semantic Input 整体形状（Observer summary 的 inputShape，闭集）。 */
export const SEMANTIC_INPUT_SHAPES = [
  "chat_context", // Pi streamFn 三元组
  "calltext", // callText 归一化后形状
  "pi_direct_summary", // generateSummary 参数
  "media_image",
  "media_video",
  "external_cli_media",
  "speech_transcribe",
  "provider_probe",
] as const;
export type SemanticInputShape = typeof SEMANTIC_INPUT_SHAPES[number];

export type SemanticInputSpan = {
  /** UTF-16 code unit，含。 */
  start: number;
  /** UTF-16 code unit，不含。 */
  end: number;
};

export type SemanticInputLocator = {
  root: SemanticInputRoot;
  /** root 内的寻址路径（messages/tools 用 index，input/parameters 用 key）。 */
  path?: Array<number | string>;
  /**
   * 文本 span；null = identity-only（知道存在与类别，无法定位到具体 span，
   * precision 必须为 structural/opaque）。非文本根可为 undefined。
   */
  span?: SemanticInputSpan | null;
};

export type SemanticInputSource = {
  type: SemanticSourceType;
  /** 安全逻辑 id（persona / memory.today / skill:<id> / tool:<name>…）。 */
  id?: string | null;
  /** 真实存在的版本（如 templateVersion）；没有则省略。 */
  version?: string | null;
};

export type SemanticInputProvenanceSection = {
  category: SemanticInputCategory;
  role?: SemanticInputRole | null;
  precision: "exact" | "structural" | "opaque";
  locator: SemanticInputLocator;
  source?: SemanticInputSource | null;
};

export type SemanticInputProvenancePrecision = "exact" | "partial" | "opaque";

export type ModelSemanticInputProvenance = {
  schemaVersion: typeof SEMANTIC_INPUT_PROVENANCE_SCHEMA_VERSION;
  inputShape: SemanticInputShape;
  /** 按 Semantic Request 实际顺序排列；ordinal = 数组下标。 */
  sections: SemanticInputProvenanceSection[];
};

/** 单 call 的 section 上限（消息级/工具级粒度；超限折叠尾段）。 */
export const MAX_PROVENANCE_SECTIONS = 1024;

/* ════════════════════════════════════════════════════════════════════════
 * D. Settings / Health / Export / Blob wire DTO
 * ════════════════════════════════════════════════════════════════════════ */

export type ModelObservabilityRetentionDays = {
  traceDays: number;
  payloadDays: number;
  blobDays: number;
};

/** 用户 desired 配置（preferences.json model_observability 的 normalized 形状）。 */
export type ModelObservabilityDesiredSettings = {
  enabled: boolean;
  persistTraceMetadata: boolean;
  persistPayloads: boolean;
  persistBlobs: boolean;
  retention: ModelObservabilityRetentionDays;
};

/** PUT /settings 请求体（全部字段可选，partial merge）。 */
export type ModelObservabilitySettingsUpdateRequest = {
  enabled?: boolean;
  persistTraceMetadata?: boolean;
  persistPayloads?: boolean;
  persistBlobs?: boolean;
  retention?: Partial<ModelObservabilityRetentionDays>;
};

/** 运行时 effective 状态（desired ≠ effective：schema_newer 等显式原因）。 */
export type ModelObservabilityEffectiveState = {
  recordingStatus: "active" | "disabled" | "closed";
  storeDisabledReasonCode: string | null;
  persistTraceMetadata: boolean;
  persistPayloads: boolean;
  persistBlobs: boolean;
  schemaVersion: number | null;
};

export type ModelObservabilitySettingsResponse = {
  desired: ModelObservabilityDesiredSettings;
  effective: ModelObservabilityEffectiveState;
  /** 诚实暴露：filesystem permissions，非密码学加密。 */
  cryptographicallyEncryptedAtRest: boolean;
};

/** PUT /settings 响应 = settings + 即时 query health。 */
export type ModelObservabilitySettingsUpdateResponse = ModelObservabilitySettingsResponse & {
  queryHealth: ModelObservabilityQueryHealth | null;
  queryError: { code: string; message: string; reasonCode?: string } | null;
};

/** GET /health 响应（recording + query 合并；绝不包含正文）。 */
export type ModelObservabilityHealthResponse = {
  recordingStatus: "active" | "disabled" | "closed";
  storeDisabledReasonCode: string | null;
  persistTraceMetadata: boolean;
  persistPayloads: boolean;
  persistBlobs: boolean;
  queuedTraceEvents: number;
  queuedPayloadRecords: number;
  queuedBlobs: number;
  queuedUsageEntries: number;
  droppedTraceEvents: number;
  droppedPayloadRecords: number;
  droppedBlobs: number;
  droppedUsageEntries: number;
  writeFailures: number;
  maintenanceErrors: number;
  lastSuccessfulFlushAt: string | null;
  interruptedByRestartCalls: number;
  atRestEncryption: boolean;
  query: ModelObservabilityQueryHealth;
};

/* ── Export wire ──────────────────────────────────────────────────────── */

export const MODEL_OBSERVABILITY_EXPORT_SCHEMA_VERSION = 1;
export const MODEL_OBSERVABILITY_EXPORT_DEFAULT_MAX_CALLS = 50_000;
export const MODEL_OBSERVABILITY_EXPORT_MAX_CALLS_LIMIT = 100_000;

/** POST /export 请求体。默认 metadata-only；系统不存在 includeRaw。 */
export type ModelObservabilityExportRequest = {
  query?: {
    filter?: ModelObservabilityCallFilterInput;
    sort?: ModelObservabilitySortKey;
    limit?: number;
  };
  includePayloads?: boolean;
  maxCalls?: number;
};

export type ModelObservabilityExportManifest = {
  type: "manifest";
  exportSchemaVersion: number;
  exportedAt: string;
  includePayloads: boolean;
  storageSchemaVersion: number | null;
  totalCalls: number;
  backfillSource: string | null;
  dataCompleteness: Record<string, number> | null;
};

export type ModelObservabilityExportCallBundle = {
  type: "model_call";
  schemaVersion: number;
  call: unknown;
  trace: unknown;
  attempts: unknown;
  usage: unknown;
  payloads: unknown;
};

/* ── Blob wire（Phase 9 exact retrieval；只有 id 寻址，无 list/search）─── */

/** blobId 安全格式（mb_ 前缀 + bounded token）；非法一律 400。 */
export const MODEL_OBSERVABILITY_BLOB_ID_PATTERN = /^mb_[A-Za-z0-9]{4,96}$/;

/** Blob 响应只允许这些安全媒体主类型；其余一律 application/octet-stream。 */
export const MODEL_OBSERVABILITY_BLOB_SAFE_MEDIA_MAJOR = ["image", "audio", "video"] as const;

/* ── API error wire（Phase 8/9 error contract，§十一）─────────────────── */

/**
 * 顶层 error 字段的闭集（route handler 应答的 {error, code, …} 第一层）。
 * `code` 字段更细（unknown_field / absent / limit_error / …），本类型是
 * UI 分支判定用的粗粒度类别。
 */
export type ModelObservabilityApiErrorKind =
  | "invalid_query"
  | "invalid_cursor"
  | "invalid_json"
  | "not_initialized"
  | "not_found"
  | "export_limit"
  | "query_failed"
  | "local_only_route"
  | "studio_owner_required"
  | "forbidden"
  | "invalid_blob_id"
  | "blob_missing";

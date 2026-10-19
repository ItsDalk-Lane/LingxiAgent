/**
 * GENERATED FILE — DO NOT EDIT.
 * Authority: rust/crates/lingxi-protocol (serde types).
 * Regenerate: cargo run -p lingxi-protocol --bin lingxi-protocol-gen
 * Verify:     cargo run -p lingxi-protocol --bin lingxi-protocol-gen -- --check
 */

export type SidebarSessionListRowMode = "two-line" | "single-line";

export type SidebarProjectViewPreferences = {
  collapsedFolderIds: string[];
  collapsedProjectIds: string[];
  showAllProjectIds: string[];
};

export type SidebarSessionListPreferences = {
  rowMode: SidebarSessionListRowMode;
};

export type SidebarUiPreferences = {
  projectView: SidebarProjectViewPreferences;
  sessionList: SidebarSessionListPreferences;
};

export type CheckpointArchiveMetadata = {
  id: string;
  path: string;
  reason: string;
  sessionPath?: string | null;
  size: number;
  source: string;
  tool: string;
  ts: number;
};

export type McpAppDescriptor = {
  _meta?: unknown;
  connectorId: string;
  description: string;
  inputSchema: unknown;
  outputSchema?: unknown;
  resourceUri: string;
  serverId: string;
  title: string;
  toolName: string;
  visibility: string[];
};

/** 页面读取同一管理者的公开配置；回执保留各动作原有未知扩展。 */
export type McpSettingsState = {
  agentConfig: unknown;
  agentId?: string | null;
  config: unknown;
  settingsResults: unknown[];
};

export type McpSettingsAction = "mcp.global.enabled" | "mcp.global.defer" | "mcp.connector.add" | "mcp.connector.add_bulk" | "mcp.connector.update" | "mcp.connector.remove" | "mcp.connector.start" | "mcp.connector.stop" | "mcp.connector.refresh_tools" | "mcp.agent.connector.enable" | "mcp.agent.tool.enable";

export type McpSettingsChange = {
  after: string;
  before: string;
  key: string;
  label: string;
};

export type McpSettingsFailure = {
  code: string;
  message: string;
};

export type McpSettingsStatus = "applied" | "failed" | "cancelled";

/** saved 与 runtimeApplied 分开记录；失败或取消不抹去已保存的用户决定。 */
export type McpSettingsUpdate = {
  action: string;
  agentId?: string | null;
  changes?: McpSettingsChange[];
  connectorId?: string | null;
  error?: McpSettingsFailure | null;
  key: string;
  runtimeApplied: boolean;
  saved: boolean;
  status: McpSettingsStatus;
  summary: string;
  title: string;
};

export type McpConnectorImportResult = {
  error?: string | null;
  id?: string | null;
  ok: boolean;
  runtimeApplied: boolean;
  saved: boolean;
};

export type SettingsArchiveFormat = "lingxi-settings";

export type AgentActivityKind = "subagent" | "workflow" | "workflow_agent" | "workflow_step" | "heartbeat" | "cron";

export type AgentActivityStatus = "running" | "done" | "failed" | "aborted";

export type WorkflowStepKind = "parallel" | "pipeline" | "log";

/** 快照与实时活动使用同一条目。扩展字段原样保留，归属及状态必须可解析。 */
export type AgentActivityEntry = {
  access?: string | null;
  agentId: string | null;
  agentName: string | null;
  childSessionId?: string | null;
  childSessionPath: string | null;
  finishedAt: number | null;
  id: string;
  kind: AgentActivityKind;
  label?: string | null;
  parentTaskId?: string | null;
  phaseLabel?: string | null;
  sessionId?: string | null;
  sessionPath: string | null;
  startedAt: number | null;
  status: AgentActivityStatus;
  stepKind?: WorkflowStepKind | null;
  summary: string | null;
  threadId?: string | null;
  threadKind?: string | null;
  tokens?: number | null;
  [key: string]: unknown;
};

export type LoopState = "running" | "paused" | "stopped" | "completed";

export type AutomationNotificationStatus = "queued" | "started" | "confirmed" | "unknown" | "failed";

export type AutomationNotificationReceipt = {
  channel: string;
  createdAt: number;
  deliveryId: string;
  error?: string | null;
  payloadJson: string;
  platformReceipt?: unknown;
  recipient: string;
  runId?: string | null;
  status: AutomationNotificationStatus;
  triggerId?: string | null;
  updatedAt: number;
};

export type AutomationDisplayReceipt = {
  channel: string;
  deliveryId: string;
  displayed: boolean;
};

export type BeautifyUploadedImage = {
  contentBase64: string;
  filename?: string | null;
};

export type BeautifyWorkbenchTarget = {
  kind: string;
  mountId?: string | null;
  name: string;
  rootId?: string | null;
  subdir?: string;
};

export type MediaArtifact = {
  fileId: string;
  filename: string;
  mime: string;
  sizeBytes: string;
};

export type MediaJobPhase = "submitting" | "submitted" | "generating" | "downloading" | "completed" | "failed" | "cancelled" | "unknown";

export type MediaKind = "image" | "video" | "speech";

export type MediaRemoteCancel = "unsupported" | "confirmed" | "unknown";

export type MediaRemoteOutcome = "failed" | "not_yet_confirmed" | "not_submitted" | "accepted" | "completed" | "unknown";

export type MediaJobReceipt = {
  createdAtMs: string;
  errorCode?: string | null;
  executionRunId?: string | null;
  files: MediaArtifact[];
  kind: MediaKind;
  localDeliveryStopped: boolean;
  message?: string | null;
  provider: string;
  remoteCancel?: MediaRemoteCancel | null;
  remoteOutcome: MediaRemoteOutcome;
  revision: string;
  sourceRunId: string;
  status: MediaJobPhase;
  taskId: string;
  updatedAtMs: string;
};

export type BrowserAction = "start" | "stop" | "navigate" | "snapshot" | "screenshot" | "click" | "type" | "scroll" | "select" | "key" | "wait" | "evaluate" | "show" | "back" | "forward" | "reload" | "new_tab" | "switch_tab" | "close_tab" | "suspend" | "resume" | "destroy" | "takeover" | "release";

export type BrowserOpenBehavior = "smart" | "current_tab" | "new_tab";

export type BrowserTab = {
  canGoBack: boolean;
  canGoForward: boolean;
  createdAt: string;
  tabId: string;
  title: string;
  updatedAt: string;
  url?: string | null;
};

export type BrowserWorkspace = {
  activeTabId?: string | null;
  executionNode: string;
  resumable: boolean;
  revoked: boolean;
  running: boolean;
  sessionId: string;
  sessionPath: string;
  suspended: boolean;
  tabs: BrowserTab[];
  title?: string | null;
  unavailableReason?: string | null;
  url?: string | null;
  userTakeover: boolean;
  visibleCapable: boolean;
};

export type BridgeAttachmentKind = "image" | "audio" | "video" | "file";

export type BridgeTargetType = "user" | "group" | "channel";

export type BridgeEffectOperation = "receipt_start" | "receipt_stop" | "stream_update" | "stream_finish" | "stream_cancel";

export type BridgeEffectRequest = {
  conversationId: string;
  deliveryId?: string | null;
  effectId: string;
  inboxId: string;
  operation: BridgeEffectOperation;
  replyContext: BridgeReplyContext;
  rich: boolean;
  runId: string;
  sequence: number;
  streamId: string;
  text: string;
};

export type BridgeReplyContext = {
  channelId?: string | null;
  contextToken?: string | null;
  guildId?: string | null;
  isGroup?: boolean | null;
  messageId?: string | null;
  messageThreadId?: number | null;
  parentId?: string | null;
  rootId?: string | null;
  senderId?: string | null;
  targetChatId?: string | null;
  targetType?: BridgeTargetType | null;
  text?: string | null;
};

export type BridgeAttachment = {
  attachmentId: string;
  filename?: string | null;
  kind: BridgeAttachmentKind;
  mime?: string | null;
  platformRef?: string | null;
  sizeBytes?: number | null;
};

export type BridgeConnectionStatus = "connecting" | "connected" | "disconnected";

export type BridgeConversationKind = "dm" | "group";

export type BridgeEffectCapabilities = {
  receipt: BridgeWaitingReceiptCapability;
  streams: BridgeStreamCapability[];
};

export type BridgeEffectReceipt = {
  deliveryId?: string | null;
  effectId: string;
  error?: string | null;
  operation: BridgeEffectOperation;
  platformAck: boolean;
  platformMessageIds: string[];
  sequence: number;
  status: BridgeEffectStatus;
  streamId: string;
};

export type BridgeEffectStatus = "confirmed" | "failed" | "unknown" | "skipped";

export type BridgeReceiptStatus = "confirmed" | "failed" | "unknown";

export type BridgeStreamCapability = {
  maxChars: number;
  minIntervalMs: number;
  mode: BridgeStreamMode;
  receiptFolded: boolean;
  rich: boolean;
  scopes: BridgeConversationKind[];
};

export type BridgeStreamMode = "batch" | "draft" | "rich_draft" | "edit_message" | "cardkit_stream" | "block";

export type BridgeWaitingReceiptCapability = {
  cancellable: boolean;
  mode: BridgeWaitingReceiptMode;
  refreshIntervalMs: number;
  scopes: BridgeConversationKind[];
};

export type BridgeWaitingReceiptMode = "native_typing" | "text";

export type BridgeDiagnosticInfo = {
  authMode?: string | null;
  credentialOk?: boolean | null;
  msg?: string | null;
  name?: string | null;
  username?: string | null;
};

export type BridgeQrcodeStatus = "waiting" | "scanned" | "confirmed" | "expired" | "error";

/** Machine-diagnosable error code for the protocol error envelope. */
export type ErrorCode = "cursor_expired" | "invalid_message" | "version_incompatible" | "unauthorized" | "forbidden" | "not_found" | "conflict" | "cancelled" | "budget_exceeded" | "upstream_unavailable" | "internal" | "unknown_schema_dialect";

/** Opaque pagination cursor. The server owns its meaning; clients must
treat it as an untranslatable token. An expired/unknown cursor fails with
`cursor_expired`; the client must re-fetch a snapshot (contract §7). */
export type Cursor = string;

/** Identity of one protocol event (dedup key; unique per stream).

Opaque string identifier. Equality and ordering carry no
semantics; the string form is preserved verbatim across
migrations (legacy IDs are never renumbered). On the wire this is
a plain JSON string (`#[serde(transparent)]`). */
export type EventId = string;

/** Monotonic sequence as a decimal string. Values may exceed 2^53-1; consumers must not parse this into a JS number when precision matters. */
export type Seq = string;

/** One entry of a paginated history listing. */
export type HistoryEntry = {
  eventId: EventId;
  eventType: string;
  seq: Seq;
  summary?: string | null;
};

/** Hash of canonical content (protocol §6). `algorithm` is `sha256`;
`canonicalization` names the canonical JSON profile. */
export type ContentDigest = {
  algorithm: string;
  canonicalization: string;
  hex: string;
};

/** Identity of one persisted resource (session file / attachment).

Opaque string identifier. Equality and ordering carry no
semantics; the string form is preserved verbatim across
migrations (legacy IDs are never renumbered). On the wire this is
a plain JSON string (`#[serde(transparent)]`). */
export type ResourceId = string;

export type ResourceKind = "session_file" | "attachment" | "artifact" | "export";

/** Reference to a real, authorized resource (contract §3: ResourceService
owns real files and grants; a model claiming a file exists never creates
a resource reference by itself). */
export type ResourceRef = {
  digest?: ContentDigest | null;
  displayName?: string | null;
  kind: ResourceKind;
  resourceId: ResourceId;
  /** Byte size as a decimal string (precision rule: all u64 on the wire
are strings). */
  sizeBytes?: string | null;
  uri?: string | null;
};

/** One block of normalized message content.

`opaque` preserves provider-specific state (e.g. reasoning signatures /
opaque blocks) **verbatim** — contract §6 forbids flattening such state
into ordinary text or dropping it for uniformity. */
export type ContentBlock = ({
  data: unknown;
  provider: string;
  type: "opaque";
}) | ({
  resource: ResourceRef;
  type: "resource_ref";
}) | ({
  text: string;
  type: "reasoning";
}) | ({
  text: string;
  type: "text";
});

/** Identity of one model call. Run != ModelCall != ToolCall.

Opaque string identifier. Equality and ordering carry no
semantics; the string form is preserved verbatim across
migrations (legacy IDs are never renumbered). On the wire this is
a plain JSON string (`#[serde(transparent)]`). */
export type ModelCallId = string;

export type PrincipalKind = "local_user" | "device" | "web_session" | "bridge" | "bridge_takeover" | "automation";

/** Identity of one attempt within a run (retry generation fence).

Opaque string identifier. Equality and ordering carry no
semantics; the string form is preserved verbatim across
migrations (legacy IDs are never renumbered). On the wire this is
a plain JSON string (`#[serde(transparent)]`). */
export type AttemptId = string;

/** Budget facts pinned to a model request (contract §6). */
export type Budget = {
  deadlineUnixMs?: string | null;
  maxCostMicros?: string | null;
  maxTokens?: string | null;
};

/** Authenticated principal reference on the wire (identity facts only;
credential material never crosses the wire). */
export type PrincipalRef = {
  kind: PrincipalKind;
  principalId: string;
};

/** Identity of one user task execution (one Run).

Opaque string identifier. Equality and ordering carry no
semantics; the string form is preserved verbatim across
migrations (legacy IDs are never renumbered). On the wire this is
a plain JSON string (`#[serde(transparent)]`). */
export type RunId = string;

/** Identity of one tool call.

Opaque string identifier. Equality and ordering carry no
semantics; the string form is preserved verbatim across
migrations (legacy IDs are never renumbered). On the wire this is
a plain JSON string (`#[serde(transparent)]`). */
export type ToolCallId = string;

/** Structured protocol error envelope.

`retryable` is advisory for clients; safety-critical retries (external
side effects) additionally require idempotency keys or state
verification, which is a kernel/service concern, not a wire concern.
`details` carries machine-readable diagnostics (e.g. supported version
ranges on `version_incompatible`); extensions to an error go there, so
the envelope itself stays closed to unknown fields. */
export type ProtocolError = {
  code: ErrorCode;
  details?: Record<string, unknown> | null;
  message: string;
  retryable: boolean;
};

export type ToolResultStatus = "success" | "failed" | "cancelled" | "unknown";

export type ApprovalDecision = {
  approvalId: string;
  approved: boolean;
  /** Who decided (principal id); set by the service auth layer, never by
the model or the frontend. */
  decidedBy: string;
  reason?: string | null;
};

/** Approval request: bound to target, principal, run, normalized argument
digest, resources, generation, expiry and use count. A JSON field saying
`"approved": true` is never by itself an authorization — the kernel-side
`PreparedInvocation` is not forgeable from model output. */
export type ApprovalRequest = {
  approvalId: string;
  argsDigest: ContentDigest;
  attempt: AttemptId;
  expiresAtUnixMs: string;
  generation: string;
  principal: PrincipalRef;
  remainingUses?: number | null;
  resources?: ResourceRef[];
  runId: RunId;
  target: string;
};

/** Canonical assistant phase, aligned with the incumbent
`server/assistant-event-normalizer.ts` vocabulary (S09). There is exactly
one authoritative MOOD/thinking-tag parsing entry; clients never
re-clean phases themselves. */
export type AssistantPhase = "reasoning" | "commentary" | "final_answer" | "unresolved";

/** A normalized assistant/user message as committed to history. */
export type NormalizedMessage = {
  content: ContentBlock[];
  modelCallId?: ModelCallId | null;
  role: string;
};

/** Lifecycle state of one Run (taskbook `02_目标架构与强制契约.md` §4).

Terminal states never migrate back to active states. Completion state
and delivery quality are separate facts: a run may complete without a
final assistant message, and that must not be fabricated. */
export type RunStatus = "interrupted_needs_attention" | "queued" | "running" | "waiting_approval" | "cancelling" | "cancelled" | "completed" | "failed";

export type SegmentKind = "mood" | "text" | "reasoning";

/** Wire descriptor of one tool call as announced at start. */
export type ToolCallDescriptor = {
  argsDigest: ContentDigest;
  argsSummary?: string | null;
  /** Registry target id at the pinned generation. */
  target: string;
  toolCallId: ToolCallId;
};

/** Structured result of one tool call (contract §5: distinguishes
success/failed/cancelled/unknown, carries content blocks, resource refs,
truncation and the error with retry semantics). */
export type ToolResultWire = {
  content?: ContentBlock[];
  /** 可选业务字段；旧消息缺字段时仍可读取。 */
  details?: Record<string, unknown> | null;
  error?: ProtocolError | null;
  resourceRefs?: ResourceRef[];
  status: ToolResultStatus;
  truncated: boolean;
};

/** Token usage of one model call; usage and causal trace belong to exactly
one modelCall record (contract §3). */
export type UsageRecord = {
  inputTokens: string;
  outputTokens: string;
};

/** The known event vocabulary of `lingxi.wire` v1. Closed per variant, but
the vocabulary itself is open through [`EventPayload::Unknown`]. */
export type KnownEventPayload = ({
  decision: ApprovalDecision;
  type: "approval_decided";
}) | ({
  delta: string;
  modelCallId: ModelCallId;
  phase: AssistantPhase;
  type: "model_call_delta";
}) | ({
  delta: string;
  segmentId: string;
  semanticPhase: AssistantPhase;
  type: "assistant_segment_delta";
}) | ({
  from: RunStatus;
  reason?: string | null;
  to: RunStatus;
  type: "run_state_changed";
}) | ({
  kind: SegmentKind;
  segmentId: string;
  semanticPhase: AssistantPhase;
  type: "assistant_segment_start";
}) | ({
  message: NormalizedMessage;
  type: "final_message_committed";
}) | ({
  model: string;
  modelCallId: ModelCallId;
  operation: string;
  provider: string;
  type: "model_call_started";
}) | ({
  modelCallId: ModelCallId;
  type: "model_call_completed";
  usage?: UsageRecord | null;
}) | ({
  request: ApprovalRequest;
  type: "approval_requested";
}) | ({
  result: ToolResultWire;
  toolCallId: ToolCallId;
  type: "tool_call_completed";
}) | ({
  segmentId: string;
  semanticPhase: AssistantPhase;
  type: "assistant_segment_end";
}) | ({
  toolCall: ToolCallDescriptor;
  type: "tool_call_started";
});

export type EventPayload = KnownEventPayload | ({
  type: string;
  [key: string]: unknown;
});

/** Identity of one user session (conversation thread).

Opaque string identifier. Equality and ordering carry no
semantics; the string form is preserved verbatim across
migrations (legacy IDs are never renumbered). On the wire this is
a plain JSON string (`#[serde(transparent)]`). */
export type SessionId = string;

/** Identity of one event stream (per session or per run subscription).

Opaque string identifier. Equality and ordering carry no
semantics; the string form is preserved verbatim across
migrations (legacy IDs are never renumbered). On the wire this is
a plain JSON string (`#[serde(transparent)]`). */
export type StreamId = string;

export type ClientModelSelection = {
  id: string;
  provider: string;
};

export type ClientModelState = "ready" | "unconfigured" | "unavailable";

export type ClientModelSummary = {
  audio: boolean;
  audioTransport?: string | null;
  audioTransportSupported: boolean;
  defaultThinkingLevel?: string | null;
  id: string;
  input: string[];
  isCurrent: boolean;
  name: string;
  provider: string;
  reasoning: boolean;
  thinkingLevels: string[];
  xhigh: boolean;
};

export type ClientAgentSummary = {
  effectiveHomeFolder?: string | null;
  hasAvatar: boolean;
  homeFolder?: string | null;
  id: string;
  isCurrent: boolean;
  isPrimary: boolean;
  memoryMasterEnabled: boolean;
  name: string;
  yuan: string;
};

export type ClientAvatarAvailability = {
  agent: boolean;
  user: boolean;
};

export type KeybindingsPreferences = {
  keybindings: Record<string, string[]>;
};

export type KeybindingsUpdate = {
  keybindings: Record<string, string[]>;
  ok: boolean;
};

export type SidebarUiResponse = {
  sidebarUi: SidebarUiPreferences;
};

export type SidebarUiUpdate = {
  ok: boolean;
  sidebarUi: SidebarUiPreferences;
};

export type CheckpointArchiveList = {
  checkpoints: CheckpointArchiveMetadata[];
};

export type McpSessionPermissionRequest = {
  capability: string;
  sessionId: string;
  targetId: string;
};

export type McpSessionPermissionResponse = {
  capability: string;
  ok: boolean;
  persisted: boolean;
  sessionId: string;
  targetId: string;
};

export type McpAppsQuery = {
  agentId?: string | null;
  connectorId?: string | null;
};

export type McpResourceQuery = {
  agentId?: string | null;
  uri: string;
};

export type McpAppLaunchRequest = {
  agentId?: string | null;
  input?: unknown;
  launchInput?: unknown;
};

export type McpOAuthStartResponse = {
  sessionId: string;
  url: string;
};

export type McpOAuthPollResponse = {
  credentialPersistence?: string | null;
  error?: string | null;
  result?: unknown;
  status: string;
};

export type McpOAuthCancelResponse = {
  cancelled: number;
};

export type McpAppsResponse = {
  apps: McpAppDescriptor[];
  state: McpSettingsState;
};

export type McpAppLaunchResponse = {
  app: McpAppDescriptor;
  binding: unknown;
  connectorId: string;
  description: string;
  launchInput: unknown;
  resourceUri: string;
  serverId: string;
  title: string;
  toolName: string;
  type: string;
  visibility: string[];
};

export type McpOAuthLogoutResponse = {
  connector: unknown;
  state: McpSettingsState;
};

export type McpSettingsRequest = {
  action: McpSettingsAction;
  agentId?: string | null;
  payload?: unknown;
};

export type McpSettingsResult = {
  settingsUpdate: McpSettingsUpdate;
};

export type McpConnectorBulkResult = {
  results: McpConnectorImportResult[];
  settingsUpdate: McpSettingsUpdate;
};

export type InteractiveCardPut = {
  code: string;
  title?: string | null;
  varsCss?: string;
};

export type InteractiveCardPutResponse = {
  frameUrl: string;
  ok: boolean;
};

export type SettingsArchive = {
  config: unknown;
  experiments: Record<string, unknown>;
  format: SettingsArchiveFormat;
  modelPreferences: unknown;
  version: number;
};

export type ModelConfigurationApplyReport = {
  activated: boolean;
  applied: boolean;
  auditRecorded: boolean;
  clearedBindings: string[];
  generation: string;
  projectionConfirmed: boolean;
  saved: boolean;
};

export type AgentActivitySnapshot = {
  entries: AgentActivityEntry[];
};

export type ActivitySummaryRequest = {
  id?: string | null;
};

export type ActivitySummaryResponse = ({
  error: string;
}) | ({
  summary?: string | null;
});

export type LoopStatusSnapshot = {
  maxTurns: number | null;
  pausedReason: string | null;
  prompt: string | null;
  runtimeError?: string | null;
  status: LoopState;
  turnCount: number;
};

export type AutomationNotificationClaim = {
  receipt: AutomationNotificationReceipt;
  reserved: boolean;
};

export type AutomationNotificationCommand = ({
  action: "ack";
  deliveryId: string;
  platformReceipt: AutomationDisplayReceipt;
}) | ({
  action: "claim";
  deliveryId: string;
}) | ({
  action: "failed";
  deliveryId: string;
  error: string;
}) | ({
  action: "unknown";
  deliveryId: string;
  error: string;
});

export type BeautifyApplyRequest = {
  filePath?: string | null;
  image?: BeautifyUploadedImage | null;
  imageFilePath?: string | null;
  requestId?: string | null;
  target?: BeautifyWorkbenchTarget | null;
};

export type BeautifyPresetRequest = {
  filePath?: string | null;
  presetId: string;
  requestId?: string | null;
  target?: BeautifyWorkbenchTarget | null;
};

export type BeautifyGenerateRequest = {
  agentId?: string | null;
  executorAgentId?: string | null;
  filePath?: string | null;
  requestId?: string | null;
  target?: BeautifyWorkbenchTarget | null;
  themeTone?: string | null;
  userGuidance?: string | null;
};

export type MediaJobListResponse = {
  jobs: MediaJobReceipt[];
};

export type MediaJobResponse = {
  job: MediaJobReceipt;
};

export type MediaGenerationUpdate = {
  mediaGeneration: MediaJobReceipt;
  sessionId: string;
};

export type MediaJobCancelRequest = Record<string, never>;

export type BrowserCommand = {
  action: BrowserAction;
  amount?: number | null;
  direction?: string | null;
  expression?: string | null;
  key?: string | null;
  pressEnter?: boolean | null;
  ref?: number | null;
  state?: string | null;
  tabId?: string | null;
  text?: string | null;
  timeout?: number | null;
  url?: string | null;
  value?: string | null;
};

export type BrowserSettings = {
  acceptCookies: boolean;
  agentOpenBehavior: BrowserOpenBehavior;
};

export type BrowserReply = {
  /** 下载是否完成与是否登记为文件分别保留真实回执。 */
  downloads?: unknown[];
  /** 文件已通过正式会话登记；元数据保留兼容字段，不含原始字节。 */
  files?: unknown[];
  result: unknown;
  state: BrowserWorkspace;
};

export type BridgeRole = "owner" | "guest";

export type BridgeWorkerCommand = ({
  attachmentId: string;
  connectorId: string;
  data: string;
  generation: number;
  kind: "media_chunk";
  seq: number;
}) | ({
  attachmentId: string;
  connectorId: string;
  filename?: string | null;
  generation: number;
  kind: "media_begin";
  messageId?: string | null;
  mime?: string | null;
  sizeBytes: number;
}) | ({
  attachmentId: string;
  connectorId: string;
  generation: number;
  kind: "media_end";
}) | ({
  attachments: string[];
  connectorId: string;
  conversationId: string;
  deliveryId: string;
  /** 可信管道在正式派发时填入；同一 worker 生命周期内严格递增。 */
  dispatchSeq?: number | null;
  generation: number;
  kind: "send";
  replyContext?: BridgeReplyContext | null;
  text: string;
}) | ({
  config: unknown;
  connectorId: string;
  generation: number;
  kind: "init";
  platform: string;
}) | ({
  connectorId: string;
  dispatchSeq?: number | null;
  effect: BridgeEffectRequest;
  generation: number;
  kind: "effect";
}) | ({
  connectorId: string;
  generation: number;
  kind: "stop";
});

export type BridgeWorkerEvent = ({
  attachmentId: string;
  connectorId: string;
  data: string;
  generation: number;
  kind: "media_chunk";
  seq: number;
}) | ({
  attachmentId: string;
  connectorId: string;
  filename?: string | null;
  generation: number;
  kind: "media_begin";
  messageId?: string | null;
  mime?: string | null;
  sizeBytes: number;
}) | ({
  attachmentId: string;
  connectorId: string;
  generation: number;
  kind: "media_end";
}) | ({
  attachments: BridgeAttachment[];
  avatarUrl?: string | null;
  connectorId: string;
  conversationId: string;
  conversationKind: BridgeConversationKind;
  displayName?: string | null;
  generation: number;
  kind: "message";
  messageId: string;
  replyContext?: BridgeReplyContext | null;
  senderId: string;
  text: string;
}) | ({
  capabilities?: BridgeEffectCapabilities | null;
  connectorId: string;
  generation: number;
  kind: "ready";
}) | ({
  connectorId: string;
  deliveryId: string;
  error?: string | null;
  generation: number;
  kind: "receipt";
  platformMessageIds: string[];
  status: BridgeReceiptStatus;
}) | ({
  connectorId: string;
  error: string;
  generation: number;
  kind: "error";
}) | ({
  connectorId: string;
  error?: string | null;
  generation: number;
  kind: "connection_status";
  status: BridgeConnectionStatus;
}) | ({
  connectorId: string;
  generation: number;
  kind: "effect_receipt";
  receipt: BridgeEffectReceipt;
}) | ({
  connectorId: string;
  generation: number;
  kind: "stopped";
});

export type BridgeTakeoverScope = {
  actorSubject: string;
  sessionId: string;
};

export type BridgeDiagnosticCommand = ({
  config: unknown;
  kind: "verify";
  platform: string;
}) | ({
  kind: "wechat_qrcode";
}) | ({
  kind: "wechat_qrcode_status";
  qrcodeId: string;
});

export type BridgeDiagnosticResult = {
  baseUrl?: string | null;
  botId?: string | null;
  botToken?: string | null;
  error?: string | null;
  info?: BridgeDiagnosticInfo | null;
  kind: "diagnostic_result";
  ok: boolean;
  qrcodeId?: string | null;
  qrcodeUrl?: string | null;
  status?: BridgeQrcodeStatus | null;
  userId?: string | null;
};

export type ContractVersions = {
  data_epoch: number;
  preload_api: number;
  server_protocol: number;
};

export type ClientHello = {
  /** Optional capability tokens the client wants; unknown tokens are
reported back in `ServerHello::rejected_caps`, never silently
dropped. */
  caps?: string[];
  /** Client kind, e.g. `desktop`, `cli`, `web`. Free-form, informational. */
  clientKind: string;
  /** Client implementation version (informational, e.g. app version). */
  clientVersion: string;
  /** Protocol family; must be `lingxi.wire`. */
  protocol: string;
  /** Highest wire protocol version the client can speak. */
  protocolMax: number;
  /** Lowest wire protocol version the client can speak. */
  protocolMin: number;
};

export type ServerHello = {
  /** Data epoch of the server's stores (mirrors DATA_EPOCH; epoch
migration strategy itself is R01-T07 scope). */
  dataEpoch: number;
  protocol: string;
  /** Capability tokens from `ClientHello::caps` the server does not
recognize. Reported, never silently dropped. */
  rejectedCaps?: string[];
  /** The single negotiated wire protocol version. */
  selectedProtocol: number;
  /** Server identity for diagnostics (kind + version, informational). */
  serverKind: string;
  serverVersion: string;
  wireProtocolMax: number;
  wireProtocolMin: number;
};

export type PageRequest = {
  cursor?: Cursor | null;
  limit?: number | null;
};

export type HistoryPage = {
  items: HistoryEntry[];
  nextCursor?: Cursor | null;
  snapshotSeq: Seq;
};

export type ModelRequest = {
  attempt: AttemptId;
  budget: Budget;
  /** Registry/configuration generation the request snapshot was taken at. */
  configGeneration: string;
  model: string;
  modelCallId: ModelCallId;
  operation: string;
  principal: PrincipalRef;
  provider: string;
  purpose: string;
  runId: RunId;
};

export type EventEnvelope = {
  attempt?: AttemptId | null;
  eventId: EventId;
  eventType: string;
  payload: EventPayload;
  runId?: RunId | null;
  schemaVersion: number;
  seq: Seq;
  sessionId: SessionId;
  streamId: StreamId;
};

export type ToolSchemaDocument = {
  dialect: string;
  schema: unknown;
};

export type ClientContextQuery = {
  agentId?: string | null;
  sessionId?: string | null;
};

export type ClientModelContext = {
  activeModel?: ClientModelSelection | null;
  /** 代次以十进制字符串传输，不能因客户端数值精度丢失归属。 */
  modelGeneration: string;
  modelState: ClientModelState;
  models: ClientModelSummary[];
  thinkingLevel: string;
};

export type MobileBootstrapResponse = {
  activeModel?: ClientModelSelection | null;
  agentName: string;
  agentYuan: string;
  agents: ClientAgentSummary[];
  /** 这里只能放入已归一化的已知外观、排版和聊天布局字段。 */
  appearance: unknown;
  avatars: ClientAvatarAvailability;
  chat: unknown;
  currentAgentId?: string | null;
  currentModel?: ClientModelSelection | null;
  cwdHistory: string[];
  editor: unknown;
  homeFolder?: string | null;
  locale: string;
  memoryEnabled: boolean;
  memoryMasterEnabled: boolean;
  modelGeneration: string;
  modelState: ClientModelState;
  models: ClientModelSummary[];
  permissionMode: string;
  thinkingLevel: string;
  userName: string;
};


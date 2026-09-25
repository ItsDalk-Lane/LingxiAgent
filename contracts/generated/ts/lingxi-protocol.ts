/**
 * GENERATED FILE — DO NOT EDIT.
 * Authority: rust/crates/lingxi-protocol (serde types).
 * Regenerate: cargo run -p lingxi-protocol --bin lingxi-protocol-gen
 * Verify:     cargo run -p lingxi-protocol --bin lingxi-protocol-gen -- --check
 */

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

export type PrincipalKind = "local_user" | "device" | "web_session" | "automation";

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

export type SegmentKind = "text" | "reasoning";

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


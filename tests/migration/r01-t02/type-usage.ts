// Type-level consumer proof for the generated TS bindings (R01-T02).
// Compiled with `tsc --noEmit` (see tsconfig.json next to this file); never
// executed, never imported by any production entry.

import type {
  ClientHello,
  EventEnvelope,
  EventPayload,
  HistoryPage,
  ModelRequest,
  ProtocolError,
  Seq,
  ServerHello,
  ToolSchemaDocument,
} from "../../../contracts/generated/ts/lingxi-protocol.js";

// Seq is a decimal string on the wire; assigning a number must be a
// compile-time error (this file compiling proves the string typing holds).
const seq: Seq = "9007199254741003";

const hello: ClientHello = {
  protocol: "lingxi.wire",
  clientKind: "desktop",
  clientVersion: "0.1.0-proto",
  protocolMin: 1,
  protocolMax: 1,
  caps: ["events.v1"],
};

const serverHello: ServerHello = {
  protocol: "lingxi.wire",
  selectedProtocol: 1,
  wireProtocolMin: 1,
  wireProtocolMax: 1,
  dataEpoch: 1,
  serverKind: "lingxi-proto-server",
  serverVersion: "0.0.0-r01t02",
  rejectedCaps: [],
};

const payload: EventPayload = {
  type: "assistant_segment_delta",
  segmentId: "seg-1",
  delta: "中文增量",
  semanticPhase: "final_answer",
};

// Unknown event payloads stay representable (open fallback branch).
const unknownPayload: EventPayload = {
  type: "future_recall_started",
  permissionHint: "memory.write",
};

const envelope: EventEnvelope = {
  schemaVersion: 1,
  eventId: "evt-1",
  streamId: "stream-1",
  seq,
  sessionId: "sess-1",
  runId: "run-1",
  attempt: "attempt-1",
  eventType: "assistant_segment_delta",
  payload,
};

const page: HistoryPage = {
  items: [{ seq, eventId: "evt-1", eventType: "run_state_changed", summary: null }],
  nextCursor: "cur-1",
  snapshotSeq: seq,
};

const request: ModelRequest = {
  principal: { kind: "local_user", principalId: "local-user:owner-0001" },
  runId: "run-1",
  attempt: "attempt-1",
  modelCallId: "mc-1",
  purpose: "chat-turn",
  provider: "anthropic",
  model: "claude-example-1",
  operation: "chat",
  budget: { maxTokens: "8192", maxCostMicros: null, deadlineUnixMs: null },
  configGeneration: "7",
};

const toolSchema: ToolSchemaDocument = {
  dialect: "json-schema/draft-07",
  schema: { type: "object", "x-vendor-extension": { preserved: true } },
};

const errorBody: ProtocolError = {
  code: "version_incompatible",
  message: "no common version",
  retryable: false,
  details: { supportedMin: 1, supportedMax: 1 },
};

// Referenced so the declarations are not tree-shaken away by linters.
export const usages = [
  hello,
  serverHello,
  envelope,
  unknownPayload,
  page,
  request,
  toolSchema,
  errorBody,
] as const;

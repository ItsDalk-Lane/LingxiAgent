// Type-level consumer proof for the generated TS bindings (R01-T02).
// Compiled with `tsc --noEmit` (see tsconfig.json next to this file); never
// executed, never imported by any production entry.

import type {
  AutomationDisplayReceipt,
  AutomationNotificationClaim,
  AutomationNotificationCommand,
  AutomationNotificationReceipt,
  AutomationNotificationStatus,
  BridgeAttachment,
  BridgeReplyContext,
  BridgeWorkerCommand,
  BridgeWorkerEvent,
  BridgeTakeoverScope,
  BridgeDiagnosticCommand,
  BridgeDiagnosticResult,
  BridgeEffectRequest,
  BridgeEffectReceipt,
  BridgeEffectCapabilities,
  BridgeConnectionStatus,
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

// 通知的领取和显示分别编码；命令不能混入另一动作的字段。
const notificationStatus: AutomationNotificationStatus = "started";
const notification: AutomationNotificationReceipt = {
  deliveryId: "delivery-1", triggerId: "trigger-1", runId: "run-1", channel: "desktop",
  recipient: "user_local", payloadJson: "{}", status: notificationStatus,
  platformReceipt: null, error: null, createdAt: 1_800_000_000_000, updatedAt: 1_800_000_000_001,
};
const notificationClaim: AutomationNotificationClaim = { reserved: true, receipt: notification };
const displayReceipt: AutomationDisplayReceipt = { deliveryId: "delivery-1", channel: "desktop", displayed: true };
const notificationCommands: AutomationNotificationCommand[] = [
  { action: "claim", deliveryId: "delivery-1" },
  { action: "ack", deliveryId: "delivery-1", platformReceipt: displayReceipt },
  { action: "failed", deliveryId: "delivery-1", error: "未显示" },
  { action: "unknown", deliveryId: "delivery-1", error: "展示结果未知" },
];
// @ts-expect-error 领取命令不能包含显示确认字段。
const invalidClaim: AutomationNotificationCommand = { action: "claim", deliveryId: "delivery-1", platformReceipt: displayReceipt };
// @ts-expect-error 未知持久状态不能被当作成功状态。
const invalidStatus: AutomationNotificationStatus = "sent";
void invalidClaim;
void invalidStatus;

// 桥接角色不会出现在消息管道；实际平台上下文和真实回执有闭合类型。
const bridgeContext: BridgeReplyContext = { isGroup: false, messageId: "platform-1", contextToken: "synthetic-context", targetType: "user" };
const bridgeAttachment: BridgeAttachment = { attachmentId: "bytes-1", kind: "file", filename: "资料.txt", mime: "text/plain", sizeBytes: 12 };
const bridgeCommand: BridgeWorkerCommand = { kind: "send", connectorId: "connector", generation: 7, deliveryId: "delivery", conversationId: "dm", text: "回复", replyContext: bridgeContext, attachments: [bridgeAttachment.attachmentId] };
const bridgeMessage: BridgeWorkerEvent = { kind: "message", connectorId: "connector", generation: 7, messageId: "platform-1", conversationId: "dm", conversationKind: "dm", senderId: "sender", text: "问答", replyContext: bridgeContext, attachments: [bridgeAttachment] };
const bridgeReceipt: BridgeWorkerEvent = { kind: "receipt", connectorId: "connector", generation: 7, deliveryId: "delivery", status: "confirmed", platformMessageIds: ["actual-platform-id"] };
const bridgeScope: BridgeTakeoverScope = { actorSubject: "bridge:synthetic", sessionId: "single-selected-desktop" };
const diagnostic: BridgeDiagnosticCommand = { kind: "wechat_qrcode" };
const diagnosticResult: BridgeDiagnosticResult = { kind: "diagnostic_result", ok: true, status: "waiting", qrcodeId: "synthetic-qr", qrcodeUrl: "https://example.invalid/qr" };
const effectCapabilities: BridgeEffectCapabilities = { receipt: { mode: "native_typing", scopes: ["dm"], refreshIntervalMs: 5000, cancellable: false }, streams: [{ mode: "rich_draft", scopes: ["dm"], minIntervalMs: 500, maxChars: 32768, rich: true, receiptFolded: false }] };
const nativeEffect: BridgeEffectRequest = { effectId: "effect-1", streamId: "stream-1", inboxId: "inbox-1", runId: "run-1", sequence: 1, operation: "stream_update", conversationId: "dm", replyContext: bridgeContext, text: "未经最终确认的预览", rich: true };
const effectCommand: BridgeWorkerCommand = { kind: "effect", connectorId: "connector", generation: 7, dispatchSeq: 2, effect: nativeEffect };
const effectReceipt: BridgeEffectReceipt = { effectId: "effect-1", streamId: "stream-1", sequence: 1, operation: "stream_update", status: "confirmed", platformAck: true, platformMessageIds: [] };
const effectEvent: BridgeWorkerEvent = { kind: "effect_receipt", connectorId: "connector", generation: 7, receipt: effectReceipt };
const nativeReady: BridgeWorkerEvent = { kind: "ready", connectorId: "connector", generation: 7, capabilities: effectCapabilities };
const recoveredStatus: BridgeConnectionStatus = "connected";
const recoveredConnection: BridgeWorkerEvent = { kind: "connection_status", connectorId: "connector", generation: 7, status: recoveredStatus };
// @ts-expect-error 消息不能自报角色。
const forgedBridgeMessage: BridgeWorkerEvent = { ...bridgeMessage, role: "owner" };
// @ts-expect-error 仅三种真实发送结果，提交不代表成功。
const forgedBridgeReceipt: BridgeWorkerEvent = { ...bridgeReceipt, status: "sent" };
// @ts-expect-error 效果命令同样不能自报身份权限。
const forgedEffect: BridgeEffectRequest = { ...nativeEffect, role: "owner" };
// @ts-expect-error 原生效果只有明确的四种回执状态。
const forgedEffectReceipt: BridgeEffectReceipt = { ...effectReceipt, status: "sent" };
void forgedBridgeMessage; void forgedBridgeReceipt;
void forgedEffect; void forgedEffectReceipt;
export const bridgeUsages = [bridgeCommand, bridgeMessage, bridgeReceipt, bridgeScope, diagnostic, diagnosticResult, effectCommand, effectEvent, nativeReady, recoveredConnection] as const;

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
  notificationClaim,
  notificationCommands,
] as const;

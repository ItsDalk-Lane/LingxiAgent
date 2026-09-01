import type { ModelObservabilitySourceIdentity } from "../../shared/model-observability-api-contract.ts";

type CallSourceRow = Record<string, unknown>;

function text(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function readableOperation(row: CallSourceRow): string | null {
  const operation = text(row.operation) ?? text(row.call_purpose);
  if (!operation || operation === "unknown") return null;
  return operation;
}

export function inferModelObservabilitySourceKey(row: CallSourceRow): {
  kind: ModelObservabilitySourceIdentity["kind"];
  entityId: string | null;
} {
  const subsystem = text(row.subsystem)?.toLowerCase() ?? "";
  const operation = text(row.operation)?.toLowerCase() ?? "";
  const attributionKind = text(row.attribution_kind)?.toLowerCase() ?? "";
  const sessionId = text(row.session_id);
  const sessionPath = text(row.session_path);
  const childSessionPath = text(row.child_session_path);
  const taskId = text(row.task_id);
  const conversationId = text(row.conversation_id);

  if (text(row.child_agent_id) || text(row.child_session_id) || subsystem === "subagent" || attributionKind.startsWith("subagent")) {
    return { kind: "subagent", entityId: taskId ?? text(row.child_session_id) ?? childSessionPath ?? text(row.child_agent_id) };
  }
  if (
    subsystem.includes("automation")
    || attributionKind.includes("automation")
    || (subsystem === "auxiliary" && operation === "activity_summary")
  ) {
    return { kind: "automation", entityId: taskId ?? sessionId ?? sessionPath ?? childSessionPath };
  }
  if (subsystem === "media" || operation.includes("image") || operation.includes("video")) {
    return { kind: "media", entityId: taskId };
  }
  if (subsystem === "speech" || operation.includes("speech") || operation.includes("transcri")) {
    return { kind: "speech", entityId: taskId };
  }
  if (subsystem === "plugin" || attributionKind === "plugin") {
    return { kind: "plugin", entityId: taskId };
  }
  if (operation.includes("probe") || subsystem.includes("provider_probe")) {
    return { kind: "provider_probe", entityId: taskId };
  }
  if (operation.includes("health") || subsystem.includes("health")) {
    return { kind: "health_check", entityId: taskId };
  }
  if (subsystem === "memory" || attributionKind === "memory" || operation.includes("memory")) {
    return { kind: "memory", entityId: taskId ?? sessionId ?? sessionPath };
  }
  if (subsystem === "diary" || operation.includes("diary")) {
    return { kind: "diary", entityId: taskId ?? sessionId ?? sessionPath };
  }
  if (attributionKind === "phone_conversation" || subsystem === "phone") {
    return { kind: "phone", entityId: conversationId ?? sessionId ?? sessionPath };
  }
  // 强会话标识优先于原始 unknown；这是显示投影，不改写原始来源。
  if (attributionKind === "session" || ((sessionId || sessionPath) && !attributionKind.includes("task"))) {
    return { kind: "chat", entityId: sessionId ?? sessionPath };
  }
  if (taskId) return { kind: "background_task", entityId: taskId };
  if (sessionId || sessionPath) return { kind: "chat", entityId: sessionId ?? sessionPath };
  return { kind: "unknown", entityId: null };
}

export function resolveModelObservabilitySourceIdentity(
  db: any,
  row: CallSourceRow,
): ModelObservabilitySourceIdentity {
  const key = inferModelObservabilitySourceKey(row);
  if (key.entityId) {
    try {
      const snapshot = db.prepare(
        `SELECT title FROM source_identity_snapshots WHERE kind = ? AND entity_id = ?`,
      ).get(key.kind, key.entityId);
      const title = text(snapshot?.title);
      if (title) return { ...key, title, resolution: "snapshot" };
    } catch {
      // v1-v3 只读兼容：表不存在时继续走派生名称。
    }
  }
  const derived = readableOperation(row);
  if (derived) return { ...key, title: derived, resolution: "derived" };
  return { ...key, title: null, resolution: key.kind === "unknown" ? "unknown" : "derived" };
}

import { describe, expect, it, vi } from "vitest";
import {
  inferModelObservabilitySourceKey,
  resolveModelObservabilitySourceIdentity,
} from "../lib/llm/model-observability-source-identity.ts";

function snapshotDb(title?: string) {
  return {
    prepare: vi.fn(() => ({
      get: vi.fn(() => title ? { title } : undefined),
    })),
  };
}

describe("Model Observatory 来源身份", () => {
  it("原始来源未知但有会话身份时仍解析为聊天，并使用标题快照", () => {
    const row = {
      subsystem: "unknown",
      operation: "unknown",
      attribution_kind: "unknown",
      session_id: "session-1",
      session_path: "/sessions/session-1.jsonl",
    };
    expect(inferModelObservabilitySourceKey(row)).toEqual({ kind: "chat", entityId: "session-1" });
    expect(resolveModelObservabilitySourceIdentity(snapshotDb("项目架构讨论"), row)).toEqual({
      kind: "chat",
      entityId: "session-1",
      title: "项目架构讨论",
      resolution: "snapshot",
    });
  });

  it("普通聊天的 conversationId 不会误判成电话对话", () => {
    expect(inferModelObservabilitySourceKey({
      attribution_kind: "session",
      session_id: "session-2",
      conversation_id: "conversation-2",
    })).toEqual({ kind: "chat", entityId: "session-2" });
  });

  it("任务、自动化和子代理保留各自类型，快照缺失时使用可读操作名", () => {
    expect(inferModelObservabilitySourceKey({ attribution_kind: "task", task_id: "task-1" }))
      .toEqual({ kind: "background_task", entityId: "task-1" });
    expect(inferModelObservabilitySourceKey({ subsystem: "automation", task_id: "auto-1" }))
      .toEqual({ kind: "automation", entityId: "auto-1" });
    expect(resolveModelObservabilitySourceIdentity(snapshotDb(), {
      attribution_kind: "subagent_child",
      task_id: "child-1",
      operation: "daily_report",
    })).toEqual({
      kind: "subagent",
      entityId: "child-1",
      title: "daily_report",
      resolution: "derived",
    });
  });

  it("自动化隔离会话路径可关联业务快照，记忆维护不再显示未知", () => {
    expect(inferModelObservabilitySourceKey({
      subsystem: "automation",
      attribution_kind: "automation",
      child_session_path: "/agents/a/activity/run.jsonl",
    })).toEqual({ kind: "automation", entityId: "/agents/a/activity/run.jsonl" });
    expect(inferModelObservabilitySourceKey({
      subsystem: "auxiliary",
      operation: "activity_summary",
      session_id: "activity-session",
    })).toEqual({ kind: "automation", entityId: "activity-session" });
    expect(resolveModelObservabilitySourceIdentity(snapshotDb(), {
      subsystem: "memory",
      operation: "extract_facts",
      attribution_kind: "memory",
    })).toEqual({
      kind: "memory",
      entityId: null,
      title: "extract_facts",
      resolution: "derived",
    });
  });
});

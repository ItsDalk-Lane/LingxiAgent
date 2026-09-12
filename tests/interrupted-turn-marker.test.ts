/**
 * 中断标记（机制 4a）：appendInterruptedTurnMarker。
 *
 * - 向会话历史追加模型可见的合成 user 消息（含核实指引措辞）。
 * - 持久化：buildSessionContext 能读到（模型上下文可见）。
 * - 幂等：历史末尾已是中断标记时不重复写。
 */
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import fs from "fs/promises";
import path from "path";
import os from "os";
import { SessionManager } from "../lib/pi-sdk/index.ts";
import {
  appendInterruptedTurnMarker,
  INTERRUPTED_TURN_MARKER_PREFIX,
} from "../core/interrupted-turn-marker.ts";
import { isHiddenTurnInputMessage } from "../lib/turn-input-presentation.ts";

let tmpDir: string;
let manager: any;

beforeEach(async () => {
  tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), "interrupt-marker-"));
  manager = SessionManager.create("/tmp/workspace", path.join(tmpDir, "sessions"));
});

afterEach(async () => {
  await fs.rm(tmpDir, { recursive: true, force: true });
});

function lastContextMessage() {
  const messages = manager.buildSessionContext().messages;
  return messages[messages.length - 1];
}

describe("appendInterruptedTurnMarker", () => {
  it("追加模型可见的合成 user 消息，含 Codex 式核实指引", () => {
    manager.appendMessage({
      role: "assistant",
      content: [{ type: "text", text: "working on it" }],
      api: "test", provider: "test", model: "test", stopReason: "aborted",
      timestamp: Date.now(),
    });

    expect(appendInterruptedTurnMarker(manager)).toBe(true);

    const last = lastContextMessage();
    expect(last.role).toBe("user");
    const text = last.content[0].text;
    expect(text.startsWith(INTERRUPTED_TURN_MARKER_PREFIX)).toBe(true);
    expect(text).toMatch(/partially executed/i);
    expect(text).toMatch(/in_progress are not actually running/i);
    expect(text).toMatch(/verify the real state/i);
  });

  it("末尾已是中断标记时不重复写（幂等）", () => {
    expect(appendInterruptedTurnMarker(manager)).toBe(true);
    expect(appendInterruptedTurnMarker(manager)).toBe(false);

    const messages = manager.buildSessionContext().messages;
    const markers = messages.filter(
      (m: any) => m.role === "user" && String(m.content?.[0]?.text || "").startsWith(INTERRUPTED_TURN_MARKER_PREFIX),
    );
    expect(markers).toHaveLength(1);
  });

  it("标记之后有新的用户消息则可再次标记（新的一轮停止）", () => {
    appendInterruptedTurnMarker(manager);
    manager.appendMessage({
      role: "user",
      content: [{ type: "text", text: "继续" }],
      timestamp: Date.now(),
    });
    expect(appendInterruptedTurnMarker(manager)).toBe(true);
  });

  it("空会话也能标记（首轮即被停止）", () => {
    expect(appendInterruptedTurnMarker(manager)).toBe(true);
    expect(lastContextMessage().role).toBe("user");
  });

  it("标记走隐藏轮输入惯例：模型可见、UI 不展示（同 <hana-background-result>）", () => {
    appendInterruptedTurnMarker(manager);
    const last = lastContextMessage();
    expect(last.content[0].text.startsWith("<hana-turn-interrupted>")).toBe(true);
    expect(isHiddenTurnInputMessage(last)).toBe(true);
  });

  it("历史末尾是旧版明文标记（开发期已写入）时也不重复写", () => {
    manager.appendMessage({
      role: "user",
      content: [{ type: "text", text: "[Turn interrupted by user] The previous turn was stopped on purpose." }],
      timestamp: Date.now(),
    });
    expect(appendInterruptedTurnMarker(manager)).toBe(false);
  });

  it("manager 缺失：返回 false 而不抛错", () => {
    expect(appendInterruptedTurnMarker(null)).toBe(false);
  });
});

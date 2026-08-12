import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SessionManager } from "../../lib/pi-sdk/index.ts";
import { SessionCoordinator } from "../../core/session-coordinator.ts";
import { LOOP_USER_PROMPT_MESSAGE_TYPE } from "../../lib/loop/loop-messages.ts";

const tmpDirs: string[] = [];
afterEach(() => {
  for (const d of tmpDirs.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

function makeTmpDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "loop-user-prompt-"));
  tmpDirs.push(dir);
  return dir;
}

describe("SessionCoordinator.recordLoopUserPrompt", () => {
  function makeFakeThis() {
    return {
      recordCustomEntry: vi.fn(),
      _d: { emitEvent: vi.fn() },
    };
  }

  it("记录 type:\"custom\" 展示条目并广播 session_user_message 事件", async () => {
    const fakeThis = makeFakeThis();
    await SessionCoordinator.prototype.recordLoopUserPrompt.call(fakeThis, "/s/a.jsonl", "每5分钟检查一次");

    expect(fakeThis.recordCustomEntry).toHaveBeenCalledTimes(1);
    const [sessionPath, customType, data] = fakeThis.recordCustomEntry.mock.calls[0];
    expect(sessionPath).toBe("/s/a.jsonl");
    expect(customType).toBe(LOOP_USER_PROMPT_MESSAGE_TYPE);
    expect(data.prompt).toBe("每5分钟检查一次");
    expect(typeof data.timestamp).toBe("number");

    expect(fakeThis._d.emitEvent).toHaveBeenCalledTimes(1);
    const [event, eventSessionPath] = fakeThis._d.emitEvent.mock.calls[0];
    expect(eventSessionPath).toBe("/s/a.jsonl");
    expect(event).toEqual({
      type: "session_user_message",
      clientMessageId: null,
      message: { text: "每5分钟检查一次", timestamp: expect.any(Number) },
    });
  });

  it("非字符串 prompt 记录为空字符串而不是 undefined", async () => {
    const fakeThis = makeFakeThis();
    await SessionCoordinator.prototype.recordLoopUserPrompt.call(fakeThis, "/s/a.jsonl", undefined);
    expect(fakeThis.recordCustomEntry.mock.calls[0][2].prompt).toBe("");
  });

  it("sessionPath 缺失时抛错，不写记录也不发事件", async () => {
    const fakeThis = makeFakeThis();
    await expect(
      SessionCoordinator.prototype.recordLoopUserPrompt.call(fakeThis, null, "t"),
    ).rejects.toThrow("sessionPath is required");
    expect(fakeThis.recordCustomEntry).not.toHaveBeenCalled();
    expect(fakeThis._d.emitEvent).not.toHaveBeenCalled();
  });
});

describe("loop-user-prompt 的模型输入零污染", () => {
  it("appendCustomEntry 写的循环 prompt 不进 buildSessionContext 产出的上下文（含 reload 后）", () => {
    const dir = makeTmpDir();
    const sessionDir = path.join(dir, "sessions");
    const manager = SessionManager.create(dir, sessionDir);
    manager.appendMessage({ role: "user", content: [{ type: "text", text: "正常用户输入" }] } as any);
    manager.appendCustomEntry(LOOP_USER_PROMPT_MESSAGE_TYPE, { prompt: "绝不应进模型", timestamp: 1760000000000 });
    // SDK 的落盘门控：首个 assistant 消息到达前 session 文件不创建，补一条 assistant
    // 让 custom entry 真正持久化（真实循环流里 kickoff 轮必然产出 assistant 消息）。
    manager.appendMessage({ role: "assistant", content: [{ type: "text", text: "循环已启动" }] } as any);

    // 防护层是 buildSessionContext/sessionEntryToContextMessages（type:"custom" 被排除），
    // 不是 convertToLlm——这里直接钉公开可调的 SessionManager.buildSessionContext()。
    const context = manager.buildSessionContext();
    const serialized = JSON.stringify(context.messages);
    // 对照组：普通 user/assistant 消息确实进了上下文，证明上下文构建本身在工作
    expect(serialized).toContain("正常用户输入");
    expect(serialized).toContain("循环已启动");
    expect(serialized).not.toContain("绝不应进模型");

    // 设计契约：prompt 持久化在磁盘上（供 UI 历史投影），但上下文构建把它排除
    const onDisk = fs.readFileSync(manager.getSessionFile(), "utf8");
    expect(onDisk).toContain("绝不应进模型");

    // reload（从磁盘重开、内存状态重建）后同样不进上下文
    const reopened = SessionManager.open(manager.getSessionFile(), sessionDir);
    const reloadedSerialized = JSON.stringify(reopened.buildSessionContext().messages);
    expect(reloadedSerialized).toContain("正常用户输入");
    expect(reloadedSerialized).not.toContain("绝不应进模型");
  });
});

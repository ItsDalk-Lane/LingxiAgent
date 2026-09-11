import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TaskStore } from "../core/media/task-store.ts";
import { Poller } from "../core/media/poller.ts";

describe("media-gen poller completion event", () => {
  let tmpDir;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-image-poller-"));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  it("emits media-gen:task-done with persisted metadata when an image task succeeds", async () => {
    const generatedDir = path.join(tmpDir, "generated");
    fs.mkdirSync(generatedDir, { recursive: true });
    // 产物必须真实存在且非零字节：完成检查不再相信文件名列表。
    fs.writeFileSync(path.join(generatedDir, "cover.png"), Buffer.from([1, 2, 3, 4]));
    const store = new TaskStore(tmpDir);
    const taskId = "task-cover";
    const events = [];
    const bus = {
      emit: vi.fn((event, sessionPath) => events.push({ event, sessionPath })),
      // 交接回执必须证明 deferred 记录已持久化，否则交付保持待办、事件不发生。
      request: vi.fn(async (type) => (type === "deferred:query" ? null : { ok: true, durable: true })),
    };
    const adapter = {
      query: vi.fn(async () => ({ status: "success", files: ["cover.png"] })),
    };
    const registry = {
      getProtocol: () => adapter,
      get: () => adapter,
    };
    store.add({
      taskId,
      adapterId: "openai",
      providerId: "openai",
      modelId: "gpt-image-1",
      protocolId: "openai-images",
      batchId: "batch-cover",
      type: "image",
      prompt: "cover prompt",
      params: { type: "image", prompt: "cover prompt", ratio: "3:2" },
      sessionPath: "/sessions/a.jsonl",
      metadata: {
        profile: "markdown-cover",
        cover: { targetFilePath: "/vault/note.md" },
      },
    });

    const poller = new Poller({
      store,
      registry,
      bus,
      generatedDir,
      log: { info: vi.fn(), warn: vi.fn(), error: vi.fn() },
      registerSessionFile: vi.fn(() => ({ fileId: "sf1", filePath: path.join(generatedDir, "cover.png") })),
    } as any);

    // 通过运行中的公开入口触发：poller 停止态的旧调用不得结算任何任务。
    poller.start();
    await poller.checkNow(taskId);
    await poller.stop();

    expect(events).toEqual([
      expect.objectContaining({
        sessionPath: "/sessions/a.jsonl",
        event: expect.objectContaining({
          type: "media-gen:task-done",
          taskId,
          files: ["cover.png"],
          metadata: {
            profile: "markdown-cover",
            cover: { targetFilePath: "/vault/note.md" },
          },
        }),
      }),
    ]);
  });
});

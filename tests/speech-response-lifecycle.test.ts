/**
 * F12/P8.3–P8.4：同步产物完成再返回 + 旧 pending 记录恢复（V07–V13）。
 *
 * response 模式：同步校验产物文件（generated 根目录内真实存在）后，返回前
 * 任务即终态 done/completed；不 poller、不 deferred、不发聊天消息。
 * session 模式：既有 deferred／会话投递保留。重启恢复：speech+response+pending
 * 幂等收尾；图片/视频 pending 不受影响。
 */

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UniversalMediaManager } from "../core/media/universal-media-manager.ts";
import { PreferencesManager } from "../core/preferences-manager.ts";

const roots: string[] = [];

function makeRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-speech-lifecycle-"));
  roots.push(root);
  return root;
}

function makePreferences(root: string) {
  const userDir = path.join(root, "user");
  const agentsDir = path.join(root, "agents");
  fs.mkdirSync(userDir, { recursive: true });
  return new PreferencesManager({ userDir, agentsDir });
}

function makeBus() {
  const handlers = new Map();
  return {
    handlers,
    handle: vi.fn((type: string, handler: any) => {
      handlers.set(type, handler);
      return () => handlers.delete(type);
    }),
    subscribe: vi.fn(() => () => {}),
    request: vi.fn(async () => ({})),
    emit: vi.fn(),
  };
}

function makeManager(root: string) {
  const providers = [{
    providerId: "openai",
    displayName: "openai",
    authType: "api-key",
    models: [{ id: "tts-1", protocolId: "openai-audio-speech" }],
    availableModels: [{ id: "tts-1", protocolId: "openai-audio-speech" }],
  }];
  return new UniversalMediaManager({
    lingxiHome: root,
    preferences: makePreferences(root),
    providerRegistry: {
      resolveMediaExecutionTarget: vi.fn((input: any) => Object.freeze({
        modelId: input.modelId,
        modality: input.modality,
        runtimeProviderId: input.runtimeProviderId,
        credentialProviderId: input.runtimeProviderId,
        credentialLaneId: null,
        credentialSource: "provider-registry",
        adapterId: input.adapterId || null,
      })),
      getMediaProviders: vi.fn((capability: string) => (
        capability === "speech_generation" ? providers : []
      )),
      resolveMediaModel: vi.fn(({ providerId, modelId }: any) => ({
        capability: "speech_generation",
        providerId,
        provider: providers[0],
        model: { id: modelId, protocolId: "openai-audio-speech" },
        credentialLane: null,
      })),
    },
    registerSessionFile: () => {},
  });
}

/** 在 generated 根目录里造一个真实产物文件，返回相对文件名。 */
function makeGeneratedFile(root: string, name: string): string {
  const dir = path.join(root, "plugin-data", "image-gen", "generated");
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, name), "audio-bytes");
  return name;
}

function speechAdapter(result: any) {
  return {
    id: "openai-speech",
    protocolId: "openai-audio-speech",
    types: ["speech"],
    submit: vi.fn(async () => result),
  };
}

beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn(async () => new Response(new ArrayBuffer(4), { status: 200 })));
});

afterEach(() => {
  vi.unstubAllGlobals();
  for (const root of roots.splice(0)) {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

describe("F12/P8.3 同步 response 完成（V07–V11）", () => {
  it("V07: response 同步文件成功 → 返回前任务 done/completed，有 completedAt", async () => {
    const root = makeRoot();
    const file = makeGeneratedFile(root, "v07-audio.mp3");
    const adapter = speechAdapter({ taskId: "t-v07", files: [file] });
    const manager = makeManager(root);
    const bus = makeBus();
    manager.start(bus);
    manager.registerAdapter(adapter);

    const result = await manager.submitSpeech({
      input: { prompt: "读一下", provider: "openai", model: "tts-1", delivery: { mode: "response" } },
    });

    expect(result.ok).toBe(true);
    const task = manager.getTask("t-v07");
    expect(task.status).toBe("done");
    expect(task.submitState).toBe("completed");
    expect(typeof task.completedAt).toBe("string");
    expect(task.failReason).toBeNull();
    expect(task.files).toEqual([file]);
    expect(result.tasks[0].files).toEqual([file]);
    manager.stop();
  });

  it("V08: response 模式无 poller/deferred/task:register/chat 消息副作用", async () => {
    const root = makeRoot();
    const file = makeGeneratedFile(root, "v08-audio.mp3");
    const adapter = speechAdapter({ taskId: "t-v08", files: [file] });
    const manager = makeManager(root);
    const bus = makeBus();
    manager.start(bus);
    manager.registerAdapter(adapter);
    const pollerAdd = vi.spyOn((manager as any)._poller, "add");

    await manager.submitSpeech({
      input: { prompt: "读一下", provider: "openai", model: "tts-1", delivery: { mode: "response" } },
    });

    expect(pollerAdd).not.toHaveBeenCalled();
    const requested = (bus.request as any).mock.calls.map(([type]) => type);
    expect(requested).not.toContain("deferred:register");
    expect(requested).not.toContain("task:register");
    expect(bus.emit).not.toHaveBeenCalled();
    manager.stop();
  });

  it("V09: session 模式保留既有会话投递（deferred/task:register/poller 各一次）", async () => {
    const root = makeRoot();
    const file = makeGeneratedFile(root, "v09-audio.mp3");
    const adapter = speechAdapter({ taskId: "t-v09", files: [file] });
    const manager = makeManager(root);
    const bus = makeBus();
    manager.start(bus);
    manager.registerAdapter(adapter);
    const sessionPath = path.join(root, "agents", "hana", "sessions", "s.jsonl");
    fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
    fs.writeFileSync(sessionPath, "{}\n");

    await manager.submitSpeech({
      input: { prompt: "读一下", provider: "openai", model: "tts-1", delivery: { mode: "session" } },
      sessionId: "sess_v09",
      sessionPath,
    });

    const requested = (bus.request as any).mock.calls.map(([type]) => type);
    expect(requested.filter((type: string) => type === "deferred:register")).toHaveLength(1);
    expect(requested.filter((type: string) => type === "task:register")).toHaveLength(1);
    manager.stop();
  });

  it("V10: 文件缺失 → 明确失败，不返回 ok=true 配永久 pending", async () => {
    const root = makeRoot();
    const adapter = speechAdapter({ taskId: "t-v10", files: ["missing-file.mp3"] });
    const manager = makeManager(root);
    const bus = makeBus();
    manager.start(bus);
    manager.registerAdapter(adapter);

    await expect(manager.submitSpeech({
      input: { prompt: "读一下", provider: "openai", model: "tts-1", delivery: { mode: "response" } },
    })).rejects.toThrow(/missing-file\.mp3|file/);
    const task = manager.getTask("t-v10");
    expect(task.status).toBe("failed");
    expect(task.submitState).toBe("failed");
    expect(typeof task.failReason).toBe("string");
    manager.stop();
  });

  it("V10b: 适配器未返回文件 → 明确失败；无 taskId → 提交失败不建任务", async () => {
    const root = makeRoot();
    const adapterNoFiles = speechAdapter({ taskId: "t-v10b" });
    const manager = makeManager(root);
    const bus = makeBus();
    manager.start(bus);
    manager.registerAdapter(adapterNoFiles);

    await expect(manager.submitSpeech({
      input: { prompt: "读一下", provider: "openai", model: "tts-1", delivery: { mode: "response" } },
    })).rejects.toThrow();
    expect(manager.getTask("t-v10b").status).toBe("failed");
    manager.stop();

    const root2 = makeRoot();
    const adapterNoTask = speechAdapter({ files: ["whatever.mp3"] });
    const manager2 = makeManager(root2);
    const bus2 = makeBus();
    manager2.start(bus2);
    manager2.registerAdapter(adapterNoTask);
    await expect(manager2.submitSpeech({
      input: { prompt: "读一下", provider: "openai", model: "tts-1", delivery: { mode: "response" } },
    })).rejects.toThrow(/task id/);
    manager2.stop();
  });

  it("V11: 重复完成幂等（completedAt 不变）；已取消任务不得被改成 done", async () => {
    const root = makeRoot();
    const file = makeGeneratedFile(root, "v11-audio.mp3");
    const adapter = speechAdapter({ taskId: "t-v11", files: [file] });
    const manager = makeManager(root);
    const bus = makeBus();
    manager.start(bus);
    manager.registerAdapter(adapter);

    await manager.submitSpeech({
      input: { prompt: "读一下", provider: "openai", model: "tts-1", delivery: { mode: "response" } },
    });
    const first = manager.getTask("t-v11");
    expect(first.status).toBe("done");

    // 幂等：同一任务的重复完成不改变完成时间、不新增任务
    const second = (manager as any)._store.completeSynchronousSpeechTask("t-v11", {
      files: [file],
      generatedDir: (manager as any)._generatedDir,
    });
    expect(second.ok).toBe(true);
    const after = manager.getTask("t-v11");
    expect(after.completedAt).toBe(first.completedAt);

    // 已取消任务不能被无条件改 done
    (manager as any)._store.update("t-v11", { status: "cancelled", submitState: "cancelled" });
    const cancelled = (manager as any)._store.completeSynchronousSpeechTask("t-v11", {
      files: [file],
      generatedDir: (manager as any)._generatedDir,
    });
    expect(cancelled.ok).toBe(false);
    expect(manager.getTask("t-v11").status).toBe("cancelled");
    manager.stop();
  });
});

describe("F12/P8.4 持久化与旧 pending 恢复（V12–V13）", () => {
  function readTasks(root: string) {
    return JSON.parse(fs.readFileSync(path.join(root, "plugin-data", "image-gen", "tasks.json"), "utf8"));
  }

  function writeTasks(root: string, tasks: any[]) {
    const filePath = path.join(root, "plugin-data", "image-gen", "tasks.json");
    fs.mkdirSync(path.dirname(filePath), { recursive: true });
    fs.writeFileSync(filePath, JSON.stringify(tasks, null, 2));
  }

  function pendingSpeechTask(taskId: string, file: string) {
    return {
      taskId,
      adapterId: "openai-speech",
      providerId: "openai",
      modelId: "tts-1",
      protocolId: "openai-audio-speech",
      batchId: "batch-old",
      type: "speech",
      prompt: "旧任务",
      params: { type: "speech", prompt: "旧任务" },
      sessionId: null,
      sessionPath: null,
      sessionRef: null,
      deliveryMode: "response",
      delivery: { mode: "response" },
      submitState: "submitted",
      status: "pending",
      failReason: null,
      files: [file],
      sessionFiles: [],
      favorited: false,
      createdAt: "2026-01-01T00:00:00.000Z",
      completedAt: null,
    };
  }

  it("V12: 完成任务持久化后重启仍是终态；旧 pending/response 文件在→done、缺→failed；不再请求供应商", async () => {
    const root = makeRoot();
    const file = makeGeneratedFile(root, "v12-audio.mp3");
    const missingFile = makeGeneratedFile(root, "will-be-deleted.mp3");
    const adapter = speechAdapter({ taskId: "t-v12", files: [file] });
    const manager = makeManager(root);
    const bus = makeBus();
    manager.start(bus);
    manager.registerAdapter(adapter);
    await manager.submitSpeech({
      input: { prompt: "读一下", provider: "openai", model: "tts-1", delivery: { mode: "response" } },
    });
    // 先让 TaskStore 按既有 flush 机制完成持久化
    (manager as any)._store.flushSync();
    manager.stop();
    fs.rmSync(path.join(root, "plugin-data", "image-gen", "generated", missingFile));

    // 注入两条旧 pending：文件存在的 + 文件缺失的
    const persisted = readTasks(root);
    writeTasks(root, [
      ...persisted,
      pendingSpeechTask("t-old-ok", file),
      pendingSpeechTask("t-old-missing", missingFile),
    ]);

    // 重启：恢复收尾（不触发任何新模型调用）
    const manager2 = makeManager(root);
    const bus2 = makeBus();
    manager2.start(bus2);
    expect(manager2.getTask("t-v12").status).toBe("done");
    expect(manager2.getTask("t-v12").completedAt).toBeTruthy();

    const recoveredOk = manager2.getTask("t-old-ok");
    expect(recoveredOk.status).toBe("done");
    expect(recoveredOk.submitState).toBe("completed");
    expect(recoveredOk.completedAt).toBeTruthy();

    const recoveredMissing = manager2.getTask("t-old-missing");
    expect(recoveredMissing.status).toBe("failed");
    expect(typeof recoveredMissing.failReason).toBe("string");

    // 恢复不产生新的模型调用/用量记录
    expect((fetch as any).mock.calls).toHaveLength(0);
    const requested = (bus2.request as any).mock.calls.map(([type]) => type);
    expect(requested).not.toContain("deferred:register");
    expect(requested).not.toContain("task:register");
    manager2.stop();
  });

  it("V13: 原图片/视频 pending 任务不被 speech 恢复特例误改", () => {
    const root = makeRoot();
    const file = makeGeneratedFile(root, "v13-audio.mp3");
    const imageTask = {
      ...pendingSpeechTask("t-image-pending", "unrelated.png"),
      type: "image",
      deliveryMode: "session",
      delivery: { mode: "session" },
    };
    const videoTask = {
      ...pendingSpeechTask("t-video-pending", "unrelated.mp4"),
      type: "video",
      deliveryMode: "session",
      delivery: { mode: "session" },
    };
    const sessionSpeechTask = {
      ...pendingSpeechTask("t-speech-session", file),
      deliveryMode: "session",
      delivery: { mode: "session" },
    };
    writeTasks(root, [imageTask, videoTask, sessionSpeechTask]);

    const manager = makeManager(root);
    const bus = makeBus();
    manager.start(bus);

    expect(manager.getTask("t-image-pending").status).toBe("pending");
    expect(manager.getTask("t-video-pending").status).toBe("pending");
    // session 投递的 speech pending 也不属于 response 恢复特例
    expect(manager.getTask("t-speech-session").status).toBe("pending");
    manager.stop();
  });
});

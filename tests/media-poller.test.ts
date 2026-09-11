/**
 * tests/media-poller.test.ts
 *
 * Tests for core/media/poller.ts: shouldCheckThisTick pure function and the
 * Poller class with injectable registry, fake timers, and fake-async detection.
 *
 * 持久化使用真实 TaskStore（临时目录）：poller 的结算是「尝试编号 + 终态
 * 一次性 + 持久交接回执」契约的下游，假 store 无法表达这些不变量。替身只
 * 留在外部边界：适配器（供应商）、bus（投递通道）、readImageSize（文件解码）。
 */

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { shouldCheckThisTick, Poller } from "../core/media/poller.ts";
import { TaskStore } from "../core/media/task-store.ts";

// Mock readImageSize so poller tests don't depend on real file I/O.
vi.mock("../core/media/image-size.ts", () => ({
  readImageSize: vi.fn(async () => null),
}));

// ── shouldCheckThisTick ──────────────────────────────────────────────────────

describe("shouldCheckThisTick", () => {
  it("always returns true for age < 2 min", () => {
    const age = 60 * 1000; // 1 min
    expect(shouldCheckThisTick(age, 1)).toBe(true);
    expect(shouldCheckThisTick(age, 2)).toBe(true);
    expect(shouldCheckThisTick(age, 5)).toBe(true);
  });

  it("returns true only every 3rd tick for age 2-10 min", () => {
    const age = 5 * 60 * 1000; // 5 min
    expect(shouldCheckThisTick(age, 3)).toBe(true);
    expect(shouldCheckThisTick(age, 6)).toBe(true);
    expect(shouldCheckThisTick(age, 1)).toBe(false);
    expect(shouldCheckThisTick(age, 2)).toBe(false);
    expect(shouldCheckThisTick(age, 4)).toBe(false);
  });

  it("returns true only every 6th tick for age >= 10 min", () => {
    const age = 15 * 60 * 1000; // 15 min
    expect(shouldCheckThisTick(age, 6)).toBe(true);
    expect(shouldCheckThisTick(age, 12)).toBe(true);
    expect(shouldCheckThisTick(age, 1)).toBe(false);
    expect(shouldCheckThisTick(age, 3)).toBe(false);
    expect(shouldCheckThisTick(age, 5)).toBe(false);
  });
});

// ── Poller class ─────────────────────────────────────────────────────────────

const tmpDirs: string[] = [];

function makeDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-media-poller-"));
  tmpDirs.push(dir);
  return dir;
}

function makeAdapter(overrides: any = {}) {
  return {
    id: "test-adapter",
    types: ["image"],
    query: vi.fn(async () => ({ status: "pending" })),
    ...overrides,
  };
}

/** 在 generated 目录下写入真实非零字节产物，满足完成检查的文件证据要求。 */
function makeOutputs(generatedDir: string, names: string[]) {
  for (const name of names) {
    fs.writeFileSync(path.join(generatedDir, name), Buffer.from([1, 2, 3, 4]));
  }
}

function makePoller(overrides: any = {}) {
  const dataDir = makeDir();
  const generatedDir = path.join(dataDir, "generated");
  fs.mkdirSync(generatedDir, { recursive: true });
  const store = new TaskStore(dataDir);
  const mockAdapter = overrides.adapter ?? makeAdapter();
  const mockBus = {
    // deferred:query 默认无既有记录；其余交接回执证明已持久化（durable）。
    request: vi.fn(async (type: string) => (type === "deferred:query" ? null : { ok: true, durable: true })),
    emit: vi.fn(),
    ...overrides.bus,
  };
  const mockRegistry = {
    get: vi.fn(() => mockAdapter),
    ...overrides.registry,
  };
  const log = {
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    ...overrides.log,
  };

  const poller = new Poller({
    store,
    registry: mockRegistry,
    bus: mockBus,
    dataDir,
    generatedDir,
    log,
    registerSessionFile: overrides.registerSessionFile,
    usageLedger: overrides.usageLedger,
  });

  return { poller, store, mockBus, mockRegistry, mockAdapter, log, dataDir, generatedDir };
}

/** 经真实 TaskStore 写入任务；add 之后再按需要补状态字段。 */
function seedTask(store: any, overrides: any = {}) {
  const taskId = overrides.taskId ?? "task1";
  store.add({
    taskId,
    adapterId: overrides.adapterId ?? "test-adapter",
    providerId: overrides.providerId ?? null,
    modelId: overrides.modelId ?? null,
    protocolId: overrides.protocolId ?? null,
    batchId: overrides.batchId ?? `batch-${taskId}`,
    type: overrides.type ?? "image",
    prompt: overrides.prompt ?? "a cat in space",
    params: overrides.params ?? {},
    sessionId: overrides.sessionId ?? null,
    sessionPath: overrides.sessionPath ?? null,
    deliveryMode: overrides.deliveryMode ?? "session",
    delivery: overrides.delivery ?? null,
    metadata: overrides.metadata ?? null,
    submitState: overrides.submitState ?? "submitted",
  });
  const patch: any = {};
  for (const key of ["adapterTaskId", "files", "submitState"]) {
    if (Object.prototype.hasOwnProperty.call(overrides, key) && key !== "submitState") patch[key] = overrides[key];
  }
  if (Object.keys(patch).length) store.update(taskId, patch);
  return store.get(taskId);
}

describe("Poller", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    for (const dir of tmpDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
  });

  // ── start / stop ───────────────────────────────────────────────────────────

  it("starts and stops without error", () => {
    const { poller } = makePoller();
    expect(() => poller.start()).not.toThrow();
    expect(() => poller.stop()).not.toThrow();
  });

  it("stop is idempotent", () => {
    const { poller } = makePoller();
    poller.start();
    poller.stop();
    expect(() => poller.stop()).not.toThrow();
  });

  it("running is false before start and after stop", () => {
    const { poller } = makePoller();
    expect(poller.running).toBe(false);
    poller.start();
    expect(poller.running).toBe(true);
    poller.stop();
    expect(poller.running).toBe(false);
  });

  it("recovers pending tasks when logger only exposes log instead of info", () => {
    const { poller, store, log } = makePoller();
    seedTask(store, {
      taskId: "recovered-task",
      prompt: "restore me",
      sessionPath: "/sessions/main.jsonl",
    });
    const fallbackLog = vi.fn();
    (log as any).info = undefined;
    (log as any).log = fallbackLog;

    expect(() => poller.start()).not.toThrow();
    expect(poller.hasPending("recovered-task")).toBe(true);
    expect(fallbackLog).toHaveBeenCalledWith("[media] poller recovered 1 pending task(s)");

    poller.stop();
  });

  // ── add / hasPending ───────────────────────────────────────────────────────

  it("adds a taskId and reports it as pending", () => {
    const { poller, store } = makePoller();
    seedTask(store, { taskId: "task1" });
    poller.start();
    poller.add("task1");
    expect(poller.hasPending("task1")).toBe(true);
    poller.stop();
  });

  it("cancels a task even when the info logger fails", async () => {
    const { poller, store, log } = makePoller({
      log: {
        info: vi.fn(() => {
          throw new Error("logger failed");
        }),
      },
    });
    seedTask(store, { taskId: "task1", sessionPath: "/sessions/main.jsonl" });

    poller.start();
    poller.add("task1");

    await expect(poller.cancel("task1")).resolves.toBeUndefined();
    expect(poller.hasPending("task1")).toBe(false);
    expect(store.get("task1")).toMatchObject({
      status: "cancelled",
      failReason: "user cancelled",
    });

    poller.stop();
  });

  it("re-adding a cancelled task after an explicit retry queries the provider again", async () => {
    const { poller, store, mockAdapter, generatedDir } = makePoller({
      adapter: makeAdapter({ query: vi.fn(async () => ({ status: "success", files: ["retry.png"] })) }),
    });
    makeOutputs(generatedDir, ["retry.png"]);
    seedTask(store, {
      taskId: "task1",
      adapterTaskId: "provider-task-1",
      sessionPath: "/sessions/main.jsonl",
    });

    poller.start();
    poller.add("task1");
    await poller.cancel("task1");
    expect(store.get("task1").status).toBe("cancelled");

    // 只有明确重试才开启新尝试；重新 add 的是新尝试，不是复活旧尝试。
    store.beginAttempt("task1", { submitState: "submitted", adapterTaskId: "provider-task-1", failReason: null });
    poller.add("task1");

    await poller.checkNow("task1");

    expect(mockAdapter.query).toHaveBeenCalledWith("provider-task-1", expect.any(Object));
    expect(store.get("task1").status).toBe("done");
    poller.stop();
  });

  it("returns false for unknown taskId", () => {
    const { poller } = makePoller();
    expect(poller.hasPending("nonexistent")).toBe(false);
  });

  // ── fake-async: task already has files ────────────────────────────────────

  it("skips adapter.query and marks success when task already has files", async () => {
    const mockAdapter = makeAdapter();
    const { poller, store, mockBus, generatedDir } = makePoller({ adapter: mockAdapter });
    makeOutputs(generatedDir, ["img1.png", "img2.png"]);
    seedTask(store, { taskId: "task1", files: ["img1.png", "img2.png"] });

    poller.start();
    poller.add("task1");

    await vi.advanceTimersByTimeAsync(5_000);

    // Adapter query must NOT be called
    expect(mockAdapter.query).not.toHaveBeenCalled();

    // Store must be updated to done
    expect(store.get("task1").status).toBe("done");

    // Bus must receive deferred:resolve with the existing files
    expect(mockBus.request).toHaveBeenCalledWith(
      "deferred:resolve",
      expect.objectContaining({ taskId: "task1", files: ["img1.png", "img2.png"] })
    );

    expect(poller.hasPending("task1")).toBe(false);

    poller.stop();
  });

  // ── real async: task without files → adapter.query ────────────────────────

  it("calls adapter.query on tick when task has no files", async () => {
    const mockAdapter = makeAdapter({
      query: vi.fn(async () => ({ status: "pending" })),
    });
    const usageLedger = {
      start: vi.fn(() => ({ requestId: "media-query-1" })),
      finish: vi.fn(),
      recordError: vi.fn(),
    };
    const { poller, store, dataDir, generatedDir } = makePoller({ adapter: mockAdapter, usageLedger });
    seedTask(store, { taskId: "task1" });

    poller.start();
    poller.add("task1");

    await vi.advanceTimersByTimeAsync(5_000);

    expect(mockAdapter.query).toHaveBeenCalledWith(
      "task1",
      expect.objectContaining({ dataDir, generatedDir })
    );
    // 控制面锁定（§四十八）：媒体任务查询只查已提交任务的状态，不产生模型
    // 用量记录，也不再被计入 usage_missing 统计。
    expect(usageLedger.start).not.toHaveBeenCalled();
    expect(usageLedger.finish).not.toHaveBeenCalled();
    expect(usageLedger.recordError).not.toHaveBeenCalled();

    poller.stop();
  });

  it("passes full task metadata to adapter.query so async video adapters can use both video_id and task_id", async () => {
    const mockAdapter = makeAdapter({
      types: ["video"],
      query: vi.fn(async () => ({ status: "pending" })),
    });
    const { poller, store } = makePoller({ adapter: mockAdapter });
    seedTask(store, {
      taskId: "task_123",
      adapterId: "agnes-videos",
      adapterTaskId: "video_123",
      providerId: "agnes",
      modelId: "agnes-video-v2.0",
      protocolId: "agnes-videos",
      type: "video",
    });

    poller.start();
    poller.add("task_123");

    await vi.advanceTimersByTimeAsync(5_000);

    expect(mockAdapter.query).toHaveBeenCalledWith(
      "video_123",
      expect.objectContaining({
        task: expect.objectContaining({
          taskId: "task_123",
          adapterTaskId: "video_123",
          modelId: "agnes-video-v2.0",
          type: "video",
        }),
      }),
    );

    poller.stop();
  });

  it("does not query while submit is still running and no provider taskId exists", async () => {
    const mockAdapter = makeAdapter({
      query: vi.fn(async () => ({ status: "pending" })),
    });
    const { poller, store } = makePoller({ adapter: mockAdapter });

    poller.start();
    // 提交在途的任务在 start 之后才落库，避免被恢复路径按「提交中断」结算。
    seedTask(store, { taskId: "local-task", submitState: "submitting", adapterTaskId: null });
    poller.add("local-task");

    await vi.advanceTimersByTimeAsync(5_000);

    expect(mockAdapter.query).not.toHaveBeenCalled();
    expect(poller.hasPending("local-task")).toBe(true);

    poller.stop();
  });

  it("queries provider taskId while preserving the local taskId for deferred delivery", async () => {
    const mockAdapter = makeAdapter({
      query: vi.fn(async () => ({ status: "success", files: ["abc.png"] })),
    });
    const { poller, store, mockBus, dataDir, generatedDir } = makePoller({ adapter: mockAdapter });
    makeOutputs(generatedDir, ["abc.png"]);
    seedTask(store, { taskId: "local-task", adapterTaskId: "remote-task" });

    poller.start();
    poller.add("local-task");

    await vi.advanceTimersByTimeAsync(5_000);

    expect(mockAdapter.query).toHaveBeenCalledWith(
      "remote-task",
      expect.objectContaining({ dataDir, generatedDir }),
    );
    expect(mockBus.request).toHaveBeenCalledWith(
      "deferred:resolve",
      expect.objectContaining({ taskId: "local-task", files: ["abc.png"] }),
    );

    poller.stop();
  });

  it("updates store, emits deferred:resolve, and removes from active on adapter success", async () => {
    const mockAdapter = makeAdapter({
      query: vi.fn(async () => ({
        status: "success",
        files: ["abc.png", "def.png"],
      })),
    });

    const { poller, store, mockBus, generatedDir } = makePoller({ adapter: mockAdapter });
    makeOutputs(generatedDir, ["abc.png", "def.png"]);
    seedTask(store, { taskId: "task1" });

    poller.start();
    poller.add("task1");

    await vi.advanceTimersByTimeAsync(5_000);

    expect(store.get("task1")).toMatchObject({ status: "done", files: ["abc.png", "def.png"] });
    expect(mockBus.request).toHaveBeenCalledWith(
      "deferred:resolve",
      expect.objectContaining({ taskId: "task1", files: ["abc.png", "def.png"] })
    );
    expect(poller.hasPending("task1")).toBe(false);

    poller.stop();
  });

  it("treats adapter done status as a completed result", async () => {
    const mockAdapter = makeAdapter({
      query: vi.fn(async () => ({
        status: "done",
        files: ["dashscope.png"],
      })),
    });

    const { poller, store, mockBus, generatedDir } = makePoller({ adapter: mockAdapter });
    makeOutputs(generatedDir, ["dashscope.png"]);
    seedTask(store, { taskId: "task1" });

    poller.start();
    poller.add("task1");

    await vi.advanceTimersByTimeAsync(5_000);

    expect(store.get("task1")).toMatchObject({ status: "done", files: ["dashscope.png"] });
    expect(mockBus.request).toHaveBeenCalledWith(
      "deferred:resolve",
      expect.objectContaining({ taskId: "task1", files: ["dashscope.png"] })
    );
    expect(poller.hasPending("task1")).toBe(false);

    poller.stop();
  });

  it("registers completed generated files as session files when the task has a sessionPath", async () => {
    const registerSessionFile = vi.fn(({ sessionPath, filePath, label, origin, storageKind }) => ({
      id: "sf_generated",
      fileId: "sf_generated",
      sessionPath,
      filePath,
      label,
      origin,
      storageKind,
    }));
    const mockAdapter = makeAdapter({
      query: vi.fn(async () => ({
        status: "success",
        files: ["abc.png"],
      })),
    });
    const { poller, store, mockBus, generatedDir } = makePoller({
      adapter: mockAdapter,
      registerSessionFile,
    });
    makeOutputs(generatedDir, ["abc.png"]);
    seedTask(store, { taskId: "task1", sessionPath: "/sessions/media.jsonl" });

    poller.start();
    poller.add("task1");

    await vi.advanceTimersByTimeAsync(5_000);

    const expectedFilePath = path.join(generatedDir, "abc.png");
    expect(registerSessionFile).toHaveBeenCalledWith(expect.objectContaining({
      sessionPath: "/sessions/media.jsonl",
      filePath: expectedFilePath,
      label: "abc.png",
      origin: "plugin_output",
      storageKind: "plugin_data",
    }));
    expect(store.get("task1").sessionFiles).toEqual([
      expect.objectContaining({
        fileId: "sf_generated",
        sessionPath: "/sessions/media.jsonl",
        filePath: expectedFilePath,
        storageKind: "plugin_data",
        origin: "plugin_output",
      }),
    ]);
    expect(mockBus.request).toHaveBeenCalledWith(
      "deferred:resolve",
      expect.objectContaining({
        taskId: "task1",
        files: ["abc.png"],
        sessionFiles: [expect.objectContaining({ fileId: "sf_generated" })],
      }),
    );

    poller.stop();
  });

  it("registers completed generated files with sessionId when the path locator is absent", async () => {
    const registerSessionFile = vi.fn(({ sessionId, sessionRef, sessionPath, filePath, label, origin, storageKind }) => ({
      id: "sf_generated",
      fileId: "sf_generated",
      sessionId,
      sessionRef,
      sessionPath,
      filePath,
      label,
      origin,
      storageKind,
    }));
    const mockAdapter = makeAdapter({
      query: vi.fn(async () => ({
        status: "success",
        files: ["id-only.png"],
      })),
    });
    const { poller, store, generatedDir } = makePoller({
      adapter: mockAdapter,
      registerSessionFile,
    });
    makeOutputs(generatedDir, ["id-only.png"]);
    seedTask(store, { taskId: "task1", sessionId: "sess_image_task" });

    poller.start();
    poller.add("task1");

    await vi.advanceTimersByTimeAsync(5_000);

    expect(registerSessionFile).toHaveBeenCalledWith(expect.objectContaining({
      sessionId: "sess_image_task",
      sessionRef: { sessionId: "sess_image_task" },
      filePath: path.join(generatedDir, "id-only.png"),
      label: "id-only.png",
      origin: "plugin_output",
      storageKind: "plugin_data",
    }));

    poller.stop();
  });

  it("keeps response delivery results out of SessionFile and DeferredResult delivery", async () => {
    const registerSessionFile = vi.fn();
    const mockAdapter = makeAdapter({
      query: vi.fn(async () => ({
        status: "success",
        files: ["response.png"],
      })),
    });
    const { poller, store, mockBus, generatedDir } = makePoller({
      adapter: mockAdapter,
      registerSessionFile,
    });
    makeOutputs(generatedDir, ["response.png"]);
    seedTask(store, { taskId: "task-response", deliveryMode: "response" });

    poller.start();
    poller.add("task-response");

    await vi.advanceTimersByTimeAsync(5_000);

    expect(registerSessionFile).not.toHaveBeenCalled();
    expect(store.get("task-response")).toMatchObject({
      status: "done",
      files: ["response.png"],
    });
    expect(mockBus.request).not.toHaveBeenCalledWith("deferred:resolve", expect.anything());
    expect(poller.hasPending("task-response")).toBe(false);

    poller.stop();
  });

  it("updates store, emits deferred:fail, and removes from active on adapter failed status", async () => {
    const mockAdapter = makeAdapter({
      query: vi.fn(async () => ({
        status: "failed",
        failReason: "content policy",
      })),
    });

    const { poller, store, mockBus } = makePoller({ adapter: mockAdapter });
    seedTask(store, { taskId: "task1" });

    poller.start();
    poller.add("task1");

    await vi.advanceTimersByTimeAsync(5_000);

    expect(store.get("task1")).toMatchObject({ status: "failed", failReason: "content policy" });
    expect(mockBus.request).toHaveBeenCalledWith(
      "deferred:fail",
      expect.objectContaining({ taskId: "task1" })
    );
    expect(poller.hasPending("task1")).toBe(false);

    poller.stop();
  });

  it("leaves task in active set when adapter returns pending status", async () => {
    const mockAdapter = makeAdapter({
      query: vi.fn(async () => ({ status: "pending" })),
    });

    const { poller, store, mockBus } = makePoller({ adapter: mockAdapter });

    poller.start();
    // start 之后才落库，避免恢复注册的 bus 调用干扰「没有任何投递」断言。
    seedTask(store, { taskId: "task1" });
    poller.add("task1");

    await vi.advanceTimersByTimeAsync(5_000);

    // Still pending, not resolved or failed
    expect(mockBus.request).not.toHaveBeenCalled();
    expect(poller.hasPending("task1")).toBe(true);

    poller.stop();
  });

  it("handles adapter.query throwing and emits deferred:fail", async () => {
    const queryError = new Error("network timeout");
    const mockAdapter = makeAdapter({
      query: vi.fn(async () => { throw queryError; }),
    });

    const { poller, store, mockBus } = makePoller({ adapter: mockAdapter });
    seedTask(store, { taskId: "task1" });

    poller.start();
    poller.add("task1");

    // MAX_CONSECUTIVE_ERRORS = 5; need 5 ticks to exhaust the retry budget.
    for (let i = 0; i < 5; i++) {
      await vi.advanceTimersByTimeAsync(5_000);
    }

    expect(store.get("task1")).toMatchObject({ status: "failed", failReason: "network timeout" });
    expect(mockBus.request).toHaveBeenCalledWith(
      "deferred:fail",
      expect.objectContaining({ taskId: "task1" })
    );
    expect(poller.hasPending("task1")).toBe(false);

    poller.stop();
  });

  // ── cancel ─────────────────────────────────────────────────────────────────

  it("cancel removes task from active, marks cancelled in store, and calls deferred:abort + task:remove", async () => {
    const { poller, store, mockBus } = makePoller();
    seedTask(store, { taskId: "task1", sessionPath: "/sessions/main.jsonl" });

    poller.start();
    poller.add("task1");
    expect(poller.hasPending("task1")).toBe(true);

    await poller.cancel("task1");

    // Removed from active set
    expect(poller.hasPending("task1")).toBe(false);

    // Store updated to cancelled
    expect(store.get("task1")).toMatchObject({ status: "cancelled", failReason: "user cancelled" });

    // Bus calls: deferred:abort and task:remove
    expect(mockBus.request).toHaveBeenCalledWith(
      "deferred:abort",
      expect.objectContaining({ taskId: "task1", reason: "user cancelled" })
    );
    expect(mockBus.request).toHaveBeenCalledWith(
      "task:remove",
      expect.objectContaining({ taskId: "task1" })
    );

    poller.stop();
  });

  it("cancel is a no-op for unknown taskId", async () => {
    const { poller, store, mockBus } = makePoller();
    poller.start();

    await poller.cancel("nonexistent");

    expect(store.listAll()).toEqual([]);
    expect(mockBus.request).not.toHaveBeenCalled();

    poller.stop();
  });

  it("cancelled task is ignored by _checkTask even if query was in-flight", async () => {
    // Simulate: adapter.query is slow, cancel arrives before query returns
    let resolveQuery;
    const mockAdapter = makeAdapter({
      query: vi.fn(() => new Promise((r) => { resolveQuery = r; })),
    });
    const { poller, store, mockBus, generatedDir } = makePoller({ adapter: mockAdapter });
    makeOutputs(generatedDir, ["img.png"]);
    seedTask(store, { taskId: "task1", sessionPath: "/sessions/main.jsonl" });

    poller.start();
    poller.add("task1");

    // Trigger tick — adapter.query starts but hasn't resolved
    await vi.advanceTimersByTimeAsync(5_000);
    expect(mockAdapter.query).toHaveBeenCalled();

    // Cancel while query is in-flight；cancel 会等在途查询回收，先不 await
    const cancelPromise = poller.cancel("task1");
    expect(poller.hasPending("task1")).toBe(false);

    // Now resolve the query — _checkTask should bail out because the attempt is settled
    resolveQuery({ status: "success", files: ["img.png"] });
    await cancelPromise;

    // deferred:resolve should NOT have been called (only deferred:abort and task:remove from cancel)
    const resolveCall = mockBus.request.mock.calls.find(
      ([type]) => type === "deferred:resolve"
    );
    expect(resolveCall).toBeUndefined();
    expect(store.get("task1").status).toBe("cancelled");

    poller.stop();
  });

  // ── recover pending from store on start ───────────────────────────────────

  it("recovers pending tasks from the store on start", async () => {
    const { poller, store, mockBus } = makePoller();
    seedTask(store, {
      taskId: "recovered1",
      prompt: "moon",
      sessionPath: "/sessions/main.jsonl",
    });
    seedTask(store, {
      taskId: "recovered2",
      prompt: "sun",
      sessionPath: "/sessions/main.jsonl",
    });

    poller.start();

    expect(poller.hasPending("recovered1")).toBe(true);
    expect(poller.hasPending("recovered2")).toBe(true);
    // 交接注册是异步的（先查既有记录再注册），等微任务链结算后再断言。
    await vi.waitFor(() => {
      expect(mockBus.request).toHaveBeenCalledWith("deferred:register", expect.objectContaining({
        taskId: "recovered1",
        sessionPath: "/sessions/main.jsonl",
        meta: expect.objectContaining({
          type: "image-generation",
          deliveryIntent: "ui_only",
          triggerParentTurn: false,
          notifyAgentOnFailure: true,
        }),
      }));
    });

    poller.stop();
  });
});

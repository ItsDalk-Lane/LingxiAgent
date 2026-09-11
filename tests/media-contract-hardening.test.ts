import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { PassThrough } from "node:stream";
import { syncBuiltinESMExports } from "node:module";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TaskStore } from "../core/media/task-store.ts";
import { Poller } from "../core/media/poller.ts";
import { retryImageTask, runSubmitInBackground } from "../core/media/image-task-runner.ts";
import { DeferredResultStore } from "../lib/deferred-result-store.ts";
import { registerDeferredResultBusHandlers } from "../server/deferred-result-bus-handlers.ts";

const roots: string[] = [];
const stores: TaskStore[] = [];
const pollers: Poller[] = [];
const deferredStores: DeferredResultStore[] = [];

function root() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-media-contract-"));
  roots.push(dir);
  return dir;
}

function pending<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function taskInput(taskId = "media-task", overrides: Record<string, any> = {}) {
  return {
    taskId, adapterId: "fake", batchId: "batch", type: "speech", prompt: "synthetic",
    params: { prompt: "synthetic", providerId: "fake", modelId: "fake", protocolId: "fake" },
    sessionId: "session", sessionPath: "/synthetic/session.jsonl", ...overrides,
  };
}

function harness({ files = [], query = async () => ({ status: "pending" }), type = "speech" }: any = {}) {
  const dir = root();
  const generatedDir = path.join(dir, "generated");
  fs.mkdirSync(generatedDir);
  const store = new TaskStore(dir);
  stores.push(store);
  store.add(taskInput("media-task", { type }));
  store.update("media-task", { files });
  const deferred = new DeferredResultStore(null, path.join(dir, "deferred.json"));
  deferredStores.push(deferred);
  const handlers = new Map<string, any>();
  const bus = {
    handle: (name, fn) => handlers.set(name, fn),
    request: vi.fn(async (name, payload) => handlers.get(name)?.(payload) ?? { ok: true }),
    emit: vi.fn(),
  };
  registerDeferredResultBusHandlers(bus, deferred);
  const adapter = { id: "fake", query: vi.fn(query) };
  const registry = { get: () => adapter, getProtocol: () => adapter };
  const poller = new Poller({ store, registry, bus, generatedDir, log: {} } as any);
  pollers.push(poller);
  poller.start();
  return { dir, generatedDir, store, poller, adapter, bus, deferred };
}

afterEach(async () => {
  vi.restoreAllMocks();
  syncBuiltinESMExports();
  for (const poller of pollers.splice(0)) await poller.stop();
  for (const store of stores.splice(0)) store.destroy();
  for (const store of deferredStores.splice(0)) store.dispose();
  for (const dir of roots.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

describe("媒体持久化与产物完成合同", () => {
  it.each(["{broken", "{}", '[null]', '[{"taskId":"same"},{"taskId":"same"}]'])
    ("损坏或歧义快照明确失败，原件保持不变：%s", (raw) => {
      const dir = root();
      const filename = path.join(dir, "tasks.json");
      fs.writeFileSync(filename, raw);
      expect(() => new TaskStore(dir)).toThrow();
      expect(fs.readFileSync(filename, "utf8")).toBe(raw);
    });

  it("读取权限错误不得当成新空库", () => {
    const dir = root();
    const filename = path.join(dir, "tasks.json");
    fs.writeFileSync(filename, "[]");
    const original = fs.readFileSync;
    vi.spyOn(fs, "readFileSync").mockImplementation(((file, ...args) => {
      if (file === filename) throw Object.assign(new Error("synthetic denied"), { code: "EACCES" });
      return original(file, ...args);
    }) as any);
    expect(() => new TaskStore(dir)).toThrow(/EACCES|denied/);
  });

  it("正常关闭在延迟保存触发前也保留新任务", () => {
    const dir = root();
    const store = new TaskStore(dir);
    store.add(taskInput());
    store.destroy();
    const restored = new TaskStore(dir);
    stores.push(restored);
    expect(restored.get("media-task")).toMatchObject({ status: "pending" });
  });

  it.each(["missing.mp3", "empty.mp3"])("session 同步产物 %s 无效时不得完成交付", async (filename) => {
    const h = harness({ files: [filename] });
    if (filename === "empty.mp3") fs.writeFileSync(path.join(h.generatedDir, filename), "");
    await h.poller.checkNow("media-task");
    expect(h.store.get("media-task")).toMatchObject({ status: "failed" });
    expect(h.bus.emit).not.toHaveBeenCalledWith(expect.objectContaining({ type: "media-gen:task-done" }), expect.anything());
    expect(h.deferred.query("media-task")).toMatchObject({ status: "failed" });
  });

  it("后台 success 无产物与同步无产物遵守相同失败条件", async () => {
    const h = harness({ query: async () => ({ status: "success", files: [] }) });
    await h.poller.checkNow("media-task");
    expect(h.store.get("media-task")).toMatchObject({ status: "failed" });
    expect(h.deferred.query("media-task")).toMatchObject({ status: "failed" });
  });
});

describe("媒体任务的迟到结果与运行代次", () => {
  it("准备完成期间取消，迟到产物不得覆盖取消或发送成功", async () => {
    const h = harness({ files: ["audio.mp3"] });
    fs.writeFileSync(path.join(h.generatedDir, "audio.mp3"), "synthetic audio");
    // 只延迟操作系统文件读取边界，真实 Poller 与尺寸解析逻辑照常执行。
    const stream = new PassThrough();
    const original = fs.createReadStream;
    vi.spyOn(fs, "createReadStream").mockImplementation(((filename, options) => (
      filename === path.join(h.generatedDir, "audio.mp3") ? stream : original(filename, options)
    )) as any);
    syncBuiltinESMExports();
    const checking = h.poller.checkNow("media-task");
    h.poller.cancel("media-task");
    stream.end(Buffer.from("synthetic audio"));
    await checking;
    expect(h.store.get("media-task").status).toBe("cancelled");
    expect(h.deferred.query("media-task").status).toBe("aborted");
    expect(h.bus.emit).not.toHaveBeenCalled();
  });

  it("同一任务的并发检查只查询一次", async () => {
    const answer = pending<any>();
    const h = harness({ query: () => answer.promise });
    const first = h.poller.checkNow("media-task");
    const second = h.poller.checkNow("media-task");
    expect(h.adapter.query).toHaveBeenCalledTimes(1);
    answer.resolve({ status: "pending" });
    await Promise.all([first, second]);
  });

  it("取消后迟到查询错误不能改写终态", async () => {
    const answer = pending<any>();
    const h = harness({ query: () => answer.promise });
    const checks = Array.from({ length: 5 }, () => h.poller.checkNow("media-task"));
    h.poller.cancel("media-task");
    answer.reject(new Error("late failure"));
    await Promise.all(checks);
    expect(h.store.get("media-task").status).toBe("cancelled");
    expect(h.deferred.query("media-task").status).toBe("aborted");
  });

  it("停止后的旧查询不能完成或投递，重启仍只查询原供应商任务", async () => {
    const answer = pending<any>();
    const h = harness({ query: () => answer.promise });
    fs.writeFileSync(path.join(h.generatedDir, "audio.mp3"), "synthetic audio");
    const checking = h.poller.checkNow("media-task");
    const stopping = h.poller.stop();
    answer.resolve({ status: "success", files: ["audio.mp3"] });
    await Promise.all([checking, stopping]);
    expect(h.store.get("media-task").status).toBe("pending");
    expect(h.bus.emit).not.toHaveBeenCalled();
    h.poller.start();
    await h.poller.checkNow("media-task");
    expect(h.store.get("media-task").status).toBe("done");
    expect(h.adapter.query).toHaveBeenCalledTimes(2);
  });

  it("取消后重试，旧提交不能把供应商任务号写入新尝试", async () => {
    const h = harness({ type: "image" });
    const old = pending<any>();
    const newer = pending<any>();
    const entered = pending<void>();
    const adapter = { id: "fake", submit: vi.fn()
      .mockImplementationOnce(() => { entered.resolve(); return old.promise; })
      .mockImplementationOnce(() => newer.promise) };
    const resolveMediaExecutionTarget = (input) => ({
      modelId: input.modelId, modality: input.modality, runtimeProviderId: "fake",
      credentialProviderId: "fake", credentialLaneId: null, credentialSource: "provider-registry",
      adapterId: "fake", resolutionReason: "runtime_provider_credentials",
    });
    const registry = { get: () => adapter };
    const ctx = {
      dataDir: h.dir, bus: h.bus, log: {}, config: { get: () => null },
      _mediaGen: { registry, store: h.store, poller: h.poller, resolveMediaExecutionTarget },
    };
    const oldRun = runSubmitInBackground({ taskId: "media-task", adapter, params: h.store.get("media-task").params,
      submitCtx: { ...ctx, resolveMediaExecutionTarget }, store: h.store, poller: h.poller, ctx });
    await entered.promise;
    h.poller.cancel("media-task");
    await retryImageTask({ taskId: "media-task", ctx });
    old.resolve({ taskId: "old-provider-job" });
    await oldRun;
    expect(h.store.get("media-task").adapterTaskId).toBeNull();
    newer.resolve({ taskId: "new-provider-job" });
    await vi.waitFor(() => expect(h.store.get("media-task").adapterTaskId).toBe("new-provider-job"));
  });
});

describe("媒体结果的持久交接", () => {
  it("终态已写而交付失败时，重启只补交付，不查询或生成", async () => {
    const h = harness({ files: ["audio.mp3"] });
    fs.writeFileSync(path.join(h.generatedDir, "audio.mp3"), "synthetic audio");
    const request = h.bus.request.getMockImplementation()!;
    h.bus.request.mockImplementation(async (name, payload) => {
      if (name === "deferred:resolve") throw new Error("synthetic handoff interruption");
      return request(name, payload);
    });
    await h.poller.checkNow("media-task");
    expect(h.store.get("media-task").status).toBe("done");
    h.store.flushSync();
    await h.poller.stop();
    const restored = new TaskStore(h.dir);
    stores.push(restored);
    h.bus.request.mockImplementation(request);
    const poller = new Poller({ store: restored, registry: { get: () => h.adapter }, bus: h.bus,
      generatedDir: h.generatedDir, log: {} } as any);
    pollers.push(poller);
    poller.start();
    await poller.checkNow("media-task");
    expect(h.deferred.query("media-task")).toMatchObject({ status: "resolved", result: ["audio.mp3"] });
    expect(h.adapter.query).not.toHaveBeenCalled();
    expect(restored.get("media-task")).toMatchObject({ status: "done", deliveryState: "handed_off" });
  });

  it("交接成功回执必须证明 deferred 记录已写盘", async () => {
    const h = harness();
    await h.bus.request("deferred:register", { taskId: "media-task", sessionId: "session", meta: { mediaAttempt: 1 } });
    const result = await h.bus.request("deferred:resolve", {
      taskId: "media-task", files: ["audio.mp3"], durable: true, expectedAttempt: 1,
    });
    expect(result).toMatchObject({ ok: true, durable: true });
    const raw = JSON.parse(fs.readFileSync(path.join(h.dir, "deferred.json"), "utf8"));
    expect(raw["media-task"]).toMatchObject({ status: "resolved" });
  });
});

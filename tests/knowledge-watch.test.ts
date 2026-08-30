import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import type {
  KnowledgeStatFileFn,
  KnowledgeWatchDirectoryFactory,
} from "../lib/knowledge/source-file-watcher.ts";

const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];

const DEBOUNCE_MS = 1500;
const POLL_INTERVAL_MS = 300_000;
const RETRY_BASE_MS = 1000;

function tempDir(prefix: string) {
  // macOS 的 os.tmpdir() 是 /var 软链：导入安全层存的是 realpath 后的路径，
  // 这里对齐 realpath，否则 watcher 的目录断言/失败注入会对不上。
  const dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), prefix)));
  tempDirs.push(dir);
  return dir;
}

beforeEach(() => {
  // watcher 只用 setTimeout/clearTimeout；setImmediate/Date 保持真实，
  // 让 tickAsync 的让出能驱动真实 fs I/O（stat/read/parse）。
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
});

afterEach(() => {
  for (const manager of managers.splice(0)) manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
  vi.useRealTimers();
});

interface FakeWatchRegistration {
  dir: string;
  onEvent: (eventType: string, fileName: string | null) => void;
  onError: (error: Error) => void;
  closed: boolean;
}

interface WatchHarness {
  registrations: FakeWatchRegistration[];
  /** 目录在集合内时工厂模拟 fs.watch 挂载即失败（目录删除/权限丢失场景）。 */
  failDirs: Set<string>;
  factory: KnowledgeWatchDirectoryFactory;
}

function createWatchHarness(): WatchHarness {
  const harness: WatchHarness = { registrations: [], failDirs: new Set(), factory: null as never };
  harness.factory = ({ dir, onEvent, onError }) => {
    const registration: FakeWatchRegistration = { dir, onEvent, onError, closed: false };
    harness.registrations.push(registration);
    if (harness.failDirs.has(dir)) {
      queueMicrotask(() => onError(Object.assign(new Error("simulated watch attach failure"), { code: "ENOENT" })));
    }
    return { close: () => { registration.closed = true; } };
  };
  return harness;
}

/** 8 维确定性伪嵌入（与 tests/knowledge-ingestion.test.ts 同款），让摄入 job 能跑到 done。 */
function createFakeEmbedder(calls: string[][]) {
  return async ({ texts }: { texts: string[] }) => {
    calls.push([...texts]);
    return {
      vectors: texts.map((text) => {
        const vector = new Array(8).fill(0);
        vector[text.length % 8] = (text.length % 7) + 1;
        return vector;
      }),
      dimensions: 8,
      model: { provider: "fake", id: "emb-1", api: "openai", dimensions: 8 },
    };
  };
}

const FAKE_MODEL_REF = { id: "emb-1", provider: "fake" };

interface ManagerHarness {
  manager: KnowledgeManager;
  logs: string[];
  statCalls: string[];
  embeddingCalls: string[][];
}

function createManager(lingxiHome: string, watch: WatchHarness): ManagerHarness {
  const logs: string[] = [];
  const statCalls: string[] = [];
  const embeddingCalls: string[][] = [];
  const statFile: KnowledgeStatFileFn = async (filePath) => {
    statCalls.push(filePath);
    const stat = await fs.promises.stat(filePath);
    return { mtimeMs: stat.mtimeMs, size: stat.size };
  };
  const manager = new KnowledgeManager({
    lingxiHome,
    ingestionLog: (message) => logs.push(message),
    embedTextsForModel: (request) => createFakeEmbedder(embeddingCalls)(request),
    canEmbedWithModel: () => true,
    // 本文件 fake 了 setTimeout/clearTimeout（watcher 防抖/退避确定性驱动），
    // provider gate 的限流计时器会被冻结：这里显式放宽（间隔 0、上限抬高），
    // 限流行为由 tests/knowledge-lifecycle.test.ts 在真实计时器下覆盖。
    embeddingGate: { maxConcurrent: 8, minRequestIntervalMs: 0 },
    fileWatcher: {
      debounceMs: DEBOUNCE_MS,
      pollIntervalMs: POLL_INTERVAL_MS,
      retryBaseMs: RETRY_BASE_MS,
      retryMaxMs: 60_000,
      watchDirectory: watch.factory,
      statFile,
    },
  });
  managers.push(manager);
  return { manager, logs, statCalls, embeddingCalls };
}

/** 与路由 POST sources(file) 相同的调用序列：导入 → 解析 → 入队。 */
async function importFileSource(
  manager: KnowledgeManager,
  studioId: string,
  notebookId: string,
  filePath: string,
) {
  const imported = await manager.importFile({ studioId, notebookId, filePath });
  const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
  const job = manager.enqueueSourceIngestion({
    studioId,
    notebookId,
    sourceId: imported.source.id,
    artifactId: artifact.id,
  });
  return { imported, artifact, job };
}

function snapshotCount(manager: KnowledgeManager, studioId: string, sourceId: string) {
  return manager.store.countContentSnapshots({ studioId, sourceId });
}

function sourceJobs(manager: KnowledgeManager, studioId: string, notebookId: string, sourceId: string) {
  return manager.store.listIngestionJobs({ studioId, notebookId, sourceId });
}

function watchState(manager: KnowledgeManager, sourceId: string) {
  return manager.listSourceFileWatchStates().find((state) => state.sourceId === sourceId);
}

/** fake timers 下让真实 fs I/O 跑完：每次 setImmediate 让出处理一批 I/O 回调与微任务。 */
async function settleIo(rounds = 20) {
  for (let i = 0; i < rounds; i++) {
    await new Promise((resolve) => setImmediate(resolve));
  }
}

async function waitFor(condition: () => boolean, label: string, debug?: () => unknown) {
  // refresh 链（stat→读源→写快照→fsync→rename→解析→写产物）全是真实 fs I/O，
  // 每个操作都要若干事件循环轮次；轮次给足避免慢机器/CI 上抖动。
  for (let i = 0; i < 3000; i++) {
    if (condition()) return;
    await new Promise((resolve) => setImmediate(resolve));
  }
  const detail = debug ? `\n${JSON.stringify(debug(), null, 2)}` : "";
  throw new Error(`waitFor timeout: ${label}${detail}`);
}

describe("Knowledge 源文件 watch", () => {
  it("目录事件按文件名过滤并 1500ms 防抖：连续多次保存只刷新一次", async () => {
    const home = tempDir("lingxi-watch-home-");
    const filesDir = tempDir("lingxi-watch-files-");
    const watch = createWatchHarness();
    const { manager, logs } = createManager(home, watch);
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const filePath = path.join(filesDir, "笔记.md");
    fs.writeFileSync(filePath, "# 第一版\n\n苹果项目九月交付。\n");
    const { imported } = await importFileSource(manager, studioId, notebook.id, filePath);
    manager.startSourceFileWatcher();
    expect(await manager.ingestion.drainQueue()).toBe(1);
    expect(snapshotCount(manager, studioId, imported.source.id)).toBe(1);
    // watch 的是父目录而不是文件本身（atomic-rename 保存会废文件级 watcher）。
    expect(watch.registrations).toHaveLength(1);
    expect(watch.registrations[0].dir).toBe(filesDir);

    // 目录内其他文件的事件被过滤，不触发任何刷新。
    watch.registrations[0].onEvent("change", "无关文件.md");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS * 2);
    await settleIo();
    expect(snapshotCount(manager, studioId, imported.source.id)).toBe(1);

    // 连续三次保存（内容均不同）落在同一防抖窗口：合并为一次 refresh，只多一个快照。
    fs.writeFileSync(filePath, "# 第二版\n\n苹果项目十月交付。\n");
    watch.registrations[0].onEvent("rename", "笔记.md");
    fs.writeFileSync(filePath, "# 第三版\n\n苹果项目十一月交付。\n");
    watch.registrations[0].onEvent("change", "笔记.md");
    fs.writeFileSync(filePath, "# 第四版\n\n苹果项目十二月交付。\n");
    watch.registrations[0].onEvent("change", "笔记.md");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS - 1);
    await settleIo();
    expect(snapshotCount(manager, studioId, imported.source.id)).toBe(1);
    await vi.advanceTimersByTimeAsync(1);
    await waitFor(
      () => snapshotCount(manager, studioId, imported.source.id) === 2
        && sourceJobs(manager, studioId, notebook.id, imported.source.id).length === 2,
      "debounced refresh produces exactly one new snapshot and job",
      () => ({
        logs,
        states: manager.listSourceFileWatchStates(),
        snapshots: snapshotCount(manager, studioId, imported.source.id),
        jobs: sourceJobs(manager, studioId, notebook.id, imported.source.id),
      }),
    );
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS * 2);
    await settleIo();
    expect(snapshotCount(manager, studioId, imported.source.id)).toBe(2);
    // 旧 job 已 done，refresh 入队的是新 job（快照内容取最后一次保存）。
    const jobs = sourceJobs(manager, studioId, notebook.id, imported.source.id);
    expect(jobs).toHaveLength(2);
    const latestSnapshot = manager.store.getLatestContentSnapshotForSource({
      studioId,
      sourceId: imported.source.id,
    });
    const refreshed = await manager.refreshFileSource({
      studioId,
      notebookId: notebook.id,
      sourceId: imported.source.id,
    });
    expect(refreshed.changed).toBe(false); // watcher 已刷新到最新内容，再刷去重
    expect(latestSnapshot.byteSize).toBe(Buffer.byteLength("# 第四版\n\n苹果项目十二月交付。\n", "utf8"));
  });

  it("内容未变（sha256 相同）时不产新快照也不入队", async () => {
    const home = tempDir("lingxi-watch-home-");
    const filesDir = tempDir("lingxi-watch-files-");
    const watch = createWatchHarness();
    const { manager, statCalls } = createManager(home, watch);
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const filePath = path.join(filesDir, "不变.txt");
    fs.writeFileSync(filePath, "恒定内容第一行。\n恒定内容第二行。\n");
    const { imported } = await importFileSource(manager, studioId, notebook.id, filePath);
    manager.startSourceFileWatcher();
    expect(await manager.ingestion.drainQueue()).toBe(1);
    await settleIo();
    const statsBefore = statCalls.length;

    // 重新写入相同内容（mtime 变化、sha 不变）并触发事件。
    fs.writeFileSync(filePath, "恒定内容第一行。\n恒定内容第二行。\n");
    watch.registrations[0].onEvent("change", "不变.txt");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    await waitFor(() => statCalls.length > statsBefore, "watcher stat ran for the event");
    await settleIo();

    expect(snapshotCount(manager, studioId, imported.source.id)).toBe(1);
    expect(sourceJobs(manager, studioId, notebook.id, imported.source.id)).toHaveLength(1);
  });

  it("文件内容变化 → 新快照 → 新摄入 job 入队（异步跑 chunk/embed）", async () => {
    const home = tempDir("lingxi-watch-home-");
    const filesDir = tempDir("lingxi-watch-files-");
    const watch = createWatchHarness();
    const { manager } = createManager(home, watch);
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const filePath = path.join(filesDir, "演进.txt");
    fs.writeFileSync(filePath, "火星项目预算八百万。\n");
    const { imported, job: importJob } = await importFileSource(manager, studioId, notebook.id, filePath);
    manager.startSourceFileWatcher();
    expect(await manager.ingestion.drainQueue()).toBe(1);
    expect(manager.store.getIngestionJob({ studioId, jobId: importJob.id }).status).toBe("done");

    fs.writeFileSync(filePath, "火星项目预算调整为九百五十万。\n");
    watch.registrations[0].onEvent("change", "演进.txt");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    await waitFor(
      () => sourceJobs(manager, studioId, notebook.id, imported.source.id).length === 2,
      "watch refresh enqueues a new ingestion job",
    );
    expect(snapshotCount(manager, studioId, imported.source.id)).toBe(2);

    const jobs = sourceJobs(manager, studioId, notebook.id, imported.source.id);
    expect(jobs).toHaveLength(2);
    const watchJob = jobs.find((job) => job.id !== importJob.id)!;
    expect(watchJob.status).toBe("queued");
    expect(watchJob.phase).toBe("parse");
    // refresh 内 parse 成功，job 绑定新解析产物（与导入时的产物不同）。
    expect(watchJob.artifactId).toBeTruthy();
    expect(watchJob.artifactId).not.toBe(importJob.artifactId);
    // 入队的 job 可被队列消费到 done（端到端跑通摄入链）。
    expect(await manager.ingestion.drainQueue()).toBe(1);
    expect(manager.store.getIngestionJob({ studioId, jobId: watchJob.id }).status).toBe("done");
  });

  it("watcher error（如目录被删）指数退避重挂，恢复后事件继续生效", async () => {
    const home = tempDir("lingxi-watch-home-");
    const filesDir = tempDir("lingxi-watch-files-");
    const watch = createWatchHarness();
    const { manager, logs } = createManager(home, watch);
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const filePath = path.join(filesDir, "观察.txt");
    fs.writeFileSync(filePath, "初始内容。\n");
    const { imported } = await importFileSource(manager, studioId, notebook.id, filePath);
    manager.startSourceFileWatcher();
    expect(await manager.ingestion.drainQueue()).toBe(1);
    expect(watchState(manager, imported.source.id)?.watching).toBe(true);

    // 目录开始失败（模拟目录删除后 fs.watch 报错/重挂 ENOENT）。
    watch.failDirs.add(filesDir);
    watch.registrations[0].onError(new Error("simulated watcher failure"));
    expect(watch.registrations[0].closed).toBe(true);
    expect(logs.some((message) => message.includes("watcher error"))).toBe(true);

    // 退避序列 1s → 2s → 4s：未到期不重挂，到期立即重挂（仍失败则继续退避）。
    await vi.advanceTimersByTimeAsync(RETRY_BASE_MS - 1);
    expect(watch.registrations).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    await settleIo();
    expect(watch.registrations).toHaveLength(2);
    expect(watchState(manager, imported.source.id)?.watching).toBe(false);

    await vi.advanceTimersByTimeAsync(RETRY_BASE_MS * 2 - 1);
    expect(watch.registrations).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(1);
    await settleIo();
    expect(watch.registrations).toHaveLength(3);

    // 目录恢复后，下一次退避到期重挂成功，事件重新生效。
    watch.failDirs.delete(filesDir);
    await vi.advanceTimersByTimeAsync(RETRY_BASE_MS * 4);
    await settleIo();
    expect(watch.registrations).toHaveLength(4);
    expect(watch.registrations[3].closed).toBe(false);
    await waitFor(
      () => watchState(manager, imported.source.id)?.watching === true,
      "watcher re-attached",
    );

    fs.writeFileSync(filePath, "恢复后的新内容。\n");
    watch.registrations[3].onEvent("change", "观察.txt");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    await waitFor(
      () => sourceJobs(manager, studioId, notebook.id, imported.source.id).length === 2,
      "refresh after re-attach",
    );
    expect(snapshotCount(manager, studioId, imported.source.id)).toBe(2);
  });

  it("fs.watch 丢事件时由 5 分钟 mtime/size 兜底轮询检出变化", async () => {
    const home = tempDir("lingxi-watch-home-");
    const filesDir = tempDir("lingxi-watch-files-");
    const watch = createWatchHarness();
    const { manager } = createManager(home, watch);
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const filePath = path.join(filesDir, "轮询.md");
    fs.writeFileSync(filePath, "# 轮询前\n\n内容 A。\n");
    const { imported } = await importFileSource(manager, studioId, notebook.id, filePath);
    manager.startSourceFileWatcher();
    expect(await manager.ingestion.drainQueue()).toBe(1);
    await settleIo();

    // 不投递任何 watch 事件，直接改文件：轮询到期前不刷新。
    fs.writeFileSync(filePath, "# 轮询后\n\n内容 A 已扩展为更长的版本。\n");
    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS - 1);
    await settleIo();
    expect(snapshotCount(manager, studioId, imported.source.id)).toBe(1);

    // 轮询检出 mtime/size 变化 → 走同一防抖窗口 → refresh。
    await vi.advanceTimersByTimeAsync(1);
    await settleIo(); // 轮询 stat 是真实 I/O：先让它完成并建防抖计时器
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    await waitFor(
      () => sourceJobs(manager, studioId, notebook.id, imported.source.id).length === 2,
      "fallback poll detects the change",
    );
    expect(snapshotCount(manager, studioId, imported.source.id)).toBe(2);
  });

  it("源文件消失标 unreachable（源文件不可达，不抛错不产失败 job），恢复后自动清除并刷新", async () => {
    const home = tempDir("lingxi-watch-home-");
    const filesDir = tempDir("lingxi-watch-files-");
    const watch = createWatchHarness();
    const { manager, logs } = createManager(home, watch);
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const filePath = path.join(filesDir, "会消失.txt");
    fs.writeFileSync(filePath, "消失前的内容。\n");
    const { imported } = await importFileSource(manager, studioId, notebook.id, filePath);
    manager.startSourceFileWatcher();
    expect(await manager.ingestion.drainQueue()).toBe(1);

    // 文件被外部删除：refresh 路径标 unreachable，不抛错、不动快照与 job。
    fs.rmSync(filePath);
    watch.registrations[0].onEvent("rename", "会消失.txt");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    await waitFor(
      () => watchState(manager, imported.source.id)?.unreachable === true,
      "source marked unreachable",
    );
    const gone = watchState(manager, imported.source.id)!;
    expect(gone.unreachableReason).toContain("ENOENT");
    expect(logs.some((message) => message.includes("unreachable"))).toBe(true);
    expect(snapshotCount(manager, studioId, imported.source.id)).toBe(1);
    expect(sourceJobs(manager, studioId, notebook.id, imported.source.id)).toHaveLength(1);

    // 文件恢复（新内容）：兜底轮询检出 → 清除 unreachable → 自动刷新入队。
    fs.writeFileSync(filePath, "恢复后的内容，比之前更长一些。\n");
    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS);
    await settleIo(); // 轮询 stat 是真实 I/O：先让它完成并建防抖计时器
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    await waitFor(
      () => sourceJobs(manager, studioId, notebook.id, imported.source.id).length === 2,
      "refresh after file restored",
    );
    expect(snapshotCount(manager, studioId, imported.source.id)).toBe(2);
    expect(watchState(manager, imported.source.id)?.unreachable).toBe(false);
    expect(logs.some((message) => message.includes("reachable again"))).toBe(true);
    expect(sourceJobs(manager, studioId, notebook.id, imported.source.id)).toHaveLength(2);
  });

  it("一源多笔记本：变化各自入队；移出/删除笔记本动态摘除 watch", async () => {
    const home = tempDir("lingxi-watch-home-");
    const filesDir = tempDir("lingxi-watch-files-");
    const watch = createWatchHarness();
    const { manager } = createManager(home, watch);
    const studioId = "studio-a";
    const notebookA = manager.createNotebook({ studioId, name: "甲" });
    manager.updateNotebookSettings({ studioId, notebookId: notebookA.id, embeddingModelRef: FAKE_MODEL_REF });
    const notebookB = manager.createNotebook({ studioId, name: "乙" });
    manager.updateNotebookSettings({ studioId, notebookId: notebookB.id, embeddingModelRef: FAKE_MODEL_REF });
    const filePath = path.join(filesDir, "共享.txt");
    fs.writeFileSync(filePath, "共享源初始内容。\n");
    const { imported } = await importFileSource(manager, studioId, notebookA.id, filePath);
    // 非 file 源不进入 watch 范围。
    await manager.importPastedText({ studioId, notebookId: notebookB.id, text: "粘贴文本" });
    manager.startSourceFileWatcher();
    expect(watch.registrations).toHaveLength(1);

    manager.addSourceToNotebook({ studioId, notebookId: notebookB.id, sourceId: imported.source.id });
    expect(watchState(manager, imported.source.id)?.notebooks.sort())
      .toEqual([notebookA.id, notebookB.id].sort());
    expect(await manager.ingestion.drainQueue()).toBe(1);

    // 文件变化：refresh 覆盖一个笔记本，其余笔记本各自入队。
    fs.writeFileSync(filePath, "共享源变更后的内容。\n");
    watch.registrations[0].onEvent("change", "共享.txt");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    await waitFor(
      () => sourceJobs(manager, studioId, notebookA.id, imported.source.id).length === 2
        && sourceJobs(manager, studioId, notebookB.id, imported.source.id).length === 1,
      "shared source refreshed for both notebooks",
    );
    expect(snapshotCount(manager, studioId, imported.source.id)).toBe(2);

    // 移出一个笔记本：watch 项保留（还有 membership）。
    manager.removeSourceFromNotebook({ studioId, notebookId: notebookA.id, sourceId: imported.source.id });
    expect(watchState(manager, imported.source.id)?.notebooks).toEqual([notebookB.id]);
    expect(watch.registrations[0].closed).toBe(false);

    // 移出最后一个笔记本：watch 项整个摘除并关闭目录 watcher。
    manager.removeSourceFromNotebook({ studioId, notebookId: notebookB.id, sourceId: imported.source.id });
    expect(watchState(manager, imported.source.id)).toBeUndefined();
    expect(watch.registrations[0].closed).toBe(true);

    // 笔记本删除同样摘 watch。
    const notebookC = manager.createNotebook({ studioId, name: "丙" });
    manager.updateNotebookSettings({ studioId, notebookId: notebookC.id, embeddingModelRef: FAKE_MODEL_REF });
    const second = await importFileSource(manager, studioId, notebookC.id, filePath);
    expect(watchState(manager, second.imported.source.id)?.watching).toBe(true);
    manager.deleteNotebook({ studioId, notebookId: notebookC.id });
    expect(watchState(manager, second.imported.source.id)).toBeUndefined();
  });
});

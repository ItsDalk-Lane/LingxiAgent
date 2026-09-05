import { afterEach, expect, it, vi } from "vitest";
import { KnowledgeEmbeddingProviderGate } from "../lib/knowledge/ingestion-service.ts";

afterEach(() => { vi.useRealTimers(); });

it("主线程延迟兑现 Promise 时，间隔仍从真正发出请求计算", async () => {
  vi.useFakeTimers(); vi.setSystemTime(1_000);
  const gate = new KnowledgeEmbeddingProviderGate({ maxConcurrent: 2, minRequestIntervalMs: 80 });
  const dispatches: number[] = [];
  const task = async () => { dispatches.push(Date.now()); };
  try {
    const first = gate.run("provider/model", task);
    // 同步经过一个窗口但不让 Promise 续段执行，复现繁忙运行机上的提前占位。
    vi.advanceTimersByTime(80);
    const second = gate.run("provider/model", task);
    await Promise.all([first, second]);
    expect(dispatches).toHaveLength(2);
    expect(dispatches[1] - dispatches[0]).toBeGreaterThanOrEqual(80);
  } finally { gate.dispose(); }
});

it("并发上限和停机拒绝仍生效，已发出的请求可以完成", async () => {
  const gate = new KnowledgeEmbeddingProviderGate({ maxConcurrent: 1, minRequestIntervalMs: 0 });
  let finish!: () => void;
  const first = gate.run("provider/model", () => new Promise<void>(resolve => { finish = resolve; }));
  await Promise.resolve();
  const queuedTask = vi.fn(async () => {});
  const second = gate.run("provider/model", queuedTask);
  const rejected = expect(second).rejects.toMatchObject({ code: "KNOWLEDGE_RETRIEVAL_UNAVAILABLE" });
  expect(gate.stats()).toEqual([{ key: "provider/model", active: 1, queued: 1 }]);
  gate.dispose(); finish();
  await rejected; await first;
  expect(queuedTask).not.toHaveBeenCalled();
  expect(gate.stats()[0].active).toBe(0);
});

import fs from "node:fs";
import path from "node:path";
import { expect, it, vi } from "vitest";
import { annFixture } from "./helpers/knowledge-ann-fixture.ts";

it("portable ready 后异步构建，替换数据使旧内存索引失效，删除变体清掉加速文件", async () => {
  const f = annFixture();
  try {
    const backend = f.start(); await backend.whenIdle();
    const id = f.add(); expect(f.store.get(id)).toBeNull();
    await backend.whenIdle(); expect(f.store.get(id)?.status).toBe("ready");
    const search = () => backend.search({ vectorIndexVariantIds: [id], model: f.model, queryVector: [1, 0, 0], limit: 1 });
    expect((await search())[0].chunkId).toBe("a-0");
    f.add("a", [[0, 1, 0], [1, 0, 0]], "replacement");
    expect(backend.cacheStats.indexes).toBe(0); await backend.whenIdle();
    expect((await search())[0].chunkId).toBe("a-1");
    const row = f.store.get(id)!; expect(row.chunkFingerprint).toBe("replacement"); expect(row.vectorCount).toBe(2);
    f.portable.removeVariant(id); expect(backend.cacheStats.indexes).toBe(0);
    expect(fs.existsSync(path.join(f.root, row.fileName))).toBe(false);
  } finally { await f.close(); }
});

it("构建分批且在独立线程进行，关闭等待线程退出并保留 paid vectors", async () => {
  const f = annFixture();
  try {
    const id = f.add("batch", Array.from({ length: 1300 }, (_, i) => [1, i / 1300, 0]));
    const read = vi.spyOn(f.portable, "readReadyVectorBatch"); const backend = f.start();
    await backend.whenIdle(); expect(read).toHaveBeenCalledTimes(4);
    const before = f.blobs(); backend.invalidate(id); backend.scheduleBuild(id);
    await backend.close(); expect(f.blobs()).toEqual(before);
    expect(fs.existsSync(path.join(f.root, f.model.key.slice(0, 16), `${id}.usearch.tmp`))).toBe(false);
  } finally { await f.close(); }
});

it("立即关闭同步释放目录库，不发起恢复读取；重复关闭等待同一次退出", async () => {
  const f = annFixture();
  const readDirectory = vi.spyOn(fs.promises, "readdir");
  const closeStore = vi.spyOn(f.store, "close");
  try {
    const backend = f.start();
    const closing = backend.close();
    expect(closeStore).toHaveBeenCalledTimes(1);
    expect(backend.close()).toBe(closing);
    await closing;
    expect(readDirectory).not.toHaveBeenCalled();
    expect(closeStore).toHaveBeenCalledTimes(1);
  } finally { vi.restoreAllMocks(); await f.close(); }
});

it("在原生构建线程运行时关闭，等待退出后才能删除目录", async () => {
  const f = annFixture();
  try {
    f.add("closing", Array.from({ length: 1300 }, (_, index) => [1, index / 1300, 0]));
    const original = f.portable.readReadyVectorBatch.bind(f.portable);
    let began!: () => void;
    const building = new Promise<void>(resolve => { began = resolve; });
    vi.spyOn(f.portable, "readReadyVectorBatch").mockImplementation((...args) => {
      const batch = original(...args); began(); return batch;
    });
    const backend = f.start();
    await building;
    const before = f.blobs();
    const closing = backend.close();
    expect(backend.close()).toBe(closing);
    await closing;
    expect(f.blobs()).toEqual(before);
    expect(fs.readdirSync(f.root, { recursive: true }).filter(file => String(file).endsWith(".tmp"))).toEqual([]);
    fs.rmSync(f.root, { recursive: true });
  } finally { vi.restoreAllMocks(); await f.close(); }
});

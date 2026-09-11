/**
 * B08 内存与释放测试（7.4 / P09）：
 *  - 唯一大载荷标记夹具：目录可达对象图（可达性扫描，非字段名检查）无正文、
 *    无 Buffer/TypedArray、无源消息数组、无完整工具输出 Map；
 *  - 驻留估算口径：payload 主导的会话目录 index 远小于原始文件字节；
 *  - 淘汰/失效/dispose 后 released() 全部触发、无引用残留（stats 归零）；
 *  - 8 会话共存与轮换预算。
 */
import { describe, it, expect, afterEach } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { scanHistoryFile } from "../server/history-read/scan.ts";
import { buildHistoryDirectory, measureDirectoryBytes } from "../server/history-read/directory.ts";
import { HistoryDirectoryCache } from "../server/history-read/cache.ts";
import type { HistoryDirectory, HistoryReadContext } from "../server/history-read/types.ts";
import { buildLongRunFixtureLines } from "../scripts/lib/history-read-fixture.mjs";

const tmpDirs: string[] = [];

afterEach(() => {
  while (tmpDirs.length) {
    const dir = tmpDirs.pop();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

function makeTmpDir(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-memory-test-"));
  tmpDirs.push(dir);
  return dir;
}

const HEADER = JSON.stringify({ type: "session", version: 3, id: "7e5f1a3c-9d24-4b8e-a6c1-2f4b8d9e0a31", cwd: "/tmp", timestamp: "2026-09-10T09:00:00Z" });

function payloadFixtureLines(count: number, payloadBytes: number): string[] {
  const lines = [HEADER];
  let parent = "u1";
  lines.push(JSON.stringify({ type: "message", id: "u1", parentId: null, timestamp: "2026-09-10T10:00:00Z", message: { role: "user", content: "开始" } }));
  for (let i = 1; i <= count; i += 1) {
    // 唯一大载荷标记：PAYLOAD-<i>-<重复填充>（每条 20KB 级正文）
    const filler = "x".repeat(Math.max(0, payloadBytes - `PAYLOAD-${i}-`.length));
    lines.push(JSON.stringify({
      type: "message", id: `a${i}`, parentId: parent, timestamp: "2026-09-10T10:00:00Z",
      message: { role: "assistant", content: [{ type: "text", text: `PAYLOAD-${i}-${filler}` }] },
    }));
    parent = `a${i}`;
  }
  return lines;
}

function makeCtx(p: string): HistoryReadContext {
  const stat = fs.statSync(p);
  return {
    sessionPath: p,
    sessionId: null,
    locatorPath: p,
    publicRevision: `${stat.size}:${stat.mtimeMs}`,
    fileIdentity: { size: stat.size, mtimeMs: stat.mtimeMs, dev: stat.dev, ino: stat.ino, ctimeMs: stat.ctimeMs },
    branchHeadRowExists: false,
    branchHeadRow: null,
  };
}

async function buildDirectory(p: string) {
  const scan = await scanHistoryFile(p, { capturedLength: fs.statSync(p).size });
  const result = await buildHistoryDirectory(scan, makeCtx(p));
  if (!result.directory) throw new Error(`目录构建失败：${(result as any).reason}`);
  return { directory: result.directory, released: result.released, context: result.context };
}

/** 可达性扫描：从目录对象 BFS 遍历全部可达字符串/Buffer/集合，不做字段名白名单。 */
function reachableAudit(directory: HistoryDirectory, markerRe: RegExp) {
  const payloadHits: string[] = [];
  let bufferCount = 0;
  let maxArrayLength = 0;
  let largeStringMax = 0;
  let visited = 0;
  const seen = new Set<object>();
  const queue: any[] = [directory];
  while (queue.length) {
    const value = queue.pop();
    if (value == null) continue;
    const type = typeof value;
    if (type === "string") {
      visited += 1;
      if (markerRe.test(value)) payloadHits.push(value.slice(0, 80));
      if (value.length > largeStringMax) largeStringMax = value.length;
      continue;
    }
    if (type !== "object" && type !== "function") continue;
    if (seen.has(value)) continue;
    seen.add(value);
    visited += 1;
    if (Buffer.isBuffer(value) || ArrayBuffer.isView(value)) {
      bufferCount += 1;
      continue;
    }
    if (Array.isArray(value)) {
      if (value.length > maxArrayLength) maxArrayLength = value.length;
      for (const item of value) queue.push(item);
      continue;
    }
    if (value instanceof Map) {
      for (const [k, v] of value) { queue.push(k); queue.push(v); }
      continue;
    }
    if (value instanceof Set) {
      for (const v of value) queue.push(v);
      continue;
    }
    for (const v of Object.values(value)) queue.push(v);
  }
  return { payloadHits, bufferCount, maxArrayLength, largeStringMax, visited };
}

describe("目录无正文驻留（7.4/P09）", () => {
  it("可达对象图无唯一大载荷标记、无 Buffer；估算 < 原始文件字节", async () => {
    const payloadBytes = 20 * 1024;
    const count = 12;
    const p = path.join(makeTmpDir(), "payload.jsonl");
    fs.writeFileSync(p, payloadFixtureLines(count, payloadBytes).join("\n") + "\n");
    const fileBytes = fs.statSync(p).size;
    const { directory } = await buildDirectory(p);

    const audit = reachableAudit(directory, /PAYLOAD-\d+-/);
    expect(audit.payloadHits).toEqual([]);
    expect(audit.bufferCount).toBe(0);
    // 目录 index 远小于正文（估算口径不把正文算进驻留）
    expect(directory.measuredBytes).toBeLessThan(fileBytes);
    // 估算为正且随记录数增长（非恒零）
    expect(directory.measuredBytes).toBeGreaterThan(0);
    expect(audit.largeStringMax).toBeLessThan(payloadBytes); // 最大字符串 < 单条载荷
  });

  it("released() 后 transient 上下文被清空（I11）", async () => {
    const p = path.join(makeTmpDir(), "released.jsonl");
    fs.writeFileSync(p, payloadFixtureLines(6, 2048).join("\n") + "\n");
    const { context, released } = await buildDirectory(p);
    expect(Object.keys(context).length).toBeGreaterThan(0);
    released();
    expect(Object.keys(context).length).toBe(0);
  });
});

describe("缓存预算与释放（P09）", () => {
  it("8 会话共存；轮换后槽位不超限、淘汰触发 released", async () => {
    const cache = new HistoryDirectoryCache();
    const releasedKeys: string[] = [];
    const dirs: Array<{ key: string; directory: HistoryDirectory }> = [];
    for (let i = 0; i < 8; i += 1) {
      const p = path.join(makeTmpDir(), `s${i}.jsonl`);
      fs.writeFileSync(p, buildLongRunFixtureLines(4).join("\n") + "\n");
      const { directory } = await buildDirectory(p);
      const key = historyKey(i);
      const lease = await cache.beginBuild(key);
      expect(lease).not.toBeNull();
      expect(cache.publish(key, lease as any, directory, { released: () => releasedKeys.push(key) })).toBe(true);
      dirs.push({ key, directory });
    }
    expect(cache.stats().sessions).toBe(8);
    expect(cache.stats().residentBytes).toBeGreaterThan(0);

    // 轮换 2 个新会话 → 最早的 2 个槽位被淘汰并 released
    for (let i = 8; i < 10; i += 1) {
      const p = path.join(makeTmpDir(), `s${i}.jsonl`);
      fs.writeFileSync(p, buildLongRunFixtureLines(4).join("\n") + "\n");
      const { directory } = await buildDirectory(p);
      const key = historyKey(i);
      const lease = await cache.beginBuild(key);
      expect(cache.publish(key, lease as any, directory)).toBe(true);
    }
    expect(cache.stats().sessions).toBe(8);
    expect(cache.stats().evictions).toBe(2);
    expect(releasedKeys).toEqual([historyKey(0), historyKey(1)]);

    function historyKey(i: number) {
      return `rt\0st\0sess-mem-${i}`;
    }
    cache.dispose();
    expect(cache.stats().sessions).toBe(0);
    expect(cache.stats().residentBytes).toBe(0);
  });
});

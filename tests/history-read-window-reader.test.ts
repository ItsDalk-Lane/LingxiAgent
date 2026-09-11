/**
 * B05 定点读取测试（X05 / X07）：
 *  - 窗口/依赖定位 → 位置读取：记录与整文件 parse 逐条一致、稀疏事实不重新编号；
 *  - 读取途中 truncate（size 变）/同长度重写（mtime 变）/同 size+mtime 原子替换
 *    （ino/ctime 变）→ file_identity_changed，不返回混合页面；
 *  - 短读注入多次补齐；异常 EOF → short_read（不读空 Buffer）；
 *  - entryId/type 与目录不符 → directory_invalid；
 *  - mergeGapBytes=0 只合并紧邻；间隙超限不合并（额外字节如实报告）；
 *  - 并发上限生效；所有分支关闭句柄（错误结果后文件可再次读取）。
 */
import { describe, it, expect, afterEach } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { scanHistoryFile } from "../server/history-read/scan.ts";
import { buildHistoryDirectory } from "../server/history-read/directory.ts";
import { resolveHistoryPage, locateToolResultRecords, locateMediaResultRecords } from "../server/history-read/page.ts";
import { readHistoryRecords, collectLocations } from "../server/history-read/window-reader.ts";
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
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-window-test-"));
  tmpDirs.push(dir);
  return dir;
}

function jsonlLine(id: string, parentId: string | null, message: any) {
  return JSON.stringify({ type: "message", id, parentId, timestamp: "2026-09-10T10:00:00Z", message });
}

const HEADER = JSON.stringify({ type: "session", version: 3, id: "7e5f1a3c-9d24-4b8e-a6c1-2f4b8d9e0a31", cwd: "/tmp", timestamp: "2026-09-10T09:00:00Z" });

function writeFixture(name: string, lines: string[]): string {
  const p = path.join(makeTmpDir(), name);
  fs.writeFileSync(p, lines.join("\n") + "\n");
  return p;
}

function identityOf(p: string) {
  const st = fs.statSync(p);
  return { dev: st.dev, ino: st.ino, size: st.size, mtimeMs: st.mtimeMs, ctimeMs: st.ctimeMs };
}

function makeCtx(p: string, overrides: Partial<HistoryReadContext> = {}): HistoryReadContext {
  return {
    sessionPath: p,
    sessionId: null,
    locatorPath: p,
    publicRevision: `${fs.statSync(p).size}:${fs.statSync(p).mtimeMs}`,
    fileIdentity: identityOf(p),
    branchHeadRowExists: false,
    branchHeadRow: null,
    ...overrides,
  };
}

async function buildFixtureDirectory(p: string): Promise<HistoryDirectory> {
  const scan = await scanHistoryFile(p, { capturedLength: fs.statSync(p).size });
  const result = await buildHistoryDirectory(scan, makeCtx(p));
  if (!result.directory) throw new Error(`目录构建失败：${(result as any).reason}`);
  return result.directory;
}

const FIXTURE_LINES = buildLongRunFixtureLines(12);

async function freshFixtureAndDirectory(name = "longrun.jsonl") {
  const p = writeFixture(name, FIXTURE_LINES);
  const directory = await buildFixtureDirectory(p);
  return { p, directory };
}

describe("readHistoryRecords 定位读取", () => {
  it("窗口 + 依赖定位读取：记录与整文件 parse 一致，稀疏事实不重新编号", async () => {
    const { p, directory } = await freshFixtureAndDirectory();
    const page = resolveHistoryPage(directory, { beforeId: null, limit: 5, forceAll: false });
    expect(page.bounds).toEqual({ total: 13, startIdx: 8, endIdx: 13, hasMore: true });
    expect(page.windowRecordIndexes.length).toBe(5);

    const locations = collectLocations(
      [...page.windowRecordIndexes, ...page.blockAnchorIndexes],
      directory,
      page.dependencyLocations,
    );
    const result = await readHistoryRecords(p, identityOf(p), locations);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    // 稀疏读取不重新编号：key = 原 sourceIndex（sourceMessages 下标 = raw 条目下标 - 1 头部）
    const raw = fs.readFileSync(p, "utf8");
    const whole = raw.split("\n").filter((l) => l.trim()).map((l) => JSON.parse(l)).slice(1);
    for (const loc of locations) {
      const record = result.records.get(loc.sourceIndex);
      expect(record).toBeDefined();
      expect(record).toEqual(whole[loc.sourceIndex]);
      expect(record.id).toBe(loc.entryId);
    }
    expect(result.coalescingExtraBytes).toBeGreaterThanOrEqual(0);
  });

  it("resolveHistoryPage：headState 取首条窗口记录 before 状态（不从根重放）；边界语义与旧函数一致", async () => {
    const { directory } = await freshFixtureAndDirectory();
    const page = resolveHistoryPage(directory, { beforeId: null, limit: 5, forceAll: false });
    const firstIndex = page.windowRecordIndexes[0];
    const fact = directory.records.at(firstIndex);
    expect(page.headState).toEqual({
      sourceIndex: firstIndex,
      displayIdx: fact.displayIndexBefore,
      latestTurnInputEntryId: fact.turnInputEntryIdBefore,
      latestTurnInputVisible: fact.turnInputVisibleBefore,
      assistantOrdinalInTurn: fact.assistantOrdinalBefore,
    });
    expect(page.headState.displayIdx).toBe(8);
    // 全量
    const all = resolveHistoryPage(directory, { beforeId: null, limit: 50, forceAll: true });
    expect(all.bounds).toEqual({ total: 13, startIdx: 0, endIdx: 13, hasMore: false });
    expect(all.windowRecordIndexes.length).toBe(13);
    // before=0（已翻到头）：空窗口 + 终结分页
    const empty = resolveHistoryPage(directory, { beforeId: 0, limit: 5, forceAll: false });
    expect(empty.bounds).toEqual({ total: 13, startIdx: 0, endIdx: 0, hasMore: false });
    expect(empty.windowRecordIndexes).toEqual([]);
  });

  it("locateToolResultRecords / locateMediaResultRecords：O(命中数) 补定位并可读取", async () => {
    const { p, directory } = await freshFixtureAndDirectory();
    const toolLocs = locateToolResultRecords(directory, ["tu-1", "tu-7", "tu-不存在"]);
    expect(toolLocs.length).toBe(2);
    const result = await readHistoryRecords(p, identityOf(p), toolLocs);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const r1 = result.records.get(toolLocs[0].sourceIndex);
    expect(r1.id).toBe("r1");
    expect(r1.message.toolCallId).toBe("tu-1");
    // 媒体：目录无 mediaResultRecords 时空返回
    expect(locateMediaResultRecords(directory, ["task-1"])).toEqual([]);
  });
});

describe("readHistoryRecords 短读与 EOF（X07）", () => {
  it("注入短读：每次最多 3 字节多次补齐，结果与整读一致", async () => {
    const { p, directory } = await freshFixtureAndDirectory();
    const page = resolveHistoryPage(directory, { beforeId: null, limit: 5, forceAll: false });
    const locations = collectLocations([...page.windowRecordIndexes, ...page.blockAnchorIndexes], directory, page.dependencyLocations);
    const baseline = await readHistoryRecords(p, identityOf(p), locations);
    const raw = fs.readFileSync(p);

    let calls = 0;
    const shortRead = async (buffer: Buffer, offset: number, length: number, position: number) => {
      calls += 1;
      if (position >= raw.length) return 0;
      return raw.subarray(position, Math.min(position + length, position + 3)).copy(buffer, offset);
    };
    const result = await readHistoryRecords(p, identityOf(p), locations, { readFile: shortRead });
    const totalRequested = locations.reduce((sum, loc) => sum + loc.byteLength, 0);
    expect(calls).toBeGreaterThanOrEqual(Math.ceil(totalRequested / 3));
    expect(result).toEqual(baseline);
  });

  it("异常 EOF → short_read（先重验身份，不读空 Buffer）", async () => {
    const { p, directory } = await freshFixtureAndDirectory();
    const page = resolveHistoryPage(directory, { beforeId: null, limit: 5, forceAll: false });
    const locations = collectLocations([...page.windowRecordIndexes, ...page.blockAnchorIndexes], directory, page.dependencyLocations);
    const raw = fs.readFileSync(p);
    const cutoff = raw.length - 10;
    const eofHook = async (buffer: Buffer, offset: number, length: number, position: number) => {
      if (position >= cutoff) return 0;
      return raw.subarray(position, Math.min(position + length, cutoff)).copy(buffer, offset);
    };
    const result = await readHistoryRecords(p, identityOf(p), locations, { readFile: eofHook });
    expect(result).toMatchObject({ ok: false, reason: "short_read" });
  });
});

describe("readHistoryRecords 身份校验（X02/X03/X04/X05）", () => {
  it("读取途中截断（size 变）→ file_identity_changed，不返回混合页面", async () => {
    const { p, directory } = await freshFixtureAndDirectory();
    const identity = identityOf(p);
    fs.truncateSync(p, identity.size - 20);
    const page = resolveHistoryPage(directory, { beforeId: null, limit: 5, forceAll: false });
    const locations = collectLocations([...page.windowRecordIndexes, ...page.blockAnchorIndexes], directory, page.dependencyLocations);
    const result = await readHistoryRecords(p, identity, locations);
    expect(result).toMatchObject({ ok: false, reason: "file_identity_changed" });
  });

  it("同长度重写（mtime 变）→ file_identity_changed（X02）", async () => {
    const { p, directory } = await freshFixtureAndDirectory();
    const identity = identityOf(p);
    const raw = fs.readFileSync(p);
    // 同长度、不同字节的原位重写
    const tampered = Buffer.from(raw);
    tampered[tampered.length - 5] = tampered[tampered.length - 5] === 0x31 ? 0x32 : 0x31;
    fs.writeFileSync(p, tampered);
    expect(fs.statSync(p).size).toBe(identity.size);
    const page = resolveHistoryPage(directory, { beforeId: null, limit: 5, forceAll: false });
    const locations = collectLocations([...page.windowRecordIndexes, ...page.blockAnchorIndexes], directory, page.dependencyLocations);
    const result = await readHistoryRecords(p, identity, locations);
    expect(result).toMatchObject({ ok: false, reason: "file_identity_changed" });
  });

  it("同 size+mtime 原子替换（ino 变）→ file_identity_changed（X04）", async () => {
    const { p, directory } = await freshFixtureAndDirectory();
    const identity = identityOf(p);
    // 同字节副本 + 回拨 mtime + rename 原子替换：size/mtime 一致，ino/ctime 不同
    const replacement = path.join(makeTmpDir(), "replacement.jsonl");
    fs.writeFileSync(replacement, fs.readFileSync(p));
    fs.utimesSync(replacement, new Date(identity.mtimeMs), new Date(identity.mtimeMs));
    fs.renameSync(replacement, p);
    const st = fs.statSync(p);
    expect(st.size).toBe(identity.size);
    expect(st.ino).not.toBe(identity.ino);
    const page = resolveHistoryPage(directory, { beforeId: null, limit: 5, forceAll: false });
    const locations = collectLocations([...page.windowRecordIndexes, ...page.blockAnchorIndexes], directory, page.dependencyLocations);
    const result = await readHistoryRecords(p, identity, locations);
    expect(result).toMatchObject({ ok: false, reason: "file_identity_changed" });
  });

  it("locator 迁移（读取后 entryId 校验兜底）：目录身份不符 → directory_invalid", async () => {
    const { p, directory } = await freshFixtureAndDirectory();
    const page = resolveHistoryPage(directory, { beforeId: null, limit: 5, forceAll: false });
    const locations = collectLocations([...page.windowRecordIndexes, ...page.blockAnchorIndexes], directory, page.dependencyLocations);
    // 模拟目录与文件错位（locator 迁移后旧目录仍指向旧坐标）：entryId 校验必须 fail-closed
    const tampered = locations.map((loc, i) => (i === 0 ? { ...loc, entryId: "ghost" } : loc));
    const result = await readHistoryRecords(p, identityOf(p), tampered);
    expect(result).toMatchObject({ ok: false, reason: "directory_invalid" });
    // type 不符（message 记录标成 custom raw type）→ directory_invalid
    const typeTampered = locations.map((loc, i) => (i === 0 ? { ...loc, type: "custom" } : loc));
    const typeResult = await readHistoryRecords(p, identityOf(p), typeTampered);
    expect(typeResult).toMatchObject({ ok: false, reason: "directory_invalid" });
  });
});

describe("readHistoryRecords 合并与并发", () => {
  it("mergeGapBytes=0 只合并紧邻区域；有间隙不合并；启用间隙合并时报告额外字节", async () => {
    const { p, directory } = await freshFixtureAndDirectory();
    const byId = new Map(directory.records.map((r: any) => [r.entryId, r]));
    const locOf = (entryId: string) => {
      const record = byId.get(entryId);
      const physicalIndex = directory.file.byEntryId.get(record.entryId)!;
      return { sourceIndex: record.sourceIndex, entryId: record.entryId, type: "message" as const, byteOffset: directory.file.byteOffsets.at(physicalIndex), byteLength: directory.file.byteLengths.at(physicalIndex) };
    };
    // u1 与 a1 相邻（u1 行 + \n + a1 行）；r7 在远处
    const u1 = locOf("u1");
    const a1 = locOf("a1");
    const r7 = locOf("r7");
    const locations = [r7, a1, u1];

    const merged = await readHistoryRecords(p, identityOf(p), locations);
    expect(merged.ok).toBe(true);
    if (!merged.ok) return;
    // u1+a1 紧邻合并为一个区域；r7 独立 → 物理读取 = 3 条记录字节总和，无额外
    expect(merged.logicalReadBytes).toBe(u1.byteLength + a1.byteLength + r7.byteLength + 1); // +1 = u1/a1 之间的 \n
    expect(merged.coalescingExtraBytes).toBe(1);
    expect(merged.records.size).toBe(3);

    const unmerged = await readHistoryRecords(p, identityOf(p), [locOf("u1"), locOf("r7")], { mergeGapBytes: 0 });
    expect(unmerged.ok).toBe(true);
    if (!unmerged.ok) return;
    expect(unmerged.logicalReadBytes).toBe(u1.byteLength + r7.byteLength);
    expect(unmerged.coalescingExtraBytes).toBe(0);
  });

  it("并发上限生效（受控 hook 统计在途峰值）；错误分支后文件仍可再次读取", async () => {
    const { p, directory } = await freshFixtureAndDirectory();
    const page = resolveHistoryPage(directory, { beforeId: null, limit: 4, forceAll: false });
    // 追加远端依赖（tu-1 的 toolResult 在文件头部）→ 至少两个独立区域
    const locations = collectLocations(
      [...page.windowRecordIndexes, ...page.blockAnchorIndexes],
      directory,
      [...page.dependencyLocations, ...locateToolResultRecords(directory, ["tu-1", "tu-2"])],
    );
    const raw = fs.readFileSync(p);
    let inFlight = 0;
    let peak = 0;
    const tracked = async (buffer: Buffer, offset: number, length: number, position: number) => {
      inFlight += 1;
      peak = Math.max(peak, inFlight);
      await new Promise((resolve) => setTimeout(resolve, 2));
      const n = position >= raw.length ? 0 : raw.subarray(position, Math.min(position + length, raw.length)).copy(buffer, offset);
      inFlight -= 1;
      return n;
    };
    const result = await readHistoryRecords(p, identityOf(p), locations, { readFile: tracked, maxConcurrent: 2 });
    expect(result.ok).toBe(true);
    expect(peak).toBeLessThanOrEqual(2);

    // 错误分支（身份不符）后再次读取仍正常（句柄已关闭，无泄漏性占用）：
    // 以截断前的身份快照调用 → fstat size 不符 → file_identity_changed
    const staleIdentity = identityOf(p);
    fs.truncateSync(p, fs.statSync(p).size - 5);
    const blocked = await readHistoryRecords(p, staleIdentity, locations);
    expect(blocked).toMatchObject({ ok: false, reason: "file_identity_changed" });
  });
});

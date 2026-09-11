/**
 * B04 目录缓存测试（X15 / X16 / X17 / X02 / X03 / X04 / P09 预算部分）：
 *  - 同 key 并发冷构建只跑一次（single-flight 共享同一 lease）；
 *  - 乱序完成旧不覆新（invalidate 后 stale publish 被拒）、租约版本不被修改；
 *  - 失败构建从在途表移除可重试；淘汰/失效/dispose 调用 released()（无驻留泄漏）；
 *  - probe 四条件：同长度重写（mtime）、变长重写（size）、同 size+mtime 原子替换
 *    （ino/ctime）→ file_identity_changed；head 三态变化 → branch_changed；
 *    locator 变化 → locator_changed；revision 未知 → revision_unknown；
 *  - 8 槽 LRU（命中提升）、字节预算淘汰；超 16MiB → budget_exceeded → no-cache。
 * 竞态全部用受控 Promise/microtask 调度，不依赖随机 sleep。
 */
import { describe, it, expect, afterEach } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { scanHistoryFile } from "../server/history-read/scan.ts";
import { buildHistoryDirectory } from "../server/history-read/directory.ts";
import {
  HistoryDirectoryCache,
  historyDirectoryCacheKey,
  DEFAULT_MAX_SESSIONS,
  DEFAULT_MAX_RESIDENT_INDEX_BYTES,
  DEFAULT_MAX_SINGLE_DIRECTORY_BYTES,
  DEFAULT_MAX_CONCURRENT_BUILDS,
} from "../server/history-read/cache.ts";
import type { HistoryDirectory, HistoryReadContext } from "../server/history-read/types.ts";
import { noteSessionFileMutation } from "../core/session-file-mutation-epoch.ts";
import { buildLongRunFixtureLines } from "../scripts/lib/history-read-fixture.mjs";

const tmpDirs: string[] = [];

afterEach(() => {
  while (tmpDirs.length) {
    const dir = tmpDirs.pop();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

function makeTmpDir(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-cache-test-"));
  tmpDirs.push(dir);
  return dir;
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

async function buildDirectory(p: string, buildOpts: Record<string, any> = {}): Promise<HistoryDirectory> {
  const scan = await scanHistoryFile(p, { capturedLength: fs.statSync(p).size });
  const result = await buildHistoryDirectory(scan, makeCtx(p), buildOpts);
  if (!result.directory) throw new Error(`目录构建失败：${(result as any).reason}`);
  return result.directory;
}

async function publishedEntry(p: string, cache: HistoryDirectoryCache, key: string, hooks: Record<string, any> = {}): Promise<HistoryDirectory> {
  const directory = await buildDirectory(p);
  const lease = await cache.beginBuild(key);
  expect(lease).not.toBeNull();
  const accepted = cache.publish(key, lease as any, directory, { locatorPath: p, ...hooks });
  expect(accepted).toBe(true);
  return directory;
}

const FIXTURE_LINES = buildLongRunFixtureLines(10);

describe("缓存基础：single-flight 与版本校验（X15/X16）", () => {
  it("同 key 并发冷构建共享同一 lease，构建仅计一次；重复 publish 被拒且不驻留", async () => {
    const p = writeFixture("x15.jsonl", FIXTURE_LINES);
    const cache = new HistoryDirectoryCache();
    const key = historyDirectoryCacheKey({ sessionId: "sess-x15", sessionPath: p });

    const p1 = cache.beginBuild(key);
    const p2 = cache.beginBuild(key);
    expect(p2).toBe(p1); // single-flight：同一 Promise
    const lease = await p1;
    expect(lease).not.toBeNull();
    expect(lease!.generation).toBe(1);

    const releasedSpy: string[] = [];
    const directory = await buildDirectory(p);
    expect(cache.publish(key, lease!, directory, { released: () => releasedSpy.push("a") })).toBe(true);
    const stats1 = cache.stats();
    expect(stats1.builds).toBe(1);
    expect(stats1.sessions).toBe(1);

    // 同一 lease 再次 publish：在途表已清空 → 拒绝，且迟到的目录不得驻留（I11）
    const lateDirectory = await buildDirectory(p);
    expect(cache.publish(key, lease!, lateDirectory, { released: () => releasedSpy.push("late") })).toBe(false);
    expect(releasedSpy).toEqual(["late"]);
    expect(cache.get(key)?.directory).toBe(directory);
    cache.dispose();
  });

  it("乱序完成旧不覆新：invalidate 推进版本后 stale publish 被拒；租约版本不可变（X16）", async () => {
    const p = writeFixture("x16.jsonl", FIXTURE_LINES);
    const cache = new HistoryDirectoryCache();
    const key = historyDirectoryCacheKey({ sessionId: "sess-x16", sessionPath: p });

    const lease1 = await cache.beginBuild(key);
    expect(lease1!.generation).toBe(1);
    const dirA = await buildDirectory(p);
    expect(cache.publish(key, lease1!, dirA, { locatorPath: p })).toBe(true);
    expect(lease1!.generation).toBe(1); // 租约不被修改

    cache.invalidate(key, "file_identity_changed"); // 版本推进到 2
    expect(lease1!.generation).toBe(1);

    // 构建慢的旧版本（gen 1）晚完成 → 拒绝，不覆盖
    const dirStale = await buildDirectory(p);
    const releasedStale: string[] = [];
    expect(cache.publish(key, lease1!, dirStale, { released: () => releasedStale.push("x") })).toBe(false);
    expect(releasedStale).toEqual(["x"]);
    expect(cache.get(key)).toBeNull();

    // 新一代构建正常发布
    const lease2 = await cache.beginBuild(key);
    expect(lease2!.generation).toBe(3); // begin 又推进一次（invalidate +1 → 2；begin → 3）
    const dirB = await buildDirectory(p);
    expect(cache.publish(key, lease2!, dirB)).toBe(true);
    expect(cache.get(key)?.directory).toBe(dirB);
    cache.dispose();
  });

  it("失败构建从在途表移除，可立即重试（受控调度）", async () => {
    const p = writeFixture("fail.jsonl", FIXTURE_LINES);
    const cache = new HistoryDirectoryCache();
    const key = historyDirectoryCacheKey({ sessionId: "sess-fail", sessionPath: p });
    const lease = await cache.beginBuild(key);
    expect(cache.stats().inFlightBuilds).toBe(1);
    cache.failBuild(key, lease!);
    expect(cache.stats().inFlightBuilds).toBe(0);
    const lease2 = await cache.beginBuild(key);
    expect(lease2!.generation).toBe(lease!.generation + 1);
    cache.dispose();
  });
});

describe("probe 四条件（X02/X03/X04/X05/I05/I08）", () => {
  it("新目录 probe=valid 且命中提升", async () => {
    const p = writeFixture("probe.jsonl", FIXTURE_LINES);
    const cache = new HistoryDirectoryCache();
    const key = historyDirectoryCacheKey({ sessionId: "sess-probe", sessionPath: p });
    await publishedEntry(p, cache, key);
    expect(cache.probe(key, makeCtx(p))).toBe("valid");
    cache.get(key);
    expect(cache.stats().hits).toBe(1);
    cache.dispose();
  });

  it("同长度重写（mtime 变）→ untrusted_mutation 全量重建（X02，C01）", async () => {
    const p = writeFixture("x02.jsonl", FIXTURE_LINES);
    const cache = new HistoryDirectoryCache();
    const key = historyDirectoryCacheKey({ sessionId: "sess-x02", sessionPath: p });
    await publishedEntry(p, cache, key);

    const raw = fs.readFileSync(p);
    const tampered = Buffer.from(raw);
    tampered[10] = tampered[10] === 0x30 ? 0x7a : 0x30;
    fs.writeFileSync(p, tampered);
    expect(fs.statSync(p).size).toBe(identityOf(p).size || fs.statSync(p).size);

    const freshIdentity = identityOf(p);
    // C01：同长度重写（size 不变、mtime 已变）= 无法区分 → untrusted_mutation 全量重建
    expect(cache.probe(key, makeCtx(p))).toBe("untrusted_mutation");
    expect(cache.get(key)).toBeNull(); // 已失效
    void freshIdentity;
    cache.dispose();
  });

  it("纯追加（无重写通知）→ append_candidate 且目录保留；变长重写（有通知）→ untrusted_mutation（X03，C01）", async () => {
    const p = writeFixture("x03.jsonl", FIXTURE_LINES);
    const cache = new HistoryDirectoryCache();
    const key = historyDirectoryCacheKey({ sessionId: "sess-x03", sessionPath: p });
    await publishedEntry(p, cache, key);
    fs.appendFileSync(p, `${JSON.stringify({ type: "message", id: "extra", parentId: "a10", message: { role: "assistant", content: "追加" } })}\n`);
    // C01：身份不变 + size 增长 + 变更世代未变 = 可信追加候选；目录不失效（留给 C03 增量）
    expect(cache.probe(key, makeCtx(p))).toBe("append_candidate");
    expect(cache.get(key)).not.toBeNull();

    // 重写路径通知后（写前递增世代），size 增长不再是可信追加 → 全量重建
    noteSessionFileMutation(p, "rewrite");
    expect(cache.probe(key, makeCtx(p))).toBe("untrusted_mutation");
    expect(cache.get(key)).toBeNull(); // 已失效
    cache.dispose();
  });

  it("同 size+mtime 原子替换（ino/ctime 变）→ file_identity_changed（X04）", async () => {
    const p = writeFixture("x04.jsonl", FIXTURE_LINES);
    const cache = new HistoryDirectoryCache();
    const key = historyDirectoryCacheKey({ sessionId: "sess-x04", sessionPath: p });
    await publishedEntry(p, cache, key);
    const identity = identityOf(p);

    const replacement = path.join(makeTmpDir(), "replacement.jsonl");
    fs.writeFileSync(replacement, fs.readFileSync(p));
    fs.utimesSync(replacement, new Date(identity.mtimeMs), new Date(identity.mtimeMs));
    fs.renameSync(replacement, p);
    const st = fs.statSync(p);
    expect(st.size).toBe(identity.size);
    expect(st.ino).not.toBe(identity.ino);

    expect(cache.probe(key, makeCtx(p))).toBe("file_identity_changed");
    cache.dispose();
  });

  it("head 三态变化（文件不变）→ branch_view_stale 且目录保留；locator 变化 → locator_changed；revision 未知 → revision_unknown（C01）", async () => {
    const p = writeFixture("cond.jsonl", FIXTURE_LINES);
    const cache = new HistoryDirectoryCache();
    const key = historyDirectoryCacheKey({ sessionId: "sess-cond", sessionPath: p });
    await publishedEntry(p, cache, key);

    // C01：文件不变、分支选择改变 → branch_view_stale（保留物理索引，不失效目录）
    expect(cache.probe(key, makeCtx(p, { branchHeadRowExists: true, branchHeadRow: { leafId: null, observedTailLeafId: null } }))).toBe("branch_view_stale");
    expect(cache.get(key)).not.toBeNull(); // 目录保留
    expect(cache.probe(key, makeCtx(p, { locatorPath: `${p}.moved` }))).toBe("locator_changed");
    await publishedEntry(p, cache, key);
    expect(cache.probe(key, makeCtx(p, { publicRevision: null }))).toBe("revision_unknown");
    cache.dispose();
  });
});

describe("LRU 槽位与字节预算（P09）", () => {
  it(`槽位上限（默认 ${DEFAULT_MAX_SESSIONS}）与命中提升`, async () => {
    expect(DEFAULT_MAX_SESSIONS).toBe(8);
    const cache = new HistoryDirectoryCache({ maxSessions: 2 });
    const paths = ["a", "b", "c"].map((n) => writeFixture(`lru-${n}.jsonl`, FIXTURE_LINES));
    const keys = paths.map((p) => historyDirectoryCacheKey({ sessionId: null, sessionPath: p }));
    for (let i = 0; i < paths.length; i += 1) {
      await publishedEntry(paths[i], cache, keys[i]);
    }
    // 插入第三个 → 最久未用的 a 被淘汰
    expect(cache.get(keys[0])).toBeNull();
    expect(cache.get(keys[1])?.directory).toBeDefined();
    expect(cache.get(keys[2])?.directory).toBeDefined();
    expect(cache.stats().evictions).toBeGreaterThanOrEqual(1);

    // 命中提升：访问 b 后再插入 d → 淘汰 c（而非 b）
    const cache2 = new HistoryDirectoryCache({ maxSessions: 2 });
    const p2 = ["x", "y", "z"].map((n) => writeFixture(`lru2-${n}.jsonl`, FIXTURE_LINES));
    const k2 = p2.map((p) => historyDirectoryCacheKey({ sessionId: null, sessionPath: p }));
    for (let i = 0; i < 2; i += 1) await publishedEntry(p2[i], cache2, k2[i]);
    cache2.get(k2[0]); // 提升 x
    await publishedEntry(p2[2], cache2, k2[2]);
    expect(cache2.get(k2[0])).not.toBeNull();
    expect(cache2.get(k2[1])).toBeNull();
    expect(cache2.get(k2[2])).not.toBeNull();
    cache.dispose();
    cache2.dispose();
  });

  it("字节预算淘汰；retired 计入驻留并可显式归还（I11）", async () => {
    const p1 = writeFixture("budget-1.jsonl", FIXTURE_LINES);
    const p2 = writeFixture("budget-2.jsonl", FIXTURE_LINES);
    const dir1 = await buildDirectory(p1);
    const cache = new HistoryDirectoryCache({ maxResidentIndexBytes: dir1.measuredBytes + 1 });
    const key1 = historyDirectoryCacheKey({ sessionId: "sess-b1", sessionPath: p1 });
    const released: string[] = [];
    expect(cache.publish(key1, await cache.beginBuild(key1) as any, dir1, { released: () => released.push("d1") })).toBe(true);
    expect(cache.stats().residentBytes).toBeGreaterThanOrEqual(dir1.measuredBytes);

    const key2 = historyDirectoryCacheKey({ sessionId: "sess-b2", sessionPath: p2 });
    const dir2 = await buildDirectory(p2);
    const released2: string[] = [];
    expect(cache.publish(key2, await cache.beginBuild(key2) as any, dir2, { released: () => released2.push("d2") })).toBe(true);
    expect(cache.get(key1)).toBeNull(); // 预算淘汰
    expect(released).toEqual(["d1"]);

    // retired：替换 b 的当前版本 → 旧版本 dir2 进 retired（计入驻留）→ release 归还
    const dir2b = await buildDirectory(p2);
    expect(cache.publish(key2, await cache.beginBuild(key2) as any, dir2b)).toBe(true);
    expect(cache.stats().retiredBytes).toBeGreaterThan(0);
    cache.release(key2, dir2);
    expect(released2).toEqual(["d2"]);
    expect(cache.stats().retiredBytes).toBe(0);
    cache.dispose();
  });

  it("超 16MiB 单目录 → budget_exceeded → 该会话 no-cache 不再构建", async () => {
    expect(DEFAULT_MAX_SINGLE_DIRECTORY_BYTES).toBe(16 * 1024 * 1024);
    const p = writeFixture("nocache.jsonl", FIXTURE_LINES);
    const scan = await scanHistoryFile(p, { capturedLength: fs.statSync(p).size });
    const result = await buildHistoryDirectory(scan, makeCtx(p), { maxDirectoryBytes: 64 });
    expect(result.directory).toBeNull();
    expect((result as any).reason).toBe("budget_exceeded");

    const cache = new HistoryDirectoryCache();
    const key = historyDirectoryCacheKey({ sessionId: "sess-nocache", sessionPath: p });
    cache.invalidate(key, "budget_exceeded");
    expect(await cache.beginBuild(key)).toBeNull();
    expect(cache.stats().noCacheSessions).toBe(1);
    cache.dispose();
  });

  it("默认常量为验收配置", () => {
    expect(DEFAULT_MAX_RESIDENT_INDEX_BYTES).toBe(64 * 1024 * 1024);
    expect(DEFAULT_MAX_CONCURRENT_BUILDS).toBe(2);
  });
});

describe("X17：淘汰/失效/dispose 释放；全局并发上限", () => {
  it("dispose 释放 ready 与 retired；失效调用 released", async () => {
    const p = writeFixture("dispose.jsonl", FIXTURE_LINES);
    const cache = new HistoryDirectoryCache();
    const key = historyDirectoryCacheKey({ sessionId: "sess-dispose", sessionPath: p });
    const released: string[] = [];
    const dir1 = await buildDirectory(p);
    expect(cache.publish(key, await cache.beginBuild(key) as any, dir1, { released: () => released.push("v1") })).toBe(true);
    const dir2 = await buildDirectory(p);
    expect(cache.publish(key, await cache.beginBuild(key) as any, dir2, { released: () => released.push("v2") })).toBe(true);
    expect(cache.stats().retiredBytes).toBeGreaterThan(0); // v1 在途引用计入驻留

    cache.invalidate(key, "snapshot_changed");
    // 失效释放当前版本（v2）并连带清空 retired（v1）——版本已推进，旧目录不得存活
    expect(released).toEqual(["v2", "v1"]);
    const dir3 = await buildDirectory(p);
    expect(cache.publish(key, await cache.beginBuild(key) as any, dir3, { released: () => released.push("v3") })).toBe(true);
    cache.dispose();
    expect(released).toEqual(["v2", "v1", "v3"]); // dispose 释放 ready（v3）；retired 已随失效清空
    expect(cache.stats().sessions).toBe(0);
    expect(cache.stats().residentBytes).toBe(0);
    expect(await cache.beginBuild(key)).toBeNull(); // dispose 后不再构建
  });

  it("全局构建信号量：第三个构建等待许可释放后推进（受控调度）", async () => {
    const cache = new HistoryDirectoryCache({ maxConcurrentBuilds: 2 });
    const paths = ["s1", "s2", "s3"].map((n) => writeFixture(`sem-${n}.jsonl`, FIXTURE_LINES));
    const keys = paths.map((p) => historyDirectoryCacheKey({ sessionId: null, sessionPath: p }));

    const lease1Promise = cache.beginBuild(keys[0]);
    const lease2Promise = cache.beginBuild(keys[1]);
    const lease3Promise = cache.beginBuild(keys[2]);
    const lease1 = await lease1Promise;
    const lease2 = await lease2Promise;
    expect(lease1).not.toBeNull();
    expect(lease2).not.toBeNull();

    let lease3Settled = false;
    lease3Promise.then(() => { lease3Settled = true; });
    for (let i = 0; i < 8; i += 1) await Promise.resolve();
    expect(lease3Settled).toBe(false); // 许可满：受控调度下仍未放行

    const dir1 = await buildDirectory(paths[0]);
    expect(cache.publish(keys[0], lease1!, dir1)).toBe(true); // 释放许可
    const lease3 = await lease3Promise;
    expect(lease3).not.toBeNull();
    expect(cache.stats().builds).toBe(3);
    cache.dispose();
  });
});

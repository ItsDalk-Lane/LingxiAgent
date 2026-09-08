import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { execFileSync, spawnSync } from "node:child_process";
import { describe, expect, it } from "vitest";

/**
 * round3（C01-C03）候选交付证据契约。
 *
 * 与 round2-delivery-evidence.test.ts 分层：本文件只约束本轮（基线 67dee5d2）
 * 的证据——命令记录日志存在且哈希可复算、源码 manifest 覆盖当前树（含本轮
 * 新增源码/测试）、存在同摘要无漂移的绿色门禁、增量补丁重放验证通过、
 * round2 日志交付不回退（C03 回归守卫）。不追认历史：round2 的验证事实由其
 * 自身的证据测试约束。
 *
 * manifest 与树的比对沿用 R10-03 的候选回退语义：封印推进提交（仅审计六文件）
 * 会让当前树偏离冻结 manifest，此时退回 VERIFIED_SOURCE_SHA 指向的候选提交，
 * 经 post-verification diff guard 验证后按该提交内容比对。
 */
const ROOT = path.resolve(import.meta.dirname, "..");
const OUT = path.join(ROOT, "artifacts", "f1-f12-repair", "round3-c01-c03");
const ROUND2 = path.join(ROOT, "artifacts", "f1-f12-repair", "round2");
const BASE = "67dee5d2de9d3b9fc75ec5ef5c555e93c65b3ccd";

function bytes(relative: string, base = OUT): Buffer {
  return fs.readFileSync(path.join(base, relative));
}

function sha256(value: Buffer | string): string {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function readJson(relative: string, base = OUT): any {
  return JSON.parse(bytes(relative, base).toString("utf8"));
}

function currentSourcePaths(): string[] {
  return execFileSync("git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard"], {
    cwd: ROOT, encoding: "utf8",
  }).split("\0").filter(Boolean)
    .filter(file => !(file.startsWith("artifacts/f1-f12-repair/") && !file.endsWith(".py")))
    .filter(file => !file.endsWith(".patch"))
    .filter(file => fs.statSync(path.join(ROOT, file)).isFile())
    .sort();
}

function sourcePathsAtCommit(commit: string): string[] {
  return execFileSync("git", ["ls-tree", "-r", "-z", "--name-only", commit], {
    cwd: ROOT, encoding: "utf8",
  }).split("\0").filter(Boolean)
    .filter(file => !(file.startsWith("artifacts/f1-f12-repair/") && !file.endsWith(".py")))
    .filter(file => !file.endsWith(".patch"))
    .sort();
}

function sourceContentsAtCommit(commit: string, paths: string[]): Map<string, Buffer> {
  const result = spawnSync("git", ["cat-file", "--batch"], {
    cwd: ROOT,
    input: `${paths.map((relative) => `${commit}:${relative}`).join("\n")}\n`,
    maxBuffer: 512 * 1024 * 1024,
  });
  expect(result.error, result.error?.message).toBeUndefined();
  expect(result.status, result.stderr.toString()).toBe(0);
  const contents = new Map<string, Buffer>();
  let offset = 0;
  for (const relative of paths) {
    const headerEnd = result.stdout.indexOf(0x0a, offset);
    expect(headerEnd).toBeGreaterThan(offset);
    const header = result.stdout.subarray(offset, headerEnd).toString("utf8");
    const match = header.match(/^[0-9a-f]+ blob (\d+)$/);
    expect(match, `${relative}: ${header}`).toBeTruthy();
    const size = Number(match![1]);
    const start = headerEnd + 1;
    const end = start + size;
    expect(result.stdout[end], `${relative}: cat-file record terminator`).toBe(0x0a);
    contents.set(relative, Buffer.from(result.stdout.subarray(start, end)));
    offset = end + 1;
  }
  expect(offset).toBe(result.stdout.length);
  return contents;
}

/** 当前树与冻结 manifest 一致 → null（直接比树）；否则经 seal guard 回退到候选提交。 */
function manifestSourceRef(manifest: any): string | null {
  const rows = manifest.files.map((row: any) => row.path);
  if (JSON.stringify(rows) === JSON.stringify(currentSourcePaths())
    && manifest.files.every((row: any) => {
      const content = fs.readFileSync(path.join(ROOT, row.path));
      return content.byteLength === row.bytes && sha256(content) === row.sha256;
    })) return null;
  const verified = fs.readFileSync(path.join(ROOT, ".sync-audit", "verified-source-sha.txt"), "utf8").trim();
  expect(verified).toMatch(/^[0-9a-f]{40}$/);
  const guard = execFileSync("node", [path.join(ROOT, ".sync-audit", "verify-post-verification-diff.mjs")], {
    cwd: ROOT, encoding: "utf8",
  });
  expect(guard).toContain("post-verification diff guard OK");
  return verified;
}

describe("round3 C01-C03 候选交付证据契约", () => {
  it("C03-回归守卫: round2 交付日志全部存在且哈希可复算（缺一即失败）", () => {
    const results = readJson("COMMAND_RESULTS.json", ROUND2);
    expect(results.length).toBeGreaterThan(0);
    for (const record of results) {
      expect(sha256(bytes(record.log, ROUND2))).toBe(record.logSha256);
    }
  });

  it("round3: 每条命令记录的日志存在、哈希可复算、manifest 齐备", () => {
    const results = readJson("COMMAND_RESULTS.json");
    expect(results.length).toBeGreaterThan(0);
    for (const record of results) {
      expect(record).toEqual(expect.objectContaining({
        cwd: "<WORKTREE>", platform: expect.any(String), arch: expect.any(String),
        node: expect.stringMatching(/^v24\./), npm: expect.any(String),
        start: expect.any(String), end: expect.any(String), exitCode: expect.any(Number),
        log: expect.any(String), logSha256: expect.any(String), sourceManifestHash: expect.any(String),
      }));
      expect(sha256(bytes(record.log))).toBe(record.logSha256);
      expect(fs.existsSync(path.join(OUT, "manifests", `${record.sourceManifestHash}.json`))).toBe(true);
    }
  });

  it("round3: 源码 manifest 可复算，且覆盖本轮新增源码与测试", () => {
    const manifest = readJson("SOURCE_MANIFEST.json");
    expect(manifest.sourceIdentity).toEqual({ kind: "worktree", base: BASE });
    const sourceRef = manifestSourceRef(manifest);
    const rows = manifest.files.map((row: any) => row.path);
    expect(rows).toEqual(sourceRef ? sourcePathsAtCommit(sourceRef) : currentSourcePaths());
    const committedContents = sourceRef ? sourceContentsAtCommit(sourceRef, rows) : null;
    for (const row of manifest.files) {
      const content = committedContents?.get(row.path) ?? fs.readFileSync(path.join(ROOT, row.path));
      expect(content.byteLength).toBe(row.bytes);
      expect(sha256(content)).toBe(row.sha256);
    }
    // 本轮新增源码/测试进入 manifest（C03.D）。
    for (const required of [
      "core/pinned-tenets-backup-dir.ts",
      "tests/round3-c01-input-rejection.test.ts",
      "tests/round3-c02-receipt-backup-dir.test.ts",
      "tests/round3-delivery-evidence.test.ts",
      "export-manifest.json",
    ]) {
      expect(rows).toContain(required);
    }
  });

  it("round3: 存在与最终源码摘要一致、执行期间无漂移的绿色门禁", () => {
    const manifestHash = sha256(bytes("SOURCE_MANIFEST.json"));
    const results = readJson("COMMAND_RESULTS.json");
    const green = results.find((record: any) => (
      record.exitCode === 0
      && record.sourceManifestHash === manifestHash
      && record.endSourceManifestHash === manifestHash
      && record.sourceChangedDuringCommand === false
    ));
    expect(green).toBeTruthy();
  });

  it("round3: 增量补丁存在且重放验证通过（生成脚本输出 VERIFIED 并入库为记录）", () => {
    const results = readJson("COMMAND_RESULTS.json");
    // 多次生成补丁时以最后一条为准（补丁文件随源码冻结重生成，旧记录是历史）。
    const patchRun = results.filter((record: any) => record.command.join(" ").includes("create-round3-patch.py")).at(-1);
    expect(patchRun).toBeTruthy();
    const log = bytes(patchRun.log).toString("utf8");
    expect(log).toContain('"result": "VERIFIED"');
    const summary = JSON.parse(log.slice(log.indexOf("{"), log.lastIndexOf("}") + 1));
    expect(summary.base).toBe(BASE);
    expect(summary.sourceManifestHash).toBe(summary.replayedSourceManifestHash);
    expect(fs.existsSync(path.join(ROOT, summary.patch))).toBe(true);
    expect(sha256(fs.readFileSync(path.join(ROOT, summary.patch)))).toBe(summary.patchSha256);
  });
});

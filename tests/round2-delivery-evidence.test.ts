import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { describe, expect, it } from "vitest";

const ROOT = path.resolve(import.meta.dirname, "..");
const OUT = path.join(ROOT, "artifacts", "f1-f12-repair", "round2");
const BASE = "89bc0b64bf0a9b84ef3532efaa66c23213affb70";

function bytes(relative: string): Buffer {
  return fs.readFileSync(path.join(OUT, relative));
}

function sha256(value: Buffer | string): string {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function readJson(relative: string): any {
  return JSON.parse(bytes(relative).toString("utf8"));
}

function manifestHash(): string {
  return sha256(bytes("SOURCE_MANIFEST.json"));
}

function currentSourcePaths(): string[] {
  return execFileSync("git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard"], {
    cwd: ROOT,
    encoding: "utf8",
  }).split("\0").filter(Boolean).filter((file) => {
    if (file.startsWith("artifacts/f1-f12-repair/") && !file.endsWith(".py")) return false;
    if (file.endsWith(".patch")) return false;
    return fs.statSync(path.join(ROOT, file)).isFile();
  }).sort();
}

function sourcePathsAtCommit(commit: string): string[] {
  return execFileSync("git", ["ls-tree", "-r", "-z", "--name-only", commit], {
    cwd: ROOT,
    encoding: "utf8",
  }).split("\0").filter(Boolean).filter((file) => {
    if (file.startsWith("artifacts/f1-f12-repair/") && !file.endsWith(".py")) return false;
    return !file.endsWith(".patch");
  }).sort();
}

function currentTreeMatchesManifest(manifest: any): boolean {
  const rows = manifest.files.map((row: any) => row.path);
  if (JSON.stringify(rows) !== JSON.stringify(currentSourcePaths())) return false;
  return manifest.files.every((row: any) => {
    const content = fs.readFileSync(path.join(ROOT, row.path));
    return content.byteLength === row.bytes && sha256(content) === row.sha256;
  });
}

function manifestSourceRef(manifest: any): string | null {
  if (currentTreeMatchesManifest(manifest)) return null;
  const verified = fs.readFileSync(path.join(ROOT, ".sync-audit", "verified-source-sha.txt"), "utf8").trim();
  expect(verified).toMatch(/^[0-9a-f]{40}$/);
  const guard = execFileSync("node", [path.join(ROOT, ".sync-audit", "verify-post-verification-diff.mjs")], {
    cwd: ROOT,
    encoding: "utf8",
  });
  expect(guard).toContain("post-verification diff guard OK");
  return verified;
}

function sourceBytes(relative: string, commit: string | null): Buffer {
  if (!commit) return fs.readFileSync(path.join(ROOT, relative));
  return execFileSync("git", ["show", `${commit}:${relative}`], {
    cwd: ROOT,
    maxBuffer: 16 * 1024 * 1024,
  });
}

describe("R10 round2 交付证据契约", () => {
  it("R10-01: 旧 F7/D 系列明确 superseded，当前无语音输入组件要求与保留能力写清", () => {
    const report = fs.readFileSync(path.join(ROOT, "artifacts/f1-f12-repair/F1_F12_REPAIR_REPORT.md"), "utf8");
    const speech = fs.readFileSync(path.join(ROOT, "artifacts/f1-f12-repair/SYSTEM_SPEECH_VALIDATION.md"), "utf8");
    for (const text of [report, speech]) {
      expect(text).toContain("聊天工具栏上没有增加语音输入的组件是用户要求的");
      expect(text).toContain("SUPERSEDED_BY_PRODUCT_CHANGE");
    }
    expect(speech).toContain("原生音频附件");
    expect(speech).toContain("TTS/朗读");
    expect(speech).toContain("宿主授权桥");
  });

  it("R10-02: COMMAND_RESULTS 的日志与源码 manifest 均存在且 SHA-256 可复算", () => {
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
      const manifestPath = `manifests/${record.sourceManifestHash}.json`;
      expect(sha256(bytes(manifestPath))).toBe(record.sourceManifestHash);
      expect(readJson(manifestPath).sourceIdentity).toEqual({ kind: "worktree", base: BASE });
    }
  });

  it("R10-03: 当前源码摘要可复算，且存在同摘要、执行期间未漂移的绿色门禁", () => {
    const manifest = readJson("SOURCE_MANIFEST.json");
    expect(manifest.sourceIdentity).toEqual({ kind: "worktree", base: BASE });
    const sourceRef = manifestSourceRef(manifest);
    const rows = manifest.files.map((row: any) => row.path);
    expect(rows).toEqual(sourceRef ? sourcePathsAtCommit(sourceRef) : currentSourcePaths());
    for (const row of manifest.files) {
      const content = sourceBytes(row.path, sourceRef);
      expect(content.byteLength).toBe(row.bytes);
      expect(sha256(content)).toBe(row.sha256);
    }
    const current = manifestHash();
    const green = readJson("COMMAND_RESULTS.json").find((record: any) => (
      record.exitCode === 0
      && record.sourceManifestHash === current
      && record.endSourceManifestHash === current
      && record.sourceChangedDuringCommand === false
    ));
    expect(green).toBeTruthy();
  });

  it("R10-04: tracked 与 untracked 源文件全部进入 manifest，证据/报告/补丁排除规则明示", () => {
    const manifest = readJson("SOURCE_MANIFEST.json");
    const sourceRef = manifestSourceRef(manifest);
    expect(manifest.files.map((row: any) => row.path)).toEqual(
      sourceRef ? sourcePathsAtCommit(sourceRef) : currentSourcePaths(),
    );
    expect(manifest.files.map((row: any) => row.path)).toContain("tests/round2-delivery-evidence.test.ts");
    expect(manifest.exclusions.join("\n")).toContain("generated evidence, reports, delivery files");
    expect(manifest.exclusions.join("\n")).toContain("*.patch");
  });

  it("R10-05: R01-R09 每项均绑定存在的真实测试文件、runner 标题、日志与源码摘要", () => {
    const matrix = readJson("R01_R10_TEST_MATRIX.json");
    expect(matrix.cases).toHaveLength(138);
    const cases = matrix.cases.filter((entry: any) => /^R0[1-9]-/.test(entry.id));
    expect(cases.length).toBeGreaterThan(0);
    for (const entry of cases) {
      expect(["VERIFIED", "BLOCKED"]).toContain(entry.status);
      if (entry.status === "BLOCKED") expect(entry.limitation).toEqual(expect.any(String));
      expect(entry.tests?.length).toBeGreaterThan(0);
      for (const test of entry.tests) {
        expect(fs.existsSync(path.join(ROOT, test.file))).toBe(true);
        const log = bytes(test.log).toString("utf8");
        expect(log).toContain(test.title);
        expect(fs.existsSync(path.join(OUT, "manifests", `${test.sourceManifestHash}.json`))).toBe(true);
        expect(test.result).toBe("PASSED");
      }
    }
  });

  it("S11-X2: X2-01 至 X2-12 均绑定最终联合回归的真实测试、日志与源码摘要", () => {
    const matrix = readJson("R01_R10_TEST_MATRIX.json");
    const cases = matrix.cases.filter((entry: any) => /^X2-(?:0[1-9]|1[0-2])$/.test(entry.id));
    expect(cases).toHaveLength(12);
    for (const entry of cases) {
      expect(entry.status).toBe("VERIFIED");
      expect(entry.tests?.length).toBeGreaterThan(0);
      for (const test of entry.tests) {
        expect(fs.existsSync(path.join(ROOT, test.file))).toBe(true);
        const log = bytes(test.log).toString("utf8");
        expect(log).toContain(test.title);
        expect(fs.existsSync(path.join(OUT, "manifests", `${test.sourceManifestHash}.json`))).toBe(true);
        expect(test.result).toBe("PASSED");
      }
    }
  });

  it("R10-06: 真机权限、真实供应商和其他平台只标 BLOCKED，不伪装成本地通过", () => {
    const report = bytes("R01_R10_REPAIR_REPORT.md").toString("utf8");
    expect(report).toMatch(/真机[^\n]*BLOCKED|BLOCKED[^\n]*真机/);
    expect(report).toMatch(/凭证[^\n]*BLOCKED|BLOCKED[^\n]*凭证/);
    expect(report).toMatch(/跨平台[^\n]*BLOCKED|BLOCKED[^\n]*跨平台/);
  });

  it("R10-07/08: 失败与成功记录并存且 exit 未改写，历史缺失日志明确 unavailable", () => {
    const results = readJson("COMMAND_RESULTS.json");
    expect(results.some((record: any) => record.exitCode !== 0)).toBe(true);
    expect(results.some((record: any) => record.exitCode === 0)).toBe(true);
    const progress = bytes("PROGRESS.md").toString("utf8");
    expect(progress).toContain("s9-typecheck-final");
    expect(progress).toMatch(/历史.*日志.*unavailable|unavailable.*历史.*日志/i);
  });

  it("R10-09: 增量补丁从干净 89bc0b64 重放后，源码摘要与当前工作树完全一致", () => {
    const output = execFileSync("python3", [
      path.join(OUT, "create-delivery-patch.py"),
    ], { cwd: ROOT, encoding: "utf8" });
    const result = JSON.parse(output.trim());
    expect(result).toMatchObject({
      base: BASE,
      result: "VERIFIED",
      sourceManifestHash: result.replayedSourceManifestHash,
    });
    expect(result.patchBytes).toBeGreaterThan(0);
    expect(fs.existsSync(path.join(ROOT, result.patch))).toBe(true);
    expect(sha256(fs.readFileSync(path.join(ROOT, result.patch)))).toBe(result.patchSha256);
  });

  it("R10-10: 本轮日志不泄漏真实用户路径，旧听写流程只以历史/superseded 身份出现", () => {
    const results = readJson("COMMAND_RESULTS.json");
    for (const record of results) {
      const log = bytes(record.log).toString("utf8");
      expect(log).not.toContain("/Users/study_superior");
    }
    for (const relative of [
      "../F1_F12_REPAIR_REPORT.md",
      "../F1_F12_TEST_REPORT.md",
      "../SYSTEM_SPEECH_VALIDATION.md",
    ]) {
      const text = fs.readFileSync(path.resolve(OUT, relative), "utf8");
      if (/system-dictation|D01.D12|旧前端听写/i.test(text)) {
        expect(text).toMatch(/SUPERSEDED_BY_PRODUCT_CHANGE|历史/);
      }
    }
  });
});

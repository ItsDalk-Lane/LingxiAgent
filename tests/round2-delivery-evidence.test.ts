import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { execFileSync, spawnSync } from "node:child_process";
import { describe, expect, it } from "vitest";
import {
  createPatchFixture,
  deliveryFamilyPaths,
  disposeFixture,
  expectCorruptPatchRejected,
  expectDeliveryConsistent,
  expectFailurePreservesDeliveredPatch,
  expectMismatch,
  expectNonCommitCoordinateRejected,
  expectReplayMismatchRejected,
  expectSealStateVerified,
  expectSourceCoordinateRejected,
  expectSourceStateVerified,
  fixtureGit,
  restoreDeliveryFamily,
  runPatchScript,
  snapshotDeliveryFamily,
  snapshotDigest,
  tamperFrozenManifest,
} from "./helpers/patch-seal-fixture";

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

/**
 * 读取审计坐标并证明其字面是存在的 commit 原始对象。必须在
 * manifestSourceRef 的快捷返回之前执行（R2 独立验收 F1：current==frozen 时
 * 单项用例可直接判绿，不能跳过坐标对象类型合同）。
 */
function verifiedCommitCoordinate(): string {
  const verified = fs.readFileSync(path.join(ROOT, ".sync-audit", "verified-source-sha.txt"), "utf8").trim();
  expect(verified).toMatch(/^[0-9a-f]{40}$/);
  const probe = spawnSync("git", ["cat-file", "-t", verified], { cwd: ROOT, encoding: "utf8" });
  expect(probe.status, `坐标对象不可读: ${probe.stderr}`).toBe(0);
  expect(probe.stdout.trim(), "VERIFIED_SOURCE_SHA 必须指向真实 commit 对象（拒绝 tree/tag/blob/缺失）")
    .toBe("commit");
  return verified;
}

function manifestSourceRef(manifest: any): string | null {
  const verified = verifiedCommitCoordinate();
  if (currentTreeMatchesManifest(manifest)) return null;
  const guard = execFileSync("node", [path.join(ROOT, ".sync-audit", "verify-post-verification-diff.mjs")], {
    cwd: ROOT,
    encoding: "utf8",
  });
  expect(guard).toContain("post-verification diff guard OK");
  return verified;
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
    const committedContents = sourceRef ? sourceContentsAtCommit(sourceRef, rows) : null;
    for (const row of manifest.files) {
      const content = committedContents?.get(row.path) ?? fs.readFileSync(path.join(ROOT, row.path));
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

  it("R10-09: 增量补丁从干净 89bc0b64 重放后按 source/seal 两状态验收", { timeout: 180_000 }, () => {
    // 真实运行绝不设置夹具专用覆盖（防以测试钩子冒充生产路径）。
    expect(process.env.LINGXI_PATCH_BASE_OVERRIDE).toBeUndefined();
    expect(process.env.LINGXI_PATCH_SHARD_BYTES_OVERRIDE).toBeUndefined();
    // 现场重放按当前树再生成补丁字节；封印态下字节必然与入库副本漂移（既有机制），
    // 断言在再生成字节上真实执行，完成后恢复整个交付族（单体或分片+清单）
    // 以保住交付产物与记录哈希（R2 F03：交付可能为受控分片形态）。
    const patchFile = path.join(OUT, "patches", "89bc0b64-to-r01-r10-source.patch.gz");
    const preserved = snapshotDeliveryFamily(patchFile);
    try {
      const output = execFileSync("python3", [
        path.join(OUT, "create-delivery-patch.py"),
      ], { cwd: ROOT, encoding: "utf8" });
      const result = JSON.parse(output.trim());
      expect(result).toMatchObject({
        base: BASE,
        result: "VERIFIED",
        replayedMatchesCurrent: true,
        failures: [],
      });
      expect(result.sourceManifestHash).toBe(result.replayedSourceManifestHash);
      // 两态共同坐标合同（R2 独立验收 F1）：任何 VERIFIED 出口都必须回报
      // 坐标 SHA 与精确对象类型 commit，source 快捷返回不得跳过。
      expect(result.verifiedSourceObjectType).toBe("commit");
      expect(result.verifiedSourceSha).toBe(
        fs.readFileSync(path.join(ROOT, ".sync-audit", "verified-source-sha.txt"), "utf8").trim(),
      );
      // 双状态锁定（C2 独立复核 F1 修复合同）：source（当前==完整冻结 manifest）
      // 或纯审计 seal（冻结==VERIFIED commit、guard 绿、当前==HEAD），字段如实区分。
      if (currentTreeMatchesManifest(readJson("SOURCE_MANIFEST.json"))) {
        expect(result.state).toBe("source");
        expect(result.replayedMatchesFrozenManifest).toBe(true);
      } else {
        expect(result.state).toBe("seal");
        expect(result.replayedMatchesFrozenManifest).toBe(false);
        expect(result.frozenMatchesVerifiedCommit).toBe(true);
        expect(result.currentMatchesHead).toBe(true);
        expect(result.sealGuardPassed).toBe(true);
        expect(result.verifiedSourceSha).toMatch(/^[0-9a-f]{40}$/);
      }
      // R2 F03：单体/分片两形态的交付一致性（逐片复算、重组==载荷哈希、
      // 每个交付文件低于 GitHub 100 MiB 推送硬限）。
      expectDeliveryConsistent(ROOT, result);
    } finally {
      restoreDeliveryFamily(patchFile, preserved);
    }
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

/**
 * R10-09 双状态验收的回归矩阵（C2 独立复核 F1）。全部在 /tmp 合成 git 夹具中
 * 隔离执行：被测脚本与 diff guard 原样复制、BASE 经夹具专用环境变量注入，
 * 不触碰本仓、不污染证据目录。负向场景任一被放宽都会在此显红。
 */
describe("R10-09 补丁脚本双状态回归矩阵（/tmp 合成夹具）", () => {
  it("source 状态：当前==完整冻结 manifest 时 seal guard 红不否决 VERIFIED", () => {
    const fx = createPatchFixture("round2", { seal: false });
    try {
      expectSourceStateVerified(fx);
    } finally {
      disposeFixture(fx);
    }
  });

  it("seal 状态：冻结==VERIFIED、VERIFIED..HEAD 仅审计、当前==HEAD → VERIFIED", () => {
    const fx = createPatchFixture("round2");
    try {
      expectSealStateVerified(fx);
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：冻结 manifest 任一条目被篡改 → MISMATCH", () => {
    const fx = createPatchFixture("round2");
    try {
      tamperFrozenManifest(fx);
      expectMismatch(fx, {
        state: "seal",
        frozenMatchesVerifiedCommit: false,
        currentMatchesHead: true,
        sealGuardPassed: true,
      });
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：当前未提交的源码修改 → MISMATCH（guard 只看 commits，需当前==HEAD）", () => {
    const fx = createPatchFixture("round2");
    try {
      fs.appendFileSync(path.join(fx.dir, "src", "app.txt"), "dirty\n");
      expectMismatch(fx, {
        state: "seal",
        frozenMatchesVerifiedCommit: true,
        currentMatchesHead: false,
        sealGuardPassed: true,
      });
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：当前未跟踪的新增源码文件 → MISMATCH", () => {
    const fx = createPatchFixture("round2");
    try {
      fs.writeFileSync(path.join(fx.dir, "src", "untracked.txt"), "new\n");
      expectMismatch(fx, { state: "seal", currentMatchesHead: false, sealGuardPassed: true });
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：当前未提交的源码删除 → MISMATCH", () => {
    const fx = createPatchFixture("round2");
    try {
      fs.rmSync(path.join(fx.dir, "src", "lib.txt"));
      expectMismatch(fx, { state: "seal", currentMatchesHead: false, sealGuardPassed: true });
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：当前未提交的审计文件变化 → MISMATCH（防候选字节冒充 HEAD）", () => {
    const fx = createPatchFixture("round2");
    try {
      fs.appendFileSync(path.join(fx.dir, "PROGRESS.md"), "\n- 未提交台账\n");
      expectMismatch(fx, { state: "seal", currentMatchesHead: false, sealGuardPassed: true });
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：VERIFIED..HEAD 含非白名单已提交路径 → MISMATCH", () => {
    const fx = createPatchFixture("round2");
    try {
      fs.appendFileSync(path.join(fx.dir, "src", "app.txt"), "committed\n");
      fixtureGit(fx, ["add", "-A"]);
      fixtureGit(fx, ["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "commit", "-q", "-m", "non-audit"]);
      expectMismatch(fx, {
        state: "seal",
        frozenMatchesVerifiedCommit: true,
        currentMatchesHead: true,
        sealGuardPassed: false,
      });
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：VERIFIED_SOURCE_SHA 指向不存在的对象 → MISMATCH", () => {
    const fx = createPatchFixture("round2");
    try {
      expectNonCommitCoordinateRejected(fx, "0".repeat(40), "missing");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：VERIFIED_SOURCE_SHA 指向 tree 对象（存在且树内容相同）→ 脚本与 guard 均拒绝（C3 F1）", () => {
    const fx = createPatchFixture("round2");
    try {
      // C3 独立复核 F1 原样反例：tree 树内容与冻结 manifest 全等，但对象不是 commit。
      const treeSha = fixtureGit(fx, ["rev-parse", `${fx.sourceCommit}^{tree}`]);
      expect(fixtureGit(fx, ["cat-file", "-t", treeSha])).toBe("tree");
      expectNonCommitCoordinateRejected(fx, treeSha, "tree");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：VERIFIED_SOURCE_SHA 指向 annotated tag 对象 → 脚本与 guard 均拒绝", () => {
    const fx = createPatchFixture("round2");
    try {
      fixtureGit(fx, [
        "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid",
        "tag", "-a", "-m", "seal-tag", "seal-tag", fx.sourceCommit,
      ]);
      const tagSha = fixtureGit(fx, ["rev-parse", "seal-tag"]);
      expect(fixtureGit(fx, ["cat-file", "-t", tagSha])).toBe("tag");
      expectNonCommitCoordinateRejected(fx, tagSha, "tag");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：VERIFIED_SOURCE_SHA 指向 blob 对象 → 脚本与 guard 均拒绝", () => {
    const fx = createPatchFixture("round2");
    try {
      const blobSha = fixtureGit(fx, ["hash-object", "-w", "src/app.txt"]);
      expect(fixtureGit(fx, ["cat-file", "-t", blobSha])).toBe("blob");
      expectNonCommitCoordinateRejected(fx, blobSha, "blob");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向（source 通道）：tree 对象坐标（内容匹配、重冻后命中快捷返回）→ MISMATCH（R2 F1）", () => {
    const fx = createPatchFixture("round2", { seal: false });
    try {
      // R2 独立验收 F1 原样反例的 source 形态：树内容与冻结清单全等，
      // 重冻后 replay==frozen 成立，脚本仍须在成功出口前拒绝非 commit 坐标。
      const treeSha = fixtureGit(fx, ["rev-parse", `${fx.sourceCommit}^{tree}`]);
      expect(fixtureGit(fx, ["cat-file", "-t", treeSha])).toBe("tree");
      expectSourceCoordinateRejected(fx, treeSha, "tree");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向（source 通道）：annotated tag 对象坐标 → MISMATCH", () => {
    const fx = createPatchFixture("round2", { seal: false });
    try {
      fixtureGit(fx, [
        "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid",
        "tag", "-a", "-m", "seal-tag", "seal-tag", fx.sourceCommit,
      ]);
      const tagSha = fixtureGit(fx, ["rev-parse", "seal-tag"]);
      expect(fixtureGit(fx, ["cat-file", "-t", tagSha])).toBe("tag");
      expectSourceCoordinateRejected(fx, tagSha, "tag");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向（source 通道）：blob 对象坐标 → MISMATCH", () => {
    const fx = createPatchFixture("round2", { seal: false });
    try {
      const blobSha = fixtureGit(fx, ["hash-object", "-w", "src/app.txt"]);
      expect(fixtureGit(fx, ["cat-file", "-t", blobSha])).toBe("blob");
      expectSourceCoordinateRejected(fx, blobSha, "blob");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向（source 通道）：坐标指向不存在的对象 → MISMATCH", () => {
    const fx = createPatchFixture("round2", { seal: false });
    try {
      expectSourceCoordinateRejected(fx, "0".repeat(40), "missing");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向（source 通道）：坐标非 40 位十六进制 → MISMATCH", () => {
    const fx = createPatchFixture("round2", { seal: false });
    try {
      expectSourceCoordinateRejected(fx, "not-a-sha", "malformed");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向（source 通道）：坐标文件缺失 → MISMATCH", () => {
    const fx = createPatchFixture("round2", { seal: false });
    try {
      expectSourceCoordinateRejected(fx, null, "missing-file");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：MISMATCH 不改写交付补丁（哨兵字节保留 / 原本不存在则仍不存在）", () => {
    const fx = createPatchFixture("round2");
    try {
      expectFailurePreservesDeliveredPatch(fx);
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：VERIFIED 指向树不等于冻结 manifest（错误冻结）→ MISMATCH", () => {
    const fx = createPatchFixture("round2");
    try {
      // V2 = V + 纯审计改动；坐标指向 V2 而冻结仍是 V 树 → frozenMatchesVerifiedCommit=false
      fixtureGit(fx, ["reset", "--hard", "-q", fx.sourceCommit]);
      fs.appendFileSync(path.join(fx.dir, "PROGRESS.md"), "\n- V2 审计改动\n");
      fixtureGit(fx, ["add", "-A"]);
      fixtureGit(fx, ["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "commit", "-q", "-m", "v2"]);
      const v2 = fixtureGit(fx, ["rev-parse", "HEAD"]);
      fs.writeFileSync(path.join(fx.dir, ".sync-audit", "verified-source-sha.txt"), `${v2}\n`);
      fixtureGit(fx, ["add", "-A"]);
      fixtureGit(fx, ["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "commit", "-q", "-m", "seal-to-v2"]);
      expectMismatch(fx, {
        state: "seal",
        frozenMatchesVerifiedCommit: false,
        currentMatchesHead: true,
        sealGuardPassed: true,
      });
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：补丁损坏 → 重放校验拒绝（非零退出）", () => {
    const fx = createPatchFixture("round2");
    try {
      expectCorruptPatchRejected(fx);
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：重放与当前 index 不等 → MISMATCH（保留原有重放校验）", () => {
    const fx = createPatchFixture("round2");
    try {
      expectReplayMismatchRejected(fx);
    } finally {
      disposeFixture(fx);
    }
  });

  it("分片正向：超阈值时交付受控分片，逐片与重组哈希可复算，单体不共存（R2 F03）", () => {
    const fx = createPatchFixture("round2");
    try {
      process.env.LINGXI_PATCH_SHARD_BYTES_OVERRIDE = "256";
      try {
        const run = runPatchScript(fx);
        expect(run.status, run.stderr).toBe(0);
        const result = JSON.parse(run.stdout.trim());
        expect(result.result).toBe("VERIFIED");
        expect(result.patchFormat).toBe("sharded");
        expect(result.patchShardBytes).toBe(256);
        expect(result.patchShards.length).toBeGreaterThan(1);
        expectDeliveryConsistent(fx.dir, result);
        expect(fs.existsSync(fx.patchPath)).toBe(false);
      } finally {
        delete process.env.LINGXI_PATCH_SHARD_BYTES_OVERRIDE;
      }
    } finally {
      disposeFixture(fx);
    }
  });

  it("分片负向：MISMATCH 不改写既有分片交付（清单与全部分片字节保留，R2 F03）", () => {
    const fx = createPatchFixture("round2");
    try {
      process.env.LINGXI_PATCH_SHARD_BYTES_OVERRIDE = "256";
      try {
        expect(runPatchScript(fx).status).toBe(0);
        expect(deliveryFamilyPaths(fx.patchPath).length).toBeGreaterThan(2);
        const before = snapshotDigest(snapshotDeliveryFamily(fx.patchPath));
        tamperFrozenManifest(fx);
        const run = runPatchScript(fx);
        expect(run.status, `应判 MISMATCH: ${run.stdout}`).toBe(1);
        expect(snapshotDigest(snapshotDeliveryFamily(fx.patchPath)),
          "MISMATCH 后分片交付族必须逐字节不变").toBe(before);
      } finally {
        delete process.env.LINGXI_PATCH_SHARD_BYTES_OVERRIDE;
      }
    } finally {
      disposeFixture(fx);
    }
  });

  it("分片/单体双向转换互斥回收：单体→分片移除单体，分片→单体回收分片与清单（R2 F03）", () => {
    const fx = createPatchFixture("round2");
    try {
      expect(runPatchScript(fx).status).toBe(0);
      expect(fs.existsSync(fx.patchPath)).toBe(true);
      process.env.LINGXI_PATCH_SHARD_BYTES_OVERRIDE = "256";
      try {
        expect(runPatchScript(fx).status).toBe(0);
      } finally {
        delete process.env.LINGXI_PATCH_SHARD_BYTES_OVERRIDE;
      }
      expect(fs.existsSync(fx.patchPath), "分片交付后单体必须移除").toBe(false);
      expect(fs.existsSync(`${fx.patchPath}.shards.json`)).toBe(true);
      expect(runPatchScript(fx).status).toBe(0);
      expect(fs.existsSync(fx.patchPath), "单体交付必须恢复").toBe(true);
      expect(deliveryFamilyPaths(fx.patchPath)).toEqual([fx.patchPath]);
    } finally {
      disposeFixture(fx);
    }
  });
});

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { execFileSync, spawnSync } from "node:child_process";
import { describe, expect, it } from "vitest";
import {
  createPatchFixture,
  disposeFixture,
  expectCorruptPatchRejected,
  expectFailurePreservesDeliveredPatch,
  expectMismatch,
  expectNonCommitCoordinateRejected,
  expectReplayMismatchRejected,
  expectSealStateVerified,
  expectSourceCoordinateRejected,
  expectSourceStateVerified,
  fixtureGit,
  tamperFrozenManifest,
} from "./helpers/patch-seal-fixture";

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

/** 当前树与冻结 manifest 一致 → null（直接比树）；否则经 seal guard 回退到候选提交。 */
function manifestSourceRef(manifest: any): string | null {
  const verified = verifiedCommitCoordinate();
  const rows = manifest.files.map((row: any) => row.path);
  if (JSON.stringify(rows) === JSON.stringify(currentSourcePaths())
    && manifest.files.every((row: any) => {
      const content = fs.readFileSync(path.join(ROOT, row.path));
      return content.byteLength === row.bytes && sha256(content) === row.sha256;
    })) return null;
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

  it("round3: 现场重放增量补丁并按 source/seal 两状态验收（不只读历史 VERIFIED 记录）", { timeout: 180_000 }, () => {
    // C2 独立复核 F1：历史记录核对不能证明现行脚本可用，必须现场执行。
    // 真实运行绝不设置夹具专用 BASE 覆盖（防以测试钩子冒充生产路径）。
    // 现场重放按当前树再生成补丁字节（round3 补丁 diff 面包含证据文件，封印态下
    // 字节必然漂移）；断言在再生成字节上真实执行，完成后恢复原字节以保住交付
    // 产物与历史记录哈希。保持在本文件末位，先于它运行的记录核对不受再生成影响。
    expect(process.env.LINGXI_PATCH_BASE_OVERRIDE).toBeUndefined();
    const patchFile = path.join(OUT, "patches", "67dee5d2-to-round3-c01-c03.patch.gz");
    const preserved = fs.readFileSync(patchFile);
    try {
      const output = execFileSync("python3", [
        path.join(OUT, "create-round3-patch.py"),
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
      const manifest = readJson("SOURCE_MANIFEST.json");
      const sourceState = JSON.stringify(manifest.files.map((row: any) => row.path)) === JSON.stringify(currentSourcePaths())
        && manifest.files.every((row: any) => {
          const content = fs.readFileSync(path.join(ROOT, row.path));
          return content.byteLength === row.bytes && sha256(content) === row.sha256;
        });
      if (sourceState) {
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
      expect(result.patchBytes).toBeGreaterThan(0);
      expect(fs.existsSync(path.join(ROOT, result.patch))).toBe(true);
      expect(sha256(fs.readFileSync(path.join(ROOT, result.patch)))).toBe(result.patchSha256);
    } finally {
      fs.writeFileSync(patchFile, preserved);
    }
  });
});

/**
 * round3 补丁脚本双状态验收的回归矩阵（C2 独立复核 F1），与 round2 同构。
 * 全部在 /tmp 合成 git 夹具中隔离执行：被测脚本与 diff guard 原样复制、
 * BASE 经夹具专用环境变量注入，不触碰本仓、不污染证据目录。
 */
describe("round3 补丁脚本双状态回归矩阵（/tmp 合成夹具）", () => {
  it("source 状态：当前==完整冻结 manifest 时 seal guard 红不否决 VERIFIED", () => {
    const fx = createPatchFixture("round3", { seal: false });
    try {
      expectSourceStateVerified(fx);
    } finally {
      disposeFixture(fx);
    }
  });

  it("seal 状态：冻结==VERIFIED、VERIFIED..HEAD 仅审计、当前==HEAD → VERIFIED", () => {
    const fx = createPatchFixture("round3");
    try {
      expectSealStateVerified(fx);
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：冻结 manifest 任一条目被篡改 → MISMATCH", () => {
    const fx = createPatchFixture("round3");
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

  it("负向：当前未提交/未跟踪的源码变化 → MISMATCH（guard 只看 commits，需当前==HEAD）", () => {
    const fx = createPatchFixture("round3");
    try {
      fs.appendFileSync(path.join(fx.dir, "src", "app.txt"), "dirty\n");
      expectMismatch(fx, { state: "seal", currentMatchesHead: false, sealGuardPassed: true });
      fixtureGit(fx, ["checkout", "--", "src/app.txt"]);
      fs.writeFileSync(path.join(fx.dir, "src", "untracked.txt"), "new\n");
      expectMismatch(fx, { state: "seal", currentMatchesHead: false, sealGuardPassed: true });
      fs.rmSync(path.join(fx.dir, "src", "untracked.txt"));
      fs.rmSync(path.join(fx.dir, "src", "lib.txt"));
      expectMismatch(fx, { state: "seal", currentMatchesHead: false, sealGuardPassed: true });
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：当前未提交的审计文件变化 → MISMATCH（防候选字节冒充 HEAD）", () => {
    const fx = createPatchFixture("round3");
    try {
      fs.appendFileSync(path.join(fx.dir, "PROGRESS.md"), "\n- 未提交台账\n");
      expectMismatch(fx, { state: "seal", currentMatchesHead: false, sealGuardPassed: true });
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：VERIFIED..HEAD 含非白名单已提交路径 → MISMATCH", () => {
    const fx = createPatchFixture("round3");
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
    const fx = createPatchFixture("round3");
    try {
      expectNonCommitCoordinateRejected(fx, "0".repeat(40), "missing");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：VERIFIED_SOURCE_SHA 指向 tree 对象（存在且树内容相同）→ 脚本与 guard 均拒绝（C3 F1）", () => {
    const fx = createPatchFixture("round3");
    try {
      const treeSha = fixtureGit(fx, ["rev-parse", `${fx.sourceCommit}^{tree}`]);
      expect(fixtureGit(fx, ["cat-file", "-t", treeSha])).toBe("tree");
      expectNonCommitCoordinateRejected(fx, treeSha, "tree");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：VERIFIED_SOURCE_SHA 指向 annotated tag 对象 → 脚本与 guard 均拒绝", () => {
    const fx = createPatchFixture("round3");
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
    const fx = createPatchFixture("round3");
    try {
      const blobSha = fixtureGit(fx, ["hash-object", "-w", "src/app.txt"]);
      expect(fixtureGit(fx, ["cat-file", "-t", blobSha])).toBe("blob");
      expectNonCommitCoordinateRejected(fx, blobSha, "blob");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向（source 通道）：tree 对象坐标（内容匹配、重冻后命中快捷返回）→ MISMATCH（R2 F1）", () => {
    const fx = createPatchFixture("round3", { seal: false });
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
    const fx = createPatchFixture("round3", { seal: false });
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
    const fx = createPatchFixture("round3", { seal: false });
    try {
      const blobSha = fixtureGit(fx, ["hash-object", "-w", "src/app.txt"]);
      expect(fixtureGit(fx, ["cat-file", "-t", blobSha])).toBe("blob");
      expectSourceCoordinateRejected(fx, blobSha, "blob");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向（source 通道）：坐标指向不存在的对象 → MISMATCH", () => {
    const fx = createPatchFixture("round3", { seal: false });
    try {
      expectSourceCoordinateRejected(fx, "0".repeat(40), "missing");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向（source 通道）：坐标非 40 位十六进制 → MISMATCH", () => {
    const fx = createPatchFixture("round3", { seal: false });
    try {
      expectSourceCoordinateRejected(fx, "not-a-sha", "malformed");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向（source 通道）：坐标文件缺失 → MISMATCH", () => {
    const fx = createPatchFixture("round3", { seal: false });
    try {
      expectSourceCoordinateRejected(fx, null, "missing-file");
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：MISMATCH 不改写交付补丁（哨兵字节保留 / 原本不存在则仍不存在）", () => {
    const fx = createPatchFixture("round3");
    try {
      expectFailurePreservesDeliveredPatch(fx);
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：VERIFIED 指向树不等于冻结 manifest（错误冻结）→ MISMATCH", () => {
    const fx = createPatchFixture("round3");
    try {
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
    const fx = createPatchFixture("round3");
    try {
      expectCorruptPatchRejected(fx);
    } finally {
      disposeFixture(fx);
    }
  });

  it("负向：重放与当前 index 不等 → MISMATCH（保留原有重放校验）", () => {
    const fx = createPatchFixture("round3");
    try {
      expectReplayMismatchRejected(fx);
    } finally {
      disposeFixture(fx);
    }
  });
});

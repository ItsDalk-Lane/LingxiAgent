/**
 * round2/round3 交付补丁脚本的 /tmp 合成夹具（C2 独立复核 F1 回归矩阵）。
 *
 * 每份夹具是一个独立的微型 git 仓库：base 提交 → source 提交 V（冻结 manifest
 * 绑定 V 树）→ 可选纯审计 seal 提交 S（仅白名单文件：坐标 + PROGRESS.md）。
 * 被测脚本与 post-verification diff guard 原样复制进夹具，BASE 经
 * LINGXI_PATCH_BASE_OVERRIDE 注入（真实仓库的证据测试断言该变量未设置，
 * 夹具覆盖不可能冒充生产路径）。全部副作用只在 os.tmpdir() 下，不触碰本仓。
 *
 * R2 独立验收 F1 补强：坐标对象类型负向同时覆盖 source 与 seal 两态
 * （source 通道需按被测脚本同一范围重冻 manifest 才真正命中快捷返回）；
 * 交付补丁改写边界锁定为「MISMATCH/异常保留原字节或保持不存在，
 * VERIFIED 后原子写入且字节与 patchSha256 一致」。
 */
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import zlib from "node:zlib";
import { execFileSync, spawnSync } from "node:child_process";
import { expect } from "vitest";

const ROOT = path.resolve(import.meta.dirname, "..", "..");

export type PatchKind = "round2" | "round3";

const SCRIPT_REL: Record<PatchKind, string> = {
  round2: "artifacts/f1-f12-repair/round2/create-delivery-patch.py",
  round3: "artifacts/f1-f12-repair/round3-c01-c03/create-round3-patch.py",
};

const PATCH_REL: Record<PatchKind, string> = {
  round2: "patches/89bc0b64-to-r01-r10-source.patch.gz",
  round3: "patches/67dee5d2-to-round3-c01-c03.patch.gz",
};

function sha256(value: Buffer): string {
  return crypto.createHash("sha256").update(value).digest("hex");
}

export interface PatchFixture {
  kind: PatchKind;
  dir: string;
  /** 夹具 BASE 提交（经 LINGXI_PATCH_BASE_OVERRIDE 注入被测脚本）。 */
  base: string;
  /** 冻结 manifest 绑定的源码提交 V。 */
  sourceCommit: string;
  /** 纯审计 seal 提交 S（HEAD；opts.seal === false 时等于 sourceCommit）。 */
  sealCommit: string;
  scriptPath: string;
  manifestPath: string;
  /** 被测脚本的交付补丁路径（夹具内）。 */
  patchPath: string;
}

export interface ScriptRun {
  status: number;
  stdout: string;
  stderr: string;
}

export function fixtureGit(fx: PatchFixture, args: string[]): string {
  return execFileSync("git", args, { cwd: fx.dir, encoding: "utf8" }).trim();
}

function commitAll(fx: Pick<PatchFixture, "dir">, message: string): string {
  fixtureGit(fx as PatchFixture, ["add", "-A"]);
  fixtureGit(fx as PatchFixture, [
    "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid",
    "commit", "-q", "-m", message,
  ]);
  return fixtureGit(fx as PatchFixture, ["rev-parse", "HEAD"]);
}

/** 用被测脚本自身的 stage/manifest 逻辑冻结夹具 manifest（范围定义与被测脚本一致）。 */
const FREEZE_DRIVER = `
import importlib.util, os, sys
spec = importlib.util.spec_from_file_location("patchmod", sys.argv[1])
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
index = mod.temp_index("fixture-freeze-")
try:
    mod.stage_current(index)
    sys.stdout.buffer.write(mod.manifest_from_index(index))
finally:
    os.unlink(index)
`;

/** 改写夹具文件后按被测脚本同一范围重冻 manifest（source 通道负向的必要输入）。 */
export function refreezeManifest(fx: PatchFixture): void {
  const frozen = runPythonDriver(fx, FREEZE_DRIVER);
  if (frozen.status !== 0) {
    throw new Error(`fixture refreeze failed: ${frozen.stderr}`);
  }
  fs.writeFileSync(fx.manifestPath, frozen.stdout);
}

/** 截断补丁必须被重放校验拒绝（round2 RuntimeError / round3 SystemExit）。 */
const CORRUPT_PATCH_DRIVER = `
import importlib.util, sys
spec = importlib.util.spec_from_file_location("patchmod", sys.argv[1])
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
if hasattr(mod, "replay_and_verify"):
    patch, stored = mod.build_patch()
    try:
        mod.replay_and_verify(patch[: len(patch) // 2], stored)
    except Exception:
        sys.exit(0)
    sys.exit(1)
patch, stored, _rows, _current = mod.build_patch()
try:
    mod.replay_patch(patch[: len(patch) // 2])
except SystemExit:
    sys.exit(0)
sys.exit(1)
`;

/** 生成期之后工作树漂移：重放结果与当前 index 不等必须判 MISMATCH。 */
const REPLAY_MISMATCH_DRIVER_R2 = `
import importlib.util, pathlib, sys
spec = importlib.util.spec_from_file_location("patchmod", sys.argv[1])
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
patch, stored = mod.build_patch()
target = pathlib.Path(sys.argv[2])
target.write_bytes(target.read_bytes() + b"drift\\n")
res = mod.replay_and_verify(patch, stored)
sys.exit(0 if (res["result"] == "MISMATCH" and res["replayedMatchesCurrent"] is False) else 1)
`;

/** 直接驱动 evaluate：重放清单与当前清单不等必须判 MISMATCH。 */
const REPLAY_MISMATCH_DRIVER_R3 = `
import importlib.util, sys
spec = importlib.util.spec_from_file_location("patchmod", sys.argv[1])
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
row_a = {"path": "src/app.txt", "bytes": 3, "sha256": "0" * 64}
row_b = {"path": "src/app.txt", "bytes": 4, "sha256": "1" * 64}
res = mod.evaluate(
    patch=b"p", stored=b"s",
    current_rows=[row_a], current=b"{}",
    replay_rows=[row_b], replayed=b"{}",
)
sys.exit(0 if (res["result"] == "MISMATCH" and res["replayedMatchesCurrent"] is False) else 1)
`;

function runPythonDriver(fx: PatchFixture, source: string, args: string[] = []): ScriptRun {
  const driverDir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-patch-driver-"));
  try {
    const driverPath = path.join(driverDir, "driver.py");
    fs.writeFileSync(driverPath, source);
    const res = spawnSync("python3", [driverPath, fx.scriptPath, ...args], {
      cwd: fx.dir,
      encoding: "utf8",
      env: { ...process.env, LINGXI_PATCH_BASE_OVERRIDE: fx.base },
      maxBuffer: 16 * 1024 * 1024,
    });
    return { status: res.status ?? -1, stdout: res.stdout, stderr: res.stderr };
  } finally {
    fs.rmSync(driverDir, { recursive: true, force: true });
  }
}

export function createPatchFixture(kind: PatchKind, opts: { seal?: boolean } = {}): PatchFixture {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `lingxi-${kind}-seal-fixture-`));
  const fx: PatchFixture = {
    kind, dir, base: "", sourceCommit: "", sealCommit: "",
    scriptPath: path.join(dir, SCRIPT_REL[kind]),
    manifestPath: path.join(dir, path.dirname(SCRIPT_REL[kind]), "SOURCE_MANIFEST.json"),
    patchPath: path.join(dir, path.dirname(SCRIPT_REL[kind]), PATCH_REL[kind]),
  };
  fixtureGit(fx, ["init", "-q"]);
  fs.mkdirSync(path.dirname(fx.scriptPath), { recursive: true });
  fs.copyFileSync(path.join(ROOT, SCRIPT_REL[kind]), fx.scriptPath);
  fs.mkdirSync(path.join(dir, ".sync-audit"), { recursive: true });
  fs.copyFileSync(
    path.join(ROOT, ".sync-audit", "verify-post-verification-diff.mjs"),
    path.join(dir, ".sync-audit", "verify-post-verification-diff.mjs"),
  );
  fs.mkdirSync(path.join(dir, "src"), { recursive: true });
  // 与主仓同一行尾合同：blob 字节 == 工作树字节（Windows autocrlf 防漂）。
  fs.writeFileSync(path.join(dir, ".gitattributes"), "* text=auto eol=lf\n");
  // 夹具卫生：生成的 manifest/补丁/缓存不入任何提交（被测脚本 keep() 本就排除，
  // 这里防 commitAll 的 add -A 误收而触发 guard 的非白名单判定）。
  fs.writeFileSync(path.join(dir, ".gitignore"), [
    "__pycache__/",
    "artifacts/f1-f12-repair/round2/SOURCE_MANIFEST.json",
    "artifacts/f1-f12-repair/round2/patches/",
    "artifacts/f1-f12-repair/round3-c01-c03/SOURCE_MANIFEST.json",
    "artifacts/f1-f12-repair/round3-c01-c03/patches/",
    "",
  ].join("\n"));
  fs.writeFileSync(path.join(dir, "src", "app.txt"), "v1\n");
  fs.writeFileSync(path.join(dir, "PROGRESS.md"), "# 进度\n");
  fx.base = commitAll(fx, "base");
  // source 提交 V：坐标文件指向 base（合法 40 位），源码演进（非白名单差异）。
  fs.writeFileSync(path.join(dir, ".sync-audit", "verified-source-sha.txt"), `${fx.base}\n`);
  fs.writeFileSync(path.join(dir, "src", "app.txt"), "v2\n");
  fs.writeFileSync(path.join(dir, "src", "lib.txt"), "lib\n");
  fx.sourceCommit = commitAll(fx, "source");
  const frozen = runPythonDriver(fx, FREEZE_DRIVER);
  if (frozen.status !== 0) {
    throw new Error(`fixture freeze failed: ${frozen.stderr}`);
  }
  fs.writeFileSync(fx.manifestPath, frozen.stdout);
  fx.sealCommit = fx.sourceCommit;
  if (opts.seal !== false) {
    // 纯审计 seal 提交 S：仅 allowlist 文件（坐标推进 + 进度台账）。
    fs.writeFileSync(path.join(dir, ".sync-audit", "verified-source-sha.txt"), `${fx.sourceCommit}\n`);
    fs.appendFileSync(path.join(dir, "PROGRESS.md"), "\n- 纯审计 seal 推进\n");
    fx.sealCommit = commitAll(fx, "seal");
  }
  return fx;
}

export function disposeFixture(fx: PatchFixture): void {
  fs.rmSync(fx.dir, { recursive: true, force: true });
}

export function runPatchScript(fx: PatchFixture): ScriptRun {
  const res = spawnSync("python3", [fx.scriptPath], {
    cwd: fx.dir,
    encoding: "utf8",
    env: { ...process.env, LINGXI_PATCH_BASE_OVERRIDE: fx.base },
    maxBuffer: 16 * 1024 * 1024,
  });
  return { status: res.status ?? -1, stdout: res.stdout, stderr: res.stderr };
}

export function runGuard(fx: PatchFixture): ScriptRun {
  const res = spawnSync(
    "node", [path.join(fx.dir, ".sync-audit", "verify-post-verification-diff.mjs")],
    { cwd: fx.dir, encoding: "utf8" },
  );
  return { status: res.status ?? -1, stdout: res.stdout, stderr: res.stderr };
}

function parseResult(run: ScriptRun): any {
  return JSON.parse(run.stdout.trim());
}

/** source 状态：当前==完整冻结 manifest → VERIFIED；seal guard 红不否决。 */
export function expectSourceStateVerified(fx: PatchFixture): void {
  const guard = runGuard(fx);
  expect(guard.status, "夹具 source 状态下 guard 应为结构性红").toBe(1);
  const run = runPatchScript(fx);
  expect(run.status, run.stderr).toBe(0);
  const result = parseResult(run);
  expect(result).toMatchObject({
    result: "VERIFIED",
    state: "source",
    replayedMatchesCurrent: true,
    replayedMatchesFrozenManifest: true,
    failures: [],
  });
  // source 成功出口同样必须证明坐标字面是 commit 原始对象（R2 独立验收 F1）。
  expect(result.verifiedSourceObjectType).toBe("commit");
  expect(result.verifiedSourceSha).toBe(
    fs.readFileSync(path.join(fx.dir, ".sync-audit", "verified-source-sha.txt"), "utf8").trim(),
  );
  // 成功时交付补丁原子写入，字节与 patchSha256 一致。
  expect(fs.existsSync(fx.patchPath)).toBe(true);
  expect(sha256(fs.readFileSync(fx.patchPath))).toBe(result.patchSha256);
}

/** seal 状态：冻结==VERIFIED、guard 绿、当前==HEAD → VERIFIED。 */
export function expectSealStateVerified(fx: PatchFixture): void {
  const run = runPatchScript(fx);
  expect(run.status, run.stderr).toBe(0);
  const result = parseResult(run);
  expect(result).toMatchObject({
    result: "VERIFIED",
    state: "seal",
    replayedMatchesCurrent: true,
    replayedMatchesFrozenManifest: false,
    frozenMatchesVerifiedCommit: true,
    currentMatchesHead: true,
    sealGuardPassed: true,
    verifiedSourceSha: fx.sourceCommit,
    verifiedSourceObjectType: "commit",
    failures: [],
  });
  // 成功时交付补丁原子写入，字节与 patchSha256 一致。
  expect(fs.existsSync(fx.patchPath)).toBe(true);
  expect(sha256(fs.readFileSync(fx.patchPath))).toBe(result.patchSha256);
}

/** 任一适用条件不满足 → 非零退出、result=MISMATCH，且指定字段如实为 false。 */
export function expectMismatch(fx: PatchFixture, fields: Record<string, unknown>): any {
  const run = runPatchScript(fx);
  expect(run.status, `应判 MISMATCH: ${run.stdout}`).toBe(1);
  const result = parseResult(run);
  expect(result.result).toBe("MISMATCH");
  expect(result.failures.length).toBeGreaterThan(0);
  expect(result).toMatchObject(fields);
  return result;
}

/** 篡改冻结 manifest 任一条目 hash。 */
export function tamperFrozenManifest(fx: PatchFixture): void {
  const raw = JSON.parse(fs.readFileSync(fx.manifestPath, "utf8"));
  raw.files[0].sha256 = "0".repeat(64);
  fs.writeFileSync(fx.manifestPath, `${JSON.stringify(raw, null, 2)}\n`);
}

/**
 * 坐标指向非 commit 对象（存在的 tree/tag/blob，或缺失对象）必须被双重拒绝：
 * 独立 guard 非零退出且给出 commit 类型错误；补丁脚本判 MISMATCH 并如实报告
 * 对象类型。tree/tag 同样被 git ls-tree/diff 接受（C3 独立复核 F1），40 位
 * 格式检查不足以证明坐标是真实源码提交。
 */
export function expectNonCommitCoordinateRejected(
  fx: PatchFixture,
  coordinate: string,
  objectType: "tree" | "tag" | "blob" | "missing",
): void {
  fs.writeFileSync(path.join(fx.dir, ".sync-audit", "verified-source-sha.txt"), `${coordinate}\n`);
  fixtureGit(fx, ["add", "-A"]);
  fixtureGit(fx, [
    "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid",
    "commit", "-q", "-m", `coord-${objectType}`,
  ]);
  const guard = runGuard(fx);
  expect(guard.status, `guard 必须拒绝 ${objectType} 坐标: ${guard.stdout}${guard.stderr}`).toBe(1);
  expect(`${guard.stdout}${guard.stderr}`).toContain("commit 对象");
  const result = expectMismatch(fx, { state: "seal" });
  expect(result.verifiedSourceSha).toBe(coordinate);
  expect(result.verifiedSourceObjectType).toBe(objectType === "missing" ? null : objectType);
  expect(result.failures.join("\n")).toContain("commit 对象");
}

/** 损坏补丁必须被拒绝（脚本自身的重放校验，driver exit 0 = 拒绝发生）。 */
export function expectCorruptPatchRejected(fx: PatchFixture): void {
  // 异常路径同样不得改写交付补丁：预置哨兵字节，driver 全程不应触碰。
  fs.mkdirSync(path.dirname(fx.patchPath), { recursive: true });
  const sentinel = Buffer.from(`lingxi-corrupt-sentinel-${fx.kind}\n`, "utf8");
  fs.writeFileSync(fx.patchPath, sentinel);
  try {
    const run = runPythonDriver(fx, CORRUPT_PATCH_DRIVER);
    expect(run.status, `损坏补丁未被拒绝: ${run.stderr}`).toBe(0);
    expect(fs.readFileSync(fx.patchPath), "异常路径必须保留交付补丁原字节").toEqual(sentinel);
  } finally {
    fs.rmSync(fx.patchPath, { force: true });
  }
}

/**
 * source 状态下的坐标负向（R2 独立验收 F1 的 source 通道）：改写坐标后按被测
 * 脚本同一范围重冻 manifest，让 current==frozen 确实命中 source 快捷返回；
 * 脚本必须 exit 1 / MISMATCH 且如实报告坐标对象类型。guard 在 source 状态
 * 结构性为红，不能依赖它否决该通道。coordinate=null 表示删除坐标文件。
 */
export function expectSourceCoordinateRejected(
  fx: PatchFixture,
  coordinate: string | null,
  objectType: "tree" | "tag" | "blob" | "missing" | "malformed" | "missing-file",
): void {
  const coordinatePath = path.join(fx.dir, ".sync-audit", "verified-source-sha.txt");
  if (coordinate === null) {
    fs.rmSync(coordinatePath);
  } else {
    fs.writeFileSync(coordinatePath, `${coordinate}\n`);
  }
  refreezeManifest(fx);
  const run = runPatchScript(fx);
  expect(run.status, `source 状态必须拒绝 ${objectType} 坐标: ${run.stdout}${run.stderr}`).toBe(1);
  const result = parseResult(run);
  // 确认确实命中 source 快捷通道（重放==冻结==当前），而非被其他前置条件拦下。
  expect(result).toMatchObject({
    state: "source",
    result: "MISMATCH",
    replayedMatchesCurrent: true,
    replayedMatchesFrozenManifest: true,
  });
  const failures = result.failures.join("\n");
  if (objectType === "missing-file") {
    expect(result.verifiedSourceSha).toBeNull();
    expect(result.verifiedSourceObjectType).toBeNull();
    expect(failures).toContain("缺少坐标文件");
  } else if (objectType === "malformed") {
    expect(result.verifiedSourceSha).toBeNull();
    expect(result.verifiedSourceObjectType).toBeNull();
    expect(failures).toContain("非 40 位十六进制");
  } else {
    expect(result.verifiedSourceSha).toBe(coordinate);
    expect(result.verifiedSourceObjectType).toBe(objectType === "missing" ? null : objectType);
    expect(failures).toContain("commit 对象");
  }
}

/**
 * 失败不改写交付补丁（R2 独立验收连带修复）：以持续 MISMATCH（冻结篡改）为
 * 触发器，锁定哨兵字节保留与文件原本不存在两种输入；成功写出由
 * expectSourceStateVerified / expectSealStateVerified 的 patchSha256 断言锁定。
 */
export function expectFailurePreservesDeliveredPatch(fx: PatchFixture): void {
  tamperFrozenManifest(fx);
  fs.mkdirSync(path.dirname(fx.patchPath), { recursive: true });
  const sentinel = Buffer.from(`lingxi-patch-sentinel-${fx.kind}\n`, "utf8");
  fs.writeFileSync(fx.patchPath, sentinel);
  const first = runPatchScript(fx);
  expect(first.status, `应判 MISMATCH: ${first.stdout}`).toBe(1);
  expect(fs.readFileSync(fx.patchPath), "MISMATCH 必须保留交付补丁原字节").toEqual(sentinel);
  fs.rmSync(fx.patchPath);
  const second = runPatchScript(fx);
  expect(second.status, `应判 MISMATCH: ${second.stdout}`).toBe(1);
  expect(fs.existsSync(fx.patchPath), "交付补丁原本不存在时失败不得创建").toBe(false);
}

/** 重放与当前 index 不等必须判 MISMATCH（driver exit 0 = 判定发生）。 */
export function expectReplayMismatchRejected(fx: PatchFixture): void {
  const run = fx.kind === "round2"
    ? runPythonDriver(fx, REPLAY_MISMATCH_DRIVER_R2, [path.join(fx.dir, "src", "app.txt")])
    : runPythonDriver(fx, REPLAY_MISMATCH_DRIVER_R3);
  expect(run.status, `重放不等未判 MISMATCH: ${run.stderr}`).toBe(0);
}

/* ------------------------------------------------------------------ *
 * R01 阶段验收 R2 F03：受控分片交付合同的共享断言与交付族快照工具。    *
 * 分片只改变存储分帧：patchSha256/patchBytes 仍钉住完整 gzip 载荷，    *
 * 逐片 path/bytes/sha256 由 <patch>.shards.json 清单钉住，消费者按序   *
 * 重组后必须复算出同一载荷哈希——完整性校验语义与单体时代一致。         *
 * ------------------------------------------------------------------ */

/** GitHub 普通 Git 单文件推送硬限 100 MiB（交付物每个文件都必须低于它）。 */
export const GITHUB_SINGLE_FILE_LIMIT = 100 * 1024 * 1024;

/**
 * 交付清单/结果里的相对路径一律先校验再读写：拒绝绝对路径与 .. 片段，
 * 解析后必须仍位于 rootAbs 内（防篡改清单把读取引出仓库/夹具根）。
 */
function resolveContained(rootAbs: string, rel: string): string {
  expect(typeof rel).toBe("string");
  expect(path.isAbsolute(rel), `交付路径不得为绝对路径: ${rel}`).toBe(false);
  expect(rel.split(/[\\/]/).includes(".."), `交付路径不得含 .. 片段: ${rel}`).toBe(false);
  const root = path.resolve(rootAbs);
  const resolved = path.resolve(root, rel);
  expect(
    resolved === root || resolved.startsWith(root + path.sep),
    `交付路径必须位于根目录内: ${rel}`,
  ).toBe(true);
  return resolved;
}

/** 交付族现存文件（单体 + <patch>.shards.json + <patch>.part-*），按名排序。 */
export function deliveryFamilyPaths(patchAbsPath: string): string[] {
  const dir = path.dirname(patchAbsPath);
  const name = path.basename(patchAbsPath);
  const out: string[] = [];
  if (fs.existsSync(patchAbsPath)) out.push(patchAbsPath);
  const manifest = `${patchAbsPath}.shards.json`;
  if (fs.existsSync(manifest)) out.push(manifest);
  if (fs.existsSync(dir)) {
    for (const entry of fs.readdirSync(dir).filter((e) => e.startsWith(`${name}.part-`)).sort()) {
      out.push(path.join(dir, entry));
    }
  }
  return out;
}

/** 交付族快照（绝对路径→字节），用于 preserve/restore 与「失败不改写」断言。 */
export function snapshotDeliveryFamily(patchAbsPath: string): Map<string, Buffer> {
  return new Map(deliveryFamilyPaths(patchAbsPath).map((p) => [p, fs.readFileSync(p)]));
}

/** 恢复交付族到快照状态：删除快照外文件、重写快照文件（含被删除形态的重建）。 */
export function restoreDeliveryFamily(patchAbsPath: string, snap: Map<string, Buffer>): void {
  for (const p of deliveryFamilyPaths(patchAbsPath)) {
    if (!snap.has(p)) fs.rmSync(p, { force: true });
  }
  for (const [p, bytes] of snap) {
    fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.writeFileSync(p, bytes);
  }
}

/** 快照内容摘要（文件名+字节），用于断言 MISMATCH/异常后交付族逐字节不变。 */
export function snapshotDigest(snap: Map<string, Buffer>): string {
  const h = crypto.createHash("sha256");
  for (const [p, b] of [...snap].sort()) {
    h.update(path.basename(p));
    h.update("\0");
    h.update(b);
    h.update("\0");
  }
  return h.digest("hex");
}

/** 交付结果（生成器 stdout JSON / 历史记录）中本断言族关心的字段形状。 */
export interface PatchDeliveryResultShape {
  patch: string;
  patchBytes: number;
  patchSha256: string;
  patchFormat?: string;
  patchManifest?: string;
  patchShardBytes?: number;
  patchShards?: Array<{ path: string; bytes: number; sha256: string }>;
  patchUncompressedBytes?: number;
}

/** 分片清单文件（.shards.json）的契约形状，与生成器写入端一一对应。 */
export interface PatchShardsManifestShape {
  format: string;
  patch: string;
  patchBytes: number;
  patchSha256: string;
  shardBytes: number;
  shards: Array<{ path: string; bytes: number; sha256: string }>;
}

/**
 * 交付一致性断言（R2 F03 分片合同）。rootAbs 为补丁相对路径的解析根
 * （主仓 ROOT 或夹具目录）。缺 patchFormat 字段的历史记录按 single 处理。
 *
 * single：单体存在、字节级哈希/尺寸与 result 一致、低于 GitHub 硬限、
 *   无清单无分片残留。
 * sharded：单体不存在；清单 format/patch/总哈希/分片阈值与 result 一致；
 *   逐片存在且 bytes/sha256 复算一致、每片 ≤ patchShardBytes 且低于硬限；
 *   按序重组 == 完整载荷（sha256 == patchSha256），重组字节可 gunzip 且
 *   仍是 git 补丁文本（重放语义不损失）。
 */
export function expectDeliveryConsistent(rootAbs: string, result: PatchDeliveryResultShape): void {
  expect(result.patchBytes).toBeGreaterThan(0);
  expect(typeof result.patch).toBe("string");
  const patchAbs = resolveContained(rootAbs, result.patch);
  const manifestAbs = `${patchAbs}.shards.json`;
  if ((result.patchFormat ?? "single") === "single") {
    expect(fs.existsSync(patchAbs), "single 形态单体必须存在").toBe(true);
    const body = fs.readFileSync(patchAbs);
    expect(body.byteLength).toBe(result.patchBytes);
    expect(sha256(body)).toBe(result.patchSha256);
    expect(body.byteLength).toBeLessThan(GITHUB_SINGLE_FILE_LIMIT);
    expect(fs.existsSync(manifestAbs), "single 形态不得残留分片清单").toBe(false);
    const leftovers = fs.readdirSync(path.dirname(patchAbs))
      .filter((e) => e.startsWith(`${path.basename(patchAbs)}.part-`));
    expect(leftovers, "single 形态不得残留分片").toEqual([]);
    return;
  }
  expect(result.patchFormat).toBe("sharded");
  expect(fs.existsSync(patchAbs), "sharded 形态单体不得存在").toBe(false);
  expect(result.patchManifest).toBe(`${result.patch}.shards.json`);
  expect(fs.existsSync(manifestAbs), "分片清单必须存在").toBe(true);
  const manifest = JSON.parse(fs.readFileSync(manifestAbs, "utf8")) as PatchShardsManifestShape;
  expect(manifest.format).toBe("patch-gzip-shards/v1");
  expect(manifest.patch).toBe(result.patch);
  expect(manifest.patchBytes).toBe(result.patchBytes);
  expect(manifest.patchSha256).toBe(result.patchSha256);
  expect(manifest.shardBytes).toBe(result.patchShardBytes);
  expect(manifest.shards.length).toBeGreaterThan(1);
  expect(manifest.shards.map((s) => s.path))
    .toEqual((result.patchShards ?? []).map((s) => s.path));
  const parts: Buffer[] = [];
  for (const shard of manifest.shards) {
    const body = fs.readFileSync(resolveContained(rootAbs, shard.path));
    expect(body.byteLength, `${shard.path} 尺寸不符`).toBe(shard.bytes);
    expect(sha256(body), `${shard.path} 哈希不符`).toBe(shard.sha256);
    expect(shard.bytes).toBeLessThanOrEqual(result.patchShardBytes);
    expect(shard.bytes, `${shard.path} 超 GitHub 单文件硬限`).toBeLessThan(GITHUB_SINGLE_FILE_LIMIT);
    parts.push(body);
  }
  const whole = Buffer.concat(parts);
  expect(whole.byteLength).toBe(result.patchBytes);
  expect(sha256(whole), "按序重组必须复算出完整载荷哈希").toBe(result.patchSha256);
  const raw = zlib.gunzipSync(whole);
  expect(raw.byteLength).toBe(result.patchUncompressedBytes);
  // 只校验补丁头部（git diff 输出必然以 diff --git 开头）：整段 toString 会让
  // 数百 MB 级补丁撞上 V8 字符串上限。
  expect(raw.subarray(0, 65536).toString("utf8")).toContain("diff --git");
}

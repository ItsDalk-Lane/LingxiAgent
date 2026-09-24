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

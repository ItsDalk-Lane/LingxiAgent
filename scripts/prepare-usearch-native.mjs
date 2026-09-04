import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** 上游双架构参数只作用于主扩展，没有作用于计算库；让两者都服从同一目标架构。 */
export function singleArchitectureUseArchGyp(source) {
  for (const key of ["OTHER_CFLAGS", "OTHER_LDFLAGS"]) {
    const line = `"${key}": ["-arch arm64", "-arch x86_64"],`;
    if (!source.includes(line)) throw new Error(`usearch 2.26.0 build contract changed: ${key}`);
    source = source.replace(line, "");
  }
  return source;
}

function nodeGypPath() {
  try { return require.resolve("node-gyp/bin/node-gyp.js"); }
  catch {
    if (process.env.npm_execpath) {
      const bundled = path.resolve(path.dirname(process.env.npm_execpath), "../node_modules/node-gyp/bin/node-gyp.js");
      if (fs.existsSync(bundled)) return bundled;
    }
    throw new Error("usearch native repair requires npm's node-gyp");
  }
}

export function repairUseArchNative({ rootDir = ROOT, platform = process.platform, arch = process.arch,
  nodeExecutable = process.execPath, required = false, log = console.log } = {}) {
  if (platform !== "darwin" || arch !== "x64") return { status: "not-needed" };
  const packageRoot = path.join(rootDir, "node_modules/usearch");
  const native = path.join(packageRoot, "prebuilds/darwin-arm64+x64/usearch.node");
  let temporary;
  try {
    if (!fs.existsSync(native)) throw new Error("usearch native extension unavailable");
    // 检查工具或编译器不可用时，也不能把已知有风险的扩展继续交给运行时。
    fs.renameSync(native, `${native}.unlinked`);
    const pkg = JSON.parse(fs.readFileSync(path.join(packageRoot, "package.json"), "utf8"));
    if (pkg.version !== "2.26.0") throw new Error("usearch native repair requires locked version 2.26.0");
    const unresolved = file => execFileSync("nm", ["-arch", "x86_64", "-u", file], { encoding: "utf8" });
    if (!/^_nk_/m.test(unresolved(`${native}.unlinked`))) {
      fs.renameSync(`${native}.unlinked`, native);
      return { status: "ready" };
    }
    log("[usearch] repairing missing x64 NumKong linkage using 2.26.0 sources and consistent target architecture");
    temporary = fs.mkdtempSync(path.join(path.dirname(packageRoot), ".lingxi-usearch-build-"));
    const source = path.join(temporary, "usearch");
    fs.cpSync(packageRoot, source, { recursive: true });
    const gyp = path.join(source, "binding.gyp");
    fs.writeFileSync(gyp, singleArchitectureUseArchGyp(fs.readFileSync(gyp, "utf8")));
    const version = execFileSync(nodeExecutable, ["-p", "process.versions.node"], { encoding: "utf8" }).trim();
    const nodeRoot = path.resolve(path.dirname(nodeExecutable), "..");
    const headers = fs.existsSync(path.join(nodeRoot, "include/node/node.h")) ? ["--nodedir", nodeRoot] : [];
    execFileSync(nodeExecutable, [nodeGypPath(), "rebuild", "--arch=x64", `--target=${version}`, "--jobs=2",
      "--directory", source, ...headers], { stdio: "inherit", env: {
      ...process.env, PATH: `${path.dirname(nodeExecutable)}${path.delimiter}${process.env.PATH ?? ""}`,
    } });
    const rebuilt = path.join(source, "build/Release/usearch.node");
    if (/^_nk_/m.test(unresolved(rebuilt))) throw new Error("rebuilt usearch still has unresolved NumKong symbols");
    const arm = path.join(temporary, "arm.node"), universal = path.join(temporary, "usearch.node");
    execFileSync("lipo", ["-thin", "arm64", `${native}.unlinked`, "-output", arm]);
    execFileSync("lipo", ["-create", arm, rebuilt, "-output", universal]);
    execFileSync("codesign", ["--force", "--sign", "-", universal], { stdio: "pipe" });
    fs.renameSync(universal, native);
    fs.rmSync(`${native}.unlinked`);
    log("[usearch] native x64 linkage repaired; arm64 slice preserved");
    return { status: "repaired" };
  } catch (error) {
    if (required) throw error;
    log(`[usearch] native preparation unavailable; portable fallback: ${error.message}`);
    return { status: "unavailable" };
  } finally {
    if (temporary) fs.rmSync(temporary, { recursive: true, force: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) repairUseArchNative();

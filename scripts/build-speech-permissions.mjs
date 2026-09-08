/**
 * build-speech-permissions.mjs — 编译宿主 Speech 授权原生桥（Node-API .node）。
 *
 * 产物：dist-speech/mac-<arch>/lingxi-speech-permissions.node，与 speech helper
 * 同一目录，经 electron-builder extraResources 进包（Resources/speech/macos/），
 * 由 Electron 主进程 desktop/speech-permissions.cjs 惰性 require。
 *
 * 硬合同：.node 必须以 Electron 头文件构建（node-gyp --target=<electron 版本>
 * --dist-url=electron headers 镜像），不得以本机 Node 头冒充；非 macOS 跳过。
 *
 * run/rootDir/electronVersion/buildDir 可注入：构建合同的自动化测试用替身驱动，
 * 不触碰真实 node-gyp 与网络。
 */

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const moduleRootDir = path.resolve(__dirname, "..");
const require = createRequire(import.meta.url);

// 与 desktop/speech-permissions.cjs 的同名常量保持一致。
export const SPEECH_PERMISSIONS_ARTIFACT_NAME = "lingxi-speech-permissions.node";

const BRIDGE_PACKAGE_DIR = path.join("desktop", "native", "LingxiSpeechPermissions");
const ELECTRON_HEADERS_DIST_URL = "https://electronjs.org/headers";

function defaultElectronVersion(rootDir) {
  const pkg = JSON.parse(fs.readFileSync(path.join(rootDir, "package.json"), "utf-8"));
  const declared = pkg?.devDependencies?.electron || pkg?.dependencies?.electron || "";
  const version = String(declared).replace(/^[^\d]*/, "");
  if (!version) throw new Error("[speech-permissions] electron version not declared in package.json");
  return version;
}

function defaultNodeGypEntry() {
  // node-gyp 是显式 devDependency；仅取其入口脚本路径，用当前 node 执行。
  return require.resolve("node-gyp/bin/node-gyp.js");
}

function defaultRun(cmd, args, { cwd, env }) {
  execFileSync(cmd, args, { cwd, env, stdio: "inherit" });
}

export function buildSpeechPermissionsBridge({
  platform = process.platform,
  env = process.env,
  arch = env.LINGXI_SPEECH_HELPER_ARCH || process.arch,
  rootDir = moduleRootDir,
  electronVersion = null,
  buildDir = null,
  run = defaultRun,
} = {}) {
  if (platform !== "darwin") {
    console.log(`[speech-permissions] skipped on ${platform}`);
    return { skipped: true };
  }
  if (arch !== "arm64" && arch !== "x64") {
    throw new Error(`[speech-permissions] unsupported arch: ${arch}`);
  }
  const effectiveElectronVersion = electronVersion || defaultElectronVersion(rootDir);

  const bridgeDir = path.join(rootDir, BRIDGE_PACKAGE_DIR);
  const effectiveBuildDir = buildDir || path.join(bridgeDir, "build");
  const gypArgs = [
    "rebuild",
    `--target=${effectiveElectronVersion}`,
    `--dist-url=${ELECTRON_HEADERS_DIST_URL}`,
    `--arch=${arch}`,
  ];

  console.log(`[speech-permissions] building ${SPEECH_PERMISSIONS_ARTIFACT_NAME} for ${arch} against electron ${effectiveElectronVersion}`);
  run(process.execPath, [defaultNodeGypEntry(), ...gypArgs], { cwd: bridgeDir, env });

  const source = path.join(effectiveBuildDir, "Release", SPEECH_PERMISSIONS_ARTIFACT_NAME);
  if (!fs.existsSync(source)) {
    throw new Error(`[speech-permissions] build did not produce ${source}`);
  }

  const outDir = path.join(rootDir, "dist-speech", `mac-${arch}`);
  fs.mkdirSync(outDir, { recursive: true });
  const target = path.join(outDir, SPEECH_PERMISSIONS_ARTIFACT_NAME);
  fs.copyFileSync(source, target);
  fs.chmodSync(target, 0o755);
  console.log(`[speech-permissions] copied ${target}`);
  return { skipped: false, target };
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  try {
    buildSpeechPermissionsBridge({ arch: process.env.LINGXI_SPEECH_HELPER_ARCH || process.arch });
  } catch (err) {
    console.error(err?.stack || err?.message || String(err));
    process.exitCode = 1;
  }
}

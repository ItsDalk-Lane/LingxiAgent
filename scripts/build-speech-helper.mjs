/**
 * build-speech-helper.mjs — 编译 macOS 系统语音识别（SFSpeechRecognizer）helper。
 *
 * 与 computer-use helper 同一产物形态：dist-speech/mac-<arch>/lingxi-speech-helper，
 * 经 electron-builder extraResources 进包（Resources/speech/macos/）。
 * 无第三方依赖，`swift build` 离线可编译；非 macOS 跳过。
 *
 * run/read/rootDir 可注入：构建合同的自动化测试用替身驱动，不触碰真实 swift。
 */

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const moduleRootDir = path.resolve(__dirname, "..");

export function swiftArchForNodeArch(arch = process.arch) {
  if (arch === "arm64") return "arm64";
  if (arch === "x64") return "x86_64";
  throw new Error(`[speech-helper] unsupported arch: ${arch}`);
}

function defaultRun(cmd, args, { cwd, env }) {
  execFileSync(cmd, args, { cwd, env, stdio: "inherit" });
}

function defaultRead(cmd, args, { cwd, env }) {
  return execFileSync(cmd, args, { cwd, env, encoding: "utf-8" }).trim();
}

export function buildSpeechHelper({
  platform = process.platform,
  env = process.env,
  arch = env.LINGXI_SPEECH_HELPER_ARCH || process.arch,
  rootDir = moduleRootDir,
  run = defaultRun,
  read = defaultRead,
} = {}) {
  if (platform !== "darwin") {
    console.log(`[speech-helper] skipped on ${platform}`);
    return { skipped: true };
  }

  const packageDir = path.join(rootDir, "desktop", "native", "LingxiSpeechHelper");
  const swiftArch = swiftArchForNodeArch(arch);
  const scratchPath = path.join(rootDir, ".cache", "speech-helper", "swift-build", `mac-${arch}`);
  const baseArgs = [
    "--package-path", packageDir,
    "--scratch-path", scratchPath,
    "-c", "release",
    "--arch", swiftArch,
    "--product", "lingxi-speech-helper",
  ];

  console.log(`[speech-helper] building for ${swiftArch}`);
  run("swift", ["build", ...baseArgs], { cwd: rootDir, env });

  const binPath = read("swift", ["build", "--show-bin-path", ...baseArgs], { cwd: rootDir, env });
  const source = path.join(binPath, "lingxi-speech-helper");
  if (!fs.existsSync(source)) {
    throw new Error(`[speech-helper] build did not produce ${source}`);
  }

  const outDir = path.join(rootDir, "dist-speech", `mac-${arch}`);
  fs.rmSync(outDir, { recursive: true, force: true });
  fs.mkdirSync(outDir, { recursive: true });
  const target = path.join(outDir, "lingxi-speech-helper");
  fs.copyFileSync(source, target);
  fs.chmodSync(target, 0o755);
  console.log(`[speech-helper] copied ${target}`);
  return { skipped: false, target };
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  try {
    buildSpeechHelper({ arch: process.env.LINGXI_SPEECH_HELPER_ARCH || process.arch });
  } catch (err) {
    console.error(err?.stack || err?.message || String(err));
    process.exitCode = 1;
  }
}

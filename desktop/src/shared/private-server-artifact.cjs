"use strict";

const path = require("path");
const { execFileSync } = require("child_process");
const { resolveRustBinary } = require("./rust-local-service.cjs");

// 只有桌面安装壳可建立此守卫；激活目录内的 CLI 不可充当自身信任根。
function createPrivateServerArtifactGuard({
  resourcesPath,
  appVersion,
  platform = process.platform,
  electron = Boolean(process.versions.electron),
  selectBinary = resolveRustBinary,
  run = execFileSync,
} = {}) {
  if (platform !== "win32" || !electron) return null;
  if (!resourcesPath || !path.isAbsolute(resourcesPath)) {
    throw new Error("Windows private artifact guard requires packaged resources");
  }
  if (typeof appVersion !== "string" || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(appVersion)) {
    throw new Error("Windows private artifact guard requires the installed app version");
  }
  return async (action, homeDir, targetDir = null) => {
    const command = {
      prepare: "--prepare-private-artifacts",
      seal: "--seal-private-artifact-tree",
      verify: "--verify-private-artifact-tree",
    }[action];
    if (!command || !homeDir || !path.isAbsolute(homeDir)
        || (action !== "prepare" && (!targetDir || !path.isAbsolute(targetDir)))) {
      throw new Error("invalid Windows private artifact guard request");
    }
    // 每次调用重新核随安装壳签名的固定程序，不能接受环境变量或激活目录里的同名文件。
    const binary = selectBinary({ packaged: true, resourcesPath, appVersion });
    const args = targetDir === null ? [command, homeDir] : [command, homeDir, targetDir];
    try {
      run(binary, args, {
        windowsHide: true,
        timeout: 300_000,
        maxBuffer: 4096,
        stdio: ["ignore", "pipe", "pipe"],
      });
    } catch {
      throw new Error(`Windows private server artifact ${action} failed; refusing to trust the extracted server`);
    }
  };
}

module.exports = { createPrivateServerArtifactGuard };

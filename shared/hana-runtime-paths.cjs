const fs = require("fs");
const os = require("os");
const path = require("path");

// 品牌改名（Hanako → Lingxi）前，默认数据目录是 ~/.hanako。
// 用户未显式设置 LINGXI_HOME 时，若 ~/.lingxi 不存在而 ~/.hanako 存在，
// 整体搬迁，避免老用户升级后配置/会话/记忆被“丢弃”。返回搬迁后的路径，未搬迁返回 null。
function migrateLegacyHanakoHome(homeDir = os.homedir()) {
  const legacyHome = path.join(homeDir, ".hanako");
  const lingxiHome = path.join(homeDir, ".lingxi");
  try {
    if (!fs.existsSync(legacyHome) || fs.existsSync(lingxiHome)) return null;
    fs.renameSync(legacyHome, lingxiHome);
    return lingxiHome;
  } catch {
    return null;
  }
}

function expandHome(input, homeDir = os.homedir()) {
  if (!input) return input;
  if (input === "~") return homeDir;
  if (input.startsWith("~/") || input.startsWith("~" + path.sep)) {
    return path.join(homeDir, input.slice(2));
  }
  return input;
}

function resolveLingxiHome(input, homeDir = os.homedir()) {
  const raw = input || path.join(homeDir, ".lingxi");
  return path.resolve(expandHome(raw, homeDir));
}

function assertLingxiHome(lingxiHome, caller) {
  if (!lingxiHome || typeof lingxiHome !== "string") {
    throw new Error(`${caller}: lingxiHome is required`);
  }
}

function resolveLingxiPiSdkRuntimeRoot(lingxiHome) {
  assertLingxiHome(lingxiHome, "resolveLingxiPiSdkRuntimeRoot");
  return path.join(lingxiHome, "runtime", "pi-sdk");
}

function resolveLingxiPiSdkManagedBinDir(lingxiHome) {
  return path.join(resolveLingxiPiSdkRuntimeRoot(lingxiHome), "bin");
}

function resolveLingxiPiSdkResourceLoaderCwd(lingxiHome) {
  return path.join(resolveLingxiPiSdkRuntimeRoot(lingxiHome), "resource-loader", "project");
}

function resolveLingxiPiSdkResourceLoaderAgentDir(lingxiHome) {
  return path.join(resolveLingxiPiSdkRuntimeRoot(lingxiHome), "resource-loader", "agent");
}

function resolveLegacyPiSdkManagedBinDir(lingxiHome) {
  assertLingxiHome(lingxiHome, "resolveLegacyPiSdkManagedBinDir");
  return path.join(lingxiHome, ".pi", "agent", "bin");
}

module.exports = {
  resolveLingxiHome,
  migrateLegacyHanakoHome,
  resolveLingxiPiSdkManagedBinDir,
  resolveLingxiPiSdkResourceLoaderAgentDir,
  resolveLingxiPiSdkResourceLoaderCwd,
  resolveLingxiPiSdkRuntimeRoot,
  resolveLegacyPiSdkManagedBinDir,
};

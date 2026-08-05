const os = require("os");
const path = require("path");

function expandHome(input, homeDir = os.homedir()) {
  if (!input) return input;
  if (input === "~") return homeDir;
  if (input.startsWith("~/") || input.startsWith("~" + path.sep)) {
    return path.join(homeDir, input.slice(2));
  }
  return input;
}

function resolveHanakoHome(input, homeDir = os.homedir()) {
  const raw = input || path.join(homeDir, ".lingxi");
  return path.resolve(expandHome(raw, homeDir));
}

function assertHanakoHome(lingxiHome, caller) {
  if (!lingxiHome || typeof lingxiHome !== "string") {
    throw new Error(`${caller}: lingxiHome is required`);
  }
}

function resolveLingxiPiSdkRuntimeRoot(lingxiHome) {
  assertHanakoHome(lingxiHome, "resolveLingxiPiSdkRuntimeRoot");
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
  assertHanakoHome(lingxiHome, "resolveLegacyPiSdkManagedBinDir");
  return path.join(lingxiHome, ".pi", "agent", "bin");
}

module.exports = {
  resolveHanakoHome,
  resolveLingxiPiSdkManagedBinDir,
  resolveLingxiPiSdkResourceLoaderAgentDir,
  resolveLingxiPiSdkResourceLoaderCwd,
  resolveLingxiPiSdkRuntimeRoot,
  resolveLegacyPiSdkManagedBinDir,
};

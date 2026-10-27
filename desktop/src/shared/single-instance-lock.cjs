/**
 * Electron client single-instance guard.
 *
 * Electron's requestSingleInstanceLock() is scoped by userData, so LingxiAgent sets
 * userData from LINGXI_HOME before requesting the lock. Production and dev homes
 * get different namespaces, while duplicate launches within the same home are
 * redirected to the first client.
 */
const path = require("path");
const fs = require("fs");

function canonicalHome(home) {
  let existing = path.resolve(home);
  const missing = [];
  while (!fs.existsSync(existing)) {
    missing.unshift(path.basename(existing));
    const parent = path.dirname(existing);
    if (parent === existing) throw new Error("无法确认桌面数据目录的真实位置");
    existing = parent;
  }
  return path.join(fs.realpathSync(existing), ...missing);
}

function getClientUserDataPath(lingxiHome, defaultHome, appData) {
  const resolved = path.resolve(lingxiHome);
  const parent = path.dirname(path.resolve(defaultHome));
  const standardHomes = [defaultHome, ...[".lingxi", ".lingxi-dev", ".hanako-dev"].map(name => path.join(parent, name))];
  const actualHome = canonicalHome(resolved);
  const standardHome = standardHomes.find(home => normalizeForCompare(canonicalHome(home)) === normalizeForCompare(actualHome));
  // 默认安装和默认开发目录延续原缓存位置；显式副本的缓存也留在副本内。
  if (standardHome) {
    return path.join(appData, getUserDataAppName(standardHome, defaultHome));
  }
  return path.join(actualHome, ".electron-user-data");
}

function normalizeForCompare(filePath) {
  const resolved = path.resolve(filePath);
  return process.platform === "win32" ? resolved.toLowerCase() : resolved;
}

function getUserDataAppName(lingxiHome, defaultHome) {
  if (normalizeForCompare(lingxiHome) === normalizeForCompare(defaultHome)) {
    return "Hanako";
  }
  const suffix = path.basename(lingxiHome).replace(/^\./, "");
  if (!suffix) return "Hanako";
  return suffix.charAt(0).toUpperCase() + suffix.slice(1);
}

function exitDuplicateClient(app) {
  if (typeof app.exit === "function") {
    app.exit(0);
    return;
  }
  app.quit();
}

function focusExistingWindow(win) {
  if (!win || win.isDestroyed?.()) return false;
  if (win.isMinimized?.()) win.restore?.();
  win.show?.();
  win.focus?.();
  return true;
}

function configureClientSingleInstance(app, opts) {
  const { lingxiHome, defaultHome, onSecondInstance } = opts;
  app.setPath("userData", getClientUserDataPath(lingxiHome, defaultHome, app.getPath("appData")));

  const gotLock = app.requestSingleInstanceLock({ lingxiHome });
  if (!gotLock) {
    exitDuplicateClient(app);
    return false;
  }

  app.on("second-instance", () => {
    onSecondInstance?.();
  });
  return true;
}

module.exports = {
  configureClientSingleInstance,
  focusExistingWindow,
  getUserDataAppName,
  getClientUserDataPath,
};

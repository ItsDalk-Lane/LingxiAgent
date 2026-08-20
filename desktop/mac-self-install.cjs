/**
 * mac-self-install.cjs — ad-hoc 签名 mac 构建的自安装路径
 *
 * 为什么存在（根因）：
 *   electron-updater 在 mac 上的安装步由 Electron 内置 Squirrel.Mac (ShipIt) 执行，
 *   其安全不变量是「新 bundle 必须满足当前运行 bundle 的 designated requirement」。
 *   无 Apple 证书时 CI 走 ad-hoc 全量重签（scripts/resign-adhoc.cjs），ad-hoc 签名的
 *   DR 是 `identifier ... and cdhash H"..."`——cdhash 随每次构建变化，因此任意两版
 *   之间 ShipIt 校验必然失败（errSecCSReqFailed，「代码未能满足指定的代码要求」）。
 *   这不是网络问题，重试无解；ad-hoc 构建的 mac 更新在 Squirrel 路径上结构性不可用。
 *
 * 本模块的做法：electron-updater 已完成下载与 sha512 校验（zip 落在
 * ~/Library/Caches/<updaterCacheDirName>/pending/），信任边界与 Squirrel 一致。
 * 我们生成一个 detached shell 脚本，在本进程退出后完成
 * 「等退出 → 杀残留 server → 同卷解压 → 校验 bundle 身份/版本 → 原子换壳 →
 * 失败回滚 → 重启」。前置条件（可写性、zip 存在、未被 AppTranslocation）在退出前
 * 于本进程内检查完，确定性失败立刻以错误码返回，由 UI 落到「手动下载」，
 * 不做静默降级。
 *
 * 自退役：CI 配上 Developer ID 证书后（resign-adhoc.cjs 会自动跳过），
 * detectMacInstallMode() 识别到 TeamIdentifier 即回到 Squirrel 路径，
 * 本模块不再被触发，无需改代码。
 */
const fs = require("fs");
const os = require("os");
const path = require("path");
const { execFileSync, spawn, spawnSync } = require("child_process");

const PLIST_BUDDY = "/usr/libexec/PlistBuddy";

/** shell 单引号转义：路径里带空格/引号都不会断句。 */
function shq(value) {
  return `'${String(value).replace(/'/g, `'\\''`)}'`;
}

/**
 * 从 exe 路径反推 bundle 路径：<X.app>/Contents/MacOS/<bin> → <X.app>。
 * 形状不符返回 null（调用方保持原 Squirrel 路径，不贸然自安装）。
 */
function bundlePathFromExePath(exePath) {
  if (!exePath) return null;
  const macDir = path.dirname(exePath);
  const contentsDir = path.dirname(macDir);
  const bundleDir = path.dirname(contentsDir);
  if (path.basename(macDir) !== "MacOS") return null;
  if (path.basename(contentsDir) !== "Contents") return null;
  if (!bundleDir.endsWith(".app")) return null;
  return bundleDir;
}

/**
 * 判定 mac 安装路径该走 Squirrel 还是自安装。
 * 依据：codesign -dv 输出里的 TeamIdentifier。ad-hoc/未签名没有 Team ID，
 * Squirrel 的 DR 校验对它们结构性失败；有 Team ID（Developer ID）才走 Squirrel。
 * spawnSyncImpl 可注入以便测试。
 */
function detectMacInstallMode({ bundlePath, spawnSyncImpl = spawnSync } = {}) {
  if (!bundlePath) return { mode: "squirrel", reason: "bundle-path-unresolved" };
  let result;
  try {
    result = spawnSyncImpl("codesign", ["-dv", bundlePath], { encoding: "utf-8" });
  } catch (err) {
    // codesign 不存在/无法执行：按 ad-hoc 处理（Squirrel 对未签名包同样必败）
    return { mode: "self-install", reason: `codesign unavailable: ${err?.message || String(err)}` };
  }
  // codesign -dv 把详情写到 stderr；未签名时退出码非零但 stderr 也有信息
  const output = `${result?.stdout || ""}\n${result?.stderr || ""}`;
  const match = output.match(/^TeamIdentifier=(.+)$/m);
  const teamId = match ? match[1].trim() : "";
  if (teamId && teamId !== "not set") return { mode: "squirrel", teamId };
  return { mode: "self-install", reason: teamId === "not set" ? "adhoc-no-team-id" : "unsigned" };
}

function readInfoPlistValue(bundlePath, key, execImpl = execFileSync) {
  const plist = path.join(bundlePath, "Contents", "Info.plist");
  return String(execImpl(PLIST_BUDDY, ["-c", `Print :${key}`, plist], { encoding: "utf-8" })).trim();
}

/**
 * 解析已下载的更新 zip：优先用 update-downloaded 事件带的 downloadedFile；
 * 否则按 electron-updater 的约定读 pending/update-info.json 里的 fileName。
 */
function resolveDownloadedZip({ explicitPath, pendingDir, fsImpl = fs } = {}) {
  if (explicitPath && typeof explicitPath === "string") {
    try {
      if (fsImpl.existsSync(explicitPath)) return explicitPath;
    } catch {}
  }
  if (!pendingDir) return null;
  try {
    const infoPath = path.join(pendingDir, "update-info.json");
    const info = JSON.parse(fsImpl.readFileSync(infoPath, "utf-8"));
    if (info && typeof info.fileName === "string" && info.fileName.endsWith(".zip")) {
      const candidate = path.join(pendingDir, info.fileName);
      if (fsImpl.existsSync(candidate)) return candidate;
    }
  } catch {}
  return null;
}

/**
 * electron-updater 的 pending 目录：~/Library/Caches/<updaterCacheDirName>/pending。
 * updaterCacheDirName 由 electron-builder 写进 app-update.yml；缺失时退回
 * <app.getName()>-updater（与 AppUpdater.js 的 dirName || app.name 逻辑同序）。
 */
function defaultPendingDir({ appUpdateYmlPath, appName, homeDir = os.homedir(), fsImpl = fs } = {}) {
  let dirName = null;
  if (appUpdateYmlPath) {
    try {
      const yml = fsImpl.readFileSync(appUpdateYmlPath, "utf-8");
      const match = yml.match(/^updaterCacheDirName:\s*(\S+)\s*$/m);
      if (match) dirName = match[1];
    } catch {}
  }
  if (!dirName) dirName = `${appName}-updater`;
  return path.join(homeDir, "Library", "Caches", dirName, "pending");
}

/**
 * 生成自安装脚本（纯函数，便于测试）。脚本职责：本进程退出后在原地完成换壳。
 * 只依赖 macOS 系统自带工具：ditto / PlistBuddy / pkill / open / sed / mv。
 */
function buildSelfInstallScript({
  appBundlePath,
  zipPath,
  expectedBundleId,
  expectedVersion,
  mainPid,
  serverInfoPath,
  lingxiHome,
  logPath,
}) {
  const parentDir = path.dirname(appBundlePath);
  const bundleName = path.basename(appBundlePath);
  return `#!/bin/sh
# Lingxi mac self-install — 由 mac-self-install.cjs 生成。ad-hoc 签名构建走不了
# Squirrel.Mac（DR 含 cdhash，跨版本必败），用这个脚本在旧进程退出后原地换壳。
set -u

PID=${mainPid}
APP=${shq(appBundlePath)}
ZIP=${shq(zipPath)}
EXPECTED_ID=${shq(expectedBundleId)}
EXPECTED_VERSION=${shq(expectedVersion || "")}
SERVER_INFO=${shq(serverInfoPath)}
HOME_MARKER=${shq(lingxiHome)}
LOG=${shq(logPath)}
STAGING_DIR=""
BACKUP=${shq(path.join(parentDir, `.${bundleName.replace(/\.app$/, "")}.pre-update`))}.$$

log() { printf '%s [mac-self-install] %s\\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$1" >> "$LOG" 2>/dev/null || true; }
die() { log "FAILED: $1"; if [ -n "$STAGING_DIR" ]; then rm -rf "$STAGING_DIR"; fi; exit 1; }

log "armed: pid=$PID app=$APP zip=$ZIP expect=$EXPECTED_VERSION"

# 1) 等旧主进程退出（有界 60s；PID 被复用也只是多等，不会误杀——后面不动这个 PID）
n=0
while kill -0 "$PID" 2>/dev/null; do
  n=$((n + 1))
  if [ "$n" -ge 300 ]; then die "old process $PID did not exit in time"; fi
  sleep 0.2
done

# 2) 停掉残留 server。server 跑在 LINGXI_HOME 下的版本化目录（不在 .app 内），
#    pkill 壳路径打不到；从 server-info.json 取 pid，杀前用 ps args 复核它确实
#    带着我们的 LINGXI_HOME 标记，防 PID 复用误杀。
if [ -f "$SERVER_INFO" ]; then
  SPID=$(sed -n 's/.*"pid":\\([0-9][0-9]*\\).*/\\1/p' "$SERVER_INFO" | head -1)
  case "$SPID" in
    ''|*[!0-9]*) : ;;
    *)
      if ps -p "$SPID" -o args= 2>/dev/null | grep -F "$HOME_MARKER" >/dev/null 2>&1; then
        kill "$SPID" 2>/dev/null || true
        n=0
        while kill -0 "$SPID" 2>/dev/null; do
          n=$((n + 1))
          if [ "$n" -ge 25 ]; then kill -9 "$SPID" 2>/dev/null || true; break; fi
          sleep 0.2
        done
        log "server pid $SPID stopped"
      else
        log "server-info pid $SPID not ours, left alone"
      fi
      ;;
  esac
fi
# 壳内残留 helper（crashpad 等）兜底
pkill -f "$APP/" 2>/dev/null || true
sleep 1

# 3) 同卷解压：staging 放在 app 同级目录，保证后面的 mv 是原子 rename
STAGING_DIR=$(mktemp -d ${shq(path.join(parentDir, ".lingxi-self-install"))}.XXXXXX) || die "mktemp failed"
ditto -x -k "$ZIP" "$STAGING_DIR" || die "ditto extract failed"
NEWAPP="$STAGING_DIR/$(basename "$APP")"
if [ ! -d "$NEWAPP" ]; then
  NEWAPP=$(find "$STAGING_DIR" -maxdepth 1 -name "*.app" | head -1)
fi
if [ -z "$NEWAPP" ] || [ ! -d "$NEWAPP" ]; then die "no .app found in archive"; fi

# 4) 身份校验：bundle id 必须与运行中的壳一致，版本必须与下载记录一致。
#    绝不把对不上的包换上去。
NEW_ID=$(${PLIST_BUDDY} -c "Print :CFBundleIdentifier" "$NEWAPP/Contents/Info.plist" 2>/dev/null) || die "new bundle plist unreadable"
if [ "$NEW_ID" != "$EXPECTED_ID" ]; then die "bundle id mismatch: $NEW_ID != $EXPECTED_ID"; fi
NEW_VER=$(${PLIST_BUDDY} -c "Print :CFBundleShortVersionString" "$NEWAPP/Contents/Info.plist" 2>/dev/null || true)
if [ -n "$EXPECTED_VERSION" ] && [ "$NEW_VER" != "$EXPECTED_VERSION" ]; then
  die "version mismatch: $NEW_VER != $EXPECTED_VERSION"
fi

# 5) 清隔离/来源标记后原子换壳；新壳放不进去就把旧壳放回去
xattr -cr "$NEWAPP" 2>/dev/null || true
mv "$APP" "$BACKUP" || die "cannot move old bundle aside"
if ! mv "$NEWAPP" "$APP"; then
  mv "$BACKUP" "$APP"
  die "cannot move new bundle into place; rolled back"
fi

# 6) 重启；起不来就回滚到旧壳并拉起旧壳
if open "$APP"; then
  rm -rf "$BACKUP" "$STAGING_DIR"
  rm -f "$ZIP"
  log "OK: updated to $NEW_VER"
  exit 0
fi
rm -rf "$APP"
mv "$BACKUP" "$APP"
rm -rf "$STAGING_DIR"
STAGING_DIR=""
open "$APP" 2>/dev/null || true
die "open new bundle failed; rolled back to previous version"
`;
}

/**
 * 武装一次自安装：检查前置条件 → 写脚本 → detached 拉起 → 返回。
 * 调用方拿到 { ok: true } 后应立即退出本进程，把舞台让给脚本。
 * 所有确定性失败在退出前返回 { ok: false, error }，绝不静默。
 */
function armSelfInstall({
  bundlePath,
  zipPath,
  expectedVersion,
  mainPid,
  serverInfoPath,
  lingxiHome,
  logPath,
  execImpl = execFileSync,
  spawnImpl = spawn,
  fsImpl = fs,
  osImpl = os,
} = {}) {
  if (!bundlePath) return { ok: false, error: "mac_bundle_path_unresolved" };
  if (bundlePath.includes("/AppTranslocation/")) {
    return { ok: false, error: "mac_app_translocated" };
  }
  if (!zipPath || !fsImpl.existsSync(zipPath)) {
    return { ok: false, error: "mac_update_zip_missing" };
  }

  let expectedBundleId;
  try {
    expectedBundleId = readInfoPlistValue(bundlePath, "CFBundleIdentifier", execImpl);
  } catch (err) {
    return { ok: false, error: `mac_bundle_id_unreadable: ${err?.message || String(err)}` };
  }
  if (!expectedBundleId) return { ok: false, error: "mac_bundle_id_unreadable: empty" };

  // 换壳 = 同级目录里 rename，需要 bundle 本身和父目录都可写
  try {
    fsImpl.accessSync(bundlePath, fsImpl.constants.W_OK);
    fsImpl.accessSync(path.dirname(bundlePath), fsImpl.constants.W_OK);
  } catch {
    return { ok: false, error: "mac_app_not_writable" };
  }

  const script = buildSelfInstallScript({
    appBundlePath: bundlePath,
    zipPath,
    expectedBundleId,
    expectedVersion,
    mainPid,
    serverInfoPath,
    lingxiHome,
    logPath,
  });

  let scriptPath;
  try {
    const dir = fsImpl.mkdtempSync(path.join(osImpl.tmpdir(), "lingxi-self-install-"));
    scriptPath = path.join(dir, "install.sh");
    fsImpl.writeFileSync(scriptPath, script, { mode: 0o755 });
  } catch (err) {
    return { ok: false, error: `mac_script_write_failed: ${err?.message || String(err)}` };
  }

  try {
    const child = spawnImpl("/bin/sh", [scriptPath], { detached: true, stdio: "ignore" });
    child.unref();
  } catch (err) {
    return { ok: false, error: `mac_script_spawn_failed: ${err?.message || String(err)}` };
  }
  return { ok: true, scriptPath };
}

module.exports = {
  bundlePathFromExePath,
  detectMacInstallMode,
  resolveDownloadedZip,
  defaultPendingDir,
  buildSelfInstallScript,
  armSelfInstall,
  readInfoPlistValue,
};

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync, spawnSync } from "node:child_process";
import { describe, it, expect, beforeEach, afterEach } from "vitest";

const mod = await import("../desktop/mac-self-install.cjs");
const {
  bundlePathFromExePath,
  detectMacInstallMode,
  resolveDownloadedZip,
  defaultPendingDir,
  buildSelfInstallScript,
  armSelfInstall,
} = mod as any;

const PLIST_BUDDY = "/usr/libexec/PlistBuddy";
const isMac = process.platform === "darwin";

let tmpDirs: string[] = [];

function makeTmpDir(prefix = "mac-self-install-") {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  tmpDirs.push(dir);
  return dir;
}

afterEach(() => {
  for (const dir of tmpDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

/** 用 PlistBuddy 造一个带真实 plist 的最小假 .app。 */
function makeFakeApp(
  parentDir: string,
  { bundleId, version, name = "LingxiTest.app" }: { bundleId: string; version: string; name?: string },
) {
  const appPath = path.join(parentDir, name);
  const contents = path.join(appPath, "Contents");
  fs.mkdirSync(path.join(contents, "MacOS"), { recursive: true });
  fs.writeFileSync(path.join(contents, "MacOS", "LingxiTest"), "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  const plist = path.join(contents, "Info.plist");
  execFileSync(PLIST_BUDDY, ["-c", `Add :CFBundleIdentifier string ${bundleId}`, plist]);
  execFileSync(PLIST_BUDDY, ["-c", `Add :CFBundleShortVersionString string ${version}`, plist]);
  return appPath;
}

function readAppVersion(appPath: string) {
  return String(
    execFileSync(PLIST_BUDDY, ["-c", "Print :CFBundleShortVersionString", path.join(appPath, "Contents", "Info.plist")], { encoding: "utf-8" }),
  ).trim();
}

/** 造 open/pkill 桩，避免测试真的拉起 LaunchServices 或杀进程。 */
function makeBinStubs(dir: string, { openExit = 0 }: { openExit?: number } = {}) {
  const binDir = path.join(dir, "stub-bin");
  fs.mkdirSync(binDir, { recursive: true });
  fs.writeFileSync(
    path.join(binDir, "open"),
    `#!/bin/sh\necho "open $*" >> "${dir}/open.log"\nexit ${openExit}\n`,
    { mode: 0o755 },
  );
  fs.writeFileSync(path.join(binDir, "pkill"), "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  return binDir;
}

/** 拿一个刚死掉的 pid（脚本的有界等待会立刻通过）。 */
function deadPid(): number {
  const res = spawnSync("true", []);
  return res.pid;
}

describe("mac-self-install pure helpers", () => {
  it("resolves the bundle path from a standard exe path", () => {
    expect(bundlePathFromExePath("/Applications/Lingxi.app/Contents/MacOS/Lingxi"))
      .toBe("/Applications/Lingxi.app");
    expect(bundlePathFromExePath("/Users/x/Apps/My App.app/Contents/MacOS/My App"))
      .toBe("/Users/x/Apps/My App.app");
  });

  it("refuses non-bundle exe paths", () => {
    expect(bundlePathFromExePath("/usr/local/bin/lingxi")).toBeNull();
    expect(bundlePathFromExePath("/Applications/Lingxi.app/Contents/MacOS")).toBeNull();
    expect(bundlePathFromExePath("")).toBeNull();
  });

  it("routes ad-hoc signed builds (TeamIdentifier=not set) to self-install", () => {
    const fakeSpawn = () => ({
      status: 0,
      stdout: "",
      stderr: "Executable=/Applications/Lingxi.app/Contents/MacOS/Lingxi\nIdentifier=com.lingxi.app\nTeamIdentifier=not set\nSignature=adhoc\n",
    });
    expect(detectMacInstallMode({ bundlePath: "/x.app", spawnSyncImpl: fakeSpawn }))
      .toEqual(expect.objectContaining({ mode: "self-install" }));
  });

  it("routes Developer ID signed builds to Squirrel", () => {
    const fakeSpawn = () => ({
      status: 0,
      stdout: "",
      stderr: "Identifier=com.lingxi.app\nTeamIdentifier=ABCDE12345\nAuthority=Developer ID Application: Example\n",
    });
    expect(detectMacInstallMode({ bundlePath: "/x.app", spawnSyncImpl: fakeSpawn }))
      .toEqual({ mode: "squirrel", teamId: "ABCDE12345" });
  });

  it("treats unsigned/unreadable bundles as self-install (Squirrel fails there too)", () => {
    const fakeSpawn = () => ({ status: 1, stdout: "", stderr: "code object is not signed at all" });
    expect(detectMacInstallMode({ bundlePath: "/x.app", spawnSyncImpl: fakeSpawn }).mode).toBe("self-install");
    const throwing = () => { throw new Error("spawn codesign ENOENT"); };
    expect(detectMacInstallMode({ bundlePath: "/x.app", spawnSyncImpl: throwing }).mode).toBe("self-install");
  });

  it("resolves the downloaded zip from the explicit path first, then update-info.json", () => {
    const dir = makeTmpDir();
    const zip = path.join(dir, "Lingxi-2.0.0-macOS-arm64.zip");
    fs.writeFileSync(zip, "zip");
    expect(resolveDownloadedZip({ explicitPath: zip, pendingDir: dir })).toBe(zip);

    fs.writeFileSync(path.join(dir, "update-info.json"), JSON.stringify({ fileName: path.basename(zip), sha512: "x" }));
    expect(resolveDownloadedZip({ explicitPath: "/nonexistent.zip", pendingDir: dir })).toBe(zip);
    expect(resolveDownloadedZip({ explicitPath: null, pendingDir: dir })).toBe(zip);

    const empty = makeTmpDir();
    expect(resolveDownloadedZip({ explicitPath: null, pendingDir: empty })).toBeNull();
  });

  it("computes the pending dir from app-update.yml's updaterCacheDirName", () => {
    const dir = makeTmpDir();
    const yml = path.join(dir, "app-update.yml");
    fs.writeFileSync(yml, "owner: o\nrepo: r\nprovider: github\nupdaterCacheDirName: lingxi-updater\n");
    expect(defaultPendingDir({ appUpdateYmlPath: yml, appName: "Lingxi", homeDir: "/home/x" }))
      .toBe(path.join("/home/x", "Library", "Caches", "lingxi-updater", "pending"));
    expect(defaultPendingDir({ appUpdateYmlPath: path.join(dir, "missing.yml"), appName: "Lingxi", homeDir: "/home/x" }))
      .toBe(path.join("/home/x", "Library", "Caches", "Lingxi-updater", "pending"));
  });

  it("shell-quotes paths in the generated script", () => {
    const script = buildSelfInstallScript({
      appBundlePath: "/Applications/Ling xi.app",
      zipPath: "/tmp/it's a.zip",
      expectedBundleId: "com.lingxi.app",
      expectedVersion: "2.0.0",
      mainPid: 1234,
      serverInfoPath: "/home/x/.lingxi/server-info.json",
      lingxiHome: "/home/x/.lingxi",
      logPath: "/home/x/.lingxi/logs/auto-update.log",
    });
    expect(script).toContain(`APP='/Applications/Ling xi.app'`);
    expect(script).toContain(`ZIP='/tmp/it'\\''s a.zip'`);
    expect(script).toContain("EXPECTED_ID='com.lingxi.app'");
    expect(script).toContain("PID=1234");
  });

  it("armSelfInstall refuses deterministic failures before touching anything", () => {
    expect(armSelfInstall({ bundlePath: null })).toEqual({ ok: false, error: "mac_bundle_path_unresolved" });
    expect(armSelfInstall({ bundlePath: "/private/var/folders/x/AppTranslocation/abc/Lingxi.app" }).error)
      .toBe("mac_app_translocated");
    expect(armSelfInstall({ bundlePath: "/Applications/Lingxi.app", zipPath: "/no/such.zip" }).error)
      .toBe("mac_update_zip_missing");
  });

  it("armSelfInstall spawns the script detached when preconditions pass", () => {
    const dir = makeTmpDir();
    const zip = path.join(dir, "update.zip");
    fs.writeFileSync(zip, "zip");
    const appPath = path.join(dir, "LingxiTest.app");
    fs.mkdirSync(appPath, { recursive: true });

    const spawned: any[] = [];
    const result = armSelfInstall({
      bundlePath: appPath,
      zipPath: zip,
      expectedVersion: "2.0.0",
      mainPid: 4321,
      serverInfoPath: path.join(dir, "server-info.json"),
      lingxiHome: dir,
      logPath: path.join(dir, "log.txt"),
      execImpl: () => "com.test.lingxi",
      spawnImpl: (cmd: string, args: string[], opts: object) => {
        spawned.push({ cmd, args, opts });
        return { unref: () => {} };
      },
    });

    expect(result.ok).toBe(true);
    expect(spawned).toHaveLength(1);
    expect(spawned[0].cmd).toBe("/bin/sh");
    expect(spawned[0].opts).toEqual(expect.objectContaining({ detached: true, stdio: "ignore" }));
    const script = fs.readFileSync(result.scriptPath, "utf-8");
    expect(script).toContain("PID=4321");
    expect(script).toContain(`EXPECTED_ID='com.test.lingxi'`);
  });
});

// 功能测试：真的用 /bin/sh 跑生成的脚本，验证解压/校验/换壳/回滚语义。
describe.skipIf(!isMac)("mac-self-install script end-to-end", () => {
  let workDir: string;
  let binDir: string;
  let logPath: string;

  beforeEach(() => {
    workDir = makeTmpDir();
    logPath = path.join(workDir, "auto-update.log");
  });

  function setup({ openExit = 0, zipBundleId = "com.test.lingxi", zipVersion = "2.0.0" } = {}) {
    binDir = makeBinStubs(workDir, { openExit });
    // 旧版本已"安装"
    const installed = makeFakeApp(workDir, { bundleId: "com.test.lingxi", version: "1.0.0" });
    // 新版本打进 zip（ditto -c -k 与 electron-builder 产物同构）
    const srcDir = path.join(workDir, "src");
    fs.mkdirSync(srcDir);
    makeFakeApp(srcDir, { bundleId: zipBundleId, version: zipVersion });
    const zipPath = path.join(workDir, "update.zip");
    execFileSync("ditto", ["-c", "-k", "--keepParent", path.join(srcDir, "LingxiTest.app"), zipPath]);

    const script = buildSelfInstallScript({
      appBundlePath: installed,
      zipPath,
      expectedBundleId: "com.test.lingxi",
      expectedVersion: "2.0.0",
      mainPid: deadPid(),
      serverInfoPath: path.join(workDir, "server-info.json"),
      lingxiHome: workDir,
      logPath,
    });
    const scriptPath = path.join(workDir, "install.sh");
    fs.writeFileSync(scriptPath, script, { mode: 0o755 });
    return { installed, zipPath, scriptPath };
  }

  function runScript(scriptPath: string) {
    return spawnSync("/bin/sh", [scriptPath], {
      encoding: "utf-8",
      timeout: 60_000,
      env: { ...process.env, PATH: `${binDir}:${process.env.PATH}` },
    });
  }

  it("is syntactically valid sh", () => {
    const { scriptPath } = setup();
    const res = spawnSync("/bin/sh", ["-n", scriptPath], { encoding: "utf-8" });
    expect(res.status).toBe(0);
  });

  it("swaps the bundle, relaunches and cleans up on the happy path", () => {
    const { installed, zipPath, scriptPath } = setup();
    const res = runScript(scriptPath);

    expect(res.status).toBe(0);
    expect(readAppVersion(installed)).toBe("2.0.0");
    expect(fs.existsSync(zipPath)).toBe(false); // zip 清掉了
    expect(fs.readdirSync(workDir).filter(f => f.includes("pre-update") || f.includes("self-install"))).toEqual([]);
    expect(fs.readFileSync(`${workDir}/open.log`, "utf-8")).toContain(installed);
    expect(fs.readFileSync(logPath, "utf-8")).toContain("OK: updated to 2.0.0");
  });

  it("rolls back to the old bundle when the new one fails to launch", () => {
    const { installed, scriptPath } = setup({ openExit: 1 });
    const res = runScript(scriptPath);

    expect(res.status).not.toBe(0);
    expect(readAppVersion(installed)).toBe("1.0.0"); // 旧壳回来了
    expect(fs.readFileSync(logPath, "utf-8")).toContain("rolled back");
  });

  it("refuses a bundle with the wrong identifier and leaves the old app untouched", () => {
    const { installed, zipPath, scriptPath } = setup({ zipBundleId: "com.test.evil" });
    const res = runScript(scriptPath);

    expect(res.status).not.toBe(0);
    expect(readAppVersion(installed)).toBe("1.0.0");
    expect(fs.existsSync(zipPath)).toBe(true);
    expect(fs.readFileSync(logPath, "utf-8")).toContain("bundle id mismatch");
  });

  it("refuses a version that does not match the download record", () => {
    const { installed, scriptPath } = setup({ zipVersion: "9.9.9" });
    const res = runScript(scriptPath);

    expect(res.status).not.toBe(0);
    expect(readAppVersion(installed)).toBe("1.0.0");
    expect(fs.readFileSync(logPath, "utf-8")).toContain("version mismatch");
  });

  it("leaves a foreign server pid in server-info.json alone", () => {
    const { scriptPath } = setup();
    // 写一个 pid 指向当前 shell 测试进程——它的 args 不含 HOME_MARKER，脚本必须跳过
    fs.writeFileSync(path.join(workDir, "server-info.json"), JSON.stringify({ pid: process.pid, port: 1 }));
    const res = runScript(scriptPath);

    expect(res.status).toBe(0);
    expect(fs.readFileSync(logPath, "utf-8")).toContain("not ours, left alone");
  });
});

import { describe, expect, it } from "vitest";
import fs from "fs";
import path from "path";

const root = process.cwd();

function extractMacro(source, name) {
  const match = source.match(new RegExp(`!macro ${name}(?:\\s|$)[\\s\\S]*?!macroend`));
  return match?.[0] || "";
}

describe("Windows NSIS installer contract", () => {
  it("does not let stale old-uninstaller failures abort a Lingxi-owned overlay", () => {
    const source = fs.readFileSync(path.join(root, "build", "installer.nsh"), "utf-8");
    const macro = extractMacro(source, "customUnInstallCheck");

    expect(macro).toContain("lingxiPrepareOwnedOverlay");
    expect(macro).toContain("ClearErrors");
    expect(macro).not.toContain("$(uninstallFailed)");
    expect(macro).not.toContain("Quit");
  });

  it("bypasses the previous uninstaller in electron-updater mode", () => {
    const source = fs.readFileSync(path.join(root, "build", "installer.nsh"), "utf-8");
    const bypass = extractMacro(source, "lingxiBypassOldUninstallerForUpdate");
    const checkRunning = extractMacro(source, "customCheckAppRunning");

    expect(checkRunning).toContain("lingxiBypassOldUninstallerForUpdate");
    expect(bypass).toContain("${isUpdated}");
    expect(bypass).toContain("lingxiPrepareOwnedOverlay");
    expect(bypass).toContain('DeleteRegKey SHELL_CONTEXT "${UNINSTALL_REGISTRY_KEY}"');
  });

  it("cleans the retired scattered server tree left behind by pre-seed installs before overlaying new files", () => {
    const source = fs.readFileSync(path.join(root, "build", "installer.nsh"), "utf-8");

    expect(source).toContain('RMDir /r "$INSTDIR\\resources\\server"');
  });

  it("removes legacy unpacked Electron app directories before overlaying new files", () => {
    const source = fs.readFileSync(path.join(root, "build", "installer.nsh"), "utf-8");
    const macro = extractMacro(source, "lingxiRemoveOwnedInstallTrees");

    expect(macro).toContain('RMDir /r "$INSTDIR\\resources\\app"');
  });

  it("cleans processes by install-directory ownership, not only fixed image names", () => {
    const source = fs.readFileSync(path.join(root, "build", "installer.nsh"), "utf-8");
    const macro = extractMacro(source, "lingxiStopInstallDirProcesses");
    const cleaner = extractMacro(source, "lingxiWriteInstallDirProcessCleaner");

    expect(macro).toContain("LINGXI_INSTALL_DIR");
    expect(macro).toContain("lingxiWriteInstallDirProcessCleaner");
    expect(cleaner).toContain("Get-CimInstance Win32_Process");
    expect(cleaner).toContain("CommandLine");
    expect(cleaner).toContain("Stop-Process");
  });

  it("escapes PowerShell variables written through NSIS FileWrite", () => {
    const source = fs.readFileSync(path.join(root, "build", "installer.nsh"), "utf-8");
    const cleaner = extractMacro(source, "lingxiWriteInstallDirProcessCleaner");
    const fileWrites = cleaner
      .split("\n")
      .filter((line) => line.includes("FileWrite"))
      .join("\n");

    expect(fileWrites).toContain("$$_.CommandLine");
    expect(fileWrites).toContain("$$installDir");
    expect(fileWrites).not.toMatch(/(^|[^$])\$(?:_|install|self|PID|false|value|full)/);
  });

  it("does not classify the running installer as a stale app process via the /D argument", () => {
    const source = fs.readFileSync(path.join(root, "build", "installer.nsh"), "utf-8");
    const cleaner = extractMacro(source, "lingxiWriteInstallDirProcessCleaner");
    const finder = extractMacro(source, "lingxiWriteInstallDirProcessFinder");

    for (const macro of [cleaner, finder]) {
      expect(macro).toContain("$$installerPid");
      expect(macro).toContain("$$_.ProcessId -ne $$installerPid");
      expect(macro).not.toContain("return $$value.IndexOf($$installFull");
    }
  });

  it("future uninstallers remove Lingxi-owned install surfaces without atomic old-install staging", () => {
    const source = fs.readFileSync(path.join(root, "build", "installer.nsh"), "utf-8");
    const macro = extractMacro(source, "customRemoveFiles");

    expect(macro).toContain("lingxiRemoveOwnedInstallTrees");
    expect(macro).toContain('Delete "$INSTDIR\\${APP_EXECUTABLE_FILENAME}"');
    expect(macro).not.toContain("old-install");
    expect(macro).not.toContain("un.atomicRMDir");
  });

  it("removes legacy Hanako-branded install entries without blind global shortcut deletion", () => {
    const source = fs.readFileSync(path.join(root, "build", "installer.nsh"), "utf-8");
    const macro = extractMacro(source, "lingxiRemoveOwnedInstallTrees");
    const overlay = extractMacro(source, "lingxiPrepareOwnedOverlay");
    const shortcutCleaner = extractMacro(source, "lingxiWriteLegacyShortcutCleaner");

    expect(macro).toContain('Delete "$INSTDIR\\Hanako.exe"');
    expect(macro).toContain('Delete "$INSTDIR\\Uninstall Hanako.exe"');
    expect(macro).toContain('Delete "$INSTDIR\\hanako-install-diagnostics.log"');
    expect(macro).not.toContain('Delete "$DESKTOP\\Hanako.lnk"');
    expect(macro).not.toContain('Delete "$SMPROGRAMS\\Hanako.lnk"');
    expect(macro).not.toContain('RMDir /r "$SMPROGRAMS\\Hanako"');
    expect(macro).toContain("lingxiRemoveLegacyGlobalShortcuts");
    expect(shortcutCleaner).toContain("WScript.Shell");
    expect(shortcutCleaner).toContain("CreateShortcut");
    expect(shortcutCleaner).toContain("Test-LingxiInstallPath $$shortcut.TargetPath");
    expect(shortcutCleaner).toContain("Test-LingxiInstallPath $$shortcut.WorkingDirectory");
    expect(shortcutCleaner).not.toContain("Remove-Item -LiteralPath $$legacyDir -Recurse");
    expect(macro).not.toContain('Delete "$INSTDIR\\*.exe"');
    expect(overlay).toContain("lingxiRemoveOwnedInstallTrees");
  });

  it("overrides app-running detection to close LingxiAgent, legacy Hanako, and the bundled server explicitly", () => {
    const source = fs.readFileSync(path.join(root, "build", "installer.nsh"), "utf-8");
    const macro = extractMacro(source, "customCheckAppRunning");

    expect(macro).toContain("Lingxi.exe");
    expect(macro).toContain("Hanako.exe");
    expect(macro).toContain("lingxi-server.exe");
    expect(macro).toContain("appCannotBeClosed");
    expect(macro).toContain("MB_RETRYCANCEL");
    expect(macro).toContain("DetailPrint");
    expect(macro).not.toContain("StartsWith('$INSTDIR'");
  });

  it("keeps silent updater installs eligible to relaunch after install", () => {
    const pkg = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf-8"));

    expect(pkg.build.nsis.runAfterFinish).not.toBe(false);
  });

  it("keeps Windows installs on a stable managed install root", () => {
    const pkg = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf-8"));

    expect(pkg.build.nsis.allowToChangeInstallationDirectory).toBe(false);
  });

  it("pins the Windows executable name to the current product identity", () => {
    const pkg = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf-8"));

    expect(pkg.build.win.executableName).toBe("Lingxi");
    expect(pkg.build.nsis.shortcutName).toBe("Lingxi");
  });

  it("runs an install surface self-check and writes diagnostics before aborting", () => {
    const source = fs.readFileSync(path.join(root, "build", "installer.nsh"), "utf-8");
    const customInstall = extractMacro(source, "customInstall");
    const verify = extractMacro(source, "lingxiVerifyInstallSurface");

    expect(customInstall).toContain("lingxiVerifyInstallSurface");
    expect(verify).toContain('hanaagent-install-diagnostics.log');
    expect(verify).toContain('$INSTDIR\\${APP_EXECUTABLE_FILENAME}');
    expect(verify).toContain('$INSTDIR\\resources\\app.asar');
    expect(verify).toContain('$INSTDIR\\resources\\app-update.yml');
    expect(verify).toContain('lingxiRequireInstallSurfaceGlob "$INSTDIR\\resources\\seed" "seed-train-*.json"');
    expect(verify).toContain('lingxiRequireInstallSurfaceGlob "$INSTDIR\\resources\\seed" "seed-train-*.json.sig"');
    expect(verify).toContain('lingxiRequireInstallSurfaceGlob "$INSTDIR\\resources\\seed" "server-*.tar.gz"');
    expect(verify).toContain('lingxiRequireInstallSurfaceGlob "$INSTDIR\\resources\\seed" "renderer-*.tar.gz"');
    expect(verify).toContain('$INSTDIR\\resources\\git\\cmd\\git.exe');
    expect(verify).toContain('$INSTDIR\\resources\\git\\usr\\bin\\sh.exe');
    expect(verify).toContain('MessageBox MB_OK|MB_ICONSTOP');
    expect(verify).toContain('Quit');
  });

  it("resolves seed archive wildcards through FindFirst/FindClose without hardcoding a version", () => {
    const source = fs.readFileSync(path.join(root, "build", "installer.nsh"), "utf-8");
    const glob = extractMacro(source, "lingxiRequireInstallSurfaceGlob");

    expect(glob).toContain("FindFirst $R3 $R4");
    expect(glob).toContain("FindClose $R3");
    expect(glob).not.toMatch(/\d+\.\d+\.\d+/);
  });

  it("verifies the MinGit install surface without requiring the retired bundled bash", () => {
    const source = fs.readFileSync(path.join(root, "build", "installer.nsh"), "utf-8");
    const verify = extractMacro(source, "lingxiVerifyInstallSurface");

    // MinGit 不再打包 bash.exe；安装器与资源是同一个包的原子产物，自检要求 sh.exe 即可
    expect(verify).not.toContain("bash.exe");
    expect(verify).not.toContain("PortableGit");
  });

  it("records installer phase timing without changing install success conditions", () => {
    const source = fs.readFileSync(path.join(root, "build", "installer.nsh"), "utf-8");
    const timing = extractMacro(source, "lingxiInstallTimingMark");
    const persist = extractMacro(source, "lingxiPersistInstallTiming");
    const customInit = extractMacro(source, "customInit");
    const customCheck = extractMacro(source, "customCheckAppRunning");
    const customInstall = extractMacro(source, "customInstall");
    const stopProcesses = extractMacro(source, "lingxiStopInstallDirProcesses");
    const removeTrees = extractMacro(source, "lingxiRemoveOwnedInstallTrees");
    const verify = extractMacro(source, "lingxiVerifyInstallSurface");

    expect(timing).toContain("GetTickCount");
    expect(timing).toContain("$PLUGINSDIR\\hanaagent-install-timing.log");
    expect(timing).toContain("phase=${_PHASE}");
    expect(timing).not.toContain("Quit");
    expect(persist).toContain("$INSTDIR\\hanaagent-install-timing.log");
    expect(customInit).toContain('lingxiInstallTimingMark "customInit" "start"');
    expect(customInit).toContain('lingxiInstallTimingMark "customInit" "end"');
    expect(customCheck).toContain('lingxiInstallTimingMark "customCheckAppRunning" "start"');
    expect(customCheck).toContain('lingxiInstallTimingMark "customCheckAppRunning" "end"');
    expect(customInstall).toContain('lingxiInstallTimingMark "customInstall" "start"');
    expect(customInstall).toContain('lingxiInstallTimingMark "customInstall" "end"');
    expect(stopProcesses).toContain('lingxiInstallTimingMark "stopInstallDirProcesses" "start"');
    expect(stopProcesses).toContain('lingxiInstallTimingMark "stopInstallDirProcesses" "end"');
    expect(removeTrees).toContain('lingxiInstallTimingMark "removeOwnedInstallTrees" "start"');
    expect(removeTrees).toContain('lingxiInstallTimingMark "removeOwnedInstallTrees" "end"');
    expect(verify).toContain("lingxiPersistInstallTiming");
  });

  it("grants the restricted-app-packages sandbox ACE on the install directory during install", () => {
    const source = fs.readFileSync(path.join(root, "build", "installer.nsh"), "utf-8");
    const macro = extractMacro(source, "lingxiGrantSandboxAce");
    const install = extractMacro(source, "customInstall");

    expect(macro).toContain('"$SYSDIR\\icacls.exe" "$INSTDIR" /grant *S-1-15-2-2:(OI)(CI)(RX)');
    expect(macro).not.toContain("Quit");
    expect(install).toContain("lingxiGrantSandboxAce");
  });
});

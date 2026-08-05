/**
 * resign-adhoc.cjs — electron-builder afterSign 钩子：无证书时全量 ad-hoc 重签
 *
 * 为什么需要这个钩子（根因见 v0.1.0 mac 崩溃报告）：
 *   CI 未配 Apple 证书时，electron-builder 走 ad-hoc 签名路径——它只把主程序
 *   Lingxi 和 Helper 重签成 ad-hoc，但 Electron Framework 等 .framework 仍保留
 *   Electron 官方 Team ID。macOS dyld 加载 framework 时发现「主进程(无 Team ID)
 *   与 framework(Electron 官方 Team ID) Team ID 不一致」，启动即 SIGABRT：
 *     "code signature ... not valid for use in process:
 *      mapping process and mapped file (non-platform) have different Team IDs"
 *
 *   本钩子把包内所有 Mach-O 组件统一重签为 ad-hoc(无 Team ID)，消除 Team ID
 *   不一致。逻辑与本地安装用的 scripts/sign-local.cjs 同源（那份处理 /Applications，
 *   且包含已废弃的散装 server/ 树重签）。
 *
 * 何时跳过（让真签名生效）：
 *   配了 CSC_LINK(Developer ID 证书) 时直接 return——electron-builder 已用统一
 *   的 Developer ID 签好所有组件，Team ID 天然一致，无需也不应覆盖。这样将来
 *   配上 Apple 证书即可无缝切到「签名+公证」正规分发，无需改本文件。
 *
 * 重签范围（对照 dist/mac-arm64/Lingxi.app 实际 Mach-O 清单）：
 *   - Resources/computer-use/macos/lingxi-computer-use-helper（裸 Mach-O）
 *   - Contents/Frameworks/*.framework（含 Electron Framework——崩溃元凶；
 *     含其内部 Libraries/*.dylib 与 Helpers，由 --deep 递归处理）
 *   - Contents/Frameworks/*.app（各 Helper，带 entitlements，V8 需 JIT 权限）
 *   - 主 app（最后，带 entitlements）
 *   注：Resources/seed/ 下是 tar.gz 归档，箱内 Mach-O 在 build-server 阶段已签，
 *   且 afterSign 时仍是归档形态，不在本层处理。
 *
 * 调用方式：作为 build.afterSign 数组的第一个元素，在 notarize.cjs 之前执行。
 */
const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");

/**
 * 签名前即时清除 codesign 阻塞性 xattr。
 *
 * codesign 对带 com.apple.FinderInfo / com.apple.fileprovider.fpfs#P 标记的文件
 * 会报 "resource fork, Finder information, or similar detritus not allowed"。
 * fix-modules.cjs(afterPack) 已清过一次，但当构建目录位于 iCloud / 文件提供者
 * 同步范围内时（如 ~/Desktop），系统会在删除后秒级重新注入这些标记，导致
 * `find -exec xattr -d` 边删边被重新打上、清不干净。
 *
 * 解法：用 `xattr -cr`（递归 clear all）在签名前的瞬间原子清空，紧跟 codesign，
 * 不给系统重新注入的窗口。CI 环境（非同步目录）本就无此问题，这里幂等无副作用。
 * com.apple.provenance 会被一并清掉，codesign 不依赖它，不影响签名有效性。
 */
function sign(target, opts = "") {
  // xattr -cr 紧跟 codesign：清完立即签，把 codesign 阻塞性 detritus
  //（FinderInfo / fileprovider 标记）的重新注入窗口压到最小。CI 环境
  //（非 iCloud 同步目录）无此问题，本地同步目录下偶发注入则靠重试兜底。
  const cmd = `xattr -cr "${target}" && codesign --sign - --force ${opts} "${target}"`;
  let lastErr;
  for (let attempt = 1; attempt <= 3; attempt++) {
    try {
      execSync(cmd, { stdio: "inherit" });
      return;
    } catch (err) {
      lastErr = err;
      if (attempt < 3) console.log(`[resign-adhoc] sign retry ${attempt + 1}/3 for ${path.basename(target)}`);
    }
  }
  throw lastErr;
}

exports.default = async function resignAdhoc(context) {
  // 仅 mac；electron-builder 对每个平台都会调用 afterSign，非 mac 直接放行。
  if (context.electronPlatformName !== "darwin") return;

  // 有 Developer ID 证书时跳过：electron-builder 已统一签名，Team ID 一致。
  if (process.env.CSC_LINK) {
    console.log("[resign-adhoc] CSC_LINK set — Developer ID signing in effect, skipping ad-hoc resign");
    return;
  }

  const appDir = path.join(context.appOutDir, context.packager.appInfo.productFilename + ".app");
  if (!fs.existsSync(appDir)) {
    throw new Error(`[resign-adhoc] app bundle not found: ${appDir}`);
  }

  const entitlements = context.packager.platformSpecificBuildOptions.entitlements;
  const entFlag = entitlements && fs.existsSync(entitlements)
    ? `--entitlements "${entitlements}"`
    : "";

  console.log(`[resign-adhoc] full ad-hoc resign for ${appDir}`);

  // 1. computer-use helper（裸 Mach-O）
  const computerUseHelper = path.join(appDir, "Contents", "Resources", "computer-use", "macos", "lingxi-computer-use-helper");
  if (fs.existsSync(computerUseHelper)) {
    sign(computerUseHelper);
  }

  // 2. Frameworks：.framework(--deep 递归内部 dylib/Helpers) + Helper.app(带 entitlements)
  const frameworksDir = path.join(appDir, "Contents", "Frameworks");
  for (const entry of fs.readdirSync(frameworksDir)) {
    const full = path.join(frameworksDir, entry);
    if (entry.endsWith(".framework")) {
      sign(full, "--deep");
    } else if (entry.endsWith(".app")) {
      sign(full, entFlag);
    }
  }

  // 3. 主 app（最后，带 entitlements——V8 需要 com.apple.security.cs.allow-jit）
  sign(appDir, entFlag);

  // 4. fail-closed 校验：签名不一致的包绝不能流出构建机
  execSync(`codesign --verify --deep --strict "${appDir}"`, { stdio: "inherit" });
  console.log("[resign-adhoc] ✓ signed and verified");
};

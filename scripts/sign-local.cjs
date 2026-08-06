/**
 * sign-local.cjs — 本地安装后的 ad-hoc 重签
 *
 * electron-builder 的 ad-hoc 签名和 Electron Framework 原始签名 Team ID 不同，
 * macOS 拒绝加载。这个脚本统一重签所有二进制，确保 Team ID 一致。
 *
 * 关键：server/node_modules 里的 .node 文件（native addon）也要签，
 * codesign --deep 不会递归进 node_modules 目录。
 */
const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const APP = "/Applications/Lingxi.app";
const ENT = path.join(__dirname, "..", "desktop", "entitlements.mac.plist");

/**
 * 签名前即时清除 codesign 阻塞性 xattr——与 scripts/resign-adhoc.cjs 的 sign
 * 完全同源。
 *
 * codesign 对带 com.apple.FinderInfo / com.apple.fileprovider.fpfs#P 标记的文件
 * 会报 "resource fork, Finder information, or similar detritus not allowed"。
 * 当构建/安装目录位于 iCloud / 文件提供者同步范围内时（如 ~/Desktop），系统
 * 会在删除后秒级重新注入这些标记，导致 `find -exec xattr -d` 边删边被重新
 * 打上、清不干净。
 *
 * 解法：用 `xattr -cr`（递归 clear all）在签名前的瞬间原子清空，紧跟 codesign，
 * 不给系统重新注入的窗口。CI 环境（非同步目录）本就无此问题，这里幂等无副作用。
 * com.apple.provenance 会被一并清掉，codesign 不依赖它，不影响签名有效性。
 */
function sign(target, opts = "") {
  const cmd = `xattr -cr "${target}" && codesign --sign - --force ${opts} "${target}"`;
  let lastErr;
  for (let attempt = 1; attempt <= 3; attempt++) {
    try {
      execSync(cmd, { stdio: "inherit" });
      return;
    } catch (err) {
      lastErr = err;
      if (attempt < 3) console.log(`[sign-local] sign retry ${attempt + 1}/3 for ${path.basename(target)}`);
    }
  }
  throw lastErr;
}

// 1. 签 server 里的所有 Mach-O 文件（node binary + .node addons）
const serverDir = path.join(APP, "Contents", "Resources", "server");
if (fs.existsSync(serverDir)) {
  // node binary
  const nodeBin = path.join(serverDir, "node");
  if (fs.existsSync(nodeBin)) sign(nodeBin);

  // .node files（native addons）
  function findNodeFiles(dir) {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        findNodeFiles(full);
      } else if (entry.name.endsWith(".node")) {
        sign(full);
      }
    }
  }
  findNodeFiles(path.join(serverDir, "node_modules"));
}

// 2. 签 Computer Use helper
const computerUseHelper = path.join(APP, "Contents", "Resources", "computer-use", "macos", "lingxi-computer-use-helper");
if (fs.existsSync(computerUseHelper)) {
  sign(computerUseHelper);
}

// 3. 签 frameworks + helpers（--deep 处理内部结构）
const frameworks = path.join(APP, "Contents", "Frameworks");
for (const entry of fs.readdirSync(frameworks)) {
  const full = path.join(frameworks, entry);
  if (entry.endsWith(".framework")) {
    sign(full, "--deep");
  } else if (entry.endsWith(".app")) {
    sign(full, `--entitlements "${ENT}"`);
  }
}

// 4. 签主 app（带 entitlements，V8 需要 JIT 权限）
sign(APP, `--entitlements "${ENT}"`);

// 5. 验证
execSync(`codesign --verify --deep --strict "${APP}"`, { stdio: "inherit" });
console.log("✓ Signed and verified");

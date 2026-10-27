#!/usr/bin/env node
"use strict";

/**
 * R11：随包 Chromium 组件获取与校验。
 *
 * 生产包不再依赖用户机器上的 Chrome。release 构建前运行本脚本，把固定
 * revision 的 Chromium 解压到 `desktop/src-tauri/resources/chromium/`，
 * 由 `tauri.release.conf.json` 的 resources 映射装入应用；宿主启动服务时
 * 注入 `LINGXI_CHROMIUM_PATH`，服务据此启动浏览器/PDF 预览。
 *
 * 供应链约束：
 * - 只接受本文件内固定的 URL + SHA-256；下载后先校验再解压；
 * - 目标目录在 `desktop/src-tauri/.gitignore` 内，不进入 git；
 * - 解压/复制失败显式退出非 0，不静默留下半套文件；
 * - 目前只提供 macOS arm64（业主当前平台）；其它平台显式报不支持。
 *
 * 用法：
 *   node scripts/fetch-chromium.cjs
 *   node scripts/fetch-chromium.cjs --force
 */

const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const https = require("node:https");

const ROOT = path.resolve(__dirname, "..");
const RESOURCE_DIR = path.join(ROOT, "desktop", "src-tauri", "resources", "chromium");
const MARKER = path.join(RESOURCE_DIR, ".pinned.json");

const PINS = {
  "darwin-arm64": {
    label: "Chromium Mac_Arm snapshot",
    revision: "1727675",
    // 与 2026-10-27 的 Mac_Arm/LAST_CHANGE 对齐；升级必须同时改 URL 与 hash。
    url: "https://storage.googleapis.com/chromium-browser-snapshots/Mac_Arm/1727675/chrome-mac.zip",
    sha256: "87d91e250130b993b62f571a4c110a5d5c0ed729143fce8a9d9bbd8616c1247c",
    archiveRoot: "chrome-mac",
    appDir: "Chromium.app",
    executable: path.join("Chromium.app", "Contents", "MacOS", "Chromium"),
  },
};

function targetKey() {
  return `${process.platform}-${process.arch}`;
}

function sha256File(filePath) {
  const hash = crypto.createHash("sha256");
  const fd = fs.openSync(filePath, "r");
  try {
    const buffer = Buffer.allocUnsafe(1024 * 1024);
    for (;;) {
      const bytes = fs.readSync(fd, buffer, 0, buffer.length, null);
      if (bytes <= 0) break;
      hash.update(buffer.subarray(0, bytes));
    }
  } finally {
    fs.closeSync(fd);
  }
  return hash.digest("hex");
}

function download(url, destination, redirectsLeft = 5) {
  return new Promise((resolve, reject) => {
    const request = https.get(url, (response) => {
      const status = response.statusCode || 0;
      if (status >= 300 && status < 400 && response.headers.location) {
        response.resume();
        if (redirectsLeft <= 0) {
          reject(new Error(`too many redirects: ${url}`));
          return;
        }
        const next = new URL(response.headers.location, url).toString();
        download(next, destination, redirectsLeft - 1).then(resolve, reject);
        return;
      }
      if (status !== 200) {
        response.resume();
        reject(new Error(`download failed HTTP ${status}: ${url}`));
        return;
      }
      const expected = Number(response.headers["content-length"] || 0);
      let received = 0;
      const out = fs.createWriteStream(destination, { mode: 0o600 });
      response.on("data", (chunk) => {
        received += chunk.length;
      });
      response.pipe(out);
      out.on("finish", () => {
        out.close(() => {
          if (expected > 0 && received !== expected) {
            reject(new Error(`truncated download: expected ${expected}, received ${received}`));
            return;
          }
          resolve(received);
        });
      });
      out.on("error", reject);
    });
    request.on("error", reject);
    request.setTimeout(10 * 60 * 1000, () => {
      request.destroy(new Error(`download timeout: ${url}`));
    });
  });
}

function removeIfExists(target) {
  fs.rmSync(target, { recursive: true, force: true });
}

async function main() {
  const force = process.argv.includes("--force");
  const key = targetKey();
  const pin = PINS[key];
  if (!pin) {
    throw new Error(
      `no pinned Chromium for ${key}; supported: ${Object.keys(PINS).join(", ")}`,
    );
  }

  const executablePath = path.join(RESOURCE_DIR, pin.executable);
  if (!force && fs.existsSync(MARKER) && fs.existsSync(executablePath)) {
    const marker = JSON.parse(fs.readFileSync(MARKER, "utf-8"));
    if (marker.revision === pin.revision && marker.sha256 === pin.sha256) {
      console.log(`[chromium] already staged: ${executablePath}`);
      return;
    }
  }

  const workDir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-chromium-"));
  const zipPath = path.join(workDir, "chrome-mac.zip");
  const extractDir = path.join(workDir, "extract");
  try {
    console.log(`[chromium] downloading ${pin.label} revision ${pin.revision} …`);
    await download(pin.url, zipPath);
    const actual = sha256File(zipPath);
    if (actual !== pin.sha256) {
      throw new Error(
        `sha256 mismatch for ${pin.url}\n  expected ${pin.sha256}\n  actual   ${actual}`,
      );
    }
    console.log(`[chromium] sha256 ok: ${actual}`);

    fs.mkdirSync(extractDir, { recursive: true });
    // macOS 自带 unzip；显式 -q -o，失败即抛。
    execFileSync("/usr/bin/unzip", ["-q", "-o", zipPath, "-d", extractDir], {
      stdio: "inherit",
    });
    const appPath = path.join(extractDir, pin.archiveRoot, pin.appDir);
    const extractedExecutable = path.join(extractDir, pin.archiveRoot, pin.executable);
    if (!fs.existsSync(appPath) || !fs.existsSync(extractedExecutable)) {
      throw new Error(`unexpected Chromium archive layout: ${appPath}`);
    }

    fs.mkdirSync(path.dirname(RESOURCE_DIR), { recursive: true });
    removeIfExists(RESOURCE_DIR);
    fs.mkdirSync(RESOURCE_DIR, { recursive: true });
    const stagedAppPath = path.join(RESOURCE_DIR, pin.appDir);
    fs.cpSync(appPath, stagedAppPath, { recursive: true, verbatimSymlinks: true });
    fs.chmodSync(executablePath, 0o755);
    // 快照的嵌套 Chromium.app 是 linker-signed 且资源签名不完整；随包前统一
    // 重新 ad-hoc 签名，避免主应用验证因嵌套资源不匹配失败（不做正式签名）。
    try {
      execFileSync("/usr/bin/codesign", ["--force", "--deep", "--sign", "-", stagedAppPath], {
        stdio: "inherit",
      });
      execFileSync("/usr/bin/codesign", ["--verify", "--deep", "--strict", stagedAppPath], {
        stdio: "inherit",
      });
    } catch (error) {
      throw new Error(`codesign bundled Chromium failed: ${error.message || error}`);
    }
    fs.writeFileSync(
      MARKER,
      `${JSON.stringify(
        {
          target: key,
          label: pin.label,
          revision: pin.revision,
          url: pin.url,
          sha256: pin.sha256,
          executable: pin.executable,
          stagedAt: new Date().toISOString(),
        },
        null,
        2,
      )}\n`,
      "utf-8",
    );
    console.log(`[chromium] staged: ${executablePath}`);
  } finally {
    removeIfExists(workDir);
  }
}

main().catch((error) => {
  console.error(`[chromium] ${error && error.message ? error.message : error}`);
  process.exitCode = 1;
});

#!/usr/bin/env node
"use strict";

/**
 * R11：随包 Node 运行时。
 *
 * 业主裁决：外围 worker（Office/媒体/AST/LSP/PTC/Bridge）保留 Node 实现，
 * 由应用携带固定 Node 运行时，不要求用户机器预装 Node。
 *
 * 供应链约束：固定官方版本 + SHA-256；下载后校验、解压、ad-hoc 重签。
 * 目标目录在 desktop/src-tauri/.gitignore 内，不进入 git。
 *
 * 目前只提供 macOS arm64（业主当前平台）；其它平台显式报不支持。
 */

const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const https = require("node:https");

const ROOT = path.resolve(__dirname, "..");
const RESOURCE_DIR = path.join(ROOT, "desktop", "src-tauri", "resources", "node");
const MARKER = path.join(RESOURCE_DIR, ".pinned.json");

const PINS = {
  "darwin-arm64": {
    version: "24.16.0",
    url: "https://nodejs.org/dist/v24.16.0/node-v24.16.0-darwin-arm64.tar.gz",
    sha256: "39189dab4eeb15706c424af0ac08a3044c9e48f7db12a7d77f6b7aafc7dd5df6",
    archiveRoot: "node-v24.16.0-darwin-arm64",
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
        if (redirectsLeft <= 0) return reject(new Error(`too many redirects: ${url}`));
        download(new URL(response.headers.location, url).toString(), destination, redirectsLeft - 1).then(resolve, reject);
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
      response.on("data", (chunk) => { received += chunk.length; });
      response.pipe(out);
      out.on("finish", () => out.close(() => {
        if (expected > 0 && received !== expected) {
          reject(new Error(`truncated download: expected ${expected}, received ${received}`));
          return;
        }
        resolve(received);
      }));
      out.on("error", reject);
    });
    request.on("error", reject);
    request.setTimeout(10 * 60 * 1000, () => request.destroy(new Error(`download timeout: ${url}`)));
  });
}

function removeIfExists(target) {
  fs.rmSync(target, { recursive: true, force: true });
}

async function main() {
  const force = process.argv.includes("--force");
  const key = targetKey();
  const pin = PINS[key];
  if (!pin) throw new Error(`no pinned Node runtime for ${key}; supported: ${Object.keys(PINS).join(", ")}`);

  const executablePath = path.join(RESOURCE_DIR, "bin", "node");
  if (!force && fs.existsSync(MARKER) && fs.existsSync(executablePath)) {
    const marker = JSON.parse(fs.readFileSync(MARKER, "utf-8"));
    if (marker.version === pin.version && marker.sha256 === pin.sha256) {
      console.log(`[node] already staged: ${executablePath}`);
      return;
    }
  }

  const workDir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-node-"));
  const archive = path.join(workDir, "node.tar.gz");
  const extractDir = path.join(workDir, "extract");
  try {
    console.log(`[node] downloading Node ${pin.version} ${key} …`);
    await download(pin.url, archive);
    const actual = sha256File(archive);
    if (actual !== pin.sha256) throw new Error(`sha256 mismatch for ${pin.url}\n  expected ${pin.sha256}\n  actual   ${actual}`);
    console.log(`[node] sha256 ok: ${actual}`);
    fs.mkdirSync(extractDir, { recursive: true });
    execFileSync("/usr/bin/tar", ["-xzf", archive, "-C", extractDir], { stdio: "inherit" });
    const root = path.join(extractDir, pin.archiveRoot);
    const sourceNode = path.join(root, "bin", "node");
    if (!fs.existsSync(sourceNode)) throw new Error(`unexpected Node archive layout: ${sourceNode}`);

    removeIfExists(RESOURCE_DIR);
    fs.mkdirSync(path.join(RESOURCE_DIR, "bin"), { recursive: true });
    fs.copyFileSync(sourceNode, executablePath);
    fs.chmodSync(executablePath, 0o755);
    const license = path.join(root, "LICENSE");
    if (fs.existsSync(license)) fs.copyFileSync(license, path.join(RESOURCE_DIR, "LICENSE"));
    // 官方预编译二进制签名在拷贝/剥壳后可能不再匹配；统一 ad-hoc 重签。
    execFileSync("/usr/bin/codesign", ["--force", "--sign", "-", executablePath], { stdio: "inherit" });
    execFileSync("/usr/bin/codesign", ["--verify", "--strict", executablePath], { stdio: "inherit" });
    fs.writeFileSync(
      MARKER,
      `${JSON.stringify({ target: key, version: pin.version, url: pin.url, sha256: pin.sha256, executable: "bin/node", stagedAt: new Date().toISOString() }, null, 2)}\n`,
      "utf-8",
    );
    console.log(`[node] staged: ${executablePath}`);
  } finally {
    removeIfExists(workDir);
  }
}

main().catch((error) => {
  console.error(`[node] ${error && error.message ? error.message : error}`);
  process.exitCode = 1;
});

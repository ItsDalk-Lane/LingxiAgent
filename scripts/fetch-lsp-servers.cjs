#!/usr/bin/env node
"use strict";

/**
 * R11：随包 LSP 语言服务器。
 *
 * `coding_ast_lsp` 通过 PATH 上的名字启动这些服务；本脚本把它们放进
 * `desktop/src-tauri/resources/lsp/bin`，由宿主把该目录放到 PATH 最前：
 * - TypeScript / JavaScript: typescript-language-server + typescript（npm）
 * - Python: pyright（npm）
 * - Go: gopls（用固定 Go 工具链构建）
 * - Rust: rust-analyzer（官方 release）
 * - C/C++: clangd（clangd 官方 standalone release）
 *
 * 供应链：固定版本/URL/SHA-256；npm 包锁版本；产物 ad-hoc 重签。
 * 目标目录在 .gitignore 内。
 */

const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const https = require("node:https");

const ROOT = path.resolve(__dirname, "..");
const RESOURCE_DIR = path.join(ROOT, "desktop", "src-tauri", "resources", "lsp");
const MARKER = path.join(RESOURCE_DIR, ".pinned.json");

const PINS = {
  clangd: {
    version: "23.1.0",
    url: "https://github.com/clangd/clangd/releases/download/23.1.0/clangd-mac-23.1.0.zip",
    sha256: "1082e6638223b785ca2daf0939f13afcd0bb95c84ee9a4bbaff4745365159253",
    archiveRoot: "clangd_23.1.0",
  },
  rustAnalyzer: {
    version: "2026-10-26",
    url: "https://github.com/rust-lang/rust-analyzer/releases/download/2026-10-26/rust-analyzer-aarch64-apple-darwin.gz",
    sha256: "8fb90cb58f5910d8d8b24ac1a93cca49f053365cbf0eb3c682144ad0a0027dcf",
  },
  gopls: { version: "v0.24.0" },
  go: {
    version: "go1.27.3",
    url: "https://go.dev/dl/go1.27.3.darwin-arm64.tar.gz",
    sha256: "dd3908cf20959d798c1e06ebe5b72213f0505ce01373b40ce3110975a6cd99c2",
  },
  typescript: { version: "5.9.3" },
  typescriptLanguageServer: { version: "6.0.2" },
  pyright: { version: "1.1.414" },
};

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
      const out = fs.createWriteStream(destination, { mode: 0o600 });
      response.pipe(out);
      out.on("finish", () => out.close(resolve));
      out.on("error", reject);
    });
    request.on("error", reject);
    request.setTimeout(10 * 60 * 1000, () => request.destroy(new Error(`download timeout: ${url}`)));
  });
}

function run(command, args, options = {}) {
  execFileSync(command, args, { stdio: "inherit", ...options });
}

function reSign(file) {
  run("/usr/bin/codesign", ["--force", "--sign", "-", file]);
  run("/usr/bin/codesign", ["--verify", "--strict", file]);
}

function writeLauncher(target, nodeRel, entryRel) {
  const script = `#!/bin/sh\nexec "$(dirname "$0")/${nodeRel}" "$(dirname "$0")/${entryRel}" "$@"\n`;
  fs.writeFileSync(target, script, "utf-8");
  fs.chmodSync(target, 0o755);
}

function copyNodePackageTree(prefix, destRoot) {
  const modules = path.join(prefix, "node_modules");
  if (!fs.existsSync(modules)) throw new Error(`npm install produced no node_modules: ${prefix}`);
  fs.mkdirSync(path.dirname(destRoot), { recursive: true });
  fs.rmSync(destRoot, { recursive: true, force: true });
  fs.cpSync(modules, destRoot, { recursive: true, verbatimSymlinks: true });
}

async function main() {
  const force = process.argv.includes("--force");
  const binDir = path.join(RESOURCE_DIR, "bin");
  const installed = {
    clangd: path.join(binDir, "clangd"),
    rustAnalyzer: path.join(binDir, "rust-analyzer"),
    gopls: path.join(binDir, "gopls"),
    typescriptLanguageServer: path.join(binDir, "typescript-language-server"),
    pyrightLangserver: path.join(binDir, "pyright-langserver"),
    pyright: path.join(binDir, "pyright"),
  };
  if (!force && fs.existsSync(MARKER) && Object.values(installed).every((file) => fs.existsSync(file))) {
    console.log(`[lsp] already staged: ${RESOURCE_DIR}`);
    return;
  }

  const workDir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-lsp-"));
  try {
    fs.mkdirSync(binDir, { recursive: true });

    // TypeScript / JavaScript + Python：npm 包按固定版本安装后整树复制。
    for (const [name, packages, dest] of [
      ["typescript", [`typescript@${PINS.typescript.version}`, `typescript-language-server@${PINS.typescriptLanguageServer.version}`], "typescript"],
      ["pyright", [`pyright@${PINS.pyright.version}`], "pyright"],
    ]) {
      const prefix = path.join(workDir, `npm-${name}`);
      fs.mkdirSync(prefix, { recursive: true });
      fs.writeFileSync(path.join(prefix, "package.json"), "{}\n", "utf-8");
      console.log(`[lsp] installing ${packages.join(", ")}`);
      run("npm", ["install", "--prefix", prefix, "--no-audit", "--no-fund", "--no-save", ...packages]);
      copyNodePackageTree(prefix, path.join(RESOURCE_DIR, "node", dest, "node_modules"));
    }
    writeLauncher(
      installed.typescriptLanguageServer,
      "../../node/bin/node",
      "../node/typescript/node_modules/typescript-language-server/lib/cli.mjs",
    );
    writeLauncher(
      installed.pyrightLangserver,
      "../../node/bin/node",
      "../node/pyright/node_modules/pyright/langserver.index.js",
    );
    writeLauncher(
      installed.pyright,
      "../../node/bin/node",
      "../node/pyright/node_modules/pyright/index.js",
    );

    // clangd
    const clangdZip = path.join(workDir, "clangd.zip");
    console.log(`[lsp] downloading clangd ${PINS.clangd.version}`);
    await download(PINS.clangd.url, clangdZip);
    const clangdHash = sha256File(clangdZip);
    if (clangdHash !== PINS.clangd.sha256) throw new Error(`clangd sha256 mismatch: ${clangdHash}`);
    run("/usr/bin/unzip", ["-q", "-o", clangdZip, "-d", path.join(workDir, "clangd")]);
    const clangdSource = path.join(workDir, "clangd", PINS.clangd.archiveRoot, "bin", "clangd");
    fs.copyFileSync(clangdSource, installed.clangd);
    fs.chmodSync(installed.clangd, 0o755);
    reSign(installed.clangd);

    // rust-analyzer
    const raGz = path.join(workDir, "rust-analyzer.gz");
    console.log(`[lsp] downloading rust-analyzer ${PINS.rustAnalyzer.version}`);
    await download(PINS.rustAnalyzer.url, raGz);
    const raHash = sha256File(raGz);
    if (raHash !== PINS.rustAnalyzer.sha256) throw new Error(`rust-analyzer sha256 mismatch: ${raHash}`);
    run("/usr/bin/gunzip", ["-f", raGz]);
    fs.copyFileSync(raGz.replace(/\.gz$/, ""), installed.rustAnalyzer);
    fs.chmodSync(installed.rustAnalyzer, 0o755);
    reSign(installed.rustAnalyzer);

    // gopls：下载固定 Go 工具链构建，产物随包，Go 本身不进入安装包。
    const goTar = path.join(workDir, "go.tar.gz");
    console.log(`[lsp] downloading Go ${PINS.go.version} to build gopls ${PINS.gopls.version}`);
    await download(PINS.go.url, goTar);
    const goHash = sha256File(goTar);
    if (goHash !== PINS.go.sha256) throw new Error(`Go sha256 mismatch: ${goHash}`);
    const goRoot = path.join(workDir, "go");
    run("/usr/bin/tar", ["-xzf", goTar, "-C", workDir]);
    const gobin = path.join(workDir, "gobin");
    fs.mkdirSync(gobin, { recursive: true });
    run(path.join(goRoot, "bin", "go"), ["install", `golang.org/x/tools/gopls@${PINS.gopls.version}`], {
      env: {
        ...process.env,
        GOBIN: gobin,
        GOPATH: path.join(workDir, "gopath"),
        GOTOOLCHAIN: "local",
        CGO_ENABLED: "0",
      },
    });
    fs.copyFileSync(path.join(gobin, "gopls"), installed.gopls);
    fs.chmodSync(installed.gopls, 0o755);
    reSign(installed.gopls);

    fs.writeFileSync(
      MARKER,
      `${JSON.stringify({
        clangd: PINS.clangd.version,
        rustAnalyzer: PINS.rustAnalyzer.version,
        gopls: PINS.gopls.version,
        goBuildToolchain: PINS.go.version,
        typescript: PINS.typescript.version,
        typescriptLanguageServer: PINS.typescriptLanguageServer.version,
        pyright: PINS.pyright.version,
        stagedAt: new Date().toISOString(),
      }, null, 2)}\n`,
      "utf-8",
    );
    console.log(`[lsp] staged under ${RESOURCE_DIR}`);
  } finally {
    try {
      fs.rmSync(workDir, { recursive: true, force: true });
    } catch (error) {
      // 临时目录清理失败不掩盖已经完成的产物；下次运行会重新创建。
      console.warn(`[lsp] temp cleanup warning: ${error && error.message ? error.message : error}`);
    }
  }
}

main().catch((error) => {
  console.error(`[lsp] ${error && error.message ? error.message : error}`);
  process.exitCode = 1;
});

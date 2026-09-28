#!/usr/bin/env node
// 为当前主机的 Electron 安装包构建 Rust 服务；--verify 只核对已有产物。
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { rustServicePlatformTag, rustServiceExecutable, rustServiceContentDigest, verifyRustServiceDirectory } = require('../desktop/src/shared/rust-local-service.cjs');
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const RUST_ROOT = path.join(ROOT, 'rust');

function pinnedToolchain(root = ROOT) {
  const content = fs.readFileSync(path.join(root, 'rust-toolchain.toml'), 'utf8');
  const channel = /^channel\s*=\s*"(\d+\.\d+\.\d+)"/m.exec(content)?.[1];
  if (!channel) throw new Error('rust-toolchain.toml has no pinned channel');
  return channel;
}

function packageVersion(root = ROOT) {
  return JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8')).version;
}

function rustupExecutable(env = process.env, platform = process.platform) {
  const name = platform === 'win32' ? 'rustup.exe' : 'rustup';
  const candidates = [
    env.RUSTUP_BIN,
    path.join(os.homedir(), '.cargo', 'bin', name),
    env.CARGO_HOME ? path.join(env.CARGO_HOME, 'bin', name) : null,
  ].filter(Boolean);
  const binary = candidates.find((candidate) => path.isAbsolute(candidate) && fs.existsSync(candidate));
  if (!binary) throw new Error(`pinned Rust toolchain requires an absolute rustup executable (${candidates.join(', ')})`);
  return binary;
}

function rustSourceDigest(root = RUST_ROOT) {
  const files = [];
  function walk(dir, relative = '') {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const name = relative ? `${relative}/${entry.name}` : entry.name;
      if (!relative && entry.name === 'target') continue;
      if (entry.isSymbolicLink()) throw new Error(`Rust source symlink refused: ${name}`);
      if (entry.isDirectory()) walk(path.join(dir, entry.name), name);
      else if (entry.isFile()) files.push(name);
    }
  }
  walk(root);
  const hash = crypto.createHash('sha256');
  hash.update('rust-toolchain.toml').update('\0');
  hash.update(fs.readFileSync(path.join(path.dirname(root), 'rust-toolchain.toml'))).update('\0');
  for (const name of files.sort()) {
    hash.update(name).update('\0');
    hash.update(fs.readFileSync(path.join(root, name))).update('\0');
  }
  return hash.digest('hex');
}

function assertHostTarget(host, platform = process.platform, arch = process.arch) {
  const cpu = arch === 'x64' ? 'x86_64' : arch === 'arm64' ? 'aarch64' : null;
  const osPart = platform === 'darwin' ? '-apple-darwin'
    : platform === 'win32' ? '-pc-windows-'
      : platform === 'linux' ? '-unknown-linux-' : null;
  if (!cpu || !osPart || !host.startsWith(`${cpu}${osPart}`)) {
    throw new Error(`Rust toolchain target ${host} does not match Electron host ${platform}-${arch}`);
  }
}

function stageDirectory(root = ROOT, platform = process.platform, arch = process.arch) {
  return path.join(root, 'dist-rust-service', `${rustServicePlatformTag(platform)}-${arch}`);
}

function verifyStage({ root = ROOT, platform = process.platform, arch = process.arch } = {}) {
  const directory = stageDirectory(root, platform, arch);
  if (!fs.existsSync(directory)) {
    throw new Error(`no prebuilt Rust service at ${directory}; run npm run build:rust-service on this platform`);
  }
  const result = verifyRustServiceDirectory({ directory, platform, arch });
  const currentSource = rustSourceDigest(path.join(root, 'rust'));
  if (result.manifest.sourceSha256 !== currentSource
      || result.manifest.toolchain !== pinnedToolchain(root)
      || result.manifest.appVersion !== packageVersion(root)) {
    throw new Error(`Rust service package is stale for current source: ${directory}`);
  }
  return result;
}

function buildStage({ root = ROOT, platform = process.platform, arch = process.arch, env = process.env } = {}) {
  const toolchain = pinnedToolchain(root);
  const version = packageVersion(root);
  const rustup = rustupExecutable(env, platform);
  const compiler = execFileSync(rustup, ['run', toolchain, 'rustc', '-vV'], { cwd: root, encoding: 'utf8', env });
  const host = /\bhost:\s*(\S+)/.exec(compiler)?.[1];
  const release = /\brelease:\s*(\S+)/.exec(compiler)?.[1];
  if (release !== toolchain) throw new Error(`rustup selected rustc ${release || 'unknown'}, expected ${toolchain}`);
  if (!host) throw new Error('Rust compiler did not report a host target');
  assertHostTarget(host, platform, arch);
  const sourceBefore = rustSourceDigest(path.join(root, 'rust'));
  const targetDir = env.CARGO_TARGET_DIR
    ? path.resolve(root, env.CARGO_TARGET_DIR)
    : path.join(os.tmpdir(), `lingxi-desktop-rust-target-${toolchain}-${rustServicePlatformTag(platform)}-${arch}-${crypto.createHash('sha256').update(root).digest('hex').slice(0, 12)}`);
  const cargoEnv = { ...env, CARGO_TARGET_DIR: targetDir };
  const cargoVersion = execFileSync(rustup, ['run', toolchain, 'cargo', '--version'], { cwd: root, encoding: 'utf8', env: cargoEnv }).trim();
  if (!cargoVersion.startsWith(`cargo ${toolchain} `)) {
    throw new Error(`rustup selected ${cargoVersion}, expected cargo ${toolchain}`);
  }
  console.log(`[rust-desktop-service] pinned toolchain ${toolchain}; ${cargoVersion}; target ${host}`);
  execFileSync(rustup, ['run', toolchain, 'cargo', 'build', '--manifest-path', path.join(root, 'rust/Cargo.toml'), '--locked', '--release', '-p', 'lingxi-service'], {
    cwd: root, env: cargoEnv, stdio: 'inherit',
  });
  const sourceAfter = rustSourceDigest(path.join(root, 'rust'));
  if (sourceBefore !== sourceAfter || version !== packageVersion(root)) {
    throw new Error('Rust source or package version changed during release build');
  }
  const filename = rustServiceExecutable(platform);
  const builtBinary = path.join(targetDir, 'release', filename);
  const builtStat = fs.lstatSync(builtBinary);
  if (!builtStat.isFile() || builtStat.size < 1024) throw new Error('Cargo did not produce the expected Rust service executable');
  const outRoot = path.join(root, 'dist-rust-service');
  fs.mkdirSync(outRoot, { recursive: true });
  const temp = fs.mkdtempSync(path.join(outRoot, '.stage-'));
  const directory = stageDirectory(root, platform, arch);
  const backup = `${directory}.previous-${process.pid}`;
  let movedOld = false;
  try {
    fs.copyFileSync(builtBinary, path.join(temp, filename));
    if (platform !== 'win32') fs.chmodSync(path.join(temp, filename), 0o755);
    const binaryBytes = fs.readFileSync(path.join(temp, filename));
    const sha256 = crypto.createHash('sha256').update(binaryBytes).digest('hex');
    const contentSha256 = rustServiceContentDigest(binaryBytes, platform);
    fs.writeFileSync(path.join(temp, 'build.json'), `${JSON.stringify({
      schemaVersion: 1, platform: rustServicePlatformTag(platform), arch,
      target: host, toolchain, appVersion: version,
      binary: filename, sha256, contentSha256, sourceSha256: sourceBefore,
    }, null, 2)}\n`, { flag: 'wx' });
    verifyRustServiceDirectory({ directory: temp, platform, arch });
    if (fs.existsSync(backup)) throw new Error(`previous Rust service backup still exists: ${backup}`);
    if (fs.existsSync(directory)) {
      if (fs.lstatSync(directory).isSymbolicLink()) throw new Error('Rust service stage is a symbolic link');
      fs.renameSync(directory, backup);
      movedOld = true;
    }
    try { fs.renameSync(temp, directory); }
    catch (err) {
      if (movedOld) fs.renameSync(backup, directory);
      throw err;
    }
    if (movedOld) fs.rmSync(backup, { recursive: true });
    return verifyStage({ root, platform, arch });
  } finally {
    if (fs.existsSync(temp)) fs.rmSync(temp, { recursive: true });
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    if (process.argv.length > 3 || (process.argv[2] && process.argv[2] !== '--verify')) {
      throw new Error('usage: build-rust-desktop-service.mjs [--verify]');
    }
    const result = process.argv[2] === '--verify' ? verifyStage() : buildStage();
    console.log(`[rust-desktop-service] verified ${result.binary} sha256=${result.manifest.sha256}`);
  } catch (err) {
    console.error(`[rust-desktop-service] ${err?.stack || err}`);
    process.exitCode = 1;
  }
}

export { assertHostTarget, pinnedToolchain, rustSourceDigest, stageDirectory, verifyStage, buildStage };

#!/usr/bin/env node
/**
 * 本地推理运行时组装器：在目标平台自身上下载上游预编译二进制并组装成
 * `<out>/<runtimeId>/<version>/<platform>/` 布局（含逐文件 sha256 的 runtime.json）。
 * CI 矩阵在 ubuntu/windows/macos runner 上各跑一次，即得到各平台运行时包；
 * 不做交叉组装（node 二进制与平台绑定）。
 *
 * 用法：node scripts/local-models/build-runtimes.mjs --out <dir>
 *   [--components llama.cpp,audio.cpp] [--llama-reuse <已组装的 darwin 运行时目录>]
 */
import { execFile } from 'node:child_process';
import { createReadStream, createWriteStream } from 'node:fs';
import fsp from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import { promisify } from 'node:util';
import { pipeline } from 'node:stream/promises';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const execFileAsync = promisify(execFile);
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const args = new Map(process.argv.slice(2).reduce((pairs, value, index, all) => {
  if (value.startsWith('--')) pairs.push([value, all[index + 1]]);
  return pairs;
}, []));
const outRoot = path.resolve(args.get('--out') ?? 'dist/local-runtimes');
const components = (args.get('--components') ?? 'llama.cpp,audio.cpp').split(',');
const llamaReuseDir = args.get('--llama-reuse') ? path.resolve(args.get('--llama-reuse')) : null;
const platform = `${process.platform}-${process.arch}`;

export const RUNTIME_PLAN = {
  'llama.cpp': {
    version: 'b10621',
    wrapper: 'llama-sidecar.mjs',
    serverBinary: (platform) => `${platform.startsWith('win32') ? 'llama-server.exe' : 'llama-server'}`,
    assets: {
      'darwin-arm64': { suffix: 'cpu', url: 'https://github.com/ggml-org/llama.cpp/releases/download/b10621/llama-b10621-bin-macos-arm64.tar.gz', backends: ['cpu'], strip: 'llama-b10621/' },
      'darwin-x64': { suffix: 'cpu', url: 'https://github.com/ggml-org/llama.cpp/releases/download/b10621/llama-b10621-bin-macos-x64.tar.gz', backends: ['cpu'], strip: 'llama-b10621/' },
      'linux-x64': [
        { suffix: 'cpu', url: 'https://github.com/ggml-org/llama.cpp/releases/download/b10621/llama-b10621-bin-ubuntu-x64.tar.gz', backends: ['cpu'], strip: 'llama-b10621/' },
        { suffix: 'vulkan', url: 'https://github.com/ggml-org/llama.cpp/releases/download/b10621/llama-b10621-bin-ubuntu-vulkan-x64.tar.gz', backends: ['cpu', 'vulkan'], strip: 'llama-b10621/' },
      ],
      'win32-x64': [
        { suffix: 'cpu', url: 'https://github.com/ggml-org/llama.cpp/releases/download/b10621/llama-b10621-bin-win-cpu-x64.zip', backends: ['cpu'] },
        { suffix: 'vulkan', url: 'https://github.com/ggml-org/llama.cpp/releases/download/b10621/llama-b10621-bin-win-vulkan-x64.zip', backends: ['cpu', 'vulkan'] },
      ],
    },
  },
  'audio.cpp': {
    version: 'v0.7.1',
    wrapper: 'audio-sidecar.mjs',
    serverBinary: (platform) => `${platform.startsWith('win32') ? 'audiocpp_server.exe' : 'audiocpp_server'}`,
    assets: {
      'darwin-arm64': { suffix: 'metal', url: 'https://github.com/0xShug0/audio.cpp/releases/download/v0.7.1/audio-v0.7.1-bin-macos-arm64-metal.tar.gz', backends: ['cpu', 'metal'] },
      'darwin-x64': { suffix: 'metal', url: 'https://github.com/0xShug0/audio.cpp/releases/download/v0.7.1/audio-v0.7.1-bin-macos-x64-metal.tar.gz', backends: ['cpu', 'metal'] },
      'linux-x64': [
        { suffix: 'cpu', url: 'https://github.com/0xShug0/audio.cpp/releases/download/v0.7.1/audio-v0.7.1-bin-ubuntu-x64-cpu.tar.gz', backends: ['cpu'] },
        { suffix: 'vulkan', url: 'https://github.com/0xShug0/audio.cpp/releases/download/v0.7.1/audio-v0.7.1-bin-ubuntu-x64-vulkan.tar.gz', backends: ['cpu', 'vulkan'] },
      ],
      'win32-x64': [
        { suffix: 'cpu', url: 'https://github.com/0xShug0/audio.cpp/releases/download/v0.7.1/audio-v0.7.1-bin-windows-x64-cpu-portable.zip', backends: ['cpu'] },
        { suffix: 'vulkan', url: 'https://github.com/0xShug0/audio.cpp/releases/download/v0.7.1/audio-v0.7.1-bin-windows-x64-vulkan.zip', backends: ['cpu', 'vulkan'] },
      ],
    },
  },
};

/** 选出当前平台要组装的变体列表；未知平台返回空（fail-closed，不猜资产）。 */
export function selectPlan(plan, platformKey, component) {
  const entry = plan[component];
  if (!entry) return [];
  const asset = entry.assets[platformKey];
  if (!asset) return [];
  return (Array.isArray(asset) ? asset : [asset]).map((variant) => ({
    component,
    version: `${entry.version}-${variant.suffix}`,
    url: variant.url,
    backends: variant.backends,
    strip: variant.strip ?? '',
    wrapper: entry.wrapper,
    serverBinary: entry.serverBinary(platformKey.split('-')[0]),
  }));
}

async function download(url, destination) {
  const expected = await contentLength(url);
  if (!expected) throw new Error(`no content-length for ${url}`);
  for (let attempt = 1; attempt <= 4; attempt++) {
    try {
      const response = await fetch(url, { redirect: 'follow', signal: AbortSignal.timeout(600000) });
      if (!response.ok || !response.body) throw new Error(`HTTP ${response.status}`);
      await pipeline(response.body, createWriteStream(destination));
      const actual = (await fsp.stat(destination)).size;
      if (actual === expected) return;
      throw new Error(`size mismatch ${actual}/${expected}`);
    } catch (error) {
      if (attempt === 4) throw error;
      process.stderr.write(`retry(${attempt}) ${path.basename(destination)}: ${error.message}\n`);
      await new Promise((resolve) => setTimeout(resolve, 3000));
    }
  }
}

async function contentLength(url) {
  for (let attempt = 1; attempt <= 4; attempt++) {
    try {
      const response = await fetch(url, { method: 'HEAD', redirect: 'follow', signal: AbortSignal.timeout(30000) });
      return Number(response.headers.get('content-length')) || null;
    } catch (error) {
      if (attempt === 4) throw error;
      process.stderr.write(`retry(${attempt}) HEAD ${url}: ${error.message}\n`);
      await new Promise((resolve) => setTimeout(resolve, 3000));
    }
  }
}

async function extract(archive, destination) {
  await fsp.mkdir(destination, { recursive: true });
  if (archive.endsWith('.tar.gz') || archive.endsWith('.tgz')) {
    await execFileAsync('tar', ['-xzf', archive, '-C', destination]);
  } else if (archive.endsWith('.zip')) {
    // macOS/linux 的 bsdtar 与 Windows 自带 tar.exe 都能解 zip；无 tar 时退回 unzip。
    try {
      await execFileAsync('tar', ['-xf', archive, '-C', destination]);
    } catch {
      await execFileAsync('unzip', ['-q', archive, '-d', destination]);
    }
  } else {
    throw new Error(`unsupported archive: ${archive}`);
  }
}

/** 解包后若只有单一顶层目录（llama 的 tar 就是），把内容上提到 destination 根。 */
async function flattenSingleTopDir(destination) {
  const entries = await fsp.readdir(destination);
  if (entries.length !== 1) return;
  const inner = path.join(destination, entries[0]);
  if (!(await fsp.stat(inner)).isDirectory()) return;
  for (const name of await fsp.readdir(inner)) {
    await fsp.rename(path.join(inner, name), path.join(destination, name));
  }
  await fsp.rmdir(inner);
}

async function copyPayload(from, to) {
  // 上游 tar 里有版本符号链接（如 libllama.so → libllama.so.0.3.0），
  // 而 runtime.json 完整性校验拒绝符号链接，落盘时统一解引用成实体文件。
  await fsp.cp(from, to, { recursive: true, dereference: true });
}

async function inventory(directory, prefix = '') {
  const rows = [];
  for (const entry of await fsp.readdir(path.join(directory, prefix), { withFileTypes: true })) {
    const relative = path.join(prefix, entry.name);
    if (relative === 'runtime.json') continue;
    if (entry.isDirectory()) rows.push(...await inventory(directory, relative));
    else if (entry.isFile()) {
      const stat = await fsp.stat(path.join(directory, relative));
      if (!stat.size) continue;
      const hash = createHash('sha256');
      for await (const chunk of createReadStream(path.join(directory, relative))) hash.update(chunk);
      rows.push({ path: relative, bytes: stat.size, sha256: hash.digest('hex') });
    } else throw new Error('unsafe runtime file');
  }
  return rows;
}

async function main() {
  await fsp.mkdir(outRoot, { recursive: true });
  const work = await fsp.mkdtemp(path.join(outRoot, '.work-'));
  const results = [];
  try {
    for (const component of components) {
      for (const variant of selectPlan(RUNTIME_PLAN, platform, component)) {
        const archiveExtension = variant.url.endsWith('.tar.gz') || variant.url.endsWith('.tgz') ? '.tar.gz' : path.extname(variant.url);
        const archive = path.join(work, `${component}-${variant.version}${archiveExtension}`);
        process.stdout.write(`downloading ${variant.url}\n`);
        await download(variant.url, archive);
        const payload = path.join(work, `${component}-${variant.version}-payload`);
        await extract(archive, payload);
        await flattenSingleTopDir(payload);

        const runtimeDir = path.join(outRoot, component, variant.version, platform);
        await fsp.rm(runtimeDir, { recursive: true, force: true });
        await fsp.mkdir(runtimeDir, { recursive: true });
        await copyPayload(payload, runtimeDir);
        await fsp.cp(path.join(repoRoot, 'scripts/local-models', variant.wrapper), path.join(runtimeDir, variant.wrapper));
        const nodeName = process.platform === 'win32' ? 'node.exe' : 'node';
        await fsp.copyFile(await fsp.realpath(process.execPath), path.join(runtimeDir, nodeName));
        if (process.platform !== 'win32') {
          await fsp.chmod(path.join(runtimeDir, nodeName), 0o755);
          await fsp.chmod(path.join(runtimeDir, variant.serverBinary), 0o755).catch(() => {});
        }
        const files = await inventory(runtimeDir);
        await fsp.writeFile(path.join(runtimeDir, 'runtime.json'), JSON.stringify({
          schemaVersion: 1,
          id: component,
          version: variant.version,
          platform,
          kind: 'sidecar',
          entrypoint: nodeName,
          backends: variant.backends,
          files,
        }, null, 2));
        results.push({ component, version: variant.version, platform, backends: variant.backends, files: files.length });
        await fsp.rm(payload, { recursive: true, force: true });
      }
    }
    // darwin 的 llama.cpp 官方包是 CPU 构建；有本地 Metal 构建时用 reuse 目录替换，
    // 让 macOS 用户继续走 GPU（reuse 目录须已是完整运行时布局，含 runtime.json）。
    if (llamaReuseDir) {
      const reuse = path.join(outRoot, 'llama.cpp', path.basename(llamaReuseDir), platform);
      await fsp.rm(reuse, { recursive: true, force: true });
      await copyPayload(llamaReuseDir, reuse);
      results.push({ component: 'llama.cpp', version: path.basename(llamaReuseDir), platform, reused: true });
    }
  } finally {
    await fsp.rm(work, { recursive: true, force: true }).catch(() => {});
  }
  console.log(JSON.stringify({ platform, outRoot, runtimes: results }, null, 2));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();

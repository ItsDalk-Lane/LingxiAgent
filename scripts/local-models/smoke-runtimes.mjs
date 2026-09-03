#!/usr/bin/env node
/**
 * 运行时冒烟：用组装好的运行时真实加载两个模型并各跑一次推理。
 * - embed：llama.cpp sidecar（官方 CPU 构建即可跑，验证 wrapper/协议/探测链路）
 * - tts：audio.cpp sidecar（darwin 用 metal 变体，其余平台用 cpu 变体）
 *
 * 用法：node scripts/local-models/smoke-runtimes.mjs --runtime-root <build-runtimes 的输出目录>
 *   --gguf-embed <本地路径或URL> --gguf-tts <本地路径或URL> [--skip-tts] [--keep]
 */
import { execFile } from 'node:child_process';
import { createReadStream, createWriteStream } from 'node:fs';
import fsp from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import { promisify } from 'node:util';
import { pipeline } from 'node:stream/promises';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const execFileAsync = promisify(execFile);
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const require = createRequire(path.join(repoRoot, 'package.json'));
const jiti = require('jiti').createJiti(import.meta.url);

const args = new Map(process.argv.slice(2).reduce((pairs, value, index, all) => {
  if (value.startsWith('--')) pairs.push([value, all[index + 1] ?? true]);
  return pairs;
}, []));
const runtimeRoot = path.resolve(String(args.get('--runtime-root') ?? 'dist/local-runtimes'));
const embedSource = String(args.get('--gguf-embed') ?? '');
const ttsSource = String(args.get('--gguf-tts') ?? '');
const skipTts = Boolean(args.get('--skip-tts'));
const keep = Boolean(args.get('--keep'));
const platform = `${process.platform}-${process.arch}`;
await fsp.mkdir('dist', { recursive: true }).catch(() => {});

const { InstanceManager } = await jiti.import(path.join(repoRoot, 'lib/local-models/instance-manager.ts'));
const { SidecarInstanceFactory } = await jiti.import(path.join(repoRoot, 'lib/local-models/sidecar-factory.ts'));
const { createFileProbeCache } = await jiti.import(path.join(repoRoot, 'lib/local-models/backend-probe.ts'));
const { MemoryGovernor } = await jiti.import(path.join(repoRoot, 'lib/local-models/memory-governor.ts'));
const { LargeSlot } = await jiti.import(path.join(repoRoot, 'lib/local-models/large-slot.ts'));
const { DEFAULT_LOCAL_MODELS_CONFIG, resolveLargeResidentCapacity } = await jiti.import(path.join(repoRoot, 'lib/local-models/config.ts'));
const { getAvailableMemoryMb } = await jiti.import(path.join(repoRoot, 'lib/local-models/runtime-service.ts'));
const { LocalModelRegistry } = await jiti.import(path.join(repoRoot, 'lib/local-models/registry.ts'));
const { SidecarManager } = await jiti.import(path.join(repoRoot, 'lib/local-models/sidecar-manager.ts'));
const { localModelKey } = await jiti.import(path.join(repoRoot, 'lib/local-models/contracts.ts'));

const work = keep ? await fsp.mkdtemp('smoke-') : await fsp.mkdtemp(path.join(path.resolve('dist'), 'smoke-'));
const config = () => structuredClone(DEFAULT_LOCAL_MODELS_CONFIG);
const signal = new AbortController().signal;

async function obtainGguf(source, name) {
  if (!source.startsWith('http')) return source;
  const destination = path.join(work, 'downloads', name);
  await fsp.mkdir(path.dirname(destination), { recursive: true });
  process.stdout.write(`downloading ${name}\n`);
  const response = await fetch(source, { redirect: 'follow', signal: AbortSignal.timeout(1800000) });
  if (!response.ok || !response.body) throw new Error(`HTTP ${response.status} for ${source}`);
  await pipeline(response.body, createWriteStream(destination));
  return destination;
}

function writeSineWav(filePath, { seconds = 1, rate = 22050 } = {}) {
  const frames = seconds * rate;
  const data = Buffer.alloc(frames * 2);
  for (let index = 0; index < frames; index++) {
    data.writeInt16LE(Math.round(Math.sin((index * 2 * Math.PI * 220) / rate) * 12000), index * 2);
  }
  const header = Buffer.alloc(44);
  header.write('RIFF', 0); header.writeUInt32LE(36 + data.length, 4); header.write('WAVE', 8);
  header.write('fmt ', 12); header.writeUInt32LE(16, 16); header.writeUInt16LE(1, 20); header.writeUInt16LE(1, 22);
  header.writeUInt32LE(rate, 24); header.writeUInt32LE(rate * 2, 28); header.writeUInt16LE(2, 32); header.writeUInt16LE(16, 34);
  header.write('data', 36); header.writeUInt32LE(data.length, 40);
  return fsp.writeFile(filePath, Buffer.concat([header, data]));
}

const registry = new LocalModelRegistry(path.join(work, 'models'));
async function importBundle(sourceDir, metadata) {
  await registry.scan({ signal });
  return registry.importUnmanagedDirectory(sourceDir, metadata, { signal });
}

async function main() {
  const bundles = [];
  const embedGguf = await obtainGguf(embedSource, 'embed.gguf');
  const embedStage = path.join(work, 'embed-src');
  await fsp.mkdir(embedStage, { recursive: true });
  await fsp.copyFile(embedGguf, path.join(embedStage, 'smoke-embed-q8.gguf'));
  bundles.push(await importBundle(embedStage, {
    category: 'embedding', id: 'smoke-embed', quant: 'q8', tier: 'small', estimatedPeakRssMb: 800,
    runtimeId: 'llama.cpp', runtimeVersion: 'b10621-cpu', runtimeKind: 'sidecar',
    runtimeArgs: ['llama-sidecar.mjs', '--threads', '{threads}', '--mmap', '{mmap}', '--mlock', '{mlock}'],
    capabilities: { category: 'embedding', modelFile: 'smoke-embed-q8.gguf', contextWindow: 4096 },
    licenseFile: null,
  }));

  let ttsEntry = null;
  if (!skipTts) {
    const ttsGguf = await obtainGguf(ttsSource, 'tts.gguf');
    const ttsStage = path.join(work, 'tts-src');
    await fsp.mkdir(path.join(ttsStage, 'voices'), { recursive: true });
    await fsp.copyFile(ttsGguf, path.join(ttsStage, 'smoke-tts-q8.gguf'));
    await writeSineWav(path.join(ttsStage, 'voices', 'smoke-ref.wav'));
    const audioVariant = process.platform === 'darwin' ? 'v0.7.1-metal' : 'v0.7.1-cpu';
    ttsEntry = await importBundle(ttsStage, {
      category: 'tts', id: 'smoke-tts', quant: 'q8', tier: 'large', estimatedPeakRssMb: 4500,
      runtimeId: 'audio.cpp', runtimeVersion: audioVariant, runtimeKind: 'sidecar',
      runtimeArgs: ['audio-sidecar.mjs', '--threads', '{threads}', '--mmap', '{mmap}', '--mlock', '{mlock}'],
      capabilities: { category: 'tts', modelFile: 'smoke-tts-q8.gguf', defaultVoice: 'smoke-ref' },
      licenseFile: null,
    });
  }

  const installed = registry.snapshot().models;
  const installedByKey = new Map(installed.map((entry) => [localModelKey({
    id: entry.id, quant: entry.quant, manifestVersion: entry.version,
  }), entry]));
  const sidecar = new SidecarInstanceFactory({
    runtimeRoot,
    logRoot: path.join(work, 'logs'),
    config,
    probeCache: createFileProbeCache(path.join(runtimeRoot, 'probe-cache.json')),
    // 无 GPU 的 CI runner 上 TTS 首次合成（含惰性加载）可达数分钟，默认 2min 必超时。
    createManager: (options) => new SidecarManager({ ...options, requestTimeoutMs: 30 * 60_000 }),
  });
  const capacity = resolveLargeResidentCapacity(config().maxLargeResident, 32 * 1024 * 1024 * 1024);
  const manager = new InstanceManager({
    loader: {
      load: async (spec, loadSignal) => sidecar.load(spec, installedByKey.get(localModelKey(spec)), loadSignal),
      unload: async (instance, spec, loadSignal) => sidecar.unload(instance, spec, loadSignal),
      getRssMb: (instance) => sidecar.rssMb(instance),
    },
    largeSlot: new LargeSlot(() => {}, { capacity }),
    memoryGovernor: new MemoryGovernor({ smallBudgetMb: config().memoryBudgetSmallMb, getAvailableMemoryMb: () => getAvailableMemoryMb() }),
    idleUnloadMs: config().idleUnloadMs,
  });

  const asDescriptor = (entry) => ({
    id: entry.id, quant: entry.quant, manifestVersion: entry.version,
    category: entry.category, tier: entry.tier, runtimeId: entry.runtimeId,
    runtimeVersion: entry.runtimeVersion, estimatedPeakRssMb: entry.estimatedPeakRssMb,
  });
  const model = (entry) => ({ id: entry.id, quant: entry.quant, manifestVersion: entry.version });

  let t0 = Date.now();
  const embedLease = await manager.acquire(asDescriptor(bundles[0]), { signal, priority: 'interactive' });
  const embedLoadMs = Date.now() - t0;
  t0 = Date.now();
  const embedded = await embedLease.instance.embed({ model: model(bundles[0]), signal, texts: ['runtime smoke'] });
  const embedInferMs = Date.now() - t0;
  if (!Array.isArray(embedded.vectors?.[0]) || embedded.vectors[0].length === 0) throw new Error('embed smoke failed');
  await embedLease.release();
  process.stdout.write(`embed OK: load=${embedLoadMs}ms infer=${embedInferMs}ms dims=${embedded.vectors[0].length}\n`);

  let ttsResult = null;
  let ttsTimings = null;
  if (ttsEntry) {
    t0 = Date.now();
    const ttsLease = await manager.acquire(asDescriptor(ttsEntry), { signal, priority: 'interactive' });
    const ttsLoadMs = Date.now() - t0;
    t0 = Date.now();
    ttsResult = await ttsLease.instance.synthesize({ model: model(ttsEntry), signal, text: 'Hello smoke.', sampleRate: 24000 });
    const ttsInferMs = Date.now() - t0;
    const audioView = Buffer.from(ttsResult.audio.buffer, ttsResult.audio.byteOffset, ttsResult.audio.byteLength);
    if (ttsResult.audio.length <= 44 || audioView.toString('latin1', 0, 4) !== 'RIFF') throw new Error(`tts smoke failed: bytes=${ttsResult.audio.length} head=${audioView.subarray(0, 12).toString('latin1')}`);
    process.stdout.write(`tts OK: load=${ttsLoadMs}ms infer=${ttsInferMs}ms bytes=${ttsResult.audio.length} rate=${ttsResult.sampleRate}\n`);
    await ttsLease.release();
    ttsTimings = { loadMs: ttsLoadMs, inferMs: ttsInferMs, bytes: ttsResult.audio.length, rate: ttsResult.sampleRate };
  }
  console.log(JSON.stringify({ smoke: 'PASS', platform, embed: { loadMs: embedLoadMs, inferMs: embedInferMs, dims: embedded.vectors[0].length }, ...(ttsTimings ? { tts: ttsTimings } : {}) }));
}

main().then(async () => {
  if (!keep) await fsp.rm(work, { recursive: true, force: true }).catch(() => {});
  process.exit(0);
}, async (error) => {
  console.error('SMOKE-FAILED', error);
  if (!keep) await fsp.rm(work, { recursive: true, force: true }).catch(() => {});
  process.exit(1);
});

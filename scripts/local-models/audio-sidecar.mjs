import { spawn } from 'node:child_process';
import fs from 'node:fs';
import fsp from 'node:fs/promises';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import process from 'node:process';
import readline from 'node:readline';
import { setTimeout as delay } from 'node:timers/promises';

// IndexTTS 2.5（audio.cpp 运行时）的受管 sidecar：把父进程标准输入转交给本机原生服务，不接受远程地址。
const root = import.meta.dirname;
const modelDir = process.env.LINGXI_LOCAL_MODEL_DIRECTORY;
const token = process.env.LINGXI_LOCAL_MODEL_TOKEN;
const backend = process.env.LINGXI_LOCAL_MODEL_BACKEND;
// 支持后端由组装器写进 runtime.json.backends；旧运行时退回 cpu/metal。
let supportedBackends = ['cpu', 'metal'];
try {
  const runtimeManifest = JSON.parse(fs.readFileSync(path.join(root, 'runtime.json'), 'utf8'));
  if (Array.isArray(runtimeManifest.backends) && runtimeManifest.backends.length > 0) supportedBackends = runtimeManifest.backends;
} catch { /* 旧运行时无清单可读：保持历史白名单 */ }
if (!modelDir || !/^[A-Za-z0-9_-]{32,256}$/.test(token ?? '') || !supportedBackends.includes(backend)) {
  throw new Error('本地运行时启动参数无效');
}
const metadata = JSON.parse(fs.readFileSync(path.join(modelDir, 'model.json'), 'utf8'));
const { category, capabilities } = metadata;
if (category !== 'tts') throw new Error('不支持的模型类别');
const ggufPath = (name) => {
  if (typeof name !== 'string' || path.basename(name) !== name || !name.endsWith('.gguf')) throw new Error('模型文件名无效');
  return path.join(modelDir, name);
};
const args = new Map();
for (let index = 2; index < process.argv.length; index += 2) args.set(process.argv[index], process.argv[index + 1]);
const threadValue = args.get('--threads') ?? 'auto';
const threads = threadValue === 'auto' ? 8 : Number(threadValue);
if (!Number.isInteger(threads) || threads < 1 || threads > 256) throw new Error('线程数无效');
const reservation = net.createServer();
await new Promise((resolve, reject) => { reservation.once('error', reject); reservation.listen(0, '127.0.0.1', resolve); });
const port = reservation.address().port;
await new Promise((resolve) => reservation.close(resolve));
// server 配置每次启动写入临时目录（模型路径为绝对路径），不进运行时文件清单。
const configDir = await fsp.mkdtemp(path.join(os.tmpdir(), 'audiocpp-cfg-'));
const configPath = path.join(configDir, 'server.json');
const family = typeof capabilities.family === 'string' && capabilities.family ? capabilities.family : 'index_tts2';
await fsp.writeFile(configPath, JSON.stringify({ models: [{ id: 'local-tts', family, path: ggufPath(capabilities.modelFile) }] }));
const serverArgs = [
  '--config', configPath, '--backend', backend,
  '--host', '127.0.0.1', '--port', String(port), '--threads', String(threads),
  '--voice-dir', path.join(modelDir, 'voices'), '--log-colors', 'off',
];
const serverBinary = `audiocpp_server${process.platform === 'win32' ? '.exe' : ''}`;
const server = spawn(path.join(root, serverBinary), serverArgs, {
  cwd: root, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
  env: { ...process.env, AUDIOCPP_API_KEY: token },
});
let exited = false;
let stopping = false;
let gpuConfirmed = false;
let ready = false;
const closed = new Promise((resolve) => server.once('close', () => { exited = true; resolve(); }));
server.on('error', () => { void shutdown(1); });
for (const stream of [server.stdout, server.stderr]) stream.on('data', (data) => {
  const line = data.toString();
  if (backend === 'metal' && /ggml_metal_(device|library)_init/.test(line)) gpuConfirmed = true;
  // cuda/vulkan 无本机可验证的日志锚点：依赖 audio.cpp 后端初始化失败即退出的硬失败语义。
  if (backend !== 'cpu' && backend !== 'metal' && new RegExp(`\\b${backend}\\b`).test(line)) gpuConfirmed = true;
  // 详细启动日志只用于核对硬件；禁记录合成正文。
  if (!ready) process.stderr.write(line.replaceAll(token, '[REDACTED]'));
});
const active = new Map();
const origin = `http://127.0.0.1:${port}`;
for (let attempt = 0; ; attempt++) {
  if (exited || attempt >= 550) { await shutdown(1); throw new Error('原生模型服务加载失败'); }
  try {
    const response = await fetch(`${origin}/health`, { redirect: 'error', signal: AbortSignal.timeout(500) });
    if (response.ok) break;
  } catch {}
  await delay(100);
}
if (backend !== 'cpu' && !gpuConfirmed) { await shutdown(1); throw new Error('未证实 GPU 后端加载，拒绝冒充 GPU 后端'); }
ready = true;
send({ type: 'ready', protocol: 1, token, runtimeId: metadata.runtimeId, runtimeVersion: metadata.runtimeVersion, backend, pid: server.pid });

const lines = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
lines.on('line', (line) => {
  if (Buffer.byteLength(line) > 16 * 1024 * 1024) return void shutdown(1);
  let request;
  try { request = JSON.parse(line); } catch { return void shutdown(1); }
  if (request.type === 'shutdown') return void shutdown(0);
  if (request.type === 'cancel') { active.get(request.id)?.abort(); return; }
  if (request.type !== 'request' || !Number.isSafeInteger(request.id) || active.has(request.id)) return void shutdown(1);
  const controller = new AbortController();
  active.set(request.id, controller);
  void invoke(request.method, request.payload, controller.signal).then(
    (result) => send({ type: 'response', id: request.id, ok: true, result }),
    () => send({ type: 'response', id: request.id, ok: false, error: { code: controller.signal.aborted ? 'ABORTED' : 'INFERENCE_FAILED', message: '本地模型推理失败或已取消' } }),
  ).finally(() => active.delete(request.id));
});
lines.on('close', () => { void shutdown(0); });
process.on('SIGTERM', () => { void shutdown(0); });
process.on('SIGINT', () => { void shutdown(0); });
server.once('close', () => { if (!stopping) process.exit(1); });

async function invoke(method, payload, signal) {
  if (!payload || typeof payload !== 'object') throw new Error('输入无效');
  if (method !== 'synthesize' || category !== 'tts') throw new Error('不支持的操作');
  if (typeof payload.text !== 'string' || payload.text.length < 1 || payload.text.length > 5000) throw new Error('合成文本超出限制');
  if (payload.sampleRate !== undefined && payload.sampleRate !== 16000 && payload.sampleRate !== 24000) throw new Error('采样率无效');
  const voice = typeof payload.voice === 'string' && payload.voice.length <= 128
    ? payload.voice
    : (typeof capabilities.defaultVoice === 'string' && capabilities.defaultVoice ? capabilities.defaultVoice : await firstVoice());
  const response = await fetch(`${origin}/v1/audio/speech`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ model: 'local-tts', input: payload.text, voice, response_format: 'wav' }), signal, redirect: 'error',
  });
  if (!response.ok) throw new Error(`原生推理状态 ${response.status}`);
  const wav = Buffer.from(await response.arrayBuffer());
  if (!wav.length) throw new Error('合成结果为空');
  const { sampleRate: nativeRate, pcm } = parseWavPcm(wav);
  const targetRate = payload.sampleRate ?? 24000;
  const samples = nativeRate === targetRate ? pcm : resamplePcm16(pcm, nativeRate, targetRate);
  return { sampleRate: targetRate, format: 'wav', audioBase64: encodeWav(samples, targetRate).toString('base64') };
}

async function firstVoice() {
  const response = await fetch(`${origin}/v1/audio/voices?model=local-tts`, { redirect: 'error', signal: AbortSignal.timeout(5000) });
  const body = await response.json();
  const voices = Array.isArray(body?.voices) ? body.voices.filter((v) => typeof v === 'string') : [];
  if (voices.length === 0) throw new Error('参考音色库为空');
  return voices[0];
}

function parseWavPcm(buffer) {
  if (buffer.length < 44 || buffer.toString('latin1', 0, 4) !== 'RIFF' || buffer.toString('latin1', 8, 12) !== 'WAVE') throw new Error('合成结果不是 WAV');
  let offset = 12, format = null, pcm = null;
  while (offset + 8 <= buffer.length) {
    const id = buffer.toString('latin1', offset, offset + 4);
    const size = buffer.readUInt32LE(offset + 4);
    if (id === 'fmt ') format = { channels: buffer.readUInt16LE(offset + 10), sampleRate: buffer.readUInt32LE(offset + 12), bits: buffer.readUInt16LE(offset + 22) };
    if (id === 'data') pcm = buffer.subarray(offset + 8, offset + 8 + size);
    offset += 8 + size + (size % 2);
  }
  if (!format || !pcm || format.channels !== 1 || format.bits !== 16) throw new Error('合成 WAV 格式不受支持（需要 16bit 单声道）');
  return { sampleRate: format.sampleRate, pcm };
}

function resamplePcm16(pcm, fromRate, toRate) {
  const input = new Int16Array(pcm.buffer, pcm.byteOffset, Math.floor(pcm.length / 2));
  const length = Math.max(1, Math.round(input.length * toRate / fromRate));
  const output = new Int16Array(length);
  for (let index = 0; index < length; index++) {
    const source = index * fromRate / toRate;
    const left = Math.floor(source);
    const right = Math.min(left + 1, input.length - 1);
    const blend = source - left;
    output[index] = Math.round(input[left] * (1 - blend) + input[right] * blend);
  }
  return output;
}

function encodeWav(samples, sampleRate) {
  const header = Buffer.alloc(44);
  const dataBytes = samples.length * 2;
  header.write('RIFF', 0); header.writeUInt32LE(36 + dataBytes, 4); header.write('WAVE', 8);
  header.write('fmt ', 12); header.writeUInt32LE(16, 16); header.writeUInt16LE(1, 20); header.writeUInt16LE(1, 22);
  header.writeUInt32LE(sampleRate, 24); header.writeUInt32LE(sampleRate * 2, 28); header.writeUInt16LE(2, 32); header.writeUInt16LE(16, 34);
  header.write('data', 36); header.writeUInt32LE(dataBytes, 40);
  return Buffer.concat([header, Buffer.from(samples.buffer, samples.byteOffset, dataBytes)]);
}

function send(message) { if (!stopping) process.stdout.write(`${JSON.stringify(message)}\n`); }
async function shutdown(code) {
  if (stopping) return;
  stopping = true;
  for (const controller of active.values()) controller.abort();
  server.kill('SIGTERM');
  const timer = setTimeout(() => server.kill('SIGKILL'), 500);
  await closed;
  clearTimeout(timer);
  await fsp.rm(configDir, { recursive: true, force: true }).catch(() => {});
  process.exit(code);
}

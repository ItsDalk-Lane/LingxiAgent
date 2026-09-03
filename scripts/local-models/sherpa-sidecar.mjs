import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import readline from 'node:readline';
import { createLocalModelRuntime } from './sherpa-runtime.mjs';

// 模型及原生内存只存在于该子进程。取消由父进程终止整个进程并确认退出。
const modelDirectory = process.env.LINGXI_LOCAL_MODEL_DIRECTORY;
const token = process.env.LINGXI_LOCAL_MODEL_TOKEN;
const backend = process.env.LINGXI_LOCAL_MODEL_BACKEND;
if (!modelDirectory || !/^[A-Za-z0-9_-]{32,256}$/.test(token ?? '') || backend !== 'cpu') {
  throw new Error('此语音运行包只启用 CPU，或启动参数无效');
}
const metadata = JSON.parse(fs.readFileSync(path.join(modelDirectory, 'model.json'), 'utf8'));
const runtimeMetadata = JSON.parse(fs.readFileSync(path.join(import.meta.dirname, 'runtime.json'), 'utf8'));
if (!['stt', 'tts'].includes(metadata.category) || runtimeMetadata.kind !== 'sidecar') throw new Error('语音运行包类别无效');
const threadValue = process.argv[2] ?? 'auto';
const threads = threadValue === 'auto' ? 4 : Number(threadValue);
if (!Number.isInteger(threads) || threads < 1 || threads > 256) throw new Error('线程数无效');
let stopping = false;
let runtime;
const controller = new AbortController();
const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
input.on('close', () => { void shutdown(); });
process.on('SIGTERM', () => { void shutdown(); });
process.on('SIGINT', () => { void shutdown(); });
runtime = await createLocalModelRuntime({ modelDirectory, threads, backend,
  capabilities: { ...metadata.capabilities, category: metadata.category }, signal: controller.signal });
if (stopping) await shutdown();
else send({ type: 'ready', protocol: 1, token, runtimeId: runtimeMetadata.id,
  runtimeVersion: runtimeMetadata.version, backend, pid: process.pid });

let busy = false;
input.on('line', (line) => {
  if (Buffer.byteLength(line) > 64 * 1024) return void shutdown(1);
  let message;
  try { message = JSON.parse(line); } catch { return void shutdown(1); }
  if (message.type === 'shutdown' || message.type === 'cancel') return void shutdown();
  if (stopping || busy || message.type !== 'request' || !Number.isSafeInteger(message.id)
    || !message.payload || typeof message.payload !== 'object') return void shutdown(1);
  const method = metadata.category === 'stt' ? 'transcribe' : 'synthesize';
  if (message.method !== method) return void shutdown(1);
  busy = true;
  void runtime[method]({ ...message.payload, signal: controller.signal }).then((result) => {
    if (method === 'synthesize') {
      if (result.audio.length > 47 * 1024 * 1024) throw new Error('语音输出超出通信上限');
      result = { sampleRate: result.sampleRate, format: result.format, audioBase64: Buffer.from(result.audio).toString('base64') };
    }
    send({ type: 'response', id: message.id, ok: true, result });
  }).catch(() => send({ type: 'response', id: message.id, ok: false,
    error: { code: 'INFERENCE_FAILED', message: '本地语音推理失败或已取消' } }))
    .finally(() => { busy = false; });
});

function send(message) { if (!stopping) process.stdout.write(`${JSON.stringify(message)}\n`); }
async function shutdown(code = 0) {
  if (stopping) return;
  stopping = true;
  controller.abort();
  // 包括加载中断和宿主管道断开；宽限期后结束整个子进程，不强杀宿主内线程。
  setTimeout(() => process.exit(code), 500);
  await runtime?.dispose();
  process.exit(code);
}

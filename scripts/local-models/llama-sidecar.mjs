import { spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import process from 'node:process';
import readline from 'node:readline';
import { setTimeout as delay } from 'node:timers/promises';

// 仅把受管父进程的标准输入转交给带随机令牌的本机原生服务，不接受远程地址。
const root = import.meta.dirname;
const modelDir = process.env.LINGXI_LOCAL_MODEL_DIRECTORY;
const token = process.env.LINGXI_LOCAL_MODEL_TOKEN;
const backend = process.env.LINGXI_LOCAL_MODEL_BACKEND;
// GPU 后端以二进制实际编译的后端为准：组装器把支持列表写进 runtime.json.backends；
// 旧运行时没有该字段时退回 cpu/metal（macOS 历史行为）。
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
if (!['embedding', 'ocr', 'stt'].includes(category)) throw new Error('不支持的模型类别');
const modelFile = (name) => {
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
const serverArgs = [
  '-m', modelFile(capabilities.modelFile), '--host', '127.0.0.1', '--port', String(port),
  '--offline', '--no-webui', '--no-agent', '--no-slots', '--no-ui-mcp-proxy', '--no-models-autoload',
  '--threads', String(threads), '--threads-http', '2', '--parallel', '1', '--ctx-size', '4096',
  '--fit', 'off', '--log-colors', 'off', '--verbosity', '4', '--flash-attn', 'auto', '--cache-ram', '0', '--gpu-layers', backend === 'cpu' ? '0' : 'all',
];
if (backend === 'cpu') serverArgs.push('--device', 'none', '--no-op-offload', '--no-kv-offload');
if (args.get('--mmap') === 'false') serverArgs.push('--no-mmap');
if (args.get('--mlock') === 'true') serverArgs.push('--mlock');
if (category === 'embedding') serverArgs.push('--embedding', '--pooling', 'last', '--batch-size', '4096', '--ubatch-size', '4096');
else {
  serverArgs.push('--mmproj', modelFile(capabilities.projectorFile));
  if (backend === 'cpu') serverArgs.push('--no-mmproj-offload');
}
const serverBinary = `llama-server${process.platform === 'win32' ? '.exe' : ''}`;
const server = spawn(path.join(root, serverBinary), serverArgs, {
  cwd: root, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
  env: { ...process.env, LLAMA_API_KEY: token },
});
let exited = false;
let stopping = false;
let gpuConfirmed = false;
let ready = false;
const closed = new Promise((resolve) => server.once('close', () => { exited = true; resolve(); }));
server.on('error', () => { void shutdown(1); });
for (const stream of [server.stdout, server.stderr]) stream.on('data', (data) => {
  const line = data.toString();
  // llama.cpp 核心对 Metal/CUDA/Vulkan 等 GPU 后端统一打印这条卸载日志；CPU 后端无需证实。
  if (backend !== 'cpu' && /offloaded [1-9][0-9]*\/[0-9]+ layers to GPU/.test(line)) gpuConfirmed = true;
  // 详细启动日志只用于核对硬件；推理期间禁记提示词和识别正文。
  if (!ready) process.stderr.write(line.replaceAll(token, '[REDACTED]'));
});
const active = new Map();
const origin = `http://127.0.0.1:${port}`;
const headers = { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' };
for (let attempt = 0; ; attempt++) {
  if (exited || attempt >= 550) { await shutdown(1); throw new Error('原生模型加载失败'); }
  try {
    const response = await fetch(`${origin}/health`, { headers, redirect: 'error', signal: AbortSignal.timeout(500) });
    if (response.ok) break;
  } catch {}
  await delay(100);
}
if (backend !== 'cpu' && !gpuConfirmed) { await shutdown(1); throw new Error('未证实 GPU 权重加载，拒绝冒充 GPU 后端'); }
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
  if (method === 'embed' && category === 'embedding') {
    if (!Array.isArray(payload.texts) || payload.texts.length < 1 || payload.texts.length > 32
      || payload.texts.some((text) => typeof text !== 'string' || text.length > 8192)) throw new Error('嵌入文本超出限制');
    const input = payload.texts.map((text) => payload.inputType === 'query'
      ? `Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery:${text}` : text);
    const body = await post('/v1/embeddings', { input, encoding_format: 'float' }, signal);
    if (!Array.isArray(body.data) || body.data.length !== input.length) throw new Error('嵌入结果数量不匹配');
    const vectors = body.data.sort((a, b) => a.index - b.index).map((entry) => entry.embedding);
    return { vectors, dimensions: vectors[0].length, modelKey: `local:${metadata.id}@${metadata.quant}@${metadata.version}` };
  }
  if (method === 'ocr' && category === 'ocr') {
    if (!['image/png', 'image/jpeg', 'image/webp'].includes(payload.mime)
      || typeof payload.imageBase64 !== 'string' || payload.imageBase64.length > 12 * 1024 * 1024
      || !/^[A-Za-z0-9+/]*={0,2}$/.test(payload.imageBase64)) throw new Error('图片输入无效');
    const body = await post('/v1/chat/completions', { messages: [{ role: 'user', content: [
      { type: 'text', text: 'Text Recognition:' },
      { type: 'image_url', image_url: { url: `data:${payload.mime};base64,${payload.imageBase64}` } },
    ] }], temperature: 0, max_tokens: 2048, stream: false }, signal);
    const text = body.choices?.[0]?.message?.content;
    if (typeof text !== 'string' || body.choices[0].finish_reason === 'length') throw new Error('识字结果无效或被截断');
    return { markdown: text, text, format: 'ocr', warnings: ['ocr:glm-ocr'] };
  }
  if (method === 'transcribe' && category === 'stt') {
    if (typeof payload.filePath !== 'string' || payload.filePath.length > 4096
      || payload.filePath !== path.resolve(payload.filePath)) throw new Error('音频路径无效');
    if (payload.language !== undefined && (typeof payload.language !== 'string' || payload.language.length > 32)) throw new Error('语言参数无效');
    const stat = await fs.promises.stat(payload.filePath).catch(() => null);
    if (!stat?.isFile() || stat.size < 128 || stat.size > 100 * 1024 * 1024) throw new Error('音频文件无效或超出大小限制');
    const form = new FormData();
    form.append('file', new Blob([await fs.promises.readFile(payload.filePath)], { type: payload.mime || 'audio/wav' }), 'audio');
    if (payload.language) form.append('language', payload.language);
    form.append('response_format', 'json');
    // Qwen3-ASR 生成以「language XX<asr_text>」开头，剥离后才是纯转写正文。
    const response = await fetch(`${origin}/v1/audio/transcriptions`, {
      method: 'POST', headers: { Authorization: `Bearer ${token}` }, body: form, signal, redirect: 'error',
    });
    if (!response.ok) throw new Error(`原生推理状态 ${response.status}`);
    const body = await response.json();
    const raw = typeof body?.text === 'string' ? body.text : '';
    const match = raw.match(/^\s*language\s+([A-Za-z+-]*)\s*<asr_text>\s*/i);
    const text = match ? raw.slice(match[0].length) : raw;
    if (!text.trim()) throw new Error('转写结果无效');
    return { text, ...(match?.[1] ? { language: match[1].toLowerCase() } : {}) };
  }
  throw new Error('不支持的操作');
}

async function post(endpoint, payload, signal) {
  const response = await fetch(origin + endpoint, { method: 'POST', headers, body: JSON.stringify(payload), signal, redirect: 'error' });
  if (!response.ok) throw new Error(`原生推理状态 ${response.status}`);
  return response.json();
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
  process.exit(code);
}

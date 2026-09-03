import { Worker } from 'node:worker_threads';
import process from 'node:process';

// 该模块随经校验的原生运行时安装；工作线程退出后释放其原生模型句柄。
export async function probeBackend({ backend }) {
  return { available: backend === 'cpu', reason: backend === 'cpu' ? undefined : '此运行包只启用 CPU' };
}

export async function createLocalModelRuntime(options) {
  if (options.backend !== 'cpu') throw new Error('此运行包只启用 CPU');
  let worker;
  let ready;
  let serial = 0;
  let disposed = false;
  let stopping;
  let queue = Promise.resolve();
  const cancellation = new Int32Array(new SharedArrayBuffer(4));
  const pending = new Map();
  const failAll = (error) => {
    for (const entry of pending.values()) entry.reject(error);
    pending.clear();
  };
  const stop = () => {
    if (stopping) return stopping;
    const previous = worker;
    worker = undefined;
    ready = undefined;
    stopping = (async () => {
      if (previous) await new Promise((resolve) => {
        previous.once('exit', resolve);
        previous.postMessage({ type: 'dispose' });
      });
      failAll(new Error('本地语音工作线程已停止'));
    })().finally(() => { stopping = undefined; });
    return stopping;
  };
  const start = () => {
    if (disposed) throw new Error('本地语音运行时已释放');
    if (ready) return ready;
    const current = new Worker(new URL('./sherpa-worker.mjs', import.meta.url), {
      workerData: { modelDirectory: options.modelDirectory, threads: options.threads, capabilities: options.capabilities, cancellation: cancellation.buffer },
    });
    worker = current;
    ready = new Promise((resolve, reject) => {
      current.on('message', (message) => {
        if (message.type === 'ready') return resolve();
        if (message.type === 'error' && !message.id) return reject(new Error(message.error));
        const entry = pending.get(message.id);
        if (!entry) return;
        pending.delete(message.id);
        if (message.type === 'error') entry.reject(new Error(message.error));
        else entry.resolve(message.result);
      });
      current.on('error', (error) => { reject(error); failAll(error); });
      current.on('exit', () => {
        if (worker === current) { worker = undefined; ready = undefined; }
        const error = new Error('本地语音工作线程已退出');
        reject(error);
        failAll(error);
      });
    });
    return ready;
  };
  const withAbort = async (signal, execute) => {
    signal.throwIfAborted();
    Atomics.store(cancellation, 0, 0);
    const abort = () => Atomics.store(cancellation, 0, 1);
    signal.addEventListener('abort', abort, { once: true });
    // 原生调用中强杀线程会使 N-API 访问失效环境；协作取消后等待原生回调收尾。
    try { const result = await execute(); signal.throwIfAborted(); return result; }
    finally { signal.removeEventListener('abort', abort); }
  };
  try { await withAbort(options.signal, start); }
  catch (error) { await stop(); throw error; }

  const call = (method, payload, signal) => {
    const result = queue.then(() => withAbort(signal, async () => {
      await stopping;
      await start();
      signal.throwIfAborted();
      const id = ++serial;
      return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
        worker.postMessage({ id, method, payload });
      });
    }));
    queue = result.catch(() => {});
    return result;
  };
  return {
    backend: 'cpu',
    rssMb: () => process.memoryUsage().rss / 1024 / 1024,
    transcribe: options.capabilities.category === 'stt'
      ? (request) => call('transcribe', { filePath: request.filePath, mime: request.mime, language: request.language }, request.signal)
      : undefined,
    synthesize: options.capabilities.category === 'tts'
      ? async (request) => {
        const output = await call('synthesize', { text: request.text, voice: request.voice, sampleRate: request.sampleRate }, request.signal);
        request.signal.throwIfAborted();
        await request.onChunk?.(output.audio);
        return output;
      }
      : undefined,
    dispose: async () => { disposed = true; await queue; await stop(); },
  };
}

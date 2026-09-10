import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { Hono } from 'hono';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createMediaRoute } from '../server/routes/media.ts';
import { execute as generateSpeechTool } from '../plugins/media/tools/generate-speech.ts';
import { resolveSpeechParameters } from '../core/media/media-parameters.ts';
import * as speechAdapters from '../core/media-adapters/speech.ts';
import { SpeechRecognitionService } from '../core/speech-recognition-service.ts';
import { SessionFileRegistry } from '../lib/session-files/session-file-registry.ts';
import { setModelCallObserver } from '../lib/llm/model-call-observer.ts';
import { createTestModelCallObserver } from '../lib/llm/model-call-observer-testing.ts';

const roots: string[] = [];
function root() { const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-speech-contract-')); roots.push(dir); return dir; }
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: Error) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
afterEach(() => { vi.unstubAllGlobals(); setModelCallObserver(null); for (const dir of roots.splice(0)) fs.rmSync(dir, { recursive: true, force: true }); });

describe('语音入口、有效参数与真实产物', () => {
  it('HTTP TTS 把请求取消信号作为独立运行时上下文交给 manager', async () => {
    const generateSpeechFromBus = vi.fn(async () => ({ ok: true }));
    const app = new Hono(); app.route('/api', createMediaRoute({ media: { generateSpeechFromBus } }));
    const controller = new AbortController();
    const request = new Request('http://local/api/media/speech/generate', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ prompt: '测试', signal: 'body-cannot-own-runtime' }), signal: controller.signal });
    const response = await app.request(request);
    expect(response.status).toBe(200);
    expect(generateSpeechFromBus).toHaveBeenCalledWith({ prompt: '测试', signal: 'body-cannot-own-runtime' }, { signal: request.signal });
  });

  it.each([undefined, null, '', 0, { invalid: true }])('相同显式值 %j 经 REST 或工具到唯一解析器，结果一致', async value => {
    const resolved: any[] = [];
    const run = async (payload: any) => {
      resolved.push(resolveSpeechParameters({ protocolId: 'openai-audio-speech', executionTarget: { modelId: 'tts-test' }, explicitInput: payload.input ?? payload, speechProviderDefaults: { voice: 'echo', speed: 1.5, format: 'wav' } }));
      return { ok: true, tasks: [{ taskId: 'synthetic-task' }] };
    };
    const input = { prompt: '测试', voice: value, speed: value, format: value };
    const app = new Hono(); app.route('/api', createMediaRoute({ media: { generateSpeechFromBus: run } }));
    const response = await app.request('/api/media/speech/generate', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(input) });
    expect(response.status).toBe(200);
    await generateSpeechTool(input, { bus: { request: (_type: string, payload: any) => run(payload) } });
    expect(resolved).toHaveLength(2);
    expect(resolved[1]).toEqual(resolved[0]);
  });

  it.each([
    ['openaiSpeechAdapter', 'openai-audio-speech'],
    ['minimaxSpeechAdapter', 'minimax-tts'],
    ['dashscopeSpeechAdapter', 'dashscope-qwen-tts'],
  ])('%s 的每一次网络请求都使用 ctx.signal', async (name, protocolId) => {
    const controller = new AbortController();
    const network = vi.fn(async (url: any) => String(url).includes('t2a_v2')
      ? new Response(JSON.stringify({ base_resp: { status_code: 0 }, data: { audio: '0011' } }))
      : String(url).includes('multimodal-generation')
        ? new Response(JSON.stringify({ output: { audio: { url: 'https://audio.invalid/sample.wav' } } }))
        : new Response(new Uint8Array([0, 1, 2, 3])));
    vi.stubGlobal('fetch', network);
    await (speechAdapters as any)[name].submit({ prompt: '测试', modelId: 'model', groupId: 'group' }, {
      dataDir: root(), signal: controller.signal,
      mediaExecutionTarget: { protocolId, modelId: 'model', credentialProviderId: 'provider' },
      bus: { request: async () => ({ apiKey: 'synthetic-key', baseUrl: 'https://provider.invalid/v1' }) },
    });
    expect(network).toHaveBeenCalled();
    for (const call of network.mock.calls as any[]) expect(call[1].signal).toBe(controller.signal);
  });

  it('MiniMax PCM 原始字节保存为 pcm，并按原始二进制类型提供下载', async () => {
    const dir = root(); const bytes = Buffer.from([0, 1, 127, 255, 0, 0]);
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ base_resp: { status_code: 0 }, data: { audio: bytes.toString('hex') } }))));
    const result = await speechAdapters.minimaxSpeechAdapter.submit({ prompt: '测试', modelId: 'model', groupId: 'group', format: 'pcm' }, {
      dataDir: dir, mediaExecutionTarget: { credentialProviderId: 'minimax' },
      bus: { request: async () => ({ apiKey: 'synthetic-key', baseUrl: 'https://provider.invalid/v1' }) },
    });
    expect(result.files[0]).toMatch(/\.pcm$/);
    const filePath = path.join(dir, 'generated', result.files[0]);
    expect(fs.readFileSync(filePath)).toEqual(bytes);
    const app = new Hono(); app.route('/api', createMediaRoute({ media: { generatedFilePath: () => filePath } }));
    const response = await app.request(`/api/media/generated/${result.files[0]}`);
    expect(response.status).toBe(200);
    expect(response.headers.get('content-type')).toBe('application/octet-stream');
    expect(Buffer.from(await response.arrayBuffer())).toEqual(bytes);
  });
});

describe('相同会话文件的 ASR 尝试所有权', () => {
  it.each(['late-success', 'late-error'])('旧尝试 %s 不覆盖新结果和事件；各自观测如实结算', async oldOutcome => {
    const dir = root(); const sessionPath = path.join(dir, 'session.jsonl'); const audioPath = path.join(dir, 'voice.wav');
    fs.writeFileSync(sessionPath, '{}\n'); fs.writeFileSync(audioPath, 'RIFF');
    const files = new SessionFileRegistry();
    const file = files.registerFile({ sessionPath, filePath: audioPath, presentation: 'voice-input', origin: 'voice_input' });
    const pending = [deferred<any>(), deferred<any>()]; const entered = [deferred<void>(), deferred<void>()]; const signals: any[] = [];
    let count = 0;
    const emitEvent = vi.fn(); const observer = createTestModelCallObserver(); setModelCallObserver(observer);
    const service = new SpeechRecognitionService({
      providerRegistry: {
        resolveMediaModel: () => ({ providerId: 'synthetic', provider: {}, model: { id: 'asr', protocolId: 'synthetic-asr' } }),
        resolveMediaExecutionTarget: () => ({ modelId: 'asr', credentialProviderId: 'synthetic' }),
      },
      resolveProviderCredentialsFresh: async () => ({}),
      preferences: { getSpeechRecognitionConfig: () => ({ enabled: true, defaultModel: { provider: 'synthetic', id: 'asr' } }) },
      sessionFiles: files, emitEvent,
      adapters: [{ id: 'synthetic', protocolId: 'synthetic-asr', types: ['speechRecognition'], transcribe: (payload: any) => { const index = count++; signals.push(payload.signal); entered[index].resolve(); return pending[index].promise; } }],
    });
    const first = service.transcribeVoiceAttachment({ sessionPath, fileId: file.id });
    await entered[0].promise;
    const second = service.transcribeAudio({ sessionPath, fileId: file.id });
    await entered[1].promise;
    pending[1].resolve({ text: '新结果' });
    expect(await second).toMatchObject({ status: 'ready', text: '新结果' });
    const emitted = emitEvent.mock.calls.length;
    if (oldOutcome === 'late-error') pending[0].reject(new Error('旧尝试失败'));
    else pending[0].resolve({ text: '旧结果' });
    expect(await first).toMatchObject({ status: 'skipped', reason: 'superseded' });
    expect(signals[0].aborted).toBe(true);
    expect(files.get(file.id, { sessionPath })?.transcription).toMatchObject({ status: 'ready', text: '新结果' });
    expect(emitEvent).toHaveBeenCalledTimes(emitted);
    const ends = observer.eventsOfType('logical_call_end');
    expect(ends).toHaveLength(2);
    expect(ends.map(event => event.status).sort()).toEqual(oldOutcome === 'late-error' ? ['error', 'ok'] : ['ok', 'ok']);
  });
});

describe('系统 TTS 子进程取消有界回收', () => {
  it('忽略 SIGTERM 的真实合成进程必须退出后，取消调用才结算', async () => {
    const run = (speechAdapters as any).runSystemSpeechProcess;
    expect(run).toBeTypeOf('function');
    const dir = root(); const ready = path.join(dir, 'ready'); const controller = new AbortController();
    const script = `require('node:fs').writeFileSync(${JSON.stringify(ready)},String(process.pid)); process.on('SIGTERM',()=>{}); setInterval(()=>{},1000);`;
    const result = run(process.execPath, ['-e', script], { signal: controller.signal, timeoutMs: 5000, killGraceMs: 30 }).then(() => null, (error: Error) => error);
    await vi.waitFor(() => expect(fs.existsSync(ready)).toBe(true), { timeout: 5000, interval: 10 });
    const pid = Number(fs.readFileSync(ready, 'utf8'));
    controller.abort();
    const error = await result;
    expect(error).toMatchObject({ name: 'AbortError' });
    expect(() => process.kill(pid, 0)).toThrow();
  });
});

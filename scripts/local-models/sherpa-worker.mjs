import { parentPort, workerData } from 'node:worker_threads';
import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';
import { splitSpeechText, splitSpeechSamples } from './speech-segments.mjs';

const require = createRequire(import.meta.url);
const sherpa = require('sherpa-onnx-node');
const { modelDirectory, capabilities } = workerData;
const threads = workerData.threads === 'auto' ? 4 : workerData.threads;
const provider = `cpu:${path.join(import.meta.dirname, 'sherpa-cpu.config')}`;
const cancellation = new Int32Array(workerData.cancellation);
const file = (name) => path.join(modelDirectory, name);
let model;
try {
  if (capabilities.category === 'stt') {
    model = new sherpa.OfflineRecognizer({
      featConfig: { sampleRate: 16000, featureDim: 80 },
      modelConfig: { senseVoice: { model: file('model.int8.onnx'), language: 'auto', useInverseTextNormalization: 1 },
        tokens: file('tokens.txt'), numThreads: threads, provider, debug: 0 },
    });
  } else if (capabilities.category === 'tts') {
    model = new sherpa.OfflineTts({
      model: { kokoro: { model: file('model.onnx'), voices: file('voices.bin'), tokens: file('tokens.txt'),
        dataDir: file('espeak-ng-data'), dictDir: file('dict'),
        lexicon: [file('lexicon-us-en.txt'), file('lexicon-zh.txt')].join(',') },
        numThreads: threads, provider, debug: 0 },
      ruleFsts: ['phone-zh.fst', 'date-zh.fst', 'number-zh.fst'].map(file).join(','),
      maxNumSentences: 1,
    });
  } else throw new Error('不支持的语音类别');
  parentPort.postMessage({ type: 'ready' });
} catch (error) {
  parentPort.postMessage({ type: 'error', error: error.message });
}

parentPort.on('message', async ({ id, method, payload, type }) => {
  if (type === 'dispose') { model = undefined; parentPort.close(); return; }
  try {
    if (!model) throw new Error('模型未就绪');
    let result;
    if (method === 'transcribe' && capabilities.category === 'stt') {
      const stat = fs.statSync(payload.filePath);
      if (!stat.isFile() || stat.size > 64 * 1024 * 1024) throw new Error('音频文件超出限制');
      const header = Buffer.alloc(12);
      const fd = fs.openSync(payload.filePath, 'r');
      try { fs.readSync(fd, header); } finally { fs.closeSync(fd); }
      if (header.toString('ascii', 0, 4) !== 'RIFF' || header.toString('ascii', 8, 12) !== 'WAVE') {
        throw new Error('当前原生语音包仅接收 WAV，其他录音格式需要先解码');
      }
      const wave = sherpa.readWave(payload.filePath);
      if (wave.samples.length / wave.sampleRate > 600) throw new Error('音频时长超出限制');
      const texts = [];
      let language;
      for (const samples of splitSpeechSamples(wave.samples, wave.sampleRate)) {
        checkCancelled();
        const stream = model.createStream();
        stream.acceptWaveform({ samples, sampleRate: wave.sampleRate });
        model.decode(stream);
        checkCancelled();
        const decoded = model.getResult(stream);
        texts.push(decoded.text);
        language ??= decoded.lang ?? decoded.language;
      }
      result = { text: texts.filter(Boolean).join(' '), language: typeof language === 'string' ? language.replace(/^<\||\|>$/g, '') : undefined, durationMs: wave.samples.length / wave.sampleRate * 1000 };
    } else if (method === 'synthesize' && capabilities.category === 'tts') {
      if (typeof payload.text !== 'string' || !payload.text.trim() || payload.text.length > 4096) throw new Error('朗读文本超出限制');
      const sid = payload.voice === undefined ? 45 : Number(payload.voice);
      if (!Number.isInteger(sid) || sid < 0 || sid >= model.numSpeakers) throw new Error('音色编号无效');
      if (payload.sampleRate && payload.sampleRate !== 24000) throw new Error('当前语音包仅支持 24000 Hz');
      const chunks = [];
      let length = 0;
      for (const text of splitSpeechText(payload.text)) {
        checkCancelled();
        if (!text.trim()) continue;
        const audio = await model.generateAsync({ text, sid, speed: 1,
          onProgress: () => Atomics.load(cancellation, 0) === 0 });
        checkCancelled();
        if (audio.sampleRate !== 24000) throw new Error('运行时返回非预期采样率');
        chunks.push(audio.samples); length += audio.samples.length;
      }
      const samples = new Float32Array(length);
      let offset = 0;
      for (const chunk of chunks) { samples.set(chunk, offset); offset += chunk.length; }
      result = { sampleRate: 24000, format: 'wav', audio: encodeWave(samples, 24000) };
    } else throw new Error('不支持的语音操作');
    parentPort.postMessage({ type: 'result', id, result });
  } catch (error) { parentPort.postMessage({ type: 'error', id, error: error.message }); }
});

function checkCancelled() {
  if (Atomics.load(cancellation, 0) !== 0) throw new Error('本地语音任务已取消');
}

function encodeWave(samples, sampleRate) {
  const bytes = Buffer.alloc(44 + samples.length * 2);
  bytes.write('RIFF'); bytes.writeUInt32LE(bytes.length - 8, 4); bytes.write('WAVEfmt ', 8);
  bytes.writeUInt32LE(16, 16); bytes.writeUInt16LE(1, 20); bytes.writeUInt16LE(1, 22);
  bytes.writeUInt32LE(sampleRate, 24); bytes.writeUInt32LE(sampleRate * 2, 28);
  bytes.writeUInt16LE(2, 32); bytes.writeUInt16LE(16, 34); bytes.write('data', 36);
  bytes.writeUInt32LE(samples.length * 2, 40);
  for (let i = 0; i < samples.length; i++) bytes.writeInt16LE(Math.round(Math.max(-1, Math.min(1, samples[i])) * 32767), 44 + i * 2);
  return bytes;
}

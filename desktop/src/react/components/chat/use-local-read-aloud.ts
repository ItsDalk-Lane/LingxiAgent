import { useCallback, useEffect, useRef, useState } from 'react';
import { lingxiFetch } from '../../hooks/use-hana-fetch';

export type ReadAloudState = 'idle' | 'loading' | 'playing' | 'paused';

let activePlayback: { id: symbol; stop: () => void } | null = null;

/**
 * 本地朗读控制器：按句分批合成，优先让第一句尽快开始播放。
 * 任意时刻只保留一条消息在朗读，停止或卸载组件时会中止请求并释放音频 URL。
 */
export function useLocalReadAloud(input: {
  text: string;
  sessionPath: string;
  onError: (message: string) => void;
}) {
  const [state, setState] = useState<ReadAloudState>('idle');
  const identityRef = useRef(Symbol('local-read-aloud'));
  const generationRef = useRef(0);
  const controllerRef = useRef<AbortController | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const objectUrlRef = useRef<string | null>(null);
  const finishCurrentRef = useRef<(() => void) | null>(null);
  const onErrorRef = useRef(input.onError);
  onErrorRef.current = input.onError;

  const releaseAudio = useCallback(() => {
    const audio = audioRef.current;
    if (audio) {
      audio.onended = null;
      audio.onerror = null;
      audio.pause();
      audio.removeAttribute('src');
      audio.load();
      audioRef.current = null;
    }
    if (objectUrlRef.current) {
      URL.revokeObjectURL(objectUrlRef.current);
      objectUrlRef.current = null;
    }
  }, []);

  const stop = useCallback(() => {
    generationRef.current += 1;
    controllerRef.current?.abort();
    controllerRef.current = null;
    finishCurrentRef.current?.();
    finishCurrentRef.current = null;
    releaseAudio();
    setState('idle');
    if (activePlayback?.id === identityRef.current) activePlayback = null;
  }, [releaseAudio]);

  useEffect(() => stop, [stop]);

  const run = useCallback(async () => {
    const chunks = sentenceChunks(input.text);
    if (chunks.length === 0) return;
    activePlayback?.stop();
    const generation = generationRef.current + 1;
    generationRef.current = generation;
    activePlayback = { id: identityRef.current, stop };
    const controller = new AbortController();
    controllerRef.current = controller;
    try {
      for (const text of chunks) {
        if (generationRef.current !== generation) return;
        setState('loading');
        const response = await lingxiFetch('/api/media/tts/synthesize', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ text, sessionPath: input.sessionPath, surface: 'desktop-chat' }),
          signal: controller.signal,
          timeout: 120_000,
        });
        const payload = await response.json() as {
          audio?: unknown;
          encoding?: unknown;
          format?: unknown;
          sampleRate?: unknown;
        };
        if (payload.encoding !== 'base64' || typeof payload.audio !== 'string') {
          throw new Error('invalid local speech response');
        }
        const raw = decodeBase64(payload.audio);
        const playable = payload.format === 'pcm_s16le'
          ? pcm16ToWav(raw, numericSampleRate(payload.sampleRate))
          : raw;
        const audioBytes = Uint8Array.from(playable);
        const blob = new Blob([audioBytes.buffer], { type: 'audio/wav' });
        const objectUrl = URL.createObjectURL(blob);
        objectUrlRef.current = objectUrl;
        const audio = new Audio(objectUrl);
        audioRef.current = audio;
        await new Promise<void>((resolve, reject) => {
          finishCurrentRef.current = resolve;
          audio.onended = () => resolve();
          audio.onerror = () => reject(new Error('local speech playback failed'));
          void audio.play().then(() => setState('playing'), reject);
        });
        finishCurrentRef.current = null;
        releaseAudio();
      }
      if (generationRef.current === generation) {
        controllerRef.current = null;
        setState('idle');
        if (activePlayback?.id === identityRef.current) activePlayback = null;
      }
    } catch (error) {
      if (generationRef.current !== generation || controller.signal.aborted) return;
      releaseAudio();
      controllerRef.current = null;
      setState('idle');
      if (activePlayback?.id === identityRef.current) activePlayback = null;
      onErrorRef.current(error instanceof Error ? error.message : String(error));
    }
  }, [input.sessionPath, input.text, releaseAudio, stop]);

  const toggle = useCallback(() => {
    if (state === 'idle') {
      void run();
      return;
    }
    if (state === 'playing') {
      audioRef.current?.pause();
      setState('paused');
      return;
    }
    if (state === 'paused' && audioRef.current) {
      void audioRef.current.play().then(() => setState('playing')).catch((error) => {
        stop();
        onErrorRef.current(error instanceof Error ? error.message : String(error));
      });
    }
  }, [run, state, stop]);

  return { state, toggle, stop };
}

function sentenceChunks(text: string): string[] {
  const normalized = text.replace(/\s+/g, ' ').trim();
  if (!normalized) return [];
  const sentences = normalized.match(/[^。！？!?；;\n]+[。！？!?；;]?/g) || [normalized];
  const chunks: string[] = [];
  for (const sentence of sentences) {
    const value = sentence.trim();
    for (let offset = 0; offset < value.length; offset += 600) {
      const part = value.slice(offset, offset + 600).trim();
      if (part) chunks.push(part);
    }
  }
  return chunks;
}

function decodeBase64(value: string): Uint8Array {
  const binary = atob(value);
  const output = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) output[index] = binary.charCodeAt(index);
  return output;
}

function numericSampleRate(value: unknown): number {
  return value === 16000 || value === 24000 ? value : 24000;
}

function pcm16ToWav(pcm: Uint8Array, sampleRate: number): Uint8Array {
  const output = new Uint8Array(44 + pcm.byteLength);
  const view = new DataView(output.buffer);
  writeAscii(output, 0, 'RIFF');
  view.setUint32(4, 36 + pcm.byteLength, true);
  writeAscii(output, 8, 'WAVE');
  writeAscii(output, 12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  writeAscii(output, 36, 'data');
  view.setUint32(40, pcm.byteLength, true);
  output.set(pcm, 44);
  return output;
}

function writeAscii(target: Uint8Array, offset: number, value: string): void {
  for (let index = 0; index < value.length; index += 1) target[offset + index] = value.charCodeAt(index);
}

/**
 * MC-09 Speech Recognition × ModelCallObserver（§三十六～§四十二/§六十三）。
 *
 * 全部 4 个远端 adapter（OpenAI/MiMo/DashScope/Volcengine BigASR）经真实
 * 业务链路 SpeechRecognitionService.transcribeAudio → adapter.transcribe →
 * fake fetch → Observer。毒钉：音频字节、base64、transcription 正文、apiKey
 * 都不得进入事件；fileId/audioFormat/languageSpecified/inputSizeBucket 允许。
 * Volcengine 特别检查：credential 在 JSON body（user.uid）——body 绝不进事件。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SpeechRecognitionService } from "../core/speech-recognition-service.ts";
import { builtinSpeechRecognitionAdapters } from "../core/speech-recognition/adapters.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import { createTestModelCallObserver } from "../lib/llm/model-call-observer-testing.ts";
import { createUsageLedger } from "../lib/llm/usage-ledger.ts";

const POISON_AUDIO = "TOP_SECRET_AUDIO_BYTES_8F91C2";
const POISON_TRANSCRIPT = "TOP_SECRET_TRANSCRIPT_8F91C2";
const POISON_API_KEY = "sk-SPEECH-SECRET-KEY-8F91C2";

const roots: string[] = [];
function makeVoiceFile() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-speech-obs-"));
  roots.push(root);
  const filePath = path.join(root, "voice.wav");
  fs.writeFileSync(filePath, POISON_AUDIO.repeat(64));
  return { fileId: "file-voice-1", filePath, realPath: filePath, mime: "audio/wav" };
}

const ADAPTER_RESPONSES: Record<string, () => Response> = {
  openai: () => new Response(JSON.stringify({ text: POISON_TRANSCRIPT }), { status: 200, headers: { "x-request-id": "req-asr-openai" } }),
  mimo: () => new Response(JSON.stringify({ choices: [{ message: { content: POISON_TRANSCRIPT } }] }), { status: 200, headers: { "x-request-id": "req-asr-mimo" } }),
  dashscope: () => new Response(JSON.stringify({ choices: [{ message: { content: POISON_TRANSCRIPT } }] }), { status: 200, headers: { "x-request-id": "req-asr-dashscope" } }),
  "volcengine-speech": () => new Response(JSON.stringify({
    result: { text: POISON_TRANSCRIPT },
    audio_info: { duration: 2345 },
  }), { status: 200, headers: { "x-request-id": "req-asr-volc" } }),
};

const PROTOCOLS: Record<string, string> = {
  openai: "openai-audio-transcriptions",
  mimo: "mimo-chat-completions-asr",
  dashscope: "dashscope-qwen-asr-chat",
  "volcengine-speech": "volcengine-bigasr-transcription",
};

function makeService({ adapterId, file, ledger, fetchImpl }: {
  adapterId: string;
  file: any;
  ledger: any;
  fetchImpl: any;
}) {
  const protocolId = PROTOCOLS[adapterId];
  return new SpeechRecognitionService({
    providerRegistry: {
      resolveMediaModel: () => ({
        providerId: adapterId,
        provider: { providerId: adapterId },
        model: { id: "asr-model-1", protocolId },
        credentialLane: null,
      }),
      getMediaProviders: () => [],
    },
    resolveProviderCredentialsFresh: async () => ({ apiKey: POISON_API_KEY, baseUrl: "https://asr.test/v1" }),
    preferences: {
      getSpeechRecognitionConfig: () => ({ enabled: true, defaultModel: { provider: adapterId, id: "asr-model-1" } }),
    },
    sessionFiles: {
      get: (fileId: string) => (fileId === file.fileId ? file : null),
      updateTranscription: vi.fn((_fileId: string, patch: any) => ({ transcription: patch })),
    },
    emitEvent: vi.fn(),
    fetch: fetchImpl,
    adapters: builtinSpeechRecognitionAdapters,
    usageLedger: ledger,
  } as any);
}

describe("MC-09 speech recognition × ModelCallObserver", () => {
  let observer: ReturnType<typeof createTestModelCallObserver>;

  beforeEach(() => {
    observer = createTestModelCallObserver();
    setModelCallObserver(observer);
  });
  afterEach(() => {
    setModelCallObserver(null);
    vi.unstubAllGlobals();
    for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
  });

  for (const adapter of builtinSpeechRecognitionAdapters.filter((entry) => entry.id !== "local")) {
    it(`coverage: ${adapter.id} transcribe → observer 完整生命周期 + ledger 关联`, async () => {
      const file = makeVoiceFile();
      const ledger = createUsageLedger({});
      const fetchMock = vi.fn(async () => ADAPTER_RESPONSES[adapter.id]());
      const service = makeService({ adapterId: adapter.id, file, ledger, fetchImpl: fetchMock });

      const transcription = await service.transcribeAudio({
        fileId: file.fileId,
        sessionId: "sess-speech-1",
        sessionPath: "/sessions/speech.jsonl",
        language: "zh",
        providerId: adapter.id,
        modelId: "asr-model-1",
      } as any);

      // 业务行为不变：转写文本照常写入 SessionFile
      expect(transcription.status).toBe("ready");
      expect(transcription.text).toContain(POISON_TRANSCRIPT);

      const callId = observer.callIds()[0];
      expect(callId).toMatch(/^mc_/);
      observer.assertLifecycle(callId, [
        "logical_call_start",
        "attempt_start",
        "provider_request_prepared",
        "provider_response_received",
        "semantic_response_completed",
        "logical_call_end",
      ]);
      const start = observer.eventsOfType("logical_call_start")[0];
      expect(start.details).toMatchObject({
        path: "speech_transcribe",
        mediaType: "audio",
        protocol: PROTOCOLS[adapter.id],
        languageSpecified: true,
      });
      expect(start.attribution).toMatchObject({
        kind: "session",
        sessionId: "sess-speech-1",
        fileId: "file-voice-1",
      });
      // inputSizeBucket 只读 size，不复制音频
      const bucket = start.details?.inputSizeBucket;
      expect(bucket === null || typeof bucket === "string").toBe(true);

      const attempt = observer.eventsOfType("attempt_start")[0];
      expect(attempt.details).toMatchObject({ attemptVisibility: "exact", providerWireVisibility: "request_response" });
      expect(observer.eventsOfType("provider_request_prepared")[0].details)
        .toMatchObject({ protocol: PROTOCOLS[adapter.id], languageSpecified: true });
      expect(observer.eventsOfType("provider_response_received")[0].details).toMatchObject({ httpStatus: 200 });
      expect(observer.eventsOfType("semantic_response_completed")[0].details)
        .toMatchObject({ hasText: true, languagePresent: Boolean(transcription.language) });
      expect(observer.events.at(-1)).toMatchObject({ status: "ok" });

      // ledger 关联 + 单条（无双计）
      const entries = ledger.list({}).entries;
      expect(entries).toHaveLength(1);
      expect(entries[0].metadata).toMatchObject({ modelCallId: callId, capability: "speech_recognition" });

      // 毒丸红线：音频字节 / base64 形态 / 转写正文 / apiKey 不进事件
      observer.assertNoSensitiveContent([POISON_AUDIO, POISON_TRANSCRIPT, POISON_API_KEY]);
    });
  }

  it("HTTP 500（毒丸 error body）：attempt_error + logical error；业务失败态照常", async () => {
    const file = makeVoiceFile();
    const ledger = createUsageLedger({});
    const fetchMock = vi.fn(async () => new Response(
      JSON.stringify({ error: { message: POISON_TRANSCRIPT } }),
      { status: 500, headers: { "Content-Type": "application/json" } },
    ));
    const service = makeService({ adapterId: "openai", file, ledger, fetchImpl: fetchMock });

    const transcription = await service.transcribeAudio({
      fileId: file.fileId,
      sessionId: "sess-speech-1",
      providerId: "openai",
      modelId: "asr-model-1",
    } as any);

    expect(transcription.status).toBe("failed");
    const callId = observer.callIds()[0];
    observer.assertLifecycle(callId, [
      "logical_call_start",
      "attempt_start",
      "provider_request_prepared",
      "provider_response_received",
      "attempt_error",
      "logical_call_error",
      "logical_call_end",
    ]);
    expect(observer.eventsOfType("attempt_error")[0].details).toMatchObject({ errorKind: "http_error", httpStatus: 500 });
    expect(observer.events.at(-1)).toMatchObject({ status: "error" });
    expect(ledger.list({}).entries[0].status).toBe("error");
    observer.assertNoSensitiveContent([POISON_TRANSCRIPT, POISON_API_KEY, POISON_AUDIO]);
  });

  it("Volcengine 特别检查：credential 在 JSON body（user.uid）——绝不进事件（§四十一）", async () => {
    const file = makeVoiceFile();
    const ledger = createUsageLedger({});
    let sentBody = "";
    const fetchMock = vi.fn(async (_url: string, init: any) => {
      sentBody = String(init.body);
      return ADAPTER_RESPONSES["volcengine-speech"]();
    });
    const service = makeService({ adapterId: "volcengine-speech", file, ledger, fetchImpl: fetchMock });

    await service.transcribeAudio({
      fileId: file.fileId,
      sessionId: "sess-speech-1",
      providerId: "volcengine-speech",
      modelId: "asr-model-1",
    } as any);

    // 前提成立：请求 body 确实携带 credential（复现审计发现）
    expect(sentBody).toContain(POISON_API_KEY);
    // Observer 从一开始就只接收结构摘要——body（含 user.uid credential）不进事件
    observer.assertNoSensitiveContent([POISON_API_KEY, POISON_AUDIO, POISON_TRANSCRIPT]);
    expect(JSON.stringify(observer.eventsOfType("provider_request_prepared")[0].details))
      .not.toContain("uid");
  });
});

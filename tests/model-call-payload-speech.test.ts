/**
 * Phase 6 MC-09 Speech × Sensitive Payload Capture（§九十九/§一百/§一百零一/
 * §一百四十八/§一百四十九）。
 *
 * 4 个 active adapter 全覆盖：audio externalize、language 保留、credential
 * 替换（含 Volcengine body.user.uid 协议专项硬验收——不依赖 generic key
 * denylist）、provider transcription body 安全捕获、semantic transcription 捕获。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SpeechRecognitionService } from "../core/speech-recognition-service.ts";
import { builtinSpeechRecognitionAdapters } from "../core/speech-recognition/adapters.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import { setModelCallPayloadSink } from "../lib/llm/model-call-payload-capture.ts";
import { createTestModelCallPayloadSink, installTestPayloadSink } from "../lib/llm/model-call-payload-testing.ts";
import { createUsageLedger } from "../lib/llm/usage-ledger.ts";

const POISON_AUDIO = "TOP_SECRET_AUDIO_BYTES_8F91C2";
const POISON_API_KEY = "sk-SPEECH-SECRET-KEY-8F91C2";
const NORMAL_TRANSCRIPT = "NORMAL_SPEECH_TRANSCRIPT_VISIBLE 今天天气很好";

const roots: string[] = [];
function makeVoiceFile() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-speech-payload-"));
  roots.push(root);
  const filePath = path.join(root, "voice.wav");
  fs.writeFileSync(filePath, Buffer.from(POISON_AUDIO.repeat(64)));
  return { fileId: "file-voice-1", filePath, realPath: filePath, mime: "audio/wav" };
}

const ADAPTER_RESPONSES: Record<string, () => Response> = {
  openai: () => new Response(JSON.stringify({ text: NORMAL_TRANSCRIPT }), { status: 200, headers: { "x-request-id": "req-asr-openai" } }),
  mimo: () => new Response(JSON.stringify({ choices: [{ message: { content: NORMAL_TRANSCRIPT } }] }), { status: 200 }),
  dashscope: () => new Response(JSON.stringify({ choices: [{ message: { content: NORMAL_TRANSCRIPT } }] }), { status: 200 }),
  "volcengine-speech": () => new Response(JSON.stringify({
    result: { text: NORMAL_TRANSCRIPT },
    audio_info: { duration: 2345 },
  }), { status: 200, headers: { "X-Api-Status-Code": "20000000" } }),
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
  return new SpeechRecognitionService({
    providerRegistry: {
      resolveMediaExecutionTarget: (input: any) => ({
        modelId: input.modelId,
        modality: input.modality,
        runtimeProviderId: input.runtimeProviderId,
        credentialProviderId: input.runtimeProviderId,
        credentialLaneId: null,
        credentialSource: "provider-registry",
        adapterId: input.adapterId,
        resolutionReason: "runtime_provider_credentials",
      }),
      resolveMediaModel: () => ({
        providerId: adapterId,
        provider: { providerId: adapterId },
        model: { id: "asr-model-1", protocolId: PROTOCOLS[adapterId] },
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

describe("MC-09 speech × payload capture", () => {
  let sink: ReturnType<typeof createTestModelCallPayloadSink>;

  beforeEach(() => {
    sink = installTestPayloadSink();
    setModelCallObserver({ handleModelCallEvent() { /* 已有独立覆盖 */ } });
  });
  afterEach(() => {
    setModelCallObserver(null);
    setModelCallPayloadSink(null);
    vi.unstubAllGlobals();
    for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
  });

  for (const adapter of builtinSpeechRecognitionAdapters) {
    it(`coverage: ${adapter.id} 四层 capture：audio externalize + language 保留 + credential 替换 + transcription 捕获`, async () => {
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
      expect(transcription.text).toBe(NORMAL_TRANSCRIPT);

      const [callId] = sink.callIds();
      expect(sink.sequenceForCall(callId)).toEqual([
        "semantic_request",
        "provider_request",
        "provider_response",
        "semantic_response",
      ]);

      // semantic_request：audio 本地路径 descriptor 化，language 保留（§九十九）
      const semantic = sink.semanticRequestForCall(callId)!;
      const parameters = (semantic.payload as any).parameters;
      expect(parameters.language).toBe("zh");
      expect(parameters.audio).toMatchObject({ kind: "local_file_reference", basename: "voice.wav" });

      // provider_request：真实构造点 body；凭证替换；音频 externalize
      const transport = (sink.providerRequestsForCall(callId)[0].payload as any).transport;
      const serializedRequest = JSON.stringify(transport);
      expect(serializedRequest).not.toContain(POISON_API_KEY);
      expect(serializedRequest).not.toContain(POISON_AUDIO);
      if (adapter.id === "openai") {
        // FormData：multipart 形状，file Blob externalize（§一百四十八）
        expect(transport.body.kind).toBe("multipart_form_data");
        expect(transport.body.fields.model).toBe("asr-model-1");
        expect(transport.body.fields.language).toBe("zh");
        expect(transport.body.files[0]).toMatchObject({ field: "file", kind: "external_blob", mediaType: "audio/wav" });
        expect(transport.headers.Authorization).toBe("<redacted:credential>");
      } else if (adapter.id === "mimo") {
        expect(transport.headers["api-key"]).toBe("<redacted:credential>");
      } else if (adapter.id === "volcengine-speech") {
        // X-Api-Key header + body.user.uid 双凭证（§一百）
        expect(transport.headers["X-Api-Key"]).toBe("<redacted:credential>");
        expect(transport.body.user.uid).toBe("<redacted:credential>");
        expect(transport.body.audio.data).toMatchObject({ kind: "external_blob", encoding: "base64" });
      } else {
        expect(transport.headers.Authorization).toBe("<redacted:credential>");
      }

      // provider_response：sanitized parsed body（§一百零一）
      const response = sink.providerResponsesForCall(callId)[0];
      expect(response.fidelity).toBe("parsed_equivalent");
      expect(JSON.stringify(response.payload)).not.toContain(POISON_API_KEY);

      // semantic_response：transcription 正文捕获（§一百零一）
      const semanticResponse = sink.semanticResponseForCall(callId)!;
      expect((semanticResponse.payload as any).transcription).toBe(NORMAL_TRANSCRIPT);

      sink.assertNoSensitiveContent([POISON_API_KEY, POISON_AUDIO]);
    });
  }

  it("Volcengine 专项硬验收：body.user.uid 由协议规则处理，generic 'uid' 键不受影响（§一百四十九）", async () => {
    const file = makeVoiceFile();
    const fetchMock = vi.fn(async () => ADAPTER_RESPONSES["volcengine-speech"]());
    const service = makeService({ adapterId: "volcengine-speech", file, ledger: createUsageLedger({}), fetchImpl: fetchMock });
    await service.transcribeAudio({ fileId: file.fileId, sessionId: "sess-speech-1", sessionPath: "/sessions/speech.jsonl", language: "zh", providerId: "volcengine-speech", modelId: "asr-model-1" } as any);

    const [callId] = sink.callIds();
    const transport = (sink.providerRequestsForCall(callId)[0].payload as any).transport;
    expect(transport.body.user.uid).toBe("<redacted:credential>");
    expect(transport.body.request.model_name).toBe("bigmodel");
    const actions = sink.providerRequestsForCall(callId)[0].sanitization.actions;
    expect(actions.some((a) => a.reason === "protocol-body-credential")).toBe(true);
  });

  it("HTTP 500：error body 安全捕获", async () => {
    const file = makeVoiceFile();
    const fetchMock = vi.fn(async () => new Response(
      JSON.stringify({ message: "asr normal diagnostic", key: POISON_API_KEY }),
      { status: 500, headers: { "Content-Type": "application/json" } },
    ));
    const service = makeService({ adapterId: "mimo", file, ledger: createUsageLedger({}), fetchImpl: fetchMock });
    // mimo 500 → 业务返回 failed 结果（不 throw）；provider_response 仍被捕获
    await service.transcribeAudio({ fileId: file.fileId, sessionId: "sess-speech-1", sessionPath: "/sessions/speech.jsonl", language: "zh", providerId: "mimo", modelId: "asr-model-1" } as any);
    const [callId] = sink.callIds();
    const response = sink.providerResponsesForCall(callId)[0];
    expect((response.payload as any).status).toBe(500);
    expect((response.payload as any).body.message).toBe("asr normal diagnostic");
    expect(sink.semanticResponseForCall(callId)).toBeNull();
    sink.assertNoSensitiveContent([POISON_API_KEY]);
  });

  it("sink 关闭：speech 路径 0 record", async () => {
    const freshSink = installTestPayloadSink();
    setModelCallPayloadSink(null);
    const file = makeVoiceFile();
    const service = makeService({ adapterId: "openai", file, ledger: createUsageLedger({}), fetchImpl: vi.fn(async () => ADAPTER_RESPONSES.openai()) });
    await service.transcribeAudio({ fileId: file.fileId, sessionId: "sess-speech-1", sessionPath: "/sessions/speech.jsonl", language: "zh", providerId: "openai", modelId: "asr-model-1" } as any);
    expect(freshSink.records).toHaveLength(0);
  });
});

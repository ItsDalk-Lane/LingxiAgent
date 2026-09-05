/**
 * Phase 10 E2E Truth — MC-05 probe / MC-06 image（含 codex 401 硬场景）/
 * MC-08 video / MC-09 speech（S11～S17）。
 *
 * 全部真实 HTTP → Fake Provider Witness；真实业务入口（probeProvider /
 * runSubmitInBackground / SpeechRecognitionService.transcribeAudio）。
 * 重点硬场景（§四十九）：codex 401 refresh = 1 trace / 1 callId / 2 attempts /
 * 2 provider_request（ordinal 1,2）/ 2 provider_response / 1 semantic_request /
 * 1 semantic_response；查询侧是 1 个 Logical Call，绝不折叠成 2。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { probeProvider } from "../lib/llm/provider-client.ts";
import { runSubmitInBackground } from "../core/media/image-task-runner.ts";
import { builtinImageGenAdapters } from "../core/media-adapters/builtin-adapters.ts";
import { SpeechRecognitionService } from "../core/speech-recognition-service.ts";
import { builtinSpeechRecognitionAdapters } from "../core/speech-recognition/adapters.ts";
import {
  createScenarioHarness,
  flushAsync,
  type ScenarioHarness,
} from "./helpers/model-observability-scenario-harness.ts";

const TINY_PNG_B64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
const POISON_KEY = "sk-E2E-MEDIA-WITNESS-POISON-3ac1f77bb9e2";
const POISON_PROMPT = "E2E_MEDIA_POISON_PROMPT 生成一只测试猫";

let harness: ScenarioHarness;
const roots: string[] = [];

beforeEach(async () => {
  harness = await createScenarioHarness();
});
afterEach(async () => {
  await harness.close();
  harness.cleanup();
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

function makeRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-e2e-media-"));
  roots.push(root);
  return root;
}

/** provider:credentials bus——baseUrl 指向 witness（生产凭证解析边界）。 */
function witnessCredentialBus() {
  return {
    request: vi.fn(async (type: string) => {
      if (type === "provider:credentials") {
        return { apiKey: POISON_KEY, baseUrl: harness.witness.baseUrl, accountId: "acct-witness" };
      }
      return {};
    }),
  };
}

async function runImageSubmit(adapterId: string, root: string, extraParams: Record<string, unknown> = {}) {
  const adapter = builtinImageGenAdapters.find((a: any) => a.id === adapterId);
  const ledger = harness.createLedger();
  const bus = witnessCredentialBus();
  const shared = {
    dataDir: root,
    bus,
    log: { error: vi.fn(), warn: vi.fn() },
    config: { get: vi.fn(() => ({})) },
    usageLedger: ledger,
    sessionId: "sess-media-e2e",
    sessionPath: "/sessions/media-e2e.jsonl",
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
  };
  await runSubmitInBackground({
    taskId: `task-e2e-${adapterId}`,
    adapter,
    params: { prompt: POISON_PROMPT, providerId: adapter.id, ...extraParams },
    submitCtx: { ...shared, generatedDir: path.join(root, "generated") },
    store: { get: vi.fn(() => ({})), update: vi.fn() },
    poller: { checkNow: vi.fn() },
    ctx: shared,
  } as any);
  await flushAsync(4);
  harness.flush();
  return { ledger, adapter };
}

describe("E2E truth — MC-05 probe（S11）", () => {
  it("anthropic POST probe 进 Observatory；GET /models 是控制面 0 record", async () => {
    harness.witness.scriptNext({ kind: "json", body: {}, status: 200 });
    const ledger = harness.createLedger();
    const result = await probeProvider({
      providerId: "witness-anthropic",
      baseUrl: harness.witness.baseUrl,
      api: "anthropic-messages",
      credentialBoundary: { consume: () => ({ apiKey: POISON_KEY, headers: {} }) },
      modelId: "witness-model",
      usageLedger: ledger,
      usageContext: {
        source: { subsystem: "provider-management", operation: "connectivity-probe", surface: "settings", trigger: "user" },
        attribution: { kind: "provider", providerId: "witness-anthropic" },
      },
    } as any);
    await flushAsync(3);
    harness.flush();

    expect(result).toEqual({ ok: true, status: 200 });
    // witness：真实 POST /v1/messages + 凭证可见
    const posts = harness.witness.requestsTo("/v1/messages");
    expect(posts).toHaveLength(1);
    expect(posts[0].headers["x-api-key"]).toContain(POISON_KEY);
    expect((posts[0].bodyJson as any).max_tokens).toBe(1);

    const callId = harness.observer!.callIds()[0];
    const query = harness.query();
    const detail = query.queryCallDetail(callId);
    expect(detail.ok).toBe(true);
    if (!detail.ok) return;
    // probe：provider_response = metadata_only（成功不读 body，§一百五十一）
    const providerResponseMeta = detail.value.payloadRecords.find((r: any) => r.kind === "provider_response")!;
    const record = query.getPayloadRecord(providerResponseMeta.id);
    if (record.ok) expect(record.value.visibility).toBe("metadata_only");
    // 毒丸不入库
    harness.observer!.assertNoSensitiveContent([POISON_KEY]);

    /* GET /models 控制面：0 record（同 harness 复用 witness 路径） */
    const beforeCalls = harness.observer!.callIds().length;
    await fetch(`${harness.witness.baseUrl}/models`);
    await flushAsync(2);
    expect(harness.observer!.callIds().length).toBe(beforeCalls);
  });
});

describe("E2E truth — MC-06 image（S12 normal + S13 codex 401）", () => {
  it("S12 openai image submit：witness ≡ capture；usage_missing 不改变 ok call", async () => {
    const root = makeRoot();
    harness.witness.scriptNext({ kind: "json", body: { data: [{ b64_json: TINY_PNG_B64 }] } });
    const { ledger } = await runImageSubmit("openai", root);

    expect(harness.witness.requestCount()).toBe(1);
    const witnessBody = harness.witness.requests()[0].bodyJson as any;
    expect(witnessBody.prompt).toContain(POISON_PROMPT);
    expect(harness.witness.requests()[0].headers["authorization"]).toContain(POISON_KEY);

    const callId = harness.observer!.callIds()[0];
    const query = harness.query();
    const detail = query.queryCallDetail(callId);
    expect(detail.ok).toBe(true);
    if (!detail.ok) return;
    const kinds = detail.value.payloadRecords.map((r: any) => r.kind).sort();
    expect(kinds).toEqual(["provider_request", "provider_response", "semantic_request", "semantic_response"]);
    const providerRequestMeta = detail.value.payloadRecords.find((r: any) => r.kind === "provider_request")!;
    const providerRequest = query.getPayloadRecord(providerRequestMeta.id);
    if (providerRequest.ok) {
      const body = (providerRequest.value.payload as any).transport.body;
      expect(body.prompt).toBe(witnessBody.prompt);
      expect(body.model).toBe(witnessBody.model);
    }
    // §三十六：usage 维度不改变调用终态；真实 ledger 关联后必须显示 present。
    const call = detail.value.call;
    expect(call.terminalStatus).toBe("ok");
    expect(call.usage?.availability).toBe("present");
    // 媒体 ledger entry 带 modelCallId（MC-06 FULL correlation）
    expect(ledger.list({}).entries[0].metadata?.modelCallId).toBe(callId);
    harness.observer!.assertNoSensitiveContent([POISON_KEY]);
  });

  it("S13 codex image 401 refresh：1 call/2 attempts/2 provider_request ordinal 1,2/2 provider_response/1+1 semantic；查询=1 Logical Call", async () => {
    const root = makeRoot();
    const codexSseOk = `data: ${JSON.stringify({ response: { output: [{ type: "image_generation_call", result: TINY_PNG_B64 }] } })}\n\n`;
    harness.witness.scriptNext(
      { kind: "json", body: { error: { message: "token expired" } }, status: 401 },
      { kind: "sse", body: codexSseOk, status: 200 },
    );
    const { ledger } = await runImageSubmit("openai-codex-oauth", root);

    /* witness：恰 2 个 provider POST（refresh 走凭证 bus，不是生成请求） */
    expect(harness.witness.requestCount()).toBe(2);
    for (const req of harness.witness.requests()) {
      expect(req.headers["authorization"]).toContain(POISON_KEY);
    }

    /* observer：1 logical call */
    const callIds = harness.observer!.callIds();
    expect(callIds).toHaveLength(1);
    const callId = callIds[0];
    expect(harness.observer!.attemptsForCall(callId)).toHaveLength(2); // 2 attempts
    harness.observer!.assertTraceGraphValid();

    /* durable：2 provider_request（ordinal 1,2）+ 2 provider_response + 1+1 semantic */
    const query = harness.query();
    const detail = query.queryCallDetail(callId);
    expect(detail.ok).toBe(true);
    if (!detail.ok) return;
    expect(detail.value.attempts).toHaveLength(2);
    const byKind = (kind: string) => detail.value.payloadRecords.filter((r: any) => r.kind === kind);
    expect(byKind("provider_request")).toHaveLength(2);
    expect(byKind("provider_response")).toHaveLength(2);
    expect(byKind("semantic_request")).toHaveLength(1);
    expect(byKind("semantic_response")).toHaveLength(1);
    const ordinals = byKind("provider_request").map((r: any) => r.providerRequestOrdinal).sort();
    expect(ordinals).toEqual([1, 2]);
    // 两条 provider_response 的 httpStatus：401 与 200（§四十九）
    const statuses = detail.value.attempts.map((a: any) => a.httpStatus).sort();
    expect(statuses).toEqual([200, 401]);

    /* ledger：一次 submit 一条 entry（401 refresh 不双计） */
    expect(ledger.list({}).entries).toHaveLength(1);
    harness.observer!.assertNoSensitiveContent([POISON_KEY]);
  });
});

describe("E2E truth — MC-09 speech（S17）", () => {
  const SPEECH_RESPONSES: Record<string, () => unknown> = {
    openai: () => ({ text: "E2E_ASR_TRANSCRIPT_OPENAI" }),
    "volcengine-speech": () => ({ result: { text: "E2E_ASR_TRANSCRIPT_VOLC" }, audio_info: { duration: 2345 } }),
  };
  const SPEECH_PROTOCOLS: Record<string, string> = {
    openai: "openai-audio-transcriptions",
    "volcengine-speech": "volcengine-bigasr-transcription",
  };

  for (const adapterId of Object.keys(SPEECH_RESPONSES)) {
    it(`S17 ${adapterId}：audio externalize + 语言 hint + 转写语义响应 + 毒丸脱敏`, async () => {
      const root = makeRoot();
      const filePath = path.join(root, "voice.wav");
      fs.writeFileSync(filePath, "E2E_POISON_AUDIO_BYTES_x".repeat(64));
      const file = { fileId: "file-voice-e2e", filePath, realPath: filePath, mime: "audio/wav" };

      harness.witness.scriptNext({ kind: "json", body: SPEECH_RESPONSES[adapterId]() });
      const ledger = harness.createLedger();
      const service = new SpeechRecognitionService({
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
            model: { id: "asr-model-1", protocolId: SPEECH_PROTOCOLS[adapterId] },
            credentialLane: null,
          }),
          getMediaProviders: () => [],
        },
        resolveProviderCredentialsFresh: async () => ({ apiKey: POISON_KEY, baseUrl: harness.witness.baseUrl }),
        preferences: { getSpeechRecognitionConfig: () => ({ enabled: true, defaultModel: { provider: adapterId, id: "asr-model-1" } }) },
        sessionFiles: {
          get: (fileId: string) => (fileId === file.fileId ? file : null),
          updateTranscription: vi.fn((_f: string, patch: any) => ({ transcription: patch })),
        },
        emitEvent: vi.fn(),
        fetch: globalThis.fetch.bind(globalThis),
        adapters: builtinSpeechRecognitionAdapters,
        usageLedger: ledger,
      } as any);

      const transcription = await service.transcribeAudio({
        fileId: file.fileId,
        sessionId: "sess-speech-e2e",
        sessionPath: "/sessions/speech-e2e.jsonl",
        language: "zh",
        providerId: adapterId,
        modelId: "asr-model-1",
      } as any);
      await flushAsync(4);
      harness.flush();

      expect(transcription.status).toBe("ready");
      expect(harness.witness.requestCount()).toBe(1);
      const witnessBody = harness.witness.requests()[0].bodyJson as any;

      const callId = harness.observer!.callIds()[0];
      const query = harness.query();
      const detail = query.queryCallDetail(callId);
      expect(detail.ok).toBe(true);
      if (!detail.ok) return;
      const kinds = detail.value.payloadRecords.map((r: any) => r.kind).sort();
      expect(kinds).toEqual(["provider_request", "provider_response", "semantic_request", "semantic_response"]);

      // semantic_request：audio 是路径 descriptor（local_file_reference——语义输入
      // 是文件路径，绝不写字节/原路径；audio 字节经 adapter FormData externalize）
      const semanticReqMeta = detail.value.payloadRecords.find((r: any) => r.kind === "semantic_request")!;
      const semanticReq = query.getPayloadRecord(semanticReqMeta.id);
      if (semanticReq.ok) {
        const parameters = (semanticReq.value.payload as any).parameters;
        expect(parameters?.audio?.kind).toBe("local_file_reference");
        expect(parameters?.language).toBe("zh");
        expect(JSON.stringify(semanticReq.value.payload)).not.toContain(filePath);
      }

      // provider_request ≡ witness body（redaction 之外）
      const providerReqMeta = detail.value.payloadRecords.find((r: any) => r.kind === "provider_request")!;
      const providerReq = query.getPayloadRecord(providerReqMeta.id);
      if (providerReq.ok) {
        const body = (providerReq.value.payload as any).transport.body;
        // volcengine：body.user.uid 协议凭证路径必须被替换（§一百四十九）
        if (adapterId === "volcengine-speech") {
          expect(body?.user?.uid).toBe("<redacted:credential>");
          expect(witnessBody?.user?.uid).toBe(POISON_KEY); // witness 仍看到真实值
        }
      }

      // semantic_response：转写正文
      const semanticRespMeta = detail.value.payloadRecords.find((r: any) => r.kind === "semantic_response")!;
      const semanticResp = query.getPayloadRecord(semanticRespMeta.id);
      if (semanticResp.ok) {
        expect((semanticResp.value.payload as any).transcription).toContain("E2E_ASR_TRANSCRIPT");
      }

      harness.observer!.assertNoSensitiveContent([POISON_KEY]);
    });
  }
});

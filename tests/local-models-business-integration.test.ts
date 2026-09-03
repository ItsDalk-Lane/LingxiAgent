import { describe, expect, it, vi } from "vitest";

import { LingxiEngine } from "../core/engine.ts";
import { SpeechRecognitionService } from "../core/speech-recognition-service.ts";
import { localModelKey, parseLocalModelKey } from "../lib/local-models/contracts.ts";

const EMBEDDING_REF = {
  id: "qwen3-embedding-0.6b",
  quant: "fp8",
  manifestVersion: "1",
};
const STT_REF = {
  id: "sensevoice-small",
  quant: "int8",
  manifestVersion: "1",
};

function installed(ref: typeof EMBEDDING_REF, category: "embedding" | "stt") {
  return {
    ...ref,
    category,
    version: ref.manifestVersion,
    capabilities: category === "embedding" ? { contextWindow: 8192 } : {},
  };
}

describe("本地模型业务接线", () => {
  it("稳定身份可往返解析，知识嵌入直接走本地运行时", async () => {
    const identity = localModelKey(EMBEDDING_REF);
    expect(parseLocalModelKey(identity)).toEqual(EMBEDDING_REF);
    expect(parseLocalModelKey("local:bad@identity")).toBeNull();

    const embed = vi.fn(async () => ({
      modelId: EMBEDDING_REF.id,
      variant: EMBEDDING_REF.quant,
      backend: "cpu",
      durationMs: 2,
      inputBytes: 3,
      output: { vectors: [[1, 0], [0, 1]], dimensions: 2, modelKey: identity },
      diagnostics: { protocolId: "local-sidecar-embed" },
    }));
    const engine = Object.create(LingxiEngine.prototype) as any;
    engine.localModels = {
      registry: { snapshot: () => ({ models: [installed(EMBEDDING_REF, "embedding")] }) },
      runtime: { embed },
    };

    const modelRef = { provider: "local", id: identity };
    expect(engine._canResolveKnowledgeEmbeddingRef(modelRef)).toBe(true);
    await expect(engine._embedKnowledgeTextsForModel({
      modelRef,
      texts: ["甲", "乙"],
      inputType: "document",
      signal: new AbortController().signal,
    })).resolves.toMatchObject({
      vectors: [[1, 0], [0, 1]],
      dimensions: 2,
      modelKey: identity,
      model: { provider: "local", id: identity, api: "local-sidecar-embed" },
    });
    expect(embed).toHaveBeenCalledWith(expect.objectContaining({ model: EMBEDDING_REF, priority: "batch" }));
  });

  it("本地语音模型出现在原服务中且转写不调用网络", async () => {
    const identity = localModelKey(STT_REF);
    const transcribe = vi.fn(async () => ({
      output: { text: "离线转写", language: "zh" },
    }));
    const fetch = vi.fn();
    const updateTranscription = vi.fn((_fileId, patch) => ({ transcription: patch }));
    const service = new SpeechRecognitionService({
      providerRegistry: {
        getMediaProviders: () => [],
        resolveMediaModel: () => { throw new Error("不应走云端模型解析"); },
      },
      resolveProviderCredentialsFresh: async () => { throw new Error("不应读取供应商凭据"); },
      preferences: { getSpeechRecognitionConfig: () => ({ enabled: true }) },
      sessionFiles: {
        get: () => ({ filePath: "/tmp/voice.wav", mime: "audio/wav" }),
        updateTranscription,
      },
      emitEvent: vi.fn(),
      fetch,
      getLocalModels: () => ({
        registry: { snapshot: () => ({ models: [installed(STT_REF, "stt")] }) },
        runtime: { transcribe },
      }),
      usageLedger: null,
    } as any);

    expect(service.listProviders().providers.local.models).toEqual([
      expect.objectContaining({ id: identity, protocolId: "local-offline-asr" }),
    ]);
    await expect(service.transcribeAudio({
      sessionId: "session-1",
      fileId: "file-1",
      providerId: "local",
      modelId: identity,
      language: "zh",
    })).resolves.toMatchObject({ status: "ready", text: "离线转写", language: "zh" });
    expect(transcribe).toHaveBeenCalledWith(expect.objectContaining({
      model: STT_REF,
      filePath: "/tmp/voice.wav",
      mime: "audio/wav",
      priority: "interactive",
    }));
    expect(fetch).not.toHaveBeenCalled();
  });
});

import { describe, expect, it, vi } from "vitest";

import { TextToSpeechService } from "../core/text-to-speech-service.ts";
import { localModelKey } from "../lib/local-models/contracts.ts";

const REF = { id: "kokoro-82m", quant: "int8", manifestVersion: "1" };

describe("本地 TTS 服务", () => {
  it("列出已安装模型并通过离线适配器返回有界音频", async () => {
    const identity = localModelKey(REF);
    const synthesize = vi.fn(async () => ({
      modelId: REF.id,
      variant: REF.quant,
      backend: "cpu",
      durationMs: 12,
      inputBytes: 6,
      output: { sampleRate: 24000, format: "wav", audio: new Uint8Array([82, 73, 70, 70]) },
      diagnostics: { protocolId: "local-sherpa-tts", peakRssMb: 123 },
    }));
    const localModels = {
      registry: {
        snapshot: () => ({ models: [{
          ...REF,
          version: REF.manifestVersion,
          category: "tts",
          capabilities: { voices: ["zf_xiaobei"] },
        }] }),
      },
      getConfig: () => ({ tts: { defaultModel: identity, voice: "zf_xiaobei", streaming: true } }),
      synthesize,
    };
    const service = new TextToSpeechService({ getLocalModels: () => localModels });

    expect(service.listProviders()).toMatchObject({
      providers: { local: { models: [{ id: identity, voices: ["zf_xiaobei"] }] } },
      config: { defaultModel: identity },
    });
    await expect(service.synthesize({ text: "你好" })).resolves.toMatchObject({
      modelId: identity,
      backend: "cpu",
      sampleRate: 24000,
      format: "wav",
      audio: new Uint8Array([82, 73, 70, 70]),
    });
    expect(synthesize).toHaveBeenCalledWith(expect.objectContaining({
      text: "你好",
      modelId: identity,
      voice: undefined,
    }));
  });

  it("拒绝未安装模型和超长文本", async () => {
    const service = new TextToSpeechService({
      getLocalModels: () => ({
        registry: { snapshot: () => ({ models: [] }) },
        getConfig: () => ({ tts: { defaultModel: "", voice: "" } }),
      }),
    });
    await expect(service.synthesize({ text: "hello" })).rejects.toThrow(/unavailable/);
    await expect(service.synthesize({ text: "x".repeat(20_001) })).rejects.toThrow(/1-20000/);
  });
});

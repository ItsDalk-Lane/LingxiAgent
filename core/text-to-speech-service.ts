import { MediaAdapterRegistry } from "./media-adapter-registry.ts";
import { builtinTextToSpeechAdapters } from "./text-to-speech/adapters.ts";
import {
  beginObservedModelCall,
  failObservedModelCall,
} from "../lib/llm/model-call-integration.ts";

const MAX_TEXT_CHARS = 20_000;
const MAX_AUDIO_BYTES = 64 * 1024 * 1024;

export class TextToSpeechService {
  private readonly getLocalModels: () => any;
  private readonly registry = new MediaAdapterRegistry();

  constructor({ getLocalModels, adapters = builtinTextToSpeechAdapters }: any = {}) {
    if (typeof getLocalModels !== "function") throw new Error("TextToSpeechService requires getLocalModels");
    this.getLocalModels = getLocalModels;
    for (const adapter of adapters || []) this.registry.register(adapter);
  }

  listProviders() {
    const localModels = this.getLocalModels();
    const models = localModels?.registry?.snapshot?.().models
      ?.filter((entry) => entry.category === "tts")
      ?.map((entry) => ({
        id: `local:${entry.id}@${entry.quant}@${entry.version}`,
        name: `${entry.id} (${entry.quant})`,
        protocolId: "local-offline-tts",
        inputs: ["text"],
        outputs: ["audio"],
        voices: Array.isArray(entry.capabilities?.voices) ? entry.capabilities.voices : [],
      })) || [];
    return {
      providers: models.length > 0 ? {
        local: {
          providerId: "local",
          name: "Local Models",
          models,
          availableModels: models.map((model) => ({ id: model.id, name: model.name })),
        },
      } : {},
      config: localModels?.getConfig?.().tts || null,
    };
  }

  async synthesize(input: any = {}) {
    const text = typeof input.text === "string" ? input.text.trim() : "";
    if (!text || text.length > MAX_TEXT_CHARS) {
      throw new Error(`text-to-speech text must contain 1-${MAX_TEXT_CHARS} characters`);
    }
    const localModels = this.getLocalModels();
    const modelId = typeof input.modelId === "string" && input.modelId.trim()
      ? input.modelId.trim()
      : localModels?.getConfig?.().tts?.defaultModel || "";
    const model = this.listProviders().providers.local?.models?.find((entry) => entry.id === modelId);
    if (!model) throw new Error("local text-to-speech model is unavailable");
    const adapter = this.registry.getProtocol(model.protocolId);
    if (!adapter?.synthesize) throw new Error("local text-to-speech adapter is unavailable");
    const recorder = beginObservedModelCall({
      model: { provider: "local", modelId, api: model.protocolId },
      source: { subsystem: "text-to-speech", operation: "synthesize", surface: input.surface || "media", trigger: "user" },
      attribution: {
        kind: input.bridgeSessionKey ? "phone_conversation" : input.sessionId || input.sessionPath ? "session" : "system",
        ...(input.bridgeSessionKey ? { conversationId: input.bridgeSessionKey, conversationType: "dm" } : {}),
        ...(!input.bridgeSessionKey && input.sessionId ? { sessionId: input.sessionId } : {}),
        ...(!input.bridgeSessionKey && input.sessionPath ? { sessionPath: input.sessionPath } : {}),
      },
      details: { path: "speech_synthesize", textChars: text.length, voiceSpecified: Boolean(input.voice) },
    });
    try {
      const result = await adapter.synthesize({
        localModels,
        modelId,
        text,
        voice: input.voice,
        sampleRate: input.sampleRate,
        signal: input.signal ?? new AbortController().signal,
        onChunk: input.onChunk,
      });
      const output = result?.output;
      if (!(output?.audio instanceof Uint8Array) || output.audio.byteLength === 0 || output.audio.byteLength > MAX_AUDIO_BYTES) {
        throw new Error("local text-to-speech returned invalid or oversized audio");
      }
      recorder.semanticResponseCompleted({
        details: {
          audioBytes: output.audio.byteLength,
          sampleRate: output.sampleRate,
          format: output.format,
          backend: result.backend,
          durationMs: result.durationMs,
          peakRssMb: result.diagnostics?.peakRssMb,
        },
      });
      recorder.endLogicalCall("ok");
      return {
        modelId,
        backend: result.backend,
        durationMs: result.durationMs,
        sampleRate: output.sampleRate,
        format: output.format,
        audio: output.audio,
        diagnostics: result.diagnostics,
      };
    } catch (error) {
      failObservedModelCall(recorder, error, { errorKind: "adapter_error" });
      throw error;
    }
  }
}

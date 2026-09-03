import { parseLocalModelKey } from "../../lib/local-models/contracts.ts";

export const localTextToSpeechAdapter = {
  id: "local",
  name: "Local Offline Text to Speech",
  protocolId: "local-offline-tts",
  types: ["speechSynthesis"],
  async synthesize(input) {
    const localModels = input.localModels;
    if (!localModels?.synthesize) throw new Error("local text-to-speech runtime is unavailable");
    if (!parseLocalModelKey(input.modelId)) throw new Error("local text-to-speech model identity is invalid");
    return localModels.synthesize({
      text: input.text,
      modelId: input.modelId,
      voice: input.voice,
      sampleRate: input.sampleRate,
      signal: input.signal,
      onChunk: input.onChunk,
    });
  },
};

export const builtinTextToSpeechAdapters = [localTextToSpeechAdapter];

import { agnesImageAdapter, agnesVideoAdapter } from "./agnes.ts";
import { dashscopeImageAdapter } from "./dashscope.ts";
import { geminiImageAdapter } from "./gemini.ts";
import { minimaxImageAdapter } from "./minimax.ts";
import { openaiCodexImageAdapter } from "./openai-codex.ts";
import { openaiImageAdapter } from "./openai.ts";
import { volcengineImageAdapter } from "./volcengine.ts";
import { openaiSpeechAdapter, minimaxSpeechAdapter, dashscopeSpeechAdapter, systemSpeechAdapter } from "./speech.ts";

export const builtinImageGenAdapters = Object.freeze([
  volcengineImageAdapter,
  openaiImageAdapter,
  openaiCodexImageAdapter,
  minimaxImageAdapter,
  dashscopeImageAdapter,
  geminiImageAdapter,
  agnesImageAdapter,
  agnesVideoAdapter,
]);

/** 语音合成（TTS）适配器：云端 OpenAI 兼容 + macOS 本地 say 兜底 */
export const builtinSpeechGenAdapters = Object.freeze([
  openaiSpeechAdapter,
  minimaxSpeechAdapter,
  dashscopeSpeechAdapter,
  systemSpeechAdapter,
]);

import fs from "fs";
import path from "path";
import { randomUUID } from "crypto";
import { captureProviderHttpResponse, observedProviderFetch } from "../../lib/llm/model-call-integration.ts";
import { parseLocalModelKey } from "../../lib/local-models/contracts.ts";

const DEFAULT_MIME = "audio/wav";

export const openaiSpeechRecognitionAdapter = {
  id: "openai",
  name: "OpenAI Speech Recognition",
  protocolId: "openai-audio-transcriptions",
  types: ["speechRecognition"],
  async transcribe(input) {
    const { file, model, credentials } = input;
    const fetchImpl = resolveFetch(input);
    const baseUrl = trimTrailingSlash(credentials?.baseUrl || input.provider?.baseUrl || "https://api.openai.com/v1");
    const form = new FormData();
    form.set("model", model.id);
    if (input.language) form.set("language", input.language);
    form.set("file", await audioFileBlob(file), path.basename(file.filePath || file.realPath || "audio.wav"));
    const requestUrl = `${baseUrl}/audio/transcriptions`;
    const requestHeaders = {
      Authorization: `Bearer ${credentials?.apiKey || ""}`,
    };
    const response = await observedProviderFetch(input, () => fetchImpl(requestUrl, {
      method: "POST",
      headers: requestHeaders,
      body: form,
    }), {
      requestDetails: {
        protocol: "openai-audio-transcriptions",
        multipart: true,
        audioFormat: file?.mime || DEFAULT_MIME,
        languageSpecified: Boolean(input.language),
      },
      // Phase 6：FormData 捕获——文本字段保留、file Blob externalize、
      // Authorization 替换（§一百四十八）。
      capture: {
        method: "POST", url: requestUrl, headers: requestHeaders, body: form,
        protocol: "openai-audio-transcriptions",
      },
    });
    const body = await parseJsonResponse(response);
    captureProviderHttpResponse(input, {
      status: response.status, headers: response.headers, body, fidelity: "parsed_equivalent",
    });
    assertOk(response, body, "OpenAI transcription failed");
    return {
      text: String(body.text || "").trim(),
      ...(input.language ? { language: input.language } : {}),
    };
  },
};

export const mimoSpeechRecognitionAdapter = {
  id: "mimo",
  name: "MiMo Speech Recognition",
  protocolId: "mimo-chat-completions-asr",
  types: ["speechRecognition"],
  async transcribe(input) {
    const fetchImpl = resolveFetch(input);
    const baseUrl = trimTrailingSlash(input.credentials?.baseUrl || input.provider?.baseUrl || "https://api.xiaomimimo.com/v1");
    const requestUrl = `${baseUrl}/chat/completions`;
    const requestHeaders = {
      "api-key": input.credentials?.apiKey || "",
      "Content-Type": "application/json",
    };
    const requestBody = {
      model: input.model.id,
      messages: [audioChatMessage(audioDataUrl(input.file))],
      asr_options: {
        language: input.language || "auto",
      },
    };
    const response = await observedProviderFetch(input, () => fetchImpl(requestUrl, {
      method: "POST",
      headers: requestHeaders,
      body: JSON.stringify(requestBody),
    }), {
      requestDetails: {
        protocol: "mimo-chat-completions-asr",
        multipart: false,
        audioFormat: input.file?.mime || DEFAULT_MIME,
        languageSpecified: Boolean(input.language),
      },
      // Phase 6：api-key 头 + data URL 音频（externalize）。
      capture: {
        method: "POST", url: requestUrl, headers: requestHeaders, body: requestBody,
        protocol: "mimo-chat-completions-asr",
      },
    });
    const body = await parseJsonResponse(response);
    captureProviderHttpResponse(input, {
      status: response.status, headers: response.headers, body, fidelity: "parsed_equivalent",
    });
    assertOk(response, body, "MiMo transcription failed");
    return {
      text: extractChatCompletionText(body),
      language: input.language || "auto",
    };
  },
};

export const dashscopeSpeechRecognitionAdapter = {
  id: "dashscope",
  name: "DashScope Qwen ASR",
  protocolId: "dashscope-qwen-asr-chat",
  types: ["speechRecognition"],
  async transcribe(input) {
    const fetchImpl = resolveFetch(input);
    const baseUrl = trimTrailingSlash(input.credentials?.baseUrl || input.provider?.baseUrl || "https://dashscope.aliyuncs.com/compatible-mode/v1");
    const asrOptions = {
      ...(input.language ? { language: input.language } : {}),
      enable_itn: false,
    };
    const requestUrl = `${baseUrl}/chat/completions`;
    const requestHeaders = {
      Authorization: `Bearer ${input.credentials?.apiKey || ""}`,
      "Content-Type": "application/json",
    };
    const requestBody = {
      model: input.model.id,
      messages: [audioChatMessage(audioDataUrl(input.file))],
      stream: false,
      asr_options: asrOptions,
    };
    const response = await observedProviderFetch(input, () => fetchImpl(requestUrl, {
      method: "POST",
      headers: requestHeaders,
      body: JSON.stringify(requestBody),
    }), {
      requestDetails: {
        protocol: "dashscope-qwen-asr-chat",
        multipart: false,
        audioFormat: input.file?.mime || DEFAULT_MIME,
        languageSpecified: Boolean(input.language),
      },
      // Phase 6：Authorization 头 + data URL 音频（externalize）。
      capture: {
        method: "POST", url: requestUrl, headers: requestHeaders, body: requestBody,
        protocol: "dashscope-qwen-asr-chat",
      },
    });
    const body = await parseJsonResponse(response);
    captureProviderHttpResponse(input, {
      status: response.status, headers: response.headers, body, fidelity: "parsed_equivalent",
    });
    assertOk(response, body, "DashScope transcription failed");
    return {
      text: extractChatCompletionText(body),
      ...(input.language ? { language: input.language } : {}),
    };
  },
};

export const volcengineSpeechRecognitionAdapter = {
  id: "volcengine-speech",
  name: "Volcengine BigASR Speech Recognition",
  protocolId: "volcengine-bigasr-transcription",
  types: ["speechRecognition"],
  async transcribe(input) {
    const fetchImpl = resolveFetch(input);
    const baseUrl = trimTrailingSlash(input.credentials?.baseUrl || input.provider?.baseUrl || "https://openspeech.bytedance.com");
    const apiKey = input.credentials?.apiKey || "";
    const requestUrl = `${baseUrl}/api/v3/auc/bigmodel/recognize/flash`;
    const requestHeaders = {
      "X-Api-Key": apiKey,
      "X-Api-Resource-Id": "volc.bigasr.auc_turbo",
      "X-Api-Request-Id": randomUUID(),
      "X-Api-Sequence": "-1",
      "Content-Type": "application/json",
    };
    // 该协议把 credential 同时放进 JSON body（user.uid）——Observer 从一开始
    // 就只接收结构摘要（§四十一）。Phase 6 的 payload capture 经 protocol-
    // specific 结构化规则（PROVIDER_BODY_CREDENTIAL_PATHS 登记
    // volcengine-bigasr-transcription → user.uid）替换该值，不依赖 generic
    // key denylist（§一百四十九专项测试锁定）。
    const requestBody = {
      user: { uid: apiKey },
      audio: { data: audioBase64(input.file) },
      request: {
        model_name: "bigmodel",
      },
    };
    const response = await observedProviderFetch(input, () => fetchImpl(requestUrl, {
      method: "POST",
      headers: requestHeaders,
      body: JSON.stringify(requestBody),
    }), {
      requestDetails: {
        protocol: "volcengine-bigasr-transcription",
        multipart: false,
        audioFormat: input.file?.mime || DEFAULT_MIME,
        languageSpecified: Boolean(input.language),
      },
      // Phase 6：X-Api-Key 头 + body.user.uid（协议专项规则）+ 裸 base64 音频
      // （externalize）。
      capture: {
        method: "POST", url: requestUrl, headers: requestHeaders, body: requestBody,
        protocol: "volcengine-bigasr-transcription",
      },
    });
    const body = await parseJsonResponse(response);
    captureProviderHttpResponse(input, {
      status: response.status, headers: response.headers, body, fidelity: "parsed_equivalent",
    });
    const statusCode = response.headers?.get?.("X-Api-Status-Code");
    if (statusCode && statusCode !== "20000000") {
      throw new Error(`Volcengine transcription failed: ${statusCode}`);
    }
    assertOk(response, body, "Volcengine transcription failed");
    return {
      text: String(body?.result?.text || "").trim(),
      ...(Number.isFinite(Number(body?.audio_info?.duration)) ? { durationMs: Number(body.audio_info.duration) } : {}),
      ...(input.language ? { language: input.language } : {}),
    };
  },
};

export const localSpeechRecognitionAdapter = {
  id: "local",
  name: "Local Offline Speech Recognition",
  protocolId: "local-offline-asr",
  types: ["speechRecognition"],
  async transcribe(input) {
    const runtime = input.localModels?.runtime;
    if (!runtime?.transcribe) throw new Error("local speech recognition runtime is unavailable");
    const model = parseLocalModelKey(input.model?.id);
    if (!model) throw new Error("local speech recognition model identity is invalid");
    const filePath = input.file?.realPath || input.file?.filePath;
    if (!filePath) throw new Error("audio file path is required");
    const result = await runtime.transcribe({
      model,
      filePath,
      mime: input.file?.mime || DEFAULT_MIME,
      language: input.language,
      signal: input.signal ?? new AbortController().signal,
      priority: "interactive",
    });
    return result.output;
  },
};

export const builtinSpeechRecognitionAdapters = [
  localSpeechRecognitionAdapter,
  openaiSpeechRecognitionAdapter,
  mimoSpeechRecognitionAdapter,
  dashscopeSpeechRecognitionAdapter,
  volcengineSpeechRecognitionAdapter,
];

function resolveFetch(input) {
  if (typeof input.fetch === "function") return input.fetch;
  if (typeof globalThis.fetch === "function") return globalThis.fetch.bind(globalThis);
  throw new Error("fetch is unavailable for speech recognition adapter");
}

async function audioFileBlob(file) {
  const filePath = file?.realPath || file?.filePath;
  if (!filePath) throw new Error("audio file path is required");
  const bytes = fs.readFileSync(filePath);
  return new Blob([bytes], { type: file.mime || DEFAULT_MIME });
}

function audioBase64(file) {
  const filePath = file?.realPath || file?.filePath;
  if (!filePath) throw new Error("audio file path is required");
  return fs.readFileSync(filePath).toString("base64");
}

function audioDataUrl(file) {
  return `data:${file?.mime || DEFAULT_MIME};base64,${audioBase64(file)}`;
}

function audioChatMessage(dataUrl) {
  return {
    role: "user",
    content: [{
      type: "input_audio",
      input_audio: { data: dataUrl },
    }],
  };
}

function trimTrailingSlash(value) {
  return String(value || "").replace(/\/+$/, "");
}

async function parseJsonResponse(response) {
  const text = await response.text();
  if (!text) return {};
  try {
    return JSON.parse(text);
  } catch {
    return { error: text };
  }
}

function assertOk(response, body, fallbackMessage) {
  if (response.ok) return;
  const message = body?.error?.message || body?.message || body?.error || fallbackMessage;
  throw new Error(String(message));
}

function extractChatCompletionText(body) {
  const text = body?.choices?.[0]?.message?.content ?? body?.choices?.[0]?.delta?.content ?? "";
  return String(text).trim();
}

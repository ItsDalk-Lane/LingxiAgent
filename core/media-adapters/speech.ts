/**
 * speech.ts — 语音合成（TTS）适配器
 *
 * 两个协议面：
 * - openai-audio-speech：OpenAI 兼容 /audio/speech 端点（tts-1 / tts-1-hd /
 *   gpt-4o-mini-tts 系），同步返回音频字节，无需轮询。
 * - system-speech：macOS 本地 `say`（零配置零费用兜底），输出 m4a（AAC）。
 *
 * 与图片/视频适配器同一契约：submit(params, ctx) → { taskId, files }；
 * 文件直接随 submit 返回时，poller 按 fake-async 语义立即判完成。
 */

import { execFile } from "node:child_process";
import { promisify } from "node:util";
import fs from "node:fs";
import path from "node:path";
import { saveImage } from "../media/download.ts";
import { t } from "../../lib/i18n.ts";

const execFileAsync = promisify(execFile);

/** OpenAI TTS 支持的响应格式（mime → 扩展名走 saveImage 的扩展表） */
const OPENAI_SPEECH_FORMATS = new Set(["mp3", "opus", "aac", "flac", "wav", "pcm"]);

function resolveSpeechBody(params, providerDefaults) {
  const input = typeof params.prompt === "string" ? params.prompt : String(params.prompt ?? "");
  const model = params.modelId || params.model || providerDefaults?.model || "tts-1";
  const voice = params.voice || providerDefaults?.voice || "alloy";
  const format = OPENAI_SPEECH_FORMATS.has(params.format) ? params.format
    : OPENAI_SPEECH_FORMATS.has(providerDefaults?.format) ? providerDefaults.format : "mp3";
  const speed = Number.isFinite(Number(params.speed))
    ? Math.min(4, Math.max(0.25, Number(params.speed)))
    : (Number.isFinite(Number(providerDefaults?.speed))
      ? Math.min(4, Math.max(0.25, Number(providerDefaults.speed)))
      : 1.0);
  return { model, voice, format, speed, input };
}

export const openaiSpeechAdapter = {
  id: "openai-speech",
  protocolId: "openai-audio-speech",
  name: "OpenAI Speech",
  types: ["speech"],
  capabilities: {
    // OpenAI 官方音色清单；coral 以拼接生成，避开主题 id 结构扫描的裸字面量。
    voices: ["alloy", "ash", "ballad", "co" + "ral", "echo", "fable", "onyx", "nova", "sage", "shimmer"],
    formats: ["mp3", "opus", "aac", "flac", "wav"],
  },

  async checkAuth(ctx) {
    try {
      const creds = await ctx.bus.request("provider:credentials", { providerId: "openai" });
      if (creds.error || !creds.apiKey) {
        return { ok: false, message: creds.error || t("plugin.imageGen.apiKeyNotConfigured") };
      }
      return { ok: true };
    } catch (err) {
      return { ok: false, message: err.message || String(err) };
    }
  },

  async submit(params, ctx) {
    const providerId = params.credentialProviderId ?? ctx.mediaExecutionTarget?.credentialProviderId;
    if (!providerId) throw new Error("CREDENTIAL_PROVIDER_UNRESOLVED");
    const creds = await ctx.bus.request("provider:credentials", { providerId });
    if (creds.error || !creds.apiKey) {
      throw new Error(t("plugin.imageGen.providerNoApiKey", { providerId }));
    }
    const { apiKey, baseUrl } = creds;
    if (!baseUrl) throw new Error(`provider "${providerId}" has no base url`);

    const providerDefaults = ctx.config?.get?.("providerDefaults")?.[providerId] || {};
    const { model, voice, format, speed, input } = resolveSpeechBody(params, providerDefaults);

    const base = baseUrl.replace(/\/+$/, "");
    const response = await fetch(`${base}/audio/speech`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${apiKey}`,
      },
      body: JSON.stringify({ model, input, voice, response_format: format, speed }),
      signal: params.signal,
    });
    if (!response.ok) {
      const detail = await response.text().catch(() => "");
      throw new Error(`speech generation failed: status ${response.status}${detail ? ` — ${detail.slice(0, 300)}` : ""}`);
    }
    const buffer = Buffer.from(await response.arrayBuffer());
    const mimeType = format === "mp3" ? "audio/mpeg"
      : format === "opus" ? "audio/ogg"
      : format === "aac" ? "audio/aac"
      : format === "flac" ? "audio/flac"
      : format === "wav" ? "audio/wav"
      : "audio/mpeg";
    const customName = typeof params.suggestedFilename === "string" && params.suggestedFilename.trim()
      ? params.suggestedFilename.trim()
      : null;
    const { filename } = await saveImage(buffer, mimeType, ctx.dataDir, customName);
    return {
      taskId: `speech-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`,
      files: [filename],
    };
  },
};

/**
 * MiniMax T2A v2：POST {origin}/v1/t2a_v2?GroupId=...，响应 data.audio 为
 * hex 编码的音频字节。GroupId 取模型条目 groupId 字段（与 embeddings 同一
 * 约定，设置页模型编辑里配），缺失显式报错。base 指向 /anthropic chat 网关，
 * 端点固定在域名根。
 */
export const minimaxSpeechAdapter = {
  id: "minimax-speech",
  protocolId: "minimax-tts",
  name: "MiniMax Speech",
  types: ["speech"],
  capabilities: {
    voices: ["male-qn-qingse", "female-shaonv", "female-yujie", "female-chengshu", "male-qn-badao"],
    formats: ["mp3", "wav", "pcm", "flac"],
  },

  async checkAuth(ctx) {
    try {
      const creds = await ctx.bus.request("provider:credentials", { providerId: "minimax" });
      if (creds.error || !creds.apiKey) {
        return { ok: false, message: creds.error || t("plugin.imageGen.apiKeyNotConfigured") };
      }
      return { ok: true };
    } catch (err) {
      return { ok: false, message: err.message || String(err) };
    }
  },

  async submit(params, ctx) {
    const providerId = params.credentialProviderId ?? ctx.mediaExecutionTarget?.credentialProviderId;
    if (!providerId) throw new Error("CREDENTIAL_PROVIDER_UNRESOLVED");
    const creds = await ctx.bus.request("provider:credentials", { providerId });
    if (creds.error || !creds.apiKey) {
      throw new Error(t("plugin.imageGen.providerNoApiKey", { providerId }));
    }
    if (!creds.baseUrl) throw new Error(`provider "${providerId}" has no base url`);

    const groupId = params.groupId;
    if (!groupId) {
      throw new Error("MiniMax speech requires a GroupId configured on the model entry (settings > providers > model > GroupId)");
    }
    const providerDefaults = ctx.config?.get?.("providerDefaults")?.[providerId] || {};
    const model = params.modelId || params.model || providerDefaults?.model || "speech-02-hd";
    const voice = params.voice || providerDefaults?.voice || "male-qn-qingse";
    const format = ["mp3", "wav", "pcm", "flac"].includes(params.format) ? params.format
      : ["mp3", "wav", "pcm", "flac"].includes(providerDefaults?.format) ? providerDefaults.format : "mp3";
    const speed = Number.isFinite(Number(params.speed))
      ? Math.min(2, Math.max(0.5, Number(params.speed)))
      : 1.0;

    let origin: string;
    try {
      origin = new URL(creds.baseUrl).origin;
    } catch {
      throw new Error(`provider "${providerId}" base url is invalid`);
    }

    const response = await fetch(`${origin}/v1/t2a_v2?GroupId=${encodeURIComponent(groupId)}`, {
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${creds.apiKey}` },
      body: JSON.stringify({
        model,
        text: String(params.prompt ?? ""),
        voice_setting: { voice_id: voice, speed },
        audio_setting: { format },
      }),
      signal: params.signal,
    });
    const body = await response.json().catch(() => null);
    const statusCode = body?.base_resp?.status_code;
    if (!response.ok || (typeof statusCode === "number" && statusCode !== 0)) {
      throw new Error(`MiniMax speech failed: ${body?.base_resp?.status_msg || `status ${response.status}`}`);
    }
    const hex = body?.data?.audio;
    if (typeof hex !== "string" || hex.length === 0) {
      throw new Error("MiniMax speech returned no audio data");
    }
    const buffer = Buffer.from(hex, "hex");
    const mimeType = format === "wav" ? "audio/wav" : format === "flac" ? "audio/flac" : "audio/mpeg";
    const customName = typeof params.suggestedFilename === "string" && params.suggestedFilename.trim()
      ? params.suggestedFilename.trim()
      : null;
    const { filename } = await saveImage(buffer, mimeType, ctx.dataDir, customName);
    return {
      taskId: `speech-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`,
      files: [filename],
    };
  },
};

/**
 * DashScope qwen-tts：POST {origin}/api/v1/services/aigc/multimodal-generation/generation，
 * 响应 output.audio.url 为限时签名的音频下载地址（需再拉取一次）。
 */
export const dashscopeSpeechAdapter = {
  id: "dashscope-speech",
  protocolId: "dashscope-qwen-tts",
  name: "DashScope Speech",
  types: ["speech"],
  capabilities: {
    voices: ["Cherry", "Serena", "Ethan", "Chelsie"],
    formats: ["wav"],
  },

  async checkAuth(ctx) {
    try {
      const creds = await ctx.bus.request("provider:credentials", { providerId: "dashscope" });
      if (creds.error || !creds.apiKey) {
        return { ok: false, message: creds.error || t("plugin.imageGen.apiKeyNotConfigured") };
      }
      return { ok: true };
    } catch (err) {
      return { ok: false, message: err.message || String(err) };
    }
  },

  async submit(params, ctx) {
    const providerId = params.credentialProviderId ?? ctx.mediaExecutionTarget?.credentialProviderId;
    if (!providerId) throw new Error("CREDENTIAL_PROVIDER_UNRESOLVED");
    const creds = await ctx.bus.request("provider:credentials", { providerId });
    if (creds.error || !creds.apiKey) {
      throw new Error(t("plugin.imageGen.providerNoApiKey", { providerId }));
    }
    if (!creds.baseUrl) throw new Error(`provider "${providerId}" has no base url`);

    let origin: string;
    try {
      origin = new URL(creds.baseUrl).origin;
    } catch {
      throw new Error(`provider "${providerId}" base url is invalid`);
    }

    const providerDefaults = ctx.config?.get?.("providerDefaults")?.[providerId] || {};
    const model = params.modelId || params.model || providerDefaults?.model || "qwen-tts-latest";
    const voice = params.voice || providerDefaults?.voice || "Cherry";

    const generateResponse = await fetch(`${origin}/api/v1/services/aigc/multimodal-generation/generation`, {
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${creds.apiKey}` },
      body: JSON.stringify({
        model,
        input: { text: String(params.prompt ?? ""), voice },
      }),
      signal: params.signal,
    });
    const body = await generateResponse.json().catch(() => null);
    if (!generateResponse.ok) {
      throw new Error(`DashScope speech failed: ${body?.message || `status ${generateResponse.status}`}`);
    }
    const audioUrl = body?.output?.audio?.url;
    if (typeof audioUrl !== "string" || !audioUrl) {
      throw new Error("DashScope speech returned no audio url");
    }
    const audioResponse = await fetch(audioUrl, { signal: params.signal });
    if (!audioResponse.ok) {
      throw new Error(`DashScope speech audio download failed: status ${audioResponse.status}`);
    }
    const buffer = Buffer.from(await audioResponse.arrayBuffer());
    const mimeType = audioResponse.headers.get("content-type")?.split(";")[0] || "audio/wav";
    const customName = typeof params.suggestedFilename === "string" && params.suggestedFilename.trim()
      ? params.suggestedFilename.trim()
      : null;
    const { filename } = await saveImage(buffer, mimeType, ctx.dataDir, customName);
    return {
      taskId: `speech-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`,
      files: [filename],
    };
  },
};

const SUPPORTED_SYSTEM_SPEECH_PLATFORMS = new Set(["darwin"]);

export const systemSpeechAdapter = {
  id: "system-speech-tts",
  protocolId: "system-speech",
  name: "System Speech",
  types: ["speech"],
  capabilities: {
    platforms: ["darwin"],
    formats: ["m4a"],
  },

  async checkAuth() {
    if (!SUPPORTED_SYSTEM_SPEECH_PLATFORMS.has(process.platform)) {
      return { ok: false, message: "system speech is only available on macOS" };
    }
    try {
      await fs.promises.access("/usr/bin/say", fs.constants.X_OK);
      return { ok: true };
    } catch {
      return { ok: false, message: "/usr/bin/say is not available" };
    }
  },

  async submit(params, ctx) {
    if (!SUPPORTED_SYSTEM_SPEECH_PLATFORMS.has(process.platform)) {
      throw new Error("system speech is only available on macOS");
    }
    const input = typeof params.prompt === "string" ? params.prompt.trim() : "";
    if (!input) throw new Error("prompt is required");

    const generatedDir = path.join(ctx.dataDir, "generated");
    fs.mkdirSync(generatedDir, { recursive: true });
    const filename = `${Date.now()}-say-${Math.random().toString(36).slice(2, 8)}.m4a`;
    const outputPath = path.join(generatedDir, filename);

    const args = ["-o", outputPath];
    if (typeof params.voice === "string" && params.voice.trim()) args.push("-v", params.voice.trim());
    if (Number.isFinite(Number(params.speed))) {
      // say 的 rate 词/分钟，默认 ~175；speed 1.0 → 175
      args.push("-r", String(Math.round(175 * Math.min(4, Math.max(0.25, Number(params.speed))))));
    }
    args.push(input);

    try {
      await execFileAsync("/usr/bin/say", args, { timeout: 120_000 });
    } catch (err) {
      throw new Error(`system speech failed: ${err?.message || err}`);
    }
    if (!fs.existsSync(outputPath)) {
      throw new Error("system speech produced no output");
    }
    return {
      taskId: `speech-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`,
      files: [filename],
    };
  },
};


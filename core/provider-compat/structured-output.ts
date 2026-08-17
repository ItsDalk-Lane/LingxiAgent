/**
 * structured-output.ts — 「结构化输出」运行时 contract
 *
 * 模型级开关 model.structuredOutput === true 表示：该模型在普通交互聊天中
 * 必须产生合法标准 JSON。这是 Lingxi 的产品级抽象，不等价于 OpenAI 狭义的
 * json_schema —— 有些协议只保证 JSON Object，因此 UI 名保持「结构化输出」。
 *
 * 规则（fail closed，禁止 prompt-only 冒充强制 JSON）：
 * - 只有 purpose === "chat"（普通用户聊天）才继承模型开关；
 *   utility / compaction / summary 等内部调用一律不动 payload；
 * - 只有协议 contract 明确支持 JSON enforcement 时才注入机制参数；
 *   Anthropic Messages 等无原生机制的协议返回 unsupported，不注入；
 * - 不覆盖调用方已有的更具体 response format；
 * - pure / immutable / idempotent：不 mutate 输入 payload，
 *   多次 normalize 不会重复插入 JSON instruction。
 *
 * Wire 形状依据（Pi 0.84.1 serializer fixture，tests/provider-compat/structured-output.test.ts）：
 * - openai-completions: before_provider_request 拿到 REST body
 *   { model, messages: [{role:"system"|"developer"|..., content: string}], ... }，
 *   JSON mode = response_format: { type: "json_object" } + 消息中的 JSON 指令；
 * - openai-responses: REST body { model, input: [{role, content: string | part[]}], ... }，
 *   JSON mode = text: { format: { type: "json_object" } }（Pi serializer 本身从不设置 text 字段）；
 * - google-generative-ai: onPayload 拿到的是 @google/genai SDK 参数
 *   { model, contents, config }，JSON mode = config.responseMimeType = "application/json"。
 */

import * as zhipuCompat from "./zhipu.ts";

export type StructuredOutputMechanism =
  | "openai-json-object"
  | "openai-responses-json-object"
  | "google-json-mime";

export interface StructuredOutputContract {
  supported: boolean;
  mechanism?: StructuredOutputMechanism;
}

// OpenAI json_object 文档要求消息中必须已有 JSON 指令才会按该模式生成。
// 指令本身固定措辞，配合 MARKER 实现 idempotent 插入。
export const STRUCTURED_OUTPUT_JSON_INSTRUCTION =
  "Important: respond with a single valid JSON value (object, array, string, number, boolean, or null). Do not wrap the output in markdown code fences.";
const JSON_INSTRUCTION_MARKER = "valid JSON value";

function isOpenAiOfficialEndpoint(model) {
  if (!model || typeof model !== "object") return false;
  if (typeof model.provider === "string" && model.provider.toLowerCase() === "openai") return true;
  const baseUrl = typeof model.baseUrl === "string" ? model.baseUrl : "";
  if (!baseUrl) return false;
  try {
    return new URL(baseUrl).hostname.toLowerCase() === "api.openai.com";
  } catch {
    return false;
  }
}

/**
 * 解析模型当前协议是否具备「强制合法 JSON」机制。
 * provider identity 依据 model.api / provider / baseUrl，不通过 payload 形状猜。
 * 未知协议一律 unsupported（UI 侧据此禁用开关），不猜参数。
 */
export function resolveStructuredOutputContract(model): StructuredOutputContract {
  const api = typeof model?.api === "string" ? model.api : "";
  if (api === "openai-completions") {
    // 智谱 GLM 的 json_object 结构化输出契约未取得官方/fixture 证据（BLOCKED #4），
    // fail closed，不套 OpenAI 参数。
    if (zhipuCompat.matches(model)) return { supported: false };
    // api=openai-completions 是显式协议声明，json_object 是该协议标准 JSON mode。
    // 不依赖 hostname 猜能力：第三方 OpenAI 兼容网关（如 DashScope）按协议声明处理，
    // 用户显式开启后若 endpoint 拒绝则透传 provider 错误，不静默退回。
    return { supported: true, mechanism: "openai-json-object" };
  }
  if (api === "openai-responses" && isOpenAiOfficialEndpoint(model)) {
    return { supported: true, mechanism: "openai-responses-json-object" };
  }
  if (api === "google-generative-ai") {
    return { supported: true, mechanism: "google-json-mime" };
  }
  return { supported: false };
}

function textOfContent(content) {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .filter((part) => part && typeof part === "object" && typeof part.text === "string")
      .map((part) => part.text)
      .join("\n");
  }
  return null;
}

function withAppendedInstruction(content) {
  if (typeof content === "string") {
    return `${content}\n\n${STRUCTURED_OUTPUT_JSON_INSTRUCTION}`;
  }
  // 数组 content：追加一个 text part，而不是改写既有 part
  if (Array.isArray(content)) {
    return [...content, { type: "text", text: STRUCTURED_OUTPUT_JSON_INSTRUCTION }];
  }
  return content;
}

/**
 * 确保 system/developer/user 指令中存在 JSON 输出要求（幂等）。
 * 优先追加到已有的 system/developer 消息；都没有时在最前面插入一条 system 消息。
 */
function ensureJsonInstructionItems(items, { fallbackRole = "system", fallbackContentShape = "string" } = {}) {
  if (!Array.isArray(items)) return items;
  const instructionItem = [...items]
    .reverse()
    .find((item) => item && typeof item === "object"
      && (item.role === "system" || item.role === "developer" || item.role === "user")
      && (textOfContent(item.content) || "").includes(JSON_INSTRUCTION_MARKER));
  if (instructionItem) return items;

  const lastDirectiveIndex = findLastDirectiveItemIndex(items);
  if (lastDirectiveIndex >= 0) {
    return items.map((item, index) => (index === lastDirectiveIndex
      ? { ...item, content: withAppendedInstruction(item.content) }
      : item));
  }
  const fallbackItem = fallbackContentShape === "parts"
    ? { role: fallbackRole, content: [{ type: "text", text: STRUCTURED_OUTPUT_JSON_INSTRUCTION }] }
    : { role: fallbackRole, content: STRUCTURED_OUTPUT_JSON_INSTRUCTION };
  return [fallbackItem, ...items];
}

function findLastDirectiveItemIndex(items) {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (item && typeof item === "object" && (item.role === "system" || item.role === "developer")) {
      return index;
    }
  }
  return -1;
}

function applyOpenAIJsonObject(payload) {
  const next = { ...payload };
  // 调用方已有更具体 response format（如 json_schema）时不覆盖
  if (!next.response_format) {
    next.response_format = { type: "json_object" };
  }
  next.messages = ensureJsonInstructionItems(next.messages);
  return next;
}

function applyOpenAIResponsesJsonObject(payload) {
  const next = { ...payload };
  const text = next.text && typeof next.text === "object" && !Array.isArray(next.text) ? next.text : {};
  if (!text.format) {
    next.text = { ...text, format: { type: "json_object" } };
  }
  next.input = ensureJsonInstructionItems(next.input);
  return next;
}

function applyGoogleJsonMime(payload) {
  // Google serializer 的 payload 是 @google/genai SDK 参数 { model, contents, config }
  const config = payload.config && typeof payload.config === "object" && !Array.isArray(payload.config)
    ? payload.config
    : {};
  if (config.responseMimeType) return payload;
  return { ...payload, config: { ...config, responseMimeType: "application/json" } };
}

/**
 * 结构化输出注入入口。由 core/provider-compat.ts 的 normalizeProviderPayload
 * 在 provider 子模块之后调用（通用 capability 层）。
 */
export function applyStructuredOutput(payload, model, options: Record<string, any> = {}) {
  if (!payload || typeof payload !== "object") return payload;
  // 只有普通用户聊天继承模型开关；utility / compaction 等内部用途默认 false
  if (options.purpose !== "chat") return payload;
  if (!model || typeof model !== "object" || model.structuredOutput !== true) return payload;
  const contract = resolveStructuredOutputContract(model);
  if (!contract.supported) return payload;
  switch (contract.mechanism) {
    case "openai-json-object":
      return applyOpenAIJsonObject(payload);
    case "openai-responses-json-object":
      return applyOpenAIResponsesJsonObject(payload);
    case "google-json-mime":
      return applyGoogleJsonMime(payload);
    default:
      return payload;
  }
}

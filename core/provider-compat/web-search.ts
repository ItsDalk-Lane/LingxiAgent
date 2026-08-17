/**
 * web-search.ts — 「联网」运行时 contract（模型级开关 → 供应商原生联网搜索）
 *
 * 规则：
 * - 只有 purpose === "chat"（普通用户聊天）且 model.web === true 时才注入；
 *   utility / compaction 等内部调用一律不联网；
 * - immutable / idempotent：不 mutate 输入 payload，重复 normalize 不重复注入；
 * - tools 去重：已有同语义 web tool 时不重复添加；不覆盖现有 tool list；
 * - 没有 contract 证据的 provider 一律 unsupported（fail closed），
 *   不通过 hostname 猜协议、不猜参数。
 *
 * 第一批 contract 与证据：
 * - google-generative-ai：Search Grounding 使用 tools: [{ googleSearch: {} }]。
 *   Pi serializer onPayload 拿到的是 @google/genai SDK 参数 { model, contents, config }，
 *   config.tools 由 convertTools 全部包装成 functionDeclarations，因此原生联网
 *   tool 必须在 before_provider_request 阶段以独立 { googleSearch: {} } 条目追加
 *   （fixture：tests/provider-compat/web-search.test.ts）。
 * - 智谱（zhipu / zhipu-coding，openai-completions）：官方文档 wire 形状为
 *   tools: [{ type: "web_search", web_search: { enable: true, search_result: true } }]
 *   （docs.bigmodel.cn/cn/guide/tools/web-search）。显式 Zhipu contract，
 *   不因 OpenAI 兼容而套用 OpenAI 参数。
 *
 * 明确 unsupported（SDK 解析层无法处理 server-tool 生命周期，fail closed）：
 * - Anthropic Messages：pi-ai content_block_start 只处理
 *   text|thinking|redacted_thinking|tool_use，server_tool_use / web_search_tool_result
 *   块被静默丢弃；pause_turn 被映射为普通 stop，continuation 生命周期无法完成。
 * - OpenAI Responses：请求侧 tools: [{ type: "web_search" }] 虽可注入，但 pi-ai
 *   createSlot() 只为 reasoning|message|function_call|custom_tool_call 建 slot，
 *   web_search_call 响应 item 被丢弃，且 store:false 回放要求保留完整 item 列表。
 * - OpenAI Chat Completions：官方 host 不构成任意模型的联网 wire contract 证明，
 *   不按 hostname 猜（任务书 #34）。
 * - Moonshot：仓库内无 adapter/fixture 证据，官方协议未在本次执行中核验 → BLOCKED.md。
 */

import * as zhipuCompat from "./zhipu.ts";

export type NativeWebSearchMechanism =
  | "google-search-grounding"
  | "zhipu-web-search";

export interface NativeWebSearchContract {
  supported: boolean;
  mechanism?: NativeWebSearchMechanism;
}

export function resolveNativeWebSearchContract(model): NativeWebSearchContract {
  const api = typeof model?.api === "string" ? model.api : "";
  if (api === "google-generative-ai") {
    return { supported: true, mechanism: "google-search-grounding" };
  }
  if (api === "openai-completions" && zhipuCompat.matches(model)) {
    return { supported: true, mechanism: "zhipu-web-search" };
  }
  return { supported: false };
}

function hasGoogleSearchToolEntry(tools) {
  return tools.some((tool) => tool && typeof tool === "object" && tool.googleSearch !== undefined);
}

function applyGoogleSearchGrounding(payload) {
  const config = payload.config && typeof payload.config === "object" && !Array.isArray(payload.config)
    ? payload.config
    : {};
  const tools = Array.isArray(config.tools) ? config.tools : [];
  if (hasGoogleSearchToolEntry(tools)) return payload;
  // 独立 { googleSearch: {} } 条目与现有 functionDeclarations 共存，不覆盖
  return {
    ...payload,
    config: { ...config, tools: [...tools, { googleSearch: {} }] },
  };
}

function hasZhipuWebSearchTool(tools) {
  return tools.some((tool) => tool && typeof tool === "object" && tool.type === "web_search");
}

function applyZhipuWebSearch(payload) {
  const tools = Array.isArray(payload.tools) ? payload.tools : [];
  if (hasZhipuWebSearchTool(tools)) return payload;
  return {
    ...payload,
    tools: [...tools, {
      type: "web_search",
      web_search: { enable: true, search_result: true },
    }],
  };
}

/**
 * 原生联网注入入口。由 core/provider-compat.ts 的 normalizeProviderPayload
 * 在 provider 子模块之后调用（通用 capability 层）。
 */
export function applyNativeWebSearch(payload, model, options: Record<string, any> = {}) {
  if (!payload || typeof payload !== "object") return payload;
  if (options.purpose !== "chat") return payload;
  if (!model || typeof model !== "object" || model.web !== true) return payload;
  const contract = resolveNativeWebSearchContract(model);
  if (!contract.supported) return payload;
  switch (contract.mechanism) {
    case "google-search-grounding":
      return applyGoogleSearchGrounding(payload);
    case "zhipu-web-search":
      return applyZhipuWebSearch(payload);
    default:
      return payload;
  }
}

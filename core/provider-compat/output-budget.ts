/**
 * Generic output budget normalization.
 *
 * This module only handles provider-independent request policy. Provider wire
 * details stay in provider-compat/<provider>.js modules.
 */

import { getOutputThinkingComposition } from "../../shared/model-capabilities.ts";

export const DEFAULT_CHAT_OUTPUT_TOKENS = 65_536;
/**
 * 「最大输出包含思维链」模型的聊天默认值里给思考留的余量。
 *
 * 这些模型（Anthropic Messages、Gemini、OpenAI Responses、显式声明的 OpenAI 兼容
 * 推理通道）的思考 token 计入输出上限，所以合成默认值不能按纯答案封顶——否则
 * 思考会把最终回答挤没。余量对齐 pi SDK adjustMaxTokensForThinking 的 high 档，
 * 覆盖不完整思考被截断的最常见场景；用户显式设置的预算不经这里。
 */
const INCLUDED_THINKING_DEFAULT_HEADROOM = 16_384;
const OUTPUT_CAP_FIELDS = [
  "max_completion_tokens",
  "max_tokens",
  "max_output_tokens",
  "maxOutputTokens",
];
const OUTPUT_CAP_FIELD_SET = new Set(OUTPUT_CAP_FIELDS);

const DEFAULT_OUTPUT_CAP_CAPABILITY = Object.freeze({
  id: "default-optional",
  required: false,
  preserveImplicitSdkDefault: false,
});
const OUTPUT_BUDGET_SOURCE_UNSPECIFIED = "unspecified";
const PRESERVED_OUTPUT_BUDGET_SOURCES = new Set(["user", "system"]);

function lower(value) {
  return typeof value === "string" ? value.toLowerCase() : "";
}

function positiveInteger(value) {
  const n = Number(value);
  return Number.isFinite(n) && n > 0 ? Math.floor(n) : null;
}

function getModelOutputLimit(model) {
  return positiveInteger(model?.maxTokens || model?.maxOutput);
}

function isOfficialDeepSeekEndpoint(model) {
  const provider = lower(model?.provider);
  const baseUrl = lower(model?.baseUrl || model?.base_url);
  return provider === "deepseek" || baseUrl.includes("api.deepseek.com");
}

const OUTPUT_CAP_CAPABILITIES = [
  {
    id: "explicit-required",
    required: true,
    preserveImplicitSdkDefault: true,
    matches: (model) => model?.compat?.outputCapRequired === true,
  },
  {
    id: "official-deepseek",
    required: false,
    preserveImplicitSdkDefault: true,
    matches: isOfficialDeepSeekEndpoint,
  },
  {
    id: "anthropic-native",
    required: true,
    preserveImplicitSdkDefault: true,
    matches: (model) => lower(model?.provider) === "anthropic"
      || lower(model?.baseUrl || model?.base_url).includes("api.anthropic.com"),
  },
  {
    id: "bedrock-native",
    required: true,
    preserveImplicitSdkDefault: true,
    matches: (model) => {
      const provider = lower(model?.provider);
      return provider === "amazon-bedrock" || provider === "bedrock";
    },
  },
  {
    id: "anthropic-messages",
    required: true,
    preserveImplicitSdkDefault: true,
    matches: (model) => lower(model?.api) === "anthropic-messages",
  },
];

export function resolveOutputCapCapability(model) {
  if (!model || typeof model !== "object") return DEFAULT_OUTPUT_CAP_CAPABILITY;
  return OUTPUT_CAP_CAPABILITIES.find((capability) => capability.matches(model))
    || DEFAULT_OUTPUT_CAP_CAPABILITY;
}

function hasOutputCap(payload) {
  return OUTPUT_CAP_FIELDS.some((field) => Object.prototype.hasOwnProperty.call(payload, field));
}

function resolveOutputCapField(model) {
  const explicit = model?.compat?.outputCapField;
  if (typeof explicit === "string" && OUTPUT_CAP_FIELD_SET.has(explicit)) return explicit;
  // Responses 协议的输出预算字段是 max_output_tokens；发 max_tokens 会被静默忽略。
  const api = lower(model?.api);
  if (api === "openai-responses" || api === "openai-codex-responses") return "max_output_tokens";
  return "max_tokens";
}

function resolveOutputBudgetSource(options: Record<string, any> = {}) {
  const outputBudgetSource = lower(options.outputBudgetSource);
  if (outputBudgetSource) return outputBudgetSource;
  const maxTokensSource = lower(options.maxTokensSource);
  if (maxTokensSource) return maxTokensSource;
  if (positiveInteger(options.userMaxTokens) !== null) return "user";
  return OUTPUT_BUDGET_SOURCE_UNSPECIFIED;
}

export function resolveOutputBudgetPolicy(model, options: Record<string, any> = {}) {
  const mode = options.mode || "chat";
  const source = resolveOutputBudgetSource(options);
  const capability = resolveOutputCapCapability(model);
  const preserveForSource = PRESERVED_OUTPUT_BUDGET_SOURCES.has(source);
  // 按模型解析「输出上限是否已包含思维链」。两类语义必须分流，不能统一计算：
  //   - separate：maxTokens 只约束最终回答（DeepSeek reasoner 等），思维链在
  //     服务端独立预算。历史 64K 封顶语义就是按这个口径写的，保持不变。
  //   - included：思考消耗计入 maxTokens。默认合成值改为「答案目标 + 一档思考
  //     余量」，封顶/降档时以合并值为基准，避免默认链路把回答挤压到没有思考空间。
  const thinkingSharesOutput = getOutputThinkingComposition(model) === "included";
  const modelLimit = getModelOutputLimit(model);
  const defaultMaxTokens = thinkingSharesOutput
    ? Math.min(
        (modelLimit ?? DEFAULT_CHAT_OUTPUT_TOKENS + INCLUDED_THINKING_DEFAULT_HEADROOM),
        DEFAULT_CHAT_OUTPUT_TOKENS + INCLUDED_THINKING_DEFAULT_HEADROOM,
      )
    : Math.min(modelLimit ?? DEFAULT_CHAT_OUTPUT_TOKENS, DEFAULT_CHAT_OUTPUT_TOKENS);
  const applyChatDefault = mode === "chat" && !preserveForSource;
  // `before_provider_request` sees the final serialized payload. Most current
  // serializers include the SDK-derived cap, but optional providers may omit
  // it. An absent field in ordinary chat still means Hana's default policy,
  // not an invitation to fall back to an unknown provider default.
  const synthesizeChatDefault = applyChatDefault;

  return {
    mode,
    source,
    capability,
    preserveForSource,
    thinkingSharesOutput,
    modelLimit,
    defaultMaxTokens,
    applyChatDefault,
    synthesizeChatDefault,
  };
}

/**
 * Normalize the final serialized request budget.
 *
 * Pi SDK already tightens its derived maxTokens to the remaining context before
 * this hook runs. Hana therefore only lowers an existing default-derived cap to
 * its own target; it never raises a smaller value. The chat-default target is
 * composition-aware: pure-answer targets for "separate" models, answer+thinking
 * headroom for "included" models. User/system-owned values may exceed the
 * default target, but still cannot exceed the model's declared output capability.
 */
export function normalizeImplicitOutputBudget(payload, model, options: Record<string, any> = {}) {
  if (!payload || typeof payload !== "object") return payload;
  const policy = resolveOutputBudgetPolicy(model, options);
  let next = payload;

  const hasPromptInput = Array.isArray(next.messages)
    || Array.isArray(next.input)
    || typeof next.input === "string";
  if (!hasOutputCap(next) && hasPromptInput) {
    const synthesizedCap = (policy.synthesizeChatDefault || (policy.applyChatDefault && policy.capability.required))
      ? policy.defaultMaxTokens
      : (policy.capability.required ? policy.modelLimit : null);
    if (synthesizedCap !== null) {
      next = { ...next, [resolveOutputCapField(model)]: synthesizedCap };
    }
  }

  const upperBound = policy.applyChatDefault
    ? policy.defaultMaxTokens
    : policy.modelLimit;
  if (upperBound === null) return next;

  for (const field of OUTPUT_CAP_FIELDS) {
    if (!Object.prototype.hasOwnProperty.call(next, field)) continue;
    const current = positiveInteger(next[field]);
    if (current === null) continue;
    const bounded = Math.min(current, upperBound);
    if (bounded === current) continue;
    if (next === payload) next = { ...payload };
    next[field] = bounded;
  }

  return next;
}

/**
 * 最终序列化请求的输出预算事实（Output Budget Fact）。
 *
 * 借鉴 deepseek-harness 的 adapter-owned materialized defaults：在请求派发边界
 * 把「这个预算是谁定的」物化成持久 request fact（Hana 落在 model_attempts 的
 * safe_details_json，即 provider_request_prepared 事件的 details），而不是只活在
 * 单次请求的局部变量里。没有它，事后排查时只能看到 `max_tokens: N` 这个数，
 * 分不清是用户选的、SDK 按剩余窗口算的、还是 Hana 合成的默认值——三者对
 * 「设置页声明值该不该生效」的回答完全不同。
 *
 * ownership 取值：
 *   - absent          ：最终 body 没有输出上限字段（无 prompt 输入时不产生 fact）
 *   - user-explicit   ：调用方标记 outputBudgetSource="user"
 *   - system-explicit ：调用方标记 outputBudgetSource="system"（utility 任务预算等）
 *   - hana-chat-default：值恰好等于本次解析出的 Hana 聊天默认值（含 included
 *     家族的 +16K 思考余量），或被 cap 到它 —— 默认策略的产物
 *   - sdk-derived      ：其余默认来源值（pi SDK clampMaxTokensToContext / 模型
 *     目录能力值透传），不是用户意图也不是 Hana 默认策略
 */
export type OutputBudgetFact = {
  field: string | null;
  value: number | null;
  composition: "included" | "separate";
  ownership: "absent" | "user-explicit" | "system-explicit" | "hana-chat-default" | "sdk-derived";
  chatDefault: number | null;
  declaredMaxOutput: number | null;
};

function findOutputCapField(payload: Record<string, any>): { field: string; value: number } | null {
  for (const field of OUTPUT_CAP_FIELDS) {
    if (!Object.prototype.hasOwnProperty.call(payload, field)) continue;
    const value = positiveInteger(payload[field]);
    if (value !== null) return { field, value };
  }
  return null;
}

export function resolveOutputBudgetFact(
  payload: any,
  model: any,
  options: Record<string, any> = {},
): OutputBudgetFact | null {
  if (!payload || typeof payload !== "object") return null;

  // 与 normalizeImplicitOutputBudget 同款 prompt 存在性判定；gemini 走 contents。
  const hasPromptInput = Array.isArray(payload.messages)
    || Array.isArray(payload.input)
    || typeof payload.input === "string"
    || Array.isArray(payload.contents);
  const cap = findOutputCapField(payload);
  if (!cap && !hasPromptInput) return null;

  const policy = resolveOutputBudgetPolicy(model, options);
  let ownership: OutputBudgetFact["ownership"];
  if (!cap) {
    ownership = "absent";
  } else if (policy.source === "user") {
    ownership = "user-explicit";
  } else if (policy.source === "system") {
    ownership = "system-explicit";
  } else if (cap.value === policy.defaultMaxTokens
    || (policy.capability.required && cap.value === policy.modelLimit)) {
    // 命中协议必填兜底（synthesize modelLimit）也属于 Hana 合成默认值。
    ownership = "hana-chat-default";
  } else {
    ownership = "sdk-derived";
  }

  return {
    field: cap?.field ?? null,
    value: cap?.value ?? null,
    composition: policy.thinkingSharesOutput ? "included" : "separate",
    ownership,
    chatDefault: policy.defaultMaxTokens,
    declaredMaxOutput: policy.modelLimit,
  };
}

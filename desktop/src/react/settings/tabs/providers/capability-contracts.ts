/**
 * capability-contracts.ts — 模型能力开关的 UI 支持状态
 *
 * 运行时唯一真理由 core/provider-compat/structured-output.ts 与
 * core/provider-compat/web-search.ts 实现（provider identity 依据
 * model.api / provider / baseUrl，不靠 payload 形状猜）。renderer 不能
 * import core/（node 依赖），这里镜像同一套判定规则用于 UI 的
 * ON/OFF/disabled 展示；两侧规则由各自测试约束保持一致。
 *
 * 协议契约是二值的：协议要么具备该 wire 机制（supported），要么没有
 * （unsupported）。没有「可尝试」的通用兜底——没有 contract 证据的协议
 * 一律 fail closed 为不可开启，不猜参数、不靠 hostname 猜能力。
 * 「unknown（未检测到）→ 默认 OFF」的三态区分属于模型 metadata 自动检测层，
 * 与这里的协议契约是两回事；编辑面板对无法检测的能力默认 OFF。
 */

export type CapabilitySupportState = 'supported' | 'unsupported';

export interface ModelContractIdentity {
  providerId?: string;
  api?: string | null;
  baseUrl?: string | null;
}

function isOpenAiOfficialEndpoint({ providerId, baseUrl }: ModelContractIdentity): boolean {
  if (providerId && providerId.toLowerCase() === 'openai') return true;
  const url = typeof baseUrl === 'string' ? baseUrl : '';
  if (!url) return false;
  try {
    return new URL(url).hostname.toLowerCase() === 'api.openai.com';
  } catch {
    return false;
  }
}

function isZhipuOpenAICompat({ providerId, api, baseUrl }: ModelContractIdentity): boolean {
  if (api !== 'openai-completions') return false;
  if (providerId === 'zhipu' || providerId === 'zhipu-coding') return true;
  const url = typeof baseUrl === 'string' ? baseUrl : '';
  if (url.includes('open.bigmodel.cn')) return true;
  return url.includes('api.z.ai')
    && (url.includes('/api/paas/v4') || url.includes('/api/coding/paas/v4'));
}

/**
 * 「结构化输出」：协议是否具备强制合法 JSON 机制。
 * - openai-completions 的 json_object 是协议标准 JSON mode，按 api 声明支持
 *   （与 runtime resolveStructuredOutputContract 对齐，第三方 OpenAI 兼容
 *   网关如 DashScope 同样按协议声明处理）；智谱除外（无官方/fixture 证据，
 *   unsupported，见 BLOCKED #4）。
 * - openai-responses 仅官方 endpoint 支持。
 * - google-generative-ai 支持 responseMimeType。
 * - 其余（含 Anthropic Messages 等无原生机制）unsupported。
 */
export function structuredOutputSupportState(identity: ModelContractIdentity): CapabilitySupportState {
  const api = typeof identity.api === 'string' ? identity.api : '';
  if (api === 'openai-completions') {
    return isZhipuOpenAICompat(identity) ? 'unsupported' : 'supported';
  }
  if (api === 'openai-responses') {
    return isOpenAiOfficialEndpoint(identity) ? 'supported' : 'unsupported';
  }
  if (api === 'google-generative-ai') return 'supported';
  return 'unsupported';
}

/**
 * 「联网」：协议是否具备原生联网 contract。
 * 已知支持：google-generative-ai、智谱。其余（Anthropic / OpenAI Responses /
 * OpenAI Chat Completions / Moonshot 等）要么 SDK parser 无法处理 server-tool
 * 生命周期，要么无 contract 证据，一律 fail closed 为 unsupported。
 */
export function nativeWebSearchSupportState(identity: ModelContractIdentity): CapabilitySupportState {
  const api = typeof identity.api === 'string' ? identity.api : '';
  if (api === 'google-generative-ai') return 'supported';
  if (isZhipuOpenAICompat(identity)) return 'supported';
  return 'unsupported';
}

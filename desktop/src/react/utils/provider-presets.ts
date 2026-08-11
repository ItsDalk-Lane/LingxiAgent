export interface ProviderPreset {
  value: string;
  label: string;
  labelZh?: string;
  url: string;
  api: string;
  local?: boolean;
  custom?: boolean;
}

export const API_PROVIDER_PRESETS: ProviderPreset[] = [
  { value: 'ollama',      label: 'Ollama (Local)',       labelZh: 'Ollama (本地)',       url: 'http://localhost:11434/v1', api: 'openai-completions', local: true },
  { value: 'dashscope',   label: 'DashScope (Qwen)',     url: 'https://dashscope.aliyuncs.com/compatible-mode/v1', api: 'openai-completions' },
  { value: 'qwen-token-plan-individual', label: 'Qwen Token Plan (Individual)', url: 'https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1', api: 'openai-completions' },
  { value: 'qwen-token-plan', label: 'Qwen Token Plan',  url: 'https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1', api: 'openai-completions' },
  { value: 'qwen-token-plan-cn', label: 'Qwen Token Plan (CN)', labelZh: 'Qwen Token Plan (国内)', url: 'https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1', api: 'openai-completions' },
  { value: 'openai',      label: 'OpenAI',               url: 'https://api.openai.com/v1', api: 'openai-completions' },
  { value: 'xai',         label: 'xAI (Grok)',           url: 'https://api.x.ai/v1', api: 'openai-completions' },
  { value: 'gemini',      label: 'Google Gemini',        url: 'https://generativelanguage.googleapis.com/v1beta', api: 'google-generative-ai' },
  { value: 'deepseek',    label: 'DeepSeek',             url: 'https://api.deepseek.com', api: 'openai-completions' },
  { value: 'deepseek-responses', label: 'DeepSeek (Responses)', url: 'https://api.deepseek.com', api: 'openai-responses' },
  { value: 'volcengine',  label: 'Volcengine (Doubao)',  labelZh: 'Volcengine (豆包)',   url: 'https://ark.cn-beijing.volces.com/api/v3', api: 'openai-completions' },
  { value: 'moonshot',    label: 'Moonshot (Kimi)',      url: 'https://api.moonshot.cn/v1', api: 'openai-completions' },
  { value: 'moonshotai',  label: 'Moonshot (International)', url: 'https://api.moonshot.ai/v1', api: 'openai-completions' },
  { value: 'kimi-coding', label: 'Kimi Coding Plan',     url: 'https://api.kimi.com/coding/', api: 'anthropic-messages' },
  { value: 'zhipu',       label: 'Zhipu (GLM)',          url: 'https://open.bigmodel.cn/api/paas/v4', api: 'openai-completions' },
  { value: 'siliconflow', label: 'SiliconFlow',          url: 'https://api.siliconflow.cn/v1', api: 'openai-completions' },
  { value: 'groq',        label: 'Groq',                 url: 'https://api.groq.com/openai/v1', api: 'openai-completions' },
  { value: 'mistral',     label: 'Mistral',              url: 'https://api.mistral.ai/v1', api: 'openai-completions' },
  { value: 'minimax',     label: 'MiniMax (CN)',         labelZh: 'MiniMax (国内)',       url: 'https://api.minimaxi.com/anthropic', api: 'anthropic-messages' },
  { value: 'minimax-intl', label: 'MiniMax (International)', url: 'https://api.minimax.io/anthropic', api: 'anthropic-messages' },
  { value: 'minimax-token-plan', label: 'MiniMax Token Plan', url: 'https://api.minimaxi.com/anthropic', api: 'anthropic-messages' },
  { value: 'openrouter',  label: 'OpenRouter',           url: 'https://openrouter.ai/api/v1', api: 'openai-completions' },
  { value: 'mimo',        label: 'Xiaomi (MiMo)',        url: 'https://api.xiaomimimo.com/v1', api: 'openai-completions' },
  { value: 'mimo-token-plan', label: 'Xiaomi MiMo Token Plan (CN)', labelZh: 'Xiaomi MiMo Token Plan (国内)', url: 'https://token-plan-cn.xiaomimimo.com/v1', api: 'openai-completions' },
  { value: 'mimo-token-plan-sgp', label: 'Xiaomi MiMo Token Plan (SGP)', url: 'https://token-plan-sgp.xiaomimimo.com/v1', api: 'openai-completions' },
  { value: 'mimo-token-plan-ams', label: 'Xiaomi MiMo Token Plan (AMS)', url: 'https://token-plan-ams.xiaomimimo.com/v1', api: 'openai-completions' },
  { value: 'zai',         label: 'Z.AI Coding',          url: 'https://api.z.ai/api/coding/paas/v4', api: 'openai-completions' },
  { value: 'zai-coding-cn', label: 'Z.AI Coding (CN)',   labelZh: 'Z.AI Coding (国内)',  url: 'https://open.bigmodel.cn/api/coding/paas/v4', api: 'openai-completions' },
  { value: 'ant-ling',    label: 'Ant Ling',             labelZh: '蚂蚁百灵',            url: 'https://api.ant-ling.com/v1', api: 'openai-completions' },
];

function currentLocale(): string | undefined {
  return typeof window === 'undefined' ? undefined : window.i18n?.locale;
}

export function getProviderPresetLabel(preset: ProviderPreset, locale = currentLocale()): string {
  return locale?.startsWith('zh') && preset.labelZh ? preset.labelZh : preset.label;
}

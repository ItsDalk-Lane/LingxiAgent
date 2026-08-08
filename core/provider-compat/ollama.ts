/**
 * Ollama OpenAI-compatible provider 兼容层。
 *
 * 处理 provider:
 *   - model.provider === "ollama"（任何走 OpenAI 兼容 /v1/chat/completions 的 ollama 模型）
 *
 * 解决的协议问题：
 *   1. 结构化输出：Ollama OpenAI 兼容层原生支持 response_format（与官方文档一致）。
 *      当 model 声明 supportsStructuredOutput 且请求 options 带 responseSchema 时，
 *      注入 OpenAI 风格的 response_format.json_schema。
 *      参考文档：https://docs.ollama.com/features/structured-outputs
 *
 * 不在本模块处理的（走通用机制）：
 *   - thinking/reasoning：Ollama OpenAI 兼容层不认 think 字段，reasoning trace 通过
 *     reasoning_content 字段返回，由 Pi SDK + chat 路由的通用解析自动处理。
 *   - tools：toolUse 契约只做元数据声明，tools 是否发送由 session context 决定。
 *
 * 接口契约：见 ./README.md
 */

function lower(value) {
  return typeof value === "string" ? value.toLowerCase() : "";
}

export function matches(model) {
  if (!model || typeof model !== "object") return false;
  return lower(model.provider) === "ollama";
}

export function apply(payload, model, options: Record<string, any> = {}) {
  if (!payload || typeof payload !== "object") return payload;

  let result = payload;

  // 结构化输出：Ollama OpenAI 兼容层通过 response_format 透传 JSON schema。
  // options.responseSchema 由调用方（chat 路由 / utility）提供，未接通时不注入。
  const schema = options?.responseSchema;
  if (schema && typeof schema === "object") {
    result = {
      ...result,
      response_format: {
        type: "json_schema",
        json_schema: {
          name: "structured_output",
          schema,
          strict: true,
        },
      },
    };
  }

  // num_ctx 桥接：把项目的 contextWindow（用户设置 / 自动探测）透传给 ollama 推理引擎。
  // 没有这层桥接时，ollama 用自己的默认 num_ctx（通常 4096），导致项目发的长输入
  // （系统提示 + 工具 schema）无法命中 prompt cache，每轮都全量 prefill——本地大模型
  // 上表现为首 token 延迟极高（几分钟）。
  //
  // num_ctx 主要影响 KV cache 预分配 + prompt cache 窗口大小：输入超过 num_ctx 时
  // ollama 会自动扩展以容纳输入（不截断），但超出部分无法命中 cache。
  const ctxWindow = typeof model?.contextWindow === "number" ? model.contextWindow : null;
  if (ctxWindow && ctxWindow > 0) {
    const existingOptions = isPlainObject(result.options) ? result.options : {};
    result = {
      ...result,
      options: { ...existingOptions, num_ctx: ctxWindow },
    };
  }

  return result;
}

function isPlainObject(value: any): value is Record<string, any> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

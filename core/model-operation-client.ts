import {
  beginObservedModelCall,
  captureProviderHttpResponse,
  failObservedModelCall,
  observedModelCallLedgerMetadata,
  observedProviderFetch,
} from "../lib/llm/model-call-integration.ts";
import { extractProviderRequestId } from "../lib/llm/model-call-observer.ts";
import { withModelRequestAccounting } from "../lib/llm/model-request-accounting.ts";
import {
  MODEL_OPERATION_PROTOCOLS,
  MODEL_OPERATION_RERANK_MAX_DOCS,
  type ModelOperation,
} from "../shared/model-operations.ts";

const DEFAULT_TIMEOUT_MS = 30_000;
const MAX_EMBED_INPUTS = 128;
const MAX_RERANK_DOCUMENTS = MODEL_OPERATION_RERANK_MAX_DOCS;
const MAX_TEXT_CHARS = 32_000;
const MAX_TOTAL_TEXT_CHARS = 500_000;

// 已识别的操作协议名，按下标解构出可读常量用于 switch 分发。
// 注意：MODEL_OPERATION_PROTOCOLS 的既有顺序是解构契约，新协议只允许末尾追加。
const [
  PROTOCOL_OPENAI_EMBEDDINGS,
  PROTOCOL_OLLAMA_EMBED,
  PROTOCOL_GEMINI_EMBED,
  PROTOCOL_VOYAGE_EMBEDDINGS,
  PROTOCOL_COHERE_RERANK,
  PROTOCOL_SILICONFLOW_RERANK,
  PROTOCOL_VOYAGE_RERANK,
  PROTOCOL_DASHSCOPE_RERANK,
  PROTOCOL_MINIMAX_EMBEDDINGS,
] = MODEL_OPERATION_PROTOCOLS;

/** embed() 的输入用途：voyage 协议映射为 input_type，其余协议忽略。 */
type EmbeddingInputType = "document" | "query";

type OperationResolver = (operation: ModelOperation) => Promise<any | null>;

export class ModelOperationRequestError extends Error {
  readonly code: string;
  readonly statusCode: number | null;
  readonly retryable: boolean;

  constructor(code: string, message: string, options: { statusCode?: number | null; retryable?: boolean } = {}) {
    super(message);
    this.name = "ModelOperationRequestError";
    this.code = code;
    this.statusCode = options.statusCode ?? null;
    this.retryable = options.retryable === true;
  }
}

export interface ModelOperationClientOptions {
  resolveOperationFresh: OperationResolver;
  fetch?: typeof globalThis.fetch;
  getUsageLedger?: () => any;
  timeoutMs?: number;
}

function assertTextList(value: unknown, label: string, maxItems: number): string[] {
  if (!Array.isArray(value) || value.length === 0 || value.length > maxItems) {
    throw new ModelOperationRequestError(
      "invalid_input",
      `${label} must contain between 1 and ${maxItems} items`,
    );
  }
  let total = 0;
  const texts = value.map((item, index) => {
    if (typeof item !== "string" || !item.trim() || item.length > MAX_TEXT_CHARS) {
      throw new ModelOperationRequestError(
        "invalid_input",
        `${label}[${index}] must be a non-empty string no longer than ${MAX_TEXT_CHARS} characters`,
      );
    }
    total += item.length;
    return item;
  });
  if (total > MAX_TOTAL_TEXT_CHARS) {
    throw new ModelOperationRequestError(
      "invalid_input",
      `${label} exceeds the total character limit`,
    );
  }
  return texts;
}

function trimBaseUrl(baseUrl: string): string {
  const trimmed = String(baseUrl || "").replace(/\/+$/, "");
  if (!trimmed) throw new ModelOperationRequestError("base_url_missing", "Provider base URL is unavailable");
  return trimmed;
}

function operationUrl(baseUrl: string, suffix: "embeddings" | "rerank" | "reranks"): string {
  const trimmed = trimBaseUrl(baseUrl);
  return trimmed.endsWith(`/${suffix}`) ? trimmed : `${trimmed}/${suffix}`;
}

// ollama 嵌入端点随 base 形状而变：OpenAI 兼容前缀 /v1 先剥掉（自添加模型继承的供应商默认
// base 带 /v1，原生端点不在其下）；base 已以 /api 结尾只补 /embed，否则补 /api/embed
function ollamaEmbedUrl(baseUrl: string): string {
  let trimmed = trimBaseUrl(baseUrl);
  if (trimmed.endsWith("/v1")) trimmed = trimmed.slice(0, -"/v1".length);
  return trimmed.endsWith("/api") ? `${trimmed}/embed` : `${trimmed}/api/embed`;
}

// DashScope 的 compatible-mode 网关不承载 rerank：base 命中 compatible-mode 时改写为
// compatible-api 并拼官方复数端点 /reranks；未命中（其余供应商）保持单数 /rerank。
function cohereRerankUrl(baseUrl: string): string {
  if (!baseUrl.includes("/compatible-mode/v1")) return operationUrl(baseUrl, "rerank");
  return operationUrl(baseUrl.replace("/compatible-mode/v1", "/compatible-api/v1"), "reranks");
}

// DashScope 原生重排端点（gte-rerank 系与 qwen3-vl-rerank 系专用，嵌套请求协议）：路径与
// base 无关，只取域名（国际站 dashscope-intl 同样成立）。
function dashscopeNativeRerankUrl(baseUrl: string): string {
  try {
    return `${new URL(trimBaseUrl(baseUrl)).origin}/api/v1/services/rerank/text-rerank/text-rerank`;
  } catch {
    throw new ModelOperationRequestError("invalid_input", "Provider base URL is invalid");
  }
}

// DashScope 兼容重排端点（qwen3-rerank* 专用，扁平 cohere 形状）：官方路径为
// compatible-api/v1/reranks（复数）；用户自定义 base 未含 compatible-mode 时按原样拼接。
function dashscopeCompatibleRerankUrl(baseUrl: string): string {
  const rewritten = baseUrl.includes("/compatible-mode/v1")
    ? baseUrl.replace("/compatible-mode/v1", "/compatible-api/v1")
    : baseUrl;
  return operationUrl(rewritten, "reranks");
}

// MiniMax 官方 embeddings 是自有协议：GroupId 为必填 URL query 参数（模型条目
// groupId 字段携带），端点固定在域名根的 /v1/embeddings（供应商 base 指向
// /anthropic chat 网关，不能直接拼接）。
function minimaxEmbeddingUrl(baseUrl: string, groupId: string): string {
  try {
    const origin = new URL(trimBaseUrl(baseUrl)).origin;
    return `${origin}/v1/embeddings?GroupId=${encodeURIComponent(groupId)}`;
  } catch {
    throw new ModelOperationRequestError("invalid_input", "Provider base URL is invalid");
  }
}

function hasHeader(headers: Record<string, string>, name: string): boolean {
  return Object.keys(headers).some((key) => key.toLowerCase() === name.toLowerCase());
}

function requestHeaders(execution: any): Record<string, string> {
  const headers: Record<string, string> = {
    ...(execution?.headers || {}),
    "Content-Type": "application/json",
  };
  if (execution?.apiKey && !hasHeader(headers, "authorization")) {
    headers.Authorization = `Bearer ${execution.apiKey}`;
  }
  return headers;
}

// ollama/gemini 等原生协议认证方言：先剥掉 Authorization，保证最终请求不携带 Bearer
function headersWithoutAuthorization(source: Record<string, string>): Record<string, string> {
  return Object.fromEntries(
    Object.entries(source).filter(([key]) => key.toLowerCase() !== "authorization"),
  );
}

function protocolHeaders(execution: any, auth: "bearer" | "none" | "google"): Record<string, string> {
  if (auth === "bearer") return requestHeaders(execution);
  const headers = headersWithoutAuthorization(execution?.headers || {});
  headers["Content-Type"] = "application/json";
  if (auth === "google" && execution?.apiKey && !hasHeader(headers, "x-goog-api-key")) {
    headers["x-goog-api-key"] = String(execution.apiKey);
  }
  return headers;
}

function passthroughResponse(body: any) {
  return body;
}

// 以下归一化函数把各厂商原生嵌入响应折成 openai 的 data[{index, embedding}] 形状，
// 让 validate/usage 逻辑与 openai 路径复用完全相同的校验强度（数量/index 归位/向量合法性/维度匹配）。
function normalizeOllamaEmbeddings(body: any) {
  return {
    data: Array.isArray(body?.embeddings)
      ? body.embeddings.map((vector: unknown, index: number) => ({ index, embedding: vector }))
      : null,
  };
}

function normalizeGeminiEmbeddings(body: any) {
  return {
    data: Array.isArray(body?.embeddings)
      ? body.embeddings.map((row: any, index: number) => ({ index, embedding: row?.values }))
      : null,
  };
}

function normalizeVoyageEmbeddings(body: any) {
  return {
    data: Array.isArray(body?.data)
      ? body.data.map((row: any, index: number) => ({
          index: Number.isSafeInteger(row?.index) ? row.index : index,
          embedding: row?.embedding,
        }))
      : null,
    usage: body?.usage,
  };
}

// DashScope 原生重排响应的 results 嵌在 output 下：折成顶层 results 让 RerankClient
// 的校验/排序路径与 cohere 形状完全复用（其余字段原样保留供 usage/留痕）。
function normalizeDashscopeNativeRerank(body: any) {
  return {
    results: Array.isArray(body?.output?.results) ? body.output.results : null,
    usage: body?.usage,
    request_id: body?.request_id,
  };
}

// MiniMax embeddings 响应为自有形状：顶层 vectors（顺序对应输入，无 index 字段）+
// total_tokens；错误以 HTTP 200 内嵌 base_resp.status_code 表达，非 0 必须显式抛错
// （禁静默降级），否则错误响应会被当作向量数据进入下游校验。
function normalizeMinimaxEmbeddings(body: any) {
  const status = body?.base_resp?.status_code;
  if (typeof status === "number" && status !== 0) {
    throw new ModelOperationRequestError(
      "invalid_provider_response",
      `MiniMax embeddings failed: ${body?.base_resp?.status_msg || status}`,
    );
  }
  const totalTokens = Number(body?.total_tokens);
  return {
    data: Array.isArray(body?.vectors)
      ? body.vectors.map((vector: unknown, index: number) => ({ index, embedding: vector }))
      : null,
    usage: Number.isFinite(totalTokens) ? { total_tokens: totalTokens } : null,
  };
}

// 协议方言：按 execution.api 把通用（openai/cohere 形状）请求体翻译成厂商原生协议的
// URL/请求体/认证头，并给出响应归一化函数；缺省与未识别协议名回退现状路径。
interface OperationDialect {
  url: string;
  headers: Record<string, string>;
  body: any;
  itemCount: number;
  normalizeResponse: (body: any) => any;
}

function operationDialect(execution: any, options: {
  operation: ModelOperation;
  requestBody: any;
  inputType?: EmbeddingInputType;
  contextWindow?: number;
}): OperationDialect {
  const api = typeof execution?.api === "string" ? execution.api : "";
  const modelId = execution?.model?.id;

  if (options.operation === "embedding") {
    const { input, dimensions } = options.requestBody;
    const contextWindow = options.contextWindow;
    switch (api) {
      case PROTOCOL_OLLAMA_EMBED:
        return {
          url: ollamaEmbedUrl(execution.baseUrl),
          headers: protocolHeaders(execution, "none"),
          // 原生 /api/embed 同样接受 dimensions（MRL 截断）与 options.num_ctx
          // （KV cache 预分配）；实测 qwen3-embedding:8b 两参数均生效。
          body: {
            model: modelId,
            input,
            ...(dimensions ? { dimensions } : {}),
            ...(contextWindow ? { options: { num_ctx: contextWindow } } : {}),
          },
          itemCount: input.length,
          normalizeResponse: normalizeOllamaEmbeddings,
        };
      case PROTOCOL_GEMINI_EMBED:
        return {
          url: `${trimBaseUrl(execution.baseUrl)}/models/${modelId}:batchEmbedContents`,
          headers: protocolHeaders(execution, "google"),
          body: {
            requests: input.map((text: string) => ({
              model: `models/${modelId}`,
              content: { parts: [{ text }] },
              ...(dimensions ? { outputDimensionality: dimensions } : {}),
            })),
          },
          itemCount: input.length,
          normalizeResponse: normalizeGeminiEmbeddings,
        };
      case PROTOCOL_VOYAGE_EMBEDDINGS:
        return {
          url: `${trimBaseUrl(execution.baseUrl)}/v1/embeddings`,
          headers: requestHeaders(execution),
          body: {
            model: modelId,
            input,
            input_type: options.inputType ?? "document",
            ...(dimensions ? { dimensions } : {}),
          },
          itemCount: input.length,
          normalizeResponse: normalizeVoyageEmbeddings,
        };
      case PROTOCOL_MINIMAX_EMBEDDINGS: {
        // GroupId 必填（官方 URL query 参数）：缺失显式报错，不静默降级。
        const groupId = execution.model?.groupId;
        if (!groupId || typeof groupId !== "string") {
          throw new ModelOperationRequestError(
            "invalid_input",
            "MiniMax embeddings require a GroupId configured on the model entry (settings > providers > model > GroupId)",
          );
        }
        return {
          url: minimaxEmbeddingUrl(execution.baseUrl, groupId),
          headers: requestHeaders(execution),
          // 官方自有请求体：texts 数组 + type（db=入库向量 / query=检索向量，算法分离）；
          // dimensions 不透传（embo-01 固定 1536 维，多余字段官方未定义）。
          body: {
            model: modelId,
            texts: input,
            type: options.inputType === "query" ? "query" : "db",
          },
          itemCount: input.length,
          normalizeResponse: normalizeMinimaxEmbeddings,
        };
      }
      case PROTOCOL_OPENAI_EMBEDDINGS:
      default:
        // openai-embeddings 协议名、缺省与未识别协议名：维持现状路径
        return {
          url: operationUrl(execution.baseUrl, "embeddings"),
          headers: requestHeaders(execution),
          body: options.requestBody,
          itemCount: input.length,
          normalizeResponse: passthroughResponse,
        };
    }
  }

  const { query, documents, top_n } = options.requestBody;
  switch (api) {
    case PROTOCOL_VOYAGE_RERANK:
      return {
        url: `${trimBaseUrl(execution.baseUrl)}/v1/rerank`,
        headers: requestHeaders(execution),
        body: { model: modelId, query, documents, top_k: top_n },
        itemCount: documents.length,
        normalizeResponse: passthroughResponse,
      };
    case PROTOCOL_DASHSCOPE_RERANK: {
      // 官方双端点按模型分流：兼容端点 compatible-api/v1/reranks（扁平 cohere 形状）
      // 仅支持 qwen3-rerank 系；gte-rerank 系与 qwen3-vl-rerank 系必须走原生嵌套协议端点
      // （input.query/input.documents/parameters，响应 output.results）。未知新模型
      // 归入兼容端点（gte 系已公告下线迁移，qwen 系新模型在兼容端点演进）。
      if (/^(gte-rerank|qwen3-vl-rerank)/.test(modelId)) {
        return {
          url: dashscopeNativeRerankUrl(execution.baseUrl),
          headers: requestHeaders(execution),
          body: {
            model: modelId,
            input: { query, documents },
            parameters: { top_n: top_n, return_documents: false },
          },
          itemCount: documents.length,
          normalizeResponse: normalizeDashscopeNativeRerank,
        };
      }
      return {
        url: dashscopeCompatibleRerankUrl(execution.baseUrl),
        headers: requestHeaders(execution),
        // 兼容端点官方字段集为 model/query/documents/top_n(/instruct)，
        // 不透传 return_documents（旧版原生端点参数，官方兼容文档未定义）。
        body: { model: modelId, query, documents, top_n },
        itemCount: documents.length,
        normalizeResponse: passthroughResponse,
      };
    }
    case PROTOCOL_COHERE_RERANK:
    case PROTOCOL_SILICONFLOW_RERANK:
      return {
        url: cohereRerankUrl(execution.baseUrl),
        headers: requestHeaders(execution),
        body: options.requestBody,
        itemCount: documents.length,
        normalizeResponse: passthroughResponse,
      };
    default:
      // 缺省与未识别协议名：维持现状路径（不做 compatible-mode 改写）
      return {
        url: operationUrl(execution.baseUrl, "rerank"),
        headers: requestHeaders(execution),
        body: options.requestBody,
        itemCount: documents.length,
        normalizeResponse: passthroughResponse,
      };
  }
}

function combinedSignal(signal: AbortSignal | undefined, timeoutMs: number): AbortSignal {
  const timeoutSignal = AbortSignal.timeout(timeoutMs);
  return signal ? AbortSignal.any([signal, timeoutSignal]) : timeoutSignal;
}

function isAbortLike(error: any): boolean {
  return error?.name === "AbortError" || error?.name === "TimeoutError" || error?.type === "aborted";
}

function endObservedFailure(recorder: any, error: unknown): void {
  if (isAbortLike(error)) {
    recorder.logicalCallAborted({ details: { errorKind: (error as any)?.name === "TimeoutError" ? "timeout" : "abort" } });
    recorder.endLogicalCall("aborted");
    return;
  }
  failObservedModelCall(recorder, error, { errorKind: "model_operation" });
}

async function responseJson(response: Response, carrier: any): Promise<any> {
  const raw = await response.text();
  let body;
  try {
    body = raw ? JSON.parse(raw) : {};
  } catch {
    captureProviderHttpResponse(carrier, {
      status: response.status,
      headers: response.headers,
      body: { parseFailed: true, byteLength: Buffer.byteLength(raw) },
      fidelity: "normalized",
    });
    throw new ModelOperationRequestError(
      "invalid_provider_response",
      "Provider returned an invalid JSON response",
      { statusCode: response.status, retryable: response.status >= 500 },
    );
  }
  captureProviderHttpResponse(carrier, {
    status: response.status,
    headers: response.headers,
    body,
    fidelity: "parsed_equivalent",
  });
  if (!response.ok) {
    throw new ModelOperationRequestError(
      "provider_http_error",
      `Provider request failed with HTTP ${response.status}`,
      { statusCode: response.status, retryable: response.status === 429 || response.status >= 500 },
    );
  }
  return body;
}

function normalizeEmbeddingUsage(body: any) {
  return body?.usage && typeof body.usage === "object" ? body.usage : null;
}

function normalizeRerankUsage(body: any) {
  const tokens = body?.meta?.tokens;
  if (tokens && typeof tokens === "object") {
    const input = Number(tokens.input_tokens);
    const output = Number(tokens.output_tokens);
    return {
      ...(Number.isFinite(input) ? { input_tokens: input } : {}),
      ...(Number.isFinite(output) ? { output_tokens: output } : {}),
      ...(Number.isFinite(input) || Number.isFinite(output)
        ? { total_tokens: (Number.isFinite(input) ? input : 0) + (Number.isFinite(output) ? output : 0) }
        : {}),
    };
  }
  // DashScope 原生/兼容端点的 usage 只带 total_tokens
  const total = Number(body?.usage?.total_tokens);
  return Number.isFinite(total) ? { total_tokens: total } : null;
}

function modelIdentity(execution: any) {
  return {
    provider: execution.provider,
    modelId: execution.model.id,
    api: execution.api,
  };
}

function assertTimeout(value: number | undefined, fallback: number): number {
  const timeout = value ?? fallback;
  if (!Number.isSafeInteger(timeout) || timeout <= 0 || timeout > 300_000) {
    throw new ModelOperationRequestError("invalid_input", "timeoutMs must be between 1 and 300000");
  }
  return timeout;
}

abstract class BaseModelOperationClient {
  protected readonly resolveOperationFresh: OperationResolver;
  protected readonly fetchImpl: typeof globalThis.fetch;
  protected readonly getUsageLedger: () => any;
  protected readonly timeoutMs: number;

  constructor(options: ModelOperationClientOptions) {
    if (typeof options?.resolveOperationFresh !== "function") {
      throw new TypeError("resolveOperationFresh is required");
    }
    this.resolveOperationFresh = options.resolveOperationFresh;
    this.fetchImpl = options.fetch || globalThis.fetch;
    this.getUsageLedger = options.getUsageLedger || (() => null);
    this.timeoutMs = assertTimeout(options.timeoutMs, DEFAULT_TIMEOUT_MS);
  }

  protected async execute({
    operation,
    requestBody,
    signal,
    timeoutMs,
    usageContext,
    semanticRequest,
    inputType,
    contextWindow,
    validate,
  }: any) {
    const execution = await this.resolveOperationFresh(operation);
    if (!execution) return null;
    // 协议方言分发：按 execution.api 决定 URL/请求体/认证头/响应归一化；缺省与未识别协议名回退现状路径
    const dialect = operationDialect(execution, {
      operation,
      requestBody: { ...requestBody, model: execution.model.id },
      inputType,
      contextWindow,
    });
    const identity = modelIdentity(execution);
    const recorder = beginObservedModelCall({
      model: identity,
      usageContext,
      details: {
        path: `model_operation_${operation}`,
        operation,
        protocol: execution.api,
      },
    });
    recorder.payloadCapture?.captureSemanticRequest({
      inputShape: "model_operation",
      parameters: semanticRequest,
    });
    const url = dialect.url;
    const headers = dialect.headers;
    const carrier = { modelCall: recorder };
    try {
      const result = await withModelRequestAccounting({
        usageLedger: this.getUsageLedger(),
        model: identity,
        usageContext,
        metadata: { operation, ...observedModelCallLedgerMetadata(recorder) },
      }, async () => {
        const response = await observedProviderFetch(carrier, () => this.fetchImpl(url, {
          method: "POST",
          headers,
          body: JSON.stringify(dialect.body),
          signal: combinedSignal(signal, assertTimeout(timeoutMs, this.timeoutMs)),
        }), {
          requestDetails: {
            protocol: execution.api,
            operation,
            itemCount: dialect.itemCount,
          },
          capture: { method: "POST", url, headers, body: dialect.body, protocol: execution.api },
        });
        const body = dialect.normalizeResponse(await responseJson(response, carrier));
        return {
          ...validate(body, execution),
          providerRequestId: extractProviderRequestId(response.headers),
        };
      });
      recorder.payloadCapture?.captureSemanticResponse({
        response: {
          structuredOutput: result.semanticSummary,
          completeness: "complete",
        },
      });
      recorder.semanticResponseCompleted({
        details: { operation, usagePresent: Boolean(result.usage), ...result.semanticSummary },
      });
      recorder.endLogicalCall("ok");
      const { semanticSummary: _semanticSummary, ...publicResult } = result;
      return publicResult;
    } catch (error) {
      endObservedFailure(recorder, error);
      throw error;
    }
  }
}

export class EmbeddingClient extends BaseModelOperationClient {
  async embed(input: {
    texts: string[];
    dimensions?: number;
    /**
     * 嵌入模型上下文窗口（token 数）：仅 ollama 原生协议透传为 options.num_ctx，
     * 控制推理端的 KV cache 预分配（否则按模型声明最大值预留，8B 模型可达数 GB）。
     */
    contextWindow?: number;
    inputType?: EmbeddingInputType;
    signal?: AbortSignal;
    timeoutMs?: number;
    usageContext?: any;
  }) {
    const texts = assertTextList(input?.texts, "texts", MAX_EMBED_INPUTS);
    const dimensions = input?.dimensions;
    if (dimensions !== undefined && (!Number.isSafeInteger(dimensions) || dimensions <= 0 || dimensions > 65_536)) {
      throw new ModelOperationRequestError("invalid_input", "dimensions must be a positive integer");
    }
    const contextWindow = input?.contextWindow;
    if (contextWindow !== undefined && (!Number.isSafeInteger(contextWindow) || contextWindow <= 0 || contextWindow > 1_048_576)) {
      throw new ModelOperationRequestError("invalid_input", "contextWindow must be a positive integer");
    }
    // ollama 的 num_ctx 必须覆盖实际输入：分块文本可能超过模型条目声明的
    // contextWindow（遗留显式分块值不受窗口 ×80% 约束），"小 num_ctx + 超长
    // 输入"组合会让推理端反复扩容重载甚至挂死。按最长文本字符 ×1.6 估算
    // token，向上取整到 1024 倍数，与声明窗口取大者，夹在 [2048, 32768]。
    const maxTextChars = texts.reduce((max, text) => Math.max(max, text.length), 0);
    const effectiveContextWindow = Math.max(
      2048,
      Math.min(
        32_768,
        Math.max(
          contextWindow ?? 0,
          Math.ceil((maxTextChars * 1.6 + 512) / 1024) * 1024,
        ),
      ),
    );
    return this.execute({
      operation: "embedding",
      requestBody: {
        model: undefined,
        input: texts,
        encoding_format: "float",
        ...(dimensions ? { dimensions } : {}),
      },
      contextWindow: effectiveContextWindow,
      inputType: input?.inputType,
      signal: input?.signal,
      timeoutMs: input?.timeoutMs,
      usageContext: input?.usageContext,
      semanticRequest: { input: texts, ...(dimensions ? { dimensions } : {}) },
      validate: (body: any, execution: any) => {
        const rows = Array.isArray(body?.data) ? body.data : null;
        if (!rows || rows.length !== texts.length) {
          throw new ModelOperationRequestError("invalid_provider_response", "Embedding response count does not match input count");
        }
        const ordered: number[][] = new Array(texts.length);
        let vectorSize = 0;
        for (const row of rows) {
          const index = row?.index;
          const vector = row?.embedding;
          if (!Number.isSafeInteger(index) || index < 0 || index >= texts.length || ordered[index]) {
            throw new ModelOperationRequestError("invalid_provider_response", "Embedding response contains an invalid index");
          }
          if (!Array.isArray(vector) || vector.length === 0 || vector.some((value) => typeof value !== "number" || !Number.isFinite(value))) {
            throw new ModelOperationRequestError("invalid_provider_response", "Embedding response contains an invalid vector");
          }
          if (vectorSize === 0) vectorSize = vector.length;
          if (vector.length !== vectorSize) {
            throw new ModelOperationRequestError("invalid_provider_response", "Embedding response dimensions are inconsistent");
          }
          ordered[index] = vector;
        }
        const expectedDimensions = dimensions || execution.model.dimensions || null;
        if (expectedDimensions && vectorSize !== expectedDimensions) {
          throw new ModelOperationRequestError(
            "invalid_provider_response",
            `Embedding response dimension ${vectorSize} does not match configured dimension ${expectedDimensions}`,
          );
        }
        return {
          vectors: ordered,
          dimensions: vectorSize,
          usage: normalizeEmbeddingUsage(body),
          model: identityForResult(execution, vectorSize),
          semanticSummary: { itemCount: ordered.length, dimensions: vectorSize },
        };
      },
    });
  }
}

export class RerankClient extends BaseModelOperationClient {
  async rerank(input: {
    query: string;
    documents: string[];
    topN?: number;
    signal?: AbortSignal;
    timeoutMs?: number;
    usageContext?: any;
  }) {
    const [query] = assertTextList([input?.query], "query", 1);
    const documents = assertTextList(input?.documents, "documents", MAX_RERANK_DOCUMENTS);
    const topN = input?.topN ?? documents.length;
    if (!Number.isSafeInteger(topN) || topN <= 0 || topN > documents.length) {
      throw new ModelOperationRequestError("invalid_input", "topN must be within the document count");
    }
    return this.execute({
      operation: "rerank",
      requestBody: {
        model: undefined,
        query,
        documents,
        top_n: topN,
        return_documents: false,
      },
      signal: input?.signal,
      timeoutMs: input?.timeoutMs,
      usageContext: input?.usageContext,
      semanticRequest: { query, documents, topN },
      validate: (body: any, execution: any) => {
        if (!Array.isArray(body?.results) || body.results.length !== topN) {
          throw new ModelOperationRequestError("invalid_provider_response", "Rerank response contains an invalid result list");
        }
        const seen = new Set<number>();
        const results = body.results.map((row: any) => {
          const index = row?.index;
          const score = row?.relevance_score ?? row?.relevanceScore ?? row?.score;
          if (!Number.isSafeInteger(index) || index < 0 || index >= documents.length || seen.has(index)) {
            throw new ModelOperationRequestError("invalid_provider_response", "Rerank response contains an invalid document index");
          }
          if (typeof score !== "number" || !Number.isFinite(score)) {
            throw new ModelOperationRequestError("invalid_provider_response", "Rerank response contains an invalid score");
          }
          seen.add(index);
          return { index, score };
        }).sort((left: any, right: any) => right.score - left.score || left.index - right.index);
        return {
          results,
          usage: normalizeRerankUsage(body),
          model: identityForResult(execution),
          semanticSummary: { candidateCount: documents.length, resultCount: results.length },
        };
      },
    });
  }
}

function identityForResult(execution: any, dimensions?: number) {
  return {
    provider: execution.provider,
    id: execution.model.id,
    api: execution.api,
    ...(dimensions ? { dimensions } : {}),
  };
}

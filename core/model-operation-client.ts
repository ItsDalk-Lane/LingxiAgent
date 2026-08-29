import {
  beginObservedModelCall,
  captureProviderHttpResponse,
  failObservedModelCall,
  observedModelCallLedgerMetadata,
  observedProviderFetch,
} from "../lib/llm/model-call-integration.ts";
import { extractProviderRequestId } from "../lib/llm/model-call-observer.ts";
import { withModelRequestAccounting } from "../lib/llm/model-request-accounting.ts";
import { MODEL_OPERATION_RERANK_MAX_DOCS, type ModelOperation } from "../shared/model-operations.ts";

const DEFAULT_TIMEOUT_MS = 30_000;
const MAX_EMBED_INPUTS = 128;
const MAX_RERANK_DOCUMENTS = MODEL_OPERATION_RERANK_MAX_DOCS;
const MAX_TEXT_CHARS = 32_000;
const MAX_TOTAL_TEXT_CHARS = 500_000;

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

function operationUrl(baseUrl: string, suffix: "embeddings" | "rerank"): string {
  const trimmed = String(baseUrl || "").replace(/\/+$/, "");
  if (!trimmed) throw new ModelOperationRequestError("base_url_missing", "Provider base URL is unavailable");
  return trimmed.endsWith(`/${suffix}`) ? trimmed : `${trimmed}/${suffix}`;
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
  if (!tokens || typeof tokens !== "object") return null;
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
    endpoint,
    signal,
    timeoutMs,
    usageContext,
    semanticRequest,
    validate,
  }: any) {
    const execution = await this.resolveOperationFresh(operation);
    if (!execution) return null;
    const providerRequestBody = { ...requestBody, model: execution.model.id };
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
    const url = operationUrl(execution.baseUrl, endpoint);
    const headers = requestHeaders(execution);
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
          body: JSON.stringify(providerRequestBody),
          signal: combinedSignal(signal, assertTimeout(timeoutMs, this.timeoutMs)),
        }), {
          requestDetails: {
            protocol: execution.api,
            operation,
            itemCount: operation === "embedding" ? providerRequestBody.input.length : providerRequestBody.documents.length,
          },
          capture: { method: "POST", url, headers, body: providerRequestBody, protocol: execution.api },
        });
        const body = await responseJson(response, carrier);
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
    signal?: AbortSignal;
    timeoutMs?: number;
    usageContext?: any;
  }) {
    const texts = assertTextList(input?.texts, "texts", MAX_EMBED_INPUTS);
    const dimensions = input?.dimensions;
    if (dimensions !== undefined && (!Number.isSafeInteger(dimensions) || dimensions <= 0 || dimensions > 65_536)) {
      throw new ModelOperationRequestError("invalid_input", "dimensions must be a positive integer");
    }
    return this.execute({
      operation: "embedding",
      requestBody: {
        model: undefined,
        input: texts,
        encoding_format: "float",
        ...(dimensions ? { dimensions } : {}),
      },
      endpoint: "embeddings",
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
      endpoint: "rerank",
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

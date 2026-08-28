import { describe, expect, it, vi } from "vitest";

import {
  EmbeddingClient,
  ModelOperationRequestError,
  RerankClient,
} from "../core/model-operation-client.ts";

function execution(operation: "embedding" | "rerank") {
  return {
    operation,
    provider: "provider-a",
    api: operation === "embedding" ? "openai-embeddings" : "siliconflow-rerank",
    apiKey: "secret-key",
    baseUrl: "https://provider-a.example/v1",
    headers: { "X-Provider": "provider-a" },
    model: {
      id: operation === "embedding" ? "embed-model" : "rerank-model",
      provider: "provider-a",
      dimensions: operation === "embedding" ? 3 : undefined,
    },
  };
}

function response(body: any, init: ResponseInit = {}) {
  return new Response(JSON.stringify(body), {
    status: init.status || 200,
    headers: { "Content-Type": "application/json", "x-request-id": "provider-request-1" },
  });
}

function ledger() {
  return {
    start: vi.fn(() => ({ requestId: "usage-1" })),
    finish: vi.fn(),
    recordError: vi.fn(),
  };
}

describe("EmbeddingClient", () => {
  it("uses the shared provider credential lane and accounts for usage", async () => {
    const fetchMock = vi.fn(async (_url: string, init: RequestInit) => response({
      data: [
        { index: 1, embedding: [0, 1, 0] },
        { index: 0, embedding: [1, 0, 0] },
      ],
      usage: { prompt_tokens: 4, total_tokens: 4 },
    }));
    const usageLedger = ledger();
    const resolve = vi.fn(async () => execution("embedding"));
    const client = new EmbeddingClient({
      resolveOperationFresh: resolve,
      fetch: fetchMock as any,
      getUsageLedger: () => usageLedger,
    });

    await expect(client.embed({
      texts: ["first", "second"],
      usageContext: {
        source: { subsystem: "knowledge", operation: "embedding", surface: "knowledge", trigger: "user" },
        attribution: { kind: "knowledge", taskId: "run-1" },
      },
    })).resolves.toMatchObject({
      vectors: [[1, 0, 0], [0, 1, 0]],
      dimensions: 3,
      providerRequestId: "provider-request-1",
      model: { provider: "provider-a", id: "embed-model", api: "openai-embeddings" },
    });

    expect(resolve).toHaveBeenCalledWith("embedding");
    expect(fetchMock).toHaveBeenCalledOnce();
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe("https://provider-a.example/v1/embeddings");
    expect(init.headers).toMatchObject({
      Authorization: "Bearer secret-key",
      "X-Provider": "provider-a",
    });
    expect(JSON.parse(String(init.body))).toEqual({
      model: "embed-model",
      input: ["first", "second"],
      encoding_format: "float",
    });
    expect(usageLedger.finish).toHaveBeenCalledWith("usage-1", expect.objectContaining({
      usage: { prompt_tokens: 4, total_tokens: 4 },
    }));
  });

  it("keeps an unconfigured embedding operation optional", async () => {
    const fetchMock = vi.fn();
    const client = new EmbeddingClient({
      resolveOperationFresh: async () => null,
      fetch: fetchMock as any,
    });
    await expect(client.embed({ texts: ["text"] })).resolves.toBeNull();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("rejects non-finite or dimension-mismatched vectors", async () => {
    const client = new EmbeddingClient({
      resolveOperationFresh: async () => execution("embedding"),
      fetch: vi.fn(async () => response({ data: [{ index: 0, embedding: [1, 2] }] })) as any,
    });
    await expect(client.embed({ texts: ["text"] })).rejects.toMatchObject({
      name: "ModelOperationRequestError",
      code: "invalid_provider_response",
    } satisfies Partial<ModelOperationRequestError>);
  });

  it("does not copy provider error bodies into public errors", async () => {
    const client = new EmbeddingClient({
      resolveOperationFresh: async () => execution("embedding"),
      fetch: vi.fn(async () => response({ error: { message: "raw sensitive provider detail" } }, { status: 429 })) as any,
    });
    const error = await client.embed({ texts: ["text"] }).catch((value) => value);
    expect(error).toMatchObject({ code: "provider_http_error", statusCode: 429, retryable: true });
    expect(error.message).not.toContain("raw sensitive provider detail");
  });

  it("honors caller cancellation", async () => {
    const controller = new AbortController();
    controller.abort();
    const client = new EmbeddingClient({
      resolveOperationFresh: async () => execution("embedding"),
      fetch: vi.fn(async (_url, init) => {
        if (init?.signal?.aborted) throw new DOMException("aborted", "AbortError");
        return response({});
      }) as any,
    });
    await expect(client.embed({ texts: ["text"], signal: controller.signal })).rejects.toMatchObject({
      name: "AbortError",
    });
  });
});

describe("RerankClient", () => {
  it("normalizes ranked indices and SiliconFlow usage", async () => {
    const fetchMock = vi.fn(async () => response({
      results: [
        { index: 0, relevance_score: 0.2 },
        { index: 2, relevance_score: 0.9 },
      ],
      meta: { tokens: { input_tokens: 10, output_tokens: 1 } },
    }));
    const client = new RerankClient({
      resolveOperationFresh: async () => execution("rerank"),
      fetch: fetchMock as any,
    });

    await expect(client.rerank({
      query: "question",
      documents: ["one", "two", "three"],
      topN: 2,
    })).resolves.toMatchObject({
      results: [{ index: 2, score: 0.9 }, { index: 0, score: 0.2 }],
      usage: { input_tokens: 10, output_tokens: 1, total_tokens: 11 },
      model: { provider: "provider-a", id: "rerank-model", api: "siliconflow-rerank" },
    });
    const [, requestInit] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(JSON.parse(String(requestInit.body))).toEqual({
      model: "rerank-model",
      query: "question",
      documents: ["one", "two", "three"],
      top_n: 2,
      return_documents: false,
    });
  });

  it("rejects duplicate or out-of-range provider indices", async () => {
    const client = new RerankClient({
      resolveOperationFresh: async () => execution("rerank"),
      fetch: vi.fn(async () => response({
        results: [
          { index: 0, relevance_score: 0.9 },
          { index: 0, relevance_score: 0.8 },
        ],
      })) as any,
    });
    await expect(client.rerank({ query: "q", documents: ["one", "two"] })).rejects.toMatchObject({
      code: "invalid_provider_response",
    });
  });
});

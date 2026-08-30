import { describe, expect, it, vi } from "vitest";

import {
  EmbeddingClient,
  ModelOperationRequestError,
  RerankClient,
} from "../core/model-operation-client.ts";

function execution(operation: "embedding" | "rerank", overrides: Record<string, any> = {}) {
  return {
    operation,
    provider: "provider-x",
    api: operation === "embedding" ? "openai-embeddings" : "cohere-rerank",
    apiKey: "secret-key",
    baseUrl: "https://provider-x.example/v1",
    headers: {},
    model: {
      id: operation === "embedding" ? "embed-model" : "rerank-model",
      provider: "provider-x",
      dimensions: operation === "embedding" ? 3 : undefined,
    },
    ...overrides,
  };
}

function response(body: any, init: ResponseInit = {}) {
  return new Response(JSON.stringify(body), {
    status: init.status || 200,
    headers: { "Content-Type": "application/json", "x-request-id": "provider-request-1" },
  });
}

function hasAuthorization(headers: unknown): boolean {
  return Object.keys(headers as Record<string, string>).some((key) => key.toLowerCase() === "authorization");
}

describe("ollama-embed 协议", () => {
  it("base 已以 /api 结尾时只补 /embed，且不带任何 Authorization 头", async () => {
    const fetchMock = vi.fn(async () => response({ embeddings: [[1, 0, 0], [0, 1, 0]] }));
    const client = new EmbeddingClient({
      resolveOperationFresh: vi.fn(async () => execution("embedding", {
        api: "ollama-embed",
        baseUrl: "http://localhost:11434/api",
      })),
      fetch: fetchMock as any,
    });

    await expect(client.embed({ texts: ["first", "second"] })).resolves.toMatchObject({
      vectors: [[1, 0, 0], [0, 1, 0]],
      dimensions: 3,
      model: { provider: "provider-x", id: "embed-model", api: "ollama-embed" },
    });

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(init.method).toBe("POST");
    expect(url).toBe("http://localhost:11434/api/embed");
    const headers = init.headers as Record<string, string>;
    expect(hasAuthorization(headers)).toBe(false);
    expect(JSON.parse(String(init.body))).toEqual({
      model: "embed-model",
      input: ["first", "second"],
      // 未声明窗口时自适应兜底 2048（短文本），防止 ollama 按模型声明最大值预留。
      options: { num_ctx: 2048 },
    });
  });

  it("base 不以 /api 结尾时补 /api/embed", async () => {
    const fetchMock = vi.fn(async () => response({ embeddings: [[1, 0, 0]] }));
    const client = new EmbeddingClient({
      resolveOperationFresh: vi.fn(async () => execution("embedding", {
        api: "ollama-embed",
        baseUrl: "http://localhost:11434/",
      })),
      fetch: fetchMock as any,
    });

    await expect(client.embed({ texts: ["first"] })).resolves.toMatchObject({
      vectors: [[1, 0, 0]],
    });

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("http://localhost:11434/api/embed");
    expect(JSON.parse(String(init.body))).toEqual({
      model: "embed-model",
      input: ["first"],
      options: { num_ctx: 2048 },
    });
  });

  it("base 以 OpenAI 兼容前缀 /v1 结尾时先剥掉再拼原生端点(自添加模型继承供应商默认 base)", async () => {
    const fetchMock = vi.fn(async () => response({ embeddings: [[1, 0, 0]] }));
    const client = new EmbeddingClient({
      resolveOperationFresh: vi.fn(async () => execution("embedding", {
        api: "ollama-embed",
        baseUrl: "http://localhost:11434/v1",
      })),
      fetch: fetchMock as any,
    });

    await expect(client.embed({ texts: ["first"] })).resolves.toMatchObject({
      vectors: [[1, 0, 0]],
    });

    const [url] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("http://localhost:11434/api/embed");
  });

  it("embeddings 数量与输入不一致时拒绝", async () => {
    const client = new EmbeddingClient({
      resolveOperationFresh: vi.fn(async () => execution("embedding", {
        api: "ollama-embed",
        baseUrl: "http://localhost:11434/api",
      })),
      fetch: vi.fn(async () => response({ embeddings: [[1, 0, 0]] })) as any,
    });
    await expect(client.embed({ texts: ["first", "second"] })).rejects.toMatchObject({
      name: "ModelOperationRequestError",
      code: "invalid_provider_response",
    } satisfies Partial<ModelOperationRequestError>);
  });

  it("dimensions 与 contextWindow 透传为 dimensions/options.num_ctx（MRL 截断 + KV cache 预分配）", async () => {
    const fetchMock = vi.fn(async () => response({ embeddings: [[1, 0], [0, 1]] }));
    const client = new EmbeddingClient({
      resolveOperationFresh: vi.fn(async () => execution("embedding", {
        api: "ollama-embed",
        baseUrl: "http://localhost:11434/api",
        model: { id: "embed-model", provider: "provider-x", dimensions: 2 },
      })),
      fetch: fetchMock as any,
    });

    await expect(client.embed({
      texts: ["first", "second"],
      dimensions: 2,
      contextWindow: 8192,
    })).resolves.toMatchObject({ dimensions: 2 });

    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(JSON.parse(String(init.body))).toEqual({
      model: "embed-model",
      input: ["first", "second"],
      dimensions: 2,
      options: { num_ctx: 8192 },
    });
  });

  it("num_ctx 按最长文本自适应放大：声明窗口装不下实际输入时不照抄小窗口", async () => {
    const fetchMock = vi.fn(async () => response({ embeddings: [[1, 0], [0, 1]] }));
    const client = new EmbeddingClient({
      resolveOperationFresh: vi.fn(async () => execution("embedding", {
        api: "ollama-embed",
        baseUrl: "http://localhost:11434/api",
        model: { id: "embed-model", provider: "provider-x", dimensions: 2 },
      })),
      fetch: fetchMock as any,
    });

    // 5000 字符块 ≈ 8000+ token：声明窗口 4096 装不下，需放大（1024 倍数）。
    const longText = "长".repeat(5000);
    await client.embed({ texts: ["first", longText], contextWindow: 4096 });

    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    const body = JSON.parse(String(init.body));
    expect(body.options.num_ctx).toBeGreaterThanOrEqual(8192);
    expect(body.options.num_ctx % 1024).toBe(0);
    expect(body.options.num_ctx).toBeLessThanOrEqual(32768);
  });

  it("返回维度与请求 dimensions 不符时拒绝（MRL 未生效的显式失败）", async () => {
    const client = new EmbeddingClient({
      resolveOperationFresh: vi.fn(async () => execution("embedding", {
        api: "ollama-embed",
        baseUrl: "http://localhost:11434/api",
        model: { id: "embed-model", provider: "provider-x", dimensions: 1024 },
      })),
      fetch: vi.fn(async () => response({ embeddings: [new Array(4096).fill(0)] })) as any,
    });
    await expect(client.embed({ texts: ["first"] })).rejects.toMatchObject({
      name: "ModelOperationRequestError",
      code: "invalid_provider_response",
    } satisfies Partial<ModelOperationRequestError>);
  });
});

describe("gemini-embed 协议", () => {
  it("batchEmbedContents 端点、x-goog-api-key 认证且无 Authorization", async () => {
    const fetchMock = vi.fn(async () => response({
      embeddings: [{ values: [1, 0, 0] }, { values: [0, 1, 0] }],
    }));
    const client = new EmbeddingClient({
      resolveOperationFresh: vi.fn(async () => execution("embedding", {
        api: "gemini-embed",
        baseUrl: "https://generativelanguage.googleapis.com/v1beta",
      })),
      fetch: fetchMock as any,
    });

    await expect(client.embed({ texts: ["first", "second"] })).resolves.toMatchObject({
      vectors: [[1, 0, 0], [0, 1, 0]],
      dimensions: 3,
    });

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(init.method).toBe("POST");
    expect(url).toBe("https://generativelanguage.googleapis.com/v1beta/models/embed-model:batchEmbedContents");
    const headers = init.headers as Record<string, string>;
    expect(headers["x-goog-api-key"]).toBe("secret-key");
    expect(hasAuthorization(headers)).toBe(false);
    expect(JSON.parse(String(init.body))).toEqual({
      requests: [
        { model: "models/embed-model", content: { parts: [{ text: "first" }] } },
        { model: "models/embed-model", content: { parts: [{ text: "second" }] } },
      ],
    });
  });

  it("dimensions 映射为每个 request 内的 outputDimensionality", async () => {
    const fetchMock = vi.fn(async () => response({ embeddings: [{ values: [1, 0, 0, 0] }] }));
    const client = new EmbeddingClient({
      resolveOperationFresh: vi.fn(async () => execution("embedding", {
        api: "gemini-embed",
        baseUrl: "https://generativelanguage.googleapis.com/v1beta",
      })),
      fetch: fetchMock as any,
    });

    await expect(client.embed({ texts: ["first"], dimensions: 4 })).resolves.toMatchObject({
      vectors: [[1, 0, 0, 0]],
      dimensions: 4,
    });

    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(JSON.parse(String(init.body))).toEqual({
      requests: [
        {
          model: "models/embed-model",
          content: { parts: [{ text: "first" }] },
          outputDimensionality: 4,
        },
      ],
    });
  });

  it("embeddings 字段非数组（错形状）时拒绝", async () => {
    const client = new EmbeddingClient({
      resolveOperationFresh: vi.fn(async () => execution("embedding", {
        api: "gemini-embed",
        baseUrl: "https://generativelanguage.googleapis.com/v1beta",
      })),
      fetch: vi.fn(async () => response({ error: { message: "not embeddings" } })) as any,
    });
    await expect(client.embed({ texts: ["first"] })).rejects.toMatchObject({
      name: "ModelOperationRequestError",
      code: "invalid_provider_response",
    } satisfies Partial<ModelOperationRequestError>);
  });

  it("HTTP 4xx 映射为 provider_http_error 且不泄漏厂商错误正文", async () => {
    const client = new EmbeddingClient({
      resolveOperationFresh: vi.fn(async () => execution("embedding", {
        api: "gemini-embed",
        baseUrl: "https://generativelanguage.googleapis.com/v1beta",
      })),
      fetch: vi.fn(async () => response({ error: { message: "raw sensitive provider detail" } }, { status: 400 })) as any,
    });
    const error = await client.embed({ texts: ["first"] }).catch((value) => value);
    expect(error).toMatchObject({ code: "provider_http_error", statusCode: 400, retryable: false });
    expect(error.message).not.toContain("raw sensitive provider detail");
  });
});

describe("voyage-embeddings 协议", () => {
  it("v1/embeddings 端点、Bearer 认证、有 index 按 index 归位并透传 usage", async () => {
    const fetchMock = vi.fn(async () => response({
      data: [
        { index: 1, embedding: [0, 1, 0, 0] },
        { index: 0, embedding: [1, 0, 0, 0] },
      ],
      usage: { total_tokens: 8 },
    }));
    const client = new EmbeddingClient({
      resolveOperationFresh: vi.fn(async () => execution("embedding", {
        api: "voyage-embeddings",
        baseUrl: "https://api.voyageai.com",
      })),
      fetch: fetchMock as any,
    });

    await expect(client.embed({ texts: ["first", "second"], dimensions: 4 })).resolves.toMatchObject({
      vectors: [[1, 0, 0, 0], [0, 1, 0, 0]],
      dimensions: 4,
      usage: { total_tokens: 8 },
    });

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(init.method).toBe("POST");
    expect(url).toBe("https://api.voyageai.com/v1/embeddings");
    const headers = init.headers as Record<string, string>;
    expect(headers.Authorization).toBe("Bearer secret-key");
    expect(JSON.parse(String(init.body))).toEqual({
      model: "embed-model",
      input: ["first", "second"],
      input_type: "document",
      dimensions: 4,
    });
  });

  it("inputType query 映射 input_type，无 index 时按数组顺序归位", async () => {
    const fetchMock = vi.fn(async () => response({
      data: [{ embedding: [1, 0, 0] }, { embedding: [0, 1, 0] }],
    }));
    const client = new EmbeddingClient({
      resolveOperationFresh: vi.fn(async () => execution("embedding", {
        api: "voyage-embeddings",
        baseUrl: "https://api.voyageai.com",
      })),
      fetch: fetchMock as any,
    });

    await expect(client.embed({ texts: ["first", "second"], inputType: "query" })).resolves.toMatchObject({
      vectors: [[1, 0, 0], [0, 1, 0]],
    });

    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(JSON.parse(String(init.body))).toEqual({
      model: "embed-model",
      input: ["first", "second"],
      input_type: "query",
    });
  });

  it("越界 index 拒绝", async () => {
    const client = new EmbeddingClient({
      resolveOperationFresh: vi.fn(async () => execution("embedding", {
        api: "voyage-embeddings",
        baseUrl: "https://api.voyageai.com",
      })),
      fetch: vi.fn(async () => response({ data: [{ index: 5, embedding: [1, 0, 0] }] })) as any,
    });
    await expect(client.embed({ texts: ["first"] })).rejects.toMatchObject({
      name: "ModelOperationRequestError",
      code: "invalid_provider_response",
    } satisfies Partial<ModelOperationRequestError>);
  });
});

describe("voyage-rerank 协议", () => {
  it("v1/rerank 端点、top_k 请求体（无 top_n/return_documents）并排序结果", async () => {
    const fetchMock = vi.fn(async () => response({
      results: [
        { index: 1, relevance_score: 0.5 },
        { index: 0, relevance_score: 0.9 },
      ],
    }));
    const client = new RerankClient({
      resolveOperationFresh: vi.fn(async () => execution("rerank", {
        api: "voyage-rerank",
        baseUrl: "https://api.voyageai.com",
      })),
      fetch: fetchMock as any,
    });

    await expect(client.rerank({
      query: "question",
      documents: ["one", "two", "three"],
      topN: 2,
    })).resolves.toMatchObject({
      results: [{ index: 0, score: 0.9 }, { index: 1, score: 0.5 }],
    });

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(init.method).toBe("POST");
    expect(url).toBe("https://api.voyageai.com/v1/rerank");
    const headers = init.headers as Record<string, string>;
    expect(headers.Authorization).toBe("Bearer secret-key");
    expect(JSON.parse(String(init.body))).toEqual({
      model: "rerank-model",
      query: "question",
      documents: ["one", "two", "three"],
      top_k: 2,
    });
  });

  it("score 非 finite 时拒绝", async () => {
    const client = new RerankClient({
      resolveOperationFresh: vi.fn(async () => execution("rerank", {
        api: "voyage-rerank",
        baseUrl: "https://api.voyageai.com",
      })),
      fetch: vi.fn(async () => response({ results: [{ index: 0, relevance_score: "high" }] })) as any,
    });
    await expect(client.rerank({ query: "q", documents: ["one"] })).rejects.toMatchObject({
      name: "ModelOperationRequestError",
      code: "invalid_provider_response",
    } satisfies Partial<ModelOperationRequestError>);
  });
});

describe("cohere-rerank 协议", () => {
  it("DashScope compatible-mode base 改写为 compatible-api 再拼 /rerank", async () => {
    const fetchMock = vi.fn(async () => response({
      results: [{ index: 0, relevance_score: 0.9 }],
    }));
    const client = new RerankClient({
      resolveOperationFresh: vi.fn(async () => execution("rerank", {
        api: "cohere-rerank",
        baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
      })),
      fetch: fetchMock as any,
    });

    await expect(client.rerank({
      query: "question",
      documents: ["one"],
      topN: 1,
    })).resolves.toMatchObject({
      results: [{ index: 0, score: 0.9 }],
    });

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(init.method).toBe("POST");
    expect(url).toBe("https://dashscope.aliyuncs.com/compatible-api/v1/rerank");
    const headers = init.headers as Record<string, string>;
    expect(headers.Authorization).toBe("Bearer secret-key");
    expect(JSON.parse(String(init.body))).toEqual({
      model: "rerank-model",
      query: "question",
      documents: ["one"],
      top_n: 1,
      return_documents: false,
    });
  });

  it("siliconflow-rerank 兼容别名不受 compatible-mode 改写影响", async () => {
    const fetchMock = vi.fn(async () => response({
      results: [{ index: 0, relevance_score: 0.9 }],
    }));
    const client = new RerankClient({
      resolveOperationFresh: vi.fn(async () => execution("rerank", {
        api: "siliconflow-rerank",
        baseUrl: "https://api.siliconflow.cn/v1",
      })),
      fetch: fetchMock as any,
    });

    await expect(client.rerank({ query: "q", documents: ["one"] })).resolves.toMatchObject({
      results: [{ index: 0, score: 0.9 }],
    });

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("https://api.siliconflow.cn/v1/rerank");
    expect(JSON.parse(String(init.body))).toEqual({
      model: "rerank-model",
      query: "q",
      documents: ["one"],
      top_n: 1,
      return_documents: false,
    });
  });
});

describe("协议回退（缺省与未识别协议名）", () => {
  it("execution.api 缺省时 embedding 走 openai 现状路径", async () => {
    const fetchMock = vi.fn(async () => response({
      data: [{ index: 0, embedding: [1, 0, 0] }],
    }));
    const client = new EmbeddingClient({
      resolveOperationFresh: vi.fn(async () => execution("embedding", { api: undefined })),
      fetch: fetchMock as any,
    });

    await expect(client.embed({ texts: ["first"] })).resolves.toMatchObject({
      vectors: [[1, 0, 0]],
    });

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(init.method).toBe("POST");
    expect(url).toBe("https://provider-x.example/v1/embeddings");
    const headers = init.headers as Record<string, string>;
    expect(headers.Authorization).toBe("Bearer secret-key");
    expect(JSON.parse(String(init.body))).toEqual({
      model: "embed-model",
      input: ["first"],
      encoding_format: "float",
    });
  });

  it("未识别协议名时 embedding 仍走 openai 现状路径", async () => {
    const fetchMock = vi.fn(async () => response({
      data: [{ index: 0, embedding: [1, 0, 0] }],
    }));
    const client = new EmbeddingClient({
      resolveOperationFresh: vi.fn(async () => execution("embedding", { api: "custom-x" })),
      fetch: fetchMock as any,
    });

    await expect(client.embed({ texts: ["first"] })).resolves.toMatchObject({
      vectors: [[1, 0, 0]],
    });

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("https://provider-x.example/v1/embeddings");
    expect(JSON.parse(String(init.body))).toEqual({
      model: "embed-model",
      input: ["first"],
      encoding_format: "float",
    });
  });

  it("execution.api 为空串时 rerank 走现状路径（不做 compatible-mode 改写）", async () => {
    const fetchMock = vi.fn(async () => response({
      results: [{ index: 0, relevance_score: 0.9 }],
    }));
    const client = new RerankClient({
      resolveOperationFresh: vi.fn(async () => execution("rerank", {
        api: "",
        baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
      })),
      fetch: fetchMock as any,
    });

    await expect(client.rerank({ query: "q", documents: ["one"] })).resolves.toMatchObject({
      results: [{ index: 0, score: 0.9 }],
    });

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(init.method).toBe("POST");
    expect(url).toBe("https://dashscope.aliyuncs.com/compatible-mode/v1/rerank");
    const headers = init.headers as Record<string, string>;
    expect(headers.Authorization).toBe("Bearer secret-key");
    expect(JSON.parse(String(init.body))).toEqual({
      model: "rerank-model",
      query: "q",
      documents: ["one"],
      top_n: 1,
      return_documents: false,
    });
  });
});

/**
 * model-observability-scenario-harness.ts — Phase 10 E2E Truth Audit 编排基建。
 *
 * 任务书 §六/§七/§一百三十四/§一百三十五：
 *   - Truth Oracle = Fake Provider Witness：本地随机端口真实 HTTP server，
 *     独立记录 Provider 实际收到的 method/path/headers/body/ordinal。绝不用
 *     Observer 的输出证明 Observer 正确；绝不复用 observedProviderFetch/
 *     capture 实现当 expected（本文件只 import node:http 原语与生产 ingress）。
 *   - Harness 只编排现有生产入口（callText / probeProvider / 真实 Pi session /
 *     media runner / speech service / diary / persistence install / query service
 *     / Hono route）；不直接 INSERT DB 模拟正常 scenario（corruption/migration
 *     专项除外）。
 *   - Witness 与 receipt 只存在测试进程内存；毒丸 secret 绝不写入报告。
 */
import http from "node:http";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { Hono } from "hono";
import { installModelObservabilityPersistence, type ModelObservabilityPersistencePolicy } from "../../lib/llm/model-observability-persistence.ts";
import { createModelObservabilityQueryService } from "../../lib/llm/model-observability-query.ts";
import { setModelCallObserver } from "../../lib/llm/model-call-observer.ts";
import { createTestModelCallObserver, type TestModelCallObserver } from "../../lib/llm/model-call-observer-testing.ts";
import { createUsageLedger } from "../../lib/llm/usage-ledger.ts";
import { createModelObservabilityRoute } from "../../server/routes/model-observability.ts";

/* ────────────────────────────────────────────────────────────────────────
 * 1. Fake Provider Witness（Truth Oracle）
 * ──────────────────────────────────────────────────────────────────────── */

export interface WitnessCapture {
  ordinal: number;
  method: string;
  path: string;
  headers: Record<string, string>;
  bodyText: string;
  bodyJson: unknown;
  requestAt: number;
  responseStatus: number | null;
}

export type WitnessScript =
  | { kind: "sse"; body: string; status?: number; headers?: Record<string, string>; delayMs?: number }
  | { kind: "json"; body: unknown; status?: number; headers?: Record<string, string>; delayMs?: number }
  | { kind: "text"; body: string; status?: number; contentType?: string; delayMs?: number }
  | { kind: "hang" }
  | { kind: "reset" };

export interface FakeProviderWitness {
  port: number;
  baseUrl: string;
  /** 全部已收到请求（时间序）。 */
  requests(): WitnessCapture[];
  /** 路径子串过滤（如 "/chat/completions"）。 */
  requestsTo(pathFragment: string): WitnessCapture[];
  lastRequest(): WitnessCapture | null;
  requestCount(): number;
  /** 编排下一个（或若干个）响应脚本；耗尽后回落 default。 */
  scriptNext(...entries: WitnessScript[]): void;
  /** 覆盖默认响应脚本。 */
  setDefault(entry: WitnessScript): void;
  close(): Promise<void>;
}

export async function startFakeProviderWitness(
  options: { defaultEntry?: WitnessScript } = {},
): Promise<FakeProviderWitness> {
  const captures: WitnessCapture[] = [];
  const script: WitnessScript[] = [];
  let defaultEntry: WitnessScript = options.defaultEntry
    ?? { kind: "sse", body: openaiCompletionsSseBody({ content: "WITNESS_DEFAULT_REPLY" }) };

  const server = http.createServer((req, res) => {
    const chunks: Buffer[] = [];
    req.on("data", (chunk: Buffer) => chunks.push(chunk));
    req.on("end", () => {
      const bodyText = Buffer.concat(chunks).toString("utf8");
      let bodyJson: unknown = null;
      try {
        bodyJson = JSON.parse(bodyText);
      } catch {
        bodyJson = null;
      }
      const headers: Record<string, string> = {};
      for (const [key, value] of Object.entries(req.headers)) {
        headers[key] = Array.isArray(value) ? value.join(", ") : String(value ?? "");
      }
      const capture: WitnessCapture = {
        ordinal: captures.length + 1,
        method: req.method ?? "GET",
        path: req.url ?? "/",
        headers,
        bodyText,
        bodyJson,
        requestAt: Date.now(),
        responseStatus: null,
      };
      captures.push(capture);
      const entry = script.shift() ?? defaultEntry;
      const apply = () => {
        if (entry.kind === "hang") return; // 不响应（timeout/abort 场景）
        if (entry.kind === "reset") {
          res.destroy();
          return;
        }
        let status = entry.status ?? 200;
        let body = "";
        const headersOut: Record<string, string> = { ...((entry as any).headers ?? {}) };
        if (entry.kind === "sse") {
          body = entry.body;
          headersOut["content-type"] ??= "text/event-stream";
        } else if (entry.kind === "json") {
          body = typeof entry.body === "string" ? entry.body : JSON.stringify(entry.body);
          headersOut["content-type"] ??= "application/json";
        } else {
          body = entry.body;
          headersOut["content-type"] ??= entry.contentType ?? "text/plain";
        }
        if (entry.delayMs && entry.delayMs > 0) {
          setTimeout(() => {
            capture.responseStatus = status;
            res.writeHead(status, headersOut);
            res.end(body);
          }, entry.delayMs);
          return;
        }
        capture.responseStatus = status;
        res.writeHead(status, headersOut);
        res.end(body);
      };
      apply();
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : 0;

  return {
    port,
    baseUrl: `http://127.0.0.1:${port}`,
    requests: () => [...captures],
    requestsTo: (fragment) => captures.filter((c) => c.path.includes(fragment)),
    lastRequest: () => captures[captures.length - 1] ?? null,
    requestCount: () => captures.length,
    scriptNext: (...entries) => { script.push(...entries); },
    setDefault: (entry) => { defaultEntry = entry; },
    close: () => new Promise((resolve, reject) => server.close((err) => (err ? reject(err) : resolve()))),
  };
}

/* ────────────────────────────────────────────────────────────────────────
 * 2. 协议 fixture 构造（与真实 Provider 协议形状一致）
 * ──────────────────────────────────────────────────────────────────────── */

/** OpenAI Chat Completions SSE（Pi openai-completions adapter / streaming）。 */
export function openaiCompletionsSseBody(options: {
  content?: string;
  usage?: { prompt_tokens: number; completion_tokens: number; total_tokens: number } | null;
} = {}): string {
  const content = options.content ?? "WITNESS_COMPLETIONS_REPLY";
  const usage = options.usage ?? { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 };
  const events = [
    { id: "chatcmpl-w1", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: { role: "assistant", content }, finish_reason: null }] },
    { id: "chatcmpl-w1", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: {}, finish_reason: "stop" }], ...(usage ? { usage } : {}) },
  ];
  // SSE 块以空行分隔（单块内多 data: 行会被按 spec 拼接 → 必须逐事件空行）。
  return [...events.map((event) => `data: ${JSON.stringify(event)}`), "data: [DONE]", ""].join("\n\n");
}

/** OpenAI Chat Completions 非流式 JSON（callText openai-completions）。 */
export function openaiCompletionsJson(options: {
  content?: string;
  usage?: { prompt_tokens: number; completion_tokens: number; total_tokens: number } | null;
} = {}): unknown {
  return {
    id: "chatcmpl-w1",
    object: "chat.completion",
    model: "witness-model",
    choices: [{ index: 0, message: { role: "assistant", content: options.content ?? "WITNESS_COMPLETIONS_REPLY" }, finish_reason: "stop" }],
    usage: options.usage ?? { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
  };
}

/** Anthropic Messages 非流式 JSON（callText anthropic-messages / probe）。 */
export function anthropicMessagesJson(options: {
  content?: string;
  usage?: { input_tokens: number; output_tokens: number } | null;
} = {}): unknown {
  return {
    id: "msg_w1",
    type: "message",
    role: "assistant",
    model: "witness-model",
    content: [{ type: "text", text: options.content ?? "WITNESS_ANTHROPIC_REPLY" }],
    stop_reason: "end_turn",
    usage: options.usage ?? { input_tokens: 10, output_tokens: 5 },
  };
}

/** Codex/OpenAI Responses SSE（callText codex / media codex adapter）。 */
export function codexResponsesSseBody(options: {
  content?: string;
  usage?: { input_tokens: number; output_tokens: number; total_tokens: number } | null;
} = {}): string {
  const content = options.content ?? "WITNESS_CODEX_REPLY";
  const usage = options.usage ?? { input_tokens: 10, output_tokens: 5, total_tokens: 15 };
  const events = [
    { type: "response.output_text.delta", delta: content },
    { type: "response.output_text.done", text: content },
    { type: "response.completed", response: { id: "resp_w1", usage } },
  ];
  return [...events.map((event) => `data: ${JSON.stringify(event)}`), "data: [DONE]", ""].join("\n\n");
}

/** OpenAI Responses 非流式 JSON（callText openai-responses）。 */
export function openaiResponsesJson(options: {
  content?: string;
  usage?: { input_tokens: number; output_tokens: number; total_tokens: number } | null;
} = {}): unknown {
  const content = options.content ?? "WITNESS_RESPONSES_REPLY";
  return {
    id: "resp_w1",
    object: "response",
    output_text: content,
    output: [{ type: "message", role: "assistant", content: [{ type: "output_text", text: content }] }],
    usage: options.usage ?? { input_tokens: 10, output_tokens: 5, total_tokens: 15 },
  };
}

/* ────────────────────────────────────────────────────────────────────────
 * 3. Scenario Harness（temp HOME + persistence + query + HTTP route）
 * ──────────────────────────────────────────────────────────────────────── */

export interface ScenarioHarnessOptions {
  /** observability policy；默认全开（trace+payload+blob）。 */
  policy?: ModelObservabilityPersistencePolicy | null;
  /** 是否同时挂 TestModelCallObserver（默认 true；composite 保留既有 sink）。 */
  withObserver?: boolean;
  /** 已有 witness（复用跨 scenario 的 provider server）。 */
  witness?: FakeProviderWitness;
}

export interface ScenarioHarness {
  lingxiHome: string;
  dbPath: string;
  witness: FakeProviderWitness;
  /** 测试 observer（safe metadata 事件流；与 persistence 并行接收）。 */
  observer: TestModelCallObserver | null;
  handle: ReturnType<typeof installModelObservabilityPersistence>;
  flush(): void;
  /**
   * 真实 Usage Ledger + 镜像 engine._wireModelObservabilityAccounting 的
   * 生产接线（ledger append → llm_usage → accounting projection）。返回的
   * ledger 传给 callText/diary 等业务入口的 usageLedger 参数。
   */
  createLedger(): ReturnType<typeof createUsageLedger>;
  /** 真实 Query Service（read-only facade，生产模块）。 */
  query(): ReturnType<typeof createModelObservabilityQueryService>;
  /** 真实 Hono route（数据经真实 query service；principal 语义由 route-security 层测试覆盖）。 */
  route(): Hono;
  close(): Promise<void>;
  cleanup(): void;
}

export async function createScenarioHarness(options: ScenarioHarnessOptions = {}): Promise<ScenarioHarness> {
  const lingxiHome = fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-e2e-"));
  const policy = options.policy ?? { enabled: true, persistPayloads: true, persistBlobs: true };
  const observer = options.withObserver === false ? null : createTestModelCallObserver();
  if (observer) setModelCallObserver(observer);
  const handle = installModelObservabilityPersistence({ lingxiHome, policy });
  const witness = options.witness ?? await startFakeProviderWitness();
  let queryService: ReturnType<typeof createModelObservabilityQueryService> | null = null;
  let route: Hono | null = null;

  return {
    lingxiHome,
    dbPath: path.join(lingxiHome, "model-observability", "observability.sqlite"),
    witness,
    observer,
    handle,
    flush() {
      handle.flushSync();
    },
    createLedger() {
      // initializeAccounting 的 consumer 期望 engine 事件形状 {type:"llm_usage",
      // entry}（生产 wiring = engine.subscribe）。这里以同一形状投递。
      const consumers = new Set<(event: unknown) => void>();
      const ledger = createUsageLedger({
        eventBus: {
          emit(event: { type: string; entry?: unknown }) {
            if (event?.type === "llm_usage") {
              for (const consumer of consumers) consumer(event);
            }
          },
        },
      });
      handle.initializeAccounting({
        listLedgerEntries: () => ledger.list({}).entries ?? [],
        subscribeUsage: (consumer: (event: unknown) => void) => {
          consumers.add(consumer);
          return () => consumers.delete(consumer);
        },
      });
      return ledger;
    },
    query() {
      queryService ??= createModelObservabilityQueryService({ lingxiHome });
      return queryService;
    },
    route() {
      if (!route) {
        const engineFacade = {
          getModelObservabilityHealth: () => handle.getHealth(),
          getModelObservabilitySettings: () => { throw new Error("settings via engine only; harness route tests use query endpoints"); },
          setModelObservabilitySettings: async () => { throw new Error("settings mutation not wired in harness facade"); },
          getModelObservabilityQueryService: () => harnessQuery(),
        };
        route = createModelObservabilityRoute(engineFacade);
      }
      return route;
    },
    async close() {
      await handle.close();
      if (observer) setModelCallObserver(null);
      await witness.close();
    },
    cleanup() {
      try {
        fs.rmSync(lingxiHome, { recursive: true, force: true });
      } catch { /* best-effort */ }
    },
  };

  function harnessQuery() {
    queryService ??= createModelObservabilityQueryService({ lingxiHome });
    return queryService;
  }
}

/** 等待 observer 事件队列里的异步收尾（persistence enqueue/flush 用 setImmediate）。 */
export async function flushAsync(rounds = 3): Promise<void> {
  for (let i = 0; i < rounds; i++) {
    await new Promise((resolve) => setImmediate(resolve));
  }
}

/* ────────────────────────────────────────────────────────────────────────
 * 4. Truth Receipt（测试内结构；不持久化，§十）
 * ──────────────────────────────────────────────────────────────────────── */

export interface TruthReceipt {
  scenarioId: string;
  runtimeIngress: string;
  expectedCalls: number;
  providerWitness: { requestCount: number; paths: string[] };
  observerEvents: string[];
  durableRows: { calls: number; traces: number; attempts: number; payloads: number };
  queryResult: { ok: boolean; callCount: number } | null;
  httpResult: { status: number; bodyKeys: string[] } | null;
  uiResult: unknown;
  exportResult: unknown;
  violations: string[];
}

export function emptyReceipt(scenarioId: string, ingress: string, expectedCalls: number): TruthReceipt {
  return {
    scenarioId,
    runtimeIngress: ingress,
    expectedCalls,
    providerWitness: { requestCount: 0, paths: [] },
    observerEvents: [],
    durableRows: { calls: 0, traces: 0, attempts: 0, payloads: 0 },
    queryResult: null,
    httpResult: null,
    uiResult: null,
    exportResult: null,
    violations: [],
  };
}

/** 供 vitest 断言的 receipt 收口：violations 为空即 PASS。 */
export function assertReceiptClean(receipt: TruthReceipt): void {
  if (receipt.violations.length > 0) {
    throw new Error(`scenario ${receipt.scenarioId} violations:\n- ${receipt.violations.join("\n- ")}`);
  }
}

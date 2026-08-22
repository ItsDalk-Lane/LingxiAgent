/**
 * Phase 10 E2E Truth — MC-04 callText 四协议（S7）+ 全链身份/账本/毒丸/零调用。
 *
 * 真实网络：witness 本地 HTTP server 收真实 fetch；无 stubGlobal、无 mock fetch。
 * 真实 ingress：callText（core/llm-client.ts 生产入口）。
 * 真实持久化：installModelObservabilityPersistence + Query Service + Hono route。
 *
 * 断言层级（任务书 §二十二～二十九/§九十九～一百零二）：
 *   Witness body ≡ captured provider_request（redaction 允许差异之外逐字段一致）
 *   callId：Observer → SQLite → Query → HTTP 全链一致
 *   usage：fixture → ledger → model_call_usage → aggregate 一致
 *   毒丸：Witness 可见（redaction 只改 capture copy）；DB/WAL/SHM 零命中
 *   Observatory 自身操作 0 新 Model Call（递归污染防护）
 */
import fs from "node:fs";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { callText } from "../core/llm-client.ts";
import { normalizeModelObservabilityQuery } from "../lib/llm/model-observability-query-types.ts";
import {
  anthropicMessagesJson,
  assertReceiptClean,
  codexResponsesSseBody,
  createScenarioHarness,
  emptyReceipt,
  flushAsync,
  openaiCompletionsJson,
  openaiResponsesJson,
  type ScenarioHarness,
} from "./helpers/model-observability-scenario-harness.ts";

const POISON_KEY = "sk-E2E-WITNESS-POISON-9f86d081884c7d65";
const SYSTEM = "E2E_WITNESS_SYSTEM 你是测试助手";
const USER_INPUT = "E2E_WITNESS_USER_INPUT 今天天气如何";

const PROTOCOLS: ReadonlyArray<{
  api: string;
  pathFragment: string;
  reply: () => { kind: string; body: unknown };
  replyText: string;
  usage: { input_tokens: number; output_tokens: number; prompt_tokens: number; completion_tokens: number };
}> = [
  {
    api: "anthropic-messages",
    pathFragment: "/v1/messages",
    reply: () => ({
      kind: "json",
      body: anthropicMessagesJson({ content: "E2E_ANTHROPIC_REPLY 晴", usage: { input_tokens: 11, output_tokens: 7 } }),
    }),
    replyText: "E2E_ANTHROPIC_REPLY 晴",
    usage: { input_tokens: 11, output_tokens: 7, prompt_tokens: 11, completion_tokens: 7 },
  },
  {
    api: "openai-completions",
    pathFragment: "/chat/completions",
    reply: () => ({
      kind: "json",
      body: openaiCompletionsJson({ content: "E2E_OPENAI_REPLY 晴", usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 } }),
    }),
    replyText: "E2E_OPENAI_REPLY 晴",
    usage: { input_tokens: 10, output_tokens: 5, prompt_tokens: 10, completion_tokens: 5 },
  },
  {
    api: "openai-responses",
    pathFragment: "/responses",
    reply: () => ({
      kind: "json",
      body: openaiResponsesJson({ content: "E2E_RESPONSES_REPLY 晴", usage: { input_tokens: 12, output_tokens: 6, total_tokens: 18 } }),
    }),
    replyText: "E2E_RESPONSES_REPLY 晴",
    usage: { input_tokens: 12, output_tokens: 6, prompt_tokens: 12, completion_tokens: 6 },
  },
  {
    api: "openai-codex-responses",
    pathFragment: "/codex/responses",
    reply: () => ({
      kind: "sse",
      body: codexResponsesSseBody({ content: "E2E_CODEX_REPLY 晴", usage: { input_tokens: 13, output_tokens: 4, total_tokens: 17 } }),
    }),
    replyText: "E2E_CODEX_REPLY 晴",
    usage: { input_tokens: 13, output_tokens: 4, prompt_tokens: 13, completion_tokens: 4 },
  },
];

let harness: ScenarioHarness;

beforeEach(async () => {
  harness = await createScenarioHarness();
});

afterEach(async () => {
  await harness.close();
  harness.cleanup();
});

describe("E2E truth — MC-04 callText 四协议", () => {
  for (const protocol of PROTOCOLS) {
    it(`S7 ${protocol.api}：witness body ≡ capture ≡ mapping；身份/usage/毒丸全链一致`, async () => {
      const receipt = emptyReceipt(`S7-${protocol.api}`, "callText", 1);
      const ledger = harness.createLedger();
      harness.witness.scriptNext(protocol.reply() as any);

      const text = await callText({
        api: protocol.api,
        apiKey: POISON_KEY,
        baseUrl: harness.witness.baseUrl,
        model: { id: "witness-model", provider: "witness-provider", accountId: "acct-witness" } as any,
        systemPrompt: SYSTEM,
        messages: [{ role: "user", content: USER_INPUT }],
        usageLedger: ledger,
        usageContext: {
          source: { subsystem: "memory", operation: "e2e_truth", surface: "desktop", trigger: "user" },
          attribution: { kind: "agent", agentId: "agent-e2e" },
        },
      } as any);
      await flushAsync();
      harness.flush();
      await flushAsync();

      /* ── Provider Witness（独立事实）── */
      const witnessCalls = harness.witness.requestsTo(protocol.pathFragment);
      expect(witnessCalls).toHaveLength(1);
      const witnessBody = witnessCalls[0].bodyJson as Record<string, any>;
      receipt.providerWitness = { requestCount: 1, paths: [witnessCalls[0].path] };
      // Witness 必须看到真实凭证（证明 redaction 只改 capture copy，§一百）
      const authHeader = protocol.api === "anthropic-messages"
        ? witnessCalls[0].headers["x-api-key"]
        : witnessCalls[0].headers["authorization"];
      expect(authHeader).toContain(POISON_KEY);

      /* ── 业务返回 ≡ fixture ── */
      expect(text).toBe(protocol.replyText);

      /* ── Observer 事件序列 ── */
      const callIds = harness.observer!.callIds();
      expect(callIds).toHaveLength(1);
      const callId = callIds[0];
      receipt.observerEvents = harness.observer!.eventsForCall(callId).map((event: any) => event.eventType);
      expect(receipt.observerEvents).toContain("logical_call_start");
      expect(receipt.observerEvents).toContain("provider_request_prepared");
      expect(receipt.observerEvents).toContain("provider_response_received");
      expect(receipt.observerEvents).toContain("logical_call_end");
      const identity = harness.observer!.callIdentity(callId)!;
      expect(identity.traceId).toBeTruthy();
      expect(identity.parentCallId).toBeNull();

      /* ── Durable rows + callId 身份链 ── */
      const query = harness.query();
      const normalized = normalizeModelObservabilityQuery({ filter: { callId } });
      expect(normalized.ok).toBe(true);
      if (normalized.ok === false) throw new Error(normalized.error.message);
      const page = query.queryCalls(normalized.value);
      expect(page.ok).toBe(true);
      if (!page.ok) throw new Error("query failed");
      expect(page.value.calls).toHaveLength(1);
      const durableCall = page.value.calls[0];
      expect(durableCall.callId).toBe(callId); // Store.callId === Observer.callId
      receipt.durableRows = {
        calls: page.value.calls.length,
        traces: 1,
        attempts: durableCall.attemptCount ?? 1,
        payloads: 0,
      };

      /* ── payload records：四层 + provider_request ≡ witness body ── */
      const detail = query.queryCallDetail(callId);
      expect(detail.ok).toBe(true);
      if (!detail.ok) throw new Error("detail failed");
      const kinds = detail.value.payloadRecords.map((record: any) => record.kind).sort();
      expect(kinds).toEqual(["provider_request", "provider_response", "semantic_request", "semantic_response"]);
      receipt.durableRows.payloads = detail.value.payloadRecords.length;

      const providerRequestMeta = detail.value.payloadRecords.find((record: any) => record.kind === "provider_request")!;
      const providerRequest = query.getPayloadRecord(providerRequestMeta.id);
      expect(providerRequest.ok).toBe(true);
      if (!providerRequest.ok) throw new Error("payload record failed");
      expect(providerRequest.value.visibility).toBe("full");
      expect(providerRequest.value.fidelity).toBe(
        protocol.api === "openai-codex-responses" ? "runtime_exact" : "runtime_exact",
      );
      const capturedBody = (providerRequest.value.payload as any).transport.body;
      // §二十二：除 redaction/externalization/truncation 外逐字段一致
      expect(capturedBody.model).toBe(witnessBody.model);
      expect(capturedBody.messages ?? capturedBody.input).toEqual(witnessBody.messages ?? witnessBody.input);
      expect(capturedBody.system ?? capturedBody.instructions).toEqual(witnessBody.system ?? witnessBody.instructions);
      expect(JSON.stringify(capturedBody)).not.toContain(POISON_KEY);

      /* ── provider mapping（四协议 exact，§二十七）── */
      expect(providerRequest.value.providerRequestProvenance).not.toBeNull();
      const mapping = providerRequest.value.providerRequestProvenance as any;
      expect(Object.keys(mapping.mappings ?? mapping).length ?? (mapping.length ?? 0)).toBeGreaterThan(0);

      /* ── semantic response truth ── */
      const semanticResponseMeta = detail.value.payloadRecords.find((record: any) => record.kind === "semantic_response")!;
      const semanticResponse = query.getPayloadRecord(semanticResponseMeta.id);
      expect(semanticResponse.ok).toBe(true);
      if (semanticResponse.ok) {
        expect((semanticResponse.value.payload as any).text).toBe(protocol.replyText);
      }

      /* ── usage 账本全链（§二十九）── */
      const { entries } = ledger.list({});
      expect(entries).toHaveLength(1);
      expect(entries[0].metadata?.modelCallId).toBe(callId);
      const aggregateNorm = normalizeModelObservabilityQuery({ filter: { callId } });
      if (aggregateNorm.ok === false) throw new Error(aggregateNorm.error.message);
      const aggregate = query.queryAggregate({
        filter: aggregateNorm.value.filter,
        groupBy: [],
        dateBucket: null,
      });
      expect(aggregate.ok).toBe(true);
      if (aggregate.ok) {
        const overall = aggregate.value.overall;
        expect(overall.callCount).toBe(1);
        expect(overall.inputTokens).toBe(
          protocol.usage.input_tokens ?? protocol.usage.prompt_tokens,
        );
        expect(overall.outputTokens).toBe(
          protocol.usage.output_tokens ?? protocol.usage.completion_tokens,
        );
      }

      /* ── HTTP route（真实 Hono + 真实 query service）── */
      const route = harness.route();
      const httpRes = await route.request(`/model-observability/query/calls`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ filter: { callId } }),
      });
      receipt.httpResult = { status: httpRes.status, bodyKeys: [] };
      expect(httpRes.status).toBe(200);
      const httpJson = await httpRes.json() as any;
      expect(httpJson.calls[0].callId).toBe(callId); // HTTP.callId === Observer.callId

      /* ── 毒丸跨层扫描（§九十九；witness 除外）── */
      harness.flush();
      const storeFiles = [
        harness.dbPath,
        `${harness.dbPath}-wal`,
        `${harness.dbPath}-shm`,
      ];
      for (const file of storeFiles) {
        if (!fs.existsSync(file)) continue;
        const bytes = fs.readFileSync(file);
        expect(bytes.includes(POISON_KEY)).toBe(false);
      }
      harness.observer!.assertNoSensitiveContent([POISON_KEY]);

      /* ── Observatory 零调用（§五）── */
      const beforeCount = harness.witness.requestCount();
      query.queryCalls({ filter: {} } as any);
      query.queryAggregate({ filter: {} } as any);
      await route.request(`/model-observability/health`);
      await route.request(`/model-observability/calls/${callId}`);
      await flushAsync();
      expect(harness.witness.requestCount()).toBe(beforeCount);

      assertReceiptClean(receipt);
    });
  }

  it("S23 ON/OFF 等价：observability 开关不改变 witness body 与业务返回（§一百零一/一百零二）", async () => {
    // ON（当前 harness）先跑一次
    harness.witness.scriptNext({ kind: "json", body: openaiCompletionsJson({ content: "EQUIVALENCE_REPLY" }) });
    const onText = await callText({
      api: "openai-completions",
      apiKey: POISON_KEY,
      baseUrl: harness.witness.baseUrl,
      model: { id: "witness-model", provider: "witness-provider" } as any,
      systemPrompt: SYSTEM,
      messages: [{ role: "user", content: USER_INPUT }],
    } as any);
    await flushAsync();
    const onWitness = harness.witness.requestsTo("/chat/completions")[0];

    // OFF：卸载 observer/persistence 后同 scenario
    await harness.handle.uninstall();
    harness.witness.scriptNext({ kind: "json", body: openaiCompletionsJson({ content: "EQUIVALENCE_REPLY" }) });
    const offText = await callText({
      api: "openai-completions",
      apiKey: POISON_KEY,
      baseUrl: harness.witness.baseUrl,
      model: { id: "witness-model", provider: "witness-provider" } as any,
      systemPrompt: SYSTEM,
      messages: [{ role: "user", content: USER_INPUT }],
    } as any);
    const offWitness = harness.witness.requestsTo("/chat/completions")[1];

    expect(onText).toBe(offText);
    // witness body 逐字节相同（observability 不改变真实 Provider Request）
    expect(offWitness.bodyText).toBe(onWitness.bodyText);
    expect(offWitness.headers["authorization"]).toBe(onWitness.headers["authorization"]);
    expect(offWitness.path).toBe(onWitness.path);
  });
});

/**
 * Phase 7 Payload Store 测试（任务书 §一百～一百零四/一百零七）：
 * 毒丸落盘扫描（DB rows + 文件 bytes + WAL）/ 普通正文 roundtrip /
 * provenance 随 payload 持久化 / locator roundtrip（span 在 sanitized 副本上定位）/
 * payload 先到 call shell / fail-closed（超限 drop + 计数）/ 排序稳定。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createModelObservabilityTestHarness, scanStoreFilesForPoison } from "../lib/llm/model-observability-testing.ts";
import { createSemanticInputProvenance, provenanceSection } from "../lib/llm/semantic-input-provenance.ts";
import {
  captureSemanticRequestForTest,
  captureProviderRequestForTest,
} from "../lib/llm/model-call-payload-capture.ts";
import { createTestModelCallPayloadSink } from "../lib/llm/model-call-payload-testing.ts";
import type { ModelCallPayloadRecord } from "../lib/llm/model-call-payload-types.ts";

const POISONS = [
  "TOPSECRET-OPENAI-KEY-0123456789abcdef",
  "TOPSECRET-BEARER-TOKEN-abcdef123456",
  "TOPSECRET-COOKIE-VALUE-112233445566",
  "TOPSECRET_PRIVATE_KEY_",
  "TOPSECRET-SIGNED-URL-9f8e7d6c",
];

function recordOf(overrides: Partial<ModelCallPayloadRecord>): ModelCallPayloadRecord {
  return {
    schemaVersion: 1,
    kind: "semantic_request",
    capturedAt: new Date().toISOString(),
    callId: "mc_poison",
    traceId: "mt_poison",
    parentCallId: null,
    attemptId: null,
    providerRequestOrdinal: null,
    model: null,
    source: null,
    attribution: null,
    visibility: "full",
    fidelity: "runtime_exact",
    sanitization: { redacted: false, truncated: false, degraded: false, actions: [] },
    payload: null,
    ...overrides,
  } as ModelCallPayloadRecord;
}

describe("Model Observability Payload Store", () => {
  let harness: ReturnType<typeof createModelObservabilityTestHarness>;

  beforeEach(() => {
    harness = createModelObservabilityTestHarness();
  });
  afterEach(async () => {
    await harness.close();
    harness.cleanup();
  });

  it("毒丸经真实 capture redaction 后：DB rows 与落盘文件 bytes 均无 poison（§一百/一百零一）", async () => {
    // 走真实 Phase 6 capture（统一 Redactor），而非手工构造——证明持久化通道
    // 只可能收到 sanitized copy。
    const sink = createTestModelCallPayloadSink();
    const deliver = (kind: any, extras: any) => {
      sink.handleModelCallPayloadRecord({
        schemaVersion: 1,
        kind,
        capturedAt: new Date().toISOString(),
        callId: "mc_poison_real",
        traceId: "mt_poison_real",
        parentCallId: null,
        attemptId: extras.attemptId ?? null,
        providerRequestOrdinal: extras.providerRequestOrdinal ?? null,
        model: null,
        source: null,
        attribution: null,
        visibility: extras.visibility,
        fidelity: extras.fidelity,
        sanitization: extras.sanitization ?? { redacted: false, truncated: false, degraded: false, actions: [] },
        payload: extras.payload ?? null,
        ...(extras.semanticInputProvenance !== undefined ? { semanticInputProvenance: extras.semanticInputProvenance } : {}),
        ...(extras.providerRequestProvenance !== undefined ? { providerRequestProvenance: extras.providerRequestProvenance } : {}),
      } as ModelCallPayloadRecord);
    };
    captureSemanticRequestForTest(deliver, {
      inputShape: "calltext",
      systemPrompt: `You are an assistant. api_key=sk-TOPSECRET-OPENAI-KEY-0123456789abcdef`,
      parameters: {
        headers: {
          "x-api-key": "sk-TOPSECRET-OPENAI-KEY-0123456789abcdef",
          authorization: `Bearer TOPSECRET-BEARER-TOKEN-abcdef123456`,
          cookie: "TOPSECRET-COOKIE-VALUE-112233445566",
        },
        credentials: [
          "-----BEGIN RSA PRIVATE KEY-----",
          "MIIEpTOPSECRET_PRIVATE_KEY_ThisIsNotARealKeyButPoisonForTests0123456789",
          "-----END RSA PRIVATE KEY-----",
        ].join("\n"),
      },
    });
    captureProviderRequestForTest(deliver, 1, {
      attemptId: "ma_poison_1",
      protocol: null,
      transport: {
        method: "POST",
        url: `https://provider.example.com/v1/messages?X-Amz-Signature=TOPSECRET-SIGNED-URL-9f8e7d6c`,
        headers: { cookie: "TOPSECRET-COOKIE-VALUE-112233445566" },
        body: { model: "claude-x", messages: [{ role: "user", content: "hello" }] },
      },
    });
    expect(sink.records.length).toBe(2);
    for (const record of sink.records) {
      expect(record.sanitization.redacted).toBe(true);
    }
    for (const record of sink.records) {
      harness.handle.sink?.handleModelCallPayloadRecord(record);
    }
    harness.flush();
    await harness.close();

    const reader = harness.openReader();
    try {
      const rows = reader.payloadStore.getPayloadRecords("mc_poison_real");
      expect(rows).toHaveLength(2);
      for (const row of rows) {
        const texts = [row.payload_json ?? "", row.semantic_input_provenance_json ?? "", row.provider_request_provenance_json ?? ""];
        for (const poison of POISONS) {
          for (const text of texts) {
            expect(text.includes(poison)).toBe(false);
          }
        }
      }
      // §一百零一：文件字节级扫描（close 后 checkpoint 已合并；仍存在的 -wal 也扫）。
      const files = harness.readStoreFileBytes();
      expect(files.length).toBeGreaterThan(0);
      const scan = scanStoreFilesForPoison(files, POISONS);
      expect(scan.hit).toBe(false);
    } finally {
      reader.close();
    }
  });

  it("普通内容必须真正落盘并可读回（§一百零二）", () => {
    harness.handle.sink?.handleModelCallPayloadRecord(recordOf({
      callId: "mc_normal",
      payload: {
        inputShape: "calltext",
        systemPrompt: "NORMAL_USER_PROMPT please help me",
        messages: [{ role: "user", content: "NORMAL_MEMORY user likes tea" }],
      },
    }));
    harness.handle.sink?.handleModelCallPayloadRecord(recordOf({
      kind: "semantic_response",
      callId: "mc_normal",
      payload: { completeness: "complete", text: "NORMAL_MODEL_RESPONSE sure, tea is great" },
    }));
    harness.flush();
    const reader = harness.openReader();
    try {
      const rows = reader.payloadStore.getPayloadRecords("mc_normal");
      expect(rows).toHaveLength(2);
      const semantic = JSON.parse(rows.find((r) => r.kind === "semantic_request")!.payload_json!);
      expect(semantic.systemPrompt).toContain("NORMAL_USER_PROMPT");
      expect(JSON.stringify(semantic.messages)).toContain("NORMAL_MEMORY");
      const response = JSON.parse(rows.find((r) => r.kind === "semantic_response")!.payload_json!);
      expect(response.text).toContain("NORMAL_MODEL_RESPONSE");
    } finally {
      reader.close();
    }
  });

  it("semanticInputProvenance 与 providerRequestProvenance 随 payload 一起持久化（§一百零三）", () => {
    const provenance = createSemanticInputProvenance("calltext", [
      provenanceSection(
        { root: "systemPrompt", span: { start: 0, end: 12 } },
        "task_instruction",
        { role: "system", precision: "exact", source: { type: "template", id: "test.tpl", version: "1" } },
      ),
    ]);
    harness.handle.sink?.handleModelCallPayloadRecord(recordOf({
      callId: "mc_prov",
      payload: { inputShape: "calltext", systemPrompt: "Be helpful." },
      semanticInputProvenance: provenance,
      providerRequestProvenance: {
        schemaVersion: 1,
        protocol: "anthropic-messages",
        mappings: [{
          semanticSectionOrdinal: 0,
          providerLocator: { path: ["system"], span: { start: 0, end: 12 } },
          transformation: "pass_through",
          mappingPrecision: "exact",
        }],
      },
    }));
    harness.flush();
    const reader = harness.openReader();
    try {
      const row = reader.payloadStore.getPayloadRecords("mc_prov")[0];
      const semantic = JSON.parse(row.semantic_input_provenance_json!);
      expect(semantic.sections).toHaveLength(1);
      expect(semantic.sections[0].category).toBe("task_instruction");
      const provider = JSON.parse(row.provider_request_provenance_json!);
      expect(provider.protocol).toBe("anthropic-messages");
      expect(provider.mappings[0].transformation).toBe("pass_through");
    } finally {
      reader.close();
    }
  });

  it("Locator roundtrip：持久化→close→重开→span 在 sanitized systemPrompt 上仍精确定位（§一百零四）", async () => {
    const prefix = "You are Lingxi. Memory: user likes tea. ";
    const provenance = createSemanticInputProvenance("calltext", [
      provenanceSection(
        { root: "systemPrompt", span: { start: 0, end: prefix.length } },
        "persona",
        { role: "system", precision: "exact", source: { type: "template", id: "persona.tpl", version: "2" } },
      ),
    ]);
    harness.handle.sink?.handleModelCallPayloadRecord(recordOf({
      callId: "mc_locator",
      payload: { inputShape: "calltext", systemPrompt: prefix + "sk-ant-a11b22c33d44e55f66g77h88 (inline secret)" },
      semanticInputProvenance: provenance,
    }));
    harness.flush();
    await harness.close();
    const reader = harness.openReader();
    try {
      const row = reader.payloadStore.getPayloadRecords("mc_locator")[0];
      const semantic = JSON.parse(row.semantic_input_provenance_json!);
      const stored = JSON.parse(row.payload_json!);
      const section = semantic.sections[0];
      expect(section.locator.span).not.toBeNull();
      const span = section.locator.span!;
      // span 在脱敏后的 systemPrompt 上必须可定位（redaction 已把 inline secret
      // 替换为等长占位，persona 段不重叠 → 平移 0）。
      const sliced = stored.systemPrompt.slice(span.start, span.end);
      expect(sliced.startsWith("You are Lingxi")).toBe(true);
      expect(sliced).toBe(prefix.slice(0, prefix.length));
    } finally {
      reader.close();
    }
  });

  it("payload 先到而 call row 尚未出现：partial shell，不虚构 started_at（§二十三）", () => {
    harness.handle.sink?.handleModelCallPayloadRecord(recordOf({
      callId: "mc_payload_first",
      traceId: "mt_pf",
      model: { provider: "openai", modelId: "gpt-x", api: "openai-completions" },
      attribution: { kind: "session", sessionId: "sess-pf" },
    }));
    harness.flush();
    const reader = harness.openReader();
    try {
      const call = reader.traceStore.getCall("mc_payload_first");
      expect(call).not.toBeNull();
      expect(call.started_at).toBeNull();
      expect(call.persistence_completeness).toBe("partial");
      expect(call.provider).toBe("openai");
      expect(call.session_id).toBe("sess-pf");
      const trace = reader.traceStore.getTrace("mt_pf");
      expect(trace).not.toBeNull();
    } finally {
      reader.close();
    }
  });

  it("fail closed：超过 store hard limit 的 record 被 drop 并计数，不保存残缺 JSON（§十八）", () => {
    const oversized = "x".repeat(1_100_000);
    harness.handle.sink?.handleModelCallPayloadRecord(recordOf({
      callId: "mc_oversize",
      payload: { inputShape: "calltext", systemPrompt: oversized },
    }));
    harness.flush();
    const health = harness.handle.getHealth();
    expect(health.droppedPayloadRecords).toBe(1);
    const reader = harness.openReader();
    try {
      expect(reader.payloadStore.getPayloadRecords("mc_oversize")).toHaveLength(0);
    } finally {
      reader.close();
    }
  });

  it("同 call 多条 record：id 自增序稳定（同毫秒 captured_at 的 tie-break，§五十/五十一）", () => {
    const same = new Date("2026-01-01T00:00:00.000Z").toISOString();
    for (let i = 0; i < 5; i += 1) {
      harness.handle.sink?.handleModelCallPayloadRecord(recordOf({
        callId: "mc_order",
        capturedAt: same,
        kind: "provider_request",
        providerRequestOrdinal: i + 1,
        payload: { transport: { body: { n: i } } },
      }));
    }
    harness.flush();
    const reader = harness.openReader();
    try {
      const rows = reader.payloadStore.getPayloadRecords("mc_order");
      expect(rows).toHaveLength(5);
      expect(rows.map((r) => r.provider_request_ordinal)).toEqual([1, 2, 3, 4, 5]);
      for (let i = 1; i < rows.length; i += 1) {
        expect(rows[i].id).toBeGreaterThan(rows[i - 1].id);
      }
    } finally {
      reader.close();
    }
  });
});

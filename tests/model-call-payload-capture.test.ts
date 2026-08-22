/**
 * Phase 6 Capture Channel 单元测试（§十一/§十三/§四十五/§一百二十五/§一百二十八）：
 * noop 快路径、sink 故障旁路、record 形状/ordinal、provenance span remap、
 * wire-unavailable 显式 record、test sink 毒丸断言。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getModelCallPayloadSink, setModelCallPayloadSink, createModelCallPayloadCaptureSession } from "../lib/llm/model-call-payload-capture.ts";
import { createTestModelCallPayloadSink, installTestPayloadSink } from "../lib/llm/model-call-payload-testing.ts";
import { createSemanticInputProvenance, provenanceSection } from "../lib/llm/semantic-input-provenance.ts";
import { sanitizeValueForCapture } from "../lib/llm/model-call-payload-redaction.ts";

describe("Payload Capture Channel", () => {
  let sink: ReturnType<typeof createTestModelCallPayloadSink>;

  beforeEach(() => {
    sink = installTestPayloadSink();
  });
  afterEach(() => {
    setModelCallPayloadSink(null);
  });

  it("默认 sink 是 noop：createModelCallPayloadCaptureSession 返回 null（快路径）", () => {
    setModelCallPayloadSink(null);
    expect(getModelCallPayloadSink()).toBeDefined();
    const session = createModelCallPayloadCaptureSession({ callId: "mc_test_1" });
    expect(session).toBeNull();
  });

  it("noop 快路径不执行正文遍历（session 为 null，结构上无 capture 调用点，§一百三十三）", async () => {
    setModelCallPayloadSink(null);
    const redaction = await import("../lib/llm/model-call-payload-redaction.ts");
    const spy = vi.spyOn(redaction, "sanitizeValueForCapture");
    const session = createModelCallPayloadCaptureSession({ callId: "mc_test_2" });
    expect(session).toBeNull();
    expect(spy).not.toHaveBeenCalled();
    spy.mockRestore();
  });

  it("sink throw 不影响 capture 调用方（§一百三十四）", () => {
    setModelCallPayloadSink({
      handleModelCallPayloadRecord() {
        throw new Error("sink exploded");
      },
    });
    const session = createModelCallPayloadCaptureSession({ callId: "mc_test_3" });
    expect(session).not.toBeNull();
    expect(() => session!.captureSemanticRequest({ inputShape: "calltext", systemPrompt: "hello" })).not.toThrow();
    expect(() => session!.captureSemanticResponse({ response: { text: "ok", completeness: "complete" } })).not.toThrow();
  });

  it("record 形状：schemaVersion/kind/identity/sanitization/payload", () => {
    const session = createModelCallPayloadCaptureSession({
      callId: "mc_shape_1",
      traceId: "mt_1",
      parentCallId: null,
      model: { provider: "openai", modelId: "gpt-test", api: "openai-completions" },
    })!;
    session.setAttempt("ma_1");
    session.captureSemanticRequest({
      inputShape: "calltext",
      systemPrompt: "persona text",
      messages: [{ role: "user", content: "NORMAL_USER_PROMPT_VISIBLE" }],
    });
    session.captureProviderRequest({
      attemptId: "ma_1",
      protocol: "openai-completions",
      transport: { method: "POST", url: "https://api.test/v1/chat/completions", headers: { Authorization: "Bearer sk-secret" }, body: { model: "gpt-test" } },
    });
    session.captureProviderResponse({ status: 200, headers: { "x-request-id": "req-1" }, body: { ok: true }, fidelity: "parsed_equivalent" });
    session.captureSemanticResponse({ response: { text: "answer", completeness: "complete" } });

    expect(sink.sequenceForCall("mc_shape_1")).toEqual([
      "semantic_request",
      "provider_request",
      "provider_response",
      "semantic_response",
    ]);
    const [request] = sink.records;
    expect(request.schemaVersion).toBe(1);
    expect(request.callId).toBe("mc_shape_1");
    expect(request.traceId).toBe("mt_1");
    expect(request.attemptId).toBe("ma_1");
    const providerRequest = sink.providerRequestsForCall("mc_shape_1")[0];
    expect(providerRequest.providerRequestOrdinal).toBe(1);
    expect(providerRequest.fidelity).toBe("runtime_exact");
    expect(providerRequest.visibility).toBe("full");
    const transport = providerRequest.payload as any;
    expect(transport.transport.headers.Authorization).toBe("<redacted:credential>");
    expect(transport.transport.body.model).toBe("gpt-test");
    const response = sink.providerResponsesForCall("mc_shape_1")[0];
    expect(response.fidelity).toBe("parsed_equivalent");
    expect((response.payload as any).status).toBe(200);
    expect(response.providerRequestOrdinal).toBe(1);
    const semantic = sink.semanticResponseForCall("mc_shape_1");
    expect((semantic!.payload as any).text).toBe("answer");
  });

  it("providerRequestOrdinal 单调递增（codex 401 refresh 场景，§十九/§九十二）", () => {
    const session = createModelCallPayloadCaptureSession({ callId: "mc_ordinal" })!;
    session.setAttempt("ma_a");
    session.captureProviderRequest({ transport: { body: { n: 1 } } });
    session.setAttempt("ma_b");
    session.captureProviderRequest({ transport: { body: { n: 2 } } });
    const ordinals = sink.providerRequestsForCall("mc_ordinal").map((r) => r.providerRequestOrdinal);
    expect(ordinals).toEqual([1, 2]);
    expect(sink.providerRequestsForCall("mc_ordinal")[1].attemptId).toBe("ma_b");
  });

  it("noteProviderWireUnavailable：显式 unavailable record（§一百零三）", () => {
    const session = createModelCallPayloadCaptureSession({ callId: "mc_unavail" })!;
    session.captureSemanticRequest({ inputShape: "pi_direct_summary", messages: [] });
    session.noteProviderWireUnavailable("provider_request", { reason: "pi-summarizer-no-provider-hook", visibility: "unavailable", fidelity: "opaque" });
    session.noteProviderWireUnavailable("provider_response", { reason: "pi-summarizer-no-provider-hook", visibility: "unavailable", fidelity: "opaque" });
    const [req, res] = [sink.recordsOfKind("mc_unavail", "provider_request")[0], sink.recordsOfKind("mc_unavail", "provider_response")[0]];
    expect(req.visibility).toBe("unavailable");
    expect(req.payload).toBeNull();
    expect(res.visibility).toBe("unavailable");
  });

  it("semantic request 的 provenance span 随 redaction remap（§四十六/§四十九）", () => {
    const secret = "api_key=TOPSECRET_SPANREMAP_0123456789abcdef";
    const prefix = "Persona prefix. ";
    const suffix = " Suffix part.";
    const systemPrompt = `${prefix}${secret}${suffix}`;
    // provenance: prefix span（无重叠）+ secret 段（重叠）
    const provenance = createSemanticInputProvenance("calltext", [
      provenanceSection({ root: "systemPrompt", span: { start: 0, end: prefix.length } }, "persona", { role: "system" }),
      provenanceSection({ root: "systemPrompt", span: { start: prefix.length, end: prefix.length + secret.length } }, "task_input"),
    ])!;
    const session = createModelCallPayloadCaptureSession({ callId: "mc_remap" })!;
    session.captureSemanticRequest({ inputShape: "calltext", systemPrompt, provenance });
    const record = sink.semanticRequestForCall("mc_remap")!;
    const captured = record.payload as any;
    // 无重叠段：span 平移后仍解析到同一文本（§四十七 硬要求）
    const remapped = record.semanticInputProvenance!.sections[0];
    expect(remapped.locator.span).not.toBeNull();
    expect(captured.systemPrompt.slice(remapped.locator.span!.start, remapped.locator.span!.end)).toBe(prefix);
    // 重叠段：span=null + precision 降级 structural（§五十）
    const degraded = record.semanticInputProvenance!.sections[1];
    expect(degraded.locator.span).toBeNull();
    expect(degraded.precision).toBe("structural");
    // 正文毒丸不进 sink
    expect(JSON.stringify(sink.records)).not.toContain("TOPSECRET");
  });

  it("test sink 毒丸断言（§一百二十八）", () => {
    const session = createModelCallPayloadCaptureSession({ callId: "mc_poison" })!;
    session.captureSemanticResponse({ response: { text: "Bearer TOPSECRET_TOKEN_IN_RESPONSE_abcdefgh", completeness: "complete" } });
    expect(() => sink.assertNoSensitiveContent(["TOPSECRET_TOKEN_IN_RESPONSE"]))
      .not.toThrow();
    sink.handleModelCallPayloadRecord({
      schemaVersion: 1, kind: "semantic_response", capturedAt: new Date().toISOString(),
      callId: "mc_bad", traceId: null, parentCallId: null, attemptId: null,
      providerRequestOrdinal: null, model: null, source: null, attribution: null,
      visibility: "full", fidelity: "normalized", sanitization: { redacted: false, truncated: false, degraded: false, actions: [] },
      payload: { text: "leaked TOPSECRET_RAW_INJECTION" },
    });
    expect(() => sink.assertNoSensitiveContent(["TOPSECRET_RAW_INJECTION"])).toThrow(/leaked sensitive content/);
  });
});

/* ── sanitizeValueForCapture 直接行为补充 ──────────────────────────── */

describe("sanitizeValueForCapture 顶层行为", () => {
  it("undefined / null / number / boolean 归一", () => {
    expect(sanitizeValueForCapture(undefined).value).toBeNull();
    expect(sanitizeValueForCapture(null).value).toBeNull();
    expect(sanitizeValueForCapture(42).value).toBe(42);
    expect(sanitizeValueForCapture(true).value).toBe(true);
    expect(sanitizeValueForCapture(Number.NaN).value).toBeNull();
  });
});

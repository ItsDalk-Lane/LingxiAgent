/**
 * Phase 7 MC-01～MC-10 Durable Matrix 测试（任务书 §九十七～九十九/一百三十四）。
 *
 * 每条 MC path 用真实生产模块（ModelCallRecorder + ModelCallPayloadCapture
 * session）按 Phase 6 已实证的运行时事件/记录序列驱动持久化，然后直接读 DB
 * 断言 durable 事实。Persistence 不得把 Phase 6 的可见度真相升级：
 * MC-02/03/10 provider wire=UNAVAILABLE、MC-07=OPAQUE、MC-01 response=
 * METADATA_ONLY 持久化后必须原样保留。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createModelObservabilityTestHarness } from "../lib/llm/model-observability-testing.ts";
import { createModelCallRecorder } from "../lib/llm/model-call-recorder.ts";
import { createModelCallPayloadCaptureSession, type ModelCallPayloadCaptureSession } from "../lib/llm/model-call-payload-capture.ts";

const MODEL_PI = { provider: "anthropic", modelId: "claude-x", api: "anthropic-messages" };
const MODEL_OPENAI = { provider: "openai", modelId: "gpt-x", api: "openai-completions" };
const MODEL_CODEX = { provider: "openai", modelId: "gpt-image", api: "openai-codex-responses" };
const MODEL_CLI = { provider: "dreamina", modelId: "dreamina-v3", api: "jimeng-cli-image" };
const MODEL_SPEECH = { provider: "volcengine", modelId: "bigasr", api: "volcengine-bigasr-transcription" };

const SOURCE = (subsystem: string, operation: string) => ({
  subsystem, operation, surface: "server", trigger: "user_turn",
} as const);

type Mc = {
  callId: string;
  traceId: string;
  observer: ReturnType<typeof createModelCallRecorder> | null;
  session: ModelCallPayloadCaptureSession | null;
};

describe("Model Observability Durable Matrix (MC-01～MC-10)", () => {
  let harness: ReturnType<typeof createModelObservabilityTestHarness>;

  beforeEach(() => {
    harness = createModelObservabilityTestHarness({
      policy: { enabled: true, persistPayloads: true, persistBlobs: true },
    });
  });
  afterEach(async () => {
    await harness.close();
    harness.cleanup();
  });

  function beginMc(
    callId: string,
    traceId: string,
    model: { provider: string; modelId: string; api: string },
    source: { subsystem: string; operation: string; surface: string; trigger: string },
    attemptDetails: Record<string, unknown> = {},
  ): Mc {
    const recorder = createModelCallRecorder({
      observer: harness.handle.observer,
      identity: {
        mintCallId: () => callId,
        mintAttemptId: (() => {
          let n = 0;
          return () => `${callId}_att_${++n}`;
        })(),
        mintTraceId: () => traceId,
      },
      context: { callId, traceId, model, source, attribution: { kind: "session", sessionId: `sess_${callId}` } },
    });
    const session = createModelCallPayloadCaptureSession({
      callId, traceId, parentCallId: null, model, source,
      attribution: { kind: "session", sessionId: `sess_${callId}` },
    });
    recorder.beginLogicalCall({ details: { traceOrigin: "user_turn" } });
    recorder.beginAttempt({ details: attemptDetails });
    recorder.attachPayloadCapture(session);
    if (session) session.setAttempt(recorder.currentAttemptId);
    return { callId, traceId, observer: recorder, session };
  }

  function rows(callId: string) {
    const reader = harness.openReader();
    try {
      return {
        call: reader.traceStore.getCall(callId),
        attempts: reader.traceStore.getAttempts(callId),
        payloads: reader.payloadStore.getPayloadRecords(callId),
      };
    } finally {
      reader.close();
    }
  }

  function payloadBodies(records: Array<{ kind: string; payload_json: string | null }>) {
    return records.map((r) => ({ kind: r.kind, body: r.payload_json ? JSON.parse(r.payload_json) : null }));
  }

  it("MC-01 Pi Chat：attempt=logical_boundary；provider_request FULL、response METADATA_ONLY、semantic 双 FULL；mapping null 原样", () => {
    const mc = beginMc("mc_d1", "mt_d1", MODEL_PI, SOURCE("pi", "chat"), {
      attemptVisibility: "logical_boundary",
    });
    mc.session!.captureSemanticRequest({
      inputShape: "chat_context",
      systemPrompt: "You are Lingxi.",
      messages: [{ role: "user", content: "hi" }],
    });
    // before_provider_request hook：compat 后最终 body，无 headers/endpoint。
    mc.session!.captureProviderRequest({
      attemptId: mc.observer!.currentAttemptId,
      transport: { body: { model: "claude-x", system: "You are Lingxi.", messages: [{ role: "user", content: "hi" }] } },
      fidelity: "runtime_exact",
      provenance: null, // Pi 路径无 sidecar（§六十一诚实 null）
    });
    // after_provider_response hook：只有 status+headers。
    mc.session!.captureProviderResponse({
      attemptId: mc.observer!.currentAttemptId,
      status: 200,
      headers: { "x-request-id": "req_d1" },
      body: null,
      fidelity: "metadata_only",
    });
    mc.session!.captureSemanticResponse({ response: { text: "Hello!", completeness: "complete" } });
    mc.observer!.providerRequestPrepared({ details: { messageCount: 1 } });
    mc.observer!.providerResponseReceived({ httpStatus: 200, providerRequestId: "req_d1" });
    mc.observer!.semanticResponseCompleted({ details: { stopReason: "end_turn" } });
    mc.observer!.endLogicalCall("ok");
    harness.flush();

    const durable = rows("mc_d1");
    expect(durable.call!.terminal_status).toBe("ok");
    expect(durable.attempts).toHaveLength(1);
    expect(durable.attempts[0].attempt_visibility).toBe("logical_boundary");
    const kinds = durable.payloads.map((p) => p.kind);
    expect(kinds).toEqual(["semantic_request", "provider_request", "provider_response", "semantic_response"]);
    const providerRequest = durable.payloads.find((p) => p.kind === "provider_request")!;
    expect(providerRequest.visibility).toBe("full");
    expect(providerRequest.provider_request_provenance_json).toBeNull(); // SDK 构造，诚实 null
    const providerResponse = durable.payloads.find((p) => p.kind === "provider_response")!;
    expect(providerResponse.visibility).toBe("metadata_only");
    expect(providerResponse.fidelity).toBe("metadata_only");
    const bodies = payloadBodies(durable.payloads);
    expect(bodies[0].body.systemPrompt).toBe("You are Lingxi.");
    expect(bodies[3].body.text).toBe("Hello!");
  });

  it("MC-02/03/10 Pi wire UNAVAILABLE：显式 unavailable record 持久化后不被升级（§九十九）", () => {
    for (const [callId, subsystem] of [["mc_d2", "compaction"], ["mc_d3", "compaction"], ["mc_d10", "diary"]]) {
      const mc = beginMc(callId, `mt_${callId}`, MODEL_PI, SOURCE(subsystem, "summarize"), {
        attemptVisibility: "logical_boundary",
      });
      mc.session!.captureSemanticRequest({ inputShape: subsystem === "diary" ? "pi_direct_summary" : "chat_context", messages: [] });
      mc.session!.noteProviderWireUnavailable("provider_request", {
        reason: "pi-summarizer-options-no-onPayload",
        visibility: "unavailable",
        fidelity: "opaque",
      });
      mc.session!.noteProviderWireUnavailable("provider_response", {
        reason: "pi-summarizer-options-no-onResponse",
        visibility: "unavailable",
        fidelity: "opaque",
      });
      mc.session!.captureSemanticResponse({ response: { text: "summary", completeness: "complete" } });
      mc.observer!.endLogicalCall("ok");
      harness.flush();

      const durable = rows(callId);
      const providerRecords = durable.payloads.filter((p) => p.kind.startsWith("provider_"));
      expect(providerRecords).toHaveLength(2);
      for (const record of providerRecords) {
        expect(record.visibility).toBe("unavailable");
        expect(record.fidelity).toBe("opaque");
        expect(record.payload_json).toBeNull(); // 无正文，不从 semantic 重建
      }
    }
  });

  it("MC-04 callText：四层 FULL + 四协议 mapping sidecar 持久化（providerRequestProvenance exact）", () => {
    const mc = beginMc("mc_d4", "mt_d4", MODEL_OPENAI, SOURCE("llm", "callText"), {
      attemptVisibility: "exact",
      providerWireVisibility: "request_response",
    });
    mc.session!.captureSemanticRequest({ inputShape: "calltext", systemPrompt: "You are Lingxi." });
    mc.session!.captureProviderRequest({
      attemptId: mc.observer!.currentAttemptId,
      transport: { method: "POST", url: "https://api.openai.com/v1/chat/completions", body: { model: "gpt-x", messages: [{ role: "system", content: "You are Lingxi." }] } },
      fidelity: "runtime_exact",
      provenance: {
        schemaVersion: 1,
        protocol: "openai-completions",
        mappings: [{ semanticSectionOrdinal: 0, providerLocator: { path: ["messages", 0, "content"], span: { start: 0, end: 16 } }, transformation: "moved", mappingPrecision: "exact" }],
      },
    });
    mc.session!.captureProviderResponse({
      attemptId: mc.observer!.currentAttemptId,
      status: 200,
      body: { choices: [{ message: { content: "OK" } }] },
      fidelity: "parsed_equivalent",
    });
    mc.session!.captureSemanticResponse({ response: { text: "OK", completeness: "complete" } });
    mc.observer!.endLogicalCall("ok");
    harness.flush();

    const durable = rows("mc_d4");
    expect(durable.attempts[0].attempt_visibility).toBe("exact");
    expect(durable.attempts[0].provider_wire_visibility).toBe("request_response");
    const providerRequest = durable.payloads.find((p) => p.kind === "provider_request")!;
    const mapping = JSON.parse(providerRequest.provider_request_provenance_json!);
    expect(mapping.protocol).toBe("openai-completions");
    expect(mapping.mappings[0].mappingPrecision).toBe("exact");
    expect(mapping.mappings[0].transformation).toBe("moved");
    expect(providerRequest.visibility).toBe("full");
    expect(providerRequest.fidelity).toBe("runtime_exact");
  });

  it("MC-05 Probe：semantic '.' 允许捕获；成功 response=METADATA_ONLY 持久化", () => {
    const mc = beginMc("mc_d5", "mt_d5", MODEL_PI, SOURCE("providers", "probe"), {
      attemptVisibility: "exact",
    });
    mc.session!.captureSemanticRequest({ inputShape: "provider_probe", messages: [{ role: "user", content: "." }] });
    mc.session!.captureProviderRequest({
      transport: { body: { model: "claude-x", messages: [{ role: "user", content: "." }], max_tokens: 1 } },
      fidelity: "runtime_exact",
    });
    mc.session!.captureProviderResponse({ status: 200, body: null, fidelity: "metadata_only" });
    mc.session!.captureSemanticResponse({ response: { structuredOutput: { ok: true }, completeness: "complete" } });
    mc.observer!.endLogicalCall("ok");
    harness.flush();
    const durable = rows("mc_d5");
    const bodies = payloadBodies(durable.payloads);
    expect(bodies.find((b) => b.kind === "semantic_request")!.body.messages[0].content).toBe(".");
    expect(durable.payloads.find((p) => p.kind === "provider_response")!.visibility).toBe("metadata_only");
    expect(bodies.find((b) => b.kind === "semantic_response")!.body.structuredOutput).toEqual({ ok: true });
  });

  it("MC-06 codex image 401 refresh：1 call / 2 attempts / 2 provider_request(ordinal 1,2) / 2 provider_response(401,200) / 1+1 semantic（§九十七）", () => {
    const mc = beginMc("mc_d6", "mt_d6", MODEL_CODEX, SOURCE("media", "image"), {
      attemptVisibility: "exact",
    });
    mc.session!.captureSemanticRequest({
      inputShape: "media_image",
      parameters: { prompt: "a cat", references: [] },
    });
    // attempt 1: 401
    mc.session!.captureProviderRequest({
      attemptId: mc.observer!.currentAttemptId,
      transport: { body: { prompt: "a cat", tool: "image" } },
      fidelity: "runtime_exact",
    });
    mc.session!.captureProviderResponse({ attemptId: mc.observer!.currentAttemptId, status: 401, body: { error: "unauthorized" }, fidelity: "parsed_equivalent" });
    mc.observer!.attemptError(Object.assign(new Error("401"), { code: "E401" }));
    // attempt 2: credential refresh 后成功
    const attempt2 = mc.observer!.beginAttempt({ details: { attemptVisibility: "exact" } });
    mc.session!.setAttempt(attempt2);
    mc.session!.captureProviderRequest({
      attemptId: attempt2,
      transport: { body: { prompt: "a cat", tool: "image" } },
      fidelity: "runtime_exact",
    });
    mc.session!.captureProviderResponse({ attemptId: attempt2, status: 200, body: { id: "img_1" }, fidelity: "stream_aggregate" });
    mc.session!.captureSemanticResponse({ response: { media: { taskId: "img_1", fileCount: 1 }, completeness: "complete" } });
    mc.observer!.endLogicalCall("ok");
    harness.flush();

    const durable = rows("mc_d6");
    expect(durable.attempts).toHaveLength(2);
    const providerRequests = durable.payloads.filter((p) => p.kind === "provider_request");
    expect(providerRequests.map((p) => p.provider_request_ordinal)).toEqual([1, 2]);
    expect(providerRequests[0].attempt_id).not.toBe(providerRequests[1].attempt_id);
    const providerResponses = durable.payloads.filter((p) => p.kind === "provider_response");
    expect(providerResponses.map((p) => JSON.parse(p.payload_json!).status)).toEqual([401, 200]);
    expect(providerResponses.map((p) => p.provider_request_ordinal)).toEqual([1, 2]);
    expect(durable.payloads.filter((p) => p.kind === "semantic_request")).toHaveLength(1);
    expect(durable.payloads.filter((p) => p.kind === "semantic_response")).toHaveLength(1);
    // attempt 与 provider request 不混淆：A1 error 事实保留，A2 干净。
    const a1 = durable.attempts.find((a) => a.attempt_id === providerRequests[0].attempt_id);
    expect(a1!.error_at).not.toBeNull();
    const a2 = durable.attempts.find((a) => a.attempt_id === providerRequests[1].attempt_id);
    expect(a2!.error_at).toBeNull();
  });

  it("MC-07 Dreamina CLI：provider wire=OPAQUE / fidelity=external_process 持久化不升级（§九十八/九十九）", () => {
    const mc = beginMc("mc_d7", "mt_d7", MODEL_CLI, SOURCE("media", "image"), {
      attemptVisibility: "external_process_boundary",
      providerWireVisibility: "opaque",
    });
    mc.session!.captureSemanticRequest({
      inputShape: "external_cli_media",
      parameters: { prompt: "a cat" },
    });
    mc.session!.noteProviderWireUnavailable("provider_request", {
      reason: "external-cli-wire",
      visibility: "opaque",
      fidelity: "external_process",
    });
    mc.session!.noteProviderWireUnavailable("provider_response", {
      reason: "external-cli-wire",
      visibility: "opaque",
      fidelity: "external_process",
    });
    mc.session!.captureSemanticResponse({ response: { media: { taskId: "dj_1" }, completeness: "complete" } });
    mc.observer!.endLogicalCall("ok");
    harness.flush();

    const durable = rows("mc_d7");
    expect(durable.attempts[0].attempt_visibility).toBe("external_process_boundary");
    expect(durable.attempts[0].provider_wire_visibility).toBe("opaque");
    for (const record of durable.payloads.filter((p) => p.kind.startsWith("provider_"))) {
      expect(record.visibility).toBe("opaque");
      expect(record.fidelity).toBe("external_process");
      expect(record.payload_json).toBeNull();
    }
  });

  it("MC-08 video：semantic/provider/semantic 全 FULL，taskId/deferred 投影", () => {
    const mc = beginMc("mc_d8", "mt_d8", MODEL_OPENAI, SOURCE("media", "video"), { attemptVisibility: "exact" });
    mc.session!.captureSemanticRequest({ inputShape: "media_video", parameters: { prompt: "a sunset", duration: 5 } });
    mc.session!.captureProviderRequest({
      transport: { body: { prompt: "a sunset" } },
      fidelity: "runtime_exact",
    });
    mc.session!.captureProviderResponse({ status: 200, body: { task_id: "vid_9" }, fidelity: "parsed_equivalent" });
    mc.session!.captureSemanticResponse({ response: { media: { taskId: "vid_9", deferred: true }, completeness: "complete" } });
    mc.observer!.endLogicalCall("ok");
    harness.flush();
    const bodies = payloadBodies(rows("mc_d8").payloads);
    expect(bodies.find((b) => b.kind === "semantic_response")!.body.media).toMatchObject({ taskId: "vid_9", deferred: true });
  });

  it("MC-09 speech：audio 经 externalizer 成 stored blob + Volcengine body credential 协议脱敏持久化", () => {
    const mc = beginMc("mc_d9", "mt_d9", MODEL_SPEECH, SOURCE("speech", "transcribe"), { attemptVisibility: "exact" });
    mc.session!.captureSemanticRequest({
      inputShape: "speech_transcribe",
      parameters: { audio: Buffer.from("FAKE_WAV_BYTES_0123456789"), language: "zh" },
    });
    mc.session!.captureProviderRequest({
      transport: {
        body: {
          user: { uid: "TOPSECRET-VOLC-UID-0011223344" },
          audio: { data: "raw-bytes-ref" },
        },
      },
      protocol: "volcengine-bigasr-transcription",
      fidelity: "runtime_exact",
    });
    mc.session!.captureProviderResponse({ status: 200, body: { text: "你好" }, fidelity: "parsed_equivalent" });
    mc.session!.captureSemanticResponse({ response: { transcription: "你好", completeness: "complete" } });
    mc.observer!.endLogicalCall("ok");
    harness.flush();

    const durable = rows("mc_d9");
    const bodies = payloadBodies(durable.payloads);
    // audio → external_blob + stored（blob persistence 开启，Buffer 可同步复制）。
    expect(bodies.find((b) => b.kind === "semantic_request")!.body.parameters.audio).toMatchObject({
      kind: "external_blob",
      captureStatus: "stored",
    });
    // 协议专项：body.user.uid credential 不落盘。
    const providerBody = JSON.stringify(durable.payloads.find((p) => p.kind === "provider_request")!.payload_json!);
    expect(providerBody.includes("TOPSECRET-VOLC-UID")).toBe(false);
    expect(bodies.find((b) => b.kind === "semantic_response")!.body.transcription).toBe("你好");
  });
});

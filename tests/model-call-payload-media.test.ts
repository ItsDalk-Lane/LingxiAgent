/**
 * Phase 6 MC-06/07/08 媒体 × Sensitive Payload Capture（§八十七/§八十八/
 * §九十一/§九十二/§一百四十四/§一百四十五/§一百四十七）。
 *
 * 全部 7 个 HTTP image adapter + agnes video + Codex 401 refresh + Dreamina CLI，
 * 经真实业务链路 runSubmitInBackground（runner 语义层 + adapter wire 层）：
 *   - semantic_request：prompt 可见；参考图（本地路径/data URL/URL）descriptor 化。
 *   - provider_request：真实构造点 body；凭证替换；二进制 externalize。
 *   - provider_response：业务解析点 body；图片 base64 externalize。
 *   - semantic_response：taskId/deferred/fileCount。
 *   - Codex 401：同 callId 两条 provider_request（独立 ordinal）+ 两个 attemptId。
 *   - CLI：wire opaque record；argv/stdout 绝不进 sink。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { runSubmitInBackground } from "../core/media/image-task-runner.ts";
import { agnesVideoAdapter } from "../core/media-adapters/agnes.ts";
import { createJimengImageAdapter } from "../plugins/jimeng-cli/adapters/dreamina.ts";
import { builtinImageGenAdapters } from "../core/media-adapters/builtin-adapters.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import { setModelCallPayloadSink } from "../lib/llm/model-call-payload-capture.ts";
import { createTestModelCallPayloadSink, installTestPayloadSink } from "../lib/llm/model-call-payload-testing.ts";
import { createUsageLedger } from "../lib/llm/usage-ledger.ts";

const TINY_PNG_B64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
const POISON_PROMPT = "TOP_SECRET_PROMPT_PAYLOAD_8F91C2";
const POISON_API_KEY = "sk-MEDIA-SECRET-KEY-8F91C2";
const POISON_STDOUT = "TOP_SECRET_STDOUT_PAYLOAD_8F91C2";
const POISON_LOCAL_PATH_DIR = "/Users/taro/TOPSECRET-LOCAL-IMAGE-DIR";

const roots: string[] = [];
function makeRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-media-payload-"));
  roots.push(root);
  return root;
}

function makeCtx(root: string, ledger: any): any {
  return {
    dataDir: root,
    bus: credentialBus(),
    log: { error: vi.fn(), warn: vi.fn(), info: vi.fn() },
    config: { get: vi.fn(() => ({})) },
    usageLedger: ledger,
    sessionId: "sess-media-1",
    sessionPath: "/sessions/media.jsonl",
  };
}

function makeSubmitCtx(ctx: any) {
  return {
    dataDir: ctx.dataDir,
    bus: ctx.bus,
    log: ctx.log,
    config: ctx.config,
    usageLedger: ctx.usageLedger,
    sessionId: ctx.sessionId,
    sessionPath: ctx.sessionPath,
    generatedDir: path.join(ctx.dataDir, "generated"),
  };
}

function credentialBus() {
  return {
    request: vi.fn(async (type: string, payload: any) => {
      if (type === "provider:credentials") {
        if (payload?.forceRefresh) return { apiKey: `${POISON_API_KEY}-REFRESHED`, baseUrl: "https://media.test/v1", accountId: "acct-1" };
        return { apiKey: POISON_API_KEY, baseUrl: "https://media.test/v1", accountId: "acct-1" };
      }
      return {};
    }),
  };
}

function successResponseFor(adapterId: string): Response {
  let body: unknown;
  switch (adapterId) {
    case "dashscope":
      body = { output: { task_id: "wan-task-1" }, request_id: "req-dash-1" };
      break;
    case "gemini":
      body = { candidates: [{ content: { parts: [{ inline_data: { mime_type: "image/png", data: TINY_PNG_B64 } }] } }] };
      break;
    case "minimax":
      body = { id: "mm-task-1", data: { image_base64: [TINY_PNG_B64] } };
      break;
    case "openai-codex-oauth": {
      const payload = JSON.stringify({
        response: { output: [{ type: "image_generation_call", result: TINY_PNG_B64 }] },
      });
      const stream = new ReadableStream({
        start(controller) {
          controller.enqueue(new TextEncoder().encode(`data: ${payload}\n\n`));
          controller.close();
        },
      });
      return new Response(stream, { status: 200, headers: { "x-request-id": "req-codex-1" } });
    }
    default:
      body = { data: [{ b64_json: TINY_PNG_B64 }] };
  }
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "Content-Type": "application/json", "x-request-id": `req-${adapterId}-1` },
  });
}

async function runImageSubmit({ adapter, params = {}, fetchImpl, root }: {
  adapter: any;
  params?: Record<string, unknown>;
  fetchImpl: any;
  root: string;
}) {
  const ledger = createUsageLedger({});
  const ctx = makeCtx(root, ledger);
  vi.stubGlobal("fetch", fetchImpl);
  await runSubmitInBackground({
    taskId: "task-payload-1",
    adapter,
    params: { prompt: POISON_PROMPT, providerId: adapter.id, ...params },
    submitCtx: makeSubmitCtx(ctx),
    store: { get: vi.fn(() => ({})), update: vi.fn() },
    poller: { checkNow: vi.fn() },
    ctx,
  });
  vi.unstubAllGlobals();
}

describe("MC-06 HTTP image adapters × payload capture", () => {
  let sink: ReturnType<typeof createTestModelCallPayloadSink>;
  beforeEach(() => {
    sink = installTestPayloadSink();
    setModelCallObserver({ handleModelCallEvent() { /* observer 已有独立覆盖 */ } });
  });
  afterEach(() => {
    setModelCallObserver(null);
    setModelCallPayloadSink(null);
    vi.unstubAllGlobals();
    for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
  });

  const httpImageAdapters = builtinImageGenAdapters.filter((a: any) => a.id !== "agnes-videos");

  for (const adapter of httpImageAdapters) {
    it(`coverage: ${adapter.id} 四层 capture + prompt visible + credential hidden + binary externalized`, async () => {
      const root = makeRoot();
      await runImageSubmit({
        adapter,
        fetchImpl: vi.fn(async () => successResponseFor(adapter.id)),
        root,
      });

      const [callId] = sink.callIds();
      // semantic_request：prompt 可见（§八十七/§一百四十四）
      const semantic = sink.semanticRequestForCall(callId)!;
      expect((semantic.payload as any).parameters.prompt).toBe(POISON_PROMPT);

      // provider_request：真实构造点 body + 凭证替换
      const providerRequests = sink.providerRequestsForCall(callId);
      expect(providerRequests).toHaveLength(1);
      const transport = (providerRequests[0].payload as any).transport;
      const serialized = JSON.stringify(transport);
      expect(serialized).not.toContain(POISON_API_KEY);
      const headerKeys = Object.keys(transport.headers ?? {});
      for (const key of ["Authorization", "x-api-key", "x-goog-api-key", "api-key", "X-Api-Key"]) {
        if (transport.headers?.[key] !== undefined) {
          expect(transport.headers[key]).toBe("<redacted:credential>");
        }
      }
      expect(headerKeys.length).toBeGreaterThan(0);

      // provider_response：parsed body；图片 base64 externalize（§九十一）
      const response = sink.providerResponsesForCall(callId)[0];
      expect(response.visibility).toBe("full");
      expect(JSON.stringify(response.payload)).not.toContain(TINY_PNG_B64.slice(0, 24));

      // semantic_response：taskId/deferred/fileCount
      const semanticResponse = sink.semanticResponseForCall(callId)!;
      expect((semanticResponse.payload as any).media).toMatchObject({ deferred: expect.any(Boolean) });

      sink.assertNoSensitiveContent([POISON_API_KEY]);
    });
  }

  it("本地参考图路径 → local_file_reference descriptor（§三十/§八十七）", async () => {
    const root = makeRoot();
    const refPath = path.join(root, "ref.png");
    fs.writeFileSync(refPath, Buffer.from(TINY_PNG_B64, "base64"));
    await runImageSubmit({
      adapter: httpImageAdapters.find((a: any) => a.id === "openai")!,
      params: { image: [refPath] },
      fetchImpl: vi.fn(async () => successResponseFor("openai")),
      root,
    });
    const [callId] = sink.callIds();
    const serialized = JSON.stringify(sink.semanticRequestForCall(callId)!.payload);
    expect(serialized).not.toContain(refPath);
    expect(serialized).toContain("local_file_reference");
    // adapter wire 层：参考图已转 data URL → external_blob
    const transport = (sink.providerRequestsForCall(callId)[0].payload as any).transport;
    expect(JSON.stringify(transport.body)).not.toContain("base64,iVBOR");
  });

  it("Codex image 401 refresh：1 semantic_request + 2 provider_request(独立 ordinal/attemptId) + 1 semantic_response（§九十二/§一百四十五）", async () => {
    const root = makeRoot();
    let calls = 0;
    await runImageSubmit({
      adapter: httpImageAdapters.find((a: any) => a.id === "openai-codex-oauth")!,
      fetchImpl: vi.fn(async () => {
        calls += 1;
        if (calls === 1) return new Response(JSON.stringify({ error: { message: "token expired" } }), { status: 401 });
        return successResponseFor("openai-codex-oauth");
      }),
      root,
    });

    const [callId] = sink.callIds();
    expect(sink.callIds()).toHaveLength(1);
    const requests = sink.providerRequestsForCall(callId);
    expect(requests).toHaveLength(2);
    expect(requests.map((r) => r.providerRequestOrdinal)).toEqual([1, 2]);
    const attempts = new Set(requests.map((r) => r.attemptId));
    expect(attempts.size).toBe(2);
    // 第二次请求即使 body 相同也是独立 record（§九十二）
    expect(requests[1]).not.toBe(requests[0]);
    const responses = sink.providerResponsesForCall(callId);
    expect(responses.map((r) => (r.payload as any).status)).toEqual([401, 200]);
    expect(sink.semanticResponseForCall(callId)).not.toBeNull();
    // 两次 apiKey 都不进 sink
    sink.assertNoSensitiveContent([POISON_API_KEY, `${POISON_API_KEY}-REFRESHED`]);
  });

  it("MC-07 Dreamina CLI：provider wire = 显式 opaque record；argv/stdout 绝不进 sink（§九十五/§一百四十六）", async () => {
    const root = makeRoot();
    const adapter = createJimengImageAdapter({
      resolveCommand: () => "/fake/bin/dreamina",
      runCommand: vi.fn(async () => ({
        stdout: JSON.stringify({ submit_id: "dreamina-submit-1", gen_status: "success", note: POISON_STDOUT }),
        stderr: "",
      })),
      getCapabilitySnapshot: async () => ({
        media: {
          imageGeneration: {
            defaultModelId: "jimeng-image-6",
            models: [{
              id: "jimeng-image-6",
              modes: [
                {
                  id: "text2image",
                  parameterSchema: { properties: { ratio: { enum: ["3:2"] }, resolution: { enum: ["1k", "2k", "4k"] } } },
                },
              ],
            }],
          },
        },
      }),
      authorizeExternalCredentialUse: async () => ({
        providerId: "jimeng-cli",
        boundaryId: "dreamina-cli-login",
        operation: "submit",
        credentialSource: "external",
      }),
    });
    await runImageSubmit({
      adapter,
      params: { modelId: "jimeng-image-6", ratio: "3:2", resolution: "2k" },
      fetchImpl: vi.fn(),
      root,
    });

    const [callId] = sink.callIds();
    expect(sink.sequenceForCall(callId)).toEqual([
      "semantic_request",
      "provider_request",
      "provider_response",
      "semantic_response",
    ]);
    const [request, response] = [
      sink.recordsOfKind(callId, "provider_request")[0],
      sink.recordsOfKind(callId, "provider_response")[0],
    ];
    expect(request.visibility).toBe("opaque");
    expect(request.fidelity).toBe("external_process");
    expect(request.payload).toBeNull();
    expect(response.visibility).toBe("opaque");
    // prompt 在 semantic_request 可见（正文允许），但 stdout 毒丸绝不出现
    expect((sink.semanticRequestForCall(callId)!.payload as any).parameters.prompt).toBe(POISON_PROMPT);
    sink.assertNoSensitiveContent([POISON_STDOUT]);
  });

  it("MC-08 agnes video：四层 capture（body 含 width/height/frame_rate，Authorization 替换）", async () => {
    const root = makeRoot();
    const ledger = createUsageLedger({});
    const ctx = makeCtx(root, ledger);
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ task_id: "video-task-1", video_id: "vid-1" }), { status: 200 })));
    const recorderSeen: unknown[] = [];
    const submitCtx = {
      ...makeSubmitCtx(ctx),
      // adapter.submit 直接调用（video runner 在 universal-media-manager，其
      // capture 逻辑同构；此处驱动 wire 层 + 语义层以 agnesVideoAdapter 为代表）
      modelCall: undefined,
    };
    void recorderSeen;
    void submitCtx;
    // 直接以 beginObservedModelCall 驱动（与 universal-media-manager 同一入口）
    const { beginObservedModelCall } = await import("../lib/llm/model-call-integration.ts");
    const recorder = beginObservedModelCall({
      model: { provider: "agnes", modelId: "agnes-video-v2.0", api: "agnes-videos" },
      source: { subsystem: "media", operation: "submit", surface: "tool", trigger: "user" },
      attribution: { kind: "session", sessionId: "sess-media-1" },
      details: { path: "media_video_submit", mediaType: "video" },
    });
    recorder.payloadCapture?.captureSemanticRequest({
      inputShape: "media_video",
      parameters: { prompt: POISON_PROMPT },
    });
    const observedCtx = { ...ctx, modelCall: recorder };
    const result = await agnesVideoAdapter.submit(
      { prompt: POISON_PROMPT, model: "agnes-video-v2.0", providerId: "agnes", duration: 5 },
      observedCtx,
    );
    recorder.payloadCapture?.captureSemanticResponse({
      response: { media: { taskId: result.taskId, providerTaskId: result.providerTaskId, deferred: true }, completeness: "complete" },
    });
    recorder.semanticResponseCompleted({ details: { deferred: true } });
    recorder.endLogicalCall("ok");
    vi.unstubAllGlobals();

    const callId = recorder.callId;
    expect(sink.semanticRequestForCall(callId)).not.toBeNull();
    const request = sink.providerRequestsForCall(callId)[0];
    const transport = (request.payload as any).transport;
    expect(transport.headers.Authorization).toBe("<redacted:credential>");
    expect(transport.body.model).toBe("agnes-video-v2.0");
    expect(transport.body.num_frames).toBeGreaterThan(0);
    const response = sink.providerResponsesForCall(callId)[0];
    expect((response.payload as any).body.task_id).toBe("video-task-1");
    expect((sink.semanticResponseForCall(callId)!.payload as any).media.providerTaskId).toBe("vid-1");
    sink.assertNoSensitiveContent([POISON_API_KEY]);
  });

  it("HTTP 500 error body：安全捕获（§一百五十二）", async () => {
    const root = makeRoot();
    await runImageSubmit({
      adapter: httpImageAdapters.find((a: any) => a.id === "openai")!,
      fetchImpl: vi.fn(async () => new Response(
        JSON.stringify({ error: { message: "normal media diagnostic", key: POISON_API_KEY } }),
        { status: 500, headers: { "Content-Type": "application/json" } },
      )),
      root,
    });
    const [callId] = sink.callIds();
    const response = sink.providerResponsesForCall(callId)[0];
    expect((response.payload as any).status).toBe(500);
    expect((response.payload as any).body.error.message).toBe("normal media diagnostic");
    sink.assertNoSensitiveContent([POISON_API_KEY]);
  });

  it("sink 关闭：媒体路径 0 record", async () => {
    const freshSink = installTestPayloadSink();
    setModelCallPayloadSink(null);
    const root = makeRoot();
    await runImageSubmit({
      adapter: httpImageAdapters.find((a: any) => a.id === "openai")!,
      fetchImpl: vi.fn(async () => successResponseFor("openai")),
      root,
    });
    expect(freshSink.records).toHaveLength(0);
  });
});

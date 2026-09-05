/**
 * MC-06/MC-07/MC-08 媒体生成 × ModelCallObserver。
 *
 * 覆盖（任务书 §五十九/§六十/§六十一/§六十二）：
 * - MC-06 全部 7 个 builtin HTTP image adapter：submit → 真实 generation HTTP
 *   request → 统一 Observer（1 logical call + exact attempts + ledger 关联）。
 * - OpenAI Codex image 401 credential refresh：同 callId 两个 attemptId（硬验收）。
 * - MC-07 Dreamina CLI：external_process_boundary + opaque，不伪造 provider wire，
 *   毒丸（prompt/本地路径/stdout）不进事件。
 * - MC-08 Agnes video submit：pre-request callId + providerTaskId 后关联；
 *   adapter.query（poll）0 新事件。
 * 全部经真实业务链路 runSubmitInBackground（manager→runner→adapter→fake fetch）。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { runSubmitInBackground } from "../core/media/image-task-runner.ts";
import { agnesVideoAdapter } from "../core/media-adapters/agnes.ts";
import { createJimengImageAdapter } from "../plugins/jimeng-cli/adapters/dreamina.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import { createTestModelCallObserver } from "../lib/llm/model-call-observer-testing.ts";
import { createUsageLedger } from "../lib/llm/usage-ledger.ts";
import {
  builtinImageGenAdapters,
} from "../core/media-adapters/builtin-adapters.ts";

// 1x1 透明 PNG
const TINY_PNG_B64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
const POISON_PROMPT = "TOP_SECRET_PROMPT_8F91C2";
const POISON_STDOUT = "TOP_SECRET_STDOUT_8F91C2";
const POISON_PATH = "/secret/local/path.png";
const POISON_API_KEY = "sk-MEDIA-SECRET-KEY-8F91C2";

const roots: string[] = [];
function makeRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-media-obs-"));
  roots.push(root);
  return root;
}

function makeCtx(root: string, ledger: any): any {
  return {
    dataDir: root,
    mediaExecutionTarget: { credentialProviderId: "agnes" },
    bus: { request: vi.fn(async () => ({})) },
    log: { error: vi.fn(), warn: vi.fn() },
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
    mediaExecutionTarget: ctx.mediaExecutionTarget,
    resolveMediaExecutionTarget: (input: any) => ({
      modelId: input.modelId,
      modality: input.modality,
      runtimeProviderId: input.runtimeProviderId,
      credentialProviderId: input.runtimeProviderId,
      credentialLaneId: null,
      credentialSource: "provider-registry",
      adapterId: input.adapterId,
      resolutionReason: "runtime_provider_credentials",
    }),
  };
}

function makeStore() {
  return { get: vi.fn(() => ({})), update: vi.fn() };
}

/** provider:credentials bus mock——apiKey 携带毒丸，验证不进事件。 */
function credentialBus() {
  return {
    request: vi.fn(async (type: string) => {
      if (type === "provider:credentials") {
        return { apiKey: POISON_API_KEY, baseUrl: "https://media.test/v1", accountId: "acct-1" };
      }
      return {};
    }),
  };
}

/** 每个内置 HTTP image adapter 的成功响应形状。 */
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
  ctx.bus = credentialBus();
  const store = makeStore();
  const poller = { checkNow: vi.fn() };
  vi.stubGlobal("fetch", fetchImpl);
  await runSubmitInBackground({
    taskId: "task-obs-1",
    adapter,
    // 不传 modelId：volcengine/openai 走 MODEL_CATALOG 默认（显式未知模型会
    // 在 fetch 前抛错——那是独立行为，不在本覆盖测试范围）。
    params: {
      prompt: POISON_PROMPT,
      providerId: adapter.id,
      ...params,
    },
    submitCtx: makeSubmitCtx(ctx),
    store,
    poller,
    ctx,
  });
  vi.unstubAllGlobals();
  return { ledger, store, poller };
}

describe("MC-06 builtin HTTP image adapters × ModelCallObserver", () => {
  let observer: ReturnType<typeof createTestModelCallObserver>;

  beforeEach(() => {
    observer = createTestModelCallObserver();
    setModelCallObserver(observer);
  });
  afterEach(() => {
    setModelCallObserver(null);
    vi.unstubAllGlobals();
    for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
  });

  const httpImageAdapters = builtinImageGenAdapters.filter((a: any) => a.id !== "agnes-videos");

  for (const adapter of httpImageAdapters) {
    it(`coverage: ${adapter.id} submit → observer 完整生命周期 + ledger 关联`, async () => {
      const root = makeRoot();
      const { ledger } = await runImageSubmit({
        adapter,
        fetchImpl: vi.fn(async () => successResponseFor(adapter.id)),
        root,
      });

      const callId = observer.callIds()[0];
      expect(callId).toMatch(/^mc_/);
      observer.assertLifecycle(callId, [
        "logical_call_start",
        "attempt_start",
        "provider_request_prepared",
        "provider_response_received",
        "semantic_response_completed",
        "logical_call_end",
      ]);
      const start = observer.eventsOfType("logical_call_start")[0];
      expect(start.details).toMatchObject({
        path: "media_image_submit",
        mediaType: "image",
        taskId: "task-obs-1",
        hasReferenceMedia: false,
        referenceCount: 0,
      });
      expect(start.attribution).toMatchObject({ kind: "session", sessionId: "sess-media-1" });
      const attempt = observer.eventsOfType("attempt_start")[0];
      expect(attempt.details).toMatchObject({ attemptVisibility: "exact", providerWireVisibility: "request_response" });
      expect(observer.eventsOfType("provider_response_received")[0].details).toMatchObject({ httpStatus: 200 });
      expect(observer.events.at(-1)).toMatchObject({ status: "ok" });

      // ledger 关联 + 无双计（§五十/§七十七）
      const entries = ledger.list({}).entries;
      expect(entries).toHaveLength(1);
      expect(entries[0].metadata).toMatchObject({ modelCallId: callId, mediaType: "image", taskId: "task-obs-1" });

      // 毒丸：prompt / apiKey 不进事件
      observer.assertNoSensitiveContent([POISON_PROMPT, POISON_API_KEY]);
    });
  }

  it("reference image 计数进 logical_call_start details（不保存 URL/path）", async () => {
    const root = makeRoot();
    await runImageSubmit({
      adapter: httpImageAdapters.find((a: any) => a.id === "openai")!,
      params: { image: ["https://example.test/ref.png"] },
      fetchImpl: vi.fn(async () => successResponseFor("openai")),
      root,
    });
    const start = observer.eventsOfType("logical_call_start")[0];
    expect(start.details).toMatchObject({ hasReferenceMedia: true, referenceCount: 1 });
    observer.assertNoSensitiveContent(["https://example.test/ref.png"]);
  });

  it("adapter HTTP 500（毒丸 error body）：attempt_error + logical error，正文不泄漏", async () => {
    const root = makeRoot();
    await runImageSubmit({
      adapter: httpImageAdapters.find((a: any) => a.id === "openai")!,
      fetchImpl: vi.fn(async () => new Response(
        JSON.stringify({ error: { message: POISON_STDOUT } }),
        { status: 500, headers: { "Content-Type": "application/json" } },
      )),
      root,
    });
    const callId = observer.callIds()[0];
    observer.assertLifecycle(callId, [
      "logical_call_start",
      "attempt_start",
      "provider_request_prepared",
      "provider_response_received",
      "attempt_error",
      "logical_call_error",
      "logical_call_end",
    ]);
    expect(observer.eventsOfType("provider_response_received")[0].details).toMatchObject({ httpStatus: 500 });
    expect(observer.eventsOfType("attempt_error")[0].details).toMatchObject({ errorKind: "http_error", httpStatus: 500 });
    expect(observer.events.at(-1)).toMatchObject({ status: "error" });
    observer.assertNoSensitiveContent([POISON_STDOUT]);
  });

  it("dashscope 异步任务：deferred=true + providerTaskId 关联（§二十八）", async () => {
    const root = makeRoot();
    await runImageSubmit({
      adapter: httpImageAdapters.find((a: any) => a.id === "dashscope")!,
      fetchImpl: vi.fn(async () => successResponseFor("dashscope")),
      root,
    });
    const semantic = observer.eventsOfType("semantic_response_completed")[0];
    expect(semantic.details).toMatchObject({ deferred: true, providerTaskId: "wan-task-1", fileCount: 0 });
  });
});

describe("MC-06 OpenAI Codex image 401 credential refresh（硬验收 §二十六/§六十）", () => {
  let observer: ReturnType<typeof createTestModelCallObserver>;

  beforeEach(() => {
    observer = createTestModelCallObserver();
    setModelCallObserver(observer);
  });
  afterEach(() => {
    setModelCallObserver(null);
    vi.unstubAllGlobals();
    for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
  });

  function sseImageResponse(): Response {
    const payload = JSON.stringify({
      response: { output: [{ type: "image_generation_call", result: TINY_PNG_B64 }] },
    });
    const stream = new ReadableStream({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(`data: ${payload}\n\n`));
        controller.close();
      },
    });
    return new Response(stream, { status: 200, headers: { "x-request-id": "req-codex-2" } });
  }

  it("401 → credential refresh → 200 = 1 callId + 2 attemptId；refresh 本身 0 logical call", async () => {
    const root = makeRoot();
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({ error: { message: POISON_STDOUT } }), {
        status: 401,
        headers: { "Content-Type": "application/json", "x-request-id": "req-codex-1" },
      }))
      .mockResolvedValueOnce(sseImageResponse());

    const ledger = createUsageLedger({});
    const ctx = makeCtx(root, ledger);
    const credentialCalls: any[] = [];
    ctx.bus = {
      request: vi.fn(async (type: string, payload: any) => {
        if (type === "provider:credentials") {
          credentialCalls.push(payload);
          return { apiKey: POISON_API_KEY, baseUrl: "https://media.test/v1", accountId: "acct-1" };
        }
        return {};
      }),
    };
    const adapter = builtinImageGenAdapters.find((a: any) => a.id === "openai-codex-oauth")!;

    vi.stubGlobal("fetch", fetchMock);
    await runSubmitInBackground({
      taskId: "task-codex-1",
      adapter,
      params: { prompt: POISON_PROMPT, providerId: adapter.id },
      submitCtx: makeSubmitCtx(ctx),
      store: makeStore(),
      poller: { checkNow: vi.fn() },
      ctx,
    });
    vi.unstubAllGlobals();

    // 事件序列精确匹配 §二十六
    const callIds = observer.callIds();
    expect(callIds).toHaveLength(1);
    const callId = callIds[0];
    observer.assertLifecycle(callId, [
      "logical_call_start",
      "attempt_start",
      "provider_request_prepared",
      "provider_response_received",
      "attempt_error",        // attempt A：401
      "attempt_start",        // attempt B：刷新凭证后重发
      "provider_request_prepared",
      "provider_response_received",
      "semantic_response_completed",
      "logical_call_end",
    ]);

    const attempts = observer.attemptsForCall(callId);
    expect(attempts).toHaveLength(2);
    expect(attempts[0]).not.toBe(attempts[1]);

    const responses = observer.eventsOfType("provider_response_received");
    expect(responses[0].details).toMatchObject({ httpStatus: 401 });
    expect(responses[0].providerRequestId).toBe("req-codex-1");
    expect(responses[0].attemptId).toBe(attempts[0]);
    expect(responses[1].details).toMatchObject({ httpStatus: 200 });
    expect(responses[1].providerRequestId).toBe("req-codex-2");
    expect(responses[1].attemptId).toBe(attempts[1]);

    expect(observer.eventsOfType("logical_call_start")).toHaveLength(1);
    expect(observer.events.at(-1)).toMatchObject({ eventType: "logical_call_end", status: "ok" });

    // credential refresh：第二次 provider:credentials 带 forceRefresh——控制面，
    // 不产生 ModelCall 事件（callIds 只有 1 个即证明），也不双计 ledger。
    expect(credentialCalls).toHaveLength(2);
    expect(credentialCalls[1]).toMatchObject({ forceRefresh: true });
    const entries = ledger.list({}).entries;
    expect(entries).toHaveLength(1);
    expect(entries[0].metadata).toMatchObject({ modelCallId: callId });

    observer.assertNoSensitiveContent([POISON_PROMPT, POISON_API_KEY, POISON_STDOUT]);
  });
});

describe("MC-07 Dreamina CLI × ModelCallObserver（opaque boundary）", () => {
  let observer: ReturnType<typeof createTestModelCallObserver>;

  beforeEach(() => {
    observer = createTestModelCallObserver();
    setModelCallObserver(observer);
  });
  afterEach(() => {
    setModelCallObserver(null);
    for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
  });

  function makeDreaminaAdapter(runCommand: any) {
    const mode = {
      id: "text2image",
      parameterSchema: {
        properties: {
          ratio: { enum: ["3:2"] },
          resolution: { enum: ["1k", "2k", "4k"] },
        },
      },
      defaults: { ratio: "3:2", resolution: "2k" },
      inputLimits: { referenceImages: { max: 10 } },
    };
    return createJimengImageAdapter({
      resolveCommand: () => "/fake/bin/dreamina",
      runCommand,
      getCapabilitySnapshot: async () => ({
        media: {
          imageGeneration: {
            defaultModelId: "jimeng-image-6",
            models: [{
              id: "jimeng-image-6",
              modes: [mode, { ...mode, id: "image2image" }],
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
  }

  it("CLI submit：external_process_boundary + opaque，不伪造 provider wire，毒丸不泄漏（§六十一）", async () => {
    const root = makeRoot();
    const ledger = createUsageLedger({});
    const ctx = makeCtx(root, ledger);
    const adapter = makeDreaminaAdapter(vi.fn(async (_cmd: string, args: any[]) => {
      // 毒丸同时进入 args（prompt/path）与 stdout
      expect(args).toContain(POISON_PROMPT);
      return { stdout: JSON.stringify({ submit_id: "dreamina-task-1", gen_status: "success", note: POISON_STDOUT }), stderr: "" };
    }));

    await runSubmitInBackground({
      taskId: "task-cli-1",
      adapter,
      params: { prompt: POISON_PROMPT, image: [POISON_PATH], providerId: adapter.id, modelId: "jimeng-image-6" },
      submitCtx: makeSubmitCtx(ctx),
      store: makeStore(),
      poller: { checkNow: vi.fn() },
      ctx,
    });

    const callId = observer.callIds()[0];
    observer.assertLifecycle(callId, [
      "logical_call_start",
      "attempt_start",
      "semantic_response_completed",
      "logical_call_end",
    ]);
    const attempt = observer.eventsOfType("attempt_start")[0];
    expect(attempt.details).toMatchObject({
      attemptVisibility: "external_process_boundary",
      providerWireVisibility: "opaque",
      adapterId: "jimeng-cli-images",
      mediaType: "image",
    });
    // 不伪造 provider wire（§三十）
    expect(observer.eventsOfType("provider_request_prepared")).toHaveLength(0);
    expect(observer.eventsOfType("provider_response_received")).toHaveLength(0);
    // submitId 是安全 ID，可以进 semantic details
    expect(observer.eventsOfType("semantic_response_completed")[0].details)
      .toMatchObject({ deferred: true, providerTaskId: "dreamina-task-1", fileCount: 0 });
    expect(observer.events.at(-1)).toMatchObject({ status: "ok" });
    // ledger 关联
    expect(ledger.list({}).entries[0].metadata).toMatchObject({ modelCallId: callId });
    // 毒丸：prompt / 本地路径 / stdout 都不进事件
    observer.assertNoSensitiveContent([POISON_PROMPT, POISON_PATH, POISON_STDOUT]);
  });

  it("CLI 进程失败：attempt_error(external_process) + logical error（CLI 毒丸 stderr 不泄漏）", async () => {
    const root = makeRoot();
    const ledger = createUsageLedger({});
    const ctx = makeCtx(root, ledger);
    const adapter = makeDreaminaAdapter(vi.fn(async () => {
      const err: any = new Error(`dreamina failed: ${POISON_STDOUT}`);
      err.stderr = POISON_STDOUT;
      throw err;
    }));

    await runSubmitInBackground({
      taskId: "task-cli-2",
      adapter,
      params: { prompt: POISON_PROMPT, providerId: adapter.id, modelId: "jimeng-image-6" },
      submitCtx: makeSubmitCtx(ctx),
      store: makeStore(),
      poller: { checkNow: vi.fn() },
      ctx,
    });

    const callId = observer.callIds()[0];
    observer.assertLifecycle(callId, [
      "logical_call_start",
      "attempt_start",
      "attempt_error",
      "logical_call_error",
      "logical_call_end",
    ]);
    expect(observer.eventsOfType("attempt_error")[0].details).toMatchObject({ errorKind: "external_process" });
    expect(observer.events.at(-1)).toMatchObject({ status: "error" });
    observer.assertNoSensitiveContent([POISON_STDOUT]);
    expect(ledger.list({}).entries[0].status).toBe("error");
  });
});

describe("MC-08 Agnes video submit × ModelCallObserver", () => {
  let observer: ReturnType<typeof createTestModelCallObserver>;

  beforeEach(() => {
    observer = createTestModelCallObserver();
    setModelCallObserver(observer);
  });
  afterEach(() => {
    setModelCallObserver(null);
    vi.unstubAllGlobals();
    for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
  });

  it("video submit：pre-request callId → exact HTTP attempt → providerTaskId → semantic 完成", async () => {
    const root = makeRoot();
    const ledger = createUsageLedger({});
    const ctx = makeCtx(root, ledger);
    ctx.bus = credentialBus();
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({
      task_id: "agnes-video-task-1",
      video_id: "vid_abc123",
    }), { status: 200, headers: { "x-request-id": "req-agnes-video-1" } }));
    vi.stubGlobal("fetch", fetchMock);

    // 直接驱动 adapter 层（业务边界 submitVideo 的下游）——callId 由边界注入 ctx。
    const { beginObservedModelCall } = await import("../lib/llm/model-call-integration.ts");
    const recorder = beginObservedModelCall({
      model: { provider: "agnes", modelId: "agnes-video-v2.0", api: "agnes-videos" },
      source: { subsystem: "media", operation: "submit", surface: "tool", trigger: "user" },
      attribution: { kind: "session", sessionId: "sess-media-1" },
      details: { path: "media_video_submit", mediaType: "video", asyncTask: true },
    });
    const result = await agnesVideoAdapter.submit(
      { prompt: POISON_PROMPT, duration: 5, modelId: "agnes-video-v2.0", providerId: "agnes" },
      { ...makeSubmitCtx(ctx), modelCall: recorder },
    );
    recorder.semanticResponseCompleted({
      details: { deferred: true, providerTaskId: result.providerTaskId ?? null },
    });
    recorder.endLogicalCall("ok");
    vi.unstubAllGlobals();

    expect(result).toMatchObject({ taskId: "agnes-video-task-1", providerTaskId: "vid_abc123" });
    const callId = observer.callIds()[0];
    observer.assertLifecycle(callId, [
      "logical_call_start",
      "attempt_start",
      "provider_request_prepared",
      "provider_response_received",
      "semantic_response_completed",
      "logical_call_end",
    ]);
    const prepared = observer.eventsOfType("provider_request_prepared")[0];
    expect(prepared.details).toMatchObject({
      protocol: "agnes-videos",
      mediaType: "video",
      asyncTask: true,
      durationConfigured: true,
      resolutionConfigured: false,
      fpsConfigured: false,
    });
    expect(observer.eventsOfType("provider_response_received")[0])
      .toMatchObject({ providerRequestId: "req-agnes-video-1" });
    expect(observer.eventsOfType("semantic_response_completed")[0].details)
      .toMatchObject({ deferred: true, providerTaskId: "vid_abc123" });
    expect(observer.events.at(-1)).toMatchObject({ status: "ok" });
    observer.assertNoSensitiveContent([POISON_PROMPT, POISON_API_KEY]);
  });

  it("adapter.query（poll）：不产生任何新 Model Call 事件（§六十二）", async () => {
    const root = makeRoot();
    const ctx = makeCtx(root, createUsageLedger({}));
    ctx.bus = credentialBus();
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({
      status: "pending",
    }), { status: 200 })));

    await agnesVideoAdapter.query("vid_abc123", { ...makeSubmitCtx(ctx), task: { modelId: "agnes-video-v2.0" } });
    vi.unstubAllGlobals();

    expect(observer.events).toHaveLength(0);
  });
});

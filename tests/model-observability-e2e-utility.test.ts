/**
 * Phase 10 E2E Truth — MC-10 diary / MC-07 CLI / MC-08 video / 错误矩阵
 * （S15/S16/S18/S19）。
 *
 * S18：同一 diary trace 内 N 临时摘要（MC-10）+ 终稿（MC-04）——same traceId、
 *      parent 语义按实际因果（任务根直接触发 → parent=null），不为画树伪造边。
 * S15：外部 CLI = external_process_boundary + OPAQUE；argv/stdout 毒丸不进 payload。
 * S16：video submit 是 Model Call；poll（adapter.query）0 新事件。
 * S19：HTTP 429/500、invalid JSON、timeout（hang）、connection reset、user abort
 *      ——错误终态正确、safe error、abort ≠ error。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { callText } from "../core/llm-client.ts";
import { createApprovalGateway, createModelApprovalReviewer } from "../lib/approval-gateway.ts";
import { generateDiaryCompactionSummary } from "../lib/diary/diary-writer.ts";
import { runWithNewModelTrace } from "../lib/llm/model-trace-scope.ts";
import { compileToday } from "../lib/memory/compile.ts";
import { getLogicalDay } from "../lib/time-utils.ts";
import { createJimengImageAdapter } from "../plugins/jimeng-cli/adapters/dreamina.ts";
import { agnesVideoAdapter } from "../core/media-adapters/agnes.ts";
import { beginObservedModelCall } from "../lib/llm/model-call-integration.ts";
import { runSubmitInBackground } from "../core/media/image-task-runner.ts";
import { createUsageLedger } from "../lib/llm/usage-ledger.ts";
import {
  createScenarioHarness,
  flushAsync,
  openaiCompletionsJson,
  openaiCompletionsSseBody,
  type ScenarioHarness,
} from "./helpers/model-observability-scenario-harness.ts";

const POISON_KEY = "sk-E2E-UTILITY-WITNESS-POISON-51ba7c2e9d";

let harness: ScenarioHarness;
const roots: string[] = [];

beforeEach(async () => {
  harness = await createScenarioHarness();
});
afterEach(async () => {
  await harness.close();
  harness.cleanup();
  vi.unstubAllGlobals();
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

function makeRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-e2e-util-"));
  roots.push(root);
  return root;
}

function diaryModel() {
  return {
    id: "witness-model",
    provider: "witness-provider",
    api: "openai-completions",
    baseUrl: `${harness.witness.baseUrl}/v1`,
    maxTokens: 8192,
    input: ["text"],
    cost: { input: 1, output: 2, cacheRead: 0.1, cacheWrite: 1.25, total: 0 },
  };
}

function semanticProvenanceCategories(callId: string): string[] {
  const query = harness.query();
  const detail = query.queryCallDetail(callId);
  expect(detail.ok).toBe(true);
  if (!detail.ok) return [];
  const semanticRequest = detail.value.payloadRecords.find((record: any) => record.kind === "semantic_request");
  expect(semanticRequest).toBeTruthy();
  if (!semanticRequest) return [];
  const payload = query.getPayloadRecord(semanticRequest.id);
  expect(payload.ok).toBe(true);
  if (!payload.ok) return [];
  expect(payload.value.semanticInputProvenanceState).toBe("present");
  return ((payload.value.semanticInputProvenance as any)?.sections ?? [])
    .map((section: any) => String(section.category));
}

describe("E2E truth — approval format repair（S9）", () => {
  it("两次真实请求分别落成调用，只有修复请求带 format_constraint", async () => {
    const ledger = harness.createLedger();
    harness.witness.scriptNext(
      { kind: "json", body: openaiCompletionsJson({ content: "not-json" }) },
      {
        kind: "json",
        body: openaiCompletionsJson({
          content: JSON.stringify({
            verdict: "authorized",
            scopeRelation: "exact",
            evidenceIds: ["u0"],
            reason: "范围一致",
          }),
        }),
      },
    );
    const reviewer = createModelApprovalReviewer({
      resolveApprovalModel: async () => ({
        api: "openai-completions",
        apiKey: POISON_KEY,
        baseUrl: harness.witness.baseUrl,
        headers: {},
        model: { id: "witness-model", provider: "witness-provider" },
      }),
      callText,
      getUsageLedger: () => ledger,
    });
    const gateway = createApprovalGateway({ intentAuthorizationReviewer: reviewer });

    const decision = await gateway.review({
      id: "approval-e2e-repair",
      kind: "tool_action",
      sessionPath: "/sessions/approval-e2e.jsonl",
      agentId: "agent-e2e",
      toolName: "write",
      actionName: "execute",
      params: { path: "notes.md" },
      target: { type: "file", label: "notes.md" },
      blastRadius: "workspace",
      reversibility: "easy",
    });
    await flushAsync(5);
    harness.flush();
    await flushAsync(3);

    expect(decision.action).toBe("allow");
    expect(harness.witness.requestCount()).toBe(2);
    const callIds = harness.observer!.callIds();
    expect(callIds).toHaveLength(2);
    expect(new Set(callIds).size).toBe(2);
    for (const callId of callIds) {
      const detail = harness.query().queryCallDetail(callId);
      expect(detail.ok).toBe(true);
      if (detail.ok) {
        expect(detail.value.call.source).toMatchObject({
          subsystem: "approval",
          operation: "review_authorization",
        });
        expect(detail.value.call.terminalStatus).toBe("ok");
        expect(detail.value.call.usage.availability).toBe("present");
      }
    }
    expect(semanticProvenanceCategories(callIds[0])).not.toContain("format_constraint");
    expect(semanticProvenanceCategories(callIds[1])).toContain("format_constraint");
    harness.observer!.assertNoSensitiveContent([POISON_KEY]);
  });
});

describe("E2E truth — memory representative prompt（S8）", () => {
  it("compileToday 生产提示经真实请求、持久化与查询保持 task_input 来源", async () => {
    const ledger = harness.createLedger();
    const root = makeRoot();
    const todayPath = path.join(root, "today.md");
    const logicalDate = getLogicalDay().logicalDate;
    const eventText = "E2E_MEMORY_EVENT 用户要求保留真实记忆证据";
    harness.witness.scriptNext({
      kind: "json",
      body: openaiCompletionsJson({ content: "E2E_MEMORY_COMPILED" }),
    });
    const summaryManager = {
      getAllSummaries: vi.fn(() => [{
        session_id: "memory-e2e",
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
        summary: `## 事情经过\n- ${logicalDate} 10:00 ${eventText}`,
      }]),
    };

    await compileToday(summaryManager, todayPath, {
      api: "openai-completions",
      apiKey: POISON_KEY,
      baseUrl: harness.witness.baseUrl,
      model: { id: "witness-model", provider: "witness-provider", maxTokens: 8192 },
      usageLedger: ledger,
      usageAgentId: "agent-e2e",
    });
    await flushAsync(5);
    harness.flush();
    await flushAsync(3);

    expect(fs.readFileSync(todayPath, "utf8")).toBe("E2E_MEMORY_COMPILED");
    expect(harness.witness.requestCount()).toBe(1);
    expect(harness.witness.lastRequest()?.bodyText).toContain(eventText);
    const callIds = harness.observer!.callIds();
    expect(callIds).toHaveLength(1);
    const detail = harness.query().queryCallDetail(callIds[0]);
    expect(detail.ok).toBe(true);
    if (detail.ok) {
      expect(detail.value.call.source).toMatchObject({
        subsystem: "memory",
        operation: "compile_today",
      });
      expect(detail.value.call.usage.availability).toBe("present");
    }
    expect(semanticProvenanceCategories(callIds[0])).toEqual(
      expect.arrayContaining(["task_instruction", "task_input"]),
    );
    harness.observer!.assertNoSensitiveContent([POISON_KEY]);
  });
});

describe("E2E truth — MC-10 diary（S18）", () => {
  it("同一 diary trace：2 临时摘要（MC-10）+ 终稿（MC-04）同 trace、parent=null、凭证不入库", async () => {
    const ledger = harness.createLedger();
    // 3 次生成请求：2 临时摘要（Pi completeSimple SSE）+ 1 终稿（callText JSON）
    harness.witness.scriptNext(
      { kind: "sse", body: openaiCompletionsSseBody({ content: "E2E_DIARY_SUMMARY_1" }) },
      { kind: "sse", body: openaiCompletionsSseBody({ content: "E2E_DIARY_SUMMARY_2" }) },
      { kind: "json", body: openaiCompletionsJson({ content: "E2E_DIARY_FINAL" }) },
    );

    await runWithNewModelTrace({ origin: "diary" }, async () => {
      await generateDiaryCompactionSummary({
        messages: [{ role: "user", content: [{ type: "text", text: "a" }], timestamp: 1 }] as any,
        model: diaryModel() as any,
        apiKey: POISON_KEY,
        headers: undefined,
        usageLedger: ledger,
        agentId: "agent-e2e",
      });
      await generateDiaryCompactionSummary({
        messages: [{ role: "user", content: [{ type: "text", text: "b" }], timestamp: 2 }] as any,
        model: diaryModel() as any,
        apiKey: POISON_KEY,
        headers: undefined,
        usageLedger: ledger,
        agentId: "agent-e2e",
      });
      // 终稿：diary-writer.ts:722 的 callText（operation diary_write）
      await callText({
        api: "openai-completions",
        apiKey: POISON_KEY,
        baseUrl: harness.witness.baseUrl,
        model: { id: "witness-model", provider: "witness-provider" } as any,
        systemPrompt: "E2E_DIARY_FINAL_SYSTEM",
        messages: [{ role: "user", content: "E2E_DIARY_FINAL_INPUT" }],
        usageLedger: ledger,
        usageContext: {
          source: { subsystem: "memory", operation: "diary_write", surface: "background", trigger: "system" },
          attribution: { kind: "agent", agentId: "agent-e2e" },
        },
      } as any);
    });
    await flushAsync(5);
    harness.flush();
    await flushAsync(3);

    expect(harness.witness.requestCount()).toBe(3);
    // 凭证经 query/header 双通道可见（witness=raw truth）
    for (const req of harness.witness.requests()) {
      expect(req.headers["authorization"]).toContain(POISON_KEY);
    }

    const callIds = harness.observer!.callIds();
    expect(callIds).toHaveLength(3);
    const identities = callIds.map((id) => harness.observer!.callIdentity(id)!);
    // 同一任务 trace：三个 call 共享 traceId
    const traceIds = new Set(identities.map((identity) => identity.traceId));
    expect(traceIds.size).toBe(1);
    // 任务根直接触发：parent=null（不伪造边，§五十四）
    for (const identity of identities) {
      expect(identity.parentCallId).toBeNull();
    }
    harness.observer!.assertTraceGraphValid();

    // MC-10 两个临时摘要：provider wire unavailable（诚实缺失）
    const query = harness.query();
    for (const callId of callIds.slice(0, 2)) {
      const detail = query.queryCallDetail(callId);
      expect(detail.ok).toBe(true);
      if (!detail.ok) continue;
      const providerReq = detail.value.payloadRecords.find((r: any) => r.kind === "provider_request");
      const record = providerReq ? query.getPayloadRecord(providerReq.id) : null;
      if (record?.ok) {
        expect(record.value.visibility).toBe("unavailable");
        expect(record.value.payload).toBeNull();
      }
    }
    // 终稿（MC-04）provider_request = FULL
    const finalDetail = query.queryCallDetail(callIds[2]);
    expect(finalDetail.ok).toBe(true);
    if (finalDetail.ok) {
      const providerReq = finalDetail.value.payloadRecords.find((r: any) => r.kind === "provider_request");
      const record = providerReq ? query.getPayloadRecord(providerReq.id) : null;
      if (record?.ok) {
        expect(record.value.visibility).toBe("full");
        const body = (record.value.payload as any).transport.body;
        expect(JSON.stringify(body)).toContain("E2E_DIARY_FINAL_INPUT");
      }
    }

    harness.observer!.assertNoSensitiveContent([POISON_KEY]);
  }, 20_000);
});

describe("E2E truth — MC-07 CLI（S15）", () => {
  it("fake executable：external_process/OPAQUE；argv+stdout 毒丸不进 payload；UI 侧 OPAQUE 可见", async () => {
    const root = makeRoot();
    const ledger = createUsageLedger({});
    const POISON_STDOUT = "E2E_CLI_STDOUT_POISON_x9";
    const resolveMediaExecutionTarget = (input: {
      modelId: string;
      modality: "image";
      runtimeProviderId: string;
      adapterId: string | null;
    }) => ({
      modelId: input.modelId,
      modality: input.modality,
      runtimeProviderId: input.runtimeProviderId,
      credentialProviderId: input.runtimeProviderId,
      credentialLaneId: null,
      credentialSource: "external",
      adapterId: input.adapterId,
      resolutionReason: "runtime_provider_credentials",
    });
    const adapter = createJimengImageAdapter({
      resolveCommand: () => "/fake/bin/dreamina",
      runCommand: vi.fn(async (_cmd: string, args: any[]) => {
        expect(args).toContain("E2E_CLI_POISON_PROMPT");
        return { stdout: JSON.stringify({ submit_id: "dreamina-e2e-1", gen_status: "success", note: POISON_STDOUT }), stderr: "" };
      }),
      getCapabilitySnapshot: async () => ({
        media: {
          imageGeneration: {
            defaultModelId: "jimeng-image-6",
            models: [{ id: "jimeng-image-6", modes: [{ id: "text2image" }, { id: "image2image" }] }],
          },
        },
      }),
      authorizeExternalCredentialUse: async () => ({
        providerId: "jimeng-cli",
        boundaryId: "dreamina-cli-login",
        operation: "submit",
        credentialSource: "external",
      }),
    } as any);

    await runSubmitInBackground({
      taskId: "task-cli-e2e",
      adapter,
      params: { prompt: "E2E_CLI_POISON_PROMPT", providerId: "jimeng-cli" },
      submitCtx: {
        dataDir: root,
        bus: { request: vi.fn(async () => ({})) },
        log: { error: vi.fn(), warn: vi.fn() },
        config: { get: vi.fn(() => ({})) },
        usageLedger: ledger,
        sessionId: "sess-cli-e2e",
        sessionPath: "/sessions/cli.jsonl",
        generatedDir: path.join(root, "generated"),
        resolveMediaExecutionTarget,
      },
      store: { get: vi.fn(() => ({})), update: vi.fn() },
      poller: { checkNow: vi.fn() },
      ctx: { dataDir: root, bus: { request: vi.fn(async () => ({})) }, log: { error: vi.fn(), warn: vi.fn() }, config: { get: vi.fn(() => ({})) }, usageLedger: ledger, sessionId: "sess-cli-e2e", sessionPath: "/sessions/cli.jsonl" },
    } as any);
    await flushAsync(4);
    harness.flush();

    // CLI：0 HTTP 到 witness（外部进程边界）
    expect(harness.witness.requestCount()).toBe(0);

    const callId = harness.observer!.callIds()[0];
    const query = harness.query();
    const detail = query.queryCallDetail(callId);
    expect(detail.ok).toBe(true);
    if (!detail.ok) return;
    const providerReq = detail.value.payloadRecords.find((r: any) => r.kind === "provider_request")!;
    const record = query.getPayloadRecord(providerReq.id);
    expect(record.ok).toBe(true);
    if (record.ok) {
      // OPAQUE 不升级（§十八）：stored/查询均 opaque，正文 null
      expect(record.value.visibility).toBe("opaque");
      expect(record.value.payload).toBeNull();
    }
    harness.observer!.assertNoSensitiveContent(["E2E_CLI_POISON_PROMPT", POISON_STDOUT]);
  });
});

describe("E2E truth — MC-08 video（S16）", () => {
  it("agnes submit 是 Model Call（witness 真实 HTTP）；poll 0 新事件", async () => {
    const root = makeRoot();
    const ledger = harness.createLedger();
    harness.witness.scriptNext({ kind: "json", body: { task_id: "vid-e2e-1" } });

    const recorder = beginObservedModelCall({
      model: { provider: "agnes", modelId: "agnes-video-v2.0", api: "agnes-videos" },
      source: { subsystem: "media", operation: "submit", surface: "tool", trigger: "user" },
      attribution: { kind: "session", sessionId: "sess-video-e2e" },
      details: { path: "media_video_submit", mediaType: "video", asyncTask: true },
    });
    const submitCtx = {
      dataDir: root,
      bus: { request: vi.fn(async (type: string) => type === "provider:credentials" ? { apiKey: POISON_KEY, baseUrl: harness.witness.baseUrl } : {}) },
      log: { error: vi.fn(), warn: vi.fn() },
      config: { get: vi.fn(() => ({})) },
      usageLedger: ledger,
      sessionId: "sess-video-e2e",
      sessionPath: "/sessions/video.jsonl",
      mediaExecutionTarget: {
        modelId: "agnes-video-v2.0",
        modality: "video",
        runtimeProviderId: "agnes",
        credentialProviderId: "agnes",
        credentialLaneId: null,
        credentialSource: "provider-registry",
        adapterId: "agnes-videos",
        resolutionReason: "runtime_provider_credentials",
      },
    };
    const result = await agnesVideoAdapter.submit(
      { prompt: "E2E_VIDEO_PROMPT", duration: 5, modelId: "agnes-video-v2.0", providerId: "agnes" },
      { ...submitCtx, modelCall: recorder },
    );
    recorder.semanticResponseCompleted({ details: { deferred: true, providerTaskId: result.providerTaskId ?? null } });
    recorder.endLogicalCall("ok");
    await flushAsync(4);
    harness.flush();

    expect(result.providerTaskId).toBe("vid-e2e-1");
    expect(harness.witness.requestCount()).toBe(1);
    expect(harness.witness.requests()[0].headers["authorization"]).toContain(POISON_KEY);

    const callId = harness.observer!.callIds()[0];
    const query = harness.query();
    const detail = query.queryCallDetail(callId);
    expect(detail.ok).toBe(true);
    if (detail.ok) {
      const providerReq = detail.value.payloadRecords.find((r: any) => r.kind === "provider_request")!;
      const record = query.getPayloadRecord(providerReq.id);
      if (record.ok) {
        const body = (record.value.payload as any).transport.body;
        expect(body.prompt).toBe("E2E_VIDEO_PROMPT");
      }
    }

    /* poll：控制面，0 新 Model Call */
    const callCountBefore = harness.observer!.callIds().length;
    harness.witness.scriptNext({ kind: "json", body: { status: "pending" } });
    await agnesVideoAdapter.query("vid-e2e-1", { ...submitCtx, task: { modelId: "agnes-video-v2.0" } } as any);
    await flushAsync(3);
    expect(harness.observer!.callIds().length).toBe(callCountBefore);
    harness.observer!.assertNoSensitiveContent([POISON_KEY]);
  });
});

describe("E2E truth — 错误矩阵（S19）", () => {
  const CASES = [
    { name: "HTTP 429", script: { kind: "json", body: { error: { message: "E2E_POISON_ERR_BODY_429" } }, status: 429 }, expectStatus: "error" },
    { name: "HTTP 500", script: { kind: "json", body: { error: { message: "E2E_POISON_ERR_BODY_500" } }, status: 500 }, expectStatus: "error" },
    { name: "invalid JSON 200", script: { kind: "text", body: "not-json{{", status: 200, contentType: "application/json" }, expectStatus: "error" },
  ] as const;

  for (const testCase of CASES) {
    it(`${testCase.name}：错误终态 + safe error + 毒丸错误正文不入库`, async () => {
      harness.witness.scriptNext(testCase.script as any);
      await expect(callText({
        api: "openai-completions",
        apiKey: POISON_KEY,
        baseUrl: harness.witness.baseUrl,
        model: { id: "witness-model", provider: "witness-provider" } as any,
        systemPrompt: "S",
        messages: [{ role: "user", content: "U" }],
        timeoutMs: 4000,
      } as any)).rejects.toThrow();
      await flushAsync(3);
      harness.flush();

      const callId = harness.observer!.callIds()[0];
      const end = harness.observer!.eventsOfType("logical_call_end").at(-1);
      expect(end?.status).toBe(testCase.expectStatus);
      // safe error：事件不携带 provider 错误正文
      harness.observer!.assertNoSensitiveContent(["E2E_POISON_ERR_BODY"]);
      // durable 终态一致
      const query = harness.query();
      const detail = query.queryCallDetail(callId);
      expect(detail.ok).toBe(true);
      if (detail.ok) {
        expect(detail.value.call.terminalStatus).toBe(testCase.expectStatus);
        // 错误正文不进 durable DTO（safe error contract；毒丸零出现）
        expect(JSON.stringify(detail.value)).not.toContain("E2E_POISON_ERR_BODY");
      }
    });
  }

  it("timeout（hang）：LLM_TIMEOUT error 终态", async () => {
    harness.witness.scriptNext({ kind: "hang" });
    await expect(callText({
      api: "openai-completions",
      apiKey: POISON_KEY,
      baseUrl: harness.witness.baseUrl,
      model: { id: "witness-model", provider: "witness-provider" } as any,
      systemPrompt: "S",
      messages: [{ role: "user", content: "U" }],
      timeoutMs: 300,
    } as any)).rejects.toThrow();
    await flushAsync(3);
    harness.flush();
    const end = harness.observer!.eventsOfType("logical_call_end").at(-1);
    expect(end?.status).toBe("error");
  }, 15_000);

  it("connection reset：error 终态（不 crash observer）", async () => {
    harness.witness.scriptNext({ kind: "reset" });
    await expect(callText({
      api: "openai-completions",
      apiKey: POISON_KEY,
      baseUrl: harness.witness.baseUrl,
      model: { id: "witness-model", provider: "witness-provider" } as any,
      systemPrompt: "S",
      messages: [{ role: "user", content: "U" }],
      timeoutMs: 5000,
    } as any)).rejects.toThrow();
    await flushAsync(3);
    harness.flush();
    const end = harness.observer!.eventsOfType("logical_call_end").at(-1);
    expect(end?.status).toBe("error");
    harness.observer!.assertTraceGraphValid();
  });

  it("user abort：aborted ≠ error（§五十七）", async () => {
    harness.witness.scriptNext({ kind: "hang" });
    const controller = new AbortController();
    const promise = callText({
      api: "openai-completions",
      apiKey: POISON_KEY,
      baseUrl: harness.witness.baseUrl,
      model: { id: "witness-model", provider: "witness-provider" } as any,
      systemPrompt: "S",
      messages: [{ role: "user", content: "U" }],
      timeoutMs: 10_000,
      signal: controller.signal,
    } as any);
    setTimeout(() => controller.abort(), 100);
    await expect(promise).rejects.toThrow();
    await flushAsync(3);
    harness.flush();
    const end = harness.observer!.eventsOfType("logical_call_end").at(-1);
    expect(end?.status).toBe("aborted");
    const query = harness.query();
    const callId = harness.observer!.callIds()[0];
    const detail = query.queryCallDetail(callId);
    expect(detail.ok).toBe(true);
    if (detail.ok) expect(detail.value.call.terminalStatus).toBe("aborted");
  }, 15_000);
});

/**
 * R00-T05 / R00-A09｜旧系统行为夹具回放（确定性、断网、临时目录隔离）。
 *
 * 回放入口全部为真实旧实现（生产路由 / 生产模块 / 真实 SDK），仅外部协议边界
 * （模型流、工具执行器）使用 tests/migration/stubs.ts 的确定性替身：
 *  - 会话读取/历史分页/损坏尾记录：真实 Hono 路由 server/routes/sessions.ts +
 *    真实 loadSessionHistoryMessages + 真实 lib/session-jsonl.ts 双层读取 + 真实 SessionManifestStore；
 *  - fork：真实 SDK SessionManager.createBranchedSession（生产 fork 链路同一调用点）；
 *  - MOOD：真实 MoodParser（流式权威入口）+ splitReservedTagSegments（历史权威入口）等价对照；
 *  - 工具调用/取消：真实 core/tool-invocation-gateway.ts + 真实 ConfirmStore/权限 wrapper；
 *  - 认证失败：真实 core/server-auth.ts authenticateRequestDetailed；
 *  - 附件：真实 SessionFileRegistry（sidecar 真实落盘于临时目录）。
 *
 * A09：设置 LINGXI_MIGRATION_REPLAY_OUT 时，每次运行把 raw 与 normalized 两份结果
 * 写入该目录；scripts/rust-tauri/r00-t05-replay.mjs 连续驱动三次并做规范化 diff。
 * normalized 结果与各夹具目录下的 expected.json（corrupted-tail 为 old-deviation.json
 * 的 old_actual）逐字相等——expected 冻结自首次验证运行并逐项对照过既有公开测试断言
 * （锚点见 OLD_BEHAVIOR_ORACLE.md），不是对 JSON 自比。
 */

import fs from "fs";
import os from "os";
import path from "path";
import { createHash } from "node:crypto";
import { afterAll, describe, expect, it } from "vitest";
import { Hono } from "hono";
import { canonicalize, stableStringify } from "./normalize.ts";
import { createScriptedModelStub, createRecordingToolStub } from "./stubs.ts";

if (process.env.LINGXI_MIGRATION_BLOCK_NETWORK !== "0") {
  const { installExternalNetworkGuard } = await import("./network-guard.ts");
  installExternalNetworkGuard("tests/migration/r00-t05-replay.test.ts");
}

const { readCurrentSessionBranch, readSessionMessages } = await import("../../lib/session-jsonl.ts");
const { SessionManager } = await import("../../lib/pi-sdk/index.ts");
const { SessionManifestStore } = await import("../../core/session-manifest/store.ts");
const { loadSessionHistoryMessages } = await import("../../core/message-utils.ts");
const { createSessionsRoute } = await import("../../server/routes/sessions.ts");
const { MoodParser, ThinkTagParser } = await import("../../core/events.ts");
const { splitReservedTagSegments } = await import("../../shared/reserved-tag-stream.ts");
const { INTERNAL_MOOD_TAGS } = await import("../../shared/internal-mood-block.ts");
const { ToolInvocationGateway } = await import("../../core/tool-invocation-gateway.ts");
const { ToolTargetRegistry } = await import("../../core/tool-target-registry.ts");
const {
  createFirstPartyToolIdentity,
  createToolSchemaValidator,
  normalizeToolPermissionContract,
  runWithPreparedInvocation,
} = await import("../../lib/tools/invocation/index.ts");
const { createServerAuthService } = await import("../../core/server-auth.ts");
const { SessionFileRegistry } = await import("../../lib/session-files/session-file-registry.ts");
const { ConfirmStore } = await import("../../lib/confirm-store.ts");
const { wrapWithSessionPermission } = await import("../../lib/tools/session-permission-wrapper.ts");

const FIXTURES_ROOT = path.resolve(import.meta.dirname, "fixtures");
const OUT_ROOT = process.env.LINGXI_MIGRATION_REPLAY_OUT
  ? path.resolve(process.env.LINGXI_MIGRATION_REPLAY_OUT)
  : null;
const BUSINESS_AGENT_ID = "hana";

/** 夹具目录内取文件：解析后必须仍位于 FIXTURES_ROOT 内，拒绝越界。 */
function fixturePath(...segments: string[]): string {
  const target = path.resolve(FIXTURES_ROOT, ...segments);
  if (target === FIXTURES_ROOT || !target.startsWith(FIXTURES_ROOT + path.sep)) {
    throw new Error(`fixture path escapes fixtures root: ${segments.join("/")}`);
  }
  return target;
}

/** 读夹具 JSON：所有 expected/spec 读取都经过这里（含上面的边界校验）。 */
function readFixtureJson(...segments: string[]): any {
  return JSON.parse(fs.readFileSync(fixturePath(...segments), "utf8"));
}

const tmpDirs: string[] = [];
function makeTmpDir(prefix: string): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  tmpDirs.push(dir);
  return dir;
}
afterAll(() => {
  for (const dir of tmpDirs) fs.rmSync(dir, { recursive: true, force: true });
});

function sha256File(filePath: string): string {
  return createHash("sha256").update(fs.readFileSync(filePath)).digest("hex");
}

/** 真实路由 + 真实 manifest 存储（镜像生产 engine 依赖面的最小桩，读取逻辑零桩）。 */
async function buildRouteApp(agentsDir: string, sessionPath: string, manifest: any) {
  const engine = {
    agentsDir,
    currentSessionPath: null,
    isSessionStreaming: () => false,
    agentIdFromSessionPath: () => BUSINESS_AGENT_ID,
    getAgent: () => ({ agentName: "Hana" }),
    getSessionWorkspaceMount: () => null,
    getSessionManifest: (id: string) => (id === manifest?.sessionId ? manifest : null),
    getSessionIdForPath: (p: string) => (p === sessionPath ? manifest?.sessionId ?? null : null),
    openSessionManagerAtCurrentBranch: (p: string, dir: string) => SessionManager.open(p, dir),
  };
  const app = new Hono();
  app.route("/api", createSessionsRoute(engine));
  return app;
}

async function requestMessages(app: Hono, sessionPath: string, query: Record<string, string>) {
  const qs = new URLSearchParams({ path: sessionPath, ...query }).toString();
  const res = await app.request(`/api/sessions/messages?${qs}`);
  return { status: res.status, body: await res.json() };
}

function recordActual(fixtureId: string, raw: unknown): void {
  if (!OUT_ROOT) return;
  if (!/^[A-Za-z0-9._-]+$/.test(fixtureId)) {
    throw new Error(`unsafe fixture id: ${JSON.stringify(fixtureId)}`);
  }
  fs.mkdirSync(OUT_ROOT, { recursive: true });
  const rawTarget = path.resolve(OUT_ROOT, `${fixtureId}.raw.json`);
  const normalizedTarget = path.resolve(OUT_ROOT, `${fixtureId}.normalized.json`);
  if (!rawTarget.startsWith(OUT_ROOT + path.sep) || !normalizedTarget.startsWith(OUT_ROOT + path.sep)) {
    throw new Error(`evidence path escapes out dir: ${fixtureId}`);
  }
  fs.writeFileSync(rawTarget, stableStringify(raw) + "\n", "utf8");
  fs.writeFileSync(normalizedTarget, canonicalize(raw) + "\n", "utf8");
}

function expectMatchesExpected(fixtureId: string, raw: unknown, expectedRelative: string): void {
  recordActual(fixtureId, raw);
  const expected = readFixtureJson(expectedRelative);
  expect(canonicalize(raw)).toBe(stableStringify(expected));
}

/** 把夹具 JSONL 部署到临时 agents 目录并建立真实 manifest。 */
function deploySessionFixture(relativeFixtureDir: string, fileName = "session.jsonl") {
  const agentsDir = makeTmpDir("hana-mig-replay-");
  const sessionPath = path.join(agentsDir, BUSINESS_AGENT_ID, "sessions", fileName);
  if (!sessionPath.startsWith(agentsDir + path.sep)) {
    throw new Error("session path escapes agents dir");
  }
  fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
  fs.copyFileSync(fixturePath(relativeFixtureDir, fileName), sessionPath);
  const manifestStore = new SessionManifestStore({ dbPath: path.join(agentsDir, "session-manifest.db") });
  const manifest = manifestStore.createForPath({
    sessionPath,
    ownerAgentId: BUSINESS_AGENT_ID,
    domain: "desktop",
    kind: "chat",
  });
  return { agentsDir, sessionPath, manifestStore, manifest };
}

/** 消息记录的稳定身份（路由返回 display 消息的 id/entryId 字段）。 */
function messageIds(body: any): unknown[] {
  return (body?.messages ?? []).map((m: any) => m.id ?? m.entryId ?? null);
}

describe("R00-T05 夹具回放（真实旧入口）", () => {
  it("multi-turn-basic：多轮会话——路由全量读取 + 双层模块读取", async () => {
    const { agentsDir, sessionPath, manifestStore, manifest } = deploySessionFixture("sessions/multi-turn-basic");
    try {
      const app = await buildRouteApp(agentsDir, sessionPath, manifest);
      const { status, body } = await requestMessages(app, sessionPath, { all: "1" });

      const strict = readCurrentSessionBranch(sessionPath);
      const lenient = readSessionMessages(sessionPath);

      expectMatchesExpected("multi-turn-basic", {
        route: { status, hasMore: body.hasMore, nextBefore: body.nextBefore, messages: body.messages },
        strictReader: { lineageIds: strict.lineage.map((e: any) => e.id), headResolution: strict.headResolution ?? null },
        lenientReader: lenient.messages.map((m: any) => ({ role: m.role, content: m.content })),
        sessionIdShape: /^sess_[0-9a-z]+_[0-9a-f]{20}$/.test(manifest.sessionId),
      }, "sessions/multi-turn-basic/expected.json");
    } finally {
      manifestStore.close();
    }
  });

  it("pagination-corpus：历史分页——limit 游标走页 + all=1 等价", async () => {
    const { agentsDir, sessionPath, manifestStore, manifest } = deploySessionFixture("sessions/pagination-corpus");
    try {
      const app = await buildRouteApp(agentsDir, sessionPath, manifest);

      const walk = async () => {
        const pages: unknown[] = [];
        const pageIds: unknown[][] = [];
        let cursor: string | null = null;
        for (;;) {
          const query: Record<string, string> = { limit: "10" };
          if (cursor !== null) query.before = cursor;
          const { status, body } = await requestMessages(app, sessionPath, query);
          const ids = messageIds(body);
          pages.push({
            query: cursor === null ? "limit=10" : `limit=10&before=${cursor}`,
            status,
            count: ids.length,
            hasMore: body.hasMore,
            nextBefore: body.nextBefore,
            firstId: ids[0] ?? null,
            lastId: ids[ids.length - 1] ?? null,
          });
          pageIds.push(ids);
          if (!body.hasMore || body.nextBefore == null) break;
          cursor = body.nextBefore;
        }
        // 检索顺序为最新页在前；恢复后的展示顺序 = 页序反转（旧→新）拼接
        const displayOrder = pageIds.slice().reverse().flat();
        return { pages, displayOrder };
      };
      const firstWalk = await walk();
      const all = await requestMessages(app, sessionPath, { all: "1" });
      const allIds = messageIds(all.body);
      const secondWalk = await walk();

      expectMatchesExpected("pagination-corpus", {
        pages: firstWalk.pages,
        walkDeterministic: JSON.stringify(firstWalk.displayOrder) === JSON.stringify(secondWalk.displayOrder),
        equivalence: {
          allStatus: all.status,
          allCount: allIds.length,
          pagedCount: firstWalk.displayOrder.length,
          orderEqual: JSON.stringify(allIds) === JSON.stringify(firstWalk.displayOrder),
          noOverlap: new Set(firstWalk.displayOrder).size === firstWalk.displayOrder.length,
        },
      }, "sessions/pagination-corpus/expected.json");
    } finally {
      manifestStore.close();
    }
  });

  it("corrupted-tail：损坏尾记录——分层归因（A09 确定性；A10 断言在独立测试）", async () => {
    // 每个入口用独立新鲜副本，避免上一个入口的修复副作用污染下一个的观察
    const observeRoute = async () => {
      const { agentsDir, sessionPath, manifestStore, manifest } = deploySessionFixture("sessions/corrupted-tail");
      try {
        const app = await buildRouteApp(agentsDir, sessionPath, manifest);
        const { status, body } = await requestMessages(app, sessionPath, { all: "1" });
        return {
          status,
          messageCount: messageIds(body).length,
          bodyAnnotationAbsent: !/dropp|corrupt|repair|degrad/i.test(JSON.stringify(body)),
          repairReceiptExists: fs.existsSync(
            path.join(path.dirname(sessionPath), path.basename(sessionPath) + ".repair.json"),
          ),
        };
      } finally {
        manifestStore.close();
      }
    };
    const observeProductionEntry = async () => {
      const { agentsDir, sessionPath, manifestStore } = deploySessionFixture("sessions/corrupted-tail");
      try {
        const production = await loadSessionHistoryMessages(null, sessionPath);
        return {
          messageCount: production.length,
          roles: production.map((m: any) => m.role ?? null),
          annotationAbsent: !/dropp|corrupt|repair|degrad/i.test(JSON.stringify(production.map((m: any) => Object.keys(m)))),
          repairReceiptExists: fs.existsSync(
            path.join(path.dirname(sessionPath), path.basename(sessionPath) + ".repair.json"),
          ),
        };
      } finally {
        manifestStore.close();
      }
    };

    const { agentsDir, sessionPath, manifestStore } = deploySessionFixture("sessions/corrupted-tail");
    let strictOutcome: any = null;
    let lenient: any = null;
    try {
      try {
        readCurrentSessionBranch(sessionPath);
        strictOutcome = { threw: false };
      } catch (error: any) {
        strictOutcome = { threw: true, name: error?.name, code: error?.code ?? null, details: error?.details ?? null };
      }
      lenient = readSessionMessages(sessionPath);
    } finally {
      manifestStore.close();
    }

    const observation = {
      strictReader: strictOutcome,
      lenientReader: {
        messageCount: lenient.messages.length,
        roles: lenient.messages.map((m: any) => m.role),
        annotationAbsent: !/dropp|corrupt|repair|degrad/i.test(JSON.stringify(lenient)),
      },
      route: await observeRoute(),
      productionEntry: await observeProductionEntry(),
    };
    recordActual("corrupted-tail", observation);

    const deviation = readFixtureJson("sessions/corrupted-tail/old-deviation.json");
    expect(canonicalize(observation)).toBe(stableStringify(deviation.old_actual));
  });

  it("fork-lineage：会话 fork——真实 SDK createBranchedSession", () => {
    const agentsDir = makeTmpDir("hana-mig-fork-");
    const sessionDir = path.join(agentsDir, BUSINESS_AGENT_ID, "sessions");
    fs.mkdirSync(sessionDir, { recursive: true });
    const sessionPath = path.join(sessionDir, "source.jsonl");
    fs.copyFileSync(fixturePath("sessions/fork-lineage/session.jsonl"), sessionPath);
    const request = readFixtureJson("sessions/fork-lineage/fork-request.json");

    const sourceHashBefore = sha256File(sessionPath);
    const manager = SessionManager.open(sessionPath, sessionDir);
    const childPath = path.resolve(manager.createBranchedSession(request.boundaryEntryId) as string);
    if (!childPath.startsWith(path.resolve(sessionDir) + path.sep)) {
      throw new Error(`child session escapes session dir: ${childPath}`);
    }
    const sourceHashAfter = sha256File(sessionPath);

    const childLines = fs.readFileSync(childPath, "utf8").split("\n").filter((l) => l.trim());
    const childEntries = childLines.map((l) => JSON.parse(l));
    const childHeader = childEntries[0];
    const sourceHeaderId = JSON.parse(fs.readFileSync(sessionPath, "utf8").split("\n")[0]).id;

    expectMatchesExpected("fork-lineage", {
      childCreated: childPath !== path.resolve(sessionPath),
      childHeader: {
        type: childHeader.type,
        version: childHeader.version,
        parentSession: childHeader.parentSession ?? null,
        headerIsNewSession: childHeader.id !== sourceHeaderId,
      },
      childLineageIds: childEntries.filter((e: any) => e.type === "message").map((e: any) => e.id),
      sourceBytesUnchanged: sourceHashBefore === sourceHashAfter,
    }, "sessions/fork-lineage/expected.json");
  });

  it("mood-stream-scripts：MOOD——真实流式/历史双入口等价（模型替身固定片段）", () => {
    const scripts = readFixtureJson("mood/stream-scripts/scripts.json");
    const results = scripts.scripts.map((script: any) => {
      const stub = createScriptedModelStub(script.chunks);
      const think = new ThinkTagParser();
      const mood = new MoodParser();
      const events: any[] = [];
      const feedText = (text: string) => {
        mood.feed(text, (moodEvent: any) => events.push(moodEvent));
      };
      stub.feed((frame) => {
        think.feed(frame, (event: any) => {
          if (event.type === "text") feedText(event.data);
          else events.push(event);
        });
      });
      think.flush((event: any) => {
        if (event.type === "text") feedText(event.data);
        else events.push(event);
      });
      mood.flush((event: any) => events.push(event));

      const fullText = script.chunks.join("");
      const segments = splitReservedTagSegments(fullText, INTERNAL_MOOD_TAGS);
      const liveVisible = events.filter((e) => e.type === "text").map((e) => e.data || "").join("");
      const historyVisible = segments.filter((s: any) => s.type === "text").map((s: any) => s.text).join("");

      return {
        id: script.id,
        liveEvents: events.map((e) => (e.data === undefined ? { type: e.type } : { type: e.type, data: e.data })),
        historySegments: segments,
        parity: { visibleEqual: liveVisible === historyVisible },
        visibleText: liveVisible,
      };
    });

    expectMatchesExpected("mood-stream-scripts", results, "mood/stream-scripts/expected.json");
  });

  it("tool-invocation-matrix：工具调用——真实网关全链路（工具替身记录调用）", async () => {
    const spec = readFixtureJson("tool/cases.json");

    function buildGateway(behavior: any) {
      const registry = new ToolTargetRegistry();
      const identity = createFirstPartyToolIdentity({
        publicName: spec.target.publicName,
        capabilityBase: spec.target.capabilityBase,
      });
      let generation = spec.request.lifecycleGeneration;
      const stub = createRecordingToolStub(behavior);
      const permission = normalizeToolPermissionContract({
        name: identity.publicName,
        sessionPermission: {
          resolveInvocation: () => ({
            action: spec.target.permission.action,
            kind: spec.target.permission.kind,
            capability: spec.target.permission.capability,
            sideEffect: spec.target.permission.sideEffect,
          }),
        },
      }, identity);
      const validator = createToolSchemaValidator(spec.target.schema, identity);
      registry.register({
        identity,
        label: "Write note",
        description: "Write one note",
        parameters: validator.schema,
        deferrable: true,
        pinned: false,
        permission,
        validator,
        availability: { eligible: true },
        getCurrentGeneration: () => generation,
        isCurrentlyAvailable: () => true,
        executeCanonical: stub.executeCanonical,
        normalizeResult: (result: unknown) => ({ normalized: result }),
      });
      const gateway = new ToolInvocationGateway({ registry, authorize: async () => undefined, log: { error: () => {} } });
      const request = {
        targetId: identity.targetId,
        route: spec.request.route,
        arguments: { ...spec.request.arguments },
        sessionId: spec.request.sessionId,
        sessionPath: spec.request.sessionPath,
        agentId: spec.request.agentId,
        lifecycleGeneration: spec.request.lifecycleGeneration,
        toolCallId: spec.request.toolCallId,
        signal: new AbortController().signal,
        onUpdate: () => {},
        ctx: { ...spec.request.ctx },
      };
      return { gateway, request, stub, bumpGeneration: () => { generation += 1; } };
    }

    async function runCase(caseId: string, behavior: any, mutateAfterPrepare?: (h: any) => void, prepared = true) {
      const h = buildGateway(behavior);
      const outcome: any = { caseId };
      try {
        if (!prepared) {
          await h.gateway.invoke(h.request);
        } else {
          const resolved = h.gateway.resolvePermission(h.request);
          mutateAfterPrepare?.(h);
          await runWithPreparedInvocation(resolved, () => h.gateway.invoke(h.request));
        }
        outcome.ok = true;
      } catch (error: any) {
        outcome.ok = false;
        outcome.errorCode = error?.code ?? null;
        outcome.errorName = error?.name ?? null;
      }
      outcome.executorCalls = h.stub.calls;
      return outcome;
    }

    const results = [
      await runCase("baseline-valid", { kind: "ok" }),
      await runCase("prepared-missing", { kind: "ok" }, undefined, false),
      await runCase(
        "tampered-arguments",
        { kind: "ok" },
        (h) => { h.request.arguments = { ...h.request.arguments, content: "tampered" }; },
      ),
      await runCase(
        "key-order-only",
        { kind: "ok" },
        (h) => { h.request.arguments = { content: h.request.arguments.content, path: h.request.arguments.path }; },
      ),
      await runCase("target-revoked", { kind: "ok" }, (h) => { h.bumpGeneration(); }),
      await runCase("executor-generic-failure", { kind: "fail", message: "synthetic executor failure" }),
    ];

    expectMatchesExpected("tool-invocation-matrix", results, "tool/expected.json");
  });

  it("cancel-edges：取消——网关取消语义 + 真实审批中止", async () => {
    function buildGateway(behavior: any, signal: AbortSignal) {
      const registry = new ToolTargetRegistry();
      const identity = createFirstPartyToolIdentity({ publicName: "write_note", capabilityBase: "write_note" });
      const stub = createRecordingToolStub(behavior);
      const permission = normalizeToolPermissionContract({
        name: identity.publicName,
        sessionPermission: {
          resolveInvocation: () => ({
            action: "write",
            kind: "review",
            capability: "write_note.write",
            sideEffect: { kind: "workspace_write", summary: "Write one note." },
          }),
        },
      }, identity);
      const validator = createToolSchemaValidator({
        type: "object", required: ["path", "content"], additionalProperties: false,
        properties: { path: { type: "string" }, content: { type: "string" } },
      }, identity);
      registry.register({
        identity, label: "Write note", description: "Write one note", parameters: validator.schema,
        deferrable: true, pinned: false, permission, validator, availability: { eligible: true },
        getCurrentGeneration: () => 3,
        isCurrentlyAvailable: () => true,
        executeCanonical: stub.executeCanonical,
        normalizeResult: (result: unknown) => ({ normalized: result }),
      });
      const gateway = new ToolInvocationGateway({ registry, authorize: async () => undefined, log: { error: () => {} } });
      const request = {
        targetId: identity.targetId,
        route: "direct" as const,
        arguments: { path: "note.md", content: "hello" },
        sessionId: "session-cancel",
        sessionPath: "/sessions/cancel.jsonl",
        agentId: "agent-1",
        lifecycleGeneration: 3,
        toolCallId: "call-cancel",
        signal,
        onUpdate: () => {},
        ctx: { caller: "model" },
      };
      return { gateway, request, stub };
    }

    async function gatewayCase(caseId: string, behavior: any, setup: (controller: AbortController) => void) {
      const controller = new AbortController();
      const resolvedBehavior = behavior.kind === "abortThenResolve" ? { ...behavior, controller } : behavior;
      const h = buildGateway(resolvedBehavior, controller.signal);
      const prepared = h.gateway.resolvePermission(h.request);
      setup(controller);
      const outcome: any = { caseId };
      try {
        await runWithPreparedInvocation(prepared, () => h.gateway.invoke(h.request));
        outcome.ok = true;
      } catch (error: any) {
        outcome.ok = false;
        outcome.errorCode = error?.code ?? null;
      }
      outcome.executorCalls = h.stub.calls;
      return outcome;
    }

    const results: unknown[] = [
      await gatewayCase("abort-before-execution", { kind: "ok" }, (controller) => controller.abort(new Error("user_abort"))),
      await gatewayCase("abort-error-from-executor", { kind: "abortError" }, () => {}),
      await gatewayCase("abort-after-completion", { kind: "abortThenResolve" }, () => {}),
    ];

    // 审批等待中止：真实 ConfirmStore + 真实权限 wrapper（ask 模式）
    {
      const confirmStore = new ConfirmStore();
      const calls: unknown[] = [];
      const tool = {
        name: "write",
        execute: async (...args: unknown[]) => {
          calls.push(args);
          return { content: [{ type: "text", text: "executed" }] };
        },
      };
      const [wrapped] = wrapWithSessionPermission([tool], {
        getPermissionMode: () => "ask",
        getConfirmStore: () => confirmStore,
        emitEvent: () => {},
      });
      const sessionPath = "/tmp/mig-cancel-session.jsonl";
      const pending = (wrapped as any).execute("call-a5", { path: "x" }, null, null, {
        sessionManager: { getSessionFile: () => sessionPath },
      });
      const deadline = Date.now() + 5000;
      while (confirmStore.size === 0 && Date.now() < deadline) {
        await new Promise((r) => setTimeout(r, 20));
      }
      if (confirmStore.size === 0) throw new Error("confirmation never became pending");
      confirmStore.abortBySession(sessionPath);
      const result = await pending;
      results.push({
        caseId: "approval-wait-aborted",
        toolExecutions: calls.length,
        result: {
          isError: result.isError,
          confirmed: result.details?.confirmed,
          confirmationStatus: result.details?.confirmation?.status,
        },
        storeEmptyAfterAbort: confirmStore.size === 0,
      });

      // 审批晚到不能授权旧调用（真实 ConfirmStore 直接用例，与 p02 同链路）
      const direct = new ConfirmStore();
      const { confirmId } = direct.create("tool_action_approval", { toolName: "write" }, sessionPath);
      direct.abortBySession(sessionPath);
      results.push({
        caseId: "late-resolve-rejected",
        lateResolveAccepted: direct.resolve(confirmId, "confirmed", undefined),
        storeSize: direct.size,
      });
    }

    expectMatchesExpected("cancel-edges", results, "cancel/expected.json");
  });

  it("auth-server-deny：认证失败——真实 authenticateRequestDetailed", () => {
    const spec = readFixtureJson("auth/cases.json");
    const home = makeTmpDir("hana-mig-auth-");
    const auth = createServerAuthService({
      lingxiHome: home,
      loopbackToken: spec.loopbackToken,
      runtimeContext: spec.runtimeContext,
    });
    const results = spec.cases.map((c: any) => {
      const outcome = (auth as any).authenticateRequestDetailed({
        authorization: c.authorization,
        connectionKind: c.connectionKind,
      });
      return {
        caseId: c.id,
        principal: outcome.principal
          ? {
              kind: outcome.principal.kind,
              credentialKind: outcome.principal.credentialKind,
              connectionKind: outcome.principal.connectionKind,
              trustState: outcome.principal.trustState,
              userId: outcome.principal.userId,
            }
          : null,
        denied: outcome.denied ? { error: outcome.denied.error, reason: outcome.denied.reason } : null,
      };
    });
    expectMatchesExpected("auth-server-deny", results, "auth/expected.json");
  });

  it("attachments-lifecycle：附件——真实 SessionFileRegistry 全程落盘", () => {
    const spec = readFixtureJson("attachments/scenario.json");
    const root = makeTmpDir("hana-mig-files-");
    const lingxiHome = path.join(root, "home");
    const agentsDir = path.join(root, "agents", BUSINESS_AGENT_ID, "sessions");
    fs.mkdirSync(agentsDir, { recursive: true });
    fs.mkdirSync(lingxiHome, { recursive: true });

    const sourceSessionPath = path.join(agentsDir, "main.jsonl");
    const otherSessionPath = path.join(agentsDir, "other.jsonl");
    const targetSessionPath = path.join(agentsDir, "fork-session.jsonl");
    for (const p of [sourceSessionPath, otherSessionPath, targetSessionPath]) {
      fs.writeFileSync(p, "{}\n", "utf8");
    }

    const sourceSessionId = "sess_mig_source_0000000000000001";
    const otherSessionId = "sess_mig_other_0000000000000002";
    const targetSessionId = "sess_mig_target_0000000000000003";
    const registry = new SessionFileRegistry({
      now: () => 1234,
      managedCacheRoot: path.join(lingxiHome, "session-files"),
      getSessionIdForPath: (p: string) => {
        if (p === sourceSessionPath) return sourceSessionId;
        if (p === targetSessionPath) return targetSessionId;
        return null;
      },
    });

    const filePaths: Record<string, string> = {};
    for (const [name, content] of Object.entries(spec.files)) {
      const filePath = path.join(root, "inputs", name);
      if (!filePath.startsWith(root + path.sep)) {
        throw new Error(`attachment input escapes tmp root: ${name}`);
      }
      fs.mkdirSync(path.dirname(filePath), { recursive: true });
      fs.writeFileSync(filePath, content as string, "utf8");
      filePaths[name] = filePath;
    }

    const note = registry.registerFile({
      sessionId: sourceSessionId,
      sessionPath: sourceSessionPath,
      filePath: filePaths["note.md"],
      origin: "user_attachment",
      storageKind: "external",
      presentation: "attachment",
      listed: true,
    });
    const csv = registry.registerFile({
      sessionId: sourceSessionId,
      sessionPath: sourceSessionPath,
      filePath: filePaths["data.csv"],
      origin: "bridge_inbound",
      storageKind: "managed_cache",
      presentation: "attachment",
      listed: true,
    });

    const crossSession = registry.get(note.id, { sessionId: otherSessionId, sessionPath: otherSessionPath });

    const fork = registry.forkSessionFiles({
      sourceSessionId,
      sourceSessionPath,
      targetSessionId,
      targetSessionPath,
      retainedEntries: [{
        type: "message",
        message: {
          role: "user",
          content: `[SessionFile] ${JSON.stringify({ fileId: note.id, sessionPath: sourceSessionPath })}\n[attached_image: ${filePaths["note.md"]}]`,
        },
      }],
    });

    const forkedCsvId = fork.fileIdMap[csv.id] ?? null;
    const forkedCsvEntry = forkedCsvId
      ? registry.get(forkedCsvId, { sessionId: targetSessionId, sessionPath: targetSessionPath })
      : null;
    const forkedManagedBytes = forkedCsvEntry ? fs.readFileSync(forkedCsvEntry.filePath, "utf8") : null;
    const targetListAfterFork = (registry.list(targetSessionPath) ?? []).map((f: any) => ({
      id: f.id,
      storageKind: f.storageKind,
      legacyFileIds: f.legacyFileIds ?? null,
    }));

    const discard = registry.discardForkedSessionFiles({ sessionId: targetSessionId, sessionPath: targetSessionPath });
    const sourceNoteAfterDiscard = registry.get(note.id, { sessionId: sourceSessionId, sessionPath: sourceSessionPath });

    expectMatchesExpected("attachments-lifecycle", {
      registered: {
        note: {
          idShape: /^sf_[a-f0-9]{16}$/.test(note.id),
          storageKind: note.storageKind,
          origin: note.origin,
          listed: note.listed,
          displayName: note.displayName ?? null,
          size: note.size,
        },
        csv: {
          idShape: /^sf_[a-f0-9]{16}$/.test(csv.id),
          storageKind: csv.storageKind,
          copiedNotMoved: csv.filePath !== filePaths["data.csv"],
        },
      },
      sidecar: {
        version: JSON.parse(
          fs.readFileSync(path.join(path.dirname(sourceSessionPath), path.basename(sourceSessionPath) + ".files.json"), "utf8"),
        ).version,
      },
      crossSessionLookup: crossSession === null,
      crossSessionSidecarUntouched: !fs.existsSync(
        path.join(path.dirname(otherSessionPath), path.basename(otherSessionPath) + ".files.json"),
      ),
      fork: {
        copiedFileCount: fork.files.length,
        fileIdMapCovers: { note: fork.fileIdMap[note.id] != null, csv: fork.fileIdMap[csv.id] != null },
        targetListAfterFork,
        forkedCsvEntry,
        forkedManagedBytes,
      },
      discard: { ...discard, resultShape: Object.keys(discard).sort() },
      sourceSurvivesDiscard: sourceNoteAfterDiscard != null && sourceNoteAfterDiscard.id === note.id,
    }, "attachments/expected.json");
  });
});

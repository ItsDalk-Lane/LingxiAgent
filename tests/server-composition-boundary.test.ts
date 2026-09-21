/**
 * Behavior lock — server composition boundary (route seam refactor).
 *
 * `server/index.ts` used to import all ~43 route factories directly and
 * mount them inline. It now statically imports
 * `server/composition/open-root.ts` (open) and accepts an optional
 * `root.registerClosedRoutes` hook (supplied by `server/main-full.ts` via
 * `server/composition/full-root.ts`, closed-product) instead. This file
 * locks the three properties the refactor must not change:
 *
 * 1. The exact set of mounted route factories (and their mount prefix) is
 *    unchanged — a pure "moved code between files" refactor, not a route
 *    behavior change.
 * 2. A real spawned full composition (`server/main-full.ts`) still serves
 *    both open-root and full-root routes behind the same global auth
 *    middleware, proven with real HTTP requests — not static assertions.
 * 3. `server/index.ts` alone boots nothing on mere import — only
 *    `server/main-full.ts` (or another composition entry that calls
 *    `startServer()`) does. This is the "no silent non-start" contract.
 */
import { describe, expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { spawn } from "child_process";
import { createServer } from "node:http";
import WebSocket from "ws";
import { openaiCompletionsSseBody } from "./helpers/model-observability-scenario-harness.ts";

const root = process.cwd();

// ---------------------------------------------------------------------------
// Part 1 — sorted mount-call inventory snapshot.
//
// Golden list captured from server/index.ts *before* the composition
// split (43 `app.route(prefix, headExpression)` call sites, sorted), plus
// route factories added since. Every
// entry is `${prefix} :: ${firstIdentifierOfSecondArg}` — enough to prove
// "same factory mounted at the same prefix", independent of exactly how
// many lines its (unchanged) argument object spans or which file now
// contains the call.
// ---------------------------------------------------------------------------

const PRE_REFACTOR_MOUNT_CALLS = Object.freeze([
  '"" :: chatWsRoute',
  '"" :: createHtmlPreviewRoute',
  '"" :: createMobileStaticRoute',
  '"/api" :: chatRestRoute',
  '"/api" :: createAccessRoute',
  '"/api" :: createAgentsRoute',
  '"/api" :: createAuthRoute',
  '"/api" :: createAvatarRoute',
  '"/api" :: createBridgeRoute',
  '"/api" :: createCardsRoute',
  '"/api" :: createChannelsRoute',
  '"/api" :: createCharacterCardsRoute',
  '"/api" :: createCheckpointsRoute',
  '"/api" :: createCommandsRoute',
  '"/api" :: createConfigRoute',
  '"/api" :: createConfirmRoute',
  // 会话地图（conversation map）：turns 投影 + 画布布局持久化面（挂载点 open-root）。
  '"/api" :: createConversationMapRoute',
  '"/api" :: createDeskRoute',
  '"/api" :: createDevicesRoute',
  '"/api" :: createDiaryRoute',
  '"/api" :: createDmRoute',
  // Added after the split: 环境依赖检测面（/api/system/env-deps/*）。
  '"/api" :: createEnvDepsRoute',
  '"/api" :: createExperimentsRoute',
  // 工作区文件历史的查询与还原面（挂载点 open-root，与 resource-io 同域）
  '"/api" :: createFileHistoryRoute',
  '"/api" :: createFsRoute',
  // Added after the split: 环境信息卡的 git 面（挂载点 full-root，与 desk 同域）。
  '"/api" :: createGitEnvironmentRoute',
  '"/api" :: createInputDraftsRoute',
  // Added after the split: the Notebook-first Knowledge owner surface.
  '"/api" :: createKnowledgeRoute',
  // Added after the split: the MCP surface used to reach the app through the
  // generic plugin route proxy, so it had no factory of its own here.
  '"/api" :: createMcpRoute',
  '"/api" :: createMediaRoute',
  '"/api" :: createMemoryDreamRoute',
  '"/api" :: createMobileWorkbenchRoute',
  '"/api" :: createModelObservabilityRoute',
  '"/api" :: createModelsRoute',
  '"/api" :: createPreferencesRoute',
  '"/api" :: createProvidersRoute',
  '"/api" :: createResourceIoRoute',
  '"/api" :: createResourcesRoute',
  '"/api" :: createServerIdentityRoute',
  '"/api" :: createSessionCollabRoute',
  '"/api" :: createSessionProjectsRoute',
  '"/api" :: createSessionsRoute',
  '"/api" :: createSettingsSnapshotRoute',
  '"/api" :: createSkillsRoute',
  '"/api" :: createSpeechRecognitionRoute',
  '"/api" :: createStudioWorkspacesRoute',
  '"/api" :: createUploadRoute',
  '"/api" :: createUsageRoute',
  '"/api" :: createWebAuthRoute',
  '"/api" :: createWebSocketAuthRoute',
]);

/** Extracts `app.route(prefix, headIdentifier` call-site pairs from one file. */
function extractMountCalls(filePath: string): string[] {
  const src = fs.readFileSync(filePath, "utf-8");
  const re = /app\.route\(\s*("(?:[^"]*)")\s*,\s*([A-Za-z0-9_]+)/g;
  const pairs: string[] = [];
  let m: RegExpExecArray | null;
  while ((m = re.exec(src))) pairs.push(`${m[1]} :: ${m[2]}`);
  return pairs;
}

describe("composition boundary behavior lock: sorted mount-call inventory", () => {
  it("server/index.ts + composition/open-root.ts + composition/full-root.ts together mount exactly the expected route factories, at the same prefixes, as the pre-refactor server/index.ts", () => {
    const combined = [
      ...extractMountCalls(path.join(root, "server", "index.ts")),
      ...extractMountCalls(path.join(root, "server", "composition", "open-root.ts")),
      ...extractMountCalls(path.join(root, "server", "composition", "full-root.ts")),
    ].sort();

    expect(combined).toEqual([...PRE_REFACTOR_MOUNT_CALLS]);
  });

  it("mobile-workbench (evidence-needed) is mounted directly by server/index.ts itself, not absorbed into open-root.ts or full-root.ts", () => {
    const indexPairs = extractMountCalls(path.join(root, "server", "index.ts"));
    const openRootPairs = extractMountCalls(path.join(root, "server", "composition", "open-root.ts"));

    expect(indexPairs).toContain('"/api" :: createMobileWorkbenchRoute');
    expect(openRootPairs).not.toContain('"/api" :: createMobileWorkbenchRoute');
  });

  it("server/index.ts imports composition/open-root.ts unconditionally and imports no closed-product route file directly", () => {
    const indexSource = fs.readFileSync(path.join(root, "server", "index.ts"), "utf-8");

    expect(indexSource).toContain('import { registerOpenRoutes } from "./composition/open-root.ts";');
    for (const p2 of ["avatar", "cards", "character-cards", "desk", "diary"]) {
      expect(indexSource).not.toContain(`from "./routes/${p2}.ts"`);
    }
    expect(indexSource).not.toContain("composition/full-root.ts");
    expect(indexSource).not.toContain("main-full.ts");
  });
});

// ---------------------------------------------------------------------------
// Part 1b — builtin media adapter injection seam. core/media/universal-media-
// manager.ts (open) never imports core/media-adapters/ (closed) itself; only
// the closed composition root supplies adapters, via the same root-argument
// seam as registerClosedRoutes.
// ---------------------------------------------------------------------------

describe("composition boundary behavior lock: builtin media adapter injection", () => {
  it("full composition supplies a non-empty builtinMediaAdapters list", async () => {
    const fullRoot = await import("../server/composition/full-root.ts");

    expect(Array.isArray(fullRoot.builtinMediaAdapters)).toBe(true);
    expect(fullRoot.builtinMediaAdapters.length).toBeGreaterThan(0);
  });

  it("server/index.ts (the open composition) only ever forwards root.builtinMediaAdapters, never constructs or imports a closed adapter list itself", () => {
    const indexSource = fs.readFileSync(path.join(root, "server", "index.ts"), "utf-8");

    expect(indexSource).toContain("builtinMediaAdapters: root.builtinMediaAdapters");
    expect(indexSource).not.toContain("core/media-adapters/");
  });

  it("main-full.ts forwards full-root's builtinMediaAdapters into startServer alongside registerClosedRoutes", () => {
    const mainFullSource = fs.readFileSync(path.join(root, "server", "main-full.ts"), "utf-8");

    expect(mainFullSource).toContain('from "./composition/full-root.ts"');
    expect(mainFullSource).toMatch(/registerClosedRoutes,\s*builtinMediaAdapters/);
    expect(mainFullSource).toMatch(/startServer\(\{\s*registerClosedRoutes,\s*builtinMediaAdapters\s*\}\)/);
    expect(mainFullSource).toContain('from "./standalone-runtime-smoke.ts"');
    expect(mainFullSource).toContain('process.env.LINGXI_INTERNAL_STANDALONE_RUNTIME_SMOKE === "1"');
    expect(mainFullSource).toContain("await runPackagedStandaloneRuntimeSmoke();");
  });
});

// ---------------------------------------------------------------------------
// Part 2 — bootstrap contract: importing server/index.ts alone must not
// start anything; only server/main-full.ts (or an equivalent caller of
// startServer()) does.
// ---------------------------------------------------------------------------

describe("composition boundary behavior lock: bootstrap contract (no silent non-start)", () => {
  it("importing server/index.ts alone defines startServer but starts nothing (no port bind, no server-info.json, clean exit)", async () => {
    const lingxiHome = fs.mkdtempSync(path.join(os.tmpdir(), "hana-index-only-import-"));
    try {
      const child = spawn(
        process.execPath,
        ["--input-type=module", "-e", 'import("./server/index.ts").then((m) => { process.stdout.write(`exports: ${Object.keys(m).sort().join(",")}\\n`); process.exit(0); })'],
        {
          cwd: root,
          env: { ...process.env, LINGXI_HOME: lingxiHome },
          stdio: ["ignore", "pipe", "pipe"],
        },
      );
      let stdout = "";
      let stderr = "";
      child.stdout.on("data", (chunk) => { stdout += chunk; });
      child.stderr.on("data", (chunk) => { stderr += chunk; });
      const result: any = await new Promise((resolve) => {
        // 60s：顺序契约（导入不启动）非墙钟 SLA；劣化 CI runner 上 TS 冷转换可超 15s
        // （2026-08-31 intel 实测假红，套件时长 24m→45m）。
        const timeout = setTimeout(() => { child.kill("SIGKILL"); resolve({ timeout: true }); }, 60000);
        child.once("close", (code, signal) => { clearTimeout(timeout); resolve({ code, signal }); });
      });

      expect(result.timeout).not.toBe(true);
      expect(result).toMatchObject({ code: 0, signal: null });
      // 除了 startServer 本身，还导出 resolveSessionMetadataRecoveryStatusForHealth
      // ——/api/health 的 sessionStore 兜底逻辑单测直接覆盖它，不需要真的拉起服务器。
      expect(stdout).toContain("exports: resolveSessionMetadataRecoveryStatusForHealth,startServer");
      // No engine/store side effects: no server-info.json written, no
      // agents/user directory seeded by ensureFirstRun.
      expect(fs.existsSync(path.join(lingxiHome, "server-info.json"))).toBe(false);
      expect(fs.existsSync(path.join(lingxiHome, "agents"))).toBe(false);
      expect(stderr).not.toContain("ensureFirstRun");
    } finally {
      // The timeout/assertion-failure paths reach here right after a SIGKILL,
      // so this rm needs the same Windows handle-latency tolerance as Part 3.
      fs.rmSync(lingxiHome, TEMP_HOME_RM_OPTIONS);
    }
  }, 90000);
});

// ---------------------------------------------------------------------------
// Part 3 — real request smoke against a real spawned full composition
// (server/main-full.ts), including the global auth middleware. Proves the
// open-root and full-root routes are both live and both still
// gated by the same auth check, end to end — not by static source
// inspection.
// ---------------------------------------------------------------------------

// Windows cleanup contract for tests that SIGKILL a spawned server: kill() is
// TerminateProcess and returns before the process dies, and the dying process
// (plus antivirus/search-indexer scans) can briefly hold handles inside the
// temp LINGXI_HOME without FILE_SHARE_DELETE. Removing the directory therefore
// has to happen after the real exit event, with retries for transient
// EPERM/EBUSY — otherwise cleanup itself fails the test on Windows CI.
const TEMP_HOME_RM_OPTIONS = { recursive: true, force: true, maxRetries: 20, retryDelay: 250 } as const;

function waitForExit(child: ReturnType<typeof spawn>, timeoutMs = 15000): Promise<void> {
  return new Promise((resolve) => {
    if (child.exitCode !== null || child.signalCode !== null) {
      resolve();
      return;
    }
    const timer = setTimeout(resolve, timeoutMs);
    child.once("exit", () => {
      clearTimeout(timer);
      resolve();
    });
  });
}

function waitForServerInfo(serverInfoPath: string, child: ReturnType<typeof spawn>, timeoutMs = 60000): Promise<any> {
  return new Promise((resolve, reject) => {
    let exited = false;
    let exitInfo: any = null;
    child.once("exit", (code, signal) => { exited = true; exitInfo = { code, signal }; });
    const deadline = Date.now() + timeoutMs;
    const poll = () => {
      if (exited) {
        reject(new Error(`server exited before writing server-info.json: ${JSON.stringify(exitInfo)}`));
        return;
      }
      try {
        const raw = fs.readFileSync(serverInfoPath, "utf-8");
        resolve(JSON.parse(raw));
        return;
      } catch {
        // not written yet
      }
      if (Date.now() > deadline) {
        reject(new Error("timed out waiting for server-info.json"));
        return;
      }
      setTimeout(poll, 200);
    };
    poll();
  });
}

describe("composition boundary behavior lock: real request smoke against the full composition (server/main-full.ts)", () => {
  it("serves an open-root route and a full-root (closed-product) route behind the same global auth middleware", async () => {
    const lingxiHome = fs.mkdtempSync(path.join(os.tmpdir(), "hana-composition-smoke-"));
    const serverInfoPath = path.join(lingxiHome, "server-info.json");
    const child = spawn(process.execPath, ["server/bootstrap.ts"], {
      cwd: root,
      env: {
        ...process.env,
        LINGXI_HOME: lingxiHome,
        LINGXI_PORT: "0",
        LINGXI_ROOT: root,
        LINGXI_SERVER_ENTRY: path.join(root, "server", "main-full.ts"),
        LINGXI_CREATE_STARTUP_SESSION: "0",
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stderr = "";
    child.stderr.on("data", (chunk) => { stderr += chunk; });

    try {
      const info = await waitForServerInfo(serverInfoPath, child);
      const base = `http://127.0.0.1:${info.port}`;
      const authHeaders = { Authorization: `Bearer ${info.token}` };

      // Public-shaped route (/api/health) still requires the loopback
      // token (route-security.ts classifies it AUTHENTICATED_ONLY, not
      // public) — proves the global middleware still runs before it.
      const healthNoAuth = await fetch(`${base}/api/health`);
      expect(healthNoAuth.status).toBe(403);

      const healthAuth = await fetch(`${base}/api/health`, { headers: authHeaders });
      expect(healthAuth.status).toBe(200);
      const healthBody = await healthAuth.json();
      expect(healthBody.status).toBe("ok");

      // Open route mounted via composition/open-root.ts (createAgentsRoute).
      const agentsNoAuth = await fetch(`${base}/api/agents`);
      expect(agentsNoAuth.status).toBe(403);
      const agentsAuth = await fetch(`${base}/api/agents`, { headers: authHeaders });
      expect(agentsAuth.status).toBe(200);
      const agentsBody = await agentsAuth.json();
      expect(Array.isArray(agentsBody.agents)).toBe(true);

      // Closed-product route mounted via composition/full-root.ts (createDiaryRoute) —
      // only reachable at all because server/main-full.ts supplied
      // registerClosedRoutes to startServer().
      const diaryNoAuth = await fetch(`${base}/api/diary/list`);
      expect(diaryNoAuth.status).toBe(403);
      const diaryAuth = await fetch(`${base}/api/diary/list`, { headers: authHeaders });
      expect(diaryAuth.status).toBe(200);
      const diaryBody = await diaryAuth.json();
      expect(Array.isArray(diaryBody.files)).toBe(true);
    } finally {
      child.kill("SIGKILL");
      await waitForExit(child);
      fs.rmSync(lingxiHome, TEMP_HOME_RM_OPTIONS);
      if (process.env.LINGXI_TEST_DEBUG) process.stderr.write(stderr);
    }
  }, 60000);
});

// ---------------------------------------------------------------------------
// Part 4 — P01 headless vertical slice (P01-T03/T07; scenarios P01-A01/A07/A10).
//
// Proves the full chain with zero desktop: real spawned full composition
// (server/main-full.ts), isolated LINGXI_HOME pre-seeded with a witness
// provider (local OpenAI-compatible protocol server = model stand-in), the
// desktop business entry (POST /api/sessions/new + WS /ws prompt — the same
// entry the renderer uses), a real Pi AgentSession on the locked SDK, a real
// `read` file-tool round, and a history read-back over HTTP.
//
// Also asserts: A07 malformed JSON via the real HTTP entry is rejected with
// no side effects; A10 a desktop-channel notification in a headless process
// does not block or crash it and normal chat continues right after.
// ---------------------------------------------------------------------------

const WITNESS_KEY = "sk-P01-VERTICAL-WITNESS-5f21";
const FILE_MARKER = "P01_VERTICAL_SLICE_FILE_CONTENT_7ad2";

type WitnessRequest = { url: string; headers: Record<string, string>; bodyJson: any };

class WitnessProvider {
  // Deterministic content-routed responses: every request is answered based on
  // its own message payload, so incidental startup background calls (agent
  // description generation etc.) can never steal a scripted turn's reply.
  readonly requests: WitnessRequest[] = [];
  readonly server: ReturnType<typeof createServer>;
  readonly ready: Promise<number>;

  private respondTo(bodyJson: any): string {
    // Route on the LAST user message only — every request carries the whole
    // conversation history, so routing on "anywhere in the payload" would
    // replay old turns' answers for new turns.
    const messages = Array.isArray(bodyJson?.messages) ? bodyJson.messages : [];
    const lastUser = [...messages].reverse().find((m: any) => m?.role === "user");
    const lastUserText = JSON.stringify(lastUser?.content ?? "");
    const historyText = JSON.stringify(messages);
    if (lastUserText.includes("请读取 slice-note.txt")) {
      if (!historyText.includes(FILE_MARKER)) {
        return this.toolCallBody("read", JSON.stringify({ path: "slice-note.txt" }));
      }
      return this.textBody("P01_VERTICAL_ASSISTANT_REPLY 文件内容已读取");
    }
    if (lastUserText.includes("桌面通知")) {
      if (historyText.includes("desktop channel probe")) {
        return this.textBody("P01_NOTIFY_DONE 已尝试通知");
      }
      return this.toolCallBody("notify", JSON.stringify({ title: "P01 headless", body: "desktop channel probe" }));
    }
    if (lastUserText.includes("普通聊天继续")) {
      return this.textBody("P01_PLAIN_CHAT_REPLY 普通聊天正常");
    }
    return this.textBody("witness default reply");
  }

  private textBody(content: string): string {
    return openaiCompletionsSseBody({ content, usage: { prompt_tokens: 9, completion_tokens: 4, total_tokens: 13 } });
  }

  private toolCallBody(toolName: string, argsJson: string): string {
    const chunk = {
      id: "chatcmpl-p01", object: "chat.completion.chunk", created: 0, model: "witness-model",
      choices: [{ index: 0, delta: { role: "assistant", tool_calls: [{ index: 0, id: `tc_p01_${toolName}`, type: "function", function: { name: toolName, arguments: argsJson } }] }, finish_reason: null }],
    };
    const done = {
      id: "chatcmpl-p01", object: "chat.completion.chunk", created: 0, model: "witness-model",
      choices: [{ index: 0, delta: {}, finish_reason: "tool_calls" }],
      usage: { prompt_tokens: 9, completion_tokens: 3, total_tokens: 12 },
    };
    return [`data: ${JSON.stringify(chunk)}`, `data: ${JSON.stringify(done)}`, "data: [DONE]", ""].join("\n\n");
  }

  constructor() {
    this.server = createServer((req, res) => {
      const chunks: Buffer[] = [];
      req.on("data", (chunk: Buffer) => chunks.push(chunk));
      req.on("end", () => {
        const raw = Buffer.concat(chunks).toString("utf8");
        let bodyJson: any = null;
        try { bodyJson = JSON.parse(raw); } catch { bodyJson = { unparseable: raw.slice(0, 512) }; }
        this.requests.push({
          url: req.url || "",
          headers: (req.headers as Record<string, string>) || {},
          bodyJson,
        });
        res.writeHead(200, { "content-type": "text/event-stream" });
        res.end(this.respondTo(bodyJson));
      });
    });
    this.ready = new Promise((resolve, reject) => {
      this.server.once("error", reject);
      this.server.listen(0, "127.0.0.1", () => {
        const address = this.server.address();
        if (address && typeof address === "object") resolve(address.port);
        else reject(new Error("witness server failed to bind"));
      });
    });
  }

  get baseUrl(): string {
    return `http://127.0.0.1:${(this.server.address() as { port: number }).port}`;
  }

  posts(): WitnessRequest[] { return this.requests.filter((r) => r.url.includes("/v1/chat/completions")); }

  close(): Promise<void> { return new Promise((resolve) => this.server.close(() => resolve())); }
}

describe("P01 headless vertical slice: witness provider → WS prompt → real read tool → history read-back", () => {
  it("runs the full chain without any desktop, rejects malformed JSON at the HTTP entry (A07), and keeps serving after a desktop-channel notify (A10)", async () => {
    const witness = new WitnessProvider();
    const witnessPort = await witness.ready;
    const lingxiHome = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p01-vertical-"));
    const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p01-ws-"));
    const serverInfoPath = path.join(lingxiHome, "server-info.json");
    fs.writeFileSync(path.join(workspaceDir, "slice-note.txt"), `${FILE_MARKER}\nsecond line\n`, "utf8");

    // Pre-seed a valid default agent config (skips seedDefaultAgent, so the real
    // ~/Desktop default workspace is never touched) pointing its chat model at
    // the witness provider, plus the provider catalog v2 entry for it.
    const agentDir = path.join(lingxiHome, "agents", "lingxi");
    fs.mkdirSync(path.join(agentDir, "sessions"), { recursive: true });
    const template = fs.readFileSync(path.join(root, "lib", "config.example.yaml"), "utf8");
    // migrations #5 之后 models.chat 必为 {id, provider} 对象（字符串被判为未配置）。
    const patched = template.replace(
      /^(\s*chat:\s*)".*"/m,
      "$1{id: witness-model, provider: witness}",
    );
    if (patched === template) throw new Error("failed to patch models.chat in config template");
    fs.writeFileSync(path.join(agentDir, "config.yaml"), patched, "utf8");
    fs.writeFileSync(path.join(lingxiHome, "provider-catalog.json"), JSON.stringify({
      catalogVersion: 2,
      providers: {
        witness: {
          base_url: `http://127.0.0.1:${witnessPort}/v1`,
          api: "openai-completions",
          api_key: WITNESS_KEY,
          models: ["witness-model"],
        },
      },
    }, null, 2), "utf8");

    const child = spawn(process.execPath, ["server/bootstrap.ts"], {
      cwd: root,
      env: {
        ...process.env,
        LINGXI_HOME: lingxiHome,
        LINGXI_PORT: "0",
        LINGXI_ROOT: root,
        LINGXI_SERVER_ENTRY: path.join(root, "server", "main-full.ts"),
        LINGXI_CREATE_STARTUP_SESSION: "0",
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stderr = "";
    child.stderr.on("data", (chunk) => { stderr += String(chunk); });

    try {
      const info = await waitForServerInfo(serverInfoPath, child);
      const base = `http://127.0.0.1:${info.port}`;
      const authHeaders = { authorization: `Bearer ${info.token}`, "content-type": "application/json" };

      // ── A07: malformed JSON at the real HTTP entry is rejected, no side effects ──
      // (P01-T05 修复后：非空但不可解析的 body → 400 invalid_json，不再被 safeJson
      //  静默吞成空对象后按默认参数建会话。)
      const beforeList = await (await fetch(`${base}/api/sessions`, { headers: authHeaders })).json();
      const malformed = await fetch(`${base}/api/sessions/new`, {
        method: "POST",
        headers: authHeaders,
        body: '{"cwd": "/tmp", "memoryEnabled": tru', // deliberately truncated JSON
      });
      expect(malformed.status).toBe(400);
      expect(await malformed.text()).toBeTruthy();
      const afterList = await (await fetch(`${base}/api/sessions`, { headers: authHeaders })).json();
      expect(afterList).toEqual(beforeList);

      // ── A07（验收修复）：字段类型错误的合法 JSON 同样被运行时拒绝 ──
      // 验收反例实测：修复前这三例分别被静默按默认执行（200 建会话 ×2）或以
      // 500 崩溃；修复后必须 400 invalid_field_type 且零副作用。
      for (const badBody of [
        '{"memoryEnabled": "yes-please"}',
        '{"thinkingLevel": {"hack": 1}}',
        '{"agentId": 12345}',
      ]) {
        const wrongType = await fetch(`${base}/api/sessions/new`, {
          method: "POST",
          headers: authHeaders,
          body: badBody,
        });
        expect(wrongType.status).toBe(400);
        const wrongTypeBody = await wrongType.json();
        expect(wrongTypeBody.code ?? wrongTypeBody.error?.code).toBe("invalid_field_type");
      }
      // ── A07（验收修复）：合法 JSON 但非对象（null/数字/字符串/数组）→ 400 invalid_body ──
      // 修复前 `null` 会在解构时抛 TypeError 变 500，`123`/`"str"`/`[]` 静默按默认执行。
      for (const nonObjectBody of ["null", "123", '"str"', "[]"]) {
        const nonObject = await fetch(`${base}/api/sessions/new`, {
          method: "POST",
          headers: authHeaders,
          body: nonObjectBody,
        });
        expect(nonObject.status).toBe(400);
        const nonObjectJson = await nonObject.json();
        expect(nonObjectJson.code ?? nonObjectJson.error?.code).toBe("invalid_body");
      }
      expect((await (await fetch(`${base}/api/sessions`, { headers: authHeaders })).json())).toEqual(beforeList);

      // ── A01: entry → SDK → read tool → reply → history read-back ──
      // (witness responses are content-routed, no pre-scripting needed)
      const created = await fetch(`${base}/api/sessions/new`, {
        method: "POST",
        headers: authHeaders,
        body: JSON.stringify({ cwd: workspaceDir }),
      });
      expect(created.status).toBe(200);
      const session = await created.json();
      expect(session.ok).toBe(true);
      expect(session.sessionId).toMatch(/^sess_/);
      expect(session.agentId).toBe("lingxi");

      const events: any[] = [];
      const ws = new WebSocket(`ws://127.0.0.1:${info.port}/ws?token=${encodeURIComponent(info.token)}`);
      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error("ws connect timeout")), 15000);
        ws.once("open", () => { clearTimeout(timer); resolve(); });
        ws.once("error", (err) => { clearTimeout(timer); reject(err); });
      });
      ws.on("message", (raw) => { try { events.push(JSON.parse(String(raw))); } catch { /* ignore */ } });
      // Settle signal seen by WS clients: assistant_run_end{status:"completed"}
      // (agent_settled is the internal Pi event the chat route consumes).
      const settledCount = () => events.filter((e) => e.type === "assistant_run_end" && e.status === "completed").length;
      const waitForSettled = async (target: number) => {
        const deadline = Date.now() + 90000;
        while (settledCount() < target) {
          if (Date.now() > deadline) throw new Error(`assistant_run_end wait timeout; last events: ${JSON.stringify(events.slice(-6))}`);
          await new Promise((r) => setTimeout(r, 150));
        }
      };

      ws.send(JSON.stringify({
        type: "prompt",
        clientMessageId: "p01-vertical-c1",
        snapshotVersion: 1,
        text: "请读取 slice-note.txt 并告诉我内容",
        sessionId: session.sessionId,
        sessionPath: session.path,
      }));
      await waitForSettled(1);

      // Witness saw the real provider exchange for the chat turn (startup
      // background calls like roster-description generation are excluded by
      // the user-text marker): C1 carries the user prompt, C2 (post-tool)
      // carries the read tool's result back to the model.
      const posts = witness.posts();
      expect(posts.some((p) => p.headers.authorization.includes(WITNESS_KEY))).toBe(true);
      const chatPosts = posts.filter((p) => JSON.stringify(p.bodyJson).includes("请读取 slice-note.txt"));
      expect(chatPosts.length).toBeGreaterThanOrEqual(2);
      expect(JSON.stringify(chatPosts[1].bodyJson)).toContain(FILE_MARKER);

      // The WS event stream carried the tool round-trip.
      expect(events.filter((e) => String(e.type).startsWith("tool_")).length).toBeGreaterThanOrEqual(2);

      // History read-back over HTTP contains the user turn and the assistant reply.
      const history = await fetch(
        `${base}/api/sessions/messages?sessionId=${encodeURIComponent(session.sessionId)}&all=1`,
        { headers: authHeaders },
      );
      expect(history.status).toBe(200);
      const historyText = JSON.stringify(await history.json());
      expect(historyText).toContain("slice-note.txt");
      expect(historyText).toContain("P01_VERTICAL_ASSISTANT_REPLY");

      // ── A10: desktop-channel notify in a headless process does not block/crash;
      //        a follow-up plain chat turn completes normally afterwards. ──
      expect((await fetch(`${base}/api/health`, { headers: authHeaders })).status).toBe(200);
      ws.send(JSON.stringify({
        type: "prompt",
        clientMessageId: "p01-vertical-notify",
        snapshotVersion: 1,
        text: "请给我发一条桌面通知",
        sessionId: session.sessionId,
        sessionPath: session.path,
      }));
      await waitForSettled(2);
      expect((await fetch(`${base}/api/health`, { headers: authHeaders })).status).toBe(200);

      ws.send(JSON.stringify({
        type: "prompt",
        clientMessageId: "p01-vertical-plain",
        snapshotVersion: 1,
        text: "普通聊天继续",
        sessionId: session.sessionId,
        sessionPath: session.path,
      }));
      await waitForSettled(3);

      expect(witness.posts().filter((p) => JSON.stringify(p.bodyJson).includes("普通聊天继续")).length).toBeGreaterThanOrEqual(1);
      const finalHistory = await fetch(
        `${base}/api/sessions/messages?sessionId=${encodeURIComponent(session.sessionId)}&all=1`,
        { headers: authHeaders },
      );
      expect(finalHistory.status).toBe(200);
      expect(JSON.stringify(await finalHistory.json())).toContain("P01_PLAIN_CHAT_REPLY");

      ws.close();
    } finally {
      child.kill("SIGTERM");
      await waitForExit(child);
      await witness.close();
      fs.rmSync(lingxiHome, TEMP_HOME_RM_OPTIONS);
      fs.rmSync(workspaceDir, TEMP_HOME_RM_OPTIONS);
      if (process.env.LINGXI_TEST_DEBUG) process.stderr.write(stderr);
    }
  }, 300000);
});

// ---------------------------------------------------------------------------
// Part 5 — P02 server restart cycle (P02-T05/T06; scenario P02-A15).
//
// Controlled temp HOME pre-seeded with a pending background-task record, then
// a real start → second-writer refusal (same-home mutex) → SIGTERM stop →
// real restart. Proves: pending records do not crash startup; no second
// writer shares the HOME; the stopped port is released (no orphan listener);
// server-info.json stale lock self-cleans and the pending record survives the
// restart untouched (recovery rules reproducible; unit-level recovering
// semantics covered by tests/p02-recovery-restart.test.ts).
// ---------------------------------------------------------------------------

describe("composition restart cycle: pending record + second-writer refusal + clean rebind (P02-A15)", () => {
  function spawnRestartCycleServer(lingxiHome: string) {
    return spawn(process.execPath, ["server/bootstrap.ts"], {
      cwd: root,
      env: {
        ...process.env,
        LINGXI_HOME: lingxiHome,
        LINGXI_PORT: "0",
        LINGXI_ROOT: root,
        LINGXI_SERVER_ENTRY: path.join(root, "server", "main-full.ts"),
        LINGXI_CREATE_STARTUP_SESSION: "0",
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
  }

  async function assertHealthOk(port: number, token: string) {
    const res = await fetch(`http://127.0.0.1:${port}/api/health`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(res.status).toBe(200);
    const body: any = await res.json();
    expect(body.status).toBe("ok");
  }

  async function assertPortRefused(port: number) {
    await expect(
      fetch(`http://127.0.0.1:${port}/api/health`, { headers: { Authorization: "Bearer x" } }),
    ).rejects.toThrow();
  }

  it("starts with a pending task record, refuses a second writer, releases the port on SIGTERM, and restarts cleanly", async () => {
    const lingxiHome = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p02-restart-"));
    const serverInfoPath = path.join(lingxiHome, "server-info.json");
    const pendingTasksPath = path.join(lingxiHome, ".ephemeral", "plugin-tasks.json");

    fs.mkdirSync(path.dirname(pendingTasksPath), { recursive: true });
    const pendingTaskId = "task_probe_pending_1";
    fs.writeFileSync(pendingTasksPath, JSON.stringify({
      tasks: [{
        taskId: pendingTaskId,
        type: "p02-restart-probe",
        parentSessionId: null,
        parentSessionPath: null,
        status: "running",
        attempt: 1,
        createdAt: Date.now(),
        updatedAt: Date.now(),
      }],
      schedules: [],
    }, null, 2), "utf8");

    const first = spawnRestartCycleServer(lingxiHome);
    let firstStderr = "";
    first.stderr.on("data", (chunk) => { firstStderr += chunk; });
    try {
      const info1 = await waitForServerInfo(serverInfoPath, first);
      await assertHealthOk(info1.port, info1.token);

      const second = spawnRestartCycleServer(lingxiHome);
      let secondStderr = "";
      second.stderr.on("data", (chunk) => { secondStderr += chunk; });
      await waitForExit(second, 30000);
      expect(second.exitCode).toBe(1);

      first.kill("SIGTERM");
      await waitForExit(first, 30000);
      expect(first.exitCode !== null || first.signalCode !== null).toBe(true);
      const port1 = info1.port;
      await assertPortRefused(port1);

      const third = spawnRestartCycleServer(lingxiHome);
      let thirdStderr = "";
      third.stderr.on("data", (chunk) => { thirdStderr += chunk; });
      try {
        const info3 = await waitForServerInfo(serverInfoPath, third);
        await assertHealthOk(info3.port, info3.token);
        const persisted = JSON.parse(fs.readFileSync(pendingTasksPath, "utf8"));
        const restored = (persisted.tasks || []).find((t: any) => t.taskId === pendingTaskId);
        expect(restored).toMatchObject({ taskId: pendingTaskId, status: "running" });
        if (info3.port !== port1) await assertPortRefused(port1);
      } finally {
        third.kill("SIGTERM");
        await waitForExit(third, 30000);
      }
    } finally {
      if (first.exitCode === null && first.signalCode === null) {
        first.kill("SIGKILL");
        await waitForExit(first, 15000);
      }
      await new Promise<void>((resolve) => fs.rm(lingxiHome, { ...TEMP_HOME_RM_OPTIONS }, () => resolve()));
      if (process.env.LINGXI_TEST_DEBUG) {
        process.stderr.write(firstStderr);
      }
    }
  }, 240000);
});

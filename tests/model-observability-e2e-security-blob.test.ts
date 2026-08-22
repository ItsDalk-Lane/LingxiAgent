/**
 * Phase 10 E2E Truth — 安全矩阵（S31 blob 面）/ Crash-Restart（S24）/
 * Query Truth（S29 cursor 遍历 + filter）/ Export Truth（S35）。
 *
 * 覆盖此前全仓缺口：getStoredBlob（Phase 9 blob exact route 后端）零测试、
 * blob 路由不在 route-security 测试、crash→query incomplete 链、bulk cursor
 * exact set、export 身份一致性。
 */
import fs from "node:fs";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { callText } from "../core/llm-client.ts";
import { beginObservedModelCall } from "../lib/llm/model-call-integration.ts";
import { installModelObservabilityPersistence } from "../lib/llm/model-observability-persistence.ts";
import { modelObservabilityBlobPathCandidates } from "../lib/llm/model-observability-blob-store.ts";
import { normalizeModelObservabilityAggregateQuery, normalizeModelObservabilityQuery } from "../lib/llm/model-observability-query-types.ts";
import {
  createScenarioHarness,
  flushAsync,
  openaiCompletionsJson,
  type ScenarioHarness,
} from "./helpers/model-observability-scenario-harness.ts";

const POISON_KEY = "sk-E2E-SEC-WITNESS-POISON-6bd4a19e";

let harness: ScenarioHarness;

beforeEach(async () => {
  harness = await createScenarioHarness();
});
afterEach(async () => {
  vi.useRealTimers();
  await harness.close();
  harness.cleanup();
  vi.unstubAllGlobals();
});

async function placeCall(provider: string, content: string) {
  harness.witness.scriptNext({ kind: "json", body: openaiCompletionsJson({ content }) });
  await callText({
    api: "openai-completions",
    apiKey: POISON_KEY,
    baseUrl: harness.witness.baseUrl,
    model: { id: "witness-model", provider } as any,
    systemPrompt: "S",
    messages: [{ role: "user", content }],
    usageContext: {
      source: { subsystem: provider === "provider-a" ? "memory" : "session", operation: "e2e", surface: "desktop", trigger: "user" },
      attribution: { kind: "agent", agentId: "agent-e2e" },
    },
  } as any);
  await flushAsync(2);
}

describe("E2E truth — blob exact route 真相（S31 缺口补齐）", () => {
  /**
   * 生产写侧 API（blobStore.writeBlobFile + insertBlobRow——与 coordinator
   * externalizer 同一函数）准备 fixture。真实链中 base64/FormData Blob 均为
   * externalized descriptor（Phase 7 §七十四 BOUNDED；durable matrix 的
   * 「audio→blob stored」措辞在 Release Acceptance 修正为 externalized）。
   */
  async function createRealBlob(): Promise<string> {
    const { openModelObservabilityDatabase } = await import("../lib/llm/model-observability-schema.ts");
    const { createModelObservabilityBlobStore, mintModelObservabilityBlobId } = await import("../lib/llm/model-observability-blob-store.ts");
    const db = openModelObservabilityDatabase(harness.dbPath);
    const blobStore = createModelObservabilityBlobStore({ lingxiHome: harness.lingxiHome, db });
    const blobId = mintModelObservabilityBlobId();
    const bytes = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==", "base64");
    expect(blobStore.writeBlobFile(blobId, bytes)).toBe(true);
    blobStore.insertBlobRow(blobId, bytes.byteLength, "image/png");
    // maintenance 只承诺保留有活引用的文件；路由 fixture 也必须满足这条生产
    // 不变量，不能依赖 GET 与后台 GC 谁先抢到事件循环。
    const capturedAt = new Date().toISOString();
    const payloadRecord = db.prepare(`
      INSERT INTO payload_records (
        call_id, kind, captured_at, visibility, fidelity, sanitization_status,
        redacted, truncated, degraded, payload_json
      ) VALUES (?, 'semantic_request', ?, 'metadata_only', 'metadata_only', 'none', 0, 0, 0, NULL)
    `).run(`mc_blob_route_${blobId}`, capturedAt);
    db.prepare(`
      INSERT INTO payload_blob_refs (payload_record_id, blob_id, created_at) VALUES (?, ?, ?)
    `).run(Number(payloadRecord.lastInsertRowid), blobId, capturedAt);
    db.close();
    return blobId;
  }

  it("route security：blobs GET/HEAD = LOCAL_ONLY；未登记 verb fail closed", async () => {
    const { authorizeHttpRoute } = await import("../server/http/route-security.ts");
    const localPrincipal = {
      kind: "local_user",
      credentialKind: "loopback_token",
      connectionKind: "local",
      scopes: ["chat", "resources", "tools"],
    };
    const remoteOwner = { kind: "desktop_owner" };
    for (const method of ["GET", "HEAD"]) {
      expect(authorizeHttpRoute({ method, path: "/api/model-observability/blobs/mb_abc12345", principal: remoteOwner as any }))
        .toMatchObject({ allowed: false, status: 403, error: "local_only_route" });
      expect(authorizeHttpRoute({ method, path: "/api/model-observability/blobs/mb_abc12345", principal: localPrincipal as any }))
        .toMatchObject({ allowed: true });
      expect(authorizeHttpRoute({ method, path: "/api/model-observability/blobs/mb_abc12345", principal: null }))
        .toMatchObject({ allowed: false });
    }
    expect(authorizeHttpRoute({ method: "POST", path: "/api/model-observability/blobs/mb_abc12345", principal: remoteOwner as any }))
      .toMatchObject({ allowed: false });
  });

  it("真实 blob：GET bytes roundtrip + HEAD 不读字节；invalid/traversal 全拒（§九十四）", async () => {
    const blobId = await createRealBlob();
    const route = harness.route();

    // GET：bytes roundtrip + 安全头
    const expectedBytes = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==", "base64");
    const getRes = await route.request(`/model-observability/blobs/${blobId}`);
    expect(getRes.status).toBe(200);
    expect(getRes.headers.get("cache-control")).toBe("no-store");
    expect(getRes.headers.get("x-content-type-options")).toBe("nosniff");
    const bytes = Buffer.from(await getRes.arrayBuffer());
    expect(bytes.equals(expectedBytes)).toBe(true);

    // HEAD：200 + 正确 length、无 body
    const headRes = await route.request(`/model-observability/blobs/${blobId}`, { method: "HEAD" });
    expect(headRes.status, await headRes.clone().text()).toBe(200);
    expect(Number(headRes.headers.get("content-length"))).toBe(bytes.byteLength);
    expect(await headRes.text()).toBe("");

    // invalid id / traversal / 超长：全部 400，不触磁盘
    const badIds = [
      "../etc/passwd",
      "%2e%2e%2fetc%2fpasswd",
      "..\\secret",
      "mb_",
      "mb_../../secret",
      `${"x".repeat(200)}`,
    ];
    for (const bad of badIds) {
      const res = await route.request(`/model-observability/blobs/${encodeURIComponent(bad)}`);
      expect(res.status).toBe(400);
      expect(await res.json()).toMatchObject({ code: "invalid_blob_id" });
    }
    // 合法形态但不存在的 id：404（不 500）
    const missing = await route.request(`/model-observability/blobs/mb_zzzzzzzz9999`);
    expect(missing.status).toBe(404);

    // 磁盘文件被删（测试专用破坏）：404 blob_missing，不 500（§一百二十九）
    const blobFile = modelObservabilityBlobPathCandidates(harness.lingxiHome, blobId)[0];
    expect(fs.existsSync(blobFile)).toBe(true);
    fs.rmSync(blobFile);
    const gone = await route.request(`/model-observability/blobs/${blobId}`);
    expect(gone.status).toBe(404);
    expect(await gone.json()).toMatchObject({ code: "blob_missing" });
  });

  it("64MB Blob：HEAD 只 stat，GET 流式读取时 event loop 仍可继续运行", async () => {
    // 性能 fixture（直写 blob 文件 + DB 行——非正常 scenario 模拟，§一百三十五
    // 允许的专项直写类别）：验证 HEAD 分支 includeBytes=false 走 statSync。
    const blobId = "mb_perf0000test";
    const storeDir = path.join(harness.lingxiHome, "model-observability", "blobs", "mb");
    fs.mkdirSync(storeDir, { recursive: true });
    const big = Buffer.alloc(64 * 1024 * 1024, 7);
    fs.writeFileSync(path.join(storeDir, `${blobId}.bin`), big);
    const { createRequire } = await import("node:module");
    const require = createRequire(import.meta.url);
    const Database = (require("better-sqlite3") as any).default ?? require("better-sqlite3");
    const db = new Database(harness.dbPath);
    db.prepare(`INSERT OR REPLACE INTO blob_objects (blob_id, byte_length, media_type, state, relative_path, created_at) VALUES (?, ?, ?, 'ready', ?, ?)`)
      .run(blobId, big.byteLength, "application/octet-stream", path.join("mb", `${blobId}.bin`), new Date().toISOString());
    db.close();

    const route = harness.route();
    const openSync = vi.spyOn(fs, "openSync");
    const readFileSync = vi.spyOn(fs, "readFileSync");
    const t0 = Date.now();
    const head = await route.request(`/model-observability/blobs/${blobId}`, { method: "HEAD" });
    const headMs = Date.now() - t0;
    expect(head.status).toBe(200);
    expect(Number(head.headers.get("content-length"))).toBe(64 * 1024 * 1024);
    // stat only：64MB HEAD 显著快于读全量（阈值宽松，防 CI 抖动）
    expect(headMs).toBeLessThan(1000);
    expect(openSync).not.toHaveBeenCalled();
    expect(readFileSync).not.toHaveBeenCalled();
    // GET 可读全量（不在此断言耗时；stall 阈值见 TRUTH_AUDIT §10）
    let timerTicks = 0;
    const timer = setInterval(() => { timerTicks += 1; }, 0);
    try {
      const get = await route.request(`/model-observability/blobs/${blobId}`);
      expect(get.status).toBe(200);
      expect((await get.arrayBuffer()).byteLength).toBe(64 * 1024 * 1024);
    } finally {
      clearInterval(timer);
    }
    expect(timerTicks).toBeGreaterThan(0);
    expect(openSync).toHaveBeenCalledTimes(1);
    expect(readFileSync).not.toHaveBeenCalled();
  });
});

describe("E2E truth — Crash/Restart（S24）", () => {
  it("provider request 已 durable、无 logical_call_end → 重启后 incomplete + interruptedByRestart，绝不 Error", async () => {
    // 真实 observer 集成点：crash 前只发 start/attempt/provider_request 事件
    const recorder = beginObservedModelCall({
      model: { provider: "witness-provider", modelId: "witness-model", api: "openai-completions" },
      source: { subsystem: "session", operation: "reply", surface: "desktop", trigger: "user" },
      attribution: { kind: "session", sessionId: "sess-crash" },
      details: { path: "callText" },
    });
    recorder.beginAttempt({ details: { attemptVisibility: "exact" } });
    recorder.providerRequestPrepared({ details: { protocol: "openai-completions" } });
    await flushAsync(3);
    harness.flush();
    // 模拟 crash：不 endLogicalCall，直接关 DB
    await harness.handle.close();

    // 重启：同一 LINGXI_HOME 重新安装（startup reconciliation）
    const restarted = installModelObservabilityPersistence({
      lingxiHome: harness.lingxiHome,
      policy: { enabled: true, persistPayloads: true, persistBlobs: true },
    });
    const query = (await import("../lib/llm/model-observability-query.ts")).createModelObservabilityQueryService({
      lingxiHome: harness.lingxiHome,
    });
    const normalized = normalizeModelObservabilityQuery({ filter: { terminalStatus: "incomplete" } });
    if (normalized.ok === false) throw new Error(normalized.error.message);
    const page = query.queryCalls(normalized.value);
    expect(page.ok).toBe(true);
    if (!page.ok) return;
    expect(page.value.calls).toHaveLength(1);
    const call = page.value.calls[0];
    // §六十二：terminal_status 保持 NULL（用户杀进程 ≠ Provider error，不伪造
    // 终态）；DTO 的 incomplete 由 terminalStatus=null + interruptedByRestart 表达。
    expect(call.terminalStatus).toBeNull();
    expect(call.interruptedByRestart).toBe(true);
    // payload pipeline：只有真实存在的 provider_request 语义（无 semantic_response card）
    const detail = query.queryCallDetail(call.callId);
    expect(detail.ok).toBe(true);
    if (detail.ok) {
      const kinds = detail.value.payloadRecords.map((r: any) => r.kind);
      expect(kinds).not.toContain("semantic_response");
    }
    await restarted.close();
  });
});

describe("E2E truth — Query Truth（S29）", () => {
  it("bulk cursor 全遍历 exact set + 独立 expected table（provider/subsystem/status filter）", async () => {
    const expected: Array<{ provider: string; subsystem: string; status: string }> = [];
    // 40 条 provider-a/memory + 40 条 provider-b/session；同秒内多条（fake Date 制造同时间戳）
    vi.useFakeTimers({ now: Date.now(), toFake: ["Date"] });
    for (let i = 0; i < 40; i++) {
      await placeCall("provider-a", `E2E_QA_${i}`);
      expected.push({ provider: "provider-a", subsystem: "memory", status: "ok" });
    }
    for (let i = 0; i < 40; i++) {
      await placeCall("provider-b", `E2E_QB_${i}`);
      expected.push({ provider: "provider-b", subsystem: "session", status: "ok" });
    }
    vi.useRealTimers();
    harness.flush();
    await flushAsync(3);

    const query = harness.query();
    // 独立 expected：80 calls；provider-a filter → 40；memory → 40；provider-a+memory → 40；provider-b+memory → 0
    const runCalls = async (filter: Record<string, unknown>) => {
      const normalized = normalizeModelObservabilityQuery({ filter });
      if (normalized.ok === false) throw new Error(normalized.error.message);
      const collected: string[] = [];
      let cursor: string | null = null;
      // limit=7 → 多页翻页（80/7 ≈ 12 页）
      for (;;) {
        const result = query.queryCalls(cursor ? { ...normalized.value, cursor, limit: 7 } : { ...normalized.value, limit: 7 });
        if (result.ok === false) throw new Error("query failed");
        for (const call of result.value.calls) collected.push(call.callId);
        cursor = result.value.nextCursor;
        if (!cursor) break;
      }
      return collected;
    };
    const all = await runCalls({});
    expect(all).toHaveLength(80);
    expect(new Set(all).size).toBe(80); // 无重复
    const a = await runCalls({ provider: "provider-a" });
    expect(a).toHaveLength(40);
    const mem = await runCalls({ subsystem: "memory" });
    expect(mem).toHaveLength(40);
    const aMem = await runCalls({ provider: "provider-a", subsystem: "memory" });
    expect(aMem).toHaveLength(40);
    // 集合等式：provider-a 的 40 个恰是 memory 的 40 个（同批构造）
    expect(new Set(a)).toEqual(new Set(mem));
    const observerIds = new Set(harness.observer!.callIds());
    expect(observerIds.size).toBe(80);
    for (const id of all) expect(observerIds.has(id)).toBe(true); // 无遗漏

    // aggregate 与独立 expected 一致（§七十七）
    const aggNorm = normalizeModelObservabilityAggregateQuery({
      groupBy: ["provider"],
    });
    if (aggNorm.ok === false) throw new Error(aggNorm.error.message);
    const agg = query.queryAggregate(aggNorm.value);
    expect(agg.ok).toBe(true);
    if (agg.ok) {
      const byProvider = new Map(agg.value.groups.map((g: any) => [g.values.provider, g.metrics]));
      expect(byProvider.get("provider-a")?.callCount).toBe(40);
      expect(byProvider.get("provider-b")?.callCount).toBe(40);
      expect(agg.value.overall.callCount).toBe(80);
    }
  }, 60_000);
});

describe("E2E truth — Export Truth（S35）", () => {
  it("真实 export：manifest + bundle 身份 ≡ query；filter exact；默认 metadata-only", async () => {
    await placeCall("provider-a", "E2E_EXPORT_A1");
    await placeCall("provider-a", "E2E_EXPORT_A2");
    await placeCall("provider-b", "E2E_EXPORT_B1");
    harness.flush();
    await flushAsync(3);

    const route = harness.route();
    const res = await route.request("/model-observability/export", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ query: { filter: { provider: "provider-a" } } }),
    });
    expect(res.status).toBe(200);
    const bodyText = await res.text();
    const lines = bodyText.split("\n").filter((line) => line.trim());
    const manifest = JSON.parse(lines[0]);
    expect(manifest.exportSchemaVersion).toBe(1);
    expect(manifest.includePayloads).toBe(false);
    expect(manifest.totalCalls).toBe(2);
    const bundles = lines.slice(1).map((line) => JSON.parse(line));
    expect(bundles).toHaveLength(2);
    // filter exact：零 provider-b（§一百一十）
    for (const bundle of bundles) {
      expect(bundle.call.model.provider).toBe("provider-a");
      expect(JSON.stringify(bundle)).not.toContain("E2E_EXPORT_B1");
      // 身份 ≡ query（§一百一十一）
      const normalized = normalizeModelObservabilityQuery({ filter: { callId: bundle.call.callId } });
      if (normalized.ok === false) throw new Error(normalized.error.message);
      const page = harness.query().queryCalls(normalized.value);
      if (page.ok) expect(page.value.calls[0].callId).toBe(bundle.call.callId);
      // metadata-only：正文零出现（§一百一十二）
      expect(JSON.stringify(bundle)).not.toContain("E2E_EXPORT_A");
    }
  });
});

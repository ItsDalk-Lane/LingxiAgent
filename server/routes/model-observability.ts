/**
 * server/routes/model-observability.ts — Model Observatory HTTP surface
 * （Phase 8 §六十三～七十二）。
 *
 * 与 Usage Ledger 是不同领域，独立于 server/routes/usage.ts（§六十三）。
 * 全部查询经 engine 的统一 Query Service / 控制面（§十七/五十八）：route
 * 不拼 SQL、不直接操作 observer/sink/SQLite。复杂 query 用 POST JSON body
 * （多值 filter + groupBy + cursor + date bucket；read-only operation，§六十五），
 * body 严格 normalize——unknown field / invalid enum / oversized array /
 * invalid date / invalid cursor 显式 400（§六十六）。
 *
 * 权限在 server/http/route-security.ts 显式登记（§六十七）：metadata 查询
 * STUDIO_OWNER；payload 正文 / settings mutation / export = LOCAL_ONLY。
 */

import { Hono } from "hono";
import fs from "node:fs";
import { Readable } from "node:stream";
import {
  normalizeModelObservabilityAggregateQuery,
  normalizeModelObservabilityQuery,
  normalizeModelObservabilityTraceQuery,
} from "../../lib/llm/model-observability-query-types.ts";
import {
  normalizeModelObservabilityExportOptions,
  startModelObservabilityExport,
} from "../../lib/llm/model-observability-export.ts";
import { MODEL_OBSERVABILITY_BLOB_SAFE_MEDIA_MAJOR } from "../../shared/model-observability-api-contract.ts";

function badRequest(c: any, error: { code: string; message: string; field?: string }) {
  return c.json({ error: "invalid_query", code: error.code, message: error.message, field: error.field ?? null }, 400);
}

function serviceError(c: any, error: { code: string; message: string; reasonCode?: string | null }) {
  if (error.code === "absent") {
    // §九十三 No-Store UX：不是 500 ENOENT，而是显式状态。
    return c.json({ error: "not_initialized", code: "absent", message: "model observability store has not been created" }, 404);
  }
  if (error.code === "invalid_cursor") {
    return c.json({ error: "invalid_cursor", code: "invalid_cursor", message: error.message }, 400);
  }
  if (error.code === "invalid_blob_id") {
    return c.json({ error: "invalid_blob_id", code: "invalid_blob_id", message: error.message }, 400);
  }
  if (error.code === "blob_missing") {
    // §一百二十九：DB ref 在但文件缺失 → 显式 404 blob_missing（不是 500）。
    return c.json({ error: "blob_missing", code: "blob_missing", message: error.message }, 404);
  }
  if (error.code === "not_found") {
    return c.json({ error: "not_found", code: "not_found", message: error.message }, 404);
  }
  return c.json({ error: "query_failed", code: error.code, message: error.message }, 500);
}

/* ── Stored blob 响应头（§一百二十七/一百二十八）─────────────────────────
 *
 *   - Cache-Control: no-store；X-Content-Type-Options: nosniff。
 *   - Content-Type 只来自保存的 media_type 且经安全校验：只允许
 *     image/audio/video 主类型 + 合法 token 形态；其余一律
 *     application/octet-stream（绝不 text/html / 可执行形态）。
 *   - 永不返回 relative_path / absolute_path / LINGXI_HOME。
 */
function safeBlobContentType(mediaType: string | null): string {
  if (typeof mediaType !== "string") return "application/octet-stream";
  const match = /^([a-z]+)\/([a-z0-9.+-]+)$/i.exec(mediaType.trim());
  if (!match) return "application/octet-stream";
  const major = match[1].toLowerCase();
  if (!(MODEL_OBSERVABILITY_BLOB_SAFE_MEDIA_MAJOR as readonly string[]).includes(major)) {
    return "application/octet-stream";
  }
  return `${major}/${match[2].toLowerCase()}`;
}

export function createModelObservabilityRoute(engine: any) {
  const route = new Hono();

  /* ── health / settings（控制面，§四十九/五十八～六十二）────────────── */

  route.get("/model-observability/health", (c) => {
    return c.json(engine.getModelObservabilityHealth());
  });

  route.get("/model-observability/settings", (c) => {
    return c.json(engine.getModelObservabilitySettings());
  });

  route.put("/model-observability/settings", async (c) => {
    let body: unknown;
    try {
      body = await c.req.json();
    } catch {
      return c.json({ error: "invalid_json", message: "request body must be JSON" }, 400);
    }
    if (!body || typeof body !== "object" || Array.isArray(body)) {
      return c.json({ error: "invalid_json", message: "request body must be a JSON object" }, 400);
    }
    const source = body as Record<string, unknown>;
    const allowed = new Set(["enabled", "persistTraceMetadata", "persistPayloads", "persistBlobs", "retention"]);
    for (const key of Object.keys(source)) {
      if (!allowed.has(key)) {
        return badRequest(c, { code: "unknown_field", message: `unknown settings field "${key}"`, field: key });
      }
    }
    if (source.retention !== undefined
      && (source.retention === null || typeof source.retention !== "object" || Array.isArray(source.retention))) {
      return badRequest(c, { code: "invalid_filter", message: "retention must be an object", field: "retention" });
    }
    const result = await engine.setModelObservabilitySettings(source);
    return c.json(result);
  });

  /* ── query（POST JSON body；read-only，§六十五）───────────────────── */

  route.post("/model-observability/query/calls", async (c) => {
    let body: unknown;
    try {
      body = await c.req.json();
    } catch {
      return c.json({ error: "invalid_json", message: "request body must be JSON" }, 400);
    }
    const normalized = normalizeModelObservabilityQuery(body);
    if (normalized.ok === false) return badRequest(c, normalized.error);
    const result = engine.getModelObservabilityQueryService().queryCalls(normalized.value);
    if (result.ok === false) return serviceError(c, result.error);
    return c.json(result.value);
  });

  route.post("/model-observability/query/traces", async (c) => {
    let body: unknown;
    try {
      body = await c.req.json();
    } catch {
      return c.json({ error: "invalid_json", message: "request body must be JSON" }, 400);
    }
    const normalized = normalizeModelObservabilityTraceQuery(body);
    if (normalized.ok === false) return badRequest(c, normalized.error);
    const result = engine.getModelObservabilityQueryService().queryTraces(normalized.value);
    if (result.ok === false) return serviceError(c, result.error);
    return c.json(result.value);
  });

  route.post("/model-observability/query/aggregate", async (c) => {
    let body: unknown;
    try {
      body = await c.req.json();
    } catch {
      return c.json({ error: "invalid_json", message: "request body must be JSON" }, 400);
    }
    const normalized = normalizeModelObservabilityAggregateQuery(body);
    if (normalized.ok === false) return badRequest(c, normalized.error);
    const result = engine.getModelObservabilityQueryService().queryAggregate(normalized.value);
    if (result.ok === false) return serviceError(c, result.error);
    return c.json(result.value);
  });

  /* ── drill-down detail（§三十二/三十）────────────────────────────── */

  route.get("/model-observability/calls/:callId", (c) => {
    const result = engine.getModelObservabilityQueryService().queryCallDetail(c.req.param("callId"));
    if (result.ok === false) return serviceError(c, result.error);
    return c.json(result.value);
  });

  route.get("/model-observability/traces/:traceId", (c) => {
    const result = engine.getModelObservabilityQueryService().queryTraceDetail(c.req.param("traceId"));
    if (result.ok === false) return serviceError(c, result.error);
    return c.json(result.value);
  });

  /** payload metadata（正文不默认 inline，§三十四）。 */
  route.get("/model-observability/calls/:callId/payloads", (c) => {
    const detail = engine.getModelObservabilityQueryService().queryCallDetail(c.req.param("callId"));
    if (detail.ok === false) return serviceError(c, detail.error);
    return c.json({ callId: detail.value.call.callId, payloadRecords: detail.value.payloadRecords });
  });

  /** exact payload retrieval（§三十五；权限比 metadata 严：LOCAL_ONLY）。 */
  route.get("/model-observability/payloads/:recordId", (c) => {
    const recordId = Number(c.req.param("recordId"));
    if (!Number.isInteger(recordId) || recordId <= 0 || recordId > Number.MAX_SAFE_INTEGER) {
      return badRequest(c, { code: "invalid_filter", message: "payload record id must be a positive integer", field: "recordId" });
    }
    const result = engine.getModelObservabilityQueryService().getPayloadRecord(recordId);
    if (result.ok === false) return serviceError(c, result.error);
    return c.json(result.value);
  });

  /* ── export（§七十三～八十二；LOCAL_ONLY；JSONL streaming）────────── */

  route.post("/model-observability/export", async (c) => {
    let body: unknown;
    try {
      body = await c.req.json();
    } catch {
      return c.json({ error: "invalid_json", message: "request body must be JSON" }, 400);
    }
    const source = body && typeof body === "object" && !Array.isArray(body) ? body as Record<string, unknown> : {};
    const options = normalizeModelObservabilityExportOptions(source);
    if (options.ok === false) return badRequest(c, options.error);
    const normalizedQuery = normalizeModelObservabilityQuery(source.query ?? {});
    if (normalizedQuery.ok === false) return badRequest(c, normalizedQuery.error);
    const start = startModelObservabilityExport(
      engine.getModelObservabilityQueryService(),
      normalizedQuery.value,
      options.value,
    );
    if (start.kind === "absent") {
      return c.json({ error: "not_initialized", code: "absent", message: "model observability store has not been created" }, 404);
    }
    if (start.kind === "limit_error") {
      return c.json({
        error: "export_limit",
        code: "limit_error",
        message: `matched ${start.matchedCalls} calls exceeds maxCalls ${start.maxCalls}; narrow the filter or raise maxCalls (≤100000)`,
        matchedCalls: start.matchedCalls,
        maxCalls: start.maxCalls,
      }, 413);
    }
    if (start.kind === "unavailable") {
      return c.json({ error: "query_failed", code: "unavailable", message: "model observability store is not readable" }, 500);
    }
    const iterator = start.iterate();
    const stream = new ReadableStream<Uint8Array>({
      async pull(controller) {
        try {
          const { value, done } = await iterator.next();
          if (done) {
            controller.close();
            return;
          }
          controller.enqueue(new TextEncoder().encode(value));
        } catch (error) {
          controller.error(error);
        }
      },
      async cancel() {
        try { await iterator.return?.(undefined); } catch { /* best-effort */ }
      },
    });
    return new Response(stream, {
      headers: {
        "content-type": "application/x-ndjson; charset=utf-8",
        "cache-control": "no-store",
        "x-export-schema-version": String(start.manifest.exportSchemaVersion),
      },
    });
  });

  /* ── stored blob exact retrieval（Phase 9 §一百一十九～一百三十一）─────
   *
   * 唯一 blob 面：GET/HEAD /blobs/:blobId（LOCAL_ONLY，route-security 登记）。
   * exact id 寻址；无 list/search/path 参数；绝不自动访问外部引用。
   * HEAD 供 UI 懒加载探测 content-type/length（不下载字节）。
   */

  const blobHandler = (c: any, includeBody: boolean) => {
    const blobId = c.req.param("blobId");
    // GET 与 HEAD 都只让 query service 做 exact-id metadata/stat 探测；正文由
    // createReadStream 分块输出，不能把 64MB 文件同步装进一个 Buffer。
    const result = engine.getModelObservabilityQueryService().getStoredBlob(blobId);
    if (result.ok === false) return serviceError(c, result.error);
    const blob = result.value;
    const headers: Record<string, string> = {
      "content-type": safeBlobContentType(blob.mediaType),
      "content-length": String(blob.byteLength),
      "cache-control": "no-store",
      "x-content-type-options": "nosniff",
    };
    if (!includeBody) {
      return new Response(null, { status: 200, headers });
    }
    try {
      // 先打开文件描述符，避免 stat 与真正开始读取之间的删除竞态；不会把正文
      // 同步读进内存。autoClose 会在流结束或失败时释放描述符。
      const fileDescriptor = fs.openSync(blob.filePath, "r");
      const nodeStream = fs.createReadStream(blob.filePath, { fd: fileDescriptor, autoClose: true });
      const stream = Readable.toWeb(nodeStream) as ReadableStream<Uint8Array>;
      return new Response(stream, { status: 200, headers });
    } catch {
      return serviceError(c, { code: "blob_missing", message: "blob file is missing on disk" });
    }
  };

  // Hono 的 GET 路由也会兜底匹配 HEAD。显式 HEAD 负责正常分流，GET
  // handler 再按实际方法兜底，确保框架匹配顺序变化时也不会打开正文文件。
  route.on("HEAD", "/model-observability/blobs/:blobId", (c) => blobHandler(c, false));
  route.get("/model-observability/blobs/:blobId", (c) => blobHandler(c, c.req.method !== "HEAD"));

  return route;
}

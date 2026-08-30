import { Hono } from "hono";
import { bodyLimit } from "hono/body-limit";

import { KnowledgeError, isKnowledgeError } from "../../lib/knowledge/errors.ts";
import type {
  ContentSnapshot,
  KnowledgeNotebook,
  KnowledgeParseArtifact,
  KnowledgeSource,
} from "../../lib/knowledge/types.ts";
import { safeJson } from "../hono-helpers.ts";
import { createRequestContext } from "../http/boundary.ts";
import { isLocalOwnerPrincipal, isStudioOwnerPrincipal } from "../http/route-security.ts";

const knowledgeMetadataBodyLimit = bodyLimit({
  maxSize: 64 * 1024,
  onError: (c) => c.json({
    error: "KNOWLEDGE_INVALID_ARGUMENT",
    message: "Knowledge request body is too large",
  }, 413),
});
const knowledgeSourceBodyLimit = bodyLimit({
  maxSize: 64 * 1024 * 1024,
  onError: (c) => c.json({
    error: "KNOWLEDGE_IMPORT_TOO_LARGE",
    message: "Knowledge source request body is too large",
  }, 413),
});

function errorStatus(error: KnowledgeError): number {
  switch (error.code) {
    case "KNOWLEDGE_NOT_FOUND":
      return 404;
    case "KNOWLEDGE_CONFLICT":
      return 409;
    case "KNOWLEDGE_IMPORT_PATH_BLOCKED":
    case "KNOWLEDGE_WEB_URL_BLOCKED":
      return 403;
    case "KNOWLEDGE_IMPORT_TOO_LARGE":
    case "KNOWLEDGE_WEB_TOO_LARGE":
      return 413;
    case "KNOWLEDGE_PARSE_FAILED":
    case "KNOWLEDGE_PARSE_NOT_READY":
    case "KNOWLEDGE_IMPORT_TYPE_UNSUPPORTED":
    case "KNOWLEDGE_SCOPE_NOT_READY":
    case "KNOWLEDGE_RETRIEVAL_EMPTY":
    case "KNOWLEDGE_WEB_TYPE_UNSUPPORTED":
      return 422;
    case "KNOWLEDGE_MODEL_OUTPUT_INVALID":
    case "KNOWLEDGE_WEB_FETCH_FAILED":
      return 502;
    case "KNOWLEDGE_MODEL_UNAVAILABLE":
    case "KNOWLEDGE_RETRIEVAL_UNAVAILABLE":
    case "KNOWLEDGE_INDEX_INVALID":
      return 503;
    case "KNOWLEDGE_STORAGE_INVALID":
    case "KNOWLEDGE_SCHEMA_NEWER":
      return 500;
    default:
      return 400;
  }
}

function routeError(c: any, error: unknown) {
  if (isKnowledgeError(error)) {
    return c.json({
      error: error.code,
      message: error.message,
      ...(Object.keys(error.details).length > 0 ? { details: error.details } : {}),
    }, errorStatus(error) as any);
  }
  return c.json({
    error: "KNOWLEDGE_INTERNAL_ERROR",
    message: "Knowledge request failed",
  }, 500);
}

function bindKnowledgeScope(c: any, engine: any) {
  const requestContext = createRequestContext(c, engine);
  const runtimeStudioId = typeof requestContext.runtimeContext?.studioId === "string"
    ? requestContext.runtimeContext.studioId.trim()
    : "";
  const principalStudioId = typeof requestContext.authPrincipal?.studioId === "string"
    ? requestContext.authPrincipal.studioId.trim()
    : "";
  if (!runtimeStudioId || !engine?.knowledge) {
    return {
      error: c.json({
        error: "KNOWLEDGE_UNAVAILABLE",
        message: "Knowledge is unavailable for this Studio",
      }, 503),
    };
  }
  if (!isStudioOwnerPrincipal(requestContext.authPrincipal)) {
    return {
      error: c.json({
        error: "KNOWLEDGE_OWNER_REQUIRED",
        message: "Studio owner access is required",
      }, 403),
    };
  }
  if (!principalStudioId || principalStudioId !== runtimeStudioId) {
    return {
      error: c.json({
        error: "KNOWLEDGE_STUDIO_MISMATCH",
        message: "Authenticated Studio does not match this server Studio",
      }, 403),
    };
  }
  return {
    requestContext,
    studioId: runtimeStudioId,
    knowledge: engine.knowledge,
    error: null,
  };
}

function publicOriginMetadata(source: KnowledgeSource) {
  const metadata = source.originMetadata || {};
  let publicUrl: string | undefined;
  const storedUrl = typeof metadata.originalUrl === "string"
    ? metadata.originalUrl
    : metadata.finalUrl;
  if (typeof storedUrl === "string") {
    try {
      const parsed = new URL(storedUrl);
      parsed.search = "";
      parsed.hash = "";
      publicUrl = parsed.href;
    } catch {
      // 损坏的来源元数据不会直接透传给客户端。
    }
  }
  return {
    ...(typeof metadata.kind === "string" ? { kind: metadata.kind } : {}),
    ...(typeof metadata.fileName === "string" ? { fileName: metadata.fileName } : {}),
    ...(publicUrl ? { url: publicUrl } : {}),
    ...(typeof metadata.fetchedAt === "string" ? { fetchedAt: metadata.fetchedAt } : {}),
  };
}

function serializeSource(source: KnowledgeSource) {
  return {
    ...source,
    originMetadata: publicOriginMetadata(source),
  };
}

function serializeSnapshot(snapshot: ContentSnapshot) {
  const { storagePath: _storagePath, ...safe } = snapshot;
  return safe;
}

function serializeArtifact(artifact: KnowledgeParseArtifact | null) {
  if (!artifact) return null;
  const { semanticArtifactPath: _semanticArtifactPath, ...safe } = artifact;
  return safe;
}

function validateExactBody(body: unknown, allowedKeys: string[], label: string): Record<string, any> {
  if (!body || typeof body !== "object" || Array.isArray(body)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${label} body must be an object`);
  }
  const record = body as Record<string, any>;
  if (
    Object.keys(record).length !== allowedKeys.length
    || allowedKeys.some(key => !Object.hasOwn(record, key))
  ) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${label} body fields are invalid`);
  }
  return record;
}

/**
 * 笔记本设置 PUT 的键校验：omitted=不变，至少给一个键。chunkTargetChars 已
 * 随"按嵌入模型上下文自动分块"退役（遗留显式列值仍生效，只是不再接受写入）。
 */
const NOTEBOOK_SETTINGS_KEYS = ["embeddingModelRef", "rerankModelRef", "retrievalTopK", "vectorRetentionDays"];

function validateSettingsBody(body: unknown): Record<string, any> {
  if (!body || typeof body !== "object" || Array.isArray(body)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Notebook settings body must be an object");
  }
  const record = body as Record<string, any>;
  const keys = Object.keys(record);
  if (keys.length === 0 || keys.some(key => !NOTEBOOK_SETTINGS_KEYS.includes(key))) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Notebook settings body fields are invalid");
  }
  return record;
}

/**
 * 笔记本就绪汇总：按每个源的最新摄入 job 归类（done / pending_embedding /
 * 处理中 / failed / 无 job），供引用菜单与知识页徽章使用。
 */
function summarizeNotebookIngestion(knowledge: any, studioId: string, notebookId: string) {
  const entries = knowledge.listNotebookSources({ studioId, notebookId });
  const ingestion = { done: 0, pendingEmbedding: 0, processing: 0, failed: 0, untracked: 0 };
  for (const entry of entries) {
    // notebookId 过滤：一源多笔记本时按本笔记本的 job 归类，避免错记到
    // 其他笔记本的摄入状态上。
    const latest = knowledge.getLatestIngestionJobForSource({
      studioId,
      sourceId: entry.source.id,
      notebookId,
    });
    if (!latest) {
      ingestion.untracked += 1;
    } else if (latest.status === "done") {
      ingestion.done += 1;
    } else if (latest.status === "pending_embedding") {
      ingestion.pendingEmbedding += 1;
    } else if (latest.status === "failed") {
      ingestion.failed += 1;
    } else {
      ingestion.processing += 1;
    }
  }
  return { sourceCount: entries.length, ingestion };
}

export function createKnowledgeRoute(engine: any) {
  const route = new Hono();

  route.get("/knowledge/notebooks", (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const notebooks = scope.knowledge.listNotebooks({ studioId: scope.studioId })
        .map((notebook: KnowledgeNotebook) => ({
          ...notebook,
          config: scope.knowledge.getNotebookConfig({ studioId: scope.studioId, notebookId: notebook.id }),
          // 生效分块尺寸（遗留显式列 > 嵌入模型上下文 ×80% 自动值），设置弹窗只读展示。
          chunkTargetCharsEffective: scope.knowledge.getNotebookEffectiveChunkTargetChars({
            studioId: scope.studioId,
            notebookId: notebook.id,
          }),
          ...summarizeNotebookIngestion(scope.knowledge, scope.studioId, notebook.id),
        }));
      return c.json({ notebooks });
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.post("/knowledge/notebooks", knowledgeMetadataBodyLimit, async (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const body = validateExactBody(await safeJson(c), ["name"], "Notebook create");
      const notebook = scope.knowledge.createNotebook({ studioId: scope.studioId, name: body?.name });
      return c.json({ notebook }, 201);
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.get("/knowledge/notebooks/:id", (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const notebook = scope.knowledge.getNotebook({
        studioId: scope.studioId,
        notebookId: c.req.param("id"),
      });
      return c.json({ notebook });
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.patch("/knowledge/notebooks/:id", knowledgeMetadataBodyLimit, async (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const body = validateExactBody(await safeJson(c), ["name"], "Notebook rename");
      const notebook = scope.knowledge.renameNotebook({
        studioId: scope.studioId,
        notebookId: c.req.param("id"),
        name: body?.name,
      });
      return c.json({ notebook });
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.delete("/knowledge/notebooks/:id", (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const notebook = scope.knowledge.deleteNotebook({
        studioId: scope.studioId,
        notebookId: c.req.param("id"),
      });
      return c.json({ notebook });
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.put("/knowledge/notebooks/:id/settings", knowledgeMetadataBodyLimit, async (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const body = validateSettingsBody(await safeJson(c));
      // 数值范围与模型引用完整性由 store 层 updateNotebookConfig 统一校验；
      // 分块尺寸/嵌入模型引用变化触发的全量重建在 manager 内完成。
      const config = scope.knowledge.updateNotebookSettings({
        studioId: scope.studioId,
        notebookId: c.req.param("id"),
        ...body,
      });
      return c.json({ config });
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.get("/knowledge/ingestion", (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const notebookId = c.req.query("notebookId") || undefined;
      const sourceId = c.req.query("sourceId") || undefined;
      const jobs = scope.knowledge.listIngestionJobs({ studioId: scope.studioId, notebookId, sourceId });
      const counts = scope.knowledge.countIngestionJobsByStatus({ studioId: scope.studioId, notebookId });
      // Phase 6 文件 watch 的"源文件不可达"显式状态顺带暴露（只列不可达项，不含可达噪音）。
      const unreachableSources = scope.knowledge.listSourceFileWatchStates()
        .filter((state: any) => state.unreachable && state.studioId === scope.studioId);
      return c.json({ jobs, counts, unreachableSources });
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.get("/knowledge/notebooks/:id/sources", (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const sources = scope.knowledge.listNotebookSources({
        studioId: scope.studioId,
        notebookId: c.req.param("id"),
      }).map((entry: any) => ({
        source: serializeSource(entry.source),
        snapshot: serializeSnapshot(entry.snapshot),
        membership: entry.membership,
        parseArtifact: serializeArtifact(entry.parseArtifact),
      }));
      return c.json({ sources });
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.post("/knowledge/notebooks/:id/sources", knowledgeSourceBodyLimit, async (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const body = await safeJson(c);
      const kind = body?.kind || (body?.filePath ? "file" : null);
      const allowedByKind = {
        file: new Set(["kind", "filePath", "displayName"]),
        pasted_text: new Set(["kind", "text", "displayName"]),
        web_snapshot: new Set(["kind", "url", "displayName"]),
      } as const;
      const allowed = allowedByKind[kind as keyof typeof allowedByKind];
      if (!allowed || !body || typeof body !== "object" || Array.isArray(body)
        || Object.keys(body).some(key => !(allowed as ReadonlySet<string>).has(key))) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge source import body is invalid");
      }
      let imported: any;
      if (kind === "file") {
        if (!isLocalOwnerPrincipal(scope.requestContext.authPrincipal)) {
          return c.json({
            error: "KNOWLEDGE_LOCAL_IMPORT_REQUIRED",
            message: "Importing a server file path requires the local Studio owner",
          }, 403);
        }
        imported = await scope.knowledge.importFile({
          studioId: scope.studioId,
          notebookId: c.req.param("id"),
          filePath: body.filePath,
          displayName: body.displayName,
        });
      } else if (kind === "pasted_text") {
        imported = await scope.knowledge.importPastedText({
          studioId: scope.studioId,
          notebookId: c.req.param("id"),
          text: body.text,
          displayName: body.displayName,
        });
      } else {
        imported = await scope.knowledge.importWebSnapshot({
          studioId: scope.studioId,
          notebookId: c.req.param("id"),
          url: body.url,
          displayName: body.displayName,
        });
      }
      let parseArtifact: any;
      try {
        parseArtifact = await scope.knowledge.parseSource({
          studioId: scope.studioId,
          sourceId: imported.source.id,
        });
      } catch (error) {
        // 解析失败也保证有摄入 job：worker 从 parse 相位重试、超限标 failed
        // （显式终态，可手动重试），不允许源静默无摄入状态。
        scope.knowledge.enqueueSourceIngestion({
          studioId: scope.studioId,
          notebookId: c.req.param("id"),
          sourceId: imported.source.id,
        });
        throw error;
      }
      // 导入+解析完成即入队摄入（chunk→fts_index→embed 异步执行），HTTP 立即返回。
      scope.knowledge.enqueueSourceIngestion({
        studioId: scope.studioId,
        notebookId: c.req.param("id"),
        sourceId: imported.source.id,
        artifactId: parseArtifact.id,
      });
      return c.json({
        source: serializeSource(imported.source),
        snapshot: serializeSnapshot(imported.snapshot),
        membership: imported.membership,
        parseArtifact: serializeArtifact(parseArtifact),
      }, 201);
    } catch (error) {
      return routeError(c, error);
    }
  });

  // §六十九 目录导入：local-owner 限定（与 file 导入同级），服务端递归展开目录、
  // sha 去重、Membership 写目录组织路径；逐源 parse + 入队摄入，部分失败显式留痕。
  route.post("/knowledge/notebooks/:id/import-directory", knowledgeSourceBodyLimit, async (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      if (!isLocalOwnerPrincipal(scope.requestContext.authPrincipal)) {
        return c.json({
          error: "KNOWLEDGE_LOCAL_IMPORT_REQUIRED",
          message: "Importing a server directory requires the local Studio owner",
        }, 403);
      }
      const body = await safeJson(c);
      const allowed = new Set(["dirPath"]);
      if (!body || typeof body !== "object" || Array.isArray(body)
        || Object.keys(body).some(key => !allowed.has(key))) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Directory import body is invalid");
      }
      const result = await scope.knowledge.importDirectory({
        studioId: scope.studioId,
        notebookId: c.req.param("id"),
        dirPath: body.dirPath,
      });
      const imported = [];
      for (const entry of result.imported) {
        try {
          const parseArtifact = await scope.knowledge.parseSource({
            studioId: scope.studioId,
            sourceId: entry.sourceId,
          });
          scope.knowledge.enqueueSourceIngestion({
            studioId: scope.studioId,
            notebookId: c.req.param("id"),
            sourceId: entry.sourceId,
            artifactId: parseArtifact.id,
          });
          imported.push({ ...entry, ingestion: "enqueued" });
        } catch {
          // 解析失败也保证有摄入 job：worker 从 parse 相位重试（显式终态，不静默）。
          scope.knowledge.enqueueSourceIngestion({
            studioId: scope.studioId,
            notebookId: c.req.param("id"),
            sourceId: entry.sourceId,
          });
          imported.push({ ...entry, ingestion: "parse_failed_enqueued_for_retry" });
        }
      }
      return c.json({ imported, skipped: result.skipped, failed: result.failed }, 201);
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.delete("/knowledge/notebooks/:id/sources/:sourceId", (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const membership = scope.knowledge.removeSourceFromNotebook({
        studioId: scope.studioId,
        notebookId: c.req.param("id"),
        sourceId: c.req.param("sourceId"),
      });
      return c.json({ membership });
    } catch (error) {
      return routeError(c, error);
    }
  });

  // Phase 5（§十九 delete wins）：显式删除源——取消该源全部活跃摄入 job（running
  // 经 abort 收尾）、清理派生索引变体与全部物理痕迹；并发 reingest 在删除标记后
  // 显式失败，被取消 job 不可重试。活跃 turn scope 冻结引用时 409 拒绝。
  route.delete("/knowledge/sources/:id", async (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const result = await scope.knowledge.deleteSource({
        studioId: scope.studioId,
        sourceId: c.req.param("id"),
      });
      return c.json({
        source: serializeSource(result.source),
        cancelledJobs: result.cancelledJobs,
        purge: result.purge,
      });
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.post("/knowledge/notebooks/:id/sources/:sourceId/refresh", async (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    if (!isLocalOwnerPrincipal(scope.requestContext.authPrincipal)) {
      return c.json({
        error: "KNOWLEDGE_LOCAL_IMPORT_REQUIRED",
        message: "Refreshing a local file source requires the local Studio owner",
      }, 403);
    }
    try {
      const refreshed = await scope.knowledge.refreshFileSource({
        studioId: scope.studioId,
        notebookId: c.req.param("id"),
        sourceId: c.req.param("sourceId"),
      });
      return c.json({
        source: serializeSource(refreshed.source),
        snapshot: serializeSnapshot(refreshed.snapshot),
        membership: refreshed.membership,
        parseArtifact: serializeArtifact(refreshed.parseArtifact),
        changed: refreshed.changed,
      });
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.post("/knowledge/notebooks/:id/sources/:sourceId/reingest", async (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      // failed 手动重试（requeue + 唤醒队列）；无 job 时兜底入队；
      // 最新 job 非 failed 时抛 KNOWLEDGE_CONFLICT（409）。
      const result = scope.knowledge.requeueSourceIngestion({
        studioId: scope.studioId,
        notebookId: c.req.param("id"),
        sourceId: c.req.param("sourceId"),
      });
      return c.json(result);
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.get("/knowledge/parse-artifacts/:id/blocks", (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const blocks = scope.knowledge.listArtifactBlocks({
        studioId: scope.studioId,
        parseArtifactId: c.req.param("id"),
      });
      return c.json({ blocks });
    } catch (error) {
      return routeError(c, error);
    }
  });

  // 冻结契约：{ chunkerConfigId, chunks: [{ id, ordinal(1-based), text,
  // tokenCount, charCount, headingPath?, pageNumber? }] }。
  route.get("/knowledge/parse-artifacts/:id/chunks", (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const result = scope.knowledge.listArtifactChunkCards({
        studioId: scope.studioId,
        parseArtifactId: c.req.param("id"),
      });
      return c.json(result);
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.get("/knowledge/citations/:citationId", (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const resolved = scope.knowledge.resolveCitation({
        studioId: scope.studioId,
        citationId: c.req.param("citationId"),
      });
      return c.json({
        citation: resolved.citation,
        block: resolved.block,
        parseArtifact: serializeArtifact(resolved.artifact),
        snapshot: serializeSnapshot(resolved.snapshot),
        source: serializeSource(resolved.source),
        viewer: {
          contentUrl: `/api/knowledge/snapshots/${encodeURIComponent(resolved.snapshot.id)}/content`,
          locator: resolved.block.locator,
        },
      });
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.get("/knowledge/snapshots/:snapshotId/content", (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const snapshot = scope.knowledge.store.getContentSnapshot({
        studioId: scope.studioId,
        snapshotId: c.req.param("snapshotId"),
      });
      const bytes = scope.knowledge.readContentSnapshot({
        studioId: scope.studioId,
        snapshotId: snapshot.id,
      });
      // 冻结网页原文只作为证据字节保存，绝不能在 Lingxi 自身源下执行其中脚本。
      c.header("Content-Type", snapshot.mimeType === "text/html"
        ? "text/plain; charset=utf-8"
        : snapshot.mimeType);
      c.header("Content-Length", String(bytes.length));
      c.header("Cache-Control", "private, immutable");
      c.header("ETag", `\"${snapshot.sha256}\"`);
      c.header("X-Content-Type-Options", "nosniff");
      c.header("Content-Security-Policy", "sandbox; default-src 'none'");
      return c.body(new Uint8Array(bytes));
    } catch (error) {
      return routeError(c, error);
    }
  });

  return route;
}

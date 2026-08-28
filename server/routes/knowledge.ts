import { Hono } from "hono";
import { bodyLimit } from "hono/body-limit";

import { KnowledgeError, isKnowledgeError } from "../../lib/knowledge/errors.ts";
import type {
  ContentSnapshot,
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
    case "KNOWLEDGE_RESEARCH_INCOMPLETE":
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

function serializeRunResult(knowledge: any, studioId: string, run: any) {
  const scope = knowledge.getScopeSnapshot({ studioId, scopeSnapshotId: run.scopeSnapshotId });
  const citations = run.citations.map((ref: any) => {
    const resolved = knowledge.resolveCitation({ studioId, citationId: ref.citationId });
    return {
      marker: ref.marker,
      citation: resolved.citation,
      source: serializeSource(resolved.source),
      snapshot: serializeSnapshot(resolved.snapshot),
      parseArtifact: serializeArtifact(resolved.artifact),
      locator: resolved.block.locator,
      viewer: {
        contentUrl: `/api/knowledge/snapshots/${encodeURIComponent(resolved.snapshot.id)}/content`,
        locator: resolved.block.locator,
      },
    };
  });
  return { run, scope, citations };
}

function validateQueryBody(body: any) {
  if (!body || typeof body !== "object" || Array.isArray(body)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge query body must be an object");
  }
  const allowed = new Set(["question", "notebookIds", "mode"]);
  if (Object.keys(body).some(key => !allowed.has(key))) {
    throw new KnowledgeError(
      "KNOWLEDGE_INVALID_ARGUMENT",
      "Knowledge query accepts only question, notebookIds, and mode",
    );
  }
  if (body.mode !== "quick" && body.mode !== "research") {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge query mode is invalid");
  }
  return body;
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

export function createKnowledgeRoute(engine: any) {
  const route = new Hono();

  route.get("/knowledge/notebooks", (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      return c.json({ notebooks: scope.knowledge.listNotebooks({ studioId: scope.studioId }) });
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
      const parseArtifact = await scope.knowledge.parseSource({
        studioId: scope.studioId,
        sourceId: imported.source.id,
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

  route.post("/knowledge/query", knowledgeMetadataBodyLimit, async (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const body = validateQueryBody(await safeJson(c));
      if (body.mode === "research") {
        const result = await scope.knowledge.startResearch({
          studioId: scope.studioId,
          notebookIds: body.notebookIds,
          question: body.question,
        });
        return c.json({
          run: result.run,
          scope: result.scope,
          research: result.research,
          citations: [],
        }, 202);
      }
      const result = await scope.knowledge.runQuickAnswer({
        studioId: scope.studioId,
        notebookIds: body.notebookIds,
        question: body.question,
      });
      return c.json({
        ...serializeRunResult(scope.knowledge, scope.studioId, result.run),
        retrievalBasis: result.retrievalBasis,
      }, 201);
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.get("/knowledge/runs", (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const runs = scope.knowledge.listActiveResearchRuns({ studioId: scope.studioId })
        .map((research: any) => {
          const run = scope.knowledge.getKnowledgeRun({
            studioId: scope.studioId,
            runId: research.runId,
          });
          return {
            ...serializeRunResult(scope.knowledge, scope.studioId, run),
            research,
          };
        });
      return c.json({ runs });
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.get("/knowledge/runs/:runId", (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const run = scope.knowledge.getKnowledgeRun({
        studioId: scope.studioId,
        runId: c.req.param("runId"),
      });
      const serialized = serializeRunResult(scope.knowledge, scope.studioId, run);
      if (run.mode !== "research") return c.json(serialized);
      return c.json({
        ...serialized,
        research: scope.knowledge.getResearchRun({
          studioId: scope.studioId,
          runId: run.id,
        }),
      });
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.post("/knowledge/runs/:runId/cancel", (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const research = scope.knowledge.cancelResearch({
        studioId: scope.studioId,
        runId: c.req.param("runId"),
      });
      const run = scope.knowledge.getKnowledgeRun({
        studioId: scope.studioId,
        runId: research.runId,
      });
      return c.json({ run, research });
    } catch (error) {
      return routeError(c, error);
    }
  });

  route.get("/knowledge/runs/:runId/report", (c) => {
    const scope = bindKnowledgeScope(c, engine);
    if (scope.error) return scope.error;
    try {
      const report = scope.knowledge.getResearchReport({
        studioId: scope.studioId,
        runId: c.req.param("runId"),
      });
      const citations = report.citations.map((ref: any) => {
        const resolved = scope.knowledge.resolveCitation({
          studioId: scope.studioId,
          citationId: ref.citationId,
        });
        return {
          marker: ref.marker,
          evidenceId: ref.evidenceId,
          citation: resolved.citation,
          source: serializeSource(resolved.source),
          snapshot: serializeSnapshot(resolved.snapshot),
          parseArtifact: serializeArtifact(resolved.artifact),
          locator: resolved.block.locator,
          viewer: {
            contentUrl: `/api/knowledge/snapshots/${encodeURIComponent(resolved.snapshot.id)}/content`,
            locator: resolved.block.locator,
          },
        };
      });
      return c.json({ report, citations });
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

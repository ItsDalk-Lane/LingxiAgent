import { Hono } from "hono";
import { LocalModelError } from "../../lib/local-models/index.ts";
import { readAuthPrincipal } from "../http/capability-guard.ts";
import { isLocalOwnerPrincipal } from "../http/route-security.ts";
import { recordSecurityAuditEvent } from "../http/security-audit.ts";
import { safeJson } from "../hono-helpers.ts";

const CATEGORIES = new Set(["embedding", "ocr", "stt", "tts"]);

export function createLocalModelsRoute(engine: any) {
  const route = new Hono();

  route.use("/local-models/*", async (c, next) => {
    if (!isLocalOwnerPrincipal(readAuthPrincipal(c))) return c.json({ error: "local_only_route" }, 403);
    return next();
  });

  route.get("/local-models", async (c) => {
    try {
      return c.json(await requireSubsystem(engine).state());
    } catch (error) {
      return errorResponse(c, error);
    }
  });

  route.put("/local-models/config", async (c) => {
    try {
      const config = await requireSubsystem(engine).setConfig(await safeJson(c));
      recordSecurityAuditEvent(c, engine, {
        action: "settings.localModels.update",
        target: "localModels",
        metadata: { backend: config.backend, downloadConcurrency: config.download.concurrency },
      });
      return c.json({ ok: true, config });
    } catch (error) {
      return errorResponse(c, error);
    }
  });

  route.post("/local-models/manifest/refresh", async (c) => {
    try {
      const result = await requireSubsystem(engine).refreshManifest({ signal: c.req.raw.signal });
      return c.json({
        source: result.source,
        version: result.manifest?.manifestVersion ?? null,
        updatedAt: result.manifest?.updatedAt ?? null,
        warning: result.warning,
      });
    } catch (error) {
      return errorResponse(c, error);
    }
  });

  route.post("/local-models/install", async (c) => {
    try {
      const body = await safeJson(c);
      const modelId = safeId(body?.modelId, "modelId");
      const quant = safeId(body?.quant, "quant");
      const installed = await requireSubsystem(engine).install(modelId, quant, { signal: c.req.raw.signal });
      recordSecurityAuditEvent(c, engine, {
        action: "localModels.install",
        target: `${modelId}@${quant}`,
        metadata: { category: installed.category, bytes: installed.bytes },
      });
      return c.json({ ok: true, model: publicInstalled(installed) }, 201);
    } catch (error) {
      return errorResponse(c, error);
    }
  });

  route.post("/local-models/import", async (c) => {
    try {
      const body = await safeJson(c);
      if (typeof body?.directory !== "string" || !body.directory.trim()) throw new Error("directory is required");
      const selection = body?.modelId === undefined && body?.quant === undefined
        ? undefined
        : { modelId: safeId(body?.modelId, "modelId"), quant: safeId(body?.quant, "quant") };
      const installed = await requireSubsystem(engine).importDirectory(body.directory, {
        signal: c.req.raw.signal,
        selection,
      });
      recordSecurityAuditEvent(c, engine, {
        action: "localModels.import",
        target: `${installed.id}@${installed.quant}`,
        metadata: { category: installed.category, integrity: installed.integrity, bytes: installed.bytes },
      });
      return c.json({ ok: true, model: publicInstalled(installed) }, 201);
    } catch (error) {
      return errorResponse(c, error);
    }
  });

  route.post("/local-models/import/inspect", async (c) => {
    try {
      const body = await safeJson(c);
      if (typeof body?.directory !== "string" || !body.directory.trim()) throw new Error("directory is required");
      return c.json(await requireSubsystem(engine).inspectImportDirectory(body.directory, { signal: c.req.raw.signal }));
    } catch (error) {
      return errorResponse(c, error);
    }
  });

  route.post("/local-models/downloads/:taskId/pause", async (c) => {
    try {
      const taskId = safeTaskId(c.req.param("taskId"));
      const paused = requireSubsystem(engine).pauseDownload(taskId);
      return paused ? c.json({ ok: true }) : c.json({ error: "download_task_not_active" }, 404);
    } catch (error) {
      return errorResponse(c, error);
    }
  });

  route.delete("/local-models/downloads/:taskId", async (c) => {
    try {
      const taskId = safeTaskId(c.req.param("taskId"));
      const cancelled = await requireSubsystem(engine).cancelDownload(taskId);
      if (!cancelled) return c.json({ error: "download_task_not_found" }, 404);
      recordSecurityAuditEvent(c, engine, {
        action: "localModels.download.cancel",
        target: taskId,
        metadata: {},
      });
      return c.json({ ok: true });
    } catch (error) {
      return errorResponse(c, error);
    }
  });

  route.delete("/local-models/models/:category/:id/:quant", async (c) => {
    try {
      const category = c.req.param("category");
      if (!CATEGORIES.has(category)) throw new Error("invalid model category");
      const id = safeId(c.req.param("id"), "id");
      const quant = safeId(c.req.param("quant"), "quant");
      const removed = await requireSubsystem(engine).remove(category as "embedding" | "ocr" | "stt" | "tts", id, quant, {
        signal: c.req.raw.signal,
      });
      if (!removed) return c.json({ error: "local_model_not_found" }, 404);
      recordSecurityAuditEvent(c, engine, {
        action: "localModels.remove",
        target: `${id}@${quant}`,
        metadata: { category },
      });
      return c.json({ ok: true });
    } catch (error) {
      return errorResponse(c, error);
    }
  });

  route.get("/local-models/models/:category/:id/:quant/license", async (c) => {
    try {
      const category = c.req.param("category");
      if (!CATEGORIES.has(category)) throw new Error("invalid model category");
      const id = safeId(c.req.param("id"), "id");
      const quant = safeId(c.req.param("quant"), "quant");
      const content = await requireSubsystem(engine).readLicense(category, id, quant);
      return c.json({ content });
    } catch (error) {
      return errorResponse(c, error);
    }
  });

  return route;
}

function requireSubsystem(engine: any) {
  if (!engine?.localModels) throw new LocalModelError("LOCAL_MODEL_UNSUPPORTED", "local model subsystem is unavailable");
  return engine.localModels;
}

function safeId(value: unknown, field: string): string {
  if (typeof value !== "string" || !/^[a-z0-9][a-z0-9._-]{0,127}$/.test(value)) {
    throw new Error(`${field} is invalid`);
  }
  return value;
}

function safeTaskId(value: string): string {
  if (!/^[a-f0-9]{64}$/.test(value)) throw new Error("taskId is invalid");
  return value;
}

function publicInstalled(installed: any) {
  return {
    id: installed.id,
    category: installed.category,
    quant: installed.quant,
    version: installed.version,
    tier: installed.tier,
    runtimeId: installed.runtimeId,
    runtimeVersion: installed.runtimeVersion,
    runtimeKind: installed.runtimeKind,
    source: installed.source,
    installedAt: installed.installedAt,
    bytes: installed.bytes,
    integrity: installed.integrity,
    licenseAvailable: Boolean(installed.licenseFile),
  };
}

function errorResponse(c: any, error: unknown) {
  const local = error instanceof LocalModelError ? error : null;
  const status = local?.code === "LOCAL_MODEL_NOT_INSTALLED" ? 404
    : local?.code === "LOCAL_MODEL_ALREADY_INSTALLED" ? 409
      : local?.code === "LOCAL_MODEL_MEMORY_INSUFFICIENT" || local?.code === "LOCAL_MODEL_DISK_SPACE" ? 507
        : local?.code === "LOCAL_MODEL_ABORTED" ? 499
          : 400;
  return c.json({
    error: local?.code ?? "LOCAL_MODEL_REQUEST_INVALID",
    message: error instanceof Error ? error.message : String(error),
    ...(local ? { details: local.details } : {}),
  }, status);
}

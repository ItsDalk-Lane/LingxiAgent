import { Hono } from "hono";
import { describe, expect, it, vi } from "vitest";
import { createLocalModelsRoute } from "../server/routes/local-models.ts";

const localOwner = Object.freeze({
  kind: "local_user",
  credentialKind: "loopback_token",
  connectionKind: "local",
  scopes: ["chat"],
});

const remoteOwner = Object.freeze({
  kind: "device",
  credentialKind: "device_credential",
  connectionKind: "lan",
  scopes: ["studio.owner", "settings.read", "settings.write"],
});

function appWith(engine: any, principal: object) {
  const app = new Hono();
  app.use("*", async (c, next) => {
    (c as any).set("authPrincipal", principal);
    await next();
  });
  app.route("/api", createLocalModelsRoute(engine));
  return app;
}

describe("local models route", () => {
  it("returns sanitized subsystem state to the local owner", async () => {
    const state = {
      manifest: { source: "builtin", configured: false, warning: "not configured" },
      catalog: [],
      installed: [],
      rejected: [],
      instances: [],
      downloads: [],
    };
    const engine = { localModels: { state: vi.fn(async () => state) } };
    const response = await appWith(engine, localOwner).request("/api/local-models");
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual(state);
    expect(engine.localModels.state).toHaveBeenCalledTimes(1);
  });

  it("denies even a remote studio owner before touching the subsystem", async () => {
    const engine = { localModels: { state: vi.fn(async () => ({})) } };
    const response = await appWith(engine, remoteOwner).request("/api/local-models");
    expect(response.status).toBe(403);
    expect(await response.json()).toEqual({ error: "local_only_route" });
    expect(engine.localModels.state).not.toHaveBeenCalled();
  });

  it("validates model ids and task ids before dispatch", async () => {
    const engine = {
      localModels: {
        install: vi.fn(),
        pauseDownload: vi.fn(),
      },
    };
    const app = appWith(engine, localOwner);
    const install = await app.request("/api/local-models/install", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ modelId: "../escape", quant: "q4" }),
    });
    expect(install.status).toBe(400);
    expect(await install.json()).toMatchObject({ error: "LOCAL_MODEL_REQUEST_INVALID" });
    expect(engine.localModels.install).not.toHaveBeenCalled();

    const pause = await app.request("/api/local-models/downloads/not-a-task/pause", { method: "POST" });
    expect(pause.status).toBe(400);
    expect(engine.localModels.pauseDownload).not.toHaveBeenCalled();
  });

  it("maps structured local model conflicts without exposing implementation paths", async () => {
    const engine = {
      localModels: {
        install: vi.fn(async () => {
          const error = new (await import("../lib/local-models/index.ts")).LocalModelError(
            "LOCAL_MODEL_ALREADY_INSTALLED",
            "already installed",
            { id: "sensevoice-small" },
          );
          throw error;
        }),
      },
    };
    const response = await appWith(engine, localOwner).request("/api/local-models/install", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ modelId: "sensevoice-small", quant: "int8" }),
    });
    expect(response.status).toBe(409);
    expect(await response.json()).toEqual({
      error: "LOCAL_MODEL_ALREADY_INSTALLED",
      message: "already installed",
      details: { id: "sensevoice-small" },
    });
  });

  it("inspects a manual directory before importing and forwards only validated selections", async () => {
    const inspectImportDirectory = vi.fn(async () => ({ hasModelJson: false, candidates: [] }));
    const importDirectory = vi.fn(async () => ({
      id: "sensevoice-small", category: "stt", quant: "int8", version: "manual-abc",
      tier: "small", runtimeId: "sherpa-onnx", runtimeVersion: "1", runtimeKind: "in-process",
      source: "manual", installedAt: "2026-09-02T00:00:00.000Z", bytes: 1, integrity: "unknown",
    }));
    const app = appWith({ localModels: { inspectImportDirectory, importDirectory } }, localOwner);
    const inspected = await app.request("/api/local-models/import/inspect", {
      method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ directory: "/tmp/model" }),
    });
    expect(inspected.status).toBe(200);
    expect(inspectImportDirectory).toHaveBeenCalledWith("/tmp/model", expect.objectContaining({ signal: expect.any(AbortSignal) }));

    const imported = await app.request("/api/local-models/import", {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ directory: "/tmp/model", modelId: "sensevoice-small", quant: "int8" }),
    });
    expect(imported.status).toBe(201);
    expect(importDirectory).toHaveBeenCalledWith("/tmp/model", expect.objectContaining({
      signal: expect.any(AbortSignal),
      selection: { modelId: "sensevoice-small", quant: "int8" },
    }));
  });

  it("returns only the declared license text for an installed model", async () => {
    const readLicense = vi.fn(async () => "Apache License fixture");
    const response = await appWith({ localModels: { readLicense } }, localOwner).request(
      "/api/local-models/models/stt/sensevoice-small/int8/license",
    );
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ content: "Apache License fixture" });
    expect(readLicense).toHaveBeenCalledWith("stt", "sensevoice-small", "int8");
  });
});

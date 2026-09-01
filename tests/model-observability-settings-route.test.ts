import { Hono } from "hono";
import { describe, expect, it, vi } from "vitest";
import { createModelObservabilityRoute } from "../server/routes/model-observability.ts";

function makeApp(engine: Record<string, unknown>) {
  const app = new Hono();
  app.route("/api", createModelObservabilityRoute(engine));
  return app;
}

describe("Model Observatory 设置接口固定全开契约", () => {
  it("旧客户端提交 false 时明确拒绝，且不静默写入", async () => {
    const setModelObservabilitySettings = vi.fn();
    const app = makeApp({ setModelObservabilitySettings });
    const response = await app.request("/api/model-observability/settings", {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ persistPayloads: false }),
    });

    expect(response.status).toBe(409);
    expect(await response.json()).toMatchObject({
      error: "immutable_setting",
      code: "immutable_setting",
      field: "persistPayloads",
    });
    expect(setModelObservabilitySettings).not.toHaveBeenCalled();
  });

  it("旧客户端提交 true 可兼容忽略，只保存保留天数", async () => {
    const result = { desired: { enabled: true } };
    const setModelObservabilitySettings = vi.fn(async () => result);
    const app = makeApp({ setModelObservabilitySettings });
    const retention = { traceDays: 120, payloadDays: 20, blobDays: 10 };
    const response = await app.request("/api/model-observability/settings", {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        enabled: true,
        persistTraceMetadata: true,
        persistPayloads: true,
        persistBlobs: true,
        retention,
      }),
    });

    expect(response.status).toBe(200);
    expect(setModelObservabilitySettings).toHaveBeenCalledWith({ retention });
    expect(await response.json()).toEqual(result);
  });
});

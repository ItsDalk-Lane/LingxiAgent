import { describe, expect, it, vi } from "vitest";
import { withModelRequestAccounting } from "../lib/llm/model-request-accounting.ts";

describe("withModelRequestAccounting", () => {
  it("closes a successful request in the shared usage ledger", async () => {
    const usageLedger = {
      start: vi.fn(() => ({ requestId: "req-media-1" })),
      finish: vi.fn(),
      recordError: vi.fn(),
    };

    await expect(withModelRequestAccounting({
      usageLedger,
      model: { provider: "openai", modelId: "gpt-image-1", api: "openai-images" },
      usageContext: {
        source: { subsystem: "media", operation: "submit", surface: "tool", trigger: "user" },
        attribution: { kind: "session", sessionId: "session-1" },
      },
      metadata: { taskId: "task-1" },
    }, async () => ({ taskId: "provider-task-1" }))).resolves.toEqual({ taskId: "provider-task-1" });

    expect(usageLedger.start).toHaveBeenCalledWith(expect.objectContaining({
      model: { provider: "openai", modelId: "gpt-image-1", api: "openai-images" },
      metadata: { taskId: "task-1" },
    }));
    expect(usageLedger.finish).toHaveBeenCalledWith("req-media-1", expect.objectContaining({ usage: null }));
    expect(usageLedger.recordError).not.toHaveBeenCalled();
  });

  it("records a failed request and rethrows the original error", async () => {
    const failure = new Error("provider unavailable");
    const usageLedger = {
      start: vi.fn(() => ({ requestId: "req-media-2" })),
      finish: vi.fn(),
      recordError: vi.fn(),
    };

    await expect(withModelRequestAccounting({
      usageLedger,
      model: { provider: "jimeng-cli", modelId: "jimeng-4", api: "external-cli" },
      usageContext: {
        source: { subsystem: "media", operation: "submit", surface: "plugin", trigger: "user" },
        attribution: { kind: "external-boundary", providerId: "jimeng-cli" },
      },
    }, async () => { throw failure; })).rejects.toBe(failure);

    expect(usageLedger.recordError).toHaveBeenCalledWith("req-media-2", failure, "error", expect.any(Object));
    expect(usageLedger.finish).not.toHaveBeenCalled();
  });

  it("records a non-success response without exposing the provider body", async () => {
    const usageLedger = {
      start: vi.fn(() => ({ requestId: "req-probe-failed" })),
      finish: vi.fn(),
      recordError: vi.fn(),
    };

    await expect(withModelRequestAccounting({ usageLedger }, async () => ({
      ok: false,
      status: 401,
      error: "SECRET provider response",
    }))).resolves.toMatchObject({ ok: false, status: 401 });

    expect(usageLedger.recordError).toHaveBeenCalledWith(
      "req-probe-failed",
      expect.objectContaining({ message: "model request returned non-success status 401" }),
      "error",
      expect.any(Object),
    );
    expect(JSON.stringify(usageLedger.recordError.mock.calls)).not.toContain("SECRET");
    expect(usageLedger.finish).not.toHaveBeenCalled();
  });

  it("does not require a ledger", async () => {
    await expect(withModelRequestAccounting({}, async () => "ok")).resolves.toBe("ok");
  });
});

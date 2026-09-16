import { describe, it, expect } from "vitest";
import {
  MODEL_CATALOG,
  resolveModelId,
  getKnownModels,
  getDefaultModelId,
} from "../core/media-adapters/model-catalog.ts";

describe("MODEL_CATALOG", () => {
  it("contains volcengine, openai, and openai-codex-oauth providers", () => {
    expect(MODEL_CATALOG).toHaveProperty("volcengine");
    expect(MODEL_CATALOG).toHaveProperty("openai");
    expect(MODEL_CATALOG).toHaveProperty("openai-codex-oauth");
  });

  it("every entry has id and name", () => {
    for (const [, models] of Object.entries(MODEL_CATALOG)) {
      for (const m of models) {
        expect(m.id).toBeTruthy();
        expect(m.name).toBeTruthy();
      }
    }
  });

  it("has no more than one explicit default per provider", () => {
    for (const [, models] of Object.entries(MODEL_CATALOG)) {
      expect(models.filter(m => (m as any).default).length).toBeLessThanOrEqual(1);
    }
  });
});

describe("resolveModelId", () => {
  it("returns exact match when given a full model ID", () => {
    expect(resolveModelId("volcengine", "doubao-seedream-3-0-t2i"))
      .toBe("doubao-seedream-3-0-t2i");
    expect(resolveModelId("openai", "gpt-image-1.5"))
      .toBe("gpt-image-1.5");
    expect(resolveModelId("openai", "gpt-image-2"))
      .toBe("gpt-image-2");
  });

  it("resolves short aliases to full IDs", () => {
    expect(resolveModelId("volcengine", "5.0"))
      .toBe("doubao-seedream-5-0-260128");
    expect(resolveModelId("volcengine", "3.0"))
      .toBe("doubao-seedream-3-0-t2i");
    expect(resolveModelId("volcengine", "4.5"))
      .toBe("doubao-seedream-4-5-251128");
  });

  it("resolves OpenAI short aliases", () => {
    expect(resolveModelId("openai", "1.5"))
      .toBe("gpt-image-1.5");
    expect(resolveModelId("openai", "mini"))
      .toBe("gpt-image-1-mini");
    expect(resolveModelId("openai", "2"))
      .toBe("gpt-image-2");
  });

  it("is case-insensitive for aliases", () => {
    expect(resolveModelId("volcengine", "5.0-LITE"))
      .toBe("doubao-seedream-5-0-lite-260128");
  });

  it("falls back to the provider default model when input is null/undefined", () => {
    expect(resolveModelId("volcengine", null))
      .toBe("doubao-seedream-5-0-lite-260128");
    expect(resolveModelId("volcengine", undefined))
      .toBe("doubao-seedream-5-0-lite-260128");
    expect(resolveModelId("openai", null))
      .toBe("gpt-image-1.5");
  });

  it("passes newly released full IDs through without substituting the old default", () => {
    expect(resolveModelId("volcengine", "doubao-seedream-future-20270101")).toBe("doubao-seedream-future-20270101");
    expect(resolveModelId("openai", "gpt-image-future")).toBe("gpt-image-future");
    expect(resolveModelId("openai-codex-oauth", "gpt-image-future")).toBe("gpt-image-future");
  });

  it("rejects unknown version aliases with an explicit request for a full model ID", () => {
    expect(() => resolveModelId("volcengine", "999.0-pro"))
      .toThrow(/Unknown image model alias.*999.0-pro.*full model ID/i);
  });

  it("returns empty string for unknown providers with no raw value", () => {
    expect(resolveModelId("unknown-provider", null)).toBe("");
  });

  it("returns raw string for unknown providers with a raw value", () => {
    expect(resolveModelId("unknown-provider", "some-model")).toBe("some-model");
  });
});

describe("getKnownModels", () => {
  it("includes both official Seedream 5.0 variants", () => {
    expect(getKnownModels("volcengine")).toEqual(expect.arrayContaining([
      { id: "doubao-seedream-5-0-lite-260128", name: "Seedream 5.0 Lite" },
      { id: "doubao-seedream-5-0-260128", name: "Seedream 5.0" },
    ]));
  });

  it("returns [{id, name}] without aliases", () => {
    const models = getKnownModels("volcengine");
    expect(models.length).toBeGreaterThan(0);
    for (const m of models) {
      expect(m).toHaveProperty("id");
      expect(m).toHaveProperty("name");
      expect(m).not.toHaveProperty("aliases");
    }
  });

  it("returns empty array for unknown providers", () => {
    expect(getKnownModels("nonexistent")).toEqual([]);
  });
});

describe("getDefaultModelId", () => {
  it("returns the provider default model", () => {
    expect(getDefaultModelId("volcengine")).toBe("doubao-seedream-5-0-lite-260128");
    expect(getDefaultModelId("openai")).toBe("gpt-image-1.5");
  });

  it("returns null for unknown providers", () => {
    expect(getDefaultModelId("nonexistent")).toBeNull();
  });
});

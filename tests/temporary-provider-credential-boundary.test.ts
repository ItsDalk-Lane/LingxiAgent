import { describe, expect, it, vi } from "vitest";
import { createTemporaryProviderCredentialBoundary } from "../core/temporary-provider-credential-boundary.ts";

describe("temporary provider credential boundary", () => {
  it("只允许指定 Provider 和用途消费一次，且审计不含秘密", () => {
    const audit = vi.fn();
    const boundary = createTemporaryProviderCredentialBoundary({
      providerId: "draft-provider",
      source: "request-draft",
      operation: "connectivity-probe",
      apiKey: "draft-secret",
      headers: { Authorization: "Bearer draft-secret" },
      audit,
    });

    expect(JSON.stringify(boundary)).not.toContain("draft-secret");
    expect(boundary.consume({
      providerId: "draft-provider",
      operation: "connectivity-probe",
    })).toEqual({
      apiKey: "draft-secret",
      headers: { Authorization: "Bearer draft-secret" },
    });
    expect(() => boundary.consume({
      providerId: "draft-provider",
      operation: "connectivity-probe",
    })).toThrow(/already consumed/i);
    expect(JSON.stringify(audit.mock.calls)).not.toContain("draft-secret");
    expect(audit).toHaveBeenCalledWith(expect.objectContaining({
      action: "provider.temporary_credentials.consume",
      result: "allowed",
      metadata: expect.objectContaining({
        providerId: "draft-provider",
        source: "request-draft",
        operation: "connectivity-probe",
      }),
    }));
  });

  it("Provider 或用途不匹配时失败关闭", () => {
    const boundary = createTemporaryProviderCredentialBoundary({
      providerId: "draft-provider",
      source: "fresh-provider",
      operation: "catalog-read",
      apiKey: "fresh-secret",
    });

    expect(() => boundary.consume({
      providerId: "other-provider",
      operation: "catalog-read",
    })).toThrow(/scope mismatch/i);
  });
});

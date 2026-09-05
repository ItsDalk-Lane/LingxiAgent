import { describe, expect, it } from "vitest";
import {
  TOOL_INVOCATION_ERROR_CODES,
  ToolInvocationError,
  createFirstPartyToolIdentity,
  isToolInvocationError,
} from "../lib/tools/invocation/index.ts";

describe("工具调用稳定错误", () => {
  it("固定公开错误码集合", () => {
    expect(TOOL_INVOCATION_ERROR_CODES).toEqual([
      "TARGET_NOT_FOUND",
      "TARGET_AMBIGUOUS",
      "TARGET_NOT_VISIBLE",
      "TARGET_DISABLED_FOR_AGENT",
      "TARGET_REVOKED",
      "PERMISSION_CONTRACT_MISSING",
      "PERMISSION_CONTRACT_CONFLICT",
      "PERMISSION_DENIED",
      "CAPABILITY_MISMATCH",
      "PREPARED_INVOCATION_MISSING",
      "PREPARED_INVOCATION_MISMATCH",
      "ARGUMENTS_NOT_OBJECT",
      "ARGUMENT_SCHEMA_INVALID",
      "TOOL_SCHEMA_INVALID",
      "CREDENTIAL_PROVIDER_UNRESOLVED",
      "CREDENTIAL_MISSING",
      "TRANSPORT_FAILURE",
      "EXECUTION_CANCELLED",
    ]);
  });

  it("携带可归因字段并可被可靠识别", () => {
    const identity = createFirstPartyToolIdentity({
      publicName: "resource_read",
      capabilityBase: "resource.read",
    });
    const cause = new Error("transport closed");
    const error = new ToolInvocationError({
      code: "TRANSPORT_FAILURE",
      message: "工具传输失败",
      route: "deferred",
      targetId: identity.targetId,
      sourceId: identity.sourceId,
      cause,
      details: { attempt: 1 },
    });

    expect(error).toMatchObject({
      name: "ToolInvocationError",
      code: "TRANSPORT_FAILURE",
      route: "deferred",
      targetId: identity.targetId,
      sourceId: "first-party",
      cause,
      details: { attempt: 1 },
    });
    expect(isToolInvocationError(error)).toBe(true);
    expect(isToolInvocationError(new Error("ordinary"))).toBe(false);
  });

  it("序列化时不会泄露密钥、令牌或完整授权头", () => {
    const error = new ToolInvocationError({
      code: "CREDENTIAL_MISSING",
      message: "credential missing for Bearer top-secret-token",
      route: "direct",
      sourceId: "provider-a",
      details: {
        apiKey: "sk-secret-value",
        authorization: "Bearer another-secret",
        safeReason: "no active lane",
        nested: { accessToken: "nested-secret" },
      },
      cause: new Error("Authorization: Bearer cause-secret"),
    });

    const serialized = JSON.stringify(error);
    expect(serialized).not.toContain("top-secret-token");
    expect(serialized).not.toContain("another-secret");
    expect(serialized).not.toContain("nested-secret");
    expect(serialized).not.toContain("cause-secret");
    expect(JSON.parse(serialized)).toMatchObject({
      code: "CREDENTIAL_MISSING",
      route: "direct",
      sourceId: "provider-a",
      details: {
        apiKey: "[REDACTED]",
        authorization: "[REDACTED]",
        safeReason: "no active lane",
        nested: { accessToken: "[REDACTED]" },
      },
    });
  });

  it("模型可见消息会遮蔽内部路径和常见明文密钥", () => {
    const error = new ToolInvocationError({
      code: "TRANSPORT_FAILURE",
      message: "failed at /Users/alice/.config/provider.json with sk-live-secret123",
      route: "deferred",
    });

    expect(error.message).not.toContain("/Users/alice");
    expect(error.message).not.toContain("sk-live-secret123");
    expect(error.message).toContain("[REDACTED_PATH]");
    expect(error.message).toContain("[REDACTED]");
  });
});

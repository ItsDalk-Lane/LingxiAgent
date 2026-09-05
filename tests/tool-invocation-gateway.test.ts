import { describe, expect, it, vi } from "vitest";
import { ToolInvocationGateway } from "../core/tool-invocation-gateway.ts";
import { ToolTargetRegistry } from "../core/tool-target-registry.ts";
import {
  createFirstPartyToolIdentity,
  createToolSchemaValidator,
  getPreparedInvocation,
  normalizeToolPermissionContract,
  runWithPreparedInvocation,
  ToolInvocationError,
} from "../lib/tools/invocation/index.ts";

function fixture() {
  const registry = new ToolTargetRegistry();
  const identity = createFirstPartyToolIdentity({
    publicName: "write_note",
    capabilityBase: "write_note",
  });
  let generation = 3;
  let available = true;
  const executeCanonical = vi.fn(async (
    toolCallId: string,
    args: Record<string, unknown>,
    signal: AbortSignal | undefined,
    onUpdate: unknown,
    ctx: unknown,
  ) => ({ toolCallId, args, signal, onUpdate, ctx, prepared: getPreparedInvocation() }));
  const permission = normalizeToolPermissionContract({
    name: identity.publicName,
    sessionPermission: {
      resolveInvocation: () => ({
        action: "write",
        kind: "review",
        capability: "write_note.write",
        sideEffect: { kind: "workspace_write", summary: "Write one note." },
      }),
    },
  }, identity);
  const validator = createToolSchemaValidator({
    type: "object",
    required: ["path", "content"],
    additionalProperties: false,
    properties: {
      path: { type: "string" },
      content: { type: "string" },
    },
  }, identity);
  const target = {
    identity,
    label: "Write note",
    description: "Write one note",
    parameters: validator.schema,
    deferrable: true,
    pinned: false,
    permission,
    validator,
    availability: { eligible: true },
    getCurrentGeneration: () => generation,
    isCurrentlyAvailable: () => available,
    executeCanonical,
    normalizeResult: (result: unknown) => ({ normalized: result }),
  };
  registry.register(target);
  const authorize = vi.fn(async () => undefined);
  const gateway = new ToolInvocationGateway({ registry, authorize });
  const request = {
    targetId: identity.targetId,
    route: "direct" as const,
    arguments: { path: "note.md", content: "hello" },
    sessionId: "session-1",
    sessionPath: "/sessions/one.jsonl",
    agentId: "agent-1",
    lifecycleGeneration: generation,
    toolCallId: "call-1",
    signal: new AbortController().signal,
    onUpdate: vi.fn(),
    ctx: { caller: "model", invocationRoute: "forged", effectiveTargetId: "forged" },
    runtimeContext: { connected: true },
  };
  return {
    registry,
    identity,
    target,
    gateway,
    authorize,
    request,
    setGeneration(value: number) { generation = value; },
    setAvailable(value: boolean) { available = value; },
  };
}

function expectCode(error: unknown, code: ToolInvocationError["code"]) {
  expect(error).toBeInstanceOf(ToolInvocationError);
  expect(error).toMatchObject({ code });
}

type GatewayRequest = Parameters<ToolInvocationGateway["invoke"]>[0];
type GatewayResult = { normalized: { prepared: { route: string } } };

const mismatchMutations: Array<[string, (request: GatewayRequest) => GatewayRequest]> = [
  ["参数替换", (request) => ({ ...request, arguments: { path: "other.md", content: "hello" } })],
  ["目标替换", (request) => ({
    ...request,
    targetId: "tool:first-party:other" as GatewayRequest["targetId"],
  })],
  ["会话替换", (request) => ({ ...request, sessionId: "session-2" })],
  ["代理替换", (request) => ({ ...request, agentId: "agent-2" })],
  ["调用编号替换", (request) => ({ ...request, toolCallId: "call-2" })],
];

describe("规范化工具调用网关", () => {
  it("合法 prepared 调用绑定完整事实并原样透传执行句柄", async () => {
    const { gateway, request, target, identity } = fixture();
    const prepared = gateway.resolvePermission(request);

    const result = await runWithPreparedInvocation(
      prepared,
      () => gateway.invoke(request),
    ) as GatewayResult;

    expect(target.executeCanonical).toHaveBeenCalledOnce();
    expect(target.executeCanonical).toHaveBeenCalledWith(
      request.toolCallId,
      request.arguments,
      request.signal,
      request.onUpdate,
      expect.objectContaining({
        caller: "model",
        invocationRoute: "direct",
        effectiveTargetId: identity.targetId,
      }),
    );
    expect(result.normalized.prepared).toBe(prepared);
    expect(Object.isFrozen(prepared.permission.sideEffect)).toBe(true);
    expect(prepared).toMatchObject({
      targetId: identity.targetId,
      route: "direct",
      sessionId: "session-1",
      sessionPath: "/sessions/one.jsonl",
      agentId: "agent-1",
      lifecycleGeneration: 3,
      toolCallId: "call-1",
    });
  });

  it("没有 prepared 上下文时拒绝模型路径", async () => {
    const { gateway, request } = fixture();

    await expect(gateway.invoke(request)).rejects.toSatisfy(
      (error: unknown) => (expectCode(error, "PREPARED_INVOCATION_MISSING"), true),
    );
  });

  it.each(mismatchMutations)("prepared 后发生%s时 fail-closed", async (_label, mutate) => {
    const { gateway, request } = fixture();
    const prepared = gateway.resolvePermission(request);

    await expect(runWithPreparedInvocation(
      prepared,
      () => gateway.invoke(mutate(request)),
    )).rejects.toSatisfy(
      (error: unknown) => (expectCode(error, "PREPARED_INVOCATION_MISMATCH"), true),
    );
  });

  it("参数键顺序变化但内容相同仍匹配稳定 digest", async () => {
    const { gateway, request } = fixture();
    const prepared = gateway.resolvePermission(request);
    const reordered = {
      ...request,
      arguments: { content: "hello", path: "note.md" },
    };

    await expect(runWithPreparedInvocation(
      prepared,
      () => gateway.invoke(reordered),
    )).resolves.toBeTruthy();
  });

  it("把 facade 的 effective invocation 解析到真实目标后再准备和执行", async () => {
    const { registry, gateway, request, identity, target } = fixture();
    const bridgeIdentity = createFirstPartyToolIdentity({
      publicName: "mcp_call",
      capabilityBase: "mcp_call",
    });
    const bridgeValidator = createToolSchemaValidator({
      type: "object",
      required: ["server", "tool"],
      properties: {
        server: { type: "string" },
        tool: { type: "string" },
      },
    }, bridgeIdentity);
    registry.register({
      identity: bridgeIdentity,
      label: "MCP call",
      description: "Facade",
      parameters: bridgeValidator.schema,
      deferrable: false,
      pinned: true,
      permission: {
        identity: bridgeIdentity,
        source: "resolver",
        legacyRoutineAutoAllow: false,
        resolveInvocation: () => ({
          action: "write",
          kind: "review",
          capability: "write_note.write",
          effectiveInvocation: {
            targetId: identity.targetId,
            toolName: identity.publicName,
            arguments: request.arguments,
            generation: 3,
          },
        }),
      },
      validator: bridgeValidator,
      availability: { eligible: true },
      getCurrentGeneration: () => 1,
      isCurrentlyAvailable: () => true,
      executeCanonical: vi.fn(async () => undefined),
      normalizeResult: (result: unknown) => result,
    });
    const outerRequest = {
      ...request,
      targetId: bridgeIdentity.targetId,
      route: "deferred" as const,
      arguments: { server: "notes", tool: "write_note" },
      lifecycleGeneration: 1,
    };

    const prepared = gateway.resolvePermission(outerRequest);
    expect(prepared).toMatchObject({
      targetId: identity.targetId,
      route: "deferred",
      arguments: request.arguments,
      lifecycleGeneration: 3,
    });
    await runWithPreparedInvocation(prepared, () => gateway.invoke({
      ...request,
      route: "deferred",
      targetId: identity.targetId,
      lifecycleGeneration: 3,
    }));
    expect(target.executeCanonical).toHaveBeenCalledOnce();
  });

  it("代次变化或实时不可用时撤销已准备调用", async () => {
    const generationFixture = fixture();
    const prepared = generationFixture.gateway.resolvePermission(generationFixture.request);
    generationFixture.setGeneration(4);
    await expect(runWithPreparedInvocation(
      prepared,
      () => generationFixture.gateway.invoke(generationFixture.request),
    )).rejects.toSatisfy(
      (error: unknown) => (expectCode(error, "TARGET_REVOKED"), true),
    );

    const availabilityFixture = fixture();
    const preparedAvailable = availabilityFixture.gateway.resolvePermission(availabilityFixture.request);
    availabilityFixture.setAvailable(false);
    await expect(runWithPreparedInvocation(
      preparedAvailable,
      () => availabilityFixture.gateway.invoke(availabilityFixture.request),
    )).rejects.toSatisfy(
      (error: unknown) => (expectCode(error, "TARGET_REVOKED"), true),
    );
  });

  it("取消在 canonical executor 前后都传播为稳定取消错误", async () => {
    const before = fixture();
    const beforeController = new AbortController();
    beforeController.abort();
    const beforeRequest = { ...before.request, signal: beforeController.signal };
    const beforePrepared = before.gateway.resolvePermission(beforeRequest);
    await expect(runWithPreparedInvocation(
      beforePrepared,
      () => before.gateway.invoke(beforeRequest),
    )).rejects.toSatisfy(
      (error: unknown) => (expectCode(error, "EXECUTION_CANCELLED"), true),
    );
    expect(before.target.executeCanonical).not.toHaveBeenCalled();

    const after = fixture();
    after.target.executeCanonical.mockRejectedValueOnce(
      Object.assign(new Error("aborted"), { name: "AbortError" }),
    );
    const afterPrepared = after.gateway.resolvePermission(after.request);
    await expect(runWithPreparedInvocation(
      afterPrepared,
      () => after.gateway.invoke(after.request),
    )).rejects.toSatisfy(
      (error: unknown) => (expectCode(error, "EXECUTION_CANCELLED"), true),
    );
  });

  it("本地开发入口自行准备并只调用一次宿主审批", async () => {
    const { gateway, request, authorize, target } = fixture();
    const localRequest = { ...request, route: "plugin-dev-http" as const };

    const result = await gateway.prepareAndInvokeForLocalDeveloper(localRequest);

    expect(authorize).toHaveBeenCalledOnce();
    expect(target.executeCanonical).toHaveBeenCalledOnce();
    expect((result as GatewayResult).normalized.prepared.route).toBe("plugin-dev-http");
  });

  it("装配时不可见目标和不合规参数在权限解析前被拒绝", () => {
    const hidden = fixture();
    (hidden.target.availability as { eligible: boolean }).eligible = false;
    expect(() => hidden.gateway.resolvePermission(hidden.request)).toThrow(
      expect.objectContaining({ code: "TARGET_NOT_VISIBLE" }),
    );

    const invalid = fixture();
    expect(() => invalid.gateway.resolvePermission({
      ...invalid.request,
      arguments: { path: "note.md" },
    })).toThrow(expect.objectContaining({ code: "ARGUMENT_SCHEMA_INVALID" }));
  });
});

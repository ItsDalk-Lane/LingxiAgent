import { describe, expect, it, vi } from "vitest";
import { ToolInvocationGateway } from "../core/tool-invocation-gateway.ts";
import { ToolTargetRegistry } from "../core/tool-target-registry.ts";
import { registerToolCapabilityDelegate } from "../lib/permission/tool-invocation-permission.ts";
import {
  createFirstPartyToolIdentity,
  createToolSchemaValidator,
  getPreparedInvocation,
  normalizeToolPermissionContract,
  type ToolInvocationRoute,
} from "../lib/tools/invocation/index.ts";
import { wrapWithSessionPermission } from "../lib/tools/session-permission-wrapper.ts";

const MODEL_ROUTES = ["direct", "deferred", "plugin-dev-chat"] as const;
const RAW_PATH = "/workspace/./report.md";
const CANONICAL_PATH = "/workspace/report.md";

type ModelRoute = typeof MODEL_ROUTES[number];
type ToolCall = {
  route: ToolInvocationRoute;
  toolCallId: string;
  arguments: Record<string, unknown>;
  signal: AbortSignal | undefined;
  onUpdate: unknown;
  targetId: string | undefined;
  argumentsDigest: string | undefined;
  capability: string | undefined;
  sideEffect: unknown;
};

function createPathParityFixture() {
  const registry = new ToolTargetRegistry();
  const identity = createFirstPartyToolIdentity({
    publicName: "stage_files",
    capabilityBase: "stage_files",
  });
  const schema = {
    type: "object",
    additionalProperties: false,
    required: ["filepaths"],
    properties: {
      filepaths: { type: "array", items: { type: "string" }, minItems: 1 },
    },
  };
  const describeSideEffect = vi.fn((args: Record<string, unknown>) => ({
    kind: "workspace_write",
    summary: `Stage ${JSON.stringify(args.filepaths)}`,
  }));
  const permission = normalizeToolPermissionContract({
    name: identity.publicName,
    sessionPermission: {
      resolveInvocation: (args: Record<string, unknown>) => ({
        action: "write",
        kind: "review",
        capability: "stage_files.write",
        sideEffect: describeSideEffect(args),
      }),
    },
  }, identity);
  const validator = createToolSchemaValidator(schema, identity);
  const calls: ToolCall[] = [];
  const executeCanonical = vi.fn(async (
    toolCallId: string,
    args: Record<string, unknown>,
    signal: AbortSignal | undefined,
    onUpdate: unknown,
    ctx: unknown,
  ) => {
    const prepared = getPreparedInvocation();
    calls.push({
      route: prepared?.route || "isolated",
      toolCallId,
      arguments: args,
      signal,
      onUpdate,
      targetId: prepared?.targetId,
      argumentsDigest: prepared?.argumentsDigest,
      capability: prepared?.permission.capability,
      sideEffect: prepared?.permission.sideEffect,
    });
    return {
      content: [{ type: "text", text: "staged" }],
      details: { appCard: { kind: "files", count: 1 } },
      provenance: { sourceId: identity.sourceId, targetId: identity.targetId },
      contextTargetId: (ctx as { effectiveTargetId?: unknown })?.effectiveTargetId,
    };
  });
  const target = registry.register({
    identity,
    label: "Stage files",
    description: "Stage workspace files",
    parameters: schema,
    deferrable: true,
    pinned: false,
    permission,
    validator,
    availability: { eligible: true },
    getCurrentGeneration: () => 7,
    isCurrentlyAvailable: () => ({ eligible: true }),
    executeCanonical,
    normalizeResult: (result) => result,
  });
  const gatewayAuthorize = vi.fn(async () => undefined);
  const gateway = new ToolInvocationGateway({ registry, authorize: gatewayAuthorize, now: () => 100 });
  type ApprovalRequest = {
    toolName: string;
    actionName: string;
    params: Record<string, unknown>;
    target: Record<string, unknown>;
    sideEffect?: unknown;
  };
  const approvalGateway = {
    review: vi.fn(async (_request: ApprovalRequest, _context: unknown) => ({
      action: "allow",
      reviewer: "path-parity",
    })),
  };
  const checkStagePath = vi.fn((filePath: string) => ({
    allowed: true,
    canonicalPath: filePath === RAW_PATH ? CANONICAL_PATH : filePath,
  }));
  const policySnapshot = Object.freeze({
    permissionMode: "auto",
    approvalPolicy: "deny_on_prompt",
  });
  const sessionRef = Object.freeze({
    sessionId: "session-path-parity",
    sessionPath: "/sessions/path-parity.jsonl",
  });
  const principal = Object.freeze({ agentId: "agent-path-parity", sessionRef });

  function facade(route: ModelRoute) {
    const outerName = route === "direct"
      ? identity.publicName
      : route === "deferred"
        ? "mcp_call"
        : "plugin_dev_invoke_tool";
    const outerIdentity = route === "direct"
      ? identity
      : createFirstPartyToolIdentity({ publicName: outerName, capabilityBase: outerName });
    const resolveInvocation = (params: Record<string, unknown>) => {
      const effectiveArguments = route === "direct"
        ? params
        : params.arguments as Record<string, unknown>;
      const descriptor = permission.resolveInvocation(effectiveArguments);
      return route === "direct"
        ? descriptor
        : {
          ...descriptor,
          effectiveInvocation: {
            targetId: identity.targetId,
            toolName: identity.publicName,
            arguments: effectiveArguments,
            generation: target.lifecycleGeneration,
          },
        };
    };
    const modelTool = {
      name: outerName,
      parameters: route === "direct" ? schema : { type: "object" },
      _toolInvocationRoute: route,
      _toolTargetIdentity: outerIdentity,
      _toolLifecycleGeneration: route === "direct" ? target.lifecycleGeneration : 1,
      _normalizedPermissionContract: route === "direct" ? permission : undefined,
      sessionPermission: { resolveInvocation },
      execute: async (
        toolCallId: string,
        params: Record<string, unknown>,
        signal: AbortSignal | undefined,
        onUpdate: unknown,
        ctx: Record<string, unknown>,
      ) => {
        const effectiveArguments = route === "direct"
          ? params
          : params.arguments as Record<string, unknown>;
        return gateway.invoke({
          targetId: identity.targetId,
          route,
          arguments: effectiveArguments,
          sessionId: sessionRef.sessionId,
          sessionPath: sessionRef.sessionPath,
          agentId: principal.agentId,
          lifecycleGeneration: target.lifecycleGeneration,
          toolCallId,
          signal,
          onUpdate,
          ctx,
          runtimeContext: { principal, sessionRef, policySnapshot },
        });
      },
    };
    if (route !== "direct") {
      registerToolCapabilityDelegate(modelTool, (capability, action) => (
        gateway.canDelegateCapability(identity.targetId, capability, action)
      ));
    }
    return wrapWithSessionPermission([modelTool], {
      invocationRoute: route,
      agentId: principal.agentId,
      getSessionRef: () => sessionRef,
      getSessionIdForPath: () => sessionRef.sessionId,
      getPermissionMode: () => policySnapshot.permissionMode,
      approvalPolicy: policySnapshot.approvalPolicy,
      approvalGateway,
      permissionBoundary: { checkStagePath },
      permissionContext: policySnapshot,
      now: () => 100,
    })[0];
  }

  return {
    approvalGateway,
    calls,
    checkStagePath,
    describeSideEffect,
    executeCanonical,
    facade,
    gateway,
    gatewayAuthorize,
    identity,
    policySnapshot,
    principal,
    sessionRef,
    target,
  };
}

function modelArguments(route: ModelRoute) {
  const argumentsValue = { filepaths: [RAW_PATH] };
  if (route === "direct") return argumentsValue;
  return route === "deferred"
    ? { server: "first-party", tool: "stage_files", arguments: argumentsValue }
    : { pluginId: "first-party", toolName: "stage_files", arguments: argumentsValue };
}

describe("tool invocation route invariance", () => {
  it("keeps model routes semantically identical when only route changes", async () => {
    const fixture = createPathParityFixture();
    const signal = new AbortController().signal;
    const onUpdate = vi.fn();
    const outputs: unknown[] = [];

    for (const route of MODEL_ROUTES) {
      const tool = fixture.facade(route);
      const approvalsBefore = fixture.approvalGateway.review.mock.calls.length;
      const executionsBefore = fixture.executeCanonical.mock.calls.length;
      outputs.push(await tool.execute(
        "call-path-parity",
        modelArguments(route),
        signal,
        onUpdate,
        {
          agentId: fixture.principal.agentId,
          sessionId: fixture.sessionRef.sessionId,
          sessionPath: fixture.sessionRef.sessionPath,
          sessionRef: fixture.sessionRef,
        },
      ));
      expect(fixture.approvalGateway.review.mock.calls.length - approvalsBefore).toBe(1);
      expect(fixture.executeCanonical.mock.calls.length - executionsBefore).toBe(1);
    }

    expect(outputs[1]).toEqual(outputs[0]);
    expect(outputs[2]).toEqual(outputs[0]);
    expect(fixture.calls.map((call) => ({
      toolCallId: call.toolCallId,
      arguments: call.arguments,
      signal: call.signal,
      onUpdate: call.onUpdate,
      targetId: call.targetId,
      argumentsDigest: call.argumentsDigest,
      capability: call.capability,
      sideEffect: call.sideEffect,
    }))).toEqual(MODEL_ROUTES.map(() => ({
      toolCallId: "call-path-parity",
      arguments: { filepaths: [CANONICAL_PATH] },
      signal,
      onUpdate,
      targetId: fixture.identity.targetId,
      argumentsDigest: fixture.calls[0].argumentsDigest,
      capability: "stage_files.write",
      sideEffect: {
        kind: "workspace_write",
        summary: `Stage ${JSON.stringify([RAW_PATH])}`,
      },
    })));
    expect(fixture.calls.map((call) => call.route)).toEqual(MODEL_ROUTES);
    expect(fixture.gatewayAuthorize).not.toHaveBeenCalled();
    expect(fixture.approvalGateway.review).toHaveBeenCalledTimes(3);
    expect(fixture.approvalGateway.review.mock.calls.map(([request]) => ({
      toolName: request.toolName,
      actionName: request.actionName,
      params: request.params,
      target: request.target,
      sideEffect: request.sideEffect,
    }))).toEqual(MODEL_ROUTES.map(() => ({
      toolName: "stage_files",
      actionName: "write",
      params: { filepaths: [RAW_PATH] },
      target: {
        type: "tool",
        id: fixture.identity.targetId,
        label: "stage_files",
      },
      sideEffect: {
        kind: "workspace_write",
        summary: `Stage ${JSON.stringify([RAW_PATH])}`,
      },
    })));
    expect(fixture.checkStagePath).toHaveBeenCalledTimes(9);
    expect(fixture.describeSideEffect).toHaveBeenCalledTimes(6);
  });

  it("keeps model-route schema failures on the same stable error code", async () => {
    const fixture = createPathParityFixture();
    const errorCodes: unknown[] = [];

    for (const route of MODEL_ROUTES) {
      const tool = fixture.facade(route);
      try {
        await tool.execute(
          "call-invalid",
          route === "direct"
            ? { filepaths: [7] }
            : { arguments: { filepaths: [7] } },
          undefined,
          undefined,
          {
            agentId: fixture.principal.agentId,
            sessionId: fixture.sessionRef.sessionId,
            sessionPath: fixture.sessionRef.sessionPath,
            sessionRef: fixture.sessionRef,
          },
        );
      } catch (error) {
        errorCodes.push((error as { code?: unknown }).code);
      }
    }

    expect(errorCodes).toEqual(MODEL_ROUTES.map(() => "ARGUMENT_SCHEMA_INVALID"));
    expect(fixture.executeCanonical).not.toHaveBeenCalled();
  });

  it("keeps the HTTP developer route on the same schema, lifecycle, identity, and executor", async () => {
    const fixture = createPathParityFixture();
    const signal = new AbortController().signal;
    const onUpdate = vi.fn();
    const principal = {
      kind: "local-developer" as const,
      principalId: "local-developer:owner-path-parity",
      ownerPrincipalId: "owner-path-parity",
      connectionKind: "local" as const,
    };

    const output = await fixture.gateway.prepareAndInvokeForLocalDeveloper({
      targetId: fixture.identity.targetId,
      route: "plugin-dev-http",
      arguments: { filepaths: [CANONICAL_PATH] },
      lifecycleGeneration: fixture.target.lifecycleGeneration,
      toolCallId: "call-http-path-parity",
      signal,
      onUpdate,
      runtimeContext: { policySnapshot: fixture.policySnapshot },
    }, principal);

    expect(output).toMatchObject({
      content: [{ type: "text", text: "staged" }],
      provenance: {
        sourceId: fixture.identity.sourceId,
        targetId: fixture.identity.targetId,
      },
    });
    expect(fixture.gatewayAuthorize).toHaveBeenCalledOnce();
    expect(fixture.executeCanonical).toHaveBeenCalledOnce();
    expect(fixture.calls[0]).toMatchObject({
      route: "plugin-dev-http",
      toolCallId: "call-http-path-parity",
      arguments: { filepaths: [CANONICAL_PATH] },
      signal,
      onUpdate,
      targetId: fixture.identity.targetId,
      capability: "stage_files.write",
    });

    await expect(fixture.gateway.prepareAndInvokeForLocalDeveloper({
      targetId: fixture.identity.targetId,
      route: "plugin-dev-http",
      arguments: { filepaths: [7] },
      lifecycleGeneration: fixture.target.lifecycleGeneration,
      toolCallId: "call-http-invalid",
    }, principal)).rejects.toMatchObject({ code: "ARGUMENT_SCHEMA_INVALID" });
    expect(fixture.executeCanonical).toHaveBeenCalledOnce();
  });
});

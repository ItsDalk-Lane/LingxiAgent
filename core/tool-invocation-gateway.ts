import {
  createPreparedInvocation,
  digestToolArguments,
  getPreparedInvocation,
  runWithPreparedInvocation,
  ToolInvocationError,
  type PreparedInvocation,
  type ToolInvocationRoute,
  type ToolTargetId,
} from "../lib/tools/invocation/index.ts";
import type {
  CatalogTargetReference,
  RegisteredToolTarget,
  ToolTargetAvailabilityDecision,
  ToolTargetRegistry,
} from "./tool-target-registry.ts";

export interface ToolInvocationGatewayRequest {
  readonly targetId: ToolTargetId;
  readonly route: ToolInvocationRoute;
  readonly arguments: Record<string, unknown>;
  readonly sessionId?: string | null;
  readonly sessionPath?: string | null;
  readonly agentId?: string | null;
  readonly lifecycleGeneration?: string | number;
  readonly toolCallId: string;
  readonly signal?: AbortSignal;
  readonly onUpdate?: unknown;
  readonly ctx?: unknown;
  readonly runtimeContext?: unknown;
}

export interface ToolInvocationGatewayOptions {
  readonly registry: ToolTargetRegistry;
  readonly authorize: (
    prepared: PreparedInvocation,
    target: RegisteredToolTarget,
    runtimeContext: unknown,
  ) => void | Promise<void>;
  readonly now?: () => number;
  readonly log?: { error?: (message: string) => void };
}

const gatewayDiagnosticLogs = new WeakMap<object, ToolInvocationGatewayOptions["log"]>();

export interface LocalDeveloperPrincipal {
  readonly kind: "local-developer";
  readonly principalId: string;
  readonly ownerPrincipalId: string;
  readonly connectionKind: "local";
}

function isLocalDeveloperPrincipal(value: unknown): value is LocalDeveloperPrincipal {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const principal = value as Record<string, unknown>;
  return principal.kind === "local-developer"
    && typeof principal.principalId === "string"
    && principal.principalId.startsWith("local-developer:")
    && typeof principal.ownerPrincipalId === "string"
    && principal.ownerPrincipalId.length > 0
    && principal.connectionKind === "local";
}

export function createLocalDeveloperPrincipal(ownerPrincipal: unknown): LocalDeveloperPrincipal {
  if (!ownerPrincipal || typeof ownerPrincipal !== "object" || Array.isArray(ownerPrincipal)) {
    throw new TypeError("local developer principal requires an authenticated local owner");
  }
  const owner = ownerPrincipal as Record<string, unknown>;
  if (
    owner.kind !== "local_user"
    || owner.connectionKind !== "local"
    || owner.credentialKind !== "loopback_token"
    || typeof owner.principalId !== "string"
    || !owner.principalId.trim()
  ) {
    throw new TypeError("local developer principal requires an authenticated local owner");
  }
  const ownerPrincipalId = owner.principalId.trim();
  return Object.freeze({
    kind: "local-developer",
    principalId: `local-developer:${ownerPrincipalId}`,
    ownerPrincipalId,
    connectionKind: "local",
  });
}

function gatewayError(
  code: "TARGET_NOT_FOUND" | "TARGET_NOT_VISIBLE" | "TARGET_DISABLED_FOR_AGENT" | "TARGET_REVOKED"
    | "PREPARED_INVOCATION_MISSING" | "PREPARED_INVOCATION_MISMATCH"
    | "PERMISSION_DENIED" | "CAPABILITY_MISMATCH" | "TRANSPORT_FAILURE"
    | "EXECUTION_CANCELLED",
  message: string,
  request: ToolInvocationGatewayRequest,
  target?: RegisteredToolTarget | null,
  details?: Record<string, unknown>,
  cause?: unknown,
): ToolInvocationError {
  const error = new ToolInvocationError({
    code,
    message,
    route: request.route,
    targetId: target?.identity.targetId ?? request.targetId ?? null,
    sourceId: target?.identity.sourceId ?? null,
    details,
    cause,
  });
  const diagnostic = {
    route: request.route,
    origin: target?.identity.origin ?? "unknown",
    targetId: target?.identity.targetId ?? request.targetId ?? null,
    sourceId: target?.identity.sourceId ?? null,
    generation: target?.lifecycleGeneration ?? request.lifecycleGeneration ?? null,
    code,
  };
  try {
    gatewayDiagnosticLogs.get(request)?.error?.(JSON.stringify(diagnostic));
  } catch {
    // 诊断输出失败不能改变调用错误本身。
  }
  return error;
}

function normalizeOptionalText(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function availabilityDecision(
  value: boolean | ToolTargetAvailabilityDecision,
): ToolTargetAvailabilityDecision {
  return typeof value === "boolean" ? { eligible: value } : value;
}

function abortError(
  request: ToolInvocationGatewayRequest,
  target: RegisteredToolTarget,
  cause?: unknown,
): ToolInvocationError {
  return gatewayError(
    "EXECUTION_CANCELLED",
    "Tool invocation was cancelled.",
    request,
    target,
    undefined,
    cause,
  );
}

function executionContext(ctx: unknown, request: ToolInvocationGatewayRequest): Record<string, unknown> {
  const base = ctx && typeof ctx === "object" && !Array.isArray(ctx)
    ? ctx as Record<string, unknown>
    : {};
  return {
    ...base,
    invocationRoute: request.route,
    effectiveTargetId: request.targetId,
  };
}

export class ToolInvocationGateway {
  private readonly registry: ToolTargetRegistry;
  private readonly authorize: ToolInvocationGatewayOptions["authorize"];
  private readonly now: () => number;
  private readonly log: ToolInvocationGatewayOptions["log"];

  constructor(options: ToolInvocationGatewayOptions) {
    if (!options?.registry || typeof options.authorize !== "function") {
      throw new TypeError("tool invocation gateway requires registry and authorize");
    }
    this.registry = options.registry;
    this.authorize = options.authorize;
    this.now = options.now ?? Date.now;
    this.log = options.log;
  }

  private bindDiagnosticLog(request: ToolInvocationGatewayRequest): void {
    if (this.log) gatewayDiagnosticLogs.set(request, this.log);
  }

  resolveTarget(reference: CatalogTargetReference): RegisteredToolTarget {
    return this.registry.resolveCatalogTarget(reference);
  }

  /** 宿主桥接器只委托注册表中真实目标明确拥有的能力。 */
  canDelegateCapability(targetId: ToolTargetId, capability: string, action: string): boolean {
    const target = this.registry.getByTargetId(targetId);
    return target?.availability.eligible === true
      && capability === `${target.identity.capabilityBase}.${action}`;
  }

  resolvePermission(request: ToolInvocationGatewayRequest): PreparedInvocation {
    this.bindDiagnosticLog(request);
    const target = this.registry.getByTargetId(request.targetId);
    if (!target) {
      throw gatewayError("TARGET_NOT_FOUND", "Tool target is not registered.", request);
    }
    if (!target.availability.eligible) {
      throw gatewayError(
        target.availability.code ?? "TARGET_NOT_VISIBLE",
        "Tool target was not eligible when this session was assembled.",
        request,
        target,
        { reason: target.availability.reason ?? null },
      );
    }
    const validatedArguments = target.validator.validate(request.arguments, request.route);
    let permission;
    try {
      permission = target.permission.resolveInvocation(validatedArguments);
    } catch (cause) {
      if (cause instanceof ToolInvocationError) throw cause;
      throw gatewayError(
        "PERMISSION_DENIED",
        "Tool permission resolver did not authorize this invocation.",
        request,
        target,
        undefined,
        cause,
      );
    }
    let effectiveTarget = target;
    let effectiveArguments = validatedArguments;
    let lifecycleGeneration = target.lifecycleGeneration;
    if (permission.effectiveInvocation) {
      const effective = permission.effectiveInvocation;
      const resolvedEffectiveTarget = this.registry.getByTargetId(effective.targetId);
      if (!resolvedEffectiveTarget) {
        throw gatewayError(
          "TARGET_NOT_FOUND",
          "Effective tool target is not registered.",
          request,
          target,
          { effectiveTargetId: effective.targetId },
        );
      }
      if (
        effective.toolName !== resolvedEffectiveTarget.identity.publicName
        && effective.toolName !== resolvedEffectiveTarget.identity.localName
      ) {
        throw gatewayError(
          "PREPARED_INVOCATION_MISMATCH",
          "Effective tool name does not match its registered target.",
          request,
          resolvedEffectiveTarget,
          { fields: ["effectiveInvocation.toolName"] },
        );
      }
      if (!resolvedEffectiveTarget.availability.eligible) {
        throw gatewayError(
          resolvedEffectiveTarget.availability.code ?? "TARGET_NOT_VISIBLE",
          "Effective tool target was not eligible when this session was assembled.",
          request,
          resolvedEffectiveTarget,
          { reason: resolvedEffectiveTarget.availability.reason ?? null },
        );
      }
      const expectedCapability = `${resolvedEffectiveTarget.identity.capabilityBase}.${permission.action}`;
      if (permission.capability !== expectedCapability) {
        throw gatewayError(
          "CAPABILITY_MISMATCH",
          "Effective invocation capability does not belong to its real target.",
          request,
          resolvedEffectiveTarget,
          {
            declaredCapability: permission.capability,
            capabilityBase: resolvedEffectiveTarget.identity.capabilityBase,
            expectedCapability,
            action: permission.action,
          },
        );
      }
      if (resolvedEffectiveTarget.lifecycleGeneration !== effective.generation
        || resolvedEffectiveTarget.getCurrentGeneration() !== effective.generation) {
        throw gatewayError(
          "TARGET_REVOKED",
          "Effective tool target generation is no longer current.",
          request,
          resolvedEffectiveTarget,
        );
      }
      effectiveArguments = resolvedEffectiveTarget.validator.validate(
        effective.arguments,
        request.route,
      );
      effectiveTarget = resolvedEffectiveTarget;
      lifecycleGeneration = effective.generation;
    }
    return createPreparedInvocation({
      targetId: effectiveTarget.identity.targetId,
      route: request.route,
      arguments: effectiveArguments,
      sessionId: normalizeOptionalText(request.sessionId),
      sessionPath: normalizeOptionalText(request.sessionPath),
      agentId: normalizeOptionalText(request.agentId),
      permission,
      lifecycleGeneration,
      toolCallId: request.toolCallId,
      createdAt: this.now(),
    });
  }

  async invoke(request: ToolInvocationGatewayRequest): Promise<unknown> {
    this.bindDiagnosticLog(request);
    const prepared = getPreparedInvocation();
    if (!prepared) {
      throw gatewayError(
        "PREPARED_INVOCATION_MISSING",
        "Model tool invocations require a host-prepared invocation context.",
        request,
      );
    }
    const target = this.registry.getByTargetId(request.targetId);
    if (target && target.getCurrentGeneration() !== target.lifecycleGeneration) {
      throw gatewayError(
        "TARGET_REVOKED",
        "Tool target generation changed after this session was assembled.",
        request,
        target,
        {
          assembledGeneration: target.lifecycleGeneration,
          currentGeneration: target.getCurrentGeneration(),
        },
      );
    }
    const revalidatedArguments = target
      ? target.validator.validate(request.arguments, request.route)
      : request.arguments;
    let argumentsDigest: string;
    try {
      argumentsDigest = digestToolArguments(revalidatedArguments);
    } catch (cause) {
      throw gatewayError(
        "PREPARED_INVOCATION_MISMATCH",
        "Tool invocation arguments are no longer a valid prepared object.",
        request,
        target,
        { fields: ["argumentsDigest"] },
        cause,
      );
    }
    const mismatches = [
      prepared.targetId !== request.targetId ? "targetId" : null,
      prepared.route !== request.route ? "route" : null,
      prepared.argumentsDigest !== argumentsDigest ? "argumentsDigest" : null,
      prepared.sessionId !== normalizeOptionalText(request.sessionId) ? "sessionId" : null,
      prepared.sessionPath !== normalizeOptionalText(request.sessionPath) ? "sessionPath" : null,
      prepared.agentId !== normalizeOptionalText(request.agentId) ? "agentId" : null,
      prepared.toolCallId !== request.toolCallId ? "toolCallId" : null,
      request.lifecycleGeneration !== undefined
        && prepared.lifecycleGeneration !== request.lifecycleGeneration
        ? "lifecycleGeneration"
        : null,
    ].filter((field): field is string => !!field);
    if (mismatches.length > 0) {
      throw gatewayError(
        "PREPARED_INVOCATION_MISMATCH",
        "Tool invocation no longer matches its host-prepared facts.",
        request,
        target,
        { fields: mismatches.sort() },
      );
    }
    if (!target) {
      throw gatewayError("TARGET_REVOKED", "Prepared tool target is no longer registered.", request);
    }
    if (target.lifecycleGeneration !== prepared.lifecycleGeneration) {
      throw gatewayError(
        "TARGET_REVOKED",
        "Prepared tool target generation is no longer current.",
        request,
        target,
      );
    }
    const currentAvailability = availabilityDecision(
      await target.isCurrentlyAvailable(request.runtimeContext),
    );
    if (!currentAvailability.eligible) {
      throw gatewayError(
        currentAvailability.code ?? "TARGET_REVOKED",
        "Prepared tool target is no longer available.",
        request,
        target,
        { reason: currentAvailability.reason ?? null },
      );
    }
    if (request.signal?.aborted) throw abortError(request, target);

    let rawResult: unknown;
    try {
      rawResult = await target.executeCanonical(
        request.toolCallId,
        revalidatedArguments,
        request.signal,
        request.onUpdate,
        executionContext(request.ctx, request),
      );
    } catch (cause) {
      if (request.signal?.aborted || (cause as { name?: unknown })?.name === "AbortError") {
        throw abortError(request, target, cause);
      }
      if (cause instanceof ToolInvocationError) throw cause;
      throw gatewayError(
        "TRANSPORT_FAILURE",
        "Canonical tool execution failed.",
        request,
        target,
        undefined,
        cause,
      );
    }
    if (request.signal?.aborted) throw abortError(request, target);
    try {
      return target.normalizeResult(rawResult);
    } catch (cause) {
      if (cause instanceof ToolInvocationError) throw cause;
      throw gatewayError(
        "TRANSPORT_FAILURE",
        "Canonical tool result normalization failed.",
        request,
        target,
        undefined,
        cause,
      );
    }
  }

  async prepareAndInvokeForLocalDeveloper(
    request: ToolInvocationGatewayRequest,
    principal: LocalDeveloperPrincipal,
  ): Promise<unknown> {
    this.bindDiagnosticLog(request);
    if (request.route !== "plugin-dev-http" || !isLocalDeveloperPrincipal(principal)) {
      throw gatewayError(
        "PERMISSION_DENIED",
        "Local developer invocation requires an authenticated local developer principal.",
        request,
      );
    }
    const runtimeContext = request.runtimeContext && typeof request.runtimeContext === "object"
      ? { ...request.runtimeContext, principal }
      : { principal };
    const ctx = request.ctx && typeof request.ctx === "object"
      ? { ...request.ctx, principal }
      : { principal };
    const localRequest = {
      ...request,
      sessionId: null,
      sessionPath: null,
      agentId: null,
      ctx,
      runtimeContext,
    };
    this.bindDiagnosticLog(localRequest);
    const prepared = this.resolvePermission(localRequest);
    const target = this.registry.getByTargetId(prepared.targetId);
    if (!target) {
      throw gatewayError("TARGET_REVOKED", "Prepared tool target is no longer registered.", localRequest);
    }
    try {
      await this.authorize(prepared, target, runtimeContext);
    } catch (cause) {
      if (cause instanceof ToolInvocationError) throw cause;
      throw gatewayError(
        "PERMISSION_DENIED",
        "Local developer invocation was not authorized.",
        localRequest,
        target,
        undefined,
        cause,
      );
    }
    return runWithPreparedInvocation(prepared, () => this.invoke(localRequest));
  }
}

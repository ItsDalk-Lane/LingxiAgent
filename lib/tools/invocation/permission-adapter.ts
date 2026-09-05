import {
  normalizeLegacyToolPermissionMetadata,
  registerToolCapabilityDelegate,
  resolveToolInvocationPermission,
  type NormalizedToolInvocationDescriptor,
} from "../../permission/tool-invocation-permission.ts";
import { classifySessionPermission } from "../../../core/session-permission-mode.ts";
import { ToolInvocationError } from "./errors.ts";
import type { ToolTargetIdentity } from "./types.ts";

export interface NormalizedToolPermissionContract {
  readonly identity: ToolTargetIdentity;
  readonly source: "legacy" | "resolver";
  readonly legacyRoutineAutoAllow: boolean;
  readonly resolveInvocation: (input: unknown) => NormalizedToolInvocationDescriptor;
}

type LegacyClassification = {
  kind: NormalizedToolInvocationDescriptor["kind"];
  autoAllow: boolean;
  behavior: string;
};

function permissionError(
  code: ToolInvocationError["code"],
  message: string,
  identity: ToolTargetIdentity,
  details?: Record<string, unknown>,
  cause?: unknown,
): ToolInvocationError {
  return new ToolInvocationError({
    code,
    message,
    route: "direct",
    targetId: identity.targetId,
    sourceId: identity.sourceId,
    details,
    cause,
  });
}

function readSessionPermission(
  tool: Record<string, unknown>,
  identity: ToolTargetIdentity,
): Record<string, unknown> {
  let descriptor: PropertyDescriptor | undefined;
  try {
    descriptor = Object.getOwnPropertyDescriptor(tool, "sessionPermission");
  } catch (cause) {
    throw permissionError(
      "PERMISSION_CONTRACT_CONFLICT",
      "Tool permission metadata could not be inspected safely.",
      identity,
      undefined,
      cause,
    );
  }
  if (!descriptor || !("value" in descriptor) || descriptor.value == null) {
    throw permissionError(
      "PERMISSION_CONTRACT_MISSING",
      "Plugin and MCP tools require an explicit permission contract.",
      identity,
    );
  }
  const permission = descriptor.value;
  if (typeof permission !== "object" || Array.isArray(permission)) {
    throw permissionError(
      "PERMISSION_CONTRACT_CONFLICT",
      "Tool permission metadata must be a plain data record.",
      identity,
    );
  }
  return permission as Record<string, unknown>;
}

function permissionBehavior(
  identity: ToolTargetIdentity,
  context: Record<string, unknown>,
): { readOnly: string; auto: string; ask: string; operate: string } {
  const decide = (mode: string) => classifySessionPermission({
    mode,
    toolName: identity.publicName,
    params: {},
    context,
  }).action;
  return {
    readOnly: decide("read_only"),
    auto: decide("auto"),
    ask: decide("ask"),
    operate: decide("operate"),
  };
}

function classifyLegacyPermission(
  permission: Record<string, unknown>,
  identity: ToolTargetIdentity,
): LegacyClassification {
  const behavior = permissionBehavior(identity, {
    toolSessionPermission: permission,
    isPluginTool: identity.origin !== "first-party",
  });
  const kind = behavior.readOnly === "allow"
    ? "read"
    : behavior.auto === "allow"
      ? "routine"
      : "review";
  return {
    kind,
    autoAllow: behavior.auto === "allow",
    behavior: JSON.stringify(behavior),
  };
}

function descriptorBehavior(
  descriptor: NormalizedToolInvocationDescriptor,
  identity: ToolTargetIdentity,
): string {
  return JSON.stringify(permissionBehavior(identity, {
    toolInvocation: descriptor,
    isPluginTool: identity.origin !== "first-party",
  }));
}

function resolveSideEffect(
  permission: Record<string, unknown>,
  input: unknown,
  identity: ToolTargetIdentity,
): Record<string, unknown> | undefined {
  if (typeof permission.describeSideEffect === "function") {
    try {
      return permission.describeSideEffect(input) as Record<string, unknown>;
    } catch (cause) {
      throw permissionError(
        "PERMISSION_DENIED",
        "Tool side-effect description failed.",
        identity,
        undefined,
        cause,
      );
    }
  }
  return permission.sideEffect as Record<string, unknown> | undefined;
}

function validateDescriptor(
  descriptor: NormalizedToolInvocationDescriptor,
  identity: ToolTargetIdentity,
): NormalizedToolInvocationDescriptor {
  const expectedCapability = `${identity.capabilityBase}.${descriptor.action}`;
  if (descriptor.capability !== expectedCapability) {
    throw permissionError(
      "CAPABILITY_MISMATCH",
      "Tool capability does not match its registered capability base.",
      identity,
      { declaredCapability: descriptor.capability, capabilityBase: identity.capabilityBase },
    );
  }
  return descriptor;
}

function normalizeMappedDescriptor(
  descriptor: NormalizedToolInvocationDescriptor,
  identity: ToolTargetIdentity,
  input: unknown,
): NormalizedToolInvocationDescriptor {
  const validationTool = {
    name: identity.localName,
    sessionPermission: {
      resolveInvocation: () => descriptor,
    },
  };
  registerToolCapabilityDelegate(validationTool, (capability, action) => (
    capability === `${identity.capabilityBase}.${action}`
  ));
  return normalizeDescriptor(validationTool, identity, input);
}

function mapResolutionFailure(
  result: Extract<ReturnType<typeof resolveToolInvocationPermission>, { ok: false }>,
  identity: ToolTargetIdentity,
): never {
  const code = result.error.reason === "unknown_capability"
    ? "CAPABILITY_MISMATCH"
    : result.error.reason === "resolver_rejected" || result.error.reason === "resolver_threw"
      ? "PERMISSION_DENIED"
      : "PERMISSION_CONTRACT_CONFLICT";
  throw permissionError(code, result.error.message, identity, {
    reason: result.error.reason,
    ...(result.error.field ? { field: result.error.field } : {}),
    ...(code === "CAPABILITY_MISMATCH"
      ? {
        declaredCapability: result.error.declaredCapability ?? null,
        capabilityBase: identity.capabilityBase,
      }
      : {}),
  });
}

function normalizeDescriptor(
  tool: Record<string, unknown>,
  identity: ToolTargetIdentity,
  input: unknown,
): NormalizedToolInvocationDescriptor {
  const result = resolveToolInvocationPermission(tool, input);
  if (result.ok === false) return mapResolutionFailure(result, identity);
  if (result.source !== "descriptor") {
    throw permissionError(
      "PERMISSION_CONTRACT_CONFLICT",
      "Normalized permission resolver did not produce an invocation descriptor.",
      identity,
    );
  }
  return validateDescriptor(result.descriptor, identity);
}

export function normalizeToolPermissionContract(
  tool: Record<string, unknown>,
  identity: ToolTargetIdentity,
): NormalizedToolPermissionContract {
  const permission = readSessionPermission(tool, identity);
  const legacy = normalizeLegacyToolPermissionMetadata(permission);
  if (!legacy.ok) {
    throw permissionError(
      "PERMISSION_CONTRACT_CONFLICT",
      "Tool permission metadata contains unsupported fields or values.",
      identity,
    );
  }
  const resolverDescriptor = Object.getOwnPropertyDescriptor(permission, "resolveInvocation");
  const hasResolver = !!resolverDescriptor;
  if (hasResolver && (!("value" in resolverDescriptor) || typeof resolverDescriptor.value !== "function")) {
    throw permissionError(
      "PERMISSION_CONTRACT_CONFLICT",
      "Tool permission resolver must be a synchronous data function.",
      identity,
    );
  }

  const legacyFields = legacy.value;
  const hasLegacyClassification = ["readOnly", "kind", "auto"].some((key) => (
    Object.hasOwn(legacyFields, key)
  ));
  const legacyClassification = hasLegacyClassification
    ? classifyLegacyPermission(legacyFields, identity)
    : null;

  if (!hasResolver) {
    if (!legacyClassification) {
      throw permissionError(
        "PERMISSION_CONTRACT_MISSING",
        "Plugin and MCP tools require an explicit permission contract.",
        identity,
      );
    }
    const contract: NormalizedToolPermissionContract = {
      identity,
      source: "legacy",
      legacyRoutineAutoAllow: legacyClassification.kind === "routine"
        && legacyClassification.autoAllow,
      resolveInvocation: (input) => {
        const action = legacyClassification.kind === "read" ? "read" : "execute";
        const sideEffect = resolveSideEffect(legacyFields, input, identity);
        return normalizeMappedDescriptor({
          action,
          kind: legacyClassification.kind,
          capability: `${identity.capabilityBase}.${action}`,
          ...(sideEffect ? { sideEffect } : {}),
        }, identity, input);
      },
    };
    return Object.freeze(contract);
  }

  registerToolCapabilityDelegate(tool, (capability, action) => (
    capability === `${identity.capabilityBase}.${action}`
  ));
  const contract: NormalizedToolPermissionContract = {
    identity,
    source: "resolver",
    legacyRoutineAutoAllow: false,
    resolveInvocation: (input) => {
      const descriptor = normalizeDescriptor(tool, identity, input);
      if (legacyClassification
        && descriptorBehavior(descriptor, identity) !== legacyClassification.behavior) {
        throw permissionError(
          "PERMISSION_CONTRACT_CONFLICT",
          "New and legacy permission declarations disagree.",
          identity,
          { resolverKind: descriptor.kind, legacyKind: legacyClassification.kind },
        );
      }
      if (descriptor.sideEffect || (!legacyFields.describeSideEffect && !legacyFields.sideEffect)) {
        return descriptor;
      }
      const sideEffect = resolveSideEffect(legacyFields, input, identity);
      if (!sideEffect) return descriptor;
      return normalizeMappedDescriptor({ ...descriptor, sideEffect }, identity, input);
    },
  };
  return Object.freeze(contract);
}

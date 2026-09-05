import {
  ToolInvocationError,
  type NormalizedToolPermissionContract,
  type ToolSchemaValidator,
  type ToolTargetId,
  type ToolTargetIdentity,
} from "../lib/tools/invocation/index.ts";

export interface ToolTargetAvailabilityDecision {
  readonly eligible: boolean;
  readonly reason?: string | null;
  readonly code?: "TARGET_NOT_VISIBLE" | "TARGET_DISABLED_FOR_AGENT" | "TARGET_REVOKED";
}

export interface RegisteredToolTarget {
  readonly identity: ToolTargetIdentity;
  readonly label: string;
  readonly description: string;
  readonly parameters: unknown;
  readonly deferrable: boolean;
  readonly pinned: boolean;
  readonly permission: NormalizedToolPermissionContract;
  readonly validator: ToolSchemaValidator;
  readonly availability: ToolTargetAvailabilityDecision;
  readonly getCurrentGeneration: () => string | number;
  readonly isCurrentlyAvailable: (
    runtimeContext: unknown,
  ) => boolean | ToolTargetAvailabilityDecision | Promise<boolean | ToolTargetAvailabilityDecision>;
  readonly executeCanonical: (
    toolCallId: string,
    args: Record<string, unknown>,
    signal: AbortSignal | undefined,
    onUpdate: unknown,
    ctx: unknown,
  ) => Promise<unknown>;
  readonly normalizeResult: (result: unknown) => unknown;
}

export interface CatalogTargetReference {
  readonly serverId?: string;
  readonly toolName: string;
}

function stableText(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function indexKey(sourceId: string, name: string): string {
  return JSON.stringify([sourceId, name]);
}

function addToIndex<Key>(index: Map<Key, Set<RegisteredToolTarget>>, key: Key, target: RegisteredToolTarget) {
  const entries = index.get(key) ?? new Set<RegisteredToolTarget>();
  entries.add(target);
  index.set(key, entries);
}

function unionTargets(...groups: Array<Set<RegisteredToolTarget> | undefined>): RegisteredToolTarget[] {
  const targets = new Set<RegisteredToolTarget>();
  for (const group of groups) {
    if (!group) continue;
    for (const target of group) targets.add(target);
  }
  return [...targets];
}

function compareTargets(left: RegisteredToolTarget, right: RegisteredToolTarget): number {
  const leftId = left.identity.targetId;
  const rightId = right.identity.targetId;
  if (leftId === rightId) return 0;
  return leftId < rightId ? -1 : 1;
}

function targetError(
  code: "TARGET_NOT_FOUND" | "TARGET_AMBIGUOUS" | "CAPABILITY_MISMATCH",
  message: string,
  details: Record<string, unknown>,
): ToolInvocationError {
  return new ToolInvocationError({
    code,
    message,
    route: "direct",
    details,
  });
}

function assertTarget(target: RegisteredToolTarget): void {
  const identity = target?.identity;
  if (!identity?.targetId || !identity.sourceId || !identity.publicName || !identity.localName) {
    throw new TypeError("registered tool target requires a complete identity");
  }
  if (target.permission?.identity?.targetId !== identity.targetId) {
    throw new TypeError("registered tool target permission identity must match target identity");
  }
  if (!target.validator || typeof target.validator.validate !== "function") {
    throw new TypeError("registered tool target requires a schema validator");
  }
  if (typeof target.deferrable !== "boolean" || typeof target.pinned !== "boolean") {
    throw new TypeError("registered tool target requires explicit deferrable and pinned flags");
  }
  if (typeof target.availability?.eligible !== "boolean") {
    throw new TypeError("registered tool target requires an assembly availability decision");
  }
  for (const field of [
    "getCurrentGeneration",
    "isCurrentlyAvailable",
    "executeCanonical",
    "normalizeResult",
  ] as const) {
    if (typeof target[field] !== "function") {
      throw new TypeError(`registered tool target requires ${field}`);
    }
  }
}

export class ToolTargetRegistry {
  private readonly targetsById = new Map<ToolTargetId, RegisteredToolTarget>();
  private readonly targetsBySourceAndName = new Map<string, Set<RegisteredToolTarget>>();
  private readonly targetsByName = new Map<string, Set<RegisteredToolTarget>>();
  private readonly targetsByCapabilityBase = new Map<string, Set<RegisteredToolTarget>>();

  register(target: RegisteredToolTarget): RegisteredToolTarget {
    assertTarget(target);
    const { identity } = target;
    if (this.targetsById.has(identity.targetId)) {
      throw targetError(
        "TARGET_AMBIGUOUS",
        "A tool target with the same TargetId is already registered.",
        { targetId: identity.targetId },
      );
    }

    this.targetsById.set(identity.targetId, target);
    // 同一目标的显示名与本名可能相同，集合负责去重；二级索引从不覆盖已有目标。
    for (const name of new Set([identity.publicName, identity.localName])) {
      addToIndex(this.targetsBySourceAndName, indexKey(identity.sourceId, name), target);
      addToIndex(this.targetsByName, name, target);
    }
    addToIndex(this.targetsByCapabilityBase, identity.capabilityBase, target);
    return target;
  }

  getByTargetId(targetId: ToolTargetId | string): RegisteredToolTarget | null {
    return this.targetsById.get(targetId as ToolTargetId) ?? null;
  }

  resolveCatalogTarget(reference: CatalogTargetReference): RegisteredToolTarget {
    const sourceId = stableText(reference?.serverId);
    const toolName = stableText(reference?.toolName);
    if (!toolName) {
      throw targetError("TARGET_NOT_FOUND", "Tool target reference requires a tool name.", {
        serverId: sourceId || null,
      });
    }
    const matches = sourceId
      ? unionTargets(this.targetsBySourceAndName.get(indexKey(sourceId, toolName)))
      : unionTargets(this.targetsByName.get(toolName));
    if (matches.length === 0) {
      throw targetError("TARGET_NOT_FOUND", "No registered tool target matches this reference.", {
        serverId: sourceId || null,
        toolName,
      });
    }
    if (matches.length > 1) {
      throw targetError("TARGET_AMBIGUOUS", "Tool target reference matches more than one target.", {
        serverId: sourceId || null,
        toolName,
        targetIds: matches.map((target) => target.identity.targetId).sort(),
      });
    }
    return matches[0];
  }

  findByCapability(capability: string, action: string): RegisteredToolTarget | null {
    const normalizedCapability = stableText(capability);
    const normalizedAction = stableText(action);
    if (!normalizedCapability || !normalizedAction) return null;
    const matches: RegisteredToolTarget[] = [];
    let ownsCapabilityBase = false;
    for (const [capabilityBase, targets] of this.targetsByCapabilityBase) {
      if (normalizedCapability.startsWith(`${capabilityBase}.`)) ownsCapabilityBase = true;
      if (normalizedCapability !== `${capabilityBase}.${normalizedAction}`) continue;
      matches.push(...targets);
    }
    const uniqueMatches = [...new Set(matches)];
    if (uniqueMatches.length === 0) {
      if (ownsCapabilityBase) {
        throw targetError(
          "CAPABILITY_MISMATCH",
          "Capability action does not match the requested action.",
          { capability: normalizedCapability, action: normalizedAction },
        );
      }
      return null;
    }
    if (uniqueMatches.length > 1) {
      throw targetError("TARGET_AMBIGUOUS", "Capability matches more than one tool target.", {
        capability: normalizedCapability,
        action: normalizedAction,
        targetIds: uniqueMatches.map((target) => target.identity.targetId).sort(),
      });
    }
    return uniqueMatches[0];
  }

  listEligible(): RegisteredToolTarget[] {
    return [...this.targetsById.values()]
      .filter((target) => target.availability.eligible)
      .sort(compareTargets);
  }

  listDeferredCandidates(): RegisteredToolTarget[] {
    return this.listEligible().filter((target) => target.deferrable && !target.pinned);
  }
}

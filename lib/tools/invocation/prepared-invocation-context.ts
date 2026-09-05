import { AsyncLocalStorage } from "node:async_hooks";
import { createHash } from "node:crypto";
import type { NormalizedToolInvocationDescriptor } from "../../permission/tool-invocation-permission.ts";
import { snapshotToolInvocationInput } from "../../permission/tool-invocation-permission.ts";
import type { ToolInvocationRoute, ToolTargetId } from "./types.ts";

export interface PreparedInvocation {
  readonly targetId: ToolTargetId;
  readonly route: ToolInvocationRoute;
  readonly arguments: Readonly<Record<string, unknown>>;
  readonly argumentsDigest: string;
  readonly sessionId: string | null;
  readonly sessionPath: string | null;
  readonly agentId: string | null;
  readonly permission: Readonly<NormalizedToolInvocationDescriptor>;
  readonly lifecycleGeneration: string | number;
  readonly toolCallId: string;
  readonly createdAt: number;
}

export interface PreparedInvocationInput {
  readonly targetId: ToolTargetId;
  readonly route: ToolInvocationRoute;
  readonly arguments: Record<string, unknown>;
  readonly sessionId?: string | null;
  readonly sessionPath?: string | null;
  readonly agentId?: string | null;
  readonly permission: NormalizedToolInvocationDescriptor;
  readonly lifecycleGeneration: string | number;
  readonly toolCallId: string;
  readonly createdAt: number;
}

const preparedInvocationStorage = new AsyncLocalStorage<PreparedInvocation>();

function canonicalJson(value: unknown): string {
  if (value === null) return "null";
  if (typeof value === "string" || typeof value === "boolean") return JSON.stringify(value);
  if (typeof value === "number") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  const record = value as Record<string, unknown>;
  return `{${Object.keys(record).sort().map((key) => (
    `${JSON.stringify(key)}:${canonicalJson(record[key])}`
  )).join(",")}}`;
}

export function digestToolArguments(argumentsValue: Record<string, unknown>): string {
  const snapshot = snapshotToolInvocationInput(argumentsValue);
  if (
    snapshot.ok === false
    || !snapshot.value
    || typeof snapshot.value !== "object"
    || Array.isArray(snapshot.value)
  ) {
    throw new TypeError("tool arguments digest requires a bounded plain JSON object");
  }
  return createHash("sha256").update(canonicalJson(snapshot.value)).digest("hex");
}

export function createPreparedInvocation(input: PreparedInvocationInput): PreparedInvocation {
  const argumentsSnapshot = snapshotToolInvocationInput(input.arguments);
  const permissionSnapshot = snapshotToolInvocationInput(input.permission);
  if (
    argumentsSnapshot.ok === false
    || !argumentsSnapshot.value
    || typeof argumentsSnapshot.value !== "object"
    || Array.isArray(argumentsSnapshot.value)
  ) {
    throw new TypeError("prepared invocation requires bounded plain JSON arguments");
  }
  if (
    permissionSnapshot.ok === false
    || !permissionSnapshot.value
    || typeof permissionSnapshot.value !== "object"
    || Array.isArray(permissionSnapshot.value)
  ) {
    throw new TypeError("prepared invocation requires a bounded permission descriptor");
  }
  const preparedArguments = argumentsSnapshot.value as Readonly<Record<string, unknown>>;
  return Object.freeze({
    targetId: input.targetId,
    route: input.route,
    arguments: preparedArguments,
    argumentsDigest: digestToolArguments(preparedArguments),
    sessionId: input.sessionId ?? null,
    sessionPath: input.sessionPath ?? null,
    agentId: input.agentId ?? null,
    permission: permissionSnapshot.value as Readonly<NormalizedToolInvocationDescriptor>,
    lifecycleGeneration: input.lifecycleGeneration,
    toolCallId: input.toolCallId,
    createdAt: input.createdAt,
  });
}

export function runWithPreparedInvocation<Result>(
  prepared: PreparedInvocation,
  fn: () => Result,
): Result {
  return preparedInvocationStorage.run(prepared, fn);
}

export function getPreparedInvocation(): PreparedInvocation | null {
  return preparedInvocationStorage.getStore() ?? null;
}

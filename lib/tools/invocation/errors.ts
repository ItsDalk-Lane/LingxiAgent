import type { ToolInvocationRoute, ToolTargetId } from "./types.ts";

export const TOOL_INVOCATION_ERROR_CODES = [
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
] as const;

export type ToolInvocationErrorCode = (typeof TOOL_INVOCATION_ERROR_CODES)[number];

export interface ToolInvocationErrorInput {
  code: ToolInvocationErrorCode;
  message: string;
  route: ToolInvocationRoute;
  targetId?: ToolTargetId | null;
  sourceId?: string | null;
  cause?: unknown;
  details?: Record<string, unknown> | null;
}

const SENSITIVE_KEY = /(?:api[-_]?key|authorization|cookie|credential|password|secret|token)/i;
const BEARER_VALUE = /\bBearer\s+[^\s,;]+/gi;
const COMMON_SECRET_VALUE = /\b(?:sk|pk|token|secret|key)-[a-z0-9._-]{6,}\b/gi;
const UNIX_INTERNAL_PATH = /\/(?:Users|home|root|private|tmp|var|opt)\/[^\s,;]+/g;
const WINDOWS_INTERNAL_PATH = /\b[a-z]:\\(?:Users|Documents and Settings|ProgramData)\\[^\s,;]+/gi;

function redactText(value: string): string {
  return value
    .replace(BEARER_VALUE, "Bearer [REDACTED]")
    .replace(COMMON_SECRET_VALUE, "[REDACTED]")
    .replace(UNIX_INTERNAL_PATH, "[REDACTED_PATH]")
    .replace(WINDOWS_INTERNAL_PATH, "[REDACTED_PATH]");
}

function sanitizeValue(value: unknown, seen: WeakSet<object>): unknown {
  if (typeof value === "string") return redactText(value);
  if (value === null || typeof value !== "object") return value;
  if (seen.has(value)) return "[CIRCULAR]";
  seen.add(value);
  if (Array.isArray(value)) {
    return Object.freeze(value.map((item) => sanitizeValue(item, seen)));
  }
  if (value instanceof Error) {
    return Object.freeze({ name: value.name, message: redactText(value.message) });
  }
  const output: Record<string, unknown> = {};
  for (const [key, descriptor] of Object.entries(Object.getOwnPropertyDescriptors(value))) {
    if (!("value" in descriptor)) continue;
    output[key] = SENSITIVE_KEY.test(key)
      ? "[REDACTED]"
      : sanitizeValue(descriptor.value, seen);
  }
  return Object.freeze(output);
}

function sanitizeDetails(
  details: Record<string, unknown> | null | undefined,
): Readonly<Record<string, unknown>> | null {
  if (!details) return null;
  return sanitizeValue(details, new WeakSet()) as Readonly<Record<string, unknown>>;
}

export class ToolInvocationError extends Error {
  readonly code: ToolInvocationErrorCode;
  readonly route: ToolInvocationRoute;
  readonly targetId: ToolTargetId | null;
  readonly sourceId: string | null;
  override readonly cause: unknown;
  readonly details: Readonly<Record<string, unknown>> | null;

  constructor(input: ToolInvocationErrorInput) {
    super(redactText(input.message));
    this.name = "ToolInvocationError";
    this.code = input.code;
    this.route = input.route;
    this.targetId = input.targetId ?? null;
    this.sourceId = input.sourceId ?? null;
    this.cause = input.cause;
    this.details = sanitizeDetails(input.details);
  }

  toJSON(): Record<string, unknown> {
    return {
      name: this.name,
      message: this.message,
      code: this.code,
      route: this.route,
      targetId: this.targetId,
      sourceId: this.sourceId,
      details: this.details,
    };
  }
}

export function isToolInvocationError(value: unknown): value is ToolInvocationError {
  return value instanceof ToolInvocationError;
}

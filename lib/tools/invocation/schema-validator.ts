import type { TSchema } from "typebox";
import { Value } from "typebox/value";
import { snapshotToolInvocationInput } from "../../permission/tool-invocation-permission.ts";
import { ToolInvocationError } from "./errors.ts";
import type { ToolInvocationRoute, ToolTargetIdentity } from "./types.ts";

export interface ToolSchemaIssue {
  readonly path: string;
  readonly message: string;
}

export interface ToolSchemaValidator {
  readonly schema: TSchema;
  validate(
    argumentsValue: unknown,
    route?: ToolInvocationRoute,
  ): Record<string, unknown>;
}

const JSON_SCHEMA_TYPES = [
  "array",
  "boolean",
  "integer",
  "null",
  "number",
  "object",
  "string",
] as const;

const SCHEMA_NODE_REF = { $ref: "#/$defs/schema" };

// 工具参数只使用 JSON Schema 的数据校验子集。这里先用同一套运行时校验器检查
// schema 自身的关键字段形状，再拿实际 schema 做探针消费，避免注册一个表面是对象、
// 实际到调用时才会抛异常的契约。
const TOOL_SCHEMA_META_SCHEMA = {
  $defs: {
    schema: {
      type: "object",
      properties: {
        type: {
          anyOf: [
            { enum: JSON_SCHEMA_TYPES },
            {
              type: "array",
              minItems: 1,
              items: { enum: JSON_SCHEMA_TYPES },
            },
          ],
        },
        properties: {
          type: "object",
          additionalProperties: SCHEMA_NODE_REF,
        },
        required: {
          type: "array",
          items: { type: "string" },
        },
        items: {
          anyOf: [
            SCHEMA_NODE_REF,
            { type: "array", items: SCHEMA_NODE_REF },
          ],
        },
        anyOf: { type: "array", minItems: 1, items: SCHEMA_NODE_REF },
        oneOf: { type: "array", minItems: 1, items: SCHEMA_NODE_REF },
        allOf: { type: "array", minItems: 1, items: SCHEMA_NODE_REF },
        not: SCHEMA_NODE_REF,
        additionalProperties: {
          anyOf: [{ type: "boolean" }, SCHEMA_NODE_REF],
        },
        enum: { type: "array", minItems: 1 },
        minimum: { type: "number" },
        maximum: { type: "number" },
        exclusiveMinimum: { type: "number" },
        exclusiveMaximum: { type: "number" },
        multipleOf: { type: "number", exclusiveMinimum: 0 },
        minLength: { type: "integer", minimum: 0 },
        maxLength: { type: "integer", minimum: 0 },
        minItems: { type: "integer", minimum: 0 },
        maxItems: { type: "integer", minimum: 0 },
        pattern: { type: "string" },
      },
      additionalProperties: true,
    },
  },
  $ref: "#/$defs/schema",
} as TSchema;

const SCHEMA_CONSUMPTION_PROBES = [
  {},
  [],
  null,
  "schema-probe",
  0,
  true,
] as const;

function compareText(left: string, right: string): number {
  if (left === right) return 0;
  return left < right ? -1 : 1;
}

function normalizeIssues(
  errors: Array<{ instancePath?: unknown; message?: unknown }>,
): ToolSchemaIssue[] {
  return errors.map((error) => ({
    path: typeof error.instancePath === "string" && error.instancePath
      ? error.instancePath
      : "/",
    message: typeof error.message === "string" && error.message
      ? error.message
      : "schema validation failed",
  })).sort((left, right) => (
    compareText(left.path, right.path) || compareText(left.message, right.message)
  ));
}

function invocationError(
  code: "TOOL_SCHEMA_INVALID" | "ARGUMENTS_NOT_OBJECT" | "ARGUMENT_SCHEMA_INVALID",
  message: string,
  identity: ToolTargetIdentity,
  route: ToolInvocationRoute,
  details?: Record<string, unknown>,
  cause?: unknown,
): ToolInvocationError {
  return new ToolInvocationError({
    code,
    message,
    route,
    targetId: identity.targetId,
    sourceId: identity.sourceId,
    details,
    cause,
  });
}

function normalizeSchema(schema: unknown, identity: ToolTargetIdentity): TSchema {
  const snapshot = snapshotToolInvocationInput(schema);
  if (
    snapshot.ok === false
    || !snapshot.value
    || typeof snapshot.value !== "object"
    || Array.isArray(snapshot.value)
  ) {
    throw invocationError(
      "TOOL_SCHEMA_INVALID",
      "Tool parameter schema must be a bounded plain JSON object.",
      identity,
      "direct",
    );
  }
  const normalized = snapshot.value as TSchema;
  try {
    if (!Value.Check(TOOL_SCHEMA_META_SCHEMA, normalized)) {
      throw invocationError(
        "TOOL_SCHEMA_INVALID",
        "Tool parameter schema contains invalid schema fields.",
        identity,
        "direct",
        { issues: normalizeIssues(Value.Errors(TOOL_SCHEMA_META_SCHEMA, normalized)) },
      );
    }
    for (const probe of SCHEMA_CONSUMPTION_PROBES) {
      Value.Check(normalized, probe);
      Value.Errors(normalized, probe);
    }
  } catch (cause) {
    if (cause instanceof ToolInvocationError) throw cause;
    throw invocationError(
      "TOOL_SCHEMA_INVALID",
      "Tool parameter schema cannot be consumed by the runtime validator.",
      identity,
      "direct",
      undefined,
      cause,
    );
  }
  return normalized;
}

export function createToolSchemaValidator(
  schema: unknown,
  identity: ToolTargetIdentity,
): ToolSchemaValidator {
  const normalizedSchema = normalizeSchema(schema, identity);
  return Object.freeze({
    schema: normalizedSchema,
    validate(
      argumentsValue: unknown,
      route: ToolInvocationRoute = "direct",
    ): Record<string, unknown> {
      const snapshot = snapshotToolInvocationInput(argumentsValue);
      if (
        snapshot.ok === false
        || !snapshot.value
        || typeof snapshot.value !== "object"
        || Array.isArray(snapshot.value)
      ) {
        throw invocationError(
          "ARGUMENTS_NOT_OBJECT",
          "Tool arguments must be a bounded plain JSON object.",
          identity,
          route,
        );
      }
      try {
        if (Value.Check(normalizedSchema, argumentsValue)) {
          return argumentsValue as Record<string, unknown>;
        }
        throw invocationError(
          "ARGUMENT_SCHEMA_INVALID",
          "Tool arguments do not match the registered parameter schema.",
          identity,
          route,
          { issues: normalizeIssues(Value.Errors(normalizedSchema, argumentsValue)) },
        );
      } catch (cause) {
        if (cause instanceof ToolInvocationError) throw cause;
        throw invocationError(
          "ARGUMENT_SCHEMA_INVALID",
          "Tool arguments could not be validated against the registered parameter schema.",
          identity,
          route,
          undefined,
          cause,
        );
      }
    },
  });
}

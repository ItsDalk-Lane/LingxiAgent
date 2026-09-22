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
  errors: Array<{ path?: unknown; instancePath?: unknown; message?: unknown }>,
): ToolSchemaIssue[] {
  return errors.map((error) => ({
    // P06：本仓 typebox@1.1.38 的 Value.Errors 错误对象键为 keyword/schemaPath/
    // instancePath/params/message（无 path 键），instancePath 在嵌套路径（如 /labels/0）
    // 上本就有值，修复前 details/issuePaths 并未降级；真实缺口是 message 通用文案
    // （见下方 ARGUMENT_SCHEMA_INVALID 构造点）。兼认 path 属跨版本防御性兼容，
    // 对本版本是 no-op（无害保留）；root（instancePath 空串）落 "/"。
    path: typeof error.path === "string" && error.path
      ? error.path
      : (typeof error.instancePath === "string" && error.instancePath ? error.instancePath : "/"),
    message: typeof error.message === "string" && error.message
      ? error.message
      : "schema validation failed",
  })).sort((left, right) => (
    compareText(left.path, right.path) || compareText(left.message, right.message)
  ));
}

/**
 * P06：给模型可见 message 提取字段定位——真实缺口是修复前 message 为通用文案
 * （本版本 typebox@1.1.38 下 details/issuePaths 本就保真，非 FIX-1 恢复所得）。
 * 非 root 路径直接用路径；root 路径上 TypeBox 的必填缺失只写进 message
 * （"must have required properties title"），用保守正则提取属性名。
 * 只影响展示文案，details 仍是权威字段明细。
 */
function summarizeIssueFields(issues: ToolSchemaIssue[]): string[] {
  const fields: string[] = [];
  for (const issue of issues) {
    if (issue.path && issue.path !== "/") {
      fields.push(issue.path);
      continue;
    }
    const requiredMatch = /required propert(?:y|ies)\s+(.+)$/i.exec(issue.message);
    if (requiredMatch) {
      for (const token of requiredMatch[1].split(/,\s*/)) {
        const name = token.trim().replace(/^['"]|['"]$/g, "");
        if (name) fields.push(`/${name}`);
      }
    }
  }
  return [...new Set(fields)];
}

function issueDetails(issues: ToolSchemaIssue[]): {
  issues: ToolSchemaIssue[];
  issuePaths: string[];
} {
  return {
    issues,
    issuePaths: [...new Set(issues.map((issue) => issue.path))],
  };
}

const ROOT_SCHEMA_ISSUE = issueDetails([{ path: "/", message: "schema validation failed" }]);

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
      ROOT_SCHEMA_ISSUE,
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
        issueDetails(normalizeIssues(Value.Errors(TOOL_SCHEMA_META_SCHEMA, normalized))),
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
      ROOT_SCHEMA_ISSUE,
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
          ROOT_SCHEMA_ISSUE,
        );
      }
      try {
        if (Value.Check(normalizedSchema, argumentsValue)) {
          return argumentsValue as Record<string, unknown>;
        }
        // P06：把校验失败的 issue 字段定位并进 message——模型在工具结果里只能看到
        // error.message（pi-agent-loop 用 createErrorToolResult(error.message) 渲染），
        // 字段级明细留在 details 里到不了调用方，常驻规则「按指出的字段与约束修正」
        // 就落空。定位（如 /title、/metadata/owner）是结构信息，不含用户内容。
        const invalidFieldDetails = issueDetails(normalizeIssues(Value.Errors(normalizedSchema, argumentsValue)));
        const fieldList = summarizeIssueFields(invalidFieldDetails.issues).join(", ");
        throw invocationError(
          "ARGUMENT_SCHEMA_INVALID",
          fieldList
            ? `Tool arguments do not match the registered parameter schema. Invalid field(s): ${fieldList}.`
            : "Tool arguments do not match the registered parameter schema.",
          identity,
          route,
          invalidFieldDetails,
        );
      } catch (cause) {
        if (cause instanceof ToolInvocationError) throw cause;
        throw invocationError(
          "ARGUMENT_SCHEMA_INVALID",
          "Tool arguments could not be validated against the registered parameter schema.",
          identity,
          route,
          ROOT_SCHEMA_ISSUE,
          cause,
        );
      }
    },
  });
}

import { describe, expect, it } from "vitest";
import {
  createFirstPartyToolIdentity,
  createToolSchemaValidator,
  ToolInvocationError,
} from "../lib/tools/invocation/index.ts";

const identity = createFirstPartyToolIdentity({
  publicName: "schema_fixture",
  capabilityBase: "schema_fixture",
});

function captureError(call: () => unknown, code: ToolInvocationError["code"]): ToolInvocationError {
  try {
    call();
  } catch (error) {
    expect(error).toBeInstanceOf(ToolInvocationError);
    expect(error).toMatchObject({ code, targetId: identity.targetId });
    return error as ToolInvocationError;
  }
  throw new Error(`expected ${code}`);
}

describe("工具参数完整 schema 校验器", () => {
  it("在注册期拒绝运行时不能安全消费的 schema", () => {
    captureError(
      () => createToolSchemaValidator(null, identity),
      "TOOL_SCHEMA_INVALID",
    );
    captureError(
      () => createToolSchemaValidator({
        type: "object",
        properties: { nested: null },
      }, identity),
      "TOOL_SCHEMA_INVALID",
    );
    captureError(
      () => createToolSchemaValidator({ type: "string", pattern: "[" }, identity),
      "TOOL_SCHEMA_INVALID",
    );
  });

  it("拒绝缺失的 required 字段", () => {
    const validator = createToolSchemaValidator({
      type: "object",
      required: ["query"],
      properties: { query: { type: "string" } },
    }, identity);

    const error = captureError(
      () => validator.validate({}),
      "ARGUMENT_SCHEMA_INVALID",
    );
    expect(error.details?.issues).toEqual([
      expect.objectContaining({ path: "/", message: expect.stringContaining("query") }),
    ]);
    expect(error.details?.issuePaths).toEqual(["/"]);
  });

  it.each([null, [], "text", 1, true])("执行前拒绝非普通对象参数：%j", (argumentsValue) => {
    const validator = createToolSchemaValidator({ type: "object" }, identity);

    const error = captureError(
      () => validator.validate(argumentsValue, "deferred"),
      "ARGUMENTS_NOT_OBJECT",
    );
    expect(error.details?.issuePaths).toEqual(["/"]);
  });

  it("拒绝超过深度/数量上限的参数（P03-A02 过深对象）", () => {
    const validator = createToolSchemaValidator({ type: "object" }, identity);
    // MAX_INPUT_DEPTH=16：17 层嵌套在快照阶段被拒，不进入 schema 校验。
    let tooDeep: unknown = { leaf: true };
    for (let index = 0; index < 17; index += 1) tooDeep = { nested: tooDeep };
    captureError(
      () => validator.validate(tooDeep, "deferred"),
      "ARGUMENTS_NOT_OBJECT",
    );

    // MAX_INPUT_ITEMS=4096：超量扁平字段同样被拒。
    const tooWide: Record<string, number> = {};
    for (let index = 0; index < 4100; index += 1) tooWide[`f${index}`] = index;
    captureError(
      () => validator.validate(tooWide, "deferred"),
      "ARGUMENTS_NOT_OBJECT",
    );
  });

  it("完整执行 required、嵌套对象、数组项、enum、union、额外字段、整数和范围约束", () => {
    const validator = createToolSchemaValidator({
      type: "object",
      required: ["profile", "mode", "choice", "count"],
      additionalProperties: false,
      properties: {
        profile: {
          type: "object",
          required: ["name", "scores"],
          additionalProperties: false,
          properties: {
            name: { type: "string", minLength: 2 },
            scores: {
              type: "array",
              minItems: 1,
              items: { type: "integer", minimum: 1, maximum: 3 },
            },
          },
        },
        mode: { enum: ["fast", "detailed"] },
        choice: { anyOf: [{ type: "string" }, { type: "integer" }] },
        count: { type: "integer", minimum: 2, maximum: 4 },
      },
    }, identity);

    const error = captureError(() => validator.validate({
      profile: { name: "ok", scores: [0, "2"], extra: true },
      mode: "unknown",
      choice: false,
      count: 5,
    }), "ARGUMENT_SCHEMA_INVALID");
    const issues = error.details?.issues as Array<{ path: string; message: string }>;
    expect(error.details?.issuePaths).toEqual([...new Set(issues.map((issue) => issue.path))]);

    expect(issues.length).toBeGreaterThanOrEqual(8);
    expect(issues.map((issue) => issue.path)).toEqual(
      [...issues.map((issue) => issue.path)].sort((left, right) => left.localeCompare(right)),
    );
    expect(issues).toEqual(expect.arrayContaining([
      expect.objectContaining({ path: "/profile/scores/0" }),
      expect.objectContaining({ path: "/profile/scores/1" }),
      expect.objectContaining({ path: "/mode" }),
      expect.objectContaining({ path: "/choice" }),
      expect.objectContaining({ path: "/count" }),
    ]));
  });

  it("接受满足 union 与范围约束的对象并原样返回，不转换或删字段", () => {
    const validator = createToolSchemaValidator({
      type: "object",
      required: ["choice", "count"],
      additionalProperties: true,
      properties: {
        choice: { anyOf: [{ type: "string" }, { type: "integer" }] },
        count: { type: "integer", minimum: 2, maximum: 4 },
      },
    }, identity);
    const argumentsValue = { choice: 3, count: 2, untouched: "keep" };

    expect(validator.validate(argumentsValue)).toBe(argumentsValue);
    expect(argumentsValue).toEqual({ choice: 3, count: 2, untouched: "keep" });
  });
});

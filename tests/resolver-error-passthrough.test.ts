// 防回归：权限层不得把「调用方可自纠」的类型化错误（参数校验失败、目标
// 不存在等）伪装成无信息量的 resolver 内部故障。
//
// 源于 2026-09-20 事故：todo_write 缺 activeForm、grep 误传 query、
// current_status 无参，三类参数错误全部被裸 catch 吞成统一的
// "Tool invocation resolver failed before producing a descriptor."，
// 模型无从自纠，被误判为平台故障。本文件锁定三层行为：
//   1. 白名单类型化错误透传原始 code/message/details；
//   2. 白名单外（含权限结论）保持统一脱敏文案；
//   3. mcp_call 端到端：目标参数缺必填字段时，字段级校验错误到达调用方。
import { describe, expect, it, vi } from "vitest";
import { createToolCatalog } from "../core/tool-catalog.ts";
import { createBridgeTools } from "../core/tool-catalog-bridge.ts";
import { resolveToolInvocationPermission, type ToolInvocationPermissionResolution } from "../lib/permission/tool-invocation-permission.ts";
import {
  createFirstPartyToolIdentity,
  createPreparedInvocation,
  createToolSchemaValidator,
  ToolInvocationError,
  type ToolTargetId,
} from "../lib/tools/invocation/index.ts";

type ResolverFailureResolution = Extract<ToolInvocationPermissionResolution, { ok: false }>;

// strictNullChecks=false（tsconfig.test）下 TS 不做真值判别收窄：if (result.ok)
// 不会把 union 缩到 ok:false 分支。显式谓词让用例在任何严格度配置下都拿到 error。
function isResolverFailure(r: ToolInvocationPermissionResolution): r is ResolverFailureResolution {
  return r.ok === false;
}

function throwingTool(makeError: () => unknown) {
  return {
    name: "demo_tool",
    sessionPermission: {
      resolveInvocation: () => {
        throw makeError();
      },
    },
  };
}

describe("resolver 异常透传（防回归）", () => {
  it("白名单内的参数校验错误透传原始 code、描述与字段明细", () => {
    const tool = throwingTool(() => new ToolInvocationError({
      code: "ARGUMENT_SCHEMA_INVALID",
      message: "Tool arguments do not match the registered parameter schema.",
      route: "deferred",
      details: { issues: [{ path: "/todos/0", message: "Required property activeForm missing" }] },
    }));
    const result = resolveToolInvocationPermission(tool, {});

    expect(result.ok).toBe(false);
    if (!isResolverFailure(result)) return;
    expect(result.error.reason).toBe("resolver_threw");
    expect(result.error.invocationCode).toBe("ARGUMENT_SCHEMA_INVALID");
    expect(result.error.invocationMessage).toContain("do not match the registered parameter schema");
    expect(JSON.stringify(result.error.invocationDetails)).toContain("activeForm");
    // 无论是否透传，脱敏摘要都保留为排查底账
    expect(result.error.cause?.name).toBe("ToolInvocationError");
  });

  it("白名单外的权限结论不透传，保持统一脱敏文案", () => {
    const tool = throwingTool(() => new ToolInvocationError({
      code: "PERMISSION_DENIED",
      message: "internal adjudication detail",
      route: "deferred",
    }));
    const result = resolveToolInvocationPermission(tool, {});

    expect(result.ok).toBe(false);
    if (!isResolverFailure(result)) return;
    expect(result.error.reason).toBe("resolver_threw");
    expect(result.error.invocationCode).toBeUndefined();
    expect(result.error.message).toBe(
      "Tool invocation resolver failed before producing a descriptor.",
    );
    // 给模型的反馈不携带内部裁决文案
    expect(JSON.stringify(result.error.invocationDetails ?? {})).not.toContain("adjudication");
  });

  it("未知异常不透传原文，仅保留脱敏摘要底账", () => {
    const tool = throwingTool(() => new Error("burst at internal step"));
    const result = resolveToolInvocationPermission(tool, {});

    expect(result.ok).toBe(false);
    if (!isResolverFailure(result)) return;
    expect(result.error.invocationCode).toBeUndefined();
    expect(result.error.invocationMessage).toBeUndefined();
    expect(result.error.message).toBe(
      "Tool invocation resolver failed before producing a descriptor.",
    );
    expect(result.error.cause).toEqual({ name: "Error", message: "burst at internal step" });
  });
});

describe("mcp_call 端到端：目标参数错误到达调用方（事故复现）", () => {
  it("todo_write 缺必填 activeForm 时返回字段级校验错误而非 resolver 内部故障", () => {
    // 与生产 todo_write 同款 schema：content/activeForm/status 均必填
    const todoSchema = {
      type: "object",
      properties: {
        todos: {
          type: "array",
          items: {
            type: "object",
            required: ["content", "activeForm", "status"],
            properties: {
              content: { type: "string" },
              activeForm: { type: "string" },
              status: { enum: ["pending", "in_progress", "completed"] },
            },
          },
        },
      },
      required: ["todos"],
    };
    const catalog = createToolCatalog();
    catalog.registerSource("builtin", [{
      // ToolTargetId 是品牌字符串；目录条目按 identity 工厂的同一格式手写，边界处显式断言
      targetId: "tool:first-party:todo_write" as ToolTargetId,
      origin: "first-party" as const,
      sourceId: "first-party",
      serverId: "first-party",
      serverLabel: "内置",
      publicName: "todo_write",
      toolName: "todo_write",
      capabilityBase: "todo_write",
      description: "Manage the session todo list.",
      paramsSummary: "todos",
      lifecycleGeneration: 0,
      deferrable: true,
      pinned: false,
      schema: todoSchema,
    }]);

    const identity = createFirstPartyToolIdentity({
      publicName: "todo_write",
      capabilityBase: "todo_write",
    });
    const validator = createToolSchemaValidator(todoSchema, identity);
    // 与真实网关同构：resolvePermission 先做目标参数校验，不合法即抛
    const gateway = {
      resolvePermission: vi.fn((request: any) => {
        const validatedArgs = validator.validate(request.arguments, "deferred");
        return createPreparedInvocation({
          targetId: request.targetId,
          route: "deferred",
          arguments: validatedArgs,
          permission: {
            action: "replace",
            kind: "routine" as const,
            capability: "todo_write.replace",
          },
          lifecycleGeneration: 0,
          toolCallId: request.toolCallId,
          // fixture 固定时间戳：PreparedInvocationInput 要求必填，仅作透传
          createdAt: 1_700_000_000_000,
        });
      }),
      invoke: vi.fn(),
      canDelegateCapability: vi.fn(() => true),
    };

    const [, , callTool] = createBridgeTools({ catalog, gateway } as any);
    // 事故原始参数：只有 content 与 status，缺 activeForm
    const result = resolveToolInvocationPermission(callTool as any, {
      tool: "todo_write",
      arguments: { todos: [{ content: "测试任务", status: "pending" }] },
    });

    expect(result.ok).toBe(false);
    if (!isResolverFailure(result)) return;
    expect(result.error.reason).toBe("resolver_threw");
    // 事故断言：透传字段级校验错误，模型可按指出的字段自纠
    expect(result.error.invocationCode).toBe("ARGUMENT_SCHEMA_INVALID");
    expect(JSON.stringify(result.error.invocationDetails)).toContain("todos");
  });
});

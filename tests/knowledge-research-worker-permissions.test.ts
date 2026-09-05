import path from "node:path";
import { describe, expect, it, vi } from "vitest";
import { wrapWithSessionPermission } from "../lib/tools/session-permission-wrapper.ts";

const SESSION_PATH = path.resolve("/tmp/knowledge-research-permission.jsonl");
const RUNTIME_CONTEXT = { sessionManager: { getSessionFile: () => SESSION_PATH } };
const ROOT_TOOLS = ["knowledge_outline", "knowledge_search", "knowledge_read", "knowledge_grep",
  "knowledge_research_update", "knowledge_research_finish", "knowledge_delegate"];
const BLOCKED_TOOLS = ["knowledge_manage", "read", "write", "edit", "exec_command", "write_stdin", "terminal",
  "browser", "computer", "web_search", "web_fetch", "session", "session_send", "channel", "workflow",
  "update_settings", "subagent", "subagent_reply", "subagent_close", "file", "stage_files", "materialize",
  "search_memory", "pin_memory", "record_experience", "unknown_tool", "plugin_custom_read"];

function readTool(name: string) {
  return {
    name,
    sessionPermission: { resolveInvocation: vi.fn(() => ({ action: "read", kind: "read", capability: `${name}.read` })) },
    execute: vi.fn(async () => ({ content: [{ type: "text", text: "已执行" }], details: { executed: true } })),
  };
}

function researchDeps(surface: unknown, overrides: Record<string, unknown> = {}) {
  return { getPermissionMode: () => "read_only", approvalPolicy: "deny_on_prompt",
    permissionContext: { knowledgeResearchSurface: surface },
    approvalGateway: { review: vi.fn(async () => ({ action: "allow" })) },
    confirmStore: { create: vi.fn() }, ...overrides };
}

describe("研究执行时再次按宿主白名单限制工具", () => {
  it.each(["knowledge_coverage_read", "knowledge_completeness_mark"])("完整性专属工具 %s 只允许在完整性工作会话中执行", async name => {
    for (const surface of ["knowledge_research_root", "knowledge_research_worker", "knowledge_completeness_worker"]) {
      const tool = readTool(name), deps = researchDeps(surface);
      const [wrapped] = wrapWithSessionPermission([tool], deps);
      const result = await wrapped.execute("coverage-call", {}, undefined, undefined, RUNTIME_CONTEXT);
      if (surface === "knowledge_completeness_worker") {
        expect(result.details.executed).toBe(true);
        expect(tool.execute).toHaveBeenCalledOnce();
      } else {
        expect(result.details.errorCode).toBe("ACTION_BLOCKED_IN_KNOWLEDGE_RESEARCH");
        expect(tool.execute).not.toHaveBeenCalled();
      }
      expect(deps.approvalGateway.review).not.toHaveBeenCalled();
    }
  });

  it("完整性工作会话不能调用普通调查、委派、写入、网络或其他内置工具", async () => {
    for (const name of [...ROOT_TOOLS, ...BLOCKED_TOOLS]) {
      const tool = readTool(name);
      const [wrapped] = wrapWithSessionPermission([tool], researchDeps("knowledge_completeness_worker"));
      expect((await wrapped.execute("coverage-forbidden", {}, undefined, undefined, RUNTIME_CONTEXT)).details.errorCode)
        .toBe("ACTION_BLOCKED_IN_KNOWLEDGE_RESEARCH");
      expect(tool.execute).not.toHaveBeenCalled();
    }
  });
  it.each(ROOT_TOOLS)("主研究允许规定的 %s，只读声明仍经过原权限检查", async name => {
    const tool = readTool(name);
    const deps = researchDeps("knowledge_research_root");
    const [wrapped] = wrapWithSessionPermission([tool], deps);
    const result = await wrapped.execute("root-call", {}, undefined, undefined, RUNTIME_CONTEXT);
    expect(result.details.executed).toBe(true);
    expect(tool.execute).toHaveBeenCalledOnce();
    expect(tool.sessionPermission.resolveInvocation).toHaveBeenCalled();
    expect(deps.approvalGateway.review).not.toHaveBeenCalled();
    expect(deps.confirmStore.create).not.toHaveBeenCalled();
  });

  it.each(ROOT_TOOLS.slice(0, 5))("工作会话允许规定的 %s", async name => {
    const tool = readTool(name);
    const [wrapped] = wrapWithSessionPermission([tool], researchDeps("knowledge_research_worker"));
    expect((await wrapped.execute("worker-call", {}, undefined, undefined, RUNTIME_CONTEXT)).details.executed).toBe(true);
    expect(tool.execute).toHaveBeenCalledOnce();
  });

  it.each(["knowledge_research_root", "knowledge_research_worker"])("%s 禁止名单外所有工具，伪装只读也无效", async surface => {
    const deps = researchDeps(surface);
    for (const name of BLOCKED_TOOLS) {
      const tool = readTool(name);
      const [wrapped] = wrapWithSessionPermission([tool], deps);
      const result = await wrapped.execute("forbidden", { action: "read" }, undefined, undefined, RUNTIME_CONTEXT);
      expect(result.details, name).toMatchObject({ errorCode: "ACTION_BLOCKED_IN_KNOWLEDGE_RESEARCH", toolName: name });
      expect(tool.sessionPermission.resolveInvocation, name).not.toHaveBeenCalled();
      expect(tool.execute, name).not.toHaveBeenCalled();
    }
    expect(deps.approvalGateway.review).not.toHaveBeenCalled();
    expect(deps.confirmStore.create).not.toHaveBeenCalled();
  });

  it.each(["knowledge_delegate", "knowledge_research_finish"])("工作会话不得执行 %s，参数和运行上下文不能冒充主研究", async name => {
    const tool = readTool(name);
    const [wrapped] = wrapWithSessionPermission([tool], researchDeps("knowledge_research_worker"));
    const result = await wrapped.execute("spoofed-root", {
      knowledgeResearchSurface: "knowledge_research_root", role: "root", permissionMode: "operate",
      permissionContext: { knowledgeResearchSurface: "knowledge_research_root" },
    }, undefined, undefined, { ...RUNTIME_CONTEXT,
      knowledgeResearchSurface: "knowledge_research_root", role: "root",
      permissionContext: { knowledgeResearchSurface: "knowledge_research_root" },
    });
    expect(result.details.errorCode).toBe("ACTION_BLOCKED_IN_KNOWLEDGE_RESEARCH");
    expect(tool.sessionPermission.resolveInvocation).not.toHaveBeenCalled();
    expect(tool.execute).not.toHaveBeenCalled();
  });

  it("装配后的工作会话入口不能被可变依赖对象改成主研究", async () => {
    const tool = readTool("knowledge_research_finish");
    const deps = researchDeps("knowledge_research_worker");
    const [wrapped] = wrapWithSessionPermission([tool], deps);
    deps.permissionContext.knowledgeResearchSurface = "knowledge_research_root";
    expect((await wrapped.execute("mutated-host-data", {}, undefined, undefined, RUNTIME_CONTEXT)).details.errorCode)
      .toBe("ACTION_BLOCKED_IN_KNOWLEDGE_RESEARCH");
    expect(tool.execute).not.toHaveBeenCalled();
  });

  it.each(["auto", "operate", "ask"])("研究入口的宿主模式误配为 %s 时直接拒绝，不靠工具自报只读补救", async mode => {
    const tool = readTool("knowledge_search");
    const deps = researchDeps("knowledge_research_root", { getPermissionMode: () => mode });
    const [wrapped] = wrapWithSessionPermission([tool], deps);
    const result = await wrapped.execute("bad-mode", {}, undefined, undefined, RUNTIME_CONTEXT);
    expect(result.details.errorCode).toBe("KNOWLEDGE_RESEARCH_PERMISSION_INVALID");
    expect(tool.sessionPermission.resolveInvocation).not.toHaveBeenCalled();
    expect(tool.execute).not.toHaveBeenCalled();
    expect(deps.approvalGateway.review).not.toHaveBeenCalled();
    expect(deps.confirmStore.create).not.toHaveBeenCalled();
  });

  it.each(["interactive", "never"])("研究入口不能使用 %s 审批策略", async approvalPolicy => {
    const tool = readTool("knowledge_read");
    const deps = researchDeps("knowledge_research_worker", { approvalPolicy });
    const [wrapped] = wrapWithSessionPermission([tool], deps);
    expect((await wrapped.execute("bad-approval", {}, undefined, undefined, RUNTIME_CONTEXT)).details.errorCode)
      .toBe("KNOWLEDGE_RESEARCH_PERMISSION_INVALID");
    expect(tool.execute).not.toHaveBeenCalled();
    expect(deps.confirmStore.create).not.toHaveBeenCalled();
  });

  it("错误的宿主研究入口值不能静默退回普通会话", async () => {
    const tool = readTool("web_search");
    const [wrapped] = wrapWithSessionPermission([tool], researchDeps("knowledge_research_typo"));
    expect((await wrapped.execute("bad-surface", {}, undefined, undefined, RUNTIME_CONTEXT)).details.errorCode)
      .toBe("ACTION_BLOCKED_IN_KNOWLEDGE_RESEARCH");
    expect(tool.execute).not.toHaveBeenCalled();
  });

  it("名单内工具若声明写入行为，仍被原来的只读权限拒绝", async () => {
    const tool = readTool("knowledge_read");
    tool.sessionPermission.resolveInvocation.mockReturnValue({ action: "write", kind: "review", capability: "knowledge_read.write" });
    const [wrapped] = wrapWithSessionPermission([tool], researchDeps("knowledge_research_worker"));
    expect((await wrapped.execute("claimed-write", {}, undefined, undefined, RUNTIME_CONTEXT)).details.errorCode)
      .toBe("ACTION_BLOCKED_BY_READ_ONLY");
    expect(tool.execute).not.toHaveBeenCalled();
  });

  it("普通会话保持现有行为，参数或运行上下文中的研究字段不改变宿主权限", async () => {
    const tool = readTool("web_search");
    const [wrapped] = wrapWithSessionPermission([tool], { getPermissionMode: () => "read_only" });
    const result = await wrapped.execute("ordinary", { knowledgeResearchSurface: "knowledge_research_worker" },
      undefined, undefined, { ...RUNTIME_CONTEXT, permissionContext: { knowledgeResearchSurface: "knowledge_research_worker" } });
    expect(result.details.executed).toBe(true);
    expect(tool.execute).toHaveBeenCalledOnce();
  });
});

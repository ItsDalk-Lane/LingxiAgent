import { describe, expect, it } from "vitest";

import { summarizeToolStartArgs } from "../server/routes/chat.ts";

describe("chat tool_start arg summary", () => {
  it.each([
    ["mcp_call", { server: "zread", tool: "get_repo_structure" }],
    ["mcp_describe_tool", { server: "zread", name: "zread_get_repo_structure" }],
  ])("%s 保留目标名称，但不带入调用正文和无关身份字段", (toolName, target) => {
    expect(summarizeToolStartArgs(toolName, {
      server: "zread", tool: "get_repo_structure", name: "zread_get_repo_structure",
      arguments: { token: "credential-sentinel", content: "private body" },
      ...target,
    })).toEqual(target);
  });

  it("不向其他工具扩大身份字段范围，也不接受非字符串或空白目标", () => {
    expect(summarizeToolStartArgs("mcp_zread_read", {
      server: "zread", tool: "read", name: "private name", query: "repo",
    })).toEqual({ query: "repo" });
    expect(summarizeToolStartArgs("mcp_call", {
      server: { token: "credential-sentinel" }, tool: " ",
    })).toBeUndefined();
    expect(summarizeToolStartArgs("mcp_describe_tool", {
      server: 123, name: ["read"],
    })).toBeUndefined();
  });

  it("桥接目标沿用摘要长度上限", () => {
    const name = "a".repeat(3000);
    const shortened = "a".repeat(2047) + "…";
    expect(summarizeToolStartArgs("mcp_call", { server: name, tool: name })).toEqual({
      server: shortened, tool: shortened,
    });
    expect(summarizeToolStartArgs("mcp_describe_tool", { name })).toEqual({ name: shortened });
  });

  it("保留读取范围，并遮盖设置值中的凭证", () => {
    expect(summarizeToolStartArgs("read", { path: "/tmp/a.ts", offset: 15, limit: 20 })).toEqual({
      path: "/tmp/a.ts", offset: 15, limit: 20,
    });
    expect(summarizeToolStartArgs("update_settings", { key: "provider.apiKey", value: "credential-sentinel" })).toEqual({
      key: "provider.apiKey", value: "********",
    });
    expect(summarizeToolStartArgs("knowledge_research_worker", { prompt: "hidden", label: "worker" })).toBeUndefined();
  });
  it("does not leak unsummarized args for other tools", () => {
    expect(summarizeToolStartArgs("write", {
      file_path: "/tmp/a.txt",
      content: "secret body",
    }, 1_700_000_000_000)).toEqual({
      file_path: "/tmp/a.txt",
    });
  });

  it("keeps exec_command cmd so the chat UI can show the running command", () => {
    expect(summarizeToolStartArgs("exec_command", {
      cmd: "python -m pip install numpy",
      command: "legacy command",
      content: "secret body",
    })).toEqual({
      command: "legacy command",
      cmd: "python -m pip install numpy",
    });
  });

  it("keeps write_stdin input summary for terminal continuation display", () => {
    expect(summarizeToolStartArgs("write_stdin", {
      process_id: "term_1",
      chars: "q\n",
      hidden: "secret",
    })).toEqual({
      chars: "q\n",
      process_id: "term_1",
    });
  });
});

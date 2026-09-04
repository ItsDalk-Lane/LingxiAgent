import { describe, expect, it } from "vitest";
import {
  assertAllToolsCategorized,
  CORE_TOOL_NAMES,
  getKnowledgeResearchToolNames,
  GLOBAL_TOOL_NAMES,
  isKnowledgeResearchSurface,
  LEGACY_INTERNAL_TOOL_NAMES,
  OPTIONAL_TOOL_NAMES,
  STANDARD_TOOL_NAMES,
} from "../shared/tool-categories.ts";

describe("研究工具清单固定为任务规定的入口", () => {
  it("主研究只有七种工具，工作会话只有五种工具", () => {
    expect(getKnowledgeResearchToolNames("knowledge_research_root")).toEqual([
      "knowledge_outline", "knowledge_search", "knowledge_read", "knowledge_grep",
      "knowledge_research_update", "knowledge_research_finish", "knowledge_delegate",
    ]);
    expect(getKnowledgeResearchToolNames("knowledge_research_worker")).toEqual([
      "knowledge_outline", "knowledge_search", "knowledge_read", "knowledge_grep", "knowledge_research_update",
    ]);
  });

  it("只认可两个准确的宿主入口名，不把普通会话或近似名字当成研究入口", () => {
    expect(isKnowledgeResearchSurface("knowledge_research_root")).toBe(true);
    expect(isKnowledgeResearchSurface("knowledge_research_worker")).toBe(true);
    for (const value of [null, undefined, "chat", "subagent", "knowledge_research", "knowledge_research_root ",
      "KNOWLEDGE_RESEARCH_ROOT", {}, ["knowledge_research_root"]]) {
      expect(isKnowledgeResearchSurface(value)).toBe(false);
    }
  });

  it("调用方不能通过修改返回数组给研究入口增加工具", () => {
    for (const surface of ["knowledge_research_root", "knowledge_research_worker"] as const) {
      const names = getKnowledgeResearchToolNames(surface);
      expect(Object.isFrozen(names)).toBe(true);
      expect(() => (names as string[]).push("exec_command")).toThrow();
      expect(getKnowledgeResearchToolNames(surface)).not.toContain("exec_command");
    }
  });

  it("三种研究专用工具各归入一次标准工具分类", () => {
    const names = ["knowledge_research_update", "knowledge_research_finish", "knowledge_delegate"];
    expect(() => assertAllToolsCategorized(names)).not.toThrow();
    const all = [...CORE_TOOL_NAMES, ...STANDARD_TOOL_NAMES, ...OPTIONAL_TOOL_NAMES,
      ...GLOBAL_TOOL_NAMES, ...LEGACY_INTERNAL_TOOL_NAMES];
    for (const name of names) {
      expect(STANDARD_TOOL_NAMES).toContain(name);
      expect(all.filter(item => item === name)).toHaveLength(1);
    }
  });
});

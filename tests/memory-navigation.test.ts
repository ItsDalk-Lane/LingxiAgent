import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";

vi.mock("../lib/i18n.js", () => ({
  getLocale: () => "zh-CN",
}));

vi.mock("../core/llm-client.js", () => ({
  callText: vi.fn().mockResolvedValue("[]"),
}));

import {
  buildNavigationSection,
  refreshNavigationFile,
  NAVIGATION_MAX_SESSIONS,
  NAVIGATION_MAX_TAGS,
  NAVIGATION_MAX_CHARS,
} from "../lib/memory/navigation.ts";
import { assemble, buildCompiledMemoryMarkdown } from "../lib/memory/compile.ts";
import { FactStore } from "../lib/memory/fact-store.ts";

describe("记忆检索导航节", () => {
  let tmpDir;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-memory-nav-"));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  it("buildNavigationSection：zh 输出包含会话行、标签行与工具提示", () => {
    const section = buildNavigationSection({
      recentSessions: [
        { title: "调试知识库检索", sessionId: "abcdefgh-1234" },
        { title: "", sessionId: "ijklmnop-5678" }, // 无标题 →（无标题）
      ],
      tagCounts: [{ tag: "饮食偏好", count: 3 }, { tag: "工作", count: 1 }],
      isZh: true,
    });
    expect(section).toContain("调试知识库检索（abcdefgh）");
    expect(section).toContain("（无标题）（ijklmnop）");
    expect(section).toContain("饮食偏好(3)");
    expect(section).toContain("search_memory");
    expect(section).toContain("session");
  });

  it("buildNavigationSection：en 输出", () => {
    const section = buildNavigationSection({
      recentSessions: [{ title: "Debug retrieval", sessionId: "abcd1234" }],
      tagCounts: [{ tag: "food", count: 2 }],
      isZh: false,
    });
    expect(section).toContain("Recent sessions");
    expect(section).toContain("food(2)");
  });

  it("buildNavigationSection：无任何数据返回空串", () => {
    expect(buildNavigationSection({ isZh: true })).toBe("");
    expect(buildNavigationSection({ recentSessions: [], tagCounts: [], isZh: true })).toBe("");
  });

  it("buildNavigationSection：会话/标签/总长上限", () => {
    const sessions = Array.from({ length: NAVIGATION_MAX_SESSIONS + 3 }, (_, i) => ({
      title: `会话${i}`,
      sessionId: `sid-${i}`,
    }));
    const tags = Array.from({ length: NAVIGATION_MAX_TAGS + 4 }, (_, i) => ({
      tag: `标签${i}`,
      count: i,
    }));
    const section = buildNavigationSection({ recentSessions: sessions, tagCounts: tags, isZh: true });
    expect(section).not.toContain(`会话${NAVIGATION_MAX_SESSIONS}`);
    expect(section).not.toContain(`标签${NAVIGATION_MAX_TAGS}`);
    expect(section.length).toBeLessThanOrEqual(NAVIGATION_MAX_CHARS + 3); // 截断省略号
  });

  it("refreshNavigationFile：过滤 agent、跳过已删 agent、写盘成功", async () => {
    const store = new FactStore(path.join(tmpDir, "facts.db"));
    store.addBatch([
      { fact: "用户不喜欢香菜", tags: ["饮食偏好", "食物"], time: "2026-08-01T10:00" },
      { fact: "用户不吃辣", tags: ["饮食偏好"], time: "2026-08-02T10:00" },
    ]);
    const listSessions = vi.fn().mockResolvedValue([
      { agentId: "agent-a", title: "我的会话", sessionId: "sess-aaa11111", modified: 2 },
      { agentId: "agent-b", title: "别人的会话", sessionId: "sess-bbb22222", modified: 3 },
      { agentId: "agent-a", agentDeleted: true, title: "已删", sessionId: "sess-ccc33333", modified: 5 },
      { agentId: "agent-a", title: null, firstMessage: "首条消息很长的内容", sessionId: "sess-ddd44444", modified: 1 },
    ]);
    const navPath = path.join(tmpDir, "memory", "navigation.md");
    const ok = await refreshNavigationFile({ listSessions, agentId: "agent-a", factStore: store, navigationPath: navPath });
    expect(ok).toBe(true);
    const content = fs.readFileSync(navPath, "utf-8");
    expect(content).toContain("我的会话（sess-aaa）");
    expect(content).toContain("首条消息很长的内容");
    expect(content).not.toContain("别人的会话");
    expect(content).not.toContain("已删");
    expect(content).toContain("饮食偏好(2)");
    store.close();
  });

  it("refreshNavigationFile：listSessions 抛错不外抛，返回 false", async () => {
    const navPath = path.join(tmpDir, "navigation.md");
    const ok = await refreshNavigationFile({
      listSessions: vi.fn().mockRejectedValue(new Error("boom")),
      agentId: "a",
      navigationPath: navPath,
    });
    expect(ok).toBe(false);
  });

  it("assemble：navigation.md 存在时拼为第 5 段，缺失时保持四段不变", () => {
    const write = (name, content) => fs.writeFileSync(path.join(tmpDir, name), content, "utf-8");
    write("facts.md", "- 事实A");
    write("today.md", "- 今天B");
    write("week.md", "### 2026-08-30\n- 昨天C");
    write("longterm.md", "- 长期D");
    fs.mkdirSync(path.join(tmpDir, "sub"), { recursive: true });
    const memoryMd = path.join(tmpDir, "sub", "memory.md");

    // 无 navigation.md：四段，输出与旧行为一致
    assemble(
      path.join(tmpDir, "facts.md"),
      path.join(tmpDir, "today.md"),
      path.join(tmpDir, "week.md"),
      path.join(tmpDir, "longterm.md"),
      memoryMd,
    );
    let md = fs.readFileSync(memoryMd, "utf-8");
    expect(md).toContain("## 重要事实");
    expect(md).toContain("## 长期情况");
    expect(md).not.toContain("记忆检索导航");

    // 有 navigation.md（放在 memory.md 同目录，默认派生路径）：拼第 5 段
    fs.writeFileSync(path.join(tmpDir, "sub", "navigation.md"), "最近会话：\n- 某会话（abcd1234）", "utf-8");
    assemble(
      path.join(tmpDir, "facts.md"),
      path.join(tmpDir, "today.md"),
      path.join(tmpDir, "week.md"),
      path.join(tmpDir, "longterm.md"),
      memoryMd,
    );
    md = fs.readFileSync(memoryMd, "utf-8");
    expect(md).toContain("## 记忆检索导航");
    expect(md).toContain("某会话（abcd1234）");
    expect(md.indexOf("## 长期情况")).toBeLessThan(md.indexOf("## 记忆检索导航"));

    // 显式传空串跳过导航（测试隔离出口）
    assemble(
      path.join(tmpDir, "facts.md"),
      path.join(tmpDir, "today.md"),
      path.join(tmpDir, "week.md"),
      path.join(tmpDir, "longterm.md"),
      memoryMd,
      "",
    );
    md = fs.readFileSync(memoryMd, "utf-8");
    expect(md).not.toContain("记忆检索导航");
  });

  it("buildCompiledMemoryMarkdown：navigation 空串时输出与旧格式逐字节一致", () => {
    const withNav = buildCompiledMemoryMarkdown({ facts: "A", today: "B", week: "", longterm: "D", navigation: "" });
    const withoutNav = buildCompiledMemoryMarkdown({ facts: "A", today: "B", week: "", longterm: "D" });
    expect(withNav).toBe(withoutNav);
  });
});

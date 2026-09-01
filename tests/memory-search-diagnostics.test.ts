import { describe, it, expect, beforeEach, afterEach } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";

import { loadLocale } from "../lib/i18n.ts";
import { FactStore } from "../lib/memory/fact-store.ts";
import { createMemorySearchTool } from "../lib/memory/memory-search.ts";

describe("search_memory 零结果诊断", () => {
  let tmpDir;
  let store;

  beforeEach(() => {
    loadLocale("zh-CN");
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-memory-diag-"));
    store = new FactStore(path.join(tmpDir, "facts.db"));
    store.addBatch([
      { fact: "用户不喜欢香菜", tags: ["饮食偏好"], time: "2026-08-01T10:00" },
      { fact: "用户养了一只猫叫橘子", tags: ["宠物"], time: "2026-08-02T10:00" },
      { fact: "项目 deadline 是九月底", tags: ["工作"], time: "2026-08-03T10:00" },
    ]);
  });

  afterEach(() => {
    store.close();
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  it("countFullTextMatches：命中词计数 > 0，未命中词为 0", () => {
    expect(store.countFullTextMatches("香菜")).toBeGreaterThan(0);
    expect(store.countFullTextMatches("deadline")).toBeGreaterThan(0);
    expect(store.countFullTextMatches("不存在的词xyz")).toBe(0);
  });

  it("countFullTextMatches：含引号等特殊字符不抛错", () => {
    expect(() => store.countFullTextMatches('"哈"噜')).not.toThrow();
    expect(() => store.countFullTextMatches("a OR b")).not.toThrow();
    expect(store.countFullTextMatches("")).toBe(0);
  });

  it("countFullTextMatches：CJK 零命中时走 LIKE 兜底口径", () => {
    // LIKE 匹配 fact 原文子串：『橘子』在原文里但可能不在 FTS n-gram 里
    expect(store.countFullTextMatches("橘子")).toBeGreaterThan(0);
  });

  it("零结果时返回逐词命中数与重试提示", async () => {
    const tool = createMemorySearchTool(store);
    const res = await tool.execute("t1", { query: "不存在词xyz 也不存在词abc" });
    const text = res.content[0].text;
    expect(text).toContain("不存在词xyz");
    expect(text).toContain("也不存在词abc");
    expect(text).toMatch(/0 条/);
    expect(text).toContain("部分检索词无命中");
    expect((res.details as any).diagnostics.terms).toEqual([
      { term: "不存在词xyz", count: expect.any(Number) },
      { term: "也不存在词abc", count: expect.any(Number) },
    ]);
  });

  it("词各有命中但结果被日期过滤清零时，有逐词计数、无重试提示", async () => {
    const tool = createMemorySearchTool(store);
    const res = await tool.execute("t2", {
      query: "香菜",
      date_from: "2027-01-01",
    });
    const text = res.content[0].text;
    expect(text).toContain("『香菜』");
    expect(text).not.toContain("部分检索词无命中");
    expect(Array.isArray((res.details as any).diagnostics.terms)).toBe(true);
  });

  it("超过 6 个词只取前 6 个做诊断", async () => {
    const tool = createMemorySearchTool(store);
    const res = await tool.execute("t3", {
      query: "a1 a2 a3 a4 a5 a6 a7 a8",
    });
    expect((res.details as any).diagnostics.terms).toHaveLength(6);
  });

  it("仅标签查询零结果时（无 query）不生成诊断", async () => {
    const tool = createMemorySearchTool(store);
    const res = await tool.execute("t4", { tags: ["不存在的标签"] });
    expect(res.details).toEqual({});
    expect(res.content[0].text).not.toContain("『");
  });

  it("正常命中路径不受诊断影响", async () => {
    const tool = createMemorySearchTool(store);
    const res = await tool.execute("t5", { query: "香菜" });
    expect(res.content[0].text).toContain("用户不喜欢香菜");
    expect((res.details as any).resultCount).toBeGreaterThan(0);
  });
});

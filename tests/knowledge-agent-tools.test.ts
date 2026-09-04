import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

import { resolveKnowledgeChunkerConfig } from "../lib/knowledge/chunker.ts";
import * as coverageUnits from "../lib/knowledge/knowledge-coverage-unit.ts";
import { createKnowledgeOutlineTool } from "../lib/tools/knowledge-outline-tool.ts";
import { createKnowledgeGrepTool } from "../lib/tools/knowledge-grep-tool.ts";
import { createKnowledgeManageTool } from "../lib/tools/knowledge-manage-tool.ts";
import { classifySessionPermission } from "../core/session-permission-mode.ts";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import type { KnowledgeTurnScope } from "../lib/knowledge/types.ts";

const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];

function tempHome() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-agent-tools-"));
  tempDirs.push(dir);
  return dir;
}

afterEach(() => {
  vi.restoreAllMocks();
  for (const manager of managers.splice(0)) manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

/** 主会话身份：scope 绑定该 sessionPath，工具执行上下文与之匹配才允许读取。 */
const MAIN_SESSION_PATH = "/tmp/lingxi-knowledge-agent-tools/main-session.jsonl";
const MAIN_SESSION = { sessionPath: MAIN_SESSION_PATH, scopeOwnerSessionPath: MAIN_SESSION_PATH };

const MARKDOWN_TEXT = [
  "# 交付计划",
  "苹果项目的交付日期是九月十五日，负责人是王芳。",
  "火星项目的预算是八百万元，负责人是李雷。",
  "# 风险登记",
  "蓝山项目仍在风险评估阶段。",
].join("\n");

/**
 * 带 heading 结构的 ready 源：markdown 文件导入 + parseSource（blocks 带
 * headingPath），返回 manager/notebook/imported/artifact。
 */
async function setupMarkdownSource(options: { parse?: boolean } = {}) {
  const studioId = "studio-a";
  const manager = new KnowledgeManager({ lingxiHome: tempHome() });
  managers.push(manager);
  const notebook = manager.createNotebook({ studioId, name: "资料" });
  const filesDir = tempHome();
  const filePath = path.join(filesDir, "项目.md");
  fs.writeFileSync(filePath, MARKDOWN_TEXT, "utf8");
  const imported = await manager.importFile({ studioId, notebookId: notebook.id, filePath });
  const artifact = options.parse === false
    ? null
    : await manager.parseSource({ studioId, sourceId: imported.source.id });
  if (artifact) {
    const targetChars = manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId: notebook.id });
    manager.queryService.indexArtifactForIngestion(studioId, artifact.id, { targetChars });
    const blocks = manager.listArtifactBlocks({ studioId, parseArtifactId: artifact.id });
    manager.store.resolveNotebookRetrievalProfile({ studioId, notebookId: notebook.id,
      strategy: resolveKnowledgeChunkerConfig(blocks, { targetChars }).strategy });
  }
  return { manager, studioId, notebook, imported, artifact };
}

function createScope(
  manager: KnowledgeManager,
  studioId: string,
  notebookIds: string[],
): KnowledgeTurnScope {
  return manager.createTurnScope({
    studioId,
    sessionPath: MAIN_SESSION_PATH,
    notebookIds,
  });
}

function makeOutlineTool(
  manager: KnowledgeManager,
  studioId: string,
  sessionContext: { sessionPath: string | null; scopeOwnerSessionPath: string | null } = MAIN_SESSION,
) {
  return createKnowledgeOutlineTool({
    getKnowledge: () => manager,
    getStudioId: () => studioId,
    resolveSessionContext: () => sessionContext,
  });
}

function makeGrepTool(
  manager: KnowledgeManager,
  studioId: string,
  sessionContext: { sessionPath: string | null; scopeOwnerSessionPath: string | null } = MAIN_SESSION,
) {
  return createKnowledgeGrepTool({
    getKnowledge: () => manager,
    getStudioId: () => studioId,
    resolveSessionContext: () => sessionContext,
  });
}

function makeManageTool(manager: KnowledgeManager, studioId: string) {
  return createKnowledgeManageTool({
    getKnowledge: () => manager,
    getStudioId: () => studioId,
  });
}

function parseResult(result: any) {
  expect(result?.isError).toBeFalsy();
  return JSON.parse(result.content[0].text);
}

function expectScopeViolation(result: any) {
  expect(result.isError).toBe(true);
  expect(result.content[0].text).toContain("KNOWLEDGE_SCOPE_VIOLATION");
}

// ─────────────────────────── knowledge_outline ───────────────────────────

describe("knowledge_outline 工具（scope 冻结集合结构枚举）", () => {
  it("列出选中 notebook 的冻结源结构（名称/类型/fidelity/chunk 数/首层 heading），不列 scope 外内容", async () => {
    const { manager, studioId, notebook, imported } = await setupMarkdownSource();
    // 同 studio 的未选中笔记本 C（独有源）：outline 不得出现。
    const notebookC = manager.createNotebook({ studioId, name: "未选中" });
    const importedC = await manager.importPastedText({
      studioId,
      notebookId: notebookC.id,
      text: "C 的私有内容。",
      displayName: "C.txt",
    });
    const scope = createScope(manager, studioId, [notebook.id]);
    const units = vi.spyOn(coverageUnits, "buildCoverageUnits");
    const fullBlocks = vi.spyOn(manager.store, "listArtifactBlocks");
    const fullChunks = vi.spyOn(manager.indexStore, "listVariantChunks");
    const expectedChunkCount = Number(manager.indexStore.db.prepare("SELECT COUNT(*) AS count FROM knowledge_chunks WHERE parse_artifact_id = ?").get(scope.sources[0].parseArtifactId).count);
    const payload = parseResult(await makeOutlineTool(manager, studioId).execute("call-1", { scopeId: scope.id }));

    expect(payload.scopeId).toBe(scope.id);
    expect(payload.notebooks).toHaveLength(1);
    const listed = payload.notebooks[0];
    expect(listed.notebookId).toBe(notebook.id);
    expect(listed.notebookName).toBe("资料");
    expect(listed.sources).toHaveLength(1);
    const source = listed.sources[0];
    expect(source.sourceId).toBe(imported.source.id);
    expect(source.sourceName).toBe("项目.md");
    expect(source.sourceType).toBe("file");
    // 目录改用持久化块数与章节元数据；不能把索引数量冒充完整性覆盖单位。
    expect(source.fidelity).toBe("citation_grade");
    expect(source).not.toHaveProperty("coverageUnits");
    expect(source.chunkCount).toBe(expectedChunkCount);
    expect(source.chunkCount).toBeGreaterThan(0);
    expect(source.status).toBe("ready");
    expect(source.sectionKeys).toEqual(["交付计划", "风险登记"]);
    expect(units).not.toHaveBeenCalled();
    expect(fullBlocks).not.toHaveBeenCalled();
    expect(fullChunks).not.toHaveBeenCalled();
    expect(source.blockCount).toBeGreaterThan(0);
    expect(source.parseArtifactId).toBe(scope.sources[0].parseArtifactId);
    expect(source.contentSnapshotId).toBe(scope.sources[0].contentSnapshotId);
    expect(source.headings).toEqual(["交付计划", "风险登记"]);
    expect(source.headingsTruncated).toBe(false);
    // scope 外 notebook/source 绝不出现。
    const allSourceIds = JSON.stringify(payload);
    expect(allSourceIds).not.toContain(notebookC.id);
    expect(allSourceIds).not.toContain(importedC.source.id);
  });

  it("scopeId 缺失 / 伪造 / 已关闭 → KNOWLEDGE_SCOPE_VIOLATION，不回落全 studio 行为", async () => {
    const { manager, studioId, notebook } = await setupMarkdownSource();
    const scope = createScope(manager, studioId, [notebook.id]);
    const tool = makeOutlineTool(manager, studioId);

    expectScopeViolation(await tool.execute("call-1", {}));
    expectScopeViolation(await tool.execute("call-2", { scopeId: "kts_forged" }));

    manager.closeTurnScope({ scopeId: scope.id });
    expectScopeViolation(await tool.execute("call-3", { scopeId: scope.id }));
  });

  it("跨 session / 跨 studio / 无会话上下文显式拒绝", async () => {
    const { manager, studioId, notebook } = await setupMarkdownSource();
    const scope = createScope(manager, studioId, [notebook.id]);

    const otherSession = makeOutlineTool(manager, studioId, {
      sessionPath: "/tmp/lingxi-knowledge-agent-tools/other-session.jsonl",
      scopeOwnerSessionPath: "/tmp/lingxi-knowledge-agent-tools/other-session.jsonl",
    });
    expectScopeViolation(await otherSession.execute("call-1", { scopeId: scope.id }));

    const otherStudio = makeOutlineTool(manager, "studio-b");
    expectScopeViolation(await otherStudio.execute("call-2", { scopeId: scope.id }));

    const noSession = createKnowledgeOutlineTool({
      getKnowledge: () => manager,
      getStudioId: () => studioId,
    });
    const result = await noSession.execute("call-3", { scopeId: scope.id });
    expect(result.isError).toBe(true);
    expect(result.content[0].text).toContain("KNOWLEDGE_MODEL_UNAVAILABLE");
  });

  it("subagent 子会话继承父会话 scope；父会话不符则拒绝", async () => {
    const { manager, studioId, notebook } = await setupMarkdownSource();
    const scope = createScope(manager, studioId, [notebook.id]);
    const childSession = makeOutlineTool(manager, studioId, {
      sessionPath: "/tmp/lingxi-knowledge-agent-tools/subagent-child.jsonl",
      scopeOwnerSessionPath: MAIN_SESSION_PATH,
    });
    const ok = await childSession.execute("call-1", { scopeId: scope.id });
    expect(ok.isError).toBeFalsy();

    const stranger = makeOutlineTool(manager, studioId, {
      sessionPath: "/tmp/lingxi-knowledge-agent-tools/subagent-child.jsonl",
      scopeOwnerSessionPath: "/tmp/lingxi-knowledge-agent-tools/stranger.jsonl",
    });
    expectScopeViolation(await stranger.execute("call-2", { scopeId: scope.id }));
  });

  it("未解析的源 fidelity=unavailable 单列，不整单失败也不静默省略", async () => {
    const { manager, studioId, notebook, imported } = await setupMarkdownSource({ parse: false });
    const scope = createScope(manager, studioId, [notebook.id]);
    expect(scope.sources[0].parseArtifactId).toBeNull();
    const payload = parseResult(await makeOutlineTool(manager, studioId).execute("call-1", { scopeId: scope.id }));
    const source = payload.notebooks[0].sources[0];
    expect(source.sourceId).toBe(imported.source.id);
    expect(source.fidelity).toBe("unavailable");
    expect(source).not.toHaveProperty("coverageUnits");
    expect(source.chunkCount).toBe(0);
    expect(source.status).toBe("parse_pending");
    expect(source.headings).toEqual([]);
  });
});

// ─────────────────────────── knowledge_grep ───────────────────────────

describe("knowledge_grep 工具（冻结原文确定性扫描）", () => {
  it("literal 匹配返回命中行与 provenance（sourceId/blockId/offset/headingPath）", async () => {
    const { manager, studioId, notebook, imported, artifact } = await setupMarkdownSource();
    const scope = createScope(manager, studioId, [notebook.id]);
    const payload = parseResult(await makeGrepTool(manager, studioId).execute("call-1", {
      scopeId: scope.id,
      pattern: "预算",
    }));

    expect(payload.mode).toBe("literal");
    expect(payload.totalMatches).toBe(1);
    expect(payload.matches).toHaveLength(1);
    const match = payload.matches[0];
    expect(match.sourceId).toBe(imported.source.id);
    expect(match.parseArtifactId).toBe(artifact!.id);
    expect(typeof match.blockId).toBe("string");
    expect(match.headingPath).toEqual(["交付计划"]);
    expect(match.match).toBe("预算");
    expect(match.snippet).toContain("八百万元");
    expect(match.offset).toBeGreaterThanOrEqual(0);
    expect(match.lineNumber).toBeGreaterThanOrEqual(1);
    const blocks = manager.listArtifactBlocks({ studioId, parseArtifactId: artifact!.id });
    const block = blocks.find(item => item.id === match.blockId)!;
    expect(block.text.slice(match.offset, match.endOffset)).toBe(match.match);
    expect(payload.scannedChars).toBe(blocks.reduce((sum, item) => sum + item.text.length, 0));
    expect(payload.matchedSourceCount).toBe(1);
  });

  it("regexp 匹配与 headingFilter 前缀过滤", async () => {
    const { manager, studioId, notebook } = await setupMarkdownSource();
    const scope = createScope(manager, studioId, [notebook.id]);
    const tool = makeGrepTool(manager, studioId);

    const regexp = parseResult(await tool.execute("call-1", {
      scopeId: scope.id,
      pattern: "负责人是(王芳|李雷)",
      regexp: true,
    }));
    expect(regexp.mode).toBe("regexp");
    expect(regexp.totalMatches).toBe(2);
    expect(regexp.matches.map(match => match.match).sort()).toEqual(["负责人是李雷", "负责人是王芳"]);

    // headingFilter 前缀匹配：只扫「风险登记」节。
    const filtered = parseResult(await tool.execute("call-2", {
      scopeId: scope.id,
      pattern: "项目",
      headingFilter: "风险登记",
    }));
    expect(filtered.totalMatches).toBe(1);
    expect(filtered.matches[0].headingPath).toEqual(["风险登记"]);

    // 前缀不命中任何节 → 零命中（显式空结果）。
    const none = parseResult(await tool.execute("call-3", {
      scopeId: scope.id,
      pattern: "项目",
      headingFilter: "不存在的节",
    }));
    expect(none.totalMatches).toBe(0);
    expect(none.matches).toEqual([]);
  });

  it("maxResults 封顶 + 超出计数提示；非法参数显式报错", async () => {
    const { manager, studioId, notebook } = await setupMarkdownSource();
    const scope = createScope(manager, studioId, [notebook.id]);
    const tool = makeGrepTool(manager, studioId);

    const capped = parseResult(await tool.execute("call-1", {
      scopeId: scope.id,
      pattern: "项目",
      maxResults: 1,
    }));
    expect(capped.matches).toHaveLength(1);
    expect(capped.totalMatches).toBe(3);
    expect(capped.truncated).toBe(true);
    expect(capped.notice).toContain("maxResults=1");

    // 非法 regexp / maxResults / pattern 超长 / 空 pattern。
    const badRegex = await tool.execute("call-2", { scopeId: scope.id, pattern: "([unclosed", regexp: true });
    expect(badRegex.isError).toBe(true);
    expect(badRegex.content[0].text).toContain("KNOWLEDGE_INVALID_ARGUMENT");

    const badMax = await tool.execute("call-3", { scopeId: scope.id, pattern: "项目", maxResults: 0 });
    expect(badMax.isError).toBe(true);
    expect(badMax.content[0].text).toContain("KNOWLEDGE_INVALID_ARGUMENT");

    const tooLong = await tool.execute("call-4", { scopeId: scope.id, pattern: "a".repeat(600) });
    expect(tooLong.isError).toBe(true);
    expect(tooLong.content[0].text).toContain("KNOWLEDGE_INVALID_ARGUMENT");

    const empty = await tool.execute("call-5", { scopeId: scope.id, pattern: "" });
    expect(empty.isError).toBe(true);
    expect(empty.content[0].text).toContain("KNOWLEDGE_INVALID_ARGUMENT");
  });

  it("sourceIds 必须全在 scope 内（含 scope 外源整单拒绝）；缺失/伪造 scopeId 拒绝；跨 session 拒绝", async () => {
    const { manager, studioId, notebook, imported } = await setupMarkdownSource();
    const notebookC = manager.createNotebook({ studioId, name: "未选中" });
    const importedC = await manager.importPastedText({
      studioId,
      notebookId: notebookC.id,
      text: "C 的私有预算内容。",
      displayName: "C.txt",
    });
    const scope = createScope(manager, studioId, [notebook.id]);
    const tool = makeGrepTool(manager, studioId);

    // 含 scope 外源 → 整单 KNOWLEDGE_SCOPE_VIOLATION。
    expectScopeViolation(await tool.execute("call-1", {
      scopeId: scope.id,
      pattern: "预算",
      sourceIds: [imported.source.id, importedC.source.id],
    }));

    // 全在 scope 内 → 正常，且只扫选中源。
    const ok = parseResult(await tool.execute("call-2", {
      scopeId: scope.id,
      pattern: "预算",
      sourceIds: [imported.source.id],
    }));
    expect(ok.totalMatches).toBe(1);
    expect(ok.matches[0].sourceId).toBe(imported.source.id);

    expectScopeViolation(await tool.execute("call-3", { pattern: "预算" }));
    expectScopeViolation(await tool.execute("call-4", { scopeId: "kts_forged", pattern: "预算" }));

    const otherSession = makeGrepTool(manager, studioId, {
      sessionPath: "/tmp/lingxi-knowledge-agent-tools/other-session.jsonl",
      scopeOwnerSessionPath: "/tmp/lingxi-knowledge-agent-tools/other-session.jsonl",
    });
    expectScopeViolation(await otherSession.execute("call-5", { scopeId: scope.id, pattern: "预算" }));
  });

  it("未解析的源进 unavailableSources 显式单列", async () => {
    const { manager, studioId, notebook } = await setupMarkdownSource({ parse: false });
    const scope = createScope(manager, studioId, [notebook.id]);
    const payload = parseResult(await makeGrepTool(manager, studioId).execute("call-1", {
      scopeId: scope.id,
      pattern: "预算",
    }));
    expect(payload.totalMatches).toBe(0);
    expect(payload.unavailableSources).toHaveLength(1);
    expect(payload.unavailableSources[0].reason).toContain("KNOWLEDGE_PARSE_NOT_READY");
  });
});

it("grep 为宿主提供精确可回读位置；保留空白、长匹配截断如实标记", async () => {
  const { manager, studioId, notebook, imported, artifact } = await setupMarkdownSource();
  const scope = createScope(manager, studioId, [notebook.id]);
  const result = await makeGrepTool(manager, studioId).execute("receipt-hook", { scopeId: scope.id, pattern: "预算" });
  const readSpans = (result.details as any).readSpans;
  expect(readSpans).toHaveLength(1);
  const blocks = manager.listArtifactBlocks({ studioId, parseArtifactId: artifact!.id });
  for (const span of readSpans) {
    expect(span).toMatchObject({ sourceId: imported.source.id, contentSnapshotId: scope.sources[0].contentSnapshotId, parseArtifactId: artifact!.id });
    expect(blocks.find(block => block.id === span.blockId)!.text.slice(span.startOffset, span.endOffset)).toBe(span.canonicalText);
    expect(result.content[0].text).toContain(span.canonicalText);
  }
  vi.spyOn(manager, "listArtifactBlocks").mockReturnValue([{ ...blocks[0], text: "前缀  空白\n" + "x".repeat(300) }]);
  const long = parseResult(await makeGrepTool(manager, studioId).execute("long", { scopeId: scope.id, pattern: "x+", regexp: true }));
  expect(long.matches[0].endOffset - long.matches[0].offset).toBe(300);
  expect(long.matches[0].matchTruncated).toBe(true); expect(long.matches[0].match).toHaveLength(200);
  expect(long.matches[0].snippet).toContain("前缀  空白\n");
});

it("grep 扫描预算只计实际扫描的字符，保留中断来源已有结果及匹配来源数", async () => {
  const { manager, studioId, notebook, artifact } = await setupMarkdownSource();
  const scope = createScope(manager, studioId, [notebook.id]);
  const blocks = manager.listArtifactBlocks({ studioId, parseArtifactId: artifact!.id });
  vi.spyOn(manager, "listArtifactBlocks").mockReturnValue([
    { ...blocks[0], text: "预算" }, { ...blocks[1], text: "x".repeat(4_000_001) },
  ]);
  const payload = parseResult(await makeGrepTool(manager, studioId).execute("bounded", { scopeId: scope.id, pattern: "预算" }));
  expect(payload.scanTruncated).toBe(true); expect(payload.scannedChars).toBe(2);
  expect(payload.scannedSources).toHaveLength(1); expect(payload.scannedSources[0]).toMatchObject({ scannedChars: 2, matchCount: 1 });
  expect(payload.totalMatches).toBe(1); expect(payload.matchedSourceCount).toBe(1);
});

// ─────────────────────────── knowledge_manage ───────────────────────────

describe("knowledge_manage 工具（修改性操作，审批档 + 委托 KnowledgeManager）", () => {
  it("resolveInvocation 返回审批档：auto→review / ask→prompt / operate→allow / read_only→拒绝", () => {
    const manager = new KnowledgeManager({ lingxiHome: tempHome() });
    managers.push(manager);
    const tool = makeManageTool(manager, "studio-a");

    const invocation = tool.sessionPermission.resolveInvocation({ action: "add" });
    expect(invocation).toMatchObject({ action: "add", kind: "review", capability: "knowledge_manage.add" });
    // 未知 action 也有稳定 capability（审批面不因参数非法而失守）。
    expect(tool.sessionPermission.resolveInvocation({})).toMatchObject({
      action: "execute",
      kind: "review",
      capability: "knowledge_manage.execute",
    });

    const context = { toolInvocation: invocation };
    expect(classifySessionPermission({ mode: "auto", toolName: "knowledge_manage", context }))
      .toMatchObject({ action: "review" });
    expect(classifySessionPermission({ mode: "ask", toolName: "knowledge_manage", context }))
      .toMatchObject({ action: "prompt", kind: "tool_action_approval" });
    expect(classifySessionPermission({ mode: "operate", toolName: "knowledge_manage", context }))
      .toEqual({ action: "allow" });
    expect(classifySessionPermission({ mode: "read_only", toolName: "knowledge_manage", context }))
      .toMatchObject({ action: "deny", code: "ACTION_BLOCKED_BY_READ_ONLY" });
  });

  it("子 Agent 上下文固定拦截（SUBAGENT_BLOCKED_TOOLS，任何档位）", () => {
    const manager = new KnowledgeManager({ lingxiHome: tempHome() });
    managers.push(manager);
    const tool = makeManageTool(manager, "studio-a");
    const context = {
      isSubagent: true,
      toolInvocation: tool.sessionPermission.resolveInvocation({ action: "add" }),
    };
    for (const mode of ["auto", "ask", "operate", "read_only"]) {
      expect(classifySessionPermission({ mode, toolName: "knowledge_manage", context }))
        .toMatchObject({ action: "deny", code: "ACTION_BLOCKED_IN_SUBAGENT" });
    }
    // 读侧工具不受 subagent 拦截表影响（read kind 放行）。
    expect(classifySessionPermission({
      mode: "read_only",
      toolName: "knowledge_grep",
      context: { toolInvocation: { action: "read", kind: "read", capability: "knowledge_grep.read" } },
    })).toEqual({ action: "allow" });
  });

  it("add 委托 importPastedText 创建源与 membership；remove 委托 removeSourceFromNotebook", async () => {
    const studioId = "studio-a";
    const manager = new KnowledgeManager({ lingxiHome: tempHome() });
    managers.push(manager);
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    const tool = makeManageTool(manager, studioId);

    const added = parseResult(await tool.execute("call-1", {
      action: "add",
      notebookId: notebook.id,
      kind: "pasted_text",
      text: "新增的粘贴文本：交付日期九月十五日。",
      displayName: "粘贴.txt",
    }));
    expect(added.action).toBe("add");
    expect(added.notebookId).toBe(notebook.id);
    expect(added.sourceType).toBe("pasted_text");
    const sourceId = added.sourceId;
    expect(manager.getSource({ studioId, sourceId }).displayName).toBe("粘贴.txt");
    expect(
      manager.listNotebookSources({ studioId, notebookId: notebook.id }).map(entry => entry.source.id),
    ).toContain(sourceId);

    const removed = parseResult(await tool.execute("call-2", {
      action: "remove",
      notebookId: notebook.id,
      sourceId,
    }));
    expect(removed.sourceId).toBe(sourceId);
    expect(
      manager.listNotebookSources({ studioId, notebookId: notebook.id }).map(entry => entry.source.id),
    ).not.toContain(sourceId);
  });

  it("refresh 不带 notebookId 时解析 owning notebook 并委托 refreshFileSource（文件变化→changed）", async () => {
    const { manager, studioId, notebook, imported, artifact } = await setupMarkdownSource();
    const tool = makeManageTool(manager, studioId);
    const filePath = imported.source.originMetadata.originalPath as string;
    fs.writeFileSync(filePath, MARKDOWN_TEXT.replace("九月十五日", "十月一日"), "utf8");

    const refreshed = parseResult(await tool.execute("call-1", {
      action: "refresh",
      sourceId: imported.source.id,
    }));
    expect(refreshed.changed).toBe(true);
    expect(refreshed.notebookId).toBe(notebook.id);
    expect(refreshed.parseArtifactId).not.toBe(artifact!.id);
  });

  it("reindex：带 sourceId 走 reingest 重试路由，不带则整本重建入队", async () => {
    const { manager, studioId, notebook, imported } = await setupMarkdownSource();
    const tool = makeManageTool(manager, studioId);

    const reingest = parseResult(await tool.execute("call-1", {
      action: "reindex",
      notebookId: notebook.id,
      sourceId: imported.source.id,
    }));
    expect(reingest.retried).toBe(false);
    expect(typeof reingest.jobId).toBe("string");

    const rebuild = parseResult(await tool.execute("call-2", {
      action: "reindex",
      notebookId: notebook.id,
    }));
    expect(rebuild.enqueuedJobs).toBe(1);
  });

  it("非法参数显式报错：未知 action / 未知 kind / 字段冲突 / 缺参", async () => {
    const studioId = "studio-a";
    const manager = new KnowledgeManager({ lingxiHome: tempHome() });
    managers.push(manager);
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    const tool = makeManageTool(manager, studioId);

    const badAction = await tool.execute("call-1", { action: "explode" });
    expect(badAction.isError).toBe(true);
    expect(badAction.content[0].text).toContain("KNOWLEDGE_INVALID_ARGUMENT");

    const badKind = await tool.execute("call-2", { action: "add", notebookId: notebook.id, kind: "carrier_pigeon", text: "x" });
    expect(badKind.isError).toBe(true);
    expect(badKind.content[0].text).toContain("KNOWLEDGE_INVALID_ARGUMENT");

    const mixed = await tool.execute("call-3", {
      action: "add",
      notebookId: notebook.id,
      kind: "pasted_text",
      text: "x",
      url: "https://example.com",
    });
    expect(mixed.isError).toBe(true);
    expect(mixed.content[0].text).toContain("KNOWLEDGE_INVALID_ARGUMENT");

    const missing = await tool.execute("call-4", { action: "remove", notebookId: notebook.id });
    expect(missing.isError).toBe(true);
    expect(missing.content[0].text).toContain("KNOWLEDGE_INVALID_ARGUMENT");
  });

  it("Knowledge 不可用时显式报错", async () => {
    const tool = createKnowledgeManageTool({ getKnowledge: () => null, getStudioId: () => "studio-a" });
    const result = await tool.execute("call-1", { action: "add", notebookId: "nb_1", kind: "pasted_text", text: "x" });
    expect(result.isError).toBe(true);
    expect(result.content[0].text).toContain("unavailable");
  });
});

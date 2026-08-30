import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { createKnowledgeReadTool } from "../lib/tools/knowledge-read-tool.ts";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";

const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];

function tempHome() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-read-"));
  tempDirs.push(dir);
  return dir;
}

afterEach(() => {
  for (const manager of managers.splice(0)) manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

async function setupReadySource(options: {
  studioId?: string;
  text?: string;
  index?: boolean;
} = {}) {
  const studioId = options.studioId ?? "studio-a";
  const manager = new KnowledgeManager({ lingxiHome: tempHome() });
  managers.push(manager);
  const notebook = manager.createNotebook({ studioId, name: "资料" });
  const imported = await manager.importPastedText({
    studioId,
    notebookId: notebook.id,
    text: options.text ?? [
      "第一章 苹果项目的交付日期是九月十五日。",
      "火星项目的预算是八百万元，负责人是李雷。",
      "蓝山项目仍在风险评估阶段。",
    ].join("\n"),
    displayName: "项目.txt",
  });
  const artifact = options.index === false
    ? null
    : await manager.parseSource({ studioId, sourceId: imported.source.id });
  if (artifact) {
    manager.queryService.indexArtifactForIngestion(studioId, artifact.id);
  }
  return { manager, studioId, notebook, imported, artifact };
}

function makeTool(manager: KnowledgeManager, studioId: string) {
  return createKnowledgeReadTool({
    getKnowledge: () => manager,
    getStudioId: () => studioId,
  });
}

function parseResult(result: any) {
  expect(result?.isError).toBeFalsy();
  return JSON.parse(result.content[0].text);
}

describe("knowledge_read 工具", () => {
  it("按 ordinal 范围读片（1-based、双闭区间、附总 chunk 数）", async () => {
    const { manager, studioId, imported } = await setupReadySource();
    const tool = makeTool(manager, studioId);
    const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
    const total = manager.indexStore.listArtifactChunks(artifact.id).length;
    expect(total).toBeGreaterThan(0);

    const payload = parseResult(await tool.execute("call-1", {
      sourceId: imported.source.id,
      fromOrdinal: 1,
      toOrdinal: 1,
    }));
    expect(payload.mode).toBe("ordinal-range");
    expect(payload.totalChunks).toBe(total);
    expect(payload.chunks).toHaveLength(1);
    expect(payload.chunks[0].ordinal).toBe(1);
    expect(payload.chunks[0].text).toContain("苹果项目");

    const all = parseResult(await tool.execute("call-2", { sourceId: imported.source.id }));
    expect(all.chunks).toHaveLength(total);
    expect(all.chunks.map(chunk => chunk.ordinal)).toEqual(
      Array.from({ length: total }, (_, index) => index + 1),
    );
  });

  it("按 query 检索该源（返回匹配片与 retrievalMode）", async () => {
    const { manager, studioId, imported } = await setupReadySource();
    const tool = makeTool(manager, studioId);
    const payload = parseResult(await tool.execute("call-1", {
      sourceId: imported.source.id,
      query: "火星 预算",
    }));
    expect(payload.mode).toBe("search");
    expect(payload.retrievalMode).toBe("fts");
    expect(payload.matches.length).toBeGreaterThan(0);
    expect(payload.matches[0].text).toContain("火星");
  });

  it("越界与超额范围显式报错（带合法 ordinal 范围）", async () => {
    const { manager, studioId, imported } = await setupReadySource();
    const tool = makeTool(manager, studioId);
    const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
    const total = manager.indexStore.listArtifactChunks(artifact.id).length;

    const outOfBounds = await tool.execute("call-1", {
      sourceId: imported.source.id,
      fromOrdinal: total + 5,
      toOrdinal: total + 6,
    });
    expect(outOfBounds.isError).toBe(true);
    expect(outOfBounds.content[0].text).toContain(`ordinals 1-${total}`);

    const tooMany = await tool.execute("call-2", {
      sourceId: imported.source.id,
      fromOrdinal: 1,
      toOrdinal: 42,
    });
    expect(tooMany.isError).toBe(true);
    expect(tooMany.content[0].text).toContain("at most 40 chunks");

    const inverted = await tool.execute("call-3", {
      sourceId: imported.source.id,
      fromOrdinal: 3,
      toOrdinal: 1,
    });
    expect(inverted.isError).toBe(true);
    expect(inverted.content[0].text).toContain("toOrdinal must be >= fromOrdinal");
  });

  it("不存在的源与 notebook 不匹配显式报错", async () => {
    const { manager, studioId, imported, notebook } = await setupReadySource();
    const tool = makeTool(manager, studioId);

    const missing = await tool.execute("call-1", { sourceId: "src_does_not_exist" });
    expect(missing.isError).toBe(true);
    expect(missing.content[0].text).toContain("KNOWLEDGE_NOT_FOUND");

    const otherNotebook = manager.createNotebook({ studioId, name: "空笔记本" });
    const mismatch = await tool.execute("call-2", {
      sourceId: imported.source.id,
      notebookId: otherNotebook.id,
      fromOrdinal: 1,
    });
    expect(mismatch.isError).toBe(true);
    expect(mismatch.content[0].text).toContain("not in this Notebook");

    // notebookId 正确指向源所在笔记本时可读。
    const ok = await tool.execute("call-3", {
      sourceId: imported.source.id,
      notebookId: notebook.id,
      fromOrdinal: 1,
    });
    expect(ok.isError).toBeFalsy();
  });

  it("studio 隔离：其他 studio 的源不可见", async () => {
    const { manager, imported } = await setupReadySource({ studioId: "studio-b" });
    const tool = makeTool(manager, "studio-a");
    const denied = await tool.execute("call-1", {
      sourceId: imported.source.id,
      fromOrdinal: 1,
    });
    expect(denied.isError).toBe(true);
    expect(denied.content[0].text).toContain("KNOWLEDGE_NOT_FOUND");
  });

  it("未解析/未就绪的源显式报错而不是返回空", async () => {
    const { manager, studioId, imported } = await setupReadySource({ index: false });
    const tool = makeTool(manager, studioId);
    const notReady = await tool.execute("call-1", {
      sourceId: imported.source.id,
      fromOrdinal: 1,
    });
    expect(notReady.isError).toBe(true);
    expect(notReady.content[0].text).toContain("KNOWLEDGE_PARSE_NOT_READY");
  });

  it("Knowledge 不可用时显式报错", async () => {
    const tool = createKnowledgeReadTool({
      getKnowledge: () => null,
      getStudioId: () => "studio-a",
    });
    const result = await tool.execute("call-1", { sourceId: "src_1", fromOrdinal: 1 });
    expect(result.isError).toBe(true);
    expect(result.content[0].text).toContain("unavailable");
  });
});

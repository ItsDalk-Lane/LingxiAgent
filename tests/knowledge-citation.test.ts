import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";

import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const tempDirs: string[] = [];

function harness() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-citation-"));
  tempDirs.push(root);
  const lingxiHome = path.join(root, "home");
  const importsDir = path.join(root, "imports");
  fs.mkdirSync(lingxiHome);
  fs.mkdirSync(importsDir);
  const manager = new KnowledgeManager({ lingxiHome });
  const notebook = manager.createNotebook({ studioId: "studio-a", name: "引用" });
  return { root, lingxiHome, importsDir, manager, notebook };
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

describe("Knowledge citation-grade parse", () => {
  it("TXT 引用在外部文件删除和进程重启后仍能返回同一行", async () => {
    const { lingxiHome, importsDir, manager, notebook } = harness();
    const inputPath = path.join(importsDir, "facts.txt");
    fs.writeFileSync(inputPath, "第一行\n第二行关键事实\n第三行\n", "utf-8");
    const imported = await manager.importFile({
      studioId: "studio-a",
      notebookId: notebook.id,
      filePath: inputPath,
    });
    const artifact = await manager.parseSource({
      studioId: "studio-a",
      sourceId: imported.source.id,
    });
    expect(artifact.status).toBe("ready");
    const blocks = manager.listArtifactBlocks({
      studioId: "studio-a",
      parseArtifactId: artifact.id,
    });
    expect(blocks.map((block) => block.text)).toEqual(["第一行", "第二行关键事实", "第三行"]);
    expect(blocks[1].locator).toMatchObject({
      lineStart: 2,
      lineEnd: 2,
    });
    const citation = manager.createCitation({
      studioId: "studio-a",
      parseArtifactId: artifact.id,
      blockId: blocks[1].id,
      startOffset: 3,
      endOffset: 7,
    });
    expect(citation.canonicalText).toBe("关键事实");
    await manager.close();
    fs.unlinkSync(inputPath);

    const restarted = new KnowledgeManager({ lingxiHome });
    const resolved = restarted.resolveCitation({
      studioId: "studio-a",
      citationId: citation.id,
    });
    expect(resolved.citation.canonicalText).toBe("关键事实");
    expect(resolved.block.text).toBe("第二行关键事实");
    expect(resolved.snapshot.id).toBe(imported.snapshot.id);
    expect(restarted.readContentSnapshot({
      studioId: "studio-a",
      snapshotId: resolved.snapshot.id,
    }).toString("utf-8")).toContain("第二行关键事实");
    await restarted.close();
  });

  it("Markdown 保留标题路径，HTML 保留结构路径", async () => {
    const { importsDir, manager, notebook } = harness();
    const markdownPath = path.join(importsDir, "guide.md");
    fs.writeFileSync(markdownPath, "# 总览\n\n## 安全\n必须冻结范围。\n", "utf-8");
    const markdown = await manager.importFile({
      studioId: "studio-a",
      notebookId: notebook.id,
      filePath: markdownPath,
    });
    const markdownArtifact = await manager.parseSource({
      studioId: "studio-a",
      sourceId: markdown.source.id,
    });
    const markdownBlocks = manager.listArtifactBlocks({
      studioId: "studio-a",
      parseArtifactId: markdownArtifact.id,
    });
    expect(markdownBlocks.find((block) => block.text === "必须冻结范围。")?.locator)
      .toMatchObject({ headingPath: ["总览", "安全"], lineStart: 4 });

    const htmlPath = path.join(importsDir, "page.html");
    fs.writeFileSync(htmlPath, `<!doctype html><html><body><main><h1>说明</h1><p>保存快照。</p><script>bad()</script></main></body></html>`);
    const html = await manager.importFile({
      studioId: "studio-a",
      notebookId: notebook.id,
      filePath: htmlPath,
    });
    const htmlArtifact = await manager.parseSource({
      studioId: "studio-a",
      sourceId: html.source.id,
    });
    const htmlBlocks = manager.listArtifactBlocks({
      studioId: "studio-a",
      parseArtifactId: htmlArtifact.id,
    });
    expect(htmlBlocks.map((block) => block.text)).toEqual(["说明", "保存快照\u3002"]);
    expect(htmlBlocks[1].locator).toMatchObject({ structuralPath: "html:nth-of-type(1) > body:nth-of-type(1) > main:nth-of-type(1) > p:nth-of-type(1)" });
    expect(htmlBlocks.some((block) => block.text.includes("bad"))).toBe(false);
    await manager.close();
  });

  it("文本层 PDF 产生页码和坐标，扫描 PDF 明确标记需要 OCR", async () => {
    const { manager, notebook } = harness();
    const textPdf = await manager.importFile({
      studioId: "studio-a",
      notebookId: notebook.id,
      filePath: path.join(ROOT, "tests", "fixtures", "document-extract", "sample-text.pdf"),
    });
    const textArtifact = await manager.parseSource({
      studioId: "studio-a",
      sourceId: textPdf.source.id,
    });
    expect(textArtifact.status).toBe("ready");
    const pdfBlocks = manager.listArtifactBlocks({
      studioId: "studio-a",
      parseArtifactId: textArtifact.id,
    });
    expect(pdfBlocks.map((block) => block.text)).toEqual(["Hello from PDF", "Second line of text"]);
    expect(pdfBlocks[0].locator).toMatchObject({
      page: 1,
      itemStart: 0,
      itemEnd: 1,
      boxes: [expect.objectContaining({ x: 72, y: 700 })],
    });

    const scannedPdf = await manager.importFile({
      studioId: "studio-a",
      notebookId: notebook.id,
      filePath: path.join(ROOT, "tests", "fixtures", "document-extract", "sample-scanned.pdf"),
    });
    const scannedArtifact = await manager.parseSource({
      studioId: "studio-a",
      sourceId: scannedPdf.source.id,
    });
    expect(scannedArtifact).toMatchObject({ status: "needs_ocr", warnings: ["needs_ocr"] });
    expect(manager.listArtifactBlocks({
      studioId: "studio-a",
      parseArtifactId: scannedArtifact.id,
    })).toEqual([]);
    expect(() => manager.createCitation({
      studioId: "studio-a",
      parseArtifactId: scannedArtifact.id,
      blockId: "missing",
      startOffset: 0,
      endOffset: 1,
    })).toThrow(/not ready/i);
    await manager.close();
  });

  it("同一内容快照与解析产物身份分离，重复解析不制造内容版本", async () => {
    const { importsDir, manager, notebook } = harness();
    const inputPath = path.join(importsDir, "stable.txt");
    fs.writeFileSync(inputPath, "稳定内容\n", "utf-8");
    const imported = await manager.importFile({
      studioId: "studio-a",
      notebookId: notebook.id,
      filePath: inputPath,
    });
    const first = await manager.parseSource({ studioId: "studio-a", sourceId: imported.source.id });
    const second = await manager.parseSource({ studioId: "studio-a", sourceId: imported.source.id });

    expect(second.id).toBe(first.id);
    expect(second.contentSnapshotId).toBe(imported.snapshot.id);
    expect(manager.store.countContentSnapshots({ studioId: "studio-a", sourceId: imported.source.id }))
      .toBe(1);
    expect(manager.store.countParseArtifacts({ studioId: "studio-a", sourceId: imported.source.id }))
      .toBe(1);
    await manager.close();
  });
});

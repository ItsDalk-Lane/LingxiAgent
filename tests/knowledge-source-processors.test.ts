import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";

import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import {
  KNOWLEDGE_PROCESSOR_VERSION,
  processKnowledgeSnapshot,
  rebuildBlocksFromProcessorOutput,
  resolveKnowledgeProcessor,
} from "../lib/knowledge/source-processors.ts";

const FIXTURES = path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "document-extract");

const tempDirs: string[] = [];

function tempRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-proc-"));
  tempDirs.push(root);
  fs.mkdirSync(path.join(root, "home"));
  fs.mkdirSync(path.join(root, "outside"));
  return root;
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

describe("source-processors（§五十八/§五十九 ProcessingArtifact 管线）", () => {
  it("DOCX：块级抽取 + 反向定位（段落序号），fidelity=structural", async () => {
    const bytes = fs.readFileSync(path.join(FIXTURES, "sample.docx"));
    const processed = await processKnowledgeSnapshot({
      mimeType: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
      bytes,
    });
    expect(processed.plan.processorId).toBe("lingxi-office-docx");
    expect(processed.plan.processorVersion).toBe(KNOWLEDGE_PROCESSOR_VERSION);
    expect(processed.fidelity).toBe("structural");
    expect(processed.outputMime).toBe("text/plain");
    expect(processed.blocks.length).toBeGreaterThan(0);

    // 不变量：输出第 N 行 = locatorMap 第 N 项 = blocks 第 N 块。
    const lines = processed.output.toString("utf8").split("\n");
    expect(lines.length).toBe(processed.blocks.length);
    processed.blocks.forEach((block, index) => {
      expect(block.ordinal).toBe(index);
      expect(block.text).toBe(lines[index]);
      expect(block.text.length).toBeGreaterThan(0);
      expect(block.locatorType).toBe("text");
      const source = block.locator.source as Record<string, unknown>;
      expect(source.kind).toBe("docx_paragraph");
      expect(typeof source.paragraphIndex).toBe("number");
      expect(processed.locatorMap[index]).toEqual(source);
    });
    // 样本含 heading「Quarterly Notes」，其 headingPath 应挂到后续段落。
    const heading = processed.blocks.find(block => block.text === "Quarterly Notes");
    expect(heading).toBeTruthy();
    const body = processed.blocks.find(block => block.text.includes("bold text"));
    expect((body?.locator.source as any)?.headingPath).toEqual(["Quarterly Notes"]);
  });

  it("XLSX：行级抽取 + 单元格坐标反向定位，rebuild round-trip 等价", async () => {
    const bytes = fs.readFileSync(path.join(FIXTURES, "sample.xlsx"));
    const processed = await processKnowledgeSnapshot({
      mimeType: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
      bytes,
    });
    expect(processed.plan.processorId).toBe("lingxi-office-xlsx");
    expect(processed.fidelity).toBe("structural");
    expect(processed.blocks.length).toBe(2);
    expect(processed.blocks[0]?.text).toBe("Region | Total");
    expect(processed.blocks[0]?.locator.source).toMatchObject({
      kind: "sheet_row",
      sheet: "Sales",
      row: 1,
      cells: ["A1", "B1"],
    });
    expect(processed.blocks[1]?.locator.source).toMatchObject({ row: 2, cells: ["A2", "B2"] });

    // 复用路径：从持久化输出 + locatorMap 重建的 blocks 与新鲜结果完全一致。
    const rebuilt = rebuildBlocksFromProcessorOutput({
      output: processed.output,
      locatorMap: processed.locatorMap,
    });
    expect(rebuilt).toEqual(processed.blocks);
  });

  it("CSV：行级抽取 + R<n>C<m> 坐标", async () => {
    const bytes = fs.readFileSync(path.join(FIXTURES, "sample.csv"));
    const processed = await processKnowledgeSnapshot({ mimeType: "text/csv", bytes });
    expect(processed.plan.processorId).toBe("lingxi-office-csv");
    expect(processed.fidelity).toBe("structural");
    expect(processed.blocks.length).toBe(3);
    expect(processed.blocks[0]?.text).toBe("Region | Total");
    expect(processed.blocks[2]?.locator.source).toMatchObject({
      kind: "csv_row",
      row: 3,
      cells: ["R3C1", "R3C2"],
    });
  });

  it("未注册 processor 的 MIME 显式拒绝；rebuild 对错位输出显式失败", async () => {
    await expect(processKnowledgeSnapshot({
      mimeType: "application/vnd.ms-powerpoint",
      bytes: Buffer.from("x"),
    })).rejects.toMatchObject({ code: "KNOWLEDGE_IMPORT_PROCESSOR_UNAVAILABLE" });
    expect(resolveKnowledgeProcessor("application/pdf")).toBeNull();

    expect(() => rebuildBlocksFromProcessorOutput({
      output: Buffer.from("a\n\nb", "utf8"),
      locatorMap: { 0: { kind: "csv_row" }, 1: { kind: "csv_row" }, 2: { kind: "csv_row" } },
    })).toThrow(expect.objectContaining({ code: "KNOWLEDGE_STORAGE_INVALID" }));
    expect(() => rebuildBlocksFromProcessorOutput({
      output: Buffer.from("a\nb", "utf8"),
      locatorMap: { 0: { kind: "csv_row" } },
    })).toThrow(expect.objectContaining({ code: "KNOWLEDGE_STORAGE_INVALID" }));
  });

  it("manager 端到端：docx 导入 → parse 落 structural fidelity + processingArtifactId，复用不重建，purge 级联清理", async () => {
    const root = tempRoot();
    const lingxiHome = path.join(root, "home");
    const externalPath = path.join(root, "outside", "notes.docx");
    fs.copyFileSync(path.join(FIXTURES, "sample.docx"), externalPath);

    const manager = new KnowledgeManager({ lingxiHome });
    const notebook = manager.createNotebook({ studioId: "studio-a", name: "资料" });
    const imported = await manager.importFile({
      studioId: "studio-a",
      notebookId: notebook.id,
      filePath: externalPath,
    });
    expect(imported.snapshot.mimeType)
      .toBe("application/vnd.openxmlformats-officedocument.wordprocessingml.document");

    const artifact = await manager.parseSource({ studioId: "studio-a", sourceId: imported.source.id });
    expect(artifact.status).toBe("ready");
    expect(artifact.fidelity).toBe("structural");
    expect(artifact.processingArtifactId).toBeTruthy();

    const blocks = manager.listArtifactBlocks({
      studioId: "studio-a",
      parseArtifactId: artifact.id,
    });
    expect(blocks.length).toBeGreaterThan(0);
    expect((blocks[0]?.locator.source as any)?.kind).toBe("docx_paragraph");

    // processing artifact 落库 + 输出文件原子落盘。
    const processing = manager.store.getProcessingArtifact({
      studioId: "studio-a",
      processingArtifactId: artifact.processingArtifactId,
    });
    expect(processing.status).toBe("ready");
    expect(processing.fidelity).toBe("structural");
    expect(processing.outputPath).toMatch(/^processed\//u);
    expect(fs.existsSync(path.join(lingxiHome, "knowledge", processing.outputPath!))).toBe(true);

    // 幂等：再次 parseSource 复用同一 parse artifact，且 processing_artifacts 仍只有一行。
    const again = await manager.parseSource({ studioId: "studio-a", sourceId: imported.source.id });
    expect(again.id).toBe(artifact.id);
    expect(again.processingArtifactId).toBe(artifact.processingArtifactId);
    const rows = manager.store.db.prepare(
      "SELECT COUNT(*) AS n FROM processing_artifacts WHERE content_snapshot_id = ?",
    ).get(imported.snapshot.id) as { n: number };
    expect(rows.n).toBe(1);

    // 模拟 parse artifact 丢失后重解析：复用已 ready 的 processing artifact（不重新跑 processor，
    // 输出文件 mtime 不变作为旁证）。
    const outputFile = path.join(lingxiHome, "knowledge", processing.outputPath!);
    const mtimeBefore = fs.statSync(outputFile).mtimeMs;
    await new Promise(resolve => setTimeout(resolve, 20));
    manager.store.db.prepare("DELETE FROM knowledge_blocks WHERE parse_artifact_id = ?").run(artifact.id);
    manager.store.db.prepare("DELETE FROM parse_artifacts WHERE id = ?").run(artifact.id);
    const reparsed = await manager.parseSource({ studioId: "studio-a", sourceId: imported.source.id });
    expect(reparsed.status).toBe("ready");
    expect(reparsed.fidelity).toBe("structural");
    expect(reparsed.processingArtifactId).toBe(processing.id);
    expect(fs.statSync(outputFile).mtimeMs).toBe(mtimeBefore);

    // purge 级联：删源后 processing_artifacts 一行不剩。
    await manager.deleteSource({ studioId: "studio-a", sourceId: imported.source.id });
    const left = manager.store.db.prepare("SELECT COUNT(*) AS n FROM processing_artifacts").get() as { n: number };
    expect(left.n).toBe(0);
    manager.close();
  });

  it("显式不支持的格式（.pptx/.doc/.xls/.epub）导入即拒绝，消息不泄露路径", async () => {
    const root = tempRoot();
    const lingxiHome = path.join(root, "home");
    const outside = path.join(root, "outside");
    const manager = new KnowledgeManager({ lingxiHome });
    const notebook = manager.createNotebook({ studioId: "studio-a", name: "资料" });
    for (const name of ["deck.pptx", "legacy.doc", "legacy.xls", "book.epub"]) {
      const filePath = path.join(outside, name);
      fs.writeFileSync(filePath, Buffer.from("stub"));
      try {
        await manager.importFile({
          studioId: "studio-a",
          notebookId: notebook.id,
          filePath,
        });
        expect.unreachable("unsupported formats must be rejected");
      } catch (error) {
        expect(error).toMatchObject({
          code: "KNOWLEDGE_IMPORT_PROCESSOR_UNAVAILABLE",
          details: { extension: path.extname(name) },
        });
        expect(String((error as Error).message)).not.toContain(outside);
      }
    }
    manager.close();
  });
});

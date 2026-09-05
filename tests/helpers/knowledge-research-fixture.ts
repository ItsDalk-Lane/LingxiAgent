import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { KnowledgeStore } from "../../lib/knowledge/knowledge-store.ts";
import { EvidenceReceiptService } from "../../lib/knowledge/evidence-receipt-service.ts";
import { ResearchStore } from "../../lib/knowledge/research/research-store.ts";

/** 研究台账测试使用真实冻结资料，不通过模拟凭据或修改数据库版本绕过来源核验。 */
export function createKnowledgeResearchFixture(texts = ["项目交付日期是九月十五日。", "项目预算是三十二万元。"]) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-research-"));
  const store = new KnowledgeStore({ dbPath: path.join(root, "knowledge.db") });
  try {
    const studioId = "research-fixture";
    const notebook = store.createNotebook({ studioId, name: "研究资料" });
    const sources = texts.map((text, index) => {
      const imported = store.createSourceWithSnapshot({
        studioId, notebookId: notebook.id, sourceType: "pasted_text", displayName: `资料${index + 1}`,
        originMetadata: {}, snapshot: {
          sha256: crypto.createHash("sha256").update(text).digest("hex"), mimeType: "text/plain",
          byteSize: Buffer.byteLength(text), storagePath: `sources/source-${index}/snapshot.txt`,
        },
      });
      const artifact = store.beginParseArtifact({
        studioId, contentSnapshotId: imported.snapshot.id, parserId: "text", parserVersion: "1", parserConfigHash: "a".repeat(64),
      });
      store.completeParseArtifact({
        studioId, parseArtifactId: artifact.id, status: "ready", warnings: [], semanticArtifactPath: `parsed/${index}.txt`,
        blocks: [{ ordinal: 0, text, locatorType: "text", locator: { lineStart: 1, headingPath: ["项目"] } }],
      });
      const block = store.listArtifactBlocks({ studioId, parseArtifactId: artifact.id })[0];
      return { sourceId: imported.source.id, contentSnapshotId: imported.snapshot.id,
        parseArtifactId: artifact.id, blockId: block.id, text: block.text };
    });
    const scope = store.createTurnScope({ studioId, notebookIds: [notebook.id], sessionPath: path.join(root, "parent.jsonl") });
    const research = new ResearchStore(store);
    const receipts = new EvidenceReceiptService(research);
    const run = research.createRun({
      turnScopeId: scope.id, turnId: scope.turnId, parentSessionPath: scope.sessionPath, question: "项目事实是什么？",
    });
    return { store, research, receipts, run, scope, studioId, sources,
      close() {
        store.close();
        fs.rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
      },
    };
  } catch (error) {
    store.close();
    fs.rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    throw error;
  }
}

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { setImmediate as nextTurn } from "node:timers/promises";
import { afterEach, describe, expect, it, vi } from "vitest";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { KnowledgeQueryService } from "../lib/knowledge/knowledge-query-service.ts";
import { KnowledgeIndexStore } from "../lib/knowledge/knowledge-index-store.ts";
import { createMetadataFixture, metadataStudio } from "./helpers/knowledge-metadata-fixture.ts";

const homes: string[] = [];
const managers: KnowledgeManager[] = [];
afterEach(() => {
  vi.restoreAllMocks();
  for (const manager of managers.splice(0)) manager.close();
  for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true });
});
async function fixture() {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-metadata-backfill-"));
  homes.push(home);
  const data = await createMetadataFixture(home); managers.push(data.manager);
  return { ...data, home };
}
function close(manager: KnowledgeManager) { manager.close(); managers.splice(managers.indexOf(manager), 1); }

describe("查询目录摄入与后台补齐", () => {
  it("真实摄入写入块数、首标题及去重章节；重复摄入不重写目录", async () => {
    const { manager, artifact, variant } = await fixture();
    const row = manager.indexStore.getReadyVariantMetadata({ parseArtifactId: artifact.id, chunkProfileHash: variant.chunkProfileHash });
    expect(row).toMatchObject({ metadataMissing: false, firstHeadingPath: ["第一章"], sectionKeys: ["第一章", "第二章"] });
    expect(row!.chunkCount).toBe(manager.indexStore.listVariantChunks(variant.id).length);
    const before = manager.indexStore.db.prepare("SELECT * FROM chunk_index_variant_metadata").all();
    expect(manager.queryService.indexArtifactForIngestion(metadataStudio, artifact.id, { targetChars: 200 }).rebuilt).toBe(false);
    expect(manager.indexStore.db.prepare("SELECT * FROM chunk_index_variant_metadata").all()).toEqual(before);
  });

  it("启动不阻塞，每批最多 20 个，就绪旧变体补齐后重复启动不再写入", async () => {
    const { manager, home, artifact, variant } = await fixture();
    const chunks = manager.indexStore.listVariantChunks(variant.id);
    for (let index = 1; index <= 24; index += 1) {
      manager.indexStore.replaceArtifactChunks({ parseArtifactId: artifact.id,
        chunkProfileHash: index.toString(16).padStart(16, "0"), blockFingerprint: "fixture",
        chunks: chunks.map(chunk => ({ ...chunk, id: `${chunk.id}_${index}` })),
      });
    }
    manager.indexStore.db.prepare("DELETE FROM chunk_index_variant_metadata").run();
    close(manager);
    const backfill = vi.spyOn(KnowledgeQueryService.prototype, "backfillVariantMetadata");
    const batches = vi.spyOn(KnowledgeIndexStore.prototype, "listReadyVariantsMissingMetadata");
    const reopened = new KnowledgeManager({ lingxiHome: home }); managers.push(reopened);
    expect(backfill).not.toHaveBeenCalled();
    await nextTurn();
    expect(backfill).toHaveBeenCalledTimes(20);
    await nextTurn();
    expect(backfill).toHaveBeenCalledTimes(25);
    expect(batches.mock.results.map(result => result.value.length)).toEqual([20, 5]);
    const before = reopened.indexStore.db.prepare("SELECT * FROM chunk_index_variant_metadata ORDER BY chunk_index_variant_id").all();
    expect(before).toHaveLength(25);
    close(reopened);
    const again = new KnowledgeManager({ lingxiHome: home }); managers.push(again);
    await nextTurn();
    expect(backfill).toHaveBeenCalledTimes(25);
    expect(again.indexStore.db.prepare("SELECT * FROM chunk_index_variant_metadata ORDER BY chunk_index_variant_id").all()).toEqual(before);
  });

  it("后台失败明确留痕且继续其他变体；关闭取消未执行的批次", async () => {
    const { manager, home, artifact, variant } = await fixture();
    manager.indexStore.db.prepare("DELETE FROM chunk_index_variant_metadata").run();
    manager.indexStore.replaceArtifactChunks({ parseArtifactId: "missing-artifact",
      chunkProfileHash: "0".repeat(16), blockFingerprint: "missing",
      chunks: manager.indexStore.listVariantChunks(variant.id).map((chunk, ordinal) => ({ ...chunk,
        parseArtifactId: "missing-artifact", id: `missing-${ordinal}` })),
    });
    close(manager);
    const log = vi.fn();
    const reopened = new KnowledgeManager({ lingxiHome: home, ingestionLog: log }); managers.push(reopened);
    await nextTurn();
    expect(log).toHaveBeenCalledWith(expect.stringMatching(/background backfill failed.*Metadata source artifact is missing/));
    expect(reopened.indexStore.getReadyVariantMetadata({ parseArtifactId: artifact.id, chunkProfileHash: variant.chunkProfileHash })!.metadataMissing).toBe(false);
    close(reopened);
    const backfill = vi.spyOn(KnowledgeQueryService.prototype, "backfillVariantMetadata");
    const cancelled = new KnowledgeManager({ lingxiHome: home });
    cancelled.close();
    await nextTurn();
    expect(backfill).not.toHaveBeenCalled();
  });
});

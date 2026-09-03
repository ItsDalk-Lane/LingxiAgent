import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { createMetadataFixture } from "./helpers/knowledge-metadata-fixture.ts";

const homes: string[] = [];
const managers: KnowledgeManager[] = [];
afterEach(() => {
  vi.restoreAllMocks();
  for (const manager of managers.splice(0)) manager.close();
  for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true });
});
async function fixture() {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-metadata-query-"));
  homes.push(home);
  const data = await createMetadataFixture(home); managers.push(data.manager);
  return data;
}

describe("冻结范围只读目录", () => {
  it.each([false, true])("目录缺失=%s 时查询均不回填、不扫描全部块", async missing => {
    const { manager, scope, variant } = await fixture();
    if (missing) manager.indexStore.db.prepare("DELETE FROM chunk_index_variant_metadata").run();
    const blocks = vi.spyOn(manager.store, "listArtifactBlocks").mockImplementation(() => { throw new Error("禁止全量回读"); });
    const chunks = vi.spyOn(manager.indexStore, "listVariantChunks").mockImplementation(() => { throw new Error("禁止全量读取"); });
    const write = vi.spyOn(manager.indexStore, "writeVariantMetadata").mockImplementation(() => { throw new Error("禁止查询回填"); });
    const backfill = vi.spyOn(manager.queryService, "backfillVariantMetadata");
    const compiled = await manager.compileTurnScope(scope);
    expect(compiled.readyChunkVariantIds).toEqual([variant.id]);
    expect(compiled.sources[0].chunkCount).toBeGreaterThan(0);
    expect(compiled.sources[0].firstHeadingPath).toEqual(missing ? null : ["第一章"]);
    expect(compiled.sources[0].sectionKeys).toEqual(missing ? [] : ["第一章", "第二章"]);
    expect(compiled.warnings.some(item => item.endsWith("section_metadata_missing"))).toBe(missing);
    expect(manager.queryService.searchCompiledScopeFts({ compiledScope: compiled, query: "后台", limit: 8 }).length).toBeGreaterThan(0);
    for (const spy of [blocks, chunks, write, backfill]) expect(spy).not.toHaveBeenCalled();
  });

  it.each([
    ["section_keys_json", "{broken"], ["section_keys_json", '["重复","重复"]'],
    ["first_heading_path_json", '[null]'], ["chunk_count", -1], ["parse_artifact_id", "wrong-artifact"],
  ])("目录损坏 %s 显式失败，不伪装为空目录", async (column, value) => {
    const { manager, scope, variant } = await fixture();
    manager.indexStore.db.prepare(`UPDATE chunk_index_variant_metadata SET ${column} = ? WHERE chunk_index_variant_id = ?`).run(value, variant.id);
    await expect(manager.compileTurnScope(scope)).rejects.toMatchObject({ code: "KNOWLEDGE_INDEX_INVALID" });
  });
});

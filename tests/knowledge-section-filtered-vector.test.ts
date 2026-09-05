import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PortableVectorIndexAdapter } from "../lib/knowledge/vector-index-adapter.ts";
import { createHierarchicalFixture } from "./helpers/knowledge-hierarchical-fixture.ts";
const cleanups: Array<() => void | Promise<void>> = [];
afterEach(async () => { vi.restoreAllMocks(); for (const cleanup of cleanups.splice(0)) await cleanup(); });

describe("选中章节的小集合向量补查", () => {
  it("portable只解码指定片段且同时受模型和产物限制；其他片段损坏不能污染补查", () => {
    const home = fs.mkdtempSync(path.join(os.tmpdir(), "section-vector-"));
    const adapter = new PortableVectorIndexAdapter({ dbPath: path.join(home, "vectors.db") });
    cleanups.push(() => { adapter.close(); fs.rmSync(home, { recursive: true, force: true }); });
    const model = { key: "fixture/embed/openai/2", provider: "fixture", modelId: "embed", protocol: "openai", dimensions: 2 };
    for (const artifact of ["selected", "outside"]) adapter.buildOrReplaceArtifact({ parseArtifactId: artifact,
      chunkFingerprint: "fingerprint", model, entries: [0, 1, 2].map(ordinal => ({ parseArtifactId: artifact,
        chunkId: `${artifact}-${ordinal}`, ordinal, vector: ordinal === 0 ? [1, 0] : [0, 1] })) });
    adapter.db.prepare("UPDATE chunk_vectors SET vector=? WHERE chunk_id=?").run(Buffer.alloc(1), "selected-2");
    const prepare = vi.spyOn(adapter.db, "prepare");
    const request = { parseArtifactIds: ["selected"], model, queryVector: [1, 0], limit: 10 };
    const result = adapter.search({ ...request, chunkIds: ["selected-0", "outside-0", "selected-0"] });
    expect(result.map(row => row.chunkId)).toEqual(["selected-0"]); expect(result[0].score).toBe(1);
    expect(prepare.mock.calls.some(([sql]) => typeof sql === "string" && /chunk_id IN \(SELECT value FROM json_each/u.test(sql))).toBe(true);
    prepare.mockClear(); expect(adapter.search({ ...request, chunkIds: [] })).toEqual([]); expect(prepare).not.toHaveBeenCalled();
    expect(() => adapter.search(request)).toThrow(/corrupt/u);
  });

  it("一次查询嵌入同时驱动来源召回和章节精确补查，补查只接收所选章节IDs", async () => {
    const f = await createHierarchicalFixture([
      { name: "match.txt", sections: [{ heading: "选中", text: "needle 正面条款" }, { heading: "排除", text: "另章语义相近但不在显式范围" }] },
      { name: "other.txt", sections: [{ heading: "无关", text: "unrelated" }] },
    ], true); cleanups.push(() => f.close());
    const vector = vi.spyOn(f.manager.vectorIndex, "search");
    const ids = vi.spyOn(f.manager.indexStore, "listSectionChunkIds");
    const result = await f.manager.searchService.search({ ...f.request, sectionKeys: ["选中"] });
    const selectedIds = f.sources[0].chunks.filter(chunk => chunk.sectionId === f.sources[0].sections[0].id).map(chunk => chunk.id);
    expect(f.embed).toHaveBeenCalledTimes(1); expect(result.remoteModelCalls).toBe(1);
    expect(ids).toHaveBeenCalledWith({ chunkIndexVariantId: f.sources[0].variantId, sectionIds: [f.sources[0].sections[0].id] });
    expect(vector.mock.calls.some(([input]) => input.chunkIds?.length === selectedIds.length && input.chunkIds.every(id => selectedIds.includes(id)))).toBe(true);
    expect(result.hits.length).toBeGreaterThan(0);
    expect(result.hits.every(hit => hit.sectionId === f.sources[0].sections[0].id)).toBe(true);
    expect(result.hits.some(hit => hit.channels.includes("vector"))).toBe(true);
  });
});

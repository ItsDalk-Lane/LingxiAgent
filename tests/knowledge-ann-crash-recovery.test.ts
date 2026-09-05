import fs from "node:fs";
import path from "node:path";
import { expect, it } from "vitest";
import { annFixture } from "./helpers/knowledge-ann-fixture.ts";

it("重启清理中断临时文件，把 building 重建为 ready，原始向量逐字节保留", async () => {
  const f = annFixture();
  try {
    const id = f.add(), before = f.blobs();
    const row = f.store.begin({ vectorIndexVariantId: id, modelKey: f.model.key, dimensions: 3, chunkFingerprint: "fingerprint-a", vectorCount: 3 });
    const file = path.join(f.root, row.fileName); fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(`${file}.tmp`, "interrupted partial native file");
    const other = path.join(path.dirname(file), "user-note.tmp"); fs.writeFileSync(other, "preserve");
    const backend = f.start(); await backend.whenIdle();
    expect(fs.existsSync(`${file}.tmp`)).toBe(false); expect(fs.readFileSync(other, "utf8")).toBe("preserve");
    expect(f.store.get(id)?.status).toBe("ready"); expect(f.blobs()).toEqual(before);
    expect((await backend.search({ vectorIndexVariantIds: [id], model: f.model, queryVector: [0, 1, 0], limit: 1 }))[0].chunkId).toBe("a-1");
  } finally { await f.close(); }
});

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { LingxiEngine } from "../core/engine.ts";
import { PERSISTENT_STORES } from "../shared/persistence/store-registry.ts";

const tempDirs: string[] = [];

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

describe("Knowledge 引擎与持久化治理接入", () => {
  it("由 Engine 持有，并在 Engine 重启后恢复 Notebook", async () => {
    const lingxiHome = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-engine-"));
    tempDirs.push(lingxiHome);
    const first = new LingxiEngine({ lingxiHome, productDir: lingxiHome, agentId: "hana" } as any);
    const notebook = first.knowledge.createNotebook({ studioId: "studio-a", name: "长期资料" });
    await first.dispose();

    const restarted = new LingxiEngine({ lingxiHome, productDir: lingxiHome, agentId: "hana" } as any);
    expect(restarted.knowledge.getNotebook({
      studioId: "studio-a",
      notebookId: notebook.id,
    })).toMatchObject({ name: "长期资料" });
    await restarted.dispose();
  });

  it("分别登记数据库、原始快照、解析产物和可重建索引", () => {
    const stores = new Map(PERSISTENT_STORES.map((store) => [store.id, store]));
    expect(stores.get("knowledge-database")).toMatchObject({
      format: "sqlite",
      affectedByEpochMigration: true,
    });
    expect(stores.get("knowledge-source-snapshots")).toMatchObject({
      pathKind: "tree",
      affectedByEpochMigration: true,
    });
    expect(stores.get("knowledge-parse-artifacts")).toMatchObject({
      pathKind: "tree",
      affectedByEpochMigration: true,
    });
    expect(stores.get("knowledge-indexes")).toMatchObject({
      epochPolicy: "regenerable",
      affectedByEpochMigration: false,
    });
  });
});

import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";

const tempDirs: string[] = [];

function tempRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-import-"));
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

describe("KnowledgeManager 文件导入", () => {
  it("托管不可变字节，外部文件删除和进程重启后仍可完整读取", async () => {
    const root = tempRoot();
    const lingxiHome = path.join(root, "home");
    const externalPath = path.join(root, "outside", "notes.txt");
    const bytes = Buffer.from("第一行\n第二行\n", "utf-8");
    fs.writeFileSync(externalPath, bytes);

    const manager = new KnowledgeManager({ lingxiHome });
    const notebook = manager.createNotebook({ studioId: "studio-a", name: "资料" });
    const imported = await manager.importFile({
      studioId: "studio-a",
      notebookId: notebook.id,
      filePath: externalPath,
    });
    expect(imported.snapshot.sha256).toBe(crypto.createHash("sha256").update(bytes).digest("hex"));
    expect(path.isAbsolute(imported.snapshot.storagePath)).toBe(false);
    expect(manager.readContentSnapshot({
      studioId: "studio-a",
      snapshotId: imported.snapshot.id,
    })).toEqual(bytes);
    await manager.close();

    fs.unlinkSync(externalPath);
    const restarted = new KnowledgeManager({ lingxiHome });
    expect(restarted.readContentSnapshot({
      studioId: "studio-a",
      snapshotId: imported.snapshot.id,
    })).toEqual(bytes);
    await restarted.close();
  });

  it("拒绝相对路径、目录、软链接、LINGXI_HOME 内文件和超限文件", async () => {
    const root = tempRoot();
    const lingxiHome = path.join(root, "home");
    const outside = path.join(root, "outside");
    const normal = path.join(outside, "small.txt");
    fs.writeFileSync(normal, "123456789", "utf-8");
    const manager = new KnowledgeManager({ lingxiHome, maxImportBytes: 8 });
    const notebook = manager.createNotebook({ studioId: "studio-a", name: "安全" });
    const input = { studioId: "studio-a", notebookId: notebook.id };

    await expect(manager.importFile({ ...input, filePath: "small.txt" }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_IMPORT_PATH_INVALID" });
    await expect(manager.importFile({ ...input, filePath: outside }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_IMPORT_FILE_REQUIRED" });
    await expect(manager.importFile({ ...input, filePath: normal }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_IMPORT_TOO_LARGE" });

    const internal = path.join(lingxiHome, "secret.txt");
    fs.writeFileSync(internal, "secret", "utf-8");
    await expect(manager.importFile({ ...input, filePath: internal }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_IMPORT_PATH_BLOCKED" });

    const link = path.join(outside, "link.txt");
    try {
      fs.symlinkSync(normal, link);
      await expect(manager.importFile({ ...input, filePath: link }))
        .rejects.toMatchObject({ code: "KNOWLEDGE_IMPORT_SYMLINK" });
    } catch (error: any) {
      if (error?.code !== "EPERM") throw error;
    }
    await manager.close();
  });
});

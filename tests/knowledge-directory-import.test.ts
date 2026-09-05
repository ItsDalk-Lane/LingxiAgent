import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";

const tempDirs: string[] = [];

function tempRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-dirimport-"));
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

describe("KnowledgeManager 目录导入（§六十九）", () => {
  it("递归导入目录树：membership 写目录组织路径，skipped/failed 显式分组", async () => {
    const root = tempRoot();
    const lingxiHome = path.join(root, "home");
    const tree = path.join(root, "outside", "docs");
    fs.mkdirSync(path.join(tree, "技术文档", "api"), { recursive: true });
    fs.writeFileSync(path.join(tree, "readme.txt"), "顶层说明\n", "utf-8");
    fs.writeFileSync(path.join(tree, "技术文档", "guide.md"), "# 指南\n正文\n", "utf-8");
    fs.writeFileSync(path.join(tree, "技术文档", "api", "spec.csv"), "Region,Total\nNorth,120\n", "utf-8");
    fs.writeFileSync(path.join(tree, "deck.pptx"), Buffer.from("stub"));
    try {
      fs.symlinkSync(path.join(tree, "readme.txt"), path.join(tree, "alias.txt"));
    } catch (error: any) {
      if (error?.code !== "EPERM") throw error;
    }

    const manager = new KnowledgeManager({ lingxiHome });
    const notebook = manager.createNotebook({ studioId: "studio-a", name: "资料" });
    const result = await manager.importDirectory({
      studioId: "studio-a",
      notebookId: notebook.id,
      dirPath: tree,
    });

    expect(result.failed).toEqual([]);
    const importedPaths = result.imported.map(entry => entry.path).sort();
    expect(importedPaths).toEqual([
      "readme.txt",
      "技术文档/api/spec.csv",
      "技术文档/guide.md",
    ]);
    expect(result.imported.every(entry => entry.reused === false)).toBe(true);
    // 不支持的格式与软链接显式 skipped，不静默。
    expect(result.skipped).toContainEqual({
      path: "deck.pptx",
      reason: "KNOWLEDGE_IMPORT_PROCESSOR_UNAVAILABLE",
    });
    if (fs.lstatSync(path.join(tree, "alias.txt")).isSymbolicLink()) {
      expect(result.skipped).toContainEqual({ path: "alias.txt", reason: "symlink_rejected" });
    }

    // 目录组织路径写入 Membership（getMembership 是 store 私有方法，走公开列表查询）。
    const byPath = new Map(result.imported.map(entry => [entry.path, entry.sourceId]));
    const sources = manager.listNotebookSources({ studioId: "studio-a", notebookId: notebook.id });
    const membership = (relative: string) => {
      const entry = sources.find(item => item.source.id === byPath.get(relative));
      expect(entry, `membership for ${relative}`).toBeTruthy();
      return entry!.membership;
    };
    expect(membership("readme.txt")).toMatchObject({
      relativePath: "readme.txt",
      folderNode: null,
    });
    expect(membership("技术文档/guide.md")).toMatchObject({
      relativePath: "技术文档/guide.md",
      folderNode: "技术文档",
    });
    expect(membership("技术文档/api/spec.csv")).toMatchObject({
      relativePath: "技术文档/api/spec.csv",
      folderNode: "技术文档/api",
    });
    // displayOrder 按目录枚举序单调递增且互不重复。
    const orders = result.imported.map(entry => membership(entry.path).displayOrder);
    expect(new Set(orders).size).toBe(orders.length);
    await manager.close();
  });

  it("同内容跨 Notebook 去重复用：同一 Source 在不同 Notebook 可有不同目录位置", async () => {
    const root = tempRoot();
    const lingxiHome = path.join(root, "home");
    const treeA = path.join(root, "outside", "a");
    const treeB = path.join(root, "outside", "b", "nested");
    fs.mkdirSync(treeA, { recursive: true });
    fs.mkdirSync(treeB, { recursive: true });
    const bytes = Buffer.from("相同内容\n", "utf-8");
    fs.writeFileSync(path.join(treeA, "shared.txt"), bytes);
    fs.writeFileSync(path.join(treeB, "shared.txt"), bytes);

    const manager = new KnowledgeManager({ lingxiHome });
    const nb1 = manager.createNotebook({ studioId: "studio-a", name: "甲" });
    const nb2 = manager.createNotebook({ studioId: "studio-a", name: "乙" });
    const first = await manager.importDirectory({
      studioId: "studio-a",
      notebookId: nb1.id,
      dirPath: treeA,
    });
    const second = await manager.importDirectory({
      studioId: "studio-a",
      notebookId: nb2.id,
      dirPath: path.join(root, "outside", "b"),
    });
    expect(first.imported).toHaveLength(1);
    expect(second.imported).toHaveLength(1);
    expect(first.imported[0]?.reused).toBe(false);
    expect(second.imported[0]?.reused).toBe(true);
    expect(second.imported[0]?.sourceId).toBe(first.imported[0]?.sourceId);

    const sourceId = first.imported[0]!.sourceId;
    const membershipOf = (notebookId: string) => {
      const entry = manager.listNotebookSources({ studioId: "studio-a", notebookId })
        .find(item => item.source.id === sourceId);
      expect(entry, `membership in ${notebookId}`).toBeTruthy();
      return entry!.membership;
    };
    expect(membershipOf(nb1.id)).toMatchObject({
      relativePath: "shared.txt",
      folderNode: null,
    });
    expect(membershipOf(nb2.id)).toMatchObject({
      relativePath: "nested/shared.txt",
      folderNode: "nested",
    });
    // 复用不产生第二个快照。
    const snapshots = manager.store.db.prepare(
      "SELECT COUNT(*) AS n FROM content_snapshots WHERE sha256 = ?",
    ).get(crypto.createHash("sha256").update(bytes).digest("hex")) as { n: number };
    expect(snapshots.n).toBe(1);
    await manager.close();
  });

  it("拒绝相对路径、文件路径与软链接目录", async () => {
    const root = tempRoot();
    const lingxiHome = path.join(root, "home");
    const outside = path.join(root, "outside");
    fs.writeFileSync(path.join(outside, "file.txt"), "x", "utf-8");
    const manager = new KnowledgeManager({ lingxiHome });
    const notebook = manager.createNotebook({ studioId: "studio-a", name: "资料" });
    const input = { studioId: "studio-a", notebookId: notebook.id };

    await expect(manager.importDirectory({ ...input, dirPath: "docs" }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_INVALID_ARGUMENT" });
    await expect(manager.importDirectory({ ...input, dirPath: path.join(outside, "file.txt") }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_INVALID_ARGUMENT" });
    await expect(manager.importDirectory({ ...input, dirPath: path.join(outside, "missing") }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_INVALID_ARGUMENT" });
    try {
      fs.symlinkSync(outside, path.join(root, "dirlink"));
      await expect(manager.importDirectory({ ...input, dirPath: path.join(root, "dirlink") }))
        .rejects.toMatchObject({ code: "KNOWLEDGE_INVALID_ARGUMENT" });
    } catch (error: any) {
      if (error?.code !== "EPERM") throw error;
    }
    await manager.close();
  });
});

import fs from "node:fs";
import path from "node:path";
import Database from "better-sqlite3";
import { spawnSync } from "node:child_process";
import { afterEach, expect, it } from "vitest";
import { KnowledgeIndexStore } from "../lib/knowledge/knowledge-index-store.ts";
const roots: string[] = [];
function fixture() {
  if (!process.env.LINGXI_HOME) throw new Error("必须使用合成 LINGXI_HOME");
  fs.mkdirSync(process.env.LINGXI_HOME, { recursive: true });
  const root = fs.mkdtempSync(path.join(process.env.LINGXI_HOME, "fulltext-")); roots.push(root);
  return path.join(root, "knowledge-index.db");
}
afterEach(() => { for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true }); });
it("positive fulltext open and reopen", () => {
  const dbPath = fixture(); new KnowledgeIndexStore({ dbPath }).close();
  const store = new KnowledgeIndexStore({ dbPath }); expect(store.health()).toEqual({ status: "ready" }); store.close();
});
it("reset keeps live inode and durable recovery copy", () => {
  const dbPath = fixture(); const store = new KnowledgeIndexStore({ dbPath });
  const observer = new Database(dbPath); observer.exec("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('original');");
  const inode = fs.statSync(dbPath).ino; store.reset();
  expect(fs.statSync(dbPath).ino).toBe(inode);
  const quarantine = fs.readdirSync(path.dirname(dbPath)).find(name => name.includes(".quarantine-")); expect(quarantine).toBeDefined();
  const backup = new Database(path.join(path.dirname(dbPath), quarantine!, "original.db"), { readonly: true });
  expect(backup.prepare("SELECT value FROM sentinel").get()).toEqual({ value: "original" });
  expect(backup.pragma("quick_check", { simple: true })).toBe("ok");
  backup.close(); observer.close(); store.close();
});
it("future live WAL and old handle refuse writes without deleting originals", () => {
  const dbPath = fixture(); const store = new KnowledgeIndexStore({ dbPath }); const writer = new Database(dbPath);
  writer.exec("PRAGMA wal_autocheckpoint=0; CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('future'); PRAGMA user_version=999;");
  const before = ["", "-wal"].map(suffix => fs.readFileSync(dbPath + suffix));
  expect(() => new KnowledgeIndexStore({ dbPath })).toThrow(/newer/i);
  expect(() => store.reset()).toThrow(/newer/i);
  expect(() => store.removeArtifact("artifact")).toThrow(/newer/i);
  expect(() => store.ensureChunkIndexVariant({parseArtifactId:"artifact",chunkProfileHash:"1234567890abcdef",blockFingerprint:"test"})).toThrow(/newer/i);
  expect(() => store.setChunkIndexVariantStatus("missing", "failed")).toThrow(/newer/i);
  expect(() => store.upsertSourceDocument({parseArtifactId:"artifact",title:"title",outlineText:"text",searchText:"text"})).toThrow(/newer/i);
  expect(["", "-wal"].map(suffix => fs.readFileSync(dbPath + suffix))).toEqual(before);
  expect(writer.prepare("SELECT value FROM sentinel").get()).toEqual({ value: "future" }); writer.close(); store.close();
});
it("unknown corrupt file and WAL stay intact", () => {
  const dbPath = fixture(); fs.writeFileSync(dbPath, "unknown corrupt"); fs.writeFileSync(dbPath + "-wal", "valuable");
  expect(() => new KnowledgeIndexStore({ dbPath })).toThrow(/preserved/i);
  expect(fs.readFileSync(dbPath, "utf8")).toBe("unknown corrupt"); expect(fs.readFileSync(dbPath + "-wal", "utf8")).toBe("valuable");
});
it("writer lock and backup permission failure do not reset cache", () => {
  const dbPath = fixture(); const store = new KnowledgeIndexStore({ dbPath }); const writer = new Database(dbPath);
  writer.exec("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('kept'); BEGIN IMMEDIATE;");
  const inode = fs.statSync(dbPath).ino; expect(() => store.reset()).toThrow();
  writer.exec("ROLLBACK"); fs.chmodSync(path.dirname(dbPath), 0o500);
  try { expect(() => store.reset()).toThrow(); expect(writer.prepare("SELECT value FROM sentinel").get()).toEqual({ value: "kept" }); expect(fs.statSync(dbPath).ino).toBe(inode); }
  finally { fs.chmodSync(path.dirname(dbPath), 0o700); writer.close(); store.close(); }
});

it("crashed future WAL remains uncheckpointed when an old opener refuses", () => {
  const dbPath = fixture();
  new KnowledgeIndexStore({ dbPath }).close();
  const child = spawnSync(process.execPath, ["--input-type=module", "-e", `
    import Database from 'better-sqlite3';
    const db=new Database(process.env.F12_DB);
    db.exec("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('uncheckpointed'); PRAGMA user_version=999;");
    process.exit(0);
  `], { env: { ...process.env, F12_DB: dbPath } });
  expect(child.status).toBe(0);
  const before=["", "-wal", "-shm"].map(suffix=>fs.readFileSync(dbPath+suffix));
  expect(()=>new KnowledgeIndexStore({dbPath})).toThrow(/newer/i);
  const after = ["", "-wal", "-shm"].map(suffix=>fs.readFileSync(dbPath+suffix));
  expect(after.slice(0, 2)).toEqual(before.slice(0, 2));
  // SHM是易失的SQLite读锁/索引，不是付费向量；读者恢复可更新它，不能删掉它。
  expect(after[2].length).toBe(before[2].length);
  const probe = new Database(dbPath, { readonly: true });
  expect(probe.prepare("SELECT value FROM sentinel").get()).toEqual({ value: "uncheckpointed" });
  expect(probe.pragma("user_version", { simple: true })).toBe(999);
  probe.close();
});

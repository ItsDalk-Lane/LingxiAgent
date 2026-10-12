import fs from "node:fs";
import path from "node:path";
import Database from "better-sqlite3";
import { spawnSync } from "node:child_process";
import { afterEach, expect, it } from "vitest";
import { PortableVectorIndexAdapter } from "../lib/knowledge/vector-index-adapter.ts";

const roots: string[] = [];
function fixture() {
  const home = process.env.LINGXI_HOME;
  if (!home) throw new Error("必须使用合成 LINGXI_HOME");
  fs.mkdirSync(home, { recursive: true });
  const root = fs.mkdtempSync(path.join(home, "ts-vector-"));
  roots.push(root);
  return path.join(root, "knowledge-vector.db");
}
afterEach(() => { for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true }); });
it("positive normal vector open and reopen", () => {
  const dbPath = fixture();
  new PortableVectorIndexAdapter({ dbPath }).close();
  const reopened = new PortableVectorIndexAdapter({ dbPath });
  expect(reopened.health()).toEqual({ status: "ready" });
  reopened.close();
});
it("future schema keeps paid sentinel and version", () => {
  const dbPath = fixture();
  const db = new Database(dbPath);
  db.exec("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('paid-vector'); PRAGMA user_version=999;");
  db.close();
  const before = fs.readFileSync(dbPath);
  expect(() => new PortableVectorIndexAdapter({ dbPath })).toThrow(/newer/i);
  expect(fs.readFileSync(dbPath)).toEqual(before);
  const check = new Database(dbPath);
  expect(check.prepare("SELECT value FROM sentinel").get()).toEqual({ value: "paid-vector" });
  expect(check.pragma("user_version", { simple: true })).toBe(999);
  check.close();
});

it("future live WAL writer keeps every database file", () => {
  const dbPath = fixture();
  new PortableVectorIndexAdapter({ dbPath }).close();
  const writer = new Database(dbPath);
  writer.exec("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('committed'); PRAGMA user_version=999; BEGIN IMMEDIATE; INSERT INTO sentinel VALUES('uncommitted');");
  const before = ["", "-wal", "-shm"].map(suffix => fs.readFileSync(dbPath + suffix));
  expect(() => new PortableVectorIndexAdapter({ dbPath })).toThrow(/newer/i);
  expect(["", "-wal", "-shm"].map(suffix => fs.readFileSync(dbPath + suffix))).toEqual(before);
  writer.exec("COMMIT");
  expect(writer.prepare("SELECT count(*) AS n FROM sentinel").get()).toEqual({ n: 2 });
  writer.close();
});
it("unknown damaged cache and sidecars are retained for offline recovery", () => {
  const dbPath = fixture();
  fs.writeFileSync(dbPath, "external damaged file");
  fs.writeFileSync(dbPath + "-wal", "valuable WAL");
  expect(() => new PortableVectorIndexAdapter({ dbPath })).toThrow(/preserved/i);
  expect(fs.readFileSync(dbPath, "utf8")).toBe("external damaged file");
  expect(fs.readFileSync(dbPath + "-wal", "utf8")).toBe("valuable WAL");
});
it("rebuild stores a consistent full original before changing live tables", () => {
  const dbPath = fixture();
  const adapter = new PortableVectorIndexAdapter({ dbPath });
  const model = { key: "paid", provider: "local", modelId: "synthetic", protocol: "test", dimensions: 2 };
  adapter.buildOrReplaceArtifact({ parseArtifactId: "source-a", chunkFingerprint: "fingerprint", model,
    entries: [{ chunkId: "chunk-a", parseArtifactId: "source-a", ordinal: 0, vector: [0.2, 0.8] }] });
  const observer = new Database(dbPath);
  expect(observer.prepare("SELECT count(*) AS n FROM chunk_vectors").get()).toEqual({ n: 1 });
  const originalInode = fs.statSync(dbPath).ino;
  adapter.rebuild();
  expect(fs.statSync(dbPath).ino).toBe(originalInode);
  expect(observer.prepare("SELECT count(*) AS n FROM chunk_vectors").get()).toEqual({ n: 0 });
  const quarantines = fs.readdirSync(path.dirname(dbPath)).filter(name => name.includes(".quarantine-"));
  expect(quarantines).toHaveLength(1);
  const backup = new Database(path.join(path.dirname(dbPath), quarantines[0], "original.db"), { readonly: true });
  expect(backup.prepare("SELECT count(*) AS n FROM chunk_vectors").get()).toEqual({ n: 1 });
  expect(backup.prepare("SELECT vector FROM chunk_vectors").get()).toEqual(expect.objectContaining({ vector: expect.any(Buffer) }));
  expect(backup.pragma("quick_check", { simple: true })).toBe("ok");
  expect(JSON.parse(fs.readFileSync(path.join(path.dirname(dbPath), quarantines[0], "recovery.json"), "utf8")))
    .toEqual(expect.objectContaining({ state: "snapshot_retained_reindex_required", embeddingsRegenerated: false }));
  backup.close(); observer.close(); adapter.close();
});
it("active other writer prevents rebuild and keeps paid vectors", () => {
  const dbPath = fixture();
  const adapter = new PortableVectorIndexAdapter({ dbPath });
  adapter.db.pragma("busy_timeout=1");
  const writer = new Database(dbPath);
  writer.exec("BEGIN IMMEDIATE");
  expect(() => adapter.rebuild()).toThrow(/locked/i);
  expect(fs.existsSync(dbPath)).toBe(true);
  expect(fs.readdirSync(path.dirname(dbPath)).some(name => name.includes(".quarantine-"))).toBe(false);
  writer.exec("ROLLBACK");
  writer.close(); adapter.close();
});
it("snapshot persistence failure cannot clear the original tables", () => {
  const dbPath = fixture();
  const adapter = new PortableVectorIndexAdapter({ dbPath });
  adapter.db.exec("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('original');");
  const root = path.dirname(dbPath);
  fs.chmodSync(root, 0o555);
  try { expect(() => adapter.rebuild()).toThrow(); }
  finally { fs.chmodSync(root, 0o755); }
  expect(adapter.db.prepare("SELECT value FROM sentinel").get()).toEqual({ value: "original" });
  adapter.close();
});

it("crashed future WAL remains uncheckpointed when an old opener refuses", () => {
  const dbPath = fixture();
  new PortableVectorIndexAdapter({ dbPath }).close();
  const child = spawnSync(process.execPath, ["--input-type=module", "-e", `
    import Database from 'better-sqlite3';
    const db=new Database(process.env.F12_DB);
    db.exec("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('uncheckpointed'); PRAGMA user_version=999;");
    process.exit(0);
  `], { env: { ...process.env, F12_DB: dbPath } });
  expect(child.status).toBe(0);
  const before=["", "-wal", "-shm"].map(suffix=>fs.readFileSync(dbPath+suffix));
  expect(()=>new PortableVectorIndexAdapter({dbPath})).toThrow(/newer/i);
  const after = ["", "-wal", "-shm"].map(suffix=>fs.readFileSync(dbPath+suffix));
  expect(after.slice(0, 2)).toEqual(before.slice(0, 2));
  // SHM是易失的SQLite读锁/索引，不是付费向量；读者恢复可更新它，不能删掉它。
  expect(after[2].length).toBe(before[2].length);
  const probe = new Database(dbPath, { readonly: true });
  expect(probe.prepare("SELECT value FROM sentinel").get()).toEqual({ value: "uncheckpointed" });
  expect(probe.pragma("user_version", { simple: true })).toBe(999);
  probe.close();
});

it("existing old vector handle refuses future schema writes", () => {
  const dbPath = fixture();
  const adapter = new PortableVectorIndexAdapter({ dbPath });
  const newer = new Database(dbPath);
  newer.pragma("user_version=999");
  expect(() => adapter.rebuild()).toThrow(/newer/i);
  expect(() => adapter.touchVectorVariants(["missing"])).toThrow(/newer/i);
  expect(() => adapter.setVariantStatus("missing", "failed")).toThrow(/newer/i);
  expect(() => adapter.removeArtifact("source")).toThrow(/newer/i);
  expect(() => adapter.removeVariant("variant")).toThrow(/newer/i);
  expect(() => adapter.removeArtifactModel({ parseArtifactId: "source", modelKey: "model" })).toThrow(/newer/i);
  const model = { key: "model", provider: "local", modelId: "synthetic", protocol: "test", dimensions: 2 };
  expect(() => adapter.buildOrReplaceArtifact({ parseArtifactId: "source", chunkFingerprint: "fingerprint", model,
    entries: [{ chunkId: "chunk", parseArtifactId: "source", ordinal: 0, vector: [0.2, 0.8] }] })).toThrow(/newer/i);
  expect(() => adapter.beginVectorVariantBuild({ parseArtifactId: "source", chunkFingerprint: "fingerprint", model })).toThrow(/newer/i);
  expect(() => adapter.upsertChunkVectorBatch({ vectorIndexVariantId: "variant", chunkFingerprint: "fingerprint", model,
    entries: [{ chunkId: "chunk", ordinal: 0, vector: [0.2, 0.8] }] })).toThrow(/newer/i);
  expect(() => adapter.completeVectorVariantBuild({ vectorIndexVariantId: "variant", chunkFingerprint: "fingerprint", expectedChunkCount: 1 })).toThrow(/newer/i);
  expect(() => adapter.failVectorVariantBuild("variant")).toThrow(/newer/i);
  expect(newer.pragma("user_version", { simple: true })).toBe(999);
  expect(fs.readdirSync(path.dirname(dbPath)).some(name => name.includes(".quarantine-"))).toBe(false);
  newer.close(); adapter.close();
});

function populatedSearch() {
  const dbPath = fixture();
  let now = "2020-01-01T00:00:00Z";
  const adapter = new PortableVectorIndexAdapter({ dbPath, now: () => now });
  const model = { key: "model", provider: "local", modelId: "synthetic", protocol: "test", dimensions: 2 };
  adapter.buildOrReplaceArtifact({ parseArtifactId: "artifact", chunkFingerprint: "fingerprint", model,
    entries: [{ chunkId: "chunk", parseArtifactId: "artifact", ordinal: 0, vector: [0.2, 0.8] }] });
  now = "2040-01-01T00:00:00Z";
  const observer = new Database(dbPath);
  const search = () => adapter.search({ parseArtifactIds: ["artifact"], model, queryVector: [0.2, 0.8] });
  return { adapter, observer, search };
}

it("search refreshes retention use time normally and survives reopen", () => {
  const { adapter, observer, search } = populatedSearch();
  expect(search()).toHaveLength(1);
  expect(adapter.listArtifactUsage()[0].lastUsedAt).toBe("2040-01-01T00:00:00Z");
  const dbPath = adapter.dbPath;
  observer.close(); adapter.close();
  const reopened = new PortableVectorIndexAdapter({ dbPath });
  expect(reopened.listArtifactUsage()[0].lastUsedAt).toBe("2040-01-01T00:00:00Z");
  reopened.close();
});

it("search refuses future schema upgraded just before usage transaction", () => {
  const { adapter, observer, search } = populatedSearch();
  const before = observer.prepare("SELECT * FROM vector_index_variants").all();
  const transaction = adapter.db.transaction.bind(adapter.db);
  let crossed = false;
  // 只安排真实连接的升级时机，不替换SQL执行和返回结果。
  adapter.db.transaction = (operation: () => unknown) => {
    const tx = transaction(operation);
    const immediate = tx.immediate.bind(tx);
    return { immediate: (...args: unknown[]) => {
      observer.pragma("user_version=999"); crossed = true;
      return immediate(...args);
    } };
  };
  expect(search).toThrow(/newer/i);
  expect(crossed).toBe(true);
  expect(observer.pragma("user_version", { simple: true })).toBe(999);
  expect(observer.prepare("SELECT * FROM vector_index_variants").all()).toEqual(before);
  expect(observer.prepare("SELECT count(*) AS n FROM chunk_vectors").get()).toEqual({ n: 1 });
  observer.close(); adapter.close();
});

it("search holds real write lock across version check and use time update", () => {
  const { adapter, observer, search } = populatedSearch();
  observer.pragma("busy_timeout=1");
  const prepare = adapter.db.prepare.bind(adapter.db);
  let attempted = false;
  adapter.db.prepare = (sql: string) => {
    if (sql.includes("UPDATE vector_index_variants SET last_used_at")) {
      attempted = true;
      expect(() => observer.pragma("user_version=999")).toThrow(/locked/i);
    }
    return prepare(sql);
  };
  expect(search()).toHaveLength(1);
  expect(attempted).toBe(true);
  expect(observer.pragma("user_version", { simple: true })).toBe(3);
  expect(adapter.listArtifactUsage()[0].lastUsedAt).toBe("2040-01-01T00:00:00Z");
  observer.close(); adapter.close();
});

it("search preserves best effort usage under actual lock and readonly failures", () => {
  const { adapter, observer, search } = populatedSearch();
  adapter.db.pragma("busy_timeout=1");
  observer.exec("BEGIN IMMEDIATE");
  expect(search()).toHaveLength(1);
  expect(adapter.listArtifactUsage()[0].lastUsedAt).toBe("2020-01-01T00:00:00Z");
  observer.exec("ROLLBACK");
  adapter.db.pragma("query_only=ON");
  expect(search()).toHaveLength(1);
  expect(adapter.listArtifactUsage()[0].lastUsedAt).toBe("2020-01-01T00:00:00Z");
  adapter.db.pragma("query_only=OFF");
  expect(search()).toHaveLength(1);
  expect(adapter.listArtifactUsage()[0].lastUsedAt).toBe("2040-01-01T00:00:00Z");
  observer.close(); adapter.close();
});

it("search cannot hide a non IO use time constraint error", () => {
  const { adapter, observer, search } = populatedSearch();
  observer.exec("CREATE TRIGGER reject_usage BEFORE UPDATE OF last_used_at ON vector_index_variants BEGIN SELECT RAISE(ABORT, 'synthetic contract failure'); END;");
  expect(search).toThrow(/query failed/i);
  expect(adapter.listArtifactUsage()[0].lastUsedAt).toBe("2020-01-01T00:00:00Z");
  observer.close(); adapter.close();
});

it("search keeps paid vectors when real storage full prevents usage commit", () => {
  const { adapter, observer, search } = populatedSearch();
  observer.exec("CREATE TABLE bounded_io(payload BLOB); CREATE TRIGGER fill_usage BEFORE UPDATE OF last_used_at ON vector_index_variants BEGIN INSERT INTO bounded_io VALUES(zeroblob(1048576)); END;");
  const pages = Number(adapter.db.pragma("page_count", { simple: true }));
  adapter.db.pragma(`max_page_count=${pages}`);
  expect(search()).toHaveLength(1);
  expect(adapter.listArtifactUsage()[0].lastUsedAt).toBe("2020-01-01T00:00:00Z");
  expect(observer.prepare("SELECT count(*) AS n FROM chunk_vectors").get()).toEqual({ n: 1 });
  expect(observer.prepare("SELECT count(*) AS n FROM bounded_io").get()).toEqual({ n: 0 });
  observer.close(); adapter.close();
});

import fs from "node:fs";
import path from "node:path";
import Database from "better-sqlite3";
import { afterEach, expect, it } from "vitest";
import { PortableVectorIndexAdapter } from "../lib/knowledge/vector-index-adapter.ts";
import { createKnowledgeVectorSearchBackend } from "../lib/knowledge/vector-search-backend-factory.ts";
import { searchVectorBackend } from "../lib/knowledge/vector-search-backend.ts";
import { AnnIndexStore } from "../lib/knowledge/ann-index-store.ts";
const roots: string[] = [];
function fixture() {
  if (!process.env.LINGXI_HOME) throw new Error("必须使用合成 LINGXI_HOME");
  fs.mkdirSync(process.env.LINGXI_HOME, { recursive: true }); const root=fs.mkdtempSync(path.join(process.env.LINGXI_HOME,"ann-")); roots.push(root); return root;
}
afterEach(() => { for(const root of roots.splice(0)) fs.rmSync(root,{recursive:true,force:true}); });
it("positive normal ANN opens and reopens", () => { const dbPath=path.join(fixture(),"knowledge-ann.db"); new AnnIndexStore({dbPath}).close(); new AnnIndexStore({dbPath}).close(); });
it("future ANN sentinel is retained with observable portable fallback", async () => {
  const root=fixture(), dbPath=path.join(root,"knowledge-ann.db"); const db=new Database(dbPath); db.exec("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('future'); PRAGMA user_version=999;"); db.close();
  const before=fs.readFileSync(dbPath); const portable=new PortableVectorIndexAdapter({dbPath:path.join(root,"knowledge-vector.db")}); const logs: string[]=[];
  const model={key:"paid",provider:"local",modelId:"synthetic",protocol:"test",dimensions:2};
  const variant=portable.buildOrReplaceArtifact({parseArtifactId:"artifact",chunkFingerprint:"fingerprint",model,entries:[{chunkId:"chunk",parseArtifactId:"artifact",ordinal:0,vector:[0.2,0.8]}]});
  const paidBefore=fs.readFileSync(portable.dbPath+"-wal");
  const backend=createKnowledgeVectorSearchBackend({indexesRoot:root,portable,log:m=>logs.push(m)});
  try { expect(fs.readFileSync(dbPath)).toEqual(before); expect(logs.join(" ")).toContain("ANN_CATALOG_UNAVAILABLE"); expect(fs.readFileSync(portable.dbPath+"-wal")).toEqual(paidBefore);
  const result=await searchVectorBackend(backend,{vectorIndexVariantIds:[variant.vectorIndexVariantId],model,queryVector:[0.2,0.8],limit:1});
  expect(result.vectorBackend).toBe("portable"); expect(result.degradedReasons).toContain("ANN_CATALOG_UNAVAILABLE"); expect(result.results[0].chunkId).toBe("chunk");
  const check=new Database(dbPath,{readonly:true}); expect(check.prepare("SELECT value FROM sentinel").get()).toEqual({value:"future"}); check.close(); }
  finally { await backend.close(); portable.close(); }
});

it("future live ANN writer and old handle cannot be reset by fallback", async () => {
 const root=fixture(),dbPath=path.join(root,"knowledge-ann.db"),store=new AnnIndexStore({dbPath});
 const writer=new Database(dbPath); writer.exec("PRAGMA wal_autocheckpoint=0; CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('future'); PRAGMA user_version=999; BEGIN IMMEDIATE;");
 const before=["","-wal"].map(s=>fs.readFileSync(dbPath+s));
 expect(()=>new AnnIndexStore({dbPath})).toThrow(/newer/);
 writer.exec("ROLLBACK"); expect(()=>store.markFailed("unknown")).toThrow(/newer/); expect(()=>store.markReady("unknown")).toThrow(/newer/);
 expect(["","-wal"].map(s=>fs.readFileSync(dbPath+s))).toEqual(before); writer.close(); store.close();
});
it("unknown corrupt ANN catalog and sidecars stay intact during fallback", async () => {
 const root=fixture(),dbPath=path.join(root,"knowledge-ann.db"); fs.writeFileSync(dbPath,"unknown");fs.writeFileSync(dbPath+"-wal","valuable");
 const portable=new PortableVectorIndexAdapter({dbPath:path.join(root,"knowledge-vector.db")}); const logs:string[]=[];
 const backend=createKnowledgeVectorSearchBackend({indexesRoot:root,portable,log:m=>logs.push(m)});
 try {expect(fs.readFileSync(dbPath,"utf8")).toBe("unknown");expect(fs.readFileSync(dbPath+"-wal","utf8")).toBe("valuable");expect(logs.join(" ")).toContain("ANN_CATALOG_UNAVAILABLE");}
 finally {await backend.close();portable.close();}
});

import { KnowledgeIndexStore } from "../lib/knowledge/knowledge-index-store.ts";
const cacheFamilies = [
  ["vector", PortableVectorIndexAdapter, 3],
  ["fulltext", KnowledgeIndexStore, 4],
  ["ann", AnnIndexStore, 1],
] as const;

it("all cache initializers reject an upgrade before the exclusive lock without changing journal bytes", () => {
  for (const [kind, Store] of cacheFamilies) {
    const dbPath = path.join(fixture(), `${kind}.db`);
    new Store({ dbPath }).close();
    const writer = new Database(dbPath, { timeout: 0 });
    writer.pragma("journal_mode = DELETE");
    writer.exec("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('paid-original');");
    let scheduled = false;
    let before!: Buffer;
    function ScheduledDatabase(file: string, options?: any) {
      const db = new Database(file, options), pragma = db.pragma.bind(db);
      if (!options?.readonly) db.pragma = ((sql: string, ...args: any[]) => {
        if (!scheduled && /^journal_mode/u.test(sql)) {
          writer.pragma("user_version = 999");
          scheduled = true;
          before = fs.readFileSync(dbPath);
        }
        return pragma(sql, ...args);
      }) as any;
      return db;
    }
    try {
      expect(() => new Store({ dbPath, Database: ScheduledDatabase })).toThrowError(expect.objectContaining({ code: "KNOWLEDGE_SCHEMA_NEWER" }));
      expect(scheduled).toBe(true);
      expect(fs.readFileSync(dbPath)).toEqual(before);
      const check = new Database(dbPath, { readonly: true });
      try {
        expect(check.pragma("journal_mode", { simple: true })).toBe("delete");
        expect(check.pragma("user_version", { simple: true })).toBe(999);
        expect(check.prepare("SELECT * FROM sentinel").all()).toEqual([{ value: "paid-original" }]);
      } finally { check.close(); }
    } finally { writer.close(); }
  }
});

it("all cache journal switches retain the real exclusive lock and release it after initialization", () => {
  for (const [kind, Store, supported] of cacheFamilies) {
    const dbPath = path.join(fixture(), `${kind}.db`);
    new Store({ dbPath }).close();
    const writer = new Database(dbPath, { timeout: 0 });
    writer.pragma("journal_mode = DELETE");
    writer.exec("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('normal-original');");
    let attempted = false, blocked = false;
    function ScheduledDatabase(file: string, options?: any) {
      const db = new Database(file, options), pragma = db.pragma.bind(db);
      if (!options?.readonly) db.pragma = ((sql: string, ...args: any[]) => {
        if (/^journal_mode\s*=\s*WAL$/iu.test(sql)) {
          attempted = true;
          try { writer.pragma("user_version = 999"); }
          catch (error: any) { expect(error.code).toBe("SQLITE_BUSY"); blocked = true; }
        }
        return pragma(sql, ...args);
      }) as any;
      return db;
    }
    const store = new Store({ dbPath, Database: ScheduledDatabase });
    try {
      expect(attempted).toBe(true); expect(blocked).toBe(true);
      expect(writer.pragma("user_version", { simple: true })).toBe(supported);
      // 正常初始化后另一真实连接能写入，证明没有留下永久独占锁。
      writer.prepare("UPDATE sentinel SET value = ?").run("normal-write-after-open");
      expect(writer.prepare("SELECT * FROM sentinel").all()).toEqual([{ value: "normal-write-after-open" }]);
    } finally { writer.close(); store.close(); }
    new Store({ dbPath }).close();
  }
});

it("all old cache handles close without checkpointing future WAL after the upgrading writer closes", () => {
  for (const [kind, Store] of cacheFamilies) {
    const dbPath = path.join(fixture(), `${kind}.db`), store = new Store({ dbPath });
    const writer = new Database(dbPath);
    writer.exec("PRAGMA wal_autocheckpoint = 0; CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('future-wal'); PRAGMA user_version = 999;");
    writer.close();
    const before = ["", "-wal"].map(s => fs.readFileSync(dbPath + s));
    expect(before[1].length).toBeGreaterThan(32);
    store.close();
    expect(["", "-wal"].map(s => fs.readFileSync(dbPath + s))).toEqual(before);
    expect(() => new Store({ dbPath })).toThrowError(expect.objectContaining({ code: "KNOWLEDGE_SCHEMA_NEWER" }));
    expect(["", "-wal"].map(s => fs.readFileSync(dbPath + s))).toEqual(before);
    const check = new Database(dbPath, { readonly: true });
    try {
      expect(check.pragma("user_version", { simple: true })).toBe(999);
      expect(check.prepare("SELECT * FROM sentinel").all()).toEqual([{ value: "future-wal" }]);
    } finally { check.close(); }
  }
});

it("all WAL initializers preserve future frames when upgraded after their readonly preflight", () => {
  for (const [kind, Store] of cacheFamilies) {
    const dbPath = path.join(fixture(), `${kind}.db`), original = new Store({ dbPath });
    const writer = new Database(dbPath);
    let upgraded = false;
    let before!: Buffer[];
    function ScheduledDatabase(file: string, options?: any) {
      const db = new Database(file, options);
      if (!options?.readonly && !upgraded) {
        writer.exec("PRAGMA wal_autocheckpoint = 0; CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('future-init-wal'); PRAGMA user_version = 999;");
        writer.close(); upgraded = true;
        before = ["", "-wal"].map(s => fs.readFileSync(dbPath + s));
      }
      return db;
    }
    try {
      expect(() => new Store({ dbPath, Database: ScheduledDatabase })).toThrowError(expect.objectContaining({ code: "KNOWLEDGE_SCHEMA_NEWER" }));
      expect(upgraded).toBe(true);
      expect(["", "-wal"].map(s => fs.readFileSync(dbPath + s))).toEqual(before);
    } finally { original.close(); }
    expect(["", "-wal"].map(s => fs.readFileSync(dbPath + s))).toEqual(before);
  }
});

it("real readonly journal configuration failures release all cache handles and preserve the rollback original", () => {
  for (const [kind, Store] of cacheFamilies) {
    const dbPath = path.join(fixture(), `${kind}.db`);
    new Store({ dbPath }).close();
    const writer = new Database(dbPath, { timeout: 0 }); writer.pragma("journal_mode = DELETE");
    writer.exec("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('original');");
    const before = fs.readFileSync(dbPath);
    function FailingDatabase(file: string, options?: any) {
      // 真实只读 SQLite 连接：版本和锁操作照常执行，持久化 WAL 切换由 SQLite 拒绝。
      return new Database(file, { ...options, readonly: true, fileMustExist: true });
    }
    try {
      expect(() => new Store({ dbPath, Database: FailingDatabase })).toThrow();
      expect(fs.readFileSync(dbPath)).toEqual(before);
      writer.prepare("UPDATE sentinel SET value = ?").run("write-after-error");
      expect(writer.prepare("SELECT * FROM sentinel").all()).toEqual([{ value: "write-after-error" }]);
    } finally { writer.close(); }
    new Store({ dbPath }).close();
  }
});

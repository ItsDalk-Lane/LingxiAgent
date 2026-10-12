import fs from "node:fs";
import path from "node:path";
import assert from "node:assert/strict";
import crypto from "node:crypto";
import {createRequire} from "node:module";
const root=process.cwd();
const {PortableVectorIndexAdapter}=await import(root+"/lib/knowledge/vector-index-adapter.ts");
const {KnowledgeIndexStore}=await import(root+"/lib/knowledge/knowledge-index-store.ts");
const {AnnIndexStore}=await import(root+"/lib/knowledge/ann-index-store.ts");
const Database=createRequire(root+"/package.json")("better-sqlite3");
const hash=b=>crypto.createHash("sha256").update(b).digest("hex");
const rows=db=>db.prepare("SELECT * FROM "+(db.name.includes("vector")?"chunk_vectors":"knowledge_chunks")).all();
const home=process.env.LINGXI_HOME; assert(home);
const model={key:"paid",provider:"local",modelId:"synthetic",protocol:"test",dimensions:2};
for(const [kind,Cls,table] of [["vector",PortableVectorIndexAdapter,"vector_index_variants"],["fulltext",KnowledgeIndexStore,"chunk_index_variants"]]) {
 const dbPath=path.join(home,kind+".db"),store=new Cls({dbPath}),observer=new Database(dbPath);
 if(kind==="vector") store.buildOrReplaceArtifact({parseArtifactId:"artifact",chunkFingerprint:"finger",model,entries:[{chunkId:"chunk",parseArtifactId:"artifact",ordinal:0,vector:[0.2,0.8]}]});
 else store.replaceArtifactChunks({parseArtifactId:"artifact",chunkProfileHash:"1234567890abcdef",blockFingerprint:"finger",chunks:[{id:"chunk",parseArtifactId:"artifact",ordinal:0,text:"精确原文",tokenCount:4,spans:[]}]});
 observer.exec("CREATE TABLE sentinel(value TEXT);INSERT INTO sentinel VALUES('foreign untouched');CREATE TRIGGER fail_late BEFORE DELETE ON "+table+" BEGIN SELECT RAISE(ABORT,'late-reset-denied'); END;");
 const before={rows:rows(observer),variants:observer.prepare("SELECT * FROM "+table).all(),inode:fs.statSync(dbPath).ino,db:hash(fs.readFileSync(dbPath)),wal:hash(fs.readFileSync(dbPath+"-wal"))};console.log(JSON.stringify({phase:"INPUT_BEFORE",kind,home,dbPath,before}));
 let error;try{kind==="vector"?store.rebuild():store.reset();}catch(e){error={code:e.code,message:e.message};}
 const after={rows:rows(observer),variants:observer.prepare("SELECT * FROM "+table).all(),inode:fs.statSync(dbPath).ino,sentinel:observer.prepare("SELECT * FROM sentinel").all()};
 const q=fs.readdirSync(home).find(n=>n.startsWith(kind+".db.quarantine-")), backup=new Database(path.join(home,q,"original.db"),{readonly:true});
 const receipt=JSON.parse(fs.readFileSync(path.join(home,q,"recovery.json"),"utf8"));
 console.log(JSON.stringify({phase:"AFTER",kind,error,after,backupRows:rows(backup),receipt,backupSha:hash(fs.readFileSync(path.join(home,q,"original.db")))}));
 assert.equal(error.code,"SQLITE_CONSTRAINT_TRIGGER");assert.deepEqual(after.rows,before.rows);assert.deepEqual(after.variants,before.variants);assert.equal(after.inode,before.inode);assert.deepEqual(rows(backup),before.rows);assert.equal(backup.pragma("quick_check",{simple:true}),"ok");
 backup.close();observer.exec("DROP TRIGGER fail_late;PRAGMA user_version=999;");
 const snapshot=observer.serialize(), denied=[];
 if(kind==="fulltext") {
  const methods={writeVariantMetadata:()=>store.writeVariantMetadata("id",{firstHeadingPath:null,sectionKeys:[]}),upsertSections:()=>store.upsertSections("artifact",[{id:"section",parseArtifactId:"artifact",sectionOrdinal:0,headingPath:[],startBlockOrdinal:0,endBlockOrdinal:0,text:"原文",tokenCount:1}]),replaceArtifactChunks:()=>store.replaceArtifactChunks({parseArtifactId:"artifact",chunkProfileHash:"1234567890abcdef",blockFingerprint:"finger",chunks:[{id:"chunk",parseArtifactId:"artifact",ordinal:0,text:"原文",tokenCount:1,spans:[]}]}),removeChunkIndexVariantsByArtifact:()=>store.removeChunkIndexVariantsByArtifact("artifact")};
  for(const [name,fn] of Object.entries(methods)){let e;try{fn()}catch(x){e=x};assert.equal(e?.code,"KNOWLEDGE_SCHEMA_NEWER");denied.push(name)}
 }
 assert.deepEqual(observer.serialize(),snapshot);console.log(JSON.stringify({phase:"FUTURE_FAMILY",kind,denied,schema:observer.pragma("user_version",{simple:true}),snapshotSha:hash(snapshot)}));
 observer.close();store.close();
}
const ann=new AnnIndexStore({dbPath:path.join(home,"ann.db")}),newer=new Database(ann.dbPath);newer.pragma("user_version=999");const beforeAnn=newer.serialize();assert.throws(()=>ann.begin({vectorIndexVariantId:"viv_"+"a".repeat(32),modelKey:"model",dimensions:2,chunkFingerprint:"f",vectorCount:1}),e=>e.code==="KNOWLEDGE_SCHEMA_NEWER");assert.deepEqual(newer.serialize(),beforeAnn);console.log(JSON.stringify({phase:"ANN_BEGIN",schema:999,sha:hash(beforeAnn),home}));newer.close();ann.close();

import { registerHooks, stripTypeScriptTypes, createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const root='/Users/study_superior/.codex/worktrees/r06-w12-review2/LingxiAgent';
const base='2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d';
const files=['lib/knowledge/vector-index-adapter.ts','lib/knowledge/knowledge-index-store.ts','lib/knowledge/ann-index-store.ts'];
if(process.env.REVIEW_BASELINE==='1') registerHooks({load(url,context,next){
  const file=files.find(p=>url==='file://'+root+'/'+p);
  if(file) return {format:'module',source:stripTypeScriptTypes(execFileSync('git',['show',base+':'+file],{cwd:root,encoding:'utf8'}),{mode:'transform'}),shortCircuit:true};
  return next(url,context);
}});
const {PortableVectorIndexAdapter}=await import('file://'+root+'/'+files[0]);
const {KnowledgeIndexStore}=await import('file://'+root+'/'+files[1]);
const {AnnIndexStore}=await import('file://'+root+'/'+files[2]);
const Database=createRequire(root+'/package.json')('better-sqlite3');
const records=[];
for(const [name,Cls] of [['vector',PortableVectorIndexAdapter],['fulltext',KnowledgeIndexStore],['ann',AnnIndexStore]]) {
  const dbPath=path.join(process.env.LINGXI_HOME,name+'.db');
  const writer=new Database(dbPath); writer.exec("CREATE TABLE sentinel(value TEXT);INSERT INTO sentinel VALUES('paid-original');PRAGMA user_version=999;");writer.close();
  const before=fs.readFileSync(dbPath); let error,opened;
  try {opened=new Cls({dbPath});} catch(e){error=e.message;}
  opened?.close();
  const probe=new Database(dbPath,{readonly:true});let sentinel;
  try{sentinel=probe.prepare('SELECT value FROM sentinel').get();}catch(e){sentinel='MISSING';}
  records.push({name,error:error??null,bytesSame:before.equals(fs.readFileSync(dbPath)),version:probe.pragma('user_version',{simple:true}),sentinel});probe.close();
}
const dbPath=path.join(process.env.LINGXI_HOME,'reset.db');const store=new KnowledgeIndexStore({dbPath});
const inode=fs.statSync(dbPath).ino;store.reset();const inodeSame=inode===fs.statSync(dbPath).ino;store.close();
console.log(JSON.stringify({baseMode:process.env.REVIEW_BASELINE==='1',home:process.env.LINGXI_HOME,records,inodeSame}));
assert(records.every(r=>r.error&&r.bytesSame&&r.version===999),'所有future库必须明确拒绝且保留字节与版本');
assert(inodeSame,'reset不得unlink活跃缓存');

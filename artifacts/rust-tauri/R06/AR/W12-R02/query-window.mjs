import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { PortableVectorIndexAdapter } from '/Users/study_superior/.codex/worktrees/r06-w12-review2/LingxiAgent/lib/knowledge/vector-index-adapter.ts';
const Database=createRequire('/Users/study_superior/.codex/worktrees/r06-w12-review2/LingxiAgent/package.json')('better-sqlite3');
for (const window of ['before-lock','after-version-check']) {
 const dbPath=path.join(process.env.LINGXI_HOME,window+'.db');let now='2020-01-01T00:00:00Z';
 const old=new PortableVectorIndexAdapter({dbPath,now:()=>now});
 const model={key:'model',provider:'local',modelId:'synthetic',protocol:'test',dimensions:2};
 old.buildOrReplaceArtifact({parseArtifactId:'artifact',chunkFingerprint:'fingerprint',model,entries:[{chunkId:'chunk',parseArtifactId:'artifact',ordinal:0,vector:[0.2,0.8]}]});
 const newer=new Database(dbPath);newer.pragma('busy_timeout=1');
 const before=newer.prepare('SELECT last_used_at FROM vector_index_variants').get();let attempted=false,writerError;
 if(window==='before-lock') {
  const transaction=old.db.transaction.bind(old.db);
  old.db.transaction=operation=>{const tx=transaction(operation);return {immediate:()=>{newer.pragma('user_version=999');attempted=true;return tx.immediate();}};};
 } else {
  const prepare=old.db.prepare.bind(old.db);
  old.db.prepare=sql=>{if(sql.includes('UPDATE vector_index_variants SET last_used_at')){attempted=true;try{newer.pragma('user_version=999');}catch(error){writerError=error.code;}}return prepare(sql);};
 }
 now='2040-01-01T00:00:00Z';let result,error;
 try{result=old.search({parseArtifactIds:['artifact'],model,queryVector:[0.2,0.8]});}catch(e){error={code:e.code,message:e.message};}
 const after=newer.prepare('SELECT last_used_at FROM vector_index_variants').get(),version=newer.pragma('user_version',{simple:true});
 console.log(JSON.stringify({window,home:process.env.LINGXI_HOME,attempted,before,after,version,writerError,result,error}));
 assert(attempted);
 if(window==='before-lock'){assert.equal(error.code,'KNOWLEDGE_SCHEMA_NEWER');assert.deepEqual(after,before);assert.equal(version,999);}
 else{assert.equal(writerError,'SQLITE_BUSY');assert.equal(version,3);assert.equal(result.length,1);assert.equal(after.last_used_at,now);}
 newer.close();old.close();
}

import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { PortableVectorIndexAdapter } from '/Users/study_superior/Desktop/Code/LingxiAgent/lib/knowledge/vector-index-adapter.ts';
const require=createRequire('/Users/study_superior/Desktop/Code/LingxiAgent/package.json');
const Database=require('better-sqlite3');
const dbPath=path.join(process.env.LINGXI_HOME,'query-race.db');
let now='2020-01-01T00:00:00Z';
const old=new PortableVectorIndexAdapter({dbPath,now:()=>now});
const model={key:'model',provider:'local',modelId:'synthetic',protocol:'test',dimensions:2};
old.buildOrReplaceArtifact({parseArtifactId:'artifact',chunkFingerprint:'fingerprint',model,entries:[{chunkId:'chunk',parseArtifactId:'artifact',ordinal:0,vector:[0.2,0.8]}]});
const newer=new Database(dbPath);
const before=newer.prepare('SELECT last_used_at FROM vector_index_variants').get();
const prepare=old.db.prepare.bind(old.db);
let crossed=false;
// 只添加确定性调度屏障：实际版本查询、UPDATE与数据库仍使用生产实现。
old.db.prepare=(sql)=>{
  if (sql.includes('UPDATE vector_index_variants SET last_used_at')) {
    newer.pragma('user_version=999');crossed=true;
  }
  return prepare(sql);
};
now='2040-01-01T00:00:00Z';
let result,error;
try { result=old.search({parseArtifactIds:['artifact'],model,queryVector:[0.2,0.8]}); }
catch(e){error={code:e.code,message:e.message};}
const after=newer.prepare('SELECT last_used_at FROM vector_index_variants').get();
console.log(JSON.stringify({home:process.env.LINGXI_HOME,crossed,before,after,version:newer.pragma('user_version',{simple:true}),result,error}));
newer.close();old.close();
assert.equal(crossed,true);
assert.deepEqual(after,before,'版本检查与写入之间升级后，旧句柄不得改写future库');

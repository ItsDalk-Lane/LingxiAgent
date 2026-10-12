import fs from 'node:fs';
import assert from 'node:assert/strict';
import YAML from '/Users/study_superior/Desktop/Code/LingxiAgent/node_modules/js-yaml/index.js';
import os from 'node:os';
import path from 'node:path';
import {loadConfig,getRawConfig,saveConfig,clearConfigCache} from '/Users/study_superior/Desktop/Code/LingxiAgent/lib/memory/config-loader.ts';
function typed(x){
 if(x instanceof Uint8Array)return {kind:'binary',hex:Buffer.from(x).toString('hex')};
 if(x instanceof Date)return {kind:'timestamp',iso:x.toISOString()};
 if(typeof x==='number'){if(!Number.isFinite(x))return {kind:'number',value:String(x)};if(Object.is(x,-0))return {kind:'number',value:'-0'};}
 if(Array.isArray(x))return x.map(typed);
 if(x&&typeof x==='object')return Object.fromEntries(Object.entries(x).map(([k,v])=>[k,typed(v)]));
 return x;
}
const file=process.argv[2]; const lines=fs.readFileSync(file,'utf8').split('\n');let rows=[];
for(const line of lines.filter(line=>line.startsWith('p03_roundtrip='))){
 const record=JSON.parse(line.substring('p03_roundtrip='.length));
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'lingxi-w06-incumbent-oracle-'));const config=path.join(dir,'config.yaml');
 let expected,after,incumbentSaved;
 try {
  fs.writeFileSync(config,record.before);clearConfigCache(config);loadConfig(config);
  saveConfig(config,{memory:{enabled:true},skills:{enabled:['中文 skill'],workspace_disabled:['project']}});loadConfig(config);expected=getRawConfig(config);incumbentSaved=fs.readFileSync(config,'utf8');
  fs.writeFileSync(config,record.after);clearConfigCache(config);loadConfig(config);after=getRawConfig(config);
 } finally {clearConfigCache(config);fs.rmSync(dir,{recursive:true});}
 const expectedSemantic=typed(expected),actual=typed(after);
 let status='PASS';try{assert.deepEqual(actual,expectedSemantic);}catch(e){status='FAIL';}
 rows.push({...record,incumbentSaved,expected:expectedSemantic,actual,status});
}
assert(rows.length>=5,'typed roundtrip records missing');
fs.writeFileSync(process.argv[3],JSON.stringify({node:process.version,js_yaml:'4.3.1',schema:'actual loadConfig/saveConfig with YAML.load/dump DEFAULT_SCHEMA; no custom schema',rows},null,2));
console.log(rows.map(r=>r.status));assert(rows.every(r=>r.status==='PASS'));

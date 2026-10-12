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
const source=JSON.parse(fs.readFileSync(process.argv[2],'utf8'));let rows=[];
for(const record of source.records){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'lingxi-r03-extra-oracle-'));const config=path.join(dir,'config.yaml');let expected,actual,readError;
 try {fs.writeFileSync(config,record.before);clearConfigCache(config);loadConfig(config);saveConfig(config,{memory:{enabled:true}});loadConfig(config);expected=getRawConfig(config);fs.writeFileSync(config,record.after);clearConfigCache(config);loadConfig(config);actual=getRawConfig(config);}catch(e){readError=String(e);}finally{clearConfigCache(config);fs.rmSync(dir,{recursive:true});}
 const expectedSemantic=typed(expected),actualSemantic=typed(actual);let status='PASS';
 if(record.expected_memory===null){if(!readError || record.actual_flags[0]!==400 || record.terminal[0]!=='failed' || record.provider_request.length!==0 || record.patch[0]!==400 || record.before!==record.after)status='FAIL';} else {try{assert.equal(record.patch[0],200);assert.deepEqual(actualSemantic,expectedSemantic);}catch(e){status='FAIL';}}
 rows.push({...record,readError,expected:expectedSemantic,actual:actualSemantic,status});
}
fs.writeFileSync(process.argv[3],JSON.stringify({node:process.version,schema:'original actual loadConfig/saveConfig/loadConfig DEFAULT_SCHEMA',rows},null,2));console.log(rows.map(r=>[r.name,r.status,r.readError]));assert(rows.every(r=>r.status==='PASS'));

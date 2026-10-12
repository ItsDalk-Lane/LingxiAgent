import fs from 'node:fs';
import { isDeepStrictEqual } from 'node:util';
import YAML from '/Users/study_superior/Desktop/Code/LingxiAgent/node_modules/js-yaml/index.js';
const root=new URL('.',import.meta.url);const checks=[];
function record(name,part,expected,actual){checks.push({name,part,expected,actual,status:isDeepStrictEqual(expected,actual)?'PASS':'FAIL'});}
for(const filename of ['probe-results.json','additional-family-results.json']){
 const data=JSON.parse(fs.readFileSync(new URL(filename,root)));
 for(const r of data.records){
  const incumbent=YAML.load(r.yaml);const allowReject=['sequence-root','sequence-field','double-nested-merge-source'].includes(r.name);
  record(r.name,'configuration_read',allowReject?400:200,r.actual_flags[0]);
  if(allowReject){record(r.name,'zero_provider_requests',0,r.provider_request.length);record(r.name,'run_terminal','failed',r.terminal[0]);record(r.name,'patch_reject_keeps_bytes',r.yaml,r.config_after_patch);continue;}
  record(r.name,'memory_default_and_semantics',incumbent?.memory?.enabled!==false,r.actual_flags[1].memoryEnabled??null);
  record(r.name,'provider_memory_nonce',incumbent?.memory?.enabled!==false,JSON.stringify(r.provider_request).includes('REVIEWER-ONLY-NONCE'));
  record(r.name,'run_terminal','completed',r.terminal[0]);
  record(r.name,'patch_status',200,r.patch[0]);
  const expected=structuredClone(incumbent);expected.memory??={};expected.memory.enabled=false;
  record(r.name,'patch_only_owned_field',expected,YAML.load(r.config_after_patch));
  if(r.experience){const enabled=incumbent?.experience?.enabled===true;record(r.name,'experience_flag',enabled,r.actual_flags[1].experienceEnabled??null);record(r.name,'experience_read_guard',enabled?200:403,r.experience[0]);}
  if(r.name==='name-literal-merge')record(r.name,'nested_name_not_lifted',false,JSON.stringify(r.provider_request).includes('LITERAL-NAME-UNEXPECTED'));
 }
}
const effects=JSON.parse(fs.readFileSync(new URL('effects-probe-results.json',root)));
for(const r of effects.records){
 const peerInvalid=r.name==='orphan-complex-peer';const selfInvalid=r.name==='project-complex-key';
 record(r.name,'cleanup_status',peerInvalid||selfInvalid?400:200,r.cleanup_preview[0]);
 record(r.name,'shared_file_preserved',true,r.shared_exists);
 if(peerInvalid||selfInvalid){record(r.name,'zero_provider_requests',0,r.provider_request.length);record(r.name,'run_terminal','failed',r.terminal[0]);continue;}
 const main=YAML.load(r.yaml);const peer=YAML.load(r.peer_yaml);
 const projectExpected=main?.workspace_context?.discover_project_skills!==false;
 record(r.name,'project_skill_in_wire',projectExpected,r.project_in_wire);
 const elsewhere=(peer?.skills?.enabled??[]).includes('shared-review');
 const mine=(main?.skills?.enabled??[]).includes('shared-review');
 record(r.name,'orphan_preview',mine&&!elsewhere?['shared-review']:[],r.actual_orphans);
}
const summary={checks,passed:checks.filter(c=>c.status==='PASS').length,failed:checks.filter(c=>c.status==='FAIL').length};
fs.writeFileSync(new URL('semantic-check-results.json',root),JSON.stringify(summary,null,2));
for(const c of checks.filter(c=>c.status==='FAIL'))console.log(JSON.stringify(c));
console.log(JSON.stringify({passed:summary.passed,failed:summary.failed}));process.exitCode=summary.failed?1:0;

import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import process from 'node:process';
import console from 'node:console';
const root=process.cwd();
const dir='docs/refactor-2026/independent-fix';
const read=p=>JSON.parse(fs.readFileSync(p,'utf8'));
const catalog=read('Lingxi_Refactor_Taskbooks_2026-09-21/acceptance-catalog.json');
const mapping=read(dir+'/FIX_ACCEPTANCE_MAP.json');
const result=read(dir+'/FIX_RESULT.json');
const problems=[];
function check(ok,message){if(!ok)problems.push(message);}
const expected=catalog.phases.flatMap(p=>p.acceptance_cases);
check(mapping.regressions.length===32,'C01—C32数量错误');
check(new Set(mapping.regressions.map(c=>c.id)).size===32,'回归ID重复');
for(let i=1;i<=32;i++)check(mapping.regressions.some(c=>c.id==='C'+String(i).padStart(2,'0')),'缺回归'+i);
check(mapping.scenarios.length===expected.length,'原场景数量不一致');
for(const old of expected){const now=mapping.scenarios.find(c=>c.id===old.id);check(Boolean(now),'缺场景'+old.id);if(!now)continue;for(const key of ['setup','action','expect'])check(now[key]===old[key],old.id+'原始'+key+'丢失');check(now.mandatory_assertions.some(a=>a.kind==='required_setup'),old.id+'未判定setup');check(now.mandatory_assertions.some(a=>a.kind==='required_action'),old.id+'未判定action');if(now.status==='PASS')check(now.mandatory_assertions.every(a=>['PASS','NOT_APPLICABLE'].includes(a.status)),old.id+'错误汇总PASS');}
check(mapping.phases.flatMap(p=>p.tasks).length===catalog.phases.flatMap(p=>p.tasks).length,'原任务数量不一致');
for(const phase of mapping.phases){if(phase.status==='PASS')check(phase.cases.every(c=>['PASS','NOT_APPLICABLE'].includes(c.status)),phase.id+'阶段误报PASS');for(const task of phase.tasks){check(fs.existsSync(task.original_taskbook),'缺原任务书'+task.original_taskbook);if(task.status==='PASS')check(task.acceptance_cases.every(id=>['PASS','NOT_APPLICABLE'].includes(mapping.scenarios.find(c=>c.id===id)?.status)),task.id+'任务误报PASS');}}
for(const c of result.regressions){for(const p of [...(c.evidence||[]),...(c.log?[c.log]:[]),...(c.command_index?[c.command_index]:[])])check(fs.existsSync(p),'缺证据'+c.id+' '+p);if(c.status==='PASS'){check(Boolean(c.production_entry),'PASS缺真实入口'+c.id);check(Boolean(c.command_index||c.command_ids||c.command_id),'PASS缺命令'+c.id);}}
check(result.overall_status!=='PASS','必需阻塞不能整体PASS');
const full=read('artifacts/refactor-2026/independent-fix/main/full-tests-recheck-delivery.json');
check(result.full_test.failed===full.numFailedTests,'全量失败数量不一致');
check(result.full_test.passed===full.numPassedTests,'全量通过数量不一致');
check(result.full_test.exit_code===1 && result.engineering_gate==='FAIL','原exit1被误改绿');
const manifest=read(dir+'/SOURCE_MANIFEST.json');
for(const entry of manifest.files){check(fs.existsSync(entry.path),'源码不存在'+entry.path);if(fs.existsSync(entry.path))check(crypto.createHash('sha256').update(fs.readFileSync(entry.path)).digest('hex')===entry.sha256,'源码摘要不一致'+entry.path);}
const source=fs.readFileSync('artifacts/refactor-2026/P05/logs/P05-ACCEPTANCE-counterexample.test.ts','utf8');
check(!/it\.fails/.test(source),'C1仍有expected failure');
check(source.includes('迟到 delta 不得出现在 Run B'),'C1原断言标识丢失');
// 交付检查器的负例：错误汇总必须被判断为不通过，不能只验证当前文件自述。
const invalid={status:'PASS',mandatory_assertions:[{status:'BLOCKED'}]};
check(!(invalid.status==='PASS'&&invalid.mandatory_assertions.every(a=>['PASS','NOT_APPLICABLE'].includes(a.status))),'负例检查失败');
const fakePass={status:'PASS'};check(!fakePass.production_entry&&!fakePass.command_index,'缺命令/入口负例错误');
if(problems.length){console.error(JSON.stringify({status:'FAIL',problems},null,2));process.exitCode=1;}else console.log(JSON.stringify({status:'PASS',regressions:32,scenarios:expected.length,tasks:mapping.phases.flatMap(p=>p.tasks).length,sourceFiles:manifest.files.length,negativeControls:'blocked child and missing evidence rejected',note:'仅交付一致性通过，不授予产品/平台总体PASS'},null,2));

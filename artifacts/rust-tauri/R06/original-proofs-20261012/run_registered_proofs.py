"""本轮只运行279原义务直接引用的唯一检查，以及上一轮已完整绑定的单叶。"""
import importlib.util,json,os,time
from pathlib import Path
root=Path.cwd();out=root/'artifacts/rust-tauri/R06/original-proofs-20261012'/os.environ.get('R06_ORIGINAL_VERIFICATION_SUBDIR','verification')
s=importlib.util.spec_from_file_location('r06_producer',root/'scripts/rust-tauri/r06_stage_inputs.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
inputs,graph=m.build(root)
assert json.loads((root/m.INPUT_PATH).read_text())==inputs and json.loads((root/m.MAP_PATH).read_text())==graph
selected=set(json.loads((out.parent/'selected-103-ids.json').read_text()));assert len(selected)==103
refs={key for collection in [inputs['leafBindings'],inputs['partialLeafBindings']] for leaf,groups in collection.items() if leaf in selected for group in groups for key in group['commandRefs']}
keys=sorted(refs,key=lambda k:(not inputs['executionBindings'][k].get('suite','').startswith('r06_original_'),inputs['executionBindings'][k].get('suite','')=='r06_original_knowledge_proofs',inputs['executionBindings'][k].get('package',''),inputs['executionBindings'][k].get('suite',''),inputs['executionBindings'][k].get('test','')))
os.environ['CARGO_TARGET_DIR']='/Users/study_superior/.codex/r06-w05-r01-runtime/target';os.environ['PATH']='/Users/study_superior/.cargo/bin:'+os.environ['PATH']
if out.exists():
 candidate=json.loads((out/'source-candidate.json').read_text());assert m.candidate_snapshot(root,out)['digestSha256']==candidate['digestSha256'],'只允许复用当前完全相同候选的真实记录'
else:
 out.mkdir(parents=True);candidate=m.candidate_snapshot(root,out);m.save(out/'source-candidate.json',candidate)
results=[];started=time.monotonic();new_failure=False
for i,key in enumerate(keys,1):
 binding=inputs['executionBindings'][key]
 if new_failure and not binding.get('suite','').startswith('r06_original_'):
  print('STOPPED_BEFORE_OLD_CHECKS_FOR_BATCHED_FIXTURE_REPAIR',flush=True);break
 if (out/'R06_BINDINGS'/key/'binding-result.json').exists():
  m.read_executed_binding(root,out/'R06_BINDINGS',key,binding,candidate);code=0;print('REUSED_CURRENT_VERIFIED_BINDING '+key,flush=True)
 else:
  code=m.execute_registered_binding(root,out/'R06_BINDINGS'/key,key,binding)
 record=json.loads((out/'R06_BINDINGS'/key/'binding-result.json').read_text());results.append({'key':key,'package':binding.get('package'),'suite':binding.get('suite'),'test':binding.get('test'),'exit':code,'passed':record['passed'],'resultPath':str(out/'R06_BINDINGS'/key/'binding-result.json')})
 m.save(out/'execution-index.json',{'selectedOriginalLeaves':sorted(selected),'total':len(keys),'executed':len(results),'passed':sum(r['passed'] for r in results),'failed':sum(not r['passed'] for r in results),'elapsedSecs':time.monotonic()-started,'results':results})
 if not record['passed']:
  print('FAILED '+binding.get('suite','')+'::'+binding.get('test',''),flush=True)
  for c in [record['command']]:
   for label in ['stdout','stderr']:
    if c.get(label):print(Path(c[label]).read_text(errors='replace')[-4000:],flush=True)
 print(f'PROGRESS {i}/{len(keys)} passed={sum(r["passed"] for r in results)}',flush=True)
 if m.candidate_snapshot(root,out)['digestSha256']!=candidate['digestSha256']:raise RuntimeError('源码在执行中变化，停止后续检查')
 if not record['passed']:
  if binding.get('suite','').startswith('r06_original_'):new_failure=True
  else:
   print('STOPPED_FOR_NECESSARY_REPAIR',flush=True);break
print('ALL_REGISTERED_PROOFS_EXECUTED' if len(results)==len(keys) else 'NOT_ALL_EXECUTED',flush=True)

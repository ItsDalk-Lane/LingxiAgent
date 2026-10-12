"""先验证上一轮真实失败的9项准备修正，成功记录供同一候选最终验证直接复用。"""
import importlib.util,json,os
from pathlib import Path

root=Path.cwd();base=root/'artifacts/rust-tauri/R06/original-proofs-20261012'
out=base/os.environ['R06_ORIGINAL_VERIFICATION_SUBDIR']
s=importlib.util.spec_from_file_location('r06',root/'scripts/rust-tauri/r06_stage_inputs.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
inputs,graph=m.build(root)
assert json.loads((root/m.INPUT_PATH).read_text())==inputs and json.loads((root/m.MAP_PATH).read_text())==graph
previous=json.loads((base/'verification-05/execution-index.json').read_text())
keys=[r['key'] for r in previous['results'] if not r['passed']];assert len(keys)==9
out.mkdir(parents=True,exist_ok=False);initial=m.candidate_snapshot(root,out);m.save(out/'source-candidate.json',initial)
os.environ['CARGO_TARGET_DIR']='/Users/study_superior/.codex/r06-w05-r01-runtime/target';os.environ['PATH']='/Users/study_superior/.cargo/bin:'+os.environ['PATH']
failures=[]
for key in keys:
 binding=inputs['executionBindings'][key]
 assert binding['suite'] in {'subagent_closeout','subagent_lifecycle','subagent_permission_inheritance'}
 m.execute_registered_binding(root,out/'R06_BINDINGS'/key,key,binding)
 record=json.loads((out/'R06_BINDINGS'/key/'binding-result.json').read_text())
 print('FAILED_FIXTURE_PREFLIGHT',binding['suite'],binding['test'],record['passed'],flush=True)
 if record['passed']:m.read_executed_binding(root,out/'R06_BINDINGS',key,binding,initial)
 else:
  failures.append(key)
  for label in ['stdout','stderr']:print(Path(record['command'][label]).read_text(errors='replace')[-6000:],flush=True)
 assert m.candidate_snapshot(root,out)['digestSha256']==initial['digestSha256'],'预检期间源码不得变化'
m.save(out/'fixture-preflight.json',{'total':len(keys),'passed':len(keys)-len(failures),'failed':len(failures),'keys':keys,'sourceCandidateSha256':initial['digestSha256']})
if failures:raise SystemExit(1)
print('ALL_NINE_PREFLIGHTS_PASSED_SAME_CANDIDATE_RECORDS_REUSABLE',flush=True)

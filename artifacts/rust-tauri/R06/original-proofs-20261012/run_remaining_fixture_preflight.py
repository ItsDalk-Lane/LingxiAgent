"""只先运行两个尚未通过的检查；成功记录可按同候选原规则被正式汇总复用。"""
import importlib.util,json,os
from pathlib import Path
root=Path.cwd();base=root/'artifacts/rust-tauri/R06/original-proofs-20261012';out=base/os.environ['R06_ORIGINAL_VERIFICATION_SUBDIR']
s=importlib.util.spec_from_file_location('r06',root/'scripts/rust-tauri/r06_stage_inputs.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
inputs,graph=m.build(root);assert json.loads((root/m.INPUT_PATH).read_text())==inputs and json.loads((root/m.MAP_PATH).read_text())==graph
os.environ['CARGO_TARGET_DIR']='/Users/study_superior/.codex/r06-w05-r01-runtime/target';os.environ['PATH']='/Users/study_superior/.cargo/bin:'+os.environ['PATH']
assert not out.exists();out.mkdir(parents=True);candidate=m.candidate_snapshot(root,out);m.save(out/'source-candidate.json',candidate)
names={'original_session_rename_preserves_title_after_invalid_foreign_and_storage_refusal','original_tools_use_authentic_run_scope_and_frozen_citable_source'}
chosen=[(key,binding) for key,binding in inputs['executionBindings'].items() if binding.get('test') in names];assert len(chosen)==2
for key,binding in chosen:
 code=m.execute_registered_binding(root,out/'R06_BINDINGS'/key,key,binding)
 if code:
  record=json.loads((out/'R06_BINDINGS'/key/'binding-result.json').read_text())['command']
  print(Path(record['stdout']).read_text(errors='replace')[-6000:],flush=True);print(Path(record['stderr']).read_text(errors='replace')[-3000:],flush=True);raise SystemExit(1)
 m.read_executed_binding(root,out/'R06_BINDINGS',key,binding,candidate)
 print('FIXTURE_PREFLIGHT_REAL_PASS '+binding['test'],flush=True)
print('BOTH_REMAINING_PREFLIGHTS_PASSED_NO_SOURCE_CHANGE',flush=True)

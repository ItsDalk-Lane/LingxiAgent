import importlib.util,json,os
from pathlib import Path
root=Path.cwd();v=root/'artifacts/rust-tauri/R06/original-proofs-20261012'/os.environ.get('R06_ORIGINAL_VERIFICATION_SUBDIR','verification')
s=importlib.util.spec_from_file_location('r06',root/'scripts/rust-tauri/r06_stage_inputs.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
os.environ['CARGO_TARGET_DIR']='/Users/study_superior/.codex/r06-w05-r01-runtime/target';os.environ['PATH']='/Users/study_superior/.cargo/bin:'+os.environ['PATH'];os.environ['R06_STAGE_INPUT_TEST_EVIDENCE']=str(v/'closing/negative-controls/raw')
initial=json.loads((v/'source-candidate.json').read_text());assert m.candidate_snapshot(root,v)['digestSha256']==initial['digestSha256'];specs=[
 ('negative-controls',{'kind':'script','argv':['python3','-B','scripts/rust-tauri/tests/test_r06_stage_inputs_contract.py'],'stdoutPattern':r'^OK$','timeoutSecs':120}),
 ('stage-map-contract',{'kind':'rust_unit_test','package':'xtask','suite':'xtask','test':'stage_map::map_tests::r06_registered_map_executes_checks_without_releasing_unproved_obligations','timeoutSecs':900}),
 ('input-consistency',{'kind':'script','argv':['python3','-B','scripts/rust-tauri/r06_stage_inputs.py','--check','--evidence',str(v/'R06_INPUTS')],'stdoutPattern':r'^$','timeoutSecs':120})]
results=[]
for label,binding in specs:
 if binding['kind']!='script':binding['argv']=m.command_argv(root,binding)
 p=v/'closing'/label;p.mkdir(parents=True,exist_ok=False)
 record=m.execute_binding(root,p,binding,1,binding['timeoutSecs'],initial);results.append({'label':label,**record});m.save(v/'closing-results.json',{'checks':results})
 print(label,record['localPassed'],flush=True)
 if not record['localPassed']:
  print(Path(record['stdout']).read_text(errors='replace')[-6000:],flush=True);print(Path(record['stderr']).read_text(errors='replace')[-6000:],flush=True);raise SystemExit(1)
actual=json.loads((v/'R06_INPUTS/inputs.json').read_text());inputs,_=m.build(root);assert actual['inputSnapshot']==inputs and actual['inputsDigestSha256']==m.sha(inputs)
print('NECESSARY_CLOSING_CHECKS_PASSED',flush=True)

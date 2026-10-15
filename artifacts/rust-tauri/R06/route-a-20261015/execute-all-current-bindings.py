"""执行当前登记全集；只编排原命令，不代替正式阶段验收。"""
import argparse,json,os,subprocess,sys
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--root',type=Path,required=True)
p.add_argument('--evidence',type=Path,required=True)
a=p.parse_args(); root=a.root.resolve(); evidence=a.evidence.absolute()
if evidence.exists():raise ValueError('证据目录必须全新，不复用旧结果')
inputs=json.loads((root/'rust/crates/xtask/src/stage_maps/R06_INPUTS.json').read_text())
script=root/'scripts/rust-tauri/r06_stage_inputs.py'
failures=[]
executed=[]
interruption=None
for index,(key,binding) in enumerate(sorted(inputs['executionBindings'].items()),1):
 env=os.environ.copy()
 if binding['kind']=='rust_desktop_test':
  env['CARGO_TARGET_DIR']=env.get('LINGXI_SKILLS_NATIVE_TARGET',str(Path.home()/'.cache/lingxi-r06-builds/tauri-resources'))
 try:
  result=subprocess.run([sys.executable,'-B',str(script),'--binding',key,'--evidence',str(evidence/'R06_BINDINGS'/key)],cwd=root,env=env,timeout=binding['timeoutSecs']+180)
 except (subprocess.TimeoutExpired,OSError) as error:
  failures.append(key);interruption={'key':key,'reason':str(error)};break
 executed.append(key)
 print(f'[{index}/{len(inputs["executionBindings"])}] {key}: exit={result.returncode}',flush=True)
 if result.returncode:failures.append(key)
rollup=subprocess.run([sys.executable,'-B',str(script),'--leaf-evidence','--binding-evidence',str(evidence/'R06_BINDINGS'),'--evidence',str(evidence/'R06_LEAVES')],cwd=root,timeout=180)
summary={'scope':'ALL_CURRENT_REGISTERED_BINDINGS_NOT_STAGE_ACCEPTANCE','registeredBindingsPlanned':len(inputs['executionBindings']),'registeredBindingsExecuted':len(executed),'interruption':interruption,'failedBindings':failures,'rollupExitCode':rollup.returncode,'stageAccepted':False,'R07Ready':False}
(evidence/'window-summary.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n')
print(json.dumps(summary,ensure_ascii=False),flush=True)
sys.exit(1 if failures or rollup.returncode else 0)

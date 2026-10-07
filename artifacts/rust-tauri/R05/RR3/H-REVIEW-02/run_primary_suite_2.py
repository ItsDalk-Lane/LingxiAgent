from capture import *
import threading,time,shutil
cargo='/Users/study_superior/.cargo/bin/cargo'
def check(label,args,extra=None):
 code=run(label,args,extra);assert code==0,(label,code)
# 使用独立的最后 rustc 参数先使 Cargo 确定重编当前源，再恢复正式默认构建参数。
# 不触碰主源码字节/时间戳，不清空任何历史缓存。
check('primary-default-build-2',[cargo,'build','--manifest-path','rust/Cargo.toml','--locked','-p','lingxi-service','--message-format=json'])
check('redaction-final-2',[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-p','lingxi-service','--lib','redaction::tests','--','--nocapture'])
check('logging-final',[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-p','lingxi-service','--lib','logging::tests','--','--nocapture'])
check('ordinary-cancel-recovery',[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-p','lingxi-service','--test','subagent_closeout','parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap','--','--exact','--nocapture'])
check('late-result-next-run',[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-p','lingxi-service','--test','late_result_fence','r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing','--','--exact','--nocapture'])
# 所有有效资源检查随后绑定实际Cargo生成对象，防止把上次同名文件当作这次产物。
check('resources-equipment-build',[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-p','lingxi-service','--test','r05_t08_resources','--no-run','--message-format=json'])
label='resources-final';codes=[]
def workload():codes.append(run(label,[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-p','lingxi-service','--test','r05_t08_resources','--','--nocapture','--test-threads=1'],{'TMPDIR':str(EV/'resource-tmp')}))
t=threading.Thread(target=workload);t.start();bound=False
while t.is_alive():
 time.sleep(1)
 if bound:continue
 out=EV/'commands'/label
 if not out.exists():continue
 probe=subprocess.run(['python3',str(EV/'runtime_snapshot.py'),label],cwd=ROOT,capture_output=True,text=True)
 if probe.returncode==0 and (out/'runtime-binding.json').exists():
  rows=json.loads((out/'runtime-binding.json').read_text())['rows']
  bound=bool(rows) and all('configSha256' in r for r in rows)
t.join();assert codes==[0],codes;assert bound,'未绑定运行中配置不能宣称完整测量'
check('resources-independent-analysis',['python3',str(EV/'analyze_resources.py'),label])
raw=json.loads((EV/'commands'/label/'f27-resource-series.json').read_text());equipment=pathlib.Path(raw['environment']['equipmentPath']);worker=pathlib.Path(raw['environment']['workerPath'])
write(EV/'resource-equipment-binding.json',{'equipment':str(equipment),'equipmentSha256':sha(equipment),'worker':str(worker),'workerSha256':sha(worker),'service':str(ROOT/'rust/target/debug/lingxi-service'),'serviceSha256':sha(ROOT/'rust/target/debug/lingxi-service'),'sourceInputs':snapshot()})
check('sampler-negatives',['python3',str(EV/'sampler_negative_probe.py'),'sampler-negative-final',str(equipment)])
check('a01-final-both-levels',['python3',str(ROOT/'scripts/rust-tauri/r02_t01_log_level_regression.py'),str(EV/'commands/a01-final-both-levels/levels')])
check('a13-final',['bash',str(ROOT/'scripts/rust-tauri/r02_t07_redaction_scan.sh'),str(EV/'commands/a13-final/A13')])
check('live-correlation-final',['python3',str(EV/'correlation_probe.py'),'live-correlation-final'])
check('f46-final-six-command',['python3',str(EV/'binary_probe.py'),'f46-final-six',str(ROOT/'rust/target/debug/lingxi-service'),'six'])
check('f46-explicit-error',['python3',str(EV/'binary_probe.py'),'f46-explicit-error',str(ROOT/'rust/target/debug/lingxi-service'),'error'])
print('主树实际对象全部指定检查完成',flush=True)

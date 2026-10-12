import os,sys,pathlib,json,hashlib,shutil,subprocess,datetime,tarfile

b=pathlib.Path(__file__).parent;r=b/'own.git';a=r/'artifacts/rust-tauri/R06/AR/W15-EXECUTOR-01'
current_window='final-safe-freeze'
def sha(p):
    h=hashlib.sha256()
    with p.open('rb') as f:
        for chunk in iter(lambda:f.read(1024*1024),b''):h.update(chunk)
    return h.hexdigest()
def save(p,v):p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
def load(p):return json.loads(p.read_text())
def entry(p,path=None):return dict(path=path or str(p),sha256=sha(p),size_bytes=p.stat().st_size)

# 封存只复制白名单：源码、只读原书、安全合成日志；不复制home/DB/凭证/target。
assert not (a/'END.json').exists()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=r,text=True).strip()=='bc2ce51fd005ea75b9949156974e15d2761eb734'
subprocess.run(['git','diff','--check'],cwd=r,check=True)
stable=b/'evidence/final-stable-direct-be-battery'
run=load(stable/'run.json');assert run['exit_code']==0 and not run['guard_breaches']
before={x['path']:x['sha256'] for x in load(stable/'binaries-before.json')}
after={x['path']:x['sha256'] for x in load(stable/'binaries-after.json')}
business=[x for x in run['processes'] if not x['controller']]
for p in business:assert before[p['actual_executable']]==after[p['actual_executable']]
assert load(stable/'source-before.json')==load(stable/'source-after.json')
assert load(b/'evidence/final-freeze-direct-audit/run.json')['exit_code']==0

source=load(b/'evidence/final-freeze-direct-audit/source-after.json')
for x in source:assert sha(r/x['path'])==x['sha256']
scope=load(a/'SCOPE_RECONCILIATION.json')
for p,h in scope['candidate_sources'].items():assert sha(r/p)==h
save(a/'frozen-source-manifest.json',dict(head=scope['head'],candidate_sources=scope['candidate_sources'],source_family='全Git tracked rust/core/lib/shared/server/scripts/desktop/tests/skills2set/cli + root files + new audit',files=source,final_test_window='final-stable-direct-be-battery',test_and_service_before_after=[dict(pid=p['pid'],path=p['actual_executable'],before_sha256=before[p['actual_executable']],after_sha256=after[p['actual_executable']]) for p in business],source_before_after_equal=True,limitations='此清单是源码封存，不把旧窗未采到执行路径补认成已测量。'))
# 完整源码仅留独立本地clone；SAFE只复制实际改动的两个候选源码。
for name in scope['candidate_sources']:
    dest=a/'candidate-source'/name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(r/name,dest)
inputs=load(b/'input-copy-manifest.json');input_paths={x['path']:x for x in inputs}
for x in inputs:assert sha(r/x['path'])==x['sha256']
for x in scope['historical_reviews']:
    assert sha(r/x['source'])==x['sha256']
    input_paths[x['source']]=dict(path=x['source'],sha256=x['sha256'],read_only_input=True)
report='docs/rust-tauri/R06/repair-current/W15_EXECUTOR_01_REPORT.md'
input_paths[report]=dict(path=report,sha256=sha(r/report),read_only_input=False,owned_handoff=True)
save(a/'frozen-inputs-manifest.json',list(input_paths.values()))
shutil.copy2(r/report,a/'HANDOFF.md')

# 记录每个实际采样PID的闭合事实；未直接取得wait状态时保留null。
ps=subprocess.check_output(['ps','-axo','pid=,ppid=,args='],text=True)
live={}
for line in ps.splitlines():
    fields=line.strip().split(None,2)
    if len(fields)==3:live[int(fields[0])]=dict(ppid=int(fields[1]),argv=fields[2])
own_active=[dict(pid=pid,**v) for pid,v in live.items() if v['argv'].startswith(str(b/'target/'))]
assert not own_active,own_active
closures=[];gaps=[];windows=[]
for w in sorted((b/'evidence').iterdir()):
    if not w.is_dir() or w.name==current_window:continue
    rp=w/'run.json'
    if not rp.exists():continue
    q=load(rp);windows.append(dict(window=w.name,exit_code=q.get('exit_code'),elapsed_seconds=q.get('elapsed_seconds'),process_count=len(q.get('processes',[])),guard_breaches=q.get('guard_breaches'),run_sha256=sha(rp)))
    raw=w/'raw/facts.jsonl';ready={};exits={}
    if raw.exists():
        for line in raw.read_text().splitlines():
            try:fact=json.loads(line)
            except json.JSONDecodeError:continue
            v=fact.get('value',{})
            if fact.get('kind')=='service_ready' and 'pid' in v:ready[v['pid']]=v
            if fact.get('kind')=='service_exit' and 'pid' in v:exits[v['pid']]=v
    nullable={x['path'] for x in q.get('actual_executables_after',[]) if x['sha256'] is None}
    for p in q.get('processes',[]):
        pid=p['pid'];alive=live.get(pid);same_owned=alive is not None and str(b) in alive['argv']
        assert not same_owned,dict(window=w.name,pid=pid,current=alive)
        closures.append(dict(window=w.name,pid=pid,recorded_argv=p.get('argv'),actual_executable=p.get('actual_executable'),closure='NOT_PRESENT_AT_FINAL_CENSUS' if alive is None else 'PID_PRESENT_WITH_DIFFERENT_IDENTITY_NOT_OUR_PROCESS',direct_service_exit=exits.get(pid),individual_exit_code=exits.get(pid,{}).get('exitCode'),sampled_peak_rss_bytes=p.get('peak_rss_bytes'),first_seen_elapsed=p.get('first_seen_elapsed'),last_seen_elapsed=p.get('last_seen_elapsed'),note='消失观察不冒充独立wait的退出码；未采值保持null。'))
        if p.get('actual_executable') in nullable:
            binding=ready.get(pid)
            exe=p['actual_executable']
            system=exe.strip('()') in ('bash','git','lsof','nc','pgrep','ps','sandbox-exec','sh','sleep','cat')
            gaps.append(dict(window=w.name,pid=pid,recorded_argv=p.get('argv'),actual_executable=exe,original_executable_after_sha256=None,same_pid_service_ready=binding,classification='SAME_PID_RAW_READY_BOUND_BUT_ORIGINAL_NULL_PRESERVED' if binding else 'SHORT_SYSTEM_PATH_OR_ZOMBIE_UNMEASURED' if system else 'ACTUAL_BUSINESS_EXECUTABLE_PATH_NOT_CAPTURED',execution_before_sha256=None,execution_after_sha256=None,note='库存SHA或当前文件不能追認本PID执行前测量；原window受此采样限制。'))
save(a/'PID_CLOSURE.json',dict(checked_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),owned_active_test_service=[],records=closures,unobserved_short_pid_windows=['old-red-null','original-build INVALID supervision'],freeze_controller=dict(pid=os.getpid(),status='IN_FLIGHT_HERE_FINAL_TERMINAL_IS_EXTERNAL_COMPANION_RUN',run=str(b/'evidence'/current_window/'run.json')),notes='所有业务/测试/旧scratch采样PID闭合观察；封存脚本本身受v3外部监督，最终exit由工具完成后 companion run.json证明。'))
save(a/'SAMPLING_AUDIT.json',dict(nullable_executable_pid_entries=gaps,nullable_entries=len(gaps),final_direct_window=dict(window=stable.name,process_entries=len(run['processes']),executable_after_nulls=[x for x in run['actual_executables_after'] if x['sha256'] is None],business_before_after_equal=True,programs=load(a/'frozen-source-manifest.json')['test_and_service_before_after']),old_windows_limitations='原窗口null不可覆盖；旧NULL红窗testbinary漏采；cargo重建final-stable的before-after不同，不用作最终程序一致绑定。'))
save(a/'WINDOW_INDEX.json',windows)

local=[]
for directory in ['home','tmp','scope28-home','persona-next-run-home','persona-next-run-workspace','resource-probe-home','resource-probe-workspace','resource-success-probe-home','resource-success-probe-workspace']:
    d=b/directory
    if d.exists():
        for p in sorted(d.rglob('*')):
            if p.is_file() and not p.is_symlink():local.append(dict(**entry(p),classification='LOCAL_ONLY_SYNTHETIC_EFFECTIVE_INPUT_OR_OUTPUT',transferred=False))
for p in b.glob('*service.json'):local.append(dict(**entry(p),classification='LOCAL_ONLY_SYNTHETIC_CONFIG',transferred=False))
save(a/'LOCAL_ONLY_INPUTS.json',dict(files=local,binary_inventory='所有window binaries-before/after及actual executable records；实际bin不传入safe包',excluded=['synthetic home/config/credential/DB/fixture contents','target','own.git/.git','node_modules','main dirty product','.vscode'],read_only_preserved=True))
untracked=subprocess.check_output(['git','ls-files','--others','--exclude-standard','-z'],cwd=r).decode().split('\0')
save(a/'OWNED_WORKING_TREE.json',dict(tracked_status=subprocess.check_output(['git','status','--porcelain'],cwd=r,text=True).splitlines(),untracked_nonignored_files=[entry(r/n,n) for n in untracked if n and (r/n).is_file() and not str(r/n).startswith(str(a))],owned_generated_artifact_tree='safe-evidence-manifest enumerates all owned artifacts separately',ignored_readonly_inputs='frozen-inputs-manifest',ignored_runtime_inputs='LOCAL_ONLY_INPUTS',no_main_directory_writes=True,no_commit_or_push=True))
patch=subprocess.check_output(['git','diff','--binary','HEAD','--','rust/crates/lingxi-service/tests/r06_t08_behavior_eval.rs'],cwd=r)
extra=subprocess.run(['git','diff','--no-index','--binary','/dev/null','scripts/rust-tauri/r06_evaluation_audit.py'],cwd=r,stdout=subprocess.PIPE)
assert extra.returncode==1
(a/'candidate.patch').write_bytes(patch+extra.stdout)

for w in sorted((b/'evidence').iterdir()):
    if w.name==current_window:continue
    dest=a/'evidence'/w.name
    if w.is_dir():shutil.copytree(w,dest)
    elif w.is_file():dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(w,dest)
for p in sorted(b.glob('*.py')):shutil.copy2(p,a/'executor-tools'/p.name) if (a/'executor-tools').exists() else ((a/'executor-tools').mkdir(),shutil.copy2(p,a/'executor-tools'/p.name))
for name in ['workspace.json','input-copy-manifest.json','candidate-before-mutants.rs']:shutil.copy2(b/name,a/'executor-tools'/name)
perf=list((b/'tmp').glob('lingxi-r05f27-f27-series-*/f27-resource-series.json'))
for p in perf:shutil.copy2(p,a/'evidence'/('actual-'+p.parent.name+'-'+p.name))

files=[]
for p in sorted(a.rglob('*')):
    if p.is_file():files.append(dict(**entry(p,str(p.relative_to(a))),safety='SAFE_SOURCE_READONLY_ORIGINAL_OR_SYNTHETIC_RAW_NO_REAL_USER_DATA'))
save(a/'safe-evidence-manifest.json',dict(schema='lingxi.r06.w15.safe-freeze.v1',files=files,excluded_local_inputs_manifest='LOCAL_ONLY_INPUTS.json',all_previous_failed_windows_preserved=True,actual_binary_transfer=False,current_freeze_controller_terminal_external=str(b/'evidence'/current_window/'run.json'),self_manifest_excluded=True))
end=dict(status='READY_FOR_INDEPENDENT_REVIEW',candidate_head=scope['head'],candidate_sources=scope['candidate_sources'],safe_manifest_sha256=sha(a/'safe-evidence-manifest.json'),frozen_source_manifest_sha256=sha(a/'frozen-source-manifest.json'),full_source_and_inputs='LOCAL_ONLY ownclone, no duplicate archives in SAFE; original files referenced by SHA',original_register_sha256=sha(r/'docs/rust-tauri/R06/repair-current/AR_ORIGINAL_OBLIGATIONS.json'),staged_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),final_test_window=stable.name,final_audit_window='final-freeze-direct-audit',formal_gate='NOT_RUN',independent_review='NOT_RUN',current_stage='R06_NOT_ACCEPTED',freeze_valid_only_after_external_controller_exit_zero=str(b/'evidence'/current_window/'run.json'),no_commit_push=True)
save(a/'END.json',end)
print(json.dumps(dict(artifact=str(a),safe_files=len(files),source_files=len(source),local_only_files=len(local),sampled_pid_records=len(closures),sampling_gap_records=len(gaps),end=end),ensure_ascii=False))

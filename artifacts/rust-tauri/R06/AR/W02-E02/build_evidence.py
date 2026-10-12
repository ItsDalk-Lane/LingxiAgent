import pathlib,json,hashlib,datetime,re,subprocess
root=pathlib.Path.cwd();p=root/'artifacts/rust-tauri/R06/AR/W02-E02'
def sha(f):return hashlib.sha256(f.read_bytes()).hexdigest()
def save(name,x): (p/name).write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
leaves=json.loads((root/'docs/rust-tauri/R06/R06_LEAF_MAP.json').read_text())['tasks']
rows=[]
for task in ['R06-T03','R06-T05']:
 k='t03' if task.endswith('03') else 't05'
 for leaf in leaves[task]['leaves']:
  row=dict(leaf);row['task']=task;row['expected']=leaf['acceptance_assertions'];row['actual']='原断言及原锚点完整保留；本轮只核对涉及文件恢复及消费者的服务范围。关联套件运行不等于本叶全部份额已验收。';row['status']='INVENTORIED_NOT_INDIVIDUALLY_RUN';row['runtime_evidence']=['attempts/final-attempt5/t03-t05-regression.log','attempts/final-attempt5/adapter-store-regression.log'];row['unverified']='原 '+str(leaf.get(k+'_status'))+' 份额/延后子句未由本轮总绿升级；桌面、LIVE及其它平台NOT_RUN。';rows.append(row)
save('original-leaf-coverage.json',{'task_memberships':len(rows),'unique_features':len(set(a['feature_id'] for a in rows)),'leaf_assertion_count':sum(len(a['expected']) for a in rows),'note':'131 Task成员逐条保留原R00断言，不自签131项PASS。','rows':rows})
obs=json.loads((root/'docs/rust-tauri/R06/repair-current/AR_ORIGINAL_OBLIGATIONS.json').read_text())['obligations']
selected=[]
for o in obs:
 if not any(t in ['R06-T03','R06-T05'] for t in o.get('task_ids',[])):continue
 row=dict(o);row['inherited_status']=row.pop('status',None);row['w02_status']='INVENTORIED_ORIGINAL_SCOPE_PRESERVED_NOT_INDIVIDUALLY_RUN';row['w02_independent_review']='NOT_RUN';row['w02_runtime_evidence']=[]
 if o['kind'] in ['original_task_step','original_deliverable','original_base_acceptance']:
  row['w02_runtime_evidence']=['attempts/final-attempt5/t03-t05-regression.log','attempts/final-attempt5/adapter-store-regression.log'];row['w02_status']='RELATED_LOCAL_REGRESSION_EXECUTED';row['actual']='真实本地生产链及存储套件已跑；远端协议成功/LIVE、桌面消费、其它平台及旧报告已顺延项未升级。'
 selected.append(row)
save('original-obligations-coverage.json',{'note':'每条保留原义务、原继承状态，不修改总控文档。单独Steps/A矩阵见REPORT。','rows':selected})
# 候选只含本执行代理拥有的15文件；不读取无关用户资料。
files=json.loads((p/'owned-files.json').read_text());frozen=json.loads((p/'own-inputs-pre-final.json').read_text());current=[dict(path=f,sha256=sha(root/f),size=(root/f).stat().st_size) for f in files]
assert current==frozen['files'],'冻结后候选发生变化，需要重新验证'
head=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip();branch=subprocess.check_output(['git','branch','--show-current'],text=True).strip()
save('candidate-summary.json',{'time':datetime.datetime.now(datetime.timezone.utc).isoformat(),'head':head,'branch':branch,'files':current,'aggregate_sha256':hashlib.sha256(json.dumps(current,sort_keys=True).encode()).hexdigest(),'freeze_unchanged':True,'protected_settings_sha256':sha(root/'.vscode/settings.json'),'isolated_review_dependency':'15文件可与baseline构造者兼容；当前运行是含W06/W12未独立通过更改的共享树组合。独立review仅复制15files到2e基线，不复制W12。','toolchain':subprocess.check_output(['/Users/study_superior/.cargo/bin/cargo','--version'],text=True).strip()})
tracked=[];new=[]
for f in files:
 code=subprocess.run(['git','ls-files','--error-unmatch',f],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode
 (tracked if code==0 else new).append(f)
patch=subprocess.check_output(['git','diff','--']+tracked)
for f in new:patch+=subprocess.run(['git','diff','--no-index','--','/dev/null',f],stdout=subprocess.PIPE).stdout
(p/'candidate.diff').write_bytes(patch)
# 最后一次每命令日志只在运行完成后归档一次；拒绝未跑完候选。
final=json.loads((p/'command-results.json').read_text());assert len(final)==6 and all(x['exit_code']==0 for x in final)
a=p/'attempts/final-attempt5';a.mkdir(parents=True,exist_ok=False)
for r in final:
 f=p/r['log'];assert sha(f)==r['evidence_hash'];f.rename(a/f.name);r['attempt']='final-attempt5';r['log']='attempts/final-attempt5/'+f.name
(p/'command-results.json').rename(a/'raw-command-results.json');save('final-command-results.json',final)
# 原始历史json不重写；另存实际唯一日志路径映射和SHA核对。
history=[]
for f in sorted(p.glob('*.log')):
 txt=f.read_text(errors='replace');counts=re.findall(r'test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored',txt)
 history.append({'log':f.name,'sha256':sha(f),'test_summaries':counts,'raw_command_available':txt.startswith('COMMAND '),'timing_available':txt.startswith('COMMAND '),'note':'非最终候选证据；0测试编译失败不得记行为红。早期单命令未在原日志写start/end，不伪造；runner日志由相应原json记录。'})
resolved=[]
for f in sorted(p.glob('runner-attempt*-command-results.json')):
 attempt=f.name.split('-command-results')[0]
 for r in json.loads(f.read_text()):
  target=p/(attempt+'-'+r['log']);resolved.append({'raw_record':f.name,'actual_log':target.name,'hash_matches':target.exists() and sha(target)==r['evidence_hash'],'command_record':r})
save('failure-history.json',{'logs':history,'runner_record_resolution':resolved,'missing_early_metadata':'早期原始日志未录绝对start/end或完整argv；保留原日志，不填造时间。old-red为1项真实行为失败，其它编译失败标0测试。'})

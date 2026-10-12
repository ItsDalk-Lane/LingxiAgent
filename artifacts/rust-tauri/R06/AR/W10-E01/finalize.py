from pathlib import Path
import json, hashlib, datetime, subprocess, re, os
root=Path(__file__).resolve().parents[5]; ev=Path(__file__).resolve().parent
now=lambda:datetime.datetime.now(datetime.timezone.utc).isoformat()
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
read=lambda p:json.loads(p.read_text())
def write(name,data): (ev/name).write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
freeze=read(ev/'owned-freeze-before-formal.json')
assert all(sha(root/x['path'])==x['sha256'] for x in freeze['owned'])
assert sha(root/'rust/Cargo.lock')==freeze['lock_sha256']
for label in ['affected-old-service-final-02','affected-old-adapters-final-02','final-nine-cases-13']:
 d=read(ev/'commands'/label/'record.json');assert d['exit']==0 and d['source_before']==d['source_after']
formal=[]
for f in sorted((ev/'verify-R05-final-01').rglob('verify-stage-result.json')):
 d=read(f);formal.append({'path':str(f.relative_to(ev)),'sha256':sha(f),'stage':d['stage'],'overall':d['overall'],'commands':d['commands'],'source_binding_stable':d['candidateSourceBinding']['stable'],'runner_binding_status':d['runnerSourceBinding']['status'],'startedAtUnixMs':d['startedAtUnixMs'],'finishedAtUnixMs':d['finishedAtUnixMs'],'candidate_digest':d['candidateSourceBinding']['before']['digestSha256'],'scenario_statuses':[{k:x.get(k) for k in ['id','status','commandRefs']} for x in d['scenarios']]})
write('formal-gates.json',{'at':now(),'execution_completed':True,'outer_exit':read(ev/'commands/verify-r05-formal-01/record.json')['exit'],'outer_guard_600_applied':False,'test_guard':{'enabled':True,'time_limit_sec':600,'memory_limit_mb':8192,'memory_measured_mb':None},'stages':formal,'r02_storage':'FAIL registry9vs7; business later legs NOT_RUN','r02_legacy':'BLOCKED_ENVIRONMENT .git directory required; business isolated copy NOT_RUN','workspace':'Actual 101 at original quota fixture, later targets NOT_RUN in each formal workspace. Earlier no-fail-fast 4 targets/5 cases FAIL retained STALE.','r06_gate':'NOT_REGISTERED / F14 OPEN','after_run_append_boundary':'This handoff adds only reports/receipts after stage ended. Whole-worktree stage binding describes the then-frozen inputs; final exact owned source stays identical. No stage PASS or acceptance is inherited.'})
d=read(ev/'self-check.json');d.update(status='READY_FOR_INDEPENDENT_REVIEW',issue_status='NOT_COMPLETED',completion='BLOCKED_DEPENDENCY_F09',independent_acceptance=False,stage_status='R05_R04_R03_ACTUAL_FAIL');d['required_next']='Fresh independent W10 Reviewer must actually review current candidate, including C04 missing running recovery; F09 may then repair/rebind, no R07 deferral.';d['final_affected_regressions']=['commands/affected-old-service-final-02/record.json','commands/affected-old-adapters-final-02/record.json'];write('self-check.json',d)
records=[]
prep={'registry-baseline','positive-preparation-01','positive-preparation-02','c01-c04-first-01','c01-c04-second-02','eight-cases-04','crash-preparation-05','nine-cases-06','nine-cases-07'}
for f in sorted((ev/'commands').glob('*/record.json')):
 d=read(f);label=f.parent.name;b=d['source_before'];a=d['source_after'];changes=[k for k in sorted(b.keys()|a.keys()) if b.get(k)!=a.get(k)]
 category=('PREPARATION_FAILURE_NOT_BUSINESS_RED' if label in prep else 'EXPECTED_BUSINESS_RED' if label in ['c02-old-red-01','c02-mutation-red-02'] else 'REGISTRY_F14_ACTUAL_FAIL' if label in ['registry-baseline-02','registry-final-01'] else 'CLIPPY_OWN_TEST_FAILURE_FIXED_LATER' if label=='clippy-final-01' else 'ACTUAL_OLD_TEST_FAIL_NOT_ACCEPTED' if label in ['workspace-final-01','quota-failure-diagnostic-01'] else 'FORMAL_STAGE_ACTUAL_FAIL' if label=='verify-r05-formal-01' else 'ACTUAL_COMMAND_EXIT_ZERO')
 if changes:category+=' / STALE_SOURCE_CHANGED_DURING_RUN'
 if label=='affected-old-service-01':category+=' / SUPERSEDED_FINAL_BOUND_02'
 if label=='affected-old-adapters-01':category+=' / SUPERSEDED_FINAL_BOUND_02'
 records.append({'label':label,'record':str(f.relative_to(ev)),'record_sha256':sha(f),'cwd':d['cwd'],'argv':d['argv'],'start':d['start'],'end':d['end'],'exit':d['exit'],'category':category,'source_before_equal_after':not changes,'changed_paths':changes,'owned_after_equal_final':{x['path']:a.get(x['path'])==x['sha256'] for x in freeze['owned']},'source_receipt_contains_full_input_sha':True,'executed_binaries':d['executed_binaries'],'binary_binding_limit':'Actual paths selected from Running lines; SHA taken after command. Earlier script/quiet runner lacks a Running binary line, not a runtime-start binary attestation. Formal nested inventory is recorded separately.','memory_measured_mb':None,'fixture_home':d['environment']['LINGXI_HOME'],'cleanup':d['fixture_cleanup']})
write('command-history.json',{'at':now(),'records':records,'rule':'No failed/preparation/intermediate record replaced or erased; command exit0 is not full Task acceptance.'})
# 真实日志证明启动路径；源冻结、target独占，散列在运行结束后采集，不伪称启动前采样。
bins={}
for f in (ev/'verify-R05-final-01').rglob('*'):
 if not f.is_file() or f.suffix not in ['.log','.out','.err']:continue
 try:t=f.read_text()
 except UnicodeDecodeError:continue
 for m in re.finditer(r'Running .*?\(([^)\n]+)\)',t):
  p=Path(m.group(1));p=p if p.is_absolute() else root/p
  if p.is_file() and str(p).startswith(str(root/'rust/target')):
   bins[str(p)]={'path':str(p),'sha256_post_execution':sha(p),'log':str(f.relative_to(ev))}
write('formal-binaries.json',{'at':now(),'executed_path_from_original_log':list(bins.values()),'boundary':'Post-execution hash of actual logged paths in exclusive target; no simultaneous source edits/foreign target sharing. Protocol dispatch shipping binary separately captured in actual crash-running receipt. This is not a new at-dispatch attestation for every historical script command.'})
final=read(ev/'commands/final-nine-cases-13/record.json');crash=read(ev/'commands/final-nine-cases-13/crash-running.json')
owned=freeze['owned'];digest=hashlib.sha256(json.dumps(owned,sort_keys=True,separators=(',',':')).encode()).hexdigest()
write('candidate-manifest.json',{'at':now(),'status':'READY_FOR_INDEPENDENT_REVIEW','issue_status':'NOT_COMPLETED','blocked_dependency':'F09 current running-job recovery / mandatory C04','independent_acceptance':False,'worktree':str(root),'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'owned':owned,'owned_digest_sha256':digest,'lock':{'path':'rust/Cargo.lock','sha256':freeze['lock_sha256'],'unchanged':True},'final_test_binary':final['executed_binaries'],'shipping_binary':{'path':crash['binaryPath'],'actual_executed_sha256':crash['binarySha256'],'readyA':crash['readyA'],'readyB':crash['readyB']},'formal_runner_binary':read(ev/'commands/verify-r05-formal-01/record.json')['executed_binaries'],'formal_binding':'Three formal stages stable; all overall FAIL. Later handoff evidence appended after completion; owned remained exact.','target':{'path':str(root/'rust/target'),'symlink':(root/'rust/target').is_symlink(),'classification':'LOCAL_ONLY_REBUILDABLE_NEVER_GIT'},'no_commit_push':True,'no_main_product_write':True,'no_paid_or_real_user_data':True})
# 先完成报告，再制作收取清单；清单不包含自身，最终回复另给它的散列。
r=ev/'REPORT.md';t=r.read_text();t=t.replace('状态：**READY_FOR_INDEPENDENT_REVIEW**。','状态：**READY_FOR_INDEPENDENT_REVIEW**；F10 整项 **NOT_COMPLETED / BLOCKED_DEPENDENCY_F09**。')
t=t.replace('原 T07 服务11例、工具6例、旧 OperationService7例已本区实际运行全绿；','原 T07 服务11例、工具6例、旧 OperationService7例已在最终冻结源码实际重跑全绿（`commands/affected-old-service-final-02`）；原 adapter 知识存储9例及 operations27例也已最终同源重跑（`commands/affected-old-adapters-final-02`）。早先01回归有专属测试输入变动，旧service还在最终JSON排版前，均保留并标 STALE；')
old='截至报告准备时，正式 `verify-stage R05` 为 NOT_RUN；将以本轮全新目录执行，后段是否运行、guard停止、返回状态及嵌套范围以 `formal-gates.json` 和原始阶段输出为准。没跑到的后段不能PASS；阶段失败不由专属九用例替代。'
new='正式 `verify-stage R05` 已按原 stage map 自然执行结束（外层 exit1），R05/R04/R03 三个结果均 overall FAIL，三层输入逐命令及结束绑定均 stable=true，没有 timedOut。外层没有额外600秒截断；测试 guard 原8192MiB/600s保持。R05 suites27套+64 lib pins及F27两例（334.85秒）实际PASS；R04工具10例（301.04秒）及RR1 repair实际PASS。三层workspace均实际在原quota fixture exit101停止，后续target在该workspace腿 NOT_RUN。R03 storage在S0实际9vs7登记红，后续业务腿 NOT_RUN；legacy物化准备因worktree只有.git引用文件而无独立.git目录受阻，业务未运行；R03 repair真实cancel_link_inheritance7匹配、subagent_closeout8匹配各101，不能把失败summary解析缺失说成0匹配。其它command逐项结果、时刻、缺证据与场景状态见 `formal-gates.json` 原收据。没跑到的后段不能PASS；阶段失败不由专属九用例替代。'
assert old in t;t=t.replace(old,new)
t+='\n完整 no-fail-fast 首轮的四个 failed target / 五个用例均保留：cancel_link_inheritance 的 subagent_timeout；cancellation_tree 的 approval roundtrip 与 panicking child；run_lifecycle 的三调用终态；subagent_closeout 的父取消零调用。源在首轮有变化故全轮 STALE，不冒称最终候选全仓验收。最终 formal repair 又实际确认 quota/closeout 红；purebc实际红只做同SHA归因。panic精确purebc未跑仍 NOT_CLOSED。\n\n交接优先顺序：全新空历史 W10 Reviewer 必须亲验当前修复与C04缺口，保留首次失败窗口；之后新F09执行者修真正续嵌/计数/ready复用，再整合冻结输入并重做独立验收。未自签验收。`command-history.json` 保留全部失败、准备失败、重试、源前后/二进制绑定限制；`formal-binaries.json` 是日志证实路径后的散列采样，未伪称每个旧脚本都具备启动前二进制采样。合成home保留为本地受保护证据、逐文件散列仅入排除索引；临时mutation源码备份已删除，target与全文源码副本不收取。\n'
r.write_text(t)
# 排除敏感home/转储/临时材料，保留其受保护散列，禁止取凭据值。
allowed=[];excluded=[];flags=[]
text_suffix={'.json','.md','.py','.log','.txt','.tsv','.sh','.err','.out','.jsonl','.ndjson','.addr','.pid','.sha256','.pass-lines','.probe-done'}
for f in sorted(ev.rglob('*')):
 if not f.is_file():continue
 rel=str(f.relative_to(ev));reason=None
 if 'synthetic-home' in f.parts:reason='LOCAL_ONLY_SENSITIVE_SYNTHETIC_HOME_NEVER_GIT'
 elif '__pycache__' in f.parts or f.suffix=='.pyc':reason='LOCAL_ONLY_REBUILDABLE_PYTHON_CACHE'
 elif 'dump' in f.name or f.name=='.last-body':reason='LOCAL_ONLY_PROTECTED_RAW_STATE_OR_REPLY'
 elif f.suffix not in text_suffix:reason='LOCAL_ONLY_UNCLASSIFIED_NOT_ALLOWLISTED'
 else:
  try:
   body=f.read_text()
   if re.search(r'-----BEGIN (?:EC |RSA |OPENSSH )?PRIVATE KEY-----\s*\n',body) or re.search(r'Bearer\s+(?!\[token\]|<redacted>|redacted|dummy)[A-Za-z0-9._-]{20,}',body):reason='LOCAL_ONLY_POTENTIAL_SECRET_PROTECTED';flags.append(rel)
  except UnicodeDecodeError:reason='LOCAL_ONLY_BINARY_NEVER_GIT'
 item={'path':rel,'sha256':sha(f),'bytes':f.stat().st_size}
 if reason:item['classification']=reason;excluded.append(item)
 else:item['classification']='SAFE_EVIDENCE_EXACT_ALLOWLIST';allowed.append(item)
assert not flags, flags
write('evidence-manifest.json',{'at':now(),'status':'READY_FOR_INDEPENDENT_REVIEW','issue_status':'NOT_COMPLETED','safe_to_collect':allowed,'local_only_never_git':excluded,'local_only_root_target':str(root/'rust/target'),'no_full_source_archive_copy':True,'protected_retention':'Synthetic homes kept locally for original SQLite/wire forensic linkage. Only hashes/path/size exported, never credential bytes. Local-only raw state excluded. No unrelated user data touched.','no_secret_header_capture':True,'memory_measured_mb':None,'manifest_self_hash':'Self excluded to avoid circular checksum; hash supplied in final handoff.','final_product_owned':'candidate-manifest.json exact five sources only; parent input/control files not new owned.'})
print(json.dumps({'owned_digest':digest,'safe_files':len(allowed),'protected_files':len(excluded),'manifest_sha256':sha(ev/'evidence-manifest.json'),'stage_results':[(x['stage'],x['overall']) for x in formal]},ensure_ascii=False))

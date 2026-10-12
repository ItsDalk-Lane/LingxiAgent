"""从已验证的真实执行记录生成收据，绝不把局部检查或原缺口写成整体验收完成。"""
import importlib.util,json,hashlib,datetime,os
from pathlib import Path
root=Path.cwd();out=root/'artifacts/rust-tauri/R06/original-proofs-20261012';v=out/os.environ.get('R06_ORIGINAL_VERIFICATION_SUBDIR','verification')
s=importlib.util.spec_from_file_location('r06',root/'scripts/rust-tauri/r06_stage_inputs.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
inputs,graph=m.build(root);inventory=inputs['originalProofInventory'];leaves=json.loads((v/'R06_LEAVES/leaf-cases.json').read_text());index=json.loads((v/'execution-index.json').read_text());closing=json.loads((v/'closing-results.json').read_text())
assert leaves['passedRegisteredAssertions'] and leaves['sourceStable'] and not leaves['executionErrors']
assert index['executed']==index['total'] and index['failed']==0
assert len(closing['checks'])==3 and all(r['localPassed'] for r in closing['checks'])
for r in closing['checks']:
 assert r['pid']>0 and r['exitCode']==0 and r['sourceStable'] and r['sourceBeforeSha256']==r['sourceAfterSha256']==leaves['sourceCandidateSha256']
 for label in ['stdout','stderr']:assert m.file_sha(Path(r[label]))==r[label+'Sha256']
 assert m.inspect_output(r['binding'],Path(r['stdout']).read_text(),Path(r['stderr']).read_text())['valid']
cases={r['case']:r for r in leaves['cases']};partial={r['case']:r for r in leaves['partialProofs']};results=[]
for row in inventory['items']:
 full=True;per=[]
 for i,a in enumerate(row['assertions']):
  case=f'{row["id"]}-assertion-{i:02}'
  if case in cases:
   proof=cases[case];assert proof['ok'] and proof['actual']==proof['expect']==1
   status='ORIGINAL_ASSERTION_VERIFIED'
  elif case in partial:
   proof=partial[case];assert proof['checkedSubsetPassed'] and proof['originalAssertionSatisfied']is False
   status='PARTIAL_ONLY_ORIGINAL_ASSERTION_NOT_PASSED';full=False
  else:proof=None;status='NOT_PASSED_NO_COMPLETE_IMPLEMENTATION_OR_PROOF';full=False
  per.append({'index':i,'originalAssertion':a,'status':status,'proofCase':case if proof else None})
 status='FULL_ORIGINAL_REQUIREMENT_VERIFIED' if row['status']=='READY' and full and not row['gaps'] else 'NOT_PASSED_ORIGINAL_GAPS_RETAINED'
 results.append({'id':row['id'],'originalThen':row['then'],'status':status,'gaps':row['gaps'],'assertions':per,'reviewSource':row['reviewSource']})
assert len(results)==279
scope=json.loads((out/'continued-103-scope.json').read_text());selected=set(scope['selectedRequirementIds']);frozen=set(scope['frozenNotPassedIds'])
assert len(selected)==103 and len(frozen)==176 and selected.isdisjoint(frozen)
assert set(index['selectedOriginalLeaves'])==selected and set(leaves['selectedOriginalLeaves'])==selected
original_rows={row['id']:row for row in inventory['items']}
assert {row['id'] for row in scope['frozenOriginalRequirements']}==frozen
for original in scope['frozenOriginalRequirements']:
 row=original_rows[original['id']]
 assert row['status']==original['status']=='GAP'
 assert row['then']==original['then'] and row['assertions']==original['assertions']
for row in results:
 if row['id'] in frozen:assert row['status']=='NOT_PASSED_ORIGINAL_GAPS_RETAINED'
selected_pass=sum(row['id'] in selected and row['status']=='FULL_ORIGINAL_REQUIREMENT_VERIFIED' for row in results)
gap_categories=[
 ('桌面文件操作、预览及保存', ['238267978DDA','2FB382A137FC','4173A08A51CC','45627E4F8173','ADB6B0580864','AF18F5CEA14F','BDE3AC44D79C','CAA876503B1B','CC738012B6EE','D6509017C382','EE18A57F7348'], '当前新版本缺少对应的完整界面流程，后台操作证明不足以判定整条通过。'),
 ('子任务使用', ['00ECC9568490','207DF594B497','5F879158C679'], '候选列表和任务进度、关闭后的最终状态、继续任务所需的原历史内容尚不完整。'),
 ('记忆恢复', ['041F938C351F'], '忙碌或内容冲突时拒绝恢复的要求尚未实现。'),
 ('整组会话归档', ['3F95164A488C'], '归档和跳过已有证明，但原要求的错误计数返回尚不完整。'),
 ('会话搜索', ['A18187567AF9'], '按标题匹配时未返回原要求的内容片段。'),
 ('文件交付', ['1CD1C31CCF44'], '没有会话身份时仍会登记文件并返回成功，未满足必须失败的原要求。'),
]
categorized={'R00-T02-LA-'+suffix for _,suffixes,_ in gap_categories for suffix in suffixes}
assert categorized=={r['id'] for r in results if r['id'] in selected and r['status']!='FULL_ORIGINAL_REQUIREMENT_VERIFIED'}

allowed={'rust/crates/xtask/src/stage_map.rs','rust/crates/xtask/src/stage_maps/R06.json','rust/crates/xtask/src/stage_maps/R06_INPUTS.json','scripts/rust-tauri/r06_stage_inputs.py','scripts/rust-tauri/tests/test_r06_stage_inputs_contract.py'}
allowed.update('rust/crates/lingxi-service/tests/'+suite+'.rs' for suite in ['subagent_closeout','subagent_lifecycle','subagent_permission_inheritance'])
preservation=[]
for r in json.loads((out/'source-before.json').read_text()):
 p=root/r['path'];actual=hashlib.sha256(p.read_bytes()).hexdigest() if p.is_file() else None
 status='UNCHANGED' if actual==r['sha256'] else 'AUTHORIZED_PROOF_PREPARATION_UPDATE' if r['path']in allowed else 'UNEXPECTED_CHANGE'
 preservation.append({**r,'afterSHA256':actual,'preservationStatus':status})
assert not any(r['preservationStatus']=='UNEXPECTED_CHANGE' for r in preservation)
m.save(out/'preserved-source-results.json',{'files':preservation,'unchanged':sum(r['preservationStatus']=='UNCHANGED' for r in preservation),'authorizedUpdates':sum(r['preservationStatus']=='AUTHORIZED_PROOF_PREPARATION_UPDATE' for r in preservation),'unexpectedChanges':0})
current=m.candidate_snapshot(root,v);assert current['digestSha256']==leaves['sourceCandidateSha256']
full_count=sum(r['status']=='FULL_ORIGINAL_REQUIREMENT_VERIFIED' for r in results)
receipt={'schema':'lingxi.r06.original-proof-receipt.v1','createdAtUTC':datetime.datetime.now(datetime.timezone.utc).isoformat(),'userScope':'只补已有功能的证明，未实现的条目保留未通过','status':'CURRENT_PROOFS_VERIFIED_ORIGINAL_UNMET_REQUIREMENTS_RETAINED','originalRequirementCount':279,'selectedOriginalRequirements':103,'selectedRequirementsPassed':selected_pass,'selectedRequirementsStillNotPassed':103-selected_pass,'previous176RequirementsRetainedNotPassed':176,'fullyVerifiedRequirements':full_count,'notPassedRequirements':279-full_count,'originalAssertions':sum(len(r['assertions']) for r in results),'fullyVerifiedAssertions':sum(a['status']=='ORIGINAL_ASSERTION_VERIFIED' for r in results for a in r['assertions']),'partiallyVerifiedAssertionsNotAccepted':sum(a['status']=='PARTIAL_ONLY_ORIGINAL_ASSERTION_NOT_PASSED' for r in results for a in r['assertions']),'uniqueExecutedChecks':index['total'],'allUniqueExecutedChecksPassed':True,'necessaryClosingChecksPassed':True,'preservedPreviousSourceItems':len(preservation),'unexpectedPreviousSourceChanges':0,'productFunctionChanges':0,'sourceCandidateSha256':current['digestSha256'],'all279RequirementsAccepted':full_count==279,'R06Accepted':False,'fullR06AndR05ClosureExecution':'NOT_RUN_OR_CLAIMED_THIS_PROOF_ONLY_SCOPE','commit':False,'push':False,'requirements':results}
m.save(out/'PROOF_RECEIPT.json',receipt)
summary=f'''# 本轮原要求证明结果\n\n这轮只补已有功能的证明，没有修改功能或放宽原标准。279条原要求及全部额外预期均保留。\n\n- 本次103条中，{selected_pass}条原要求已有完整证明，并通过本轮实际检查。
- 本次另有{103-selected_pass}条仍不满足完整原要求，具体原因逐条保留。
- 原来的176条全部保留未通过，本次没有运行其专项检查。\n- {279-full_count}条仍未通过，逐条保留未实现、行为不符或证明不足的具体原因。\n- 共有{index['total']}项独立检查实际通过；多条要求共享的检查只运行一次。\n- 必要的证明防错检查通过，局部证明不能被当成整条通过。\n- 原有{len(preservation)}项资料已对照：除5项证明登记更新外均未改变，之前修好的功能保留。\n\nR06尚未整体验收通过。未执行完整阶段及前序全量验收，没有提交或推送。\n\n详细结果在同目录的PROOF_RECEIPT.json，真实执行记录和原始输出在{v.name}目录。\n'''
summary=summary.replace('除5项证明登记更新外均未改变','除5项证明登记和3项旧检查准备更新外均未改变')
gap_table='| 本次未通过的内容 | 条数 | 原因 |\n| --- | ---: | --- |\n'+'\n'.join(f'| {label} | {len(ids)} | {reason} |' for label,ids,reason in gap_categories)
summary=summary.replace('R06尚未整体验收通过。',gap_table+'\n\nR06尚未整体验收通过。')
(out/'SUMMARY.md').write_text(summary)
print(json.dumps({k:val for k,val in receipt.items() if k!='requirements'},ensure_ascii=False,indent=2))

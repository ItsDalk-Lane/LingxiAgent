"""为实际补充修复增加具名证明；不减少原断言或移走已知缺口。"""
from pathlib import Path
import hashlib,json,re,runpy,subprocess
ROOT=Path(__file__).resolve().parents[4]
helpers=runpy.run_path(str(ROOT/'artifacts/rust-tauri/R06/continuation-20261013/review-proof-anchors.py'))
def location(test):
 path=test.get('sourcePath') or f"rust/crates/{test['package']}/tests/{test['suite']}.rs"
 raw=(ROOT/path).read_text();name=test['test'].rsplit('::',1)[-1]
 d=re.search(r'\bfn\s+'+re.escape(name)+r'\s*\(',raw);assert d,name
 depth=0;opened=False
 for t in helpers['tokens'](raw[d.end():]):
  if t.group()=='{':depth+=1;opened=True
  elif t.group()=='}':
   depth-=1
   if opened and depth==0:end=d.end()+t.end();break
 return dict(path=path,test=test['test'],sourceSHA256=hashlib.sha256(raw.encode()).hexdigest(),testStartLine=raw[:d.start()].count('\n')+1,testEndLine=raw[:end].count('\n')+1,assertions=helpers['assertions_in_body'](raw,raw[d.start():end],d))
def integration(suite,name,package='lingxi-service'):
 return dict(package=package,suite=suite,test=name)
def github(name):
 return dict(package='lingxi-service',suite='lib',test='memorytools::github_tests::'+name,kind='rust_library_test',sourcePath='rust/crates/lingxi-service/src/skills_github_tests.rs')
p=ROOT/'scripts/rust-tauri/r06_original_leaf_bindings.json';j=json.loads(p.read_text());rows={r['id']:r for r in j['items']}
adds=[]
def add(suffix,index,test):
 row=rows['R00-T02-LA-'+suffix];g=next(g for g in row['groups'] if g['index']==index)
 if not any(t==test for t in g['tests']):
  g['tests'].append(test);g['proofLocations'].append(location(test));adds.append({'leaf':row['id'],'group':index,'test':test['test']})
for index,names in {
 0:['github_real_http_flags_full_package_review_and_only_current_agent','github_registered_real_driver_requires_review_rejects_readonly_and_never_fetches_before_approval'],
 1:['github_review_rejection_token_changed_package_failure_and_cancellation_zero_install'],
 2:['github_review_rejection_token_changed_package_failure_and_cancellation_zero_install'],
 3:['github_invalid_archive_and_install_destination_keep_pool_unchanged','github_invalid_sources_and_skipped_review_local_zip_skill_preserved','github_actual_public_host_obeys_current_dns_refusal_without_install'],
}.items():
 for name in names:add('3236B05B284F',index,github(name))
for index,names in {0:['ex02_actual_skill_and_notes_survive_compaction_restart_next_run_and_repeat'],1:['ex02_unreadable_notes_fail_before_model_and_preserve_history','ex02_pending_tool_approval_and_current_goal_cannot_be_compacted_away']}.items():
 for name in names:add('D62A81C2AEB2',index,integration('r06_ex01_manual_compaction',name))
add('E2AF86A962D0',0,integration('r06_original_knowledge_proofs','original_citation_non_bmp_offsets_match_current_viewer_and_survive_restart'))
add('01E1E3CF7FA8',0,integration('ex08_processing_retention_review','actual_csv_processing_retains_until_seventh_day_then_removes_facts_outputs_not_user_file'))
for name in ['actual_second_connection_cannot_attach_between_gc_check_and_delete','gc_uses_opened_root_after_path_is_replaced_and_never_follows_output_leaf_link']:
 add('01E1E3CF7FA8',1,integration('ex08_independent_review',name,'lingxi-adapters'))
rows['R00-T02-LA-3236B05B284F']['groups'][0]['meaning']+='；另有真实GitHub归档HTTP替身，经实际注册/R03审批及两个开关，完整安装且仅当前助手启用，生产公网DNS拒绝另有真实反例，不冒称公网成功。'
rows['R00-T02-LA-D62A81C2AEB2']['groups'][0]['meaning']+='；实际技能read和本会话便签经压缩、重启、下一模型请求及再次压缩保留，恢复材料按普通历史保留来源，不提升权限。'
rows['R00-T02-LA-E2AF86A962D0']['groups'][0]['meaning']+='；公开引用位置按当前JS界面UTF16正确高亮扩展字符，旧引用事实与原快照逐字节保留；内部检索/读取位置保留既有scalar合同并明确单位和等价UTF16位置。'
baseline=json.loads(subprocess.check_output(['git','show','6d19332793c0819f5219a4ed42fcc8f0d7f813a4:scripts/rust-tauri/r06_original_leaf_bindings.json'],cwd=ROOT))
old={r['id']:(r['assertions'],r['then']) for r in baseline['items']}
changed=[r['id'] for r in j['items'] if old[r['id']]!=(r['assertions'],r['then'])]
assert changed==['R00-T02-LA-01E1E3CF7FA8'],changed
assert rows['R00-T02-LA-7432B00DEC94']['status']=='GAP'
assert rows['R00-T02-LA-041F938C351F']['status']=='GAP'
p.write_text(json.dumps(j,ensure_ascii=False,indent=2)+'\n')
Path(__file__).with_name('extra-current-binding-review.json').write_text(json.dumps({'status':'REGISTERED_PENDING_CURRENT_WINDOW','addedLinks':adds,'preservedOriginalLeafCount':278,'authorizedChangedLeaf':changed,'knownGapsPreserved':['R00-T02-LA-7432B00DEC94','R00-T02-LA-041F938C351F'],'judgementLogicChanged':False,'stageAccepted':False},ensure_ascii=False,indent=2)+'\n')
print('added',len(adds),'proof links; known failures remain GAP')

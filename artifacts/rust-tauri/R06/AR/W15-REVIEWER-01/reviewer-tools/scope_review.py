import pathlib,json,hashlib,re,collections
ROOT=pathlib.Path('/private/tmp/lingxi-r06-w15-reviewer-01');OUT=ROOT/'artifacts/rust-tauri/R06/AR/W15-REVIEWER-01'
def load(path):return json.loads((ROOT/path).read_text())
def sha(path):return hashlib.sha256((ROOT/path).read_bytes()).hexdigest()
original=load('docs/rust-tauri/R06/repair-current/AR_ORIGINAL_OBLIGATIONS.json');author=load('artifacts/rust-tauri/R06/AR/W15-EXECUTOR-01/SCOPE_RECONCILIATION.json');catalog=load('Lingxi_Rust_Tauri_Taskbooks_2026-09-23/task-catalog.json');r00=load('docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json');mapping=load('docs/rust-tauri/R06/R06_LEAF_MAP.json');scope28=load('artifacts/rust-tauri/R06/AR/W15-REVIEWER-01/windows/original-scope28-runtime/scope28-observations.json')
feature_index={x['feature_id']:x for x in r00['features']};tasks={x['id']:x for x in catalog['tasks'] if x['stage_id']=='R06'};members=collections.defaultdict(list)
for task,value in mapping['tasks'].items():
 for leaf in value['leaves']:members[leaf['feature_id']].append({'task':task,'historicalLabel':leaf.get('t'+task[-2:]+'_status'), 'candidateAnchor':leaf.get('t'+task[-2:]+'_anchor'), 'rawLeaf':leaf})
original_features={x['feature_id'] for x in r00['features'] if any(t.startswith('R06-') for t in x['task_ids'])};assert original_features==set(members)=={x['id'] for x in original['r00_scope_dispositions']};assert len(tasks)==8
sources={str(x['source']) for x in original['obligations']};sources.update(['artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md','Lingxi_Rust_Tauri_Taskbooks_2026-09-23/stage-index.json','Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R06_上下文、会话语义、记忆与知识库.md','Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R07_外围能力、后台自动化与多入口接入.md','docs/rust-tauri/R03/R03_HANDOFF.json','docs/rust-tauri/R04/R04_HANDOFF.json','docs/rust-tauri/R05/R05_HANDOFF.json'])
reads=[]
for source in sorted(sources):
 p=ROOT/source
 if not p.exists():reads.append({'source':source,'status':'MISSING'});continue
 text=p.read_text();reads.append({'source':source,'sha256':sha(source),'bytes':len(p.read_bytes()),'lines':len(text.splitlines()),'status':'FULL_FILE_READ_FOR_ORIGINAL_CLAUSE_RECONCILIATION'})
observed28={x['featureId']:x for x in scope28['cases']};leaves=[]
for row in original['r00_scope_dispositions']:
 id=row['id'];f=feature_index[id];later=row['legal_later_basis'];actual='原始成功/拒绝/副作用断言与当前服务份额逐项保留；源锚存在或原旧标签不构成完整业务证明。';state='CURRENT_SERVICE_SHARE_UNPROVEN';evidence=[]
 if later:state='CURRENT_SERVICE_SHARE_UNPROVEN_WITH_LEGAL_LATER_SHARE';actual+='原后续Task只豁免其自身宿主、界面或外围份额，不能豁免当前服务。'
 if id in observed28:
  c=observed28[id];codes=[o['status'] for o in c['observations']];state='R06_ORIGINAL_DEADLINE_REQUIRED_NO_R07_WAIVER';actual='亲跑实际HTTP状态='+str(codes)+'；原R00仅标R06，原R07-T08通用管理任务不覆盖此到期登记。现有保存/读取仅局部份额，运行时刷新、广播、ticker、列表、秘密掩码及跨Agent字段隔离未完成完整证明。';evidence=['windows/original-scope28-runtime/scope28-observations.json']
  if any(code in (404,405) for code in codes):state='FAIL_CURRENT_REQUIRED_ROUTE_MISSING'
  if id.endswith('E4BEE8'):state='FAIL_ETAG_304_SEMANTICS_MISSING';actual+='实际GET缺ETag，条件GET仍200。'
 if id in original['extra_root_clusters'][9]['r00_feature_ids']:
  actual+='原R04交接与R00要求mount成功面，当前源明确无provider；实际local-file正控200、loopbackURL直读200而正式URL stat/read400，mount未建立真实注册正控，仅确认当前不支持入口，不冒称成功fixture验收。';evidence.append('windows/original-resource-precondition/resource-production-observations.json');state='FAIL_URL_REQUIRED_SUCCESS_WITH_MOUNT_SOURCE_GAP'
 leaves.append({'id':id,'originalR00':f,'originalClauseSource':'docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json','currentServiceShare':row['current_service_share'],'legalLaterBasis':later,'historicalMemberships':members[id],'expected':'当前R06服务份额和原成功/拒绝/副作用完整；合法后续仅按原条款保留。','actual':actual,'status':state,'evidenceLevel':'ACTUAL_SCOPED_PROBE_PLUS_SOURCE' if evidence else 'SOURCE_RECONCILIATION_FULL_BEHAVIOR_NOT_EXECUTED','runtimeEvidence':evidence,'fullBusinessAccepted':False})
leaf_by_id={x['id']:x for x in leaves};obligations=[]
for row in original['obligations']:
 id=row['id'];state='NOT_CLOSED_CURRENT_REQUIREMENT';actual='原条款已与来源和候选映射核对；完整当前业务未由本候选提供有效关闭证据。';level='SOURCE_RECONCILIATION';evidence=[]
 if row['kind']=='original_task_step':
  task=id.rsplit('-S',1)[0];index=int(id.rsplit('-S',1)[1])-1;assert tasks[task]['steps'][index]==row['original_clause'];actual='原任务书真实Step逐字一致；'+actual
 if row['kind']=='r00_leaf_assertion':
  fid=id.split(':ASSERT-')[0];assert row['original_clause'] in feature_index[fid]['acceptance_assertions'];leaf=leaf_by_id[fid];state=leaf['status'];actual=leaf['actual'];level=leaf['evidenceLevel'];evidence=leaf['runtimeEvidence']
 if row['kind']=='inherited_stage_share':actual='逐条保留前序图的stageShare/laterShare/assertionContract；F08局部份额及本次workspace检查不能扩成原R03完整通过。正式序链NOT_RUN。'
 if id in ('R06-A15','R06-A16'):
  state='PASS_REQUIRED_LOCAL_DETERMINISTIC_ACCEPTANCE';actual='亲跑BE10实际a原字节不变/b被写、四维True/True/False/False；A16亲跑BE04/05正常及注入拒绝，未放宽网关，失败调用费用保留。';level='ACTUAL_BINARY_WIRE_SQL_AND_FILES';evidence=['windows/independent-be-battery-02','windows/independent-sql-wire-mutations-02/REVIEW.json']
 if id.startswith('R06-T08-S'):
  actual={'R06-T08-S01':'原十BE亲跑；BE02只常驻声明，不覆盖发现→描述→调用；BE07手工scope份额不证明正式聊天来源自动建scope。','R06-T08-S02':'各费用实测可从生产SQL重算；同模型/参数/权限旧引擎实际比较NOT_RUN，不以incumbentAnchor字符串替代。','R06-T08-S03':'本地原BE13检查通过；LIVE多次采样未授权且NOT_RUN，合法后期R10，不能称真实模型通过。','R06-T08-S04':'当前仅评测与记录代码，无生产接线修复；同族虚报检测缺陷仍在候选。','R06-T08-S05':'PI原14能力/15hooks与前序90份额逐项保留；交接有未关闭项，不能通过全部标OPEN完成Task。'}[id]
 if id.startswith('R06-AR-F15-C'):
  state={'R06-AR-F15-C01':'FAIL','R06-AR-F15-C02':'PASS_SCOPED_ACTUAL_METRICS','R06-AR-F15-C03':'FAIL_NEW_FAMILY_FALSE_REPORTS_ACCEPTED','R06-AR-F15-C04':'PASS_HONEST_CURRENT_STATE'}[id];actual='详见独立REPORT四C结果；本记录不替代运行。';evidence=['REPORT.md']
 obligations.append({'id':id,'kind':row['kind'],'source':row['source'],'originalClause':row['original_clause'],'originalDeadline':row['deadline'],'originalTaskIds':row.get('task_ids',[]),'relatedFindings':row.get('related_findings',[]),'expected':row['original_clause'],'actual':actual,'status':state,'evidenceLevel':level,'runtimeEvidence':evidence,'laterBasis':row.get('later_basis')})
assert len(obligations)==1234;assert sum(len(x.get('acceptance_assertions',[])) for x in feature_index.values() if x['feature_id'] in original_features)==687
report={'schema':'lingxi.r06.w15.independent-original-scope.v1','originalCounts':original['counts'],'readSources':reads,'sourceMissing':[x for x in reads if x['status']=='MISSING'],'originalTasks':list(tasks.values()),'obligations':obligations,'leaves':leaves,'all12ExtraClusters':original['extra_root_clusters'],'historicalReviews':[{'source':x['source'],'originalSha':x['sha256'],'actualSha':sha(x['source']),'unchanged':sha(x['source'])==x['sha256'],'ids':x['obligation_ids'],'decision':'旧PASS和LOW/OBS标签保留历史；按原当前契约重新审，登记不是关闭。'} for x in author['historical_reviews']],'inherited90Shares':[x for x in obligations if x['kind']=='inherited_stage_share'],'28ScopeDecision':'原R00只登记R06的28叶当前服务到期；R07-T08通用管理不能单方面延期。R08/09完整UI不提前实施；保留原Taskbook及R00字节。','noOriginalSourceEdits':True,'fullTaskVerdict':'FAIL','allOpenRegistrationIsNotCompletion':True}
(OUT/'ORIGINAL_SCOPE_REVIEW.json').write_text(json.dumps(report,ensure_ascii=False,indent=2));print(json.dumps({'obligations':len(obligations),'leaves':len(leaves),'assertions':687,'memberships':sum(map(len,members.values())),'historicalReviews':len(report['historicalReviews']),'scope28':len(observed28),'missingSources':report['sourceMissing']},ensure_ascii=False))

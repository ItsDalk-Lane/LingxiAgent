import json,pathlib,hashlib,subprocess,re,datetime
root=pathlib.Path.cwd(); ev=root/'artifacts/rust-tauri/R06/AR/W10-R01'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def put(n,x):(ev/n).write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
owned=['rust/crates/lingxi-service/src/knowledge.rs','rust/crates/lingxi-service/src/operations.rs','rust/crates/lingxi-kernel/src/knowledge.rs','rust/crates/lingxi-adapters/src/models/operations/embedding.rs','rust/crates/lingxi-service/tests/r06_ar_w10_embedding_identity.rs']
records={p.parent.name:json.loads(p.read_text()) for p in (ev/'commands').glob('*/record.json')}
first=records['c-all-01']['inputs_before']; source={n:sha(root/n) for n in owned}; assert all(first[n]==s for n,s in source.items()); assert all(x['inputs_stable'] for x in records.values())
actual_bins=[]
for label,r in records.items():
 for b in r['executed_binaries']:actual_bins.append(dict(command=label,**b,current_sha256=sha(pathlib.Path(b['path']))))
crash=json.load(open(ev/'commands/c-all-01/crash-running.json'));alias=json.load(open(ev/'commands/provider-alias-02/provider-alias-result.json'))
actual_bins.append({'command':'C04 actual shipping SIGKILL/restart; provider-alias-02','path':crash['binaryPath'],'sha256':crash['binarySha256'],'current_sha256':sha(pathlib.Path(crash['binaryPath']))})
assert all(b['sha256']==b['current_sha256'] for b in actual_bins)
head=subprocess.check_output(['git','rev-parse','HEAD']).decode().strip(); assert head=='bc2ce51fd005ea75b9949156974e15d2761eb734'
diff=subprocess.check_output(['git','diff','--name-only']).decode().splitlines();assert set(diff)==set(owned[:4])
spec='artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md'
task='Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R06_上下文、会话语义、记忆与知识库.md'
r05='Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R05_模型协议、凭证、流式处理与最小完整闭环.md'
put('final-binding.json',{'time':datetime.datetime.now(datetime.timezone.utc).isoformat(),'head':head,'tracked_product_diff':diff,'owned':source,'lock':{'path':'rust/Cargo.lock','sha256':sha(root/'rust/Cargo.lock')},'spec_and_original_books':{n:sha(root/n) for n in [spec,task,r05]},'all_runs_input_stable':True,'input_scope':'git ls-files minus artifacts plus dedicated untracked fifth owned test; per-run full hashes retained; immutable specification and taskbook bindings added above','executed_binaries':actual_bins,'cargo':'/Users/study_superior/.cargo/bin/cargo 1.98.1; --locked -j8 on all cargo build/test/clippy/xtask','guard':{'seconds':600,'memory_mib':8192,'enabled':True,'measured_peak_mib':None},'product_or_original_tests_modified_by_reviewer':False,'commit_push_reset_clean':False,'r07_started':False})
run_summaries=[]
for label,r in sorted(records.items()):
 log=(ev/'commands'/label/'output.log').read_text(); tests=[{'passed':int(a),'failed':int(b),'ignored':int(c),'filtered':int(d)} for a,b,c,d in re.findall(r'test result: .*? (\d+) passed; (\d+) failed; (\d+) ignored; \d+ measured; (\d+) filtered out',log)]
 status='PASS' if r['exit_code']==0 else 'FAIL'
 if label=='mutant-c02-red-01':status='EXPECTED_RED_DETECTED'
 if label=='provider-alias-01':status='PREPARATION_FAIL'
 if label=='mutation-green-control-01':status='NOT_RUN_ZERO_MATCH'
 run_summaries.append({'label':label,'status':status,'exit_code':r['exit_code'],'tests':tests,'guard_armed':bool('LINGXI_TEST_GUARD: armed' in log),'elapsed_seconds':r['elapsed_seconds'],'input_stable':r['inputs_stable'],'record':f'commands/{label}/record.json','log':f'commands/{label}/output.log','note':'nine dedicated tests green includes an intentional running409 assertion; full C04 remains FAIL' if label=='c-all-01' else ''})
checks=[{'id':'R06-AR-F10-C01','status':'PASS','expected':'实际 dispatch/served identity/usage/variant 同源，reload不能事后贴错标签','actual':'实际quota屏障后reload；wire A/B两请求，usage A/B对应；旧A向量保留，后续B拒写','evidence':['commands/c-all-01/c01.json'],'remaining':'F09重复请求与F11归属不因此关闭'}, {'id':'R06-AR-F10-C02','status':'PASS','expected':'同维度不同模型跨批不能混写旧variant','actual':'A64块已写，随后B2维真实返回；任务failed，A64保留，B未混写','evidence':['commands/c-all-01/c02.json','commands/mutation-green-control-02/c02.json'],'remaining':'兼容同模型检查点恢复仍另需F09'}, {'id':'R06-AR-F10-C03','status':'PASS','expected':'查询served身份只能选兼容variant，或明确降级','actual':'在途A查询reload后仍匹配A；新B2维及C3维均FTS+KNOWLEDGE_VECTOR_NOT_READY，searchedVectorVariants为空','evidence':['commands/c-all-01/c03.json'],'remaining':'正式聊天自动scope不由此证明'}, {'id':'R06-AR-F10-C04','status':'FAIL','expected':'中断改模型重启，通过实际恢复入口产生准确新构建，不混用旧产物且完整恢复','actual':'已failed分支重启reingest200后B89新向量ready、旧A64保留；实际SIGKILL running同home重启B，正式reingest409，job仍running64/89，无Bvariant；仅安全拒绝不足完整恢复','evidence':['commands/c-all-01/c04.json','commands/c-all-01/crash-running.json','crash-independent-readback.json'],'remaining':'R01-F01 当前功能缺口，依赖F09不是外部环境豁免'}]
put('review-results.json',{'verdict':'FAIL','role':'one-time independent Task Reviewer01, not final Stage Reviewer','c_checks':checks,'runs':run_summaries,'new_counterexample':{'status':'PASS','case':'provider alpha→beta，同model same-model、同endpoint/protocol/dim2；外部首响应屏障间reload','actual':'旧alpha64行failed保留，beta不写alpha key；正式reingest beta90新行ready；五wire对应五usage alpha,beta,beta,beta,beta','evidence':['commands/provider-alias-02/provider-alias-result.json','provider-alias-ledger-final.json'],'mock_boundary':'Only external loopback HTTP responses; candidate shipping Gateway/OperationService/KnowledgeService/state/SQLite real'},'mutation':{'status':'EXPECTED_RED_DETECTED_AND_GREEN_CONTROL','actual':'准确候选隔离副本仅去掉 served_identity != identity 跨批检查；c02实际done而非failed，断言失败101；候选未改、同测试实际1匹配绿0','evidence':['mutation-setup.json','mutation-readback.json','commands/mutant-c02-red-01/record.json','commands/mutant-c02-red-01/output.log','commands/mutation-green-control-02/record.json'],'first_green_attempt':'mutation-green-control-01 zero matched; preserved NOT_RUN, never PASS'},'full_stage_gates':{'R05_verify_stage':'NOT_RUN','R04_verify_stage':'NOT_RUN','R03_verify_stage':'NOT_RUN','full_rust_workspace_tests':'NOT_RUN','full_npm_and_audit_seal':'NOT_RUN; directed legacy E5 BY_SCOPE SKIP','R06_formal_registered_gate':'NOT_RUN; F14 remains OPEN'},'measurement_limits':{'peak_rss_mib':None,'live_suppliers':'NOT_RUN','five_protocols':'build/parse/fingerprint golden only; no five HTTP or LIVE proof','BE04_real_model_sampling':'NOT_RUN'},'source_home_boundary':'Dedicated C and new alias use own effective LINGXI_HOME and actual READY; incumbent original suites explicitly construct own OS temp HomeSource::Cli overriding env, actual DB homes recorded, never real user home','read_only_preparation_errors':['first alias oversized JSON request ConnectionReset, zero supplier requests, preparation failure retained then reduced independent fixture only','mutation-green-control-01 wrong exact test name matched0; second command exact1 matched','final alias ledger first projection nonexistent operation column, corrected using actual schema; no product change']})
# 仅复用原始条款和范围ID；不继承执行者结论。所有结果来自本Reviewer运行或明确未运行。
old=json.load(open(root/'artifacts/rust-tauri/R06/AR/W10-E01/original-scope-matrix.json')); rows=[]
leafmap={a['id']:a for a in old['leaves']}; allold=(ev/'commands/old-service-01/output.log').read_text()
step_actual={
'R06-T07-S01':('PARTIAL','原suite实跑导入/分块/检索/读取/引用/删除/设置；全部24到期叶的所有原错误边界与EX07/08/09尚未逐一全验'),
'R06-T07-S02':('PARTIAL','向量与显式scope旧回归绿；research_engine_tables_are_not_restored只证明未新建研究表，旧研究记录读取/展示兼容NOT_RUN'),
'R06-T07-S03':('PARTIAL','实际同维度A/B、不同维度C、同ID端点变化、provider别名变化、旧key、新key重建均隔离；完整任务统计仍F09 FAIL'),
'R06-T07-S04':('FAIL','实际SIGKILL running重启409卡死；取消deadline上下文仍None；缓存schema/错误分类仍源码OPEN；failed新B重建与旧paid保存仅该分支通过'),
'R06-T07-S05':('PARTIAL','本报告完整登记全部发现，未擅自恢复/删除研究流程；未决必需行为阻断完整功能验收'),
'R06-T07-D01':('PARTIAL','KnowledgeService真实可执行；running恢复、统计、归属、缓存、聊天范围仍未闭合'),
'R06-T07-D02':('PARTIAL','本轮真实兼容矩阵已记录；C04完整恢复FAIL，五协议仅golden不能冒充LIVE'),
'R06-T07-D03':('PARTIAL','引用/旧迁移受影响suite实跑绿；旧research读取、损坏BLOB直接测试与全部旧错误边界NOT_RUN'),
'R06-A13':('PASS_SCOPED_CASE','a13_search_within_scope_returns_only_allowed_sources与a13_citation_markdown_resolves_to_actual_text真实scope服务/工具/SQLite绿；不推出真实聊天自动选源通过'),
'R06-A14':('PASS_SCOPED_CASE','旧a14不兼容维度拒绝+本轮A/B同维度、C异维度FTS显式降级通过；不推出C04与损坏BLOB未测通过')}
for a in old['obligations']:
 rid=a['id'];kind=a['kind']; row={'id':rid,'kind':kind,'original_clause':a['original_clause'],'expectation':'原条款完整保留为当前R06到期要求；仅本轮实际覆盖范围可认定','status':'NOT_RUN','actual':'该独立完整案例本轮未执行，不能继承执行者/旧Reviewer结果','evidence':[],'source_ref':f'{task}:297-329' if rid.startswith('R06-T07') or rid in ['R06-A13','R06-A14'] else spec}
 if rid in step_actual:
  row.update(status=step_actual[rid][0],actual=step_actual[rid][1],evidence=['commands/old-service-01/output.log','commands/c-all-01/record.json','review-results.json'])
 elif kind=='r00_leaf_assertion':
  lid=rid.split(':ASSERT-')[0];cases=leafmap[lid]['exact_regression_cases'];matched=[c for c in cases if f'test {c} ... ok' in allold]
  row.update(status='PARTIAL_REGRESSION_ONLY' if matched else 'NOT_RUN',actual='已亲跑相关断言案例：'+','.join(matched)+'。文件级绿色不能证明此条款全部分支/拒绝/副作用；完整原断言仍未统一验收。',evidence=['commands/old-service-01/output.log'] if matched else [],exact_cases_run=matched,full_assertion_status='NOT_FULLY_VERIFIED')
  if lid.endswith('76E2F5') or lid.endswith('8A6247'):
   row.update(status='FAIL',actual='原摄入/重试相关旧回归通过，但本轮实际running64/89重启恢复409与重复统计2/1违反任务状态/副作用要求；见R01-F01/F02',evidence=['commands/old-service-01/output.log','commands/c-all-01/crash-running.json','commands/c-all-01/positive.json'])
 elif rid.startswith('R06-AR-F10:'):
  row.update(status='PARTIAL' if 'REPAIR-' in rid else next(x['status'] for x in checks if x['id'] in rid),actual='C01/C02/C03实际身份链通过；C04 running完整恢复FAIL；见本轮逐C结果',evidence=['review-results.json'])
 elif rid.startswith('R06-AR-F09:'):
  row.update(status='FAIL' if 'REPAIR-' in rid else 'NOT_RUN_COMPLETE_CASE_WITH_COUNTEREVIDENCE',actual='实际正例重复首批，2/1及153/89统计超量；running恢复失败。F09规定1/64/65/128/129全矩阵本轮未重跑，不能PASS',evidence=['commands/c-all-01/positive.json','commands/c-all-01/c04.json','commands/c-all-01/crash-running.json'])
 elif rid.startswith('R06-AR-F11:'):
  row.update(status='FAIL' if 'REPAIR-' in rid else 'NOT_RUN_COMPLETE_CASE_WITH_COUNTEREVIDENCE',actual='实际已发wire2仅usage1，session/run/attempt/cause null；Knowledge传None deadline/context。F11完整四C本轮NOT_RUN',evidence=['crash-independent-readback.json'],source_ref='rust/crates/lingxi-service/src/knowledge.rs:136')
 elif rid.startswith('R06-AR-F12:') or rid=='DECLARED:R06-T07:POST-02':
  row.update(status='SOURCE_ONLY_OPEN',actual='两cache任何open失败均remove重建仍在；本轮facts schema较新旧测试不能代替两cache生产入口四C',source_ref='rust/crates/lingxi-adapters/src/storage/knowledge.rs:1859,2333')
 elif rid.startswith('R06-AR-F13:'):
  row.update(status='NOT_RUN',actual='显式scope旧工具回归通过，但真实聊天自动选源/主体绑定/历史重启四C未运行，不能继承或关闭',evidence=['commands/old-service-01/output.log'])
 elif rid=='DECLARED:R06-T07:POST-01':row.update(status='NOT_RUN',actual='损坏BLOB第三门本轮无直接负例；旧A14维度用例不能替代')
 elif rid=='DECLARED:R06-T07:POST-04':row.update(status='SOURCE_ONLY',actual='.vscode/settings.json没有Reviewer改动；没有Git提交/删除')
 rows.append(row)
leaves=[]
for a in old['leaves']:
 f=a['original_feature'];cases=[c for c in a['exact_regression_cases'] if f'test {c} ... ok' in allold]; status='PARTIAL_REGRESSION_ONLY' if cases else 'NOT_RUN';remaining='原每一断言完整边界仍未逐项全验；不归R07、不据登记关闭'
 if a['id']=='F-D08-C-RESEARCH-LEDGER':remaining='不创建research表的absence检查绿；旧研究记录读取/展示与来源引用保护完整兼容NOT_RUN'
 if a['id'].endswith(('76E2F5','8A6247')):status='FAIL';remaining='running重启409与统计超量已实证违反原任务状态/恢复条款'
 leaves.append({'id':a['id'],'title':a['title'],'original_requirement':f.get('acceptance_requirement',f.get('visible_result')),'original_source_refs':f.get('source_ref'),'assertion_rows':a['assertion_rows'],'exact_cases_run':cases,'status':status,'actual':'本轮真正旧suite相关案例通过，证据并不覆盖全部原分支','remaining':remaining,'evidence':['commands/old-service-01/output.log'] if cases else []})
r05steps=['从 ModelManager、model-operation-resolver、llm-client、媒体适配与 Pi 适配收集实际协议/目的/能力，而非枚举市场全部模型。','定义模型标识 provider+model+operation、输入输出模态、工具/推理支持、上下文预算及协议选项；同名 modelId 不能跨 provider 混用。','主对话、标题、摘要、记忆、审批、视觉、embedding与媒体均由同一 Rust Gateway解析身份、策略和观测。','不支持的能力在请求前明确报错；供应商特有字段保存在受控扩展区，不能被公共转换静默丢失。']
r05rows=[{'id':f'R05-T01-S{i+1:02d}','original_clause':t,'status':'PARTIAL_F10_EMBEDDING_SHARE','actual':'embedding实际Gateway不可变served route及usage/variant一致已验；该Step所有其它purpose/能力完整检查NOT_RUN','evidence':['commands/c-all-01/c01.json','provider-alias-ledger-final.json']} for i,t in enumerate(r05steps)]
for i,t in enumerate(['ModelGateway','PROVIDER_SUPPORT_MATRIX.json','操作到模型映射']):r05rows.append({'id':f'R05-T01-D{i+1:02d}','original_clause':t,'status':'PARTIAL_F10_EMBEDDING_SHARE','actual':'只亲验当前embedding消费者；所有模型操作完整矩阵NOT_RUN'})
r05rows += [{'id':'R05-A01','original_clause':'同名模型不串凭证：只使用对应endpoint/凭证/trace标签','status':'PARTIAL','actual':'alpha/beta同名模型loopback身份与usage/key已验证；两配置同一合成凭据，独立不同凭据完整反例NOT_RUN，不冒充全A01'},{'id':'R05-A02','original_clause':'不支持能力提前失败：明确配置错误、不发送损坏请求或偷偷换模型','status':'NOT_RUN_COMPLETE_CASE','actual':'本轮旧27operation adapters及7operation service回归绿；未重跑A02完整工具/视觉能力矩阵'}]
put('original-scope-review.json',{'verdict':'FAIL','provenance':'仅原ID/条款/叶与断言文字采用已固定范围库存；独立结果全部由Reviewer本轮运行或明示SOURCE_ONLY/NOT_RUN生成，无执行者PASS继承','counts':{'t07_steps':5,'t07_deliverables':3,'t07_base_acceptances':2,'t07_leaves':len(leaves),'t07_assertions':sum(a['kind']=='r00_leaf_assertion' for a in rows),'related_obligations':len(rows)},'r06_t07_and_related_rows':rows,'leaves':leaves,'r05_t01':r05rows,'full_scope_counts':json.load(open(ev/'preparation.json'))['scope_counts'],'stage_status':'NOT_COMPLETED; all required missing shares remain R06; no rename/share/defer closes them','spec_gap':'SCOPE inventory declares missing original 2026-10-08 actually-issued prompt receipt; catalog-only metadata, this Reviewer does not claim independently searched external personal files'})
print('generated',len(rows),'rows',len(leaves),'leaves',len(actual_bins),'bin receipts')

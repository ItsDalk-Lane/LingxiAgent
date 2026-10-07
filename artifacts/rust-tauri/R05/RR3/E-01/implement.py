from pathlib import Path
import json,hashlib,subprocess,datetime,collections,os
ROOT=Path.cwd(); E=ROOT/'artifacts/rust-tauri/R05/RR3/E-01'; D=ROOT/'docs/rust-tauri/R05'
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def dump(p,d): Path(p).write_text(json.dumps(d,ensure_ascii=False,indent=2)+'\n')
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
base=json.loads((E/'before.json').read_text()); HEAD=base['head']
# 输入摘要明确只覆盖所列源文件，不冒充 xtask 全工作树冻结摘要。
files=[]
for dirname,excluded in [('rust',{'target','.git'}),('scripts/rust-tauri',{'__pycache__'})]:
 for directory,dirs,names in os.walk(ROOT/dirname):
  dirs[:]=[n for n in dirs if n not in excluded]
  files.extend(Path(directory)/n for n in names if (Path(directory)/n).is_file())
files=sorted(set(files+[ROOT/'rust-toolchain.toml',ROOT/'shared/contract-versions.json']))
entries={str(p.relative_to(ROOT)):sha(p) for p in files}; payload=''.join(f'{h}  {p}\n' for p,h in sorted(entries.items()))
inputs={'capturedAt':now(),'algorithm':'sha256(sorted UTF-8 lines: file_sha256 + two spaces + repo-relative path + newline)','scope':'rust/** excluding target and .git; scripts/rust-tauri/** excluding __pycache__; rust-toolchain.toml; shared/contract-versions.json. 包含测试与构建输入；不包含文档、artifact、主树其他目录；不是 xtask candidateSourceBinding 或终审冻结摘要。','count':len(entries),'digest':hashlib.sha256(payload.encode()).hexdigest(),'files':entries}
dump(E/'inputhash-before.json',inputs)
guards={p:sha(ROOT/p) for p in base['authority']}
for p in (E/'dispatch').rglob('*'):
 if p.is_file():guards[str(p.relative_to(ROOT))]=sha(p)
for p in (D/'repair-current').glob('*'):
 if p.is_file(): guards[str(p.relative_to(ROOT))]=sha(p)
dump(E/'protected-before.json',guards)
R='artifacts/rust-tauri/R05/RR3/'
refs={
 'historical_audit':R+'TASK0/historical-layer-audit.json',
 'rr2_final':'artifacts/rust-tauri/R05/RR2/FINAL-01/verify-r05-2/verify-stage-result.json',
 'rr2_final_review':'artifacts/rust-tauri/R05/RR2/FINAL-01/STAGE_REVIEW.md',
 'a2':R+'A-02/REPORT.md','a1_review':R+'A-REVIEW-01/REVIEW.md',
 'b_review':R+'B-REVIEW-01/REVIEW.md','c':R+'C-01/REPORT.md','i10':R+'C-01/I-MAPPING.md',
 'ordinary_cancel':R+'C-01/ordinary-cancel-subagent-01/command.json','late_result':R+'C-01/ordinary-cancel-late-result-01/command.json',
 'f46':R+'F46-01/REPORT.md','resource_receipt':R+'F46-01/resources-01/command.json','resource_raw':R+'F46-01/resources-01/f27-resource-series.json','runtime_binding':R+'F46-01/resources-01/runtime-binding.json','resource_negative':R+'F46-01/sampler-negative-isolated/result.json',
 'd':R+'D-01/REPORT.md','d_prepared':R+'D-01/PREPARED-ALLOW.md',
 'rr2_b_review':'artifacts/rust-tauri/R05/RR2/B-R2/R2/REVIEW-r2.md','rr2_d_review':'artifacts/rust-tauri/R05/RR2/D-R2/REVIEW-r1.md','rr2_i':'artifacts/rust-tauri/R05/RR2/G-R2/I-MAPPING.md','rr2_negative':'artifacts/rust-tauri/R05/RR2/G-R2/NEG-GATE-RR2.md'
}
for p in refs.values(): assert (ROOT/p).is_file(),p
artifact_hashes={p:sha(ROOT/p) for p in refs.values()}
audit=json.loads((ROOT/refs['historical_audit']).read_text())
historical=[{'stage':r['stage'],'result_ref':r['path'],'sha256':r['sha256'],'overall':r['overall'],'candidate_stable':r['candidateStable'],'runner_status':r['runnerStatus'],'stable_checkpoints':sum(x['stable'] for x in r['checkpoints']),'checkpoints':len(r['checkpoints']),'commands_passed':sum(x['status']=='PASS' for x in r['commands']),'commands_total':len(r['commands']),'changed_paths':sorted(set(p for x in r['checkpoints'] for p in x['changedPaths']))} for r in audit]
packages={
 'A':{'status':'SELF_CHECKED','independent_review':'PENDING','note':'A-02仅补shell Git查询异常拒绝；24/24自检绿。A-REVIEW-01 FAIL永久保留；新A复验未签收。','evidence_ref':refs['a2']},
 'B':{'status':'CLOSED','independent_review':'PASS','note':'B-REVIEW-01独立PASS，无包级mustFix。有效N03不代表当前N01–N16全跑。','evidence_ref':refs['b_review']},
 'C_F46':{'status':'SELF_CHECKED','independent_review':'PENDING','review_execution':'RUNNING（新CLI联合独立验收已启动，尚无本轮可签收结论）','note':'F46完整160负载2/2自检绿；C旧measurement-01的2/2因最终4日志超3不证明全部资源PASS。','evidence_ref':refs['f46']},
 'D':{'status':'BLOCKED','independent_review':'PENDING','note':'真实非回环r00 0/1/0/0 exit101；确切系统操作仅准备未执行，当前身份不能冒充最终构建身份。','evidence_ref':refs['d']},
 'E':{'status':'SELF_CHECKED','independent_review':'PENDING','note':'文档实施自检，不作独立签收；交全新E审查者。','evidence_ref':R+'E-01/REPORT.md'},
 'G':{'status':'NOT RUN','independent_review':'PENDING','note':'新默认N01–N16及必要新增负测尚未执行；历史16/16保留。'},
 'FINAL':{'status':'NOT RUN','independent_review':'PENDING','result_ref':None,'tested_sha':None,'note':'不预造新FINAL目录、结果、最终SHA。'}
}
current={'round':'RR3','recorded_by':'rr3_e_impl_01（外部新CLI exec --ephemeral；本轮未spawn代理）','recorded_at':now(),'status_snapshot':'按RR3 E文件化brief及本轮用户给定截点，不追写正在运行的其他验收结果；正式最终结果由新文档轮补录。','stage_readiness':'NOT_ACCEPTED','R06_READY':False,'offline_gate':'FAIL','independent_review':'FAIL','independent_review_basis':'最新完成的正式阶段审查RR2/FINAL-01；RR3新阶段审查PENDING、FINAL NOT RUN。','rr3_independent_review':'PENDING','live_verification':'BLOCKED_NOT_AUTHORIZED','release_state':'NOT_IN_SCOPE','packages':packages,'historical_layers':historical,'scope_counts':{'applicable_leaves':130,'shared':119,'full':6,'deferred':5,'dual_stage':124,'dual_stage_includes_deferred':5,'original_A':16,'original_C':100,'additional_C':3,'C_total':103,'cid':93,'command':10,'original_negative':16},'raw_npm':'历史candidate raw npm exit1（3文件/6失败，seal-coordinate-lag）；base exit0。未重跑，不把合法directed/E5登记形态写为raw npm全绿。','directed_E5':'RR2 directed-no-seal-family在原明确授权内E0–E4.5通过/E5 SKIP；完整s5-full-5 exit0含E5严格已登记patch-too-large分类，登记红不转正式绿、不泛化豁免。','permissions':'仅继承RR-BLK-CREDENTIALS真实账号/LIVE最迟R10及既有平台义务；Windows/Linux本轮未验证，macOS arm64部分实测且必需r00仍BLOCKED。不新增延期。','evidence_refs':refs,'silent_window':'本实施交付即停止写全部文件；正式全链静默窗口不写主树或artifact。最终真实结果由另一新文档轮补录并新独立文档终审。'}
dump(E/'current-state.json',current)
# 大账本仅追加顶层元数据；保留全部原始条目及其字节顺序。
def append_json(name,extra):
 p=D/name;s=p.read_text();d=json.loads(s)
 for k in extra:assert k not in d,k
 pos=s.rfind('}');assert not s[pos+1:].strip()
 p.write_text(s[:pos].rstrip()+',\n'+json.dumps(extra,ensure_ascii=False,indent=2)[2:-2]+'\n}\n')
notice='rr3_current为现行状态；此前baseline/rr1/rr2、PASS、尚未终审、uncommitted、候选及绿窗字段均为原轮次历史，不覆盖最新RR2 FINAL FAIL或RR3 PENDING。原历史条目全部保留。'
for name in ['PROGRESS_LEDGER.json','R05_ACCEPTANCE_LEDGER.json','R05_TEST_MAP.json','R05_PERFORMANCE_RESULTS.json','R05_LIVE_VERIFICATION.json']:
 extra={'historical_record_notice':notice,'rr3_current':current}
 if name=='R05_TEST_MAP.json': extra['rr3_I10_mapping']={'ordinary_cancel_same_instance':[{'test':'subagent_closeout::parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap','receipt':refs['ordinary_cancel'],'exit_code':0,'passed':1,'failed':0,'ignored':0,'filtered':7},{'test':'late_result_fence::r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing','receipt':refs['late_result'],'exit_code':0,'passed':1,'failed':0,'ignored':0,'filtered':4}],'budget_408_restart_recovery':'F46 raw series：60条running，cancelSettled=0、cancelDanglingActive=60，重启后消解且零provider重执行；不是普通取消同实例恢复。','mapping_ref':refs['i10']}
 if name=='R05_PERFORMANCE_RESULTS.json':
  raw=json.loads((ROOT/refs['resource_raw']).read_text());series=raw['series'];owners=raw['ownerResourceSeries']
  extra['rr3_resource_selfcheck']={'acceptance':'SELF_CHECKED；新C-F46独立验收PENDING','receipt':refs['resource_receipt'],'raw_series':refs['resource_raw'],'raw_sha256':sha(ROOT/refs['resource_raw']),'exit_code':0,'test_summary':{'passed':2,'failed':0,'ignored':0,'filtered':0,'test_duration_seconds':335.40},'cycles':len(raw['cycleResults']),'load':raw['load'],'binary_points':len(series),'owner_points':len(owners),'service_rss_kib':[min(x['rssKiB'] for x in series),max(x['rssKiB'] for x in series)],'service_fds':[min(x['fds'] for x in series),max(x['fds'] for x in series)],'tree_rss_kib':[min(x['serviceTree']['rssKiB'] for x in series),max(x['serviceTree']['rssKiB'] for x in series)],'tree_fds':[min(x['serviceTree']['fds'] for x in series),max(x['serviceTree']['fds'] for x in series)],'binary_phases':dict(collections.Counter(x['phase'] for x in series)),'owner_phases':dict(collections.Counter(x['phase'] for x in owners)),'thresholds':raw['thresholds'],'cleanup':{k:v for k,v in raw['cleanup'].items() if k!='filesBeforeCleanup'},'boundary':'正式binary进程树及RSS/FD/TCP与进程内正式组合根owner对象补证分开；不宣称直接观测binary内部全部Tokio任务。15worker存活2进程/TCP>=2，释放1进程/TCP=0；owner45稳态permit/waiter/active/ids归零，日志≤3；持久DB及日志不是临时泄漏。','historical_failure':'C-01 measurement-01旧断言2/2但最终重启及owner稳态4日志>3；F46旧6次启动[1,2,3,4,4,4]红、新[1,2,3,3,3,3]绿；原阈值3未放宽。','negative_control':refs['resource_negative']}
 if name=='R05_LIVE_VERIFICATION.json': extra['rr3_platform_verification']={'macOS arm64':'PARTIAL：本轮本机资源自检有效，r00非回环仍BLOCKED，整门禁未新跑；历史“all offline”不再为现行结论。','Windows':'NOT VERIFIED（继承平台义务）','Linux':'NOT VERIFIED（继承真机义务）','policy_unchanged':True,'offline_scope':'FAIL／NOT_ACCEPTED；历史whatIsNotBlocked不代表当前所有offline已通过。'}
 append_json(name,extra)
# 当前交接字段与真实可消费样例；历史RR1/RR2段原样保留。
h=json.loads((D/'R05_HANDOFF.json').read_text());h['historical_record_notice']=notice;h['rr3_current']=current
h['source_sha']=HEAD;h['source_sha_semantics']='本轮只读实查HEAD，与origin跟踪引用相同；RR3未提交生产改动仍在主树。不是最终被测候选SHA、不是终审/封印回执。'
h['working_tree_digest']=inputs['digest'];h['working_tree_digest_scope']={'manifest':R+'E-01/inputhash-before.json','algorithm':inputs['algorithm'],'count':inputs['count'],'scope':inputs['scope'],'qualification':'文档前后比对该范围输入，不替代正式终审candidateSourceBinding。'}
h['protocol_version']={'name':'lingxi.wire','min_supported':1,'max_supported':1,'event_schema_version':1,'legacy_preload_api':1,'legacy_server_protocol':1,'negotiation':'最高交集；无交集version_incompatible，未知握手字段拒绝，不猜默认。','source_refs':['rust/crates/lingxi-protocol/src/handshake.rs','rust/crates/lingxi-protocol/src/wire.rs','rust/crates/lingxi-protocol/src/lib.rs']}
h['data_epoch']=1;h['storage_schema_version']=7
h['dependency_locks']={p:{'sha256':sha(ROOT/p)} for p in ['rust/Cargo.lock','rust-toolchain.toml','package-lock.json']}
h['dependency_locks']['rust-toolchain.toml']['channel']='1.98.1'
h['accepted_tasks']=[];h['accepted_tasks_semantics']='当前阶段未accepted，无可凭本交接开始R06的accepted_tasks；RR1/RR2历史任务级独立PASS保留于原账本，不等同RR3最终验收。'
h['unresolved_items']=[{'id':k,**v} for k,v in packages.items() if k!='B']
h['allowed_next_scope']={'stage':'R05_ONLY','allowed':['A补修新独立复验','C/F46完整资源新联合独立验收','最终构建身份的D独立核对与受阻项处理（系统操作另按有效授权执行）','E全新独立文档验收','冻结后G默认16项及新增反例','全新正式FINAL四层亲跑','终审后新文档轮真实补录与独立文档终审'],'forbidden':['R06实现或READY=true预写','将R05必需OPEN/SELF_CHECKED/BLOCKED延期给R06','本E轮系统/Git/对外写操作'],'release_condition':'仅原RR1 §6.1全部满足、新独立阶段PASS且无本阶段必需未关闭项；原LIVE/平台许可不扩缩。'}
h['artifact_hashes']=artifact_hashes
h['consumer_guidance_r06']['availability']='接口说明供只读准备；当前禁止R06执行。'
h['consumer_guidance_r06']['ready_to_consume']=[x.replace('usage 台账 v5','usage 台账 v7') for x in h['consumer_guidance_r06']['ready_to_consume']]
h['stage_gate_registration']['map']='rust/crates/xtask/src/stage_maps/R05.json：16A+2SUP，7命令；130适用叶=119shared+6full+5deferred，124dualStage包含5deferred（非124shared）。'
h['review_status']={'independent_review':'FAIL（最新完成正式RR2/FINAL-01）','rr3_independent_review':'PENDING','stage_review':'NOT_ACCEPTED；RR3 FINAL NOT RUN；R06_READY=false','historical_status':'原RR1“尚未终审”及RR2“READY”留于baseline/rr1/rr2历史；最新状态只读rr3_current。'}
h['consumer_contract']={
 'source_checked_at':now(),'boundary':'仅当前实际源码的接口说明及受控测试调用样例；E未运行cargo或真实供应商，不新增能力承诺。',
 'gateway':{'source':'rust/crates/lingxi-kernel/src/model_exchange.rs','request_fields':['operation','provider','model'],'signature':'ModelGatewayPort::resolve_route(&ModelRouteRequest) -> Result<ResolvedModelRoute, ModelGatewayError>','normal_call':'gateway.resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Embedding))','rule':'显式pin必须provider/model成对；route是配置快照，未知provider/能力/未配置显式拒绝，无fallback。'},
 'credentials':{'source':'rust/crates/lingxi-adapters/src/models/credentials.rs','signature':'ProviderCredentialPort::resolve(&ResolvedModelRoute) -> Future<Result<ApplicableAuth, CredentialError>>','normal_call':'credentials.resolve(&route).await','rule':'宿主独占，认证材料不返回worker/UI/账本；401经report_unauthorized(route, used)同provider协调一次刷新，不重选模型；撤销/代次栅栏仍有效。'},
 'context':{'source':'rust/crates/lingxi-kernel/src/lib.rs','fields':['principal','session_id','run_id','attempt','generation'],'rule':'RunContext由宿主认证/lineage铸造，不能从模型或worker身份主张填充。'},
 'model_turn':{'source':'rust/crates/lingxi-kernel/src/model_exchange.rs','runtime_source':'rust/crates/lingxi-service/src/runs.rs','port_source':'rust/crates/lingxi-kernel/src/ports.rs','fields':['submission','system_prompt','turn','prior','tools','deadline_unix_ms','images','max_output_tokens'],'normal_input_rust':'ModelTurnInput { submission, system_prompt: None, turn, prior: exchange.clone(), tools: tool_snapshot, deadline_unix_ms: call_deadline, images: Vec::new(), max_output_tokens: None }','normal_call':'provider.next_turn(&call_ctx, &call_id, &turn_input, &sink).await','variables':'与runs.rs真实调用一致：submission/turn/exchange由run driver维护，tool_snapshot取实时目录，call_ctx/call_id宿主铸造，sink为driver的流事件端口，call_deadline是宿主剩余预算绝对期限；不是用户自造这些权限。','rule':'正常主对话system_prompt仍None（人格业务尚未接入）；deadline实际已接线，不能沿用类型旧注释“None today”。'},
 'exchange_and_messages':{'source':'rust/crates/lingxi-kernel/src/model_exchange.rs','normalizer_source':'rust/crates/lingxi-service/src/streaming_norm.rs','assistant_fields':['call','content','tool_calls','origin'],'tool_result_fields':['tool_call_id','provider_call_id','outcome'],'rule':'工具结果携带真实宿主ToolCallId+供应商关联ID+完整Outcome；内容原顺序与opaque原位置保留。TurnOrigin(provider,model)精确匹配源；同协议族不能授权跨provider/model重放，缺来源opaque拒绝。normalize_final_message唯一终态投影，实时/历史同源；partial/Empty不伪造final。'},
 'auxiliary':{'source':'rust/crates/lingxi-adapters/src/models/auxiliary.rs','request_fields':['prompt','images','max_output_tokens','deadline_unix_ms'],'signature':'AuxiliaryExecutor::complete(&RunContext, AuxiliarySlot, &ModelCallId, &AuxiliaryRequest) -> Future<Result<AuxiliaryOutcome, AuxiliaryFailure>>','rule':'真实宿主白名单slot；turn1/无tools/无prior，共享路由凭证/permit/deadline，载荷不选择身份；失败携带usage_report/transport_attempts/served_by/served_protocol，先落账后返回worker。生产组合根LedgerWorkerCallbackTrace已接线，非Noop。'},
 'embedding':{'source':'rust/crates/lingxi-service/src/operations.rs','request_source':'rust/crates/lingxi-adapters/src/models/operations/embedding.rs','signature':'OperationService::embed(EmbeddingRequest, Option<u64>, Option<&OperationCallContext>) -> Result<EmbeddingOutcome, OperationFailure>（async）','rerank_signature':'OperationService::rerank(RerankRequest, Option<u64>, Option<&OperationCallContext>) -> Result<RerankOutcome, OperationFailure>（async）','request_fields':['inputs','dimensions','context_window','input_type'],'context_fields':['session_id','run_id','attempt','cause_ref'],'normal_sample_source':'rust/crates/lingxi-service/tests/r05_t07_rr1_usage_ledger.rs::rr1_f21_operation_context_carries_session_run_and_cause','normal_sample_rust':'let context = OperationCallContext { session_id: "sess_rr1_ctx".into(), run_id: "run-rr1-ctx".into(), attempt: Some("run-rr1-ctx#a1".into()), cause_ref: Some("run-rr1-ctx-tc0007".into()) };\nlet result = state.operations().unwrap().embed(EmbeddingRequest { inputs: vec!["context-bound input".into()], dimensions: Some(2), context_window: None, input_type: Default::default() }, None, Some(&context)).await;','sample_precondition':'原具名受控测试boot隔离ServiceState+配置embedding路由+测试自有loopback服务；使用对应类型import。此为原测试实际调用片段，非独立可执行程序，非生产账号外发样例。','normal_response':{'data':[{'index':0,'embedding':[0.25,0.75]}],'usage':{'prompt_tokens':5,'total_tokens':9}},'normal_processing':'成功结果向量[0.25,0.75]保持输入索引；账行Succeeded+真实session/run/cause及开始/结算时刻。已知input=5，output缺失仍未知，不能用total9减5猜4。','rule':'context只来自可信宿主；None为合法独立根，owner范围不可见。当前embed/rerank支持context，不泛化到媒体所有入口。'},
 'usage_query':{'source':'rust/crates/lingxi-kernel/src/usage.rs','storage_source':'rust/crates/lingxi-storage/src/run_store.rs','port_source':'rust/crates/lingxi-kernel/src/ports.rs','fields':['owner_user_id','session_id','run_id','purpose','model','recorded_from_unix_ms','recorded_to_unix_ms'],'normal_query_rust':'ModelUsageQuery { owner_user_id: Some(authenticated_owner_id), session_id: Some(real_session_id), run_id: None, purpose: Some("embedding".into()), model: None, recorded_from_unix_ms: None, recorded_to_unix_ms: None }','normal_call':'storage.query_model_call_usage(query).await','rule':'owner经sessions JOIN隔离；测试样例独立根context未建立sessions时仅内部unscoped查询可见，不能冒充owner验证。缺失token/attempts保留NULL，estimated不当reported；cost_basis未知，cache/reasoning含于总量的分量不再加。'},
 'error_unknown_cancel_recovery_budget':{'error':'路由/能力/凭证失败显式拒绝，零外发不虚构1次；queue-timeout具名rr1_f21_operation_queue_timeout_never_invents_an_http_attempt在已解析路由/准入前settle处记0次未知账行，不泛称所有非法输入都有行。invalid usage保留有效操作结果、诊断类型化无payload。','unknown':'ReportedUsage Unknown/Partial/Invalid保留语义；真实0与未知NULL不同。ToolOutcome Unknown/StopUnconfirmed保持不确定性，不当success；已可能外部生效不得盲重发。','cancel':'driver取消竞态/fence迟到臂落cancelled账行，attempts未知NULL或已观测真实值；不发布伪造model_call_completed。worker deadline经abandoned，run-drop RAII脱离尽力落账，失败显式日志；进程死亡仍是崩溃窗口。','ordinary_recovery_refs':[refs['ordinary_cancel'],refs['late_result']],'restart_recovery':'预算408的60running重启recovery scan消解且零provider重执行，独立于普通同实例取消恢复；已确认工具交换保留，不重做写操作；中途崩溃started无usage行仍登记边界。','budget':'主循环deadline在排队前从总预算铸造，同call刷新/重试共享剩余期限；model permit在工具执行前释放，worker callback共享QuotaManager，invocation deadline及回调数/token/prompt预算不可由载荷扩大。'}
}
dump(D/'R05_HANDOFF.json',h)
# 报告正文旧版本逐字保留；当前块在最前，完整细节新增§11。
def banner(name,body):
 p=D/name;s=p.read_text();i=s.find('\n');p.write_text(s[:i+1]+'\n'+body+'\n\n'+s[i+1:].lstrip('\n'))
short='> **RR3 当前（2026-10-07）：stage_readiness=NOT_ACCEPTED，R06_READY=false；offline_gate=FAIL、最新完成阶段 independent_review=FAIL（RR2/FINAL-01），RR3 新终审 PENDING／FINAL NOT RUN。** 本轮 E 仅文档自检，独立 E 验收 PENDING。下文既有首轮/RR1/RR2 的 PASS、尚未终审、候选及未提交描述均为各轮历史，不代表当前放行。现行细节见 [R05_REPORT §11](R05_REPORT.md#11-rr3-当前状态与交接2026-10-07)。'
for name in ['R05_REPORT.md','R05_INDEPENDENT_REVIEW.md','R05_BLOCKERS.md','R05_NEGATIVE_GATE_REPORT.md']: banner(name,short)
def link(label,p):return f'[{label}](../../../{p})'
# R05目录深度3；返回仓库根，再到真实证据。
report='''\n## 11. RR3 当前状态与交接（2026-10-07）

本节及 HANDOFF `rr3_current` 为本轮现行结论；§1–§10 原文及旧候选/“尚未FINAL”/uncommitted是历史。最新**已完成**正式终审仍 RR2/FINAL-01 FAIL，不能说“只剩 ALF”。RR3 FINAL **NOT RUN**，没有新结果路径或最终被测 SHA。当前观察 HEAD `'''+HEAD+'''` 与 origin 跟踪引用同值；包含未提交 RR3 生产改动。本 E 轮未提交/推送、未执行系统操作、未spawn代理、未启动R06。

| 当前范围 | 状态与限制 | 实际证据 |
|---|---|---|
| A/F42 | A-02仅shell查询异常拒绝补修，自检24/24绿；独立复验PENDING。A-REVIEW-01 FAIL永久保留 | '''+link('A-02',refs['a2'])+'；'+link('旧独立FAIL',refs['a1_review'])+''' |
| B/F45 | B-REVIEW-01独立PASS、包级CLOSED；有效N03正/负/恢复对照不等于新16/16 | '''+link('B独立审查',refs['b_review'])+''' |
| C/F27 + F46 | F46完整160自检2/2绿，新CLI联合独立验收RUNNING但结论PENDING | '''+link('C说明',refs['c'])+'；'+link('F46自检',refs['f46'])+''' |
| D-01 | BLOCKED：真实非回环0通过/1失败/0ignored/0filtered，exit101；确切操作只准备未执行 | '''+link('D实际测量',refs['d'])+'；'+link('准备单',refs['d_prepared'])+''' |
| E/F28 | 当前文档实施SELF_CHECKED，独立验收PENDING，不自签PASS | '''+link('本轮REPORT',R+'E-01/REPORT.md')+''' |
| G默认16 + 新FINAL | 均NOT RUN；需冻结后新独立执行与四层有效证据 | 原N01–N16义务完整保留，历史绿不继承 |

### 11.1 最新完成终审的跨层失败（历史保留，非新终审）

'''
report+='| 层 | 命令通过 | overall | candidateStable | checkpoint稳定 | 原始结果SHA256 |\n|---|---|---|---|---|---|\n'
for r in historical:report+=f"| {r['stage']} | {r['commands_passed']}/{r['commands_total']} | {r['overall']} | {str(r['candidate_stable']).lower()} | {r['stable_checkpoints']}/{r['checkpoints']} | `{r['sha256']}` |\n"
report+='\n'+link('逐层原始核对',refs['historical_audit'])+'；'+link('正式R05结果',refs['rr2_final'])+'。R04全部8 checkpoint漂移的是外层 `r04_regression_gate/stdout.log`；R03全部15 checkpoint漂移的是父层 `r03_regression_gate/stdout.log`。顶层7/7稳定与runner PASS不能覆盖嵌套来源漂移；workspace101、R04 gate1原样保留。RR2 STAGE_REVIEW及旧“仅ALF”结论作为当时报告保留，当前已由原始层结果纠正。\n'
report+='''
### 11.2 资源、自检与普通取消的边界

C旧measurement-01虽cargo2/2、exit0，但最后重启及owner稳态真实4日志超过原上限3，旧断言漏检，历史资源全绿结论不继承。F46旧6次启动[1,2,3,4,4,4]真实红；修后[1,2,3,3,3,3]绿；新完整负载2/2、0ignored/0filtered、335.40s仅为自检。

160轮=60预算取消+60错误+15正常+10长响应（512KiB）+15真实worker，2会话；54正式binary采样点、61进程内owner对象点。服务RSS27328–36656 KiB/FD15–19，进程树RSS27328–39232/FD15–22；15worker存活点2进程且TCP≥2，释放点1进程/TCP0。owner15活跃+45稳态（每轮3×100ms），稳态active/permit/waiter/IDs归零；最后重启日志3，owner45稳态≤3，无临时残留且服务/worker真实回收。保留DB/日志属合法持久产物，不作泄漏。正式binary外部进程观测与进程内正式组合根owner补证分开，不夸称直接测得binary全部内部任务。

'''+link('完整命令回执',refs['resource_receipt'])+'、'+link('原始160及资源序列',refs['resource_raw'])+'、'+link('实际运行身份',refs['runtime_binding'])+'、'+link('伪FD/TCP零值负控',refs['resource_negative'])+'（各exit101点名，恢复0）。\n\n原I10普通取消同实例恢复已有两具名测试：`subagent_closeout::parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap`（新C定向1/0/0/7、exit0）及 `late_result_fence::r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing`（1/0/0/4、exit0），见'+link('I10准确映射',refs['i10'])+'及对应命令回执。预算408负载cancelSettled=0、cancelDanglingActive=60，60条running须重启消解且零provider重执行；不是普通同实例取消恢复，也不是永久泄漏。\n'
report+='''
### 11.3 保留范围与下一步

权威范围130=119shared+6full+5deferred；124dualStage包含5deferred，不能误称124shared。16A、103C（100原+3追加；93cid+10command）、27套件+64lib钉及16原负测身份不改；本轮不写pins、scope或原任务书。N03维护缺口F45已独立关闭，不能作为维护债转R06；新G默认16仍未跑。

raw npm candidate历史exit1（3文件/6失败，seal-coordinate-lag）与base exit0保留，E未重跑。原授权directed-no-seal-family E0–E4.5绿、E5 SKIP合法；RR2完整s5-full-5 exit0的E5严格已登记patch-too-large形态属登记红，不当raw npm全绿或豁免扩大。LIVE仍BLOCKED_NOT_AUTHORIZED（RR-BLK-CREDENTIALS最迟R10）；Windows/Linux本轮未验，macOS arm64部分实测，必需r00仍BLOCKED。无新增R05必需项延期。

D当前二进制SHA c5975a452505bd4f90fcf87509902f812148f809b21674302e6dbd34886daa6a、CDHash 6eadd46c232408547f08e305c792f1c4c2614a94；监听0.0.0.0:50220，192.168.3.5登录读0/20秒，真实exit101非supervisor timeout。同路径permitted不证明当前binary有效；C差分LAN红/Apple Python绿支持应用相关过滤，不把ALF内部机制或唯一根因写成已证明。准备的remove/add/unblock仅对应当前确切程序，未执行；最终重链须新身份核对，不以本准备单代替最终验证。

§6.2消费字段及源码正常调用片段已补入 [HANDOFF](R05_HANDOFF.json)：wire1、event1、data_epoch1与存储schema7分轴，真实锁文件哈希及范围限定工作树摘要；ModelGateway/Credential、ModelTurnInput/ExchangeItem/opaque来源、辅助/embedding/context、usage查询、错误/unknown/取消/恢复/预算都有当前源码定位。`accepted_tasks=[]`（阶段未accepted），仅允许R05剩余修复/独立验收/G/FINAL；不得凭接口准备启动R06。usage v7、LedgerWorkerCallbackTrace生产接线、embed/rerank可选宿主上下文已纠正。

正式静默窗口前本E停止所有文件写入；新FINAL真实结果由另一新文档轮补录，并以生产输入相等及真实testedSha重新独立核验，不伪造最终SHA。总控RR3矩阵/进度/交接仍由总控唯一维护，本轮不修改。
'''
with (D/'R05_REPORT.md').open('a') as f:f.write(report)
with (D/'R05_BLOCKERS.md').open('a') as f:f.write('''\n## 8. RR3 当前阻断（2026-10-07）

stage_readiness=NOT_ACCEPTED／R06_READY=false。A2独立复验PENDING、C/F46新联合独立验收PENDING、D非回环BLOCKED且准备操作未执行、E新独立验收PENDING、G默认16及RR3 FINAL NOT RUN，均须在R05处理。历史“只剩ALF”不完整：RR2 FINAL同时有R04 8/8与R03 15/15来源漂移；C旧4日志超3也曾是真实缺口，F46虽自检绿尚未独立关闭。B/F45独立PASS不意味着默认全16通过。

§5旧invalid usage缺口归属为历史，后由RR1 F21/F22完成持久记账及诊断安全，不再把必需功能挪R06；WORKER生产trace已接Ledger，usage当前v7。I10普通同实例取消恢复与预算408重启消解分别引用现行报告§11.2。raw npm登记红、合法directed/E5、LIVE/平台原许可保持；新义务不得扩大延期。D具体真实身份、限制、未执行准备及当前跨层失败见[R05_REPORT §11](R05_REPORT.md#11-rr3-当前状态与交接2026-10-07)。
''')
with (D/'R05_NEGATIVE_GATE_REPORT.md').open('a') as f:f.write('\n## RR3 当前负测状态（2026-10-07）\n\n'+link('B-REVIEW-01',refs['b_review'])+'已独立PASS：N03 normal8/0/0/113，动态计数24→23唯一变异，目标0/1/0/120 exit101、字节还原1/0/0/120 exit0。F45 CLOSED，无包级mustFix；不作为R06维护债。新默认N01–N16及新增反例由G冻结后执行，当前**NOT RUN**，历史16/16+6/6原文保留而不继承。A补修自检24/24不代替独立签收；C/F46资源负控各101及恢复0只是其自检，联合独立结论PENDING。全局NOT_ACCEPTED／R06_READY=false，raw npm红及directed/E5原许可见现行报告§11.3。\n')
with (D/'R05_INDEPENDENT_REVIEW.md').open('a') as f:f.write('\n## RR3 文档实施注记（不构成新独立审查）\n\n本文原首轮PASS为历史，已由RR1对抗审查撤销继承；RR2 FINAL正式FAIL及嵌套来源漂移见[R05_REPORT §11](R05_REPORT.md#11-rr3-当前状态与交接2026-10-07)。RR3 B独立PASS；A2、C/F46、D/E及新G/FINAL未最终独立签收。本注记由rr3_e_impl_01写，E自检不是独立验收，不能替审查者写PASS。原审查者亲跑记录逐字保留。\n')
# 当前语义是接口契约，最小修正旧错误，不重排历史演进。
p=D/'WORKER_MODEL_BOUNDARY.md';s=p.read_text().replace('版本：2026-10-03／1.0。状态：T06 实现登记（工作树候选，未提交）。','版本：2026-10-07／RR3 当前源码核对。原 T06 未提交描述为历史；阶段 NOT_ACCEPTED，R06_READY=false。').replace('| usage 聚合账本 | R05-T07（T06 仅在回执/结果上携带 usage 原始事实） |','| usage 聚合账本 | R05-T07 已实现 v7；成功/失败/取消事实经真实生产端持久化，当前阶段验收未通过 |').replace('| worker 回调的 trace 持久化（`WorkerCallbackTracePort` 生产端为 Noop） | T07（测试以记录型端口钉住父子关联） |','| worker 回调的 trace 持久化 | 生产组合根 `lib.rs` 已注入 `LedgerWorkerCallbackTrace`（storage+clock），成功/失败先落账后回话；deadline/drop经 `abandoned`，RAII脱离落账为尽力语义，非 Noop |')
s+='\n## 7. RR3 源码核对与交接\n\n`rust/crates/lingxi-service/src/lib.rs` 的worker组合块实际 `Arc::new(workermodel::LedgerWorkerCallbackTrace::new(storage, clock))`；`workermodel.rs`/`workerrpc.rs` 实现成功/失败及abandoned事实。旧Noop说明已过期，不能留到R06补必需trace。主模型permit在工具执行前释放，回调仍共享配额/截止时间及宿主白名单。真实端口字段和最小调用片段见[HANDOFF](R05_HANDOFF.json) consumer_contract；本轮仅源码与已有证据核对，自检不独立签收。LIVE/平台边界不变。\n';p.write_text(s)
p=D/'MODEL_USAGE_SEMANTICS.md';s=p.read_text().replace('版本：2026-10-03。','版本：2026-10-07／RR3 当前源码核对（stage_readiness=NOT_ACCEPTED，R06_READY=false）。').replace('每次真实模型请求（物理 HTTP 请求）都在 `model_call_usage` 台账中可追溯：','每个逻辑 ModelCall 在 `model_call_usage` 台账中记一条结算事实；其实际物理 HTTP 请求次数由 `transport_attempts` 表达（同一次401刷新重试仍是同一行，见§5）。不是每个物理请求另造一行：').replace('（`origin=operation`，无 session/run 归属——属内部记账，owner 范围查询看不到）。','（`origin=operation`）。`embed/rerank` 可选可信宿主 `OperationCallContext` 承接 session/run/attempt/cause_ref；未传上下文才是合法独立根，owner范围不可见。媒体其他入口不可据此宣称同样支持上下文，见§7.2。')
s+='\n## 11. RR3 消费与证据边界\n\n当前schema为v7（migration `model_call_usage_rr1_f38_attempts_nullable`），与data_epoch=1、wire=1分开；生产worker `LedgerWorkerCallbackTrace`已接线。真实 `EmbeddingRequest`/`OperationCallContext`/`ModelUsageQuery`字段、正常调用及错误/未知处理在[HANDOFF](R05_HANDOFF.json) consumer_contract，正常样例来自 `r05_t07_rr1_usage_ledger::rr1_f21_operation_context_carries_session_run_and_cause` 实际源码，不冒充E新跑。prompt_tokens=5,total_tokens=9缺output不能猜4。\n\n普通取消同实例恢复已有 `subagent_closeout::parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap` 和 `late_result_fence::r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing` 新C定向回执；预算408的60条running在重启后消解属于另一恢复义务，不能混作普通取消证据。新资源160自检绿、C/F46独立结论PENDING，原崩溃窗口/脱离尽力写序边界不改变。现行失败/许可见[R05_REPORT §11](R05_REPORT.md#11-rr3-当前状态与交接2026-10-07)。\n';p.write_text(s)
# 总控进度只改当前根字段与R05区域，其他任务和阶段保持深度相等。
op=ROOT/'docs/rust-tauri/ORCHESTRATOR_PROGRESS.json';o=json.loads(op.read_text())
o['current_head']=HEAD;o['current_head_note']='RR3 E只读实查HEAD=origin跟踪引用；尚有RR3未提交生产改动。最新正式RR2 FINAL FAIL，R05 NOT_ACCEPTED、R06_READY=false；当前HEAD不是最终testedSha或封印。旧RR2候选记录保留于R05历史段。'
o['current_task']='R05 RR3 E文档实施自检，交全新独立E验收；A2/C-F46复验PENDING，D BLOCKED，G默认16/FINAL NOT RUN；不进入R06。'
s=o['stages']['R05'];s['status']='NOT_ACCEPTED';s['stage_verdict']='FAIL';s['stage_acceptance_commit_sha']=None;s['R06_READY']=False;s['blockers']=['A2独立复验PENDING','C/F46独立验收PENDING','D真实非回环BLOCKED，准备系统操作未执行','E独立文档验收PENDING','G默认16及RR3 FINAL NOT RUN'];s['rr3_current']=current
for k,t in o['tasks'].items():
 if k.startswith('R05-'):
  t['historical_orchestrator_placeholder']={'status':t['status'],'acceptance_results':t['acceptance_results'].copy(),'note':'原占位，不否定已存在RR1/RR2任务级独立证据；当前不能继承为RR3正式接受。'}
  t['status']='IN_PROGRESS';t['execution_result']='IMPLEMENTED_PENDING_RR3_REVALIDATION';t['acceptance_results']={a:'PENDING_RR3_REVALIDATION' for a in t['acceptance_results']};t['rr3_status_ref']='docs/rust-tauri/R05/R05_HANDOFF.json#rr3_current'
dump(op,o)
dump(E/'implementation.json',{'recordedAt':now(),'status':'IMPLEMENTED_PENDING_SELF_CHECK','changed_owned_files':[p for p in base['owned'] if sha(ROOT/p)!=base['owned'][p]],'optional_interface_evolution_changed':False,'production_changed_by_E':False,'report_detail_ref':'docs/rust-tauri/R05/R05_REPORT.md §11','evidence_hashes':artifact_hashes})
print('IMPLEMENTED: owned changes',sum(sha(ROOT/p)!=v for p,v in base['owned'].items()),'input files',len(entries),'scoped digest',inputs['digest'])

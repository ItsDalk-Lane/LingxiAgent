import collections,datetime,hashlib,json,re,subprocess,tomllib
from pathlib import Path
ROOT=Path(__file__).resolve().parent
MAIN=ROOT.parents[4]
CANDIDATE=Path('/Users/study_superior/.codex/worktrees/r06-w06-review3/LingxiAgent')
def read(name):return json.loads((ROOT/name).read_text())
def write(name,value):(ROOT/name).write_text(json.dumps(value,ensure_ascii=False,indent=2))
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
freeze=read('freeze-01.json');owned=freeze['owned']
assert {p:sha(CANDIDATE/p) for p in owned}==owned
assert {p:sha(MAIN/p) for p in owned}==owned
assert sha(MAIN/'.vscode/settings.json')==freeze['protected_settings_sha256']
checks=sorted(ROOT.glob('final-*.json'))
runs=[json.loads(p.read_text()) for p in checks if 'command' in json.loads(p.read_text())]
for run in runs:
    assert run['status']=='PASS' and run['exit_code']==0,run['tag']
    assert run['owned_hashes_before']==run['owned_hashes_after']==owned,run['tag']
    assert Path(run['cwd'])==CANDIDATE
    assert sha(ROOT/'logs'/f"{run['tag']}.log")==run['evidence_hash']
assert read('final-four-c.json')['passed_count']==25
assert read('final-regression.json')['passed_count']==77
four={
 'C01':{'expected':'block/flow/merge/别名等有效false都关闭真实记忆注入，不默认为true','actual':'正常block、flow及历史merge/显式bool/quotedkey均HTTP200，真实provider请求无记忆nonce，SQLite completed','tests':['positive_block_disabled_real_request','c01_flow_disabled_real_request','c01_c02_c03_c04_real_binary_authenticated_requests'],'evidence':['final-four-c.json','final-family-R01/probe-results.json','final-family-R02/probe-results-v2.json']},
 'C02':{'expected':'名称#、转义、中文冒号、注释、BOM/CRLF按标准保真','actual':'真实gather与HTTP/Run请求检查相同名称；quote/escape/Unicode正常控制通过，原配置读后不改写','tests':['c02_quoted_hash_gather_preserved','c02_c03_standard_semantics_and_fail_closed_gather','c01_c02_c03_c04_real_binary_authenticated_requests'],'evidence':['final-four-c.json']},
 'C03':{'expected':'深层同名键不提升，语法/敏感类型/重复与坏merge来源错误显式拒绝且无写入','actual':'所有旧复杂键/重复/坏merge/标签历史及P03 local/binary/date/depth/alias反例均在读写入口拒绝；Run failed零wire，Gateway零proposal/installed，文件原件不变','tests':['p01_owned_complex_keys_fail_closed_across_wire_and_shared_preview','p02_literal_keys_tags_and_config_roundtrip_use_real_wire_and_http','p03_tags_cannot_erase_type_and_unsupported_data_never_rewrites'],'evidence':['final-four-c.json','final-r01-contract-results.json','final-family-R02/semantic-check-results-v2.json','final-family-R02/tagged-check-results-v2.json']},
 'C04':{'expected':'缺文件、空、部分、现役样本默认正确；只改指定字段且未知兼容值保存重载保真','actual':'默认与config.example脱敏样本正常通过，未知nested/literal键、标准binary/timestamp/set/omap/pairs及现役数值保存重载按实际原loadConfig/saveConfig相等；错误原件不写回','tests':['c04_incumbent_sample_defaults_and_unknown_fields_survive_updates','p03_standard_unknown_types_roundtrip_without_erasure','p03_delete_receipts_match_real_files_and_preflight'],'evidence':['final-four-c.json','final-incumbent-typed-results.json','incumbent-schema-binding.json']}}
write('four-c-self-check.json',{'status':'SELF_CHECK_PASS','candidate_digest':freeze['working_tree_digest'],'four_c':four,'independent_review':'NOT_RUN_NEW_REVIEWER_REQUIRED'})
matrix=read('family-repair-evidence-matrix.json')
for rc in matrix['root_causes']:
    if rc['id']!='RC08':rc['P03']['status']='SELF_CHECK_PASS_ON_FROZEN_CANDIDATE'
    rc['P03']['original_scope']='完整原义务不由P03验收，原owner/deadline保持'
for path in matrix['paths']:
    path['P03']['status']='SELF_CHECK_PASS_ON_FROZEN_CANDIDATE'
    if path['id']=='P09':path['P03']['current_chain']=['full current and peer configuration preflight','checked bundle read','actual remove directory','reread/atomic scrub writes','checked bundle save','honest success/partial effect error']
matrix['current_status']='READY_FOR_INDEPENDENT_REREVIEW'
matrix['final_candidate']='W06-CANDIDATE-03'
matrix['final_binding']='binding-after-final.json'
write('family-repair-evidence-matrix.json',matrix)
scope=read('original-obligation-review.json')
step_actual={
 'R06-T01-S01':('真实配置采集到artifact再到HTTP模型wire：人格名称/locale/Yuan、记忆开关、技能目录保持实际顺序与来源；历史完整性另属F01/F07','final-four-c.json'),
 'R06-T01-S02':('闭环wire与observed内容同源回归通过；本轮配置异常前置失败、预算回归通过；全artifact新模型另属原owner','final-regression.json'),
 'R06-T01-S03':('配置动态内容及技能启停回归通过，static prefix golden未改变；下一Run刷新F05仍OPEN，工具常驻目录F07仍OPEN','final-regression.json'),
 'R06-T01-S04':('中文/英文模板、语言fallback及子代理裁剪原回归通过；配置BOM/CRLF/转义/坏敏感类型实际入口通过','final-regression.json'),
 'R06-T01-S05':('原中英文golden、segment token预算与prompt观测回归通过；未新增模板段；完整迁移差异报告/行为评估未由本轮完成','final-regression.json'),
 'R06-T06-S01':('配置读取、memory master switch、experience gate及tenet真实Gateway正常/异常边界通过；原事实/梦境/整理回归通过，后台调度未验','final-four-c.json'),
 'R06-T06-S02':('原A11多Agent物理隔离、技能来源/白名单回归通过，peer异常拒绝、共享/陌生材料保存不变','final-regression.json'),
 'R06-T06-S03':('配置技能读写、runtime目录、preview、install Gateway和DELETE真实文件/收据全链通过；包格式整族EX03、其它安装形式未验','final-four-c.json'),
 'R06-T06-S04':('原4 hooks回归通过；上游F08已接受并在basebc2，完整Pi扩展逐项迁入尚属原owner而非本配置修复','final-regression.json'),
 'R06-T06-S05':('原A12 install确认token/TTL/风险与工具越界拒绝回归通过；本次故障不改授权模型','final-regression.json'),
 'R06-T01-D01':('既有ContextCompiler的配置份额及真实wire已验证；没有以配置修复宣布整个交付物验收','final-four-c.json'),
 'R06-T01-D02':('既有ContextArtifact字节/预算/观测原回归通过，完整交付仍由原owner','final-regression.json'),
 'R06-T01-D03':('模板golden回归通过；完整提示词差异报告本轮NOT_RUN，不用回归数量替代','final-regression.json'),
 'R06-T06-D01':('既有MemoryService配置读写、experience、tenet Gateway全族边界和原隔离回归通过','final-four-c.json'),
 'R06-T06-D02':('既有SkillService全配置消费者与安装/删除真实副作用收据通过；未知包生命周期不扩大自免','final-four-c.json'),
 'R06-T06-D03':('未改PI_REPLACEMENT_MATRIX，完整矩阵更新本轮NOT_RUN',''),
 'R06-A01':('原closed_loop sent_wire_system_bytes_match_the_observation_digests及配置真实Run/SQLite回归通过；完整A01全供应商/全部组成仍未本轮验收','final-regression.json'),
 'R06-A02':('原20 kernel budget/token/golden及service预算失败回归通过；本轮无模板段变化，完整新旧请求开销评估未由配置份额关闭','final-regression.json'),
 'R06-A11':('原a11多Agent事实/原则/工具绑定及权限拒绝回归通过；配置peer拒绝与陌生材料原件不变','final-regression.json'),
 'R06-A12':('原a12安装风险确认、过期与一次性token及Gateway越界实际回归通过；后置写入失败如实报告已安装但启用失败','final-regression.json')}
leaf_map={
 'AGENT-CONFIG-UPDATE-8C8083':('P03/P12：只改memory/experience/workspace与skill lists的保存重载及错误不写回已验；原chat/mixed/global/完整runtime代次未验',['final-four-c.json','final-incumbent-typed-results.json']),
 'AGENT-MEMORY-TOGGLE-0B483D':('P03/P01：master switch false/true真实请求及保存重载通过；disabledSince/reenableAt/ticker与mixed全局部分成功本轮NOT_RUN',['final-four-c.json']),
 'AGENT-WORKSPACE-SKILL-POLICY-UPD-23172C':('P03/P06/P07：原发现策略/禁用列表在真实wire目录生效及写后新session重载通过；同session刷新F05及mixed全局本轮未关闭',['final-four-c.json','final-family-R02/effects-probe-results-v2.json']),
 'AGENTS-ID-CONFIG-READ-BC675C':('P03：配置flags真实HTTP正常/坏类型读取通过；完整旧config GET秘密/供应商/GC投影未验',['final-four-c.json']),
 'AGENTS-ID-IDENTITY-READ-A9B0BB':('P05：实际identity GET依赖配置locale/Yuan，local/binary/日期敏感类型错误拒绝；原persona fallback回归通过，完整原界面变体未验',['final-four-c.json','final-regression.json']),
 'AGENTS-ID-EXPERIENCE-FEED-245D72':('P04：真实feedback遇坏配置400并保持旧经验；正常feedback shape回归通过，原全部up/down/duplicate变体未逐叶验收',['final-four-c.json','final-regression.json']),
 'AGENTS-ID-EXPERIENCE-PUT-DCF083':('P04：真实PUT坏配置400且旧经验不变；暂停403与正常分类文档回归通过，广播/完整索引变体未逐叶验收',['final-four-c.json','final-regression.json']),
 'AGENTS-ID-EXPERIENCE-READ-C8380C':('P04：真实GET正确experience gate/literal<<和坏配置拒绝，正常经验回归通过',['final-four-c.json','final-family-R02/additional-family-results-v2.json']),
 'TOOL-TENET-PROPOSE-38AD68':('P10：生产Gateway读取master switch，暂停无pending、坏类型/depth无pending；原原则隔离回归通过，完整批准/拒绝生命周期不据本条关闭',['final-four-c.json','final-regression.json']),
 'AGENT-SKILL-BUNDLE-DISABLE-A7B51C':('P06：同配置writer/runtime来源/白名单bundle正常回归；未知compat字段保存重载与错误原件不变，完整逐成员变体未逐叶验收',['final-four-c.json','final-regression.json']),
 'AGENT-SKILL-BUNDLE-ENABLE-52055F':('P06：同配置writer/runtime来源/白名单bundle正常回归；未知compat字段保存重载与错误原件不变，完整逐成员变体未逐叶验收',['final-four-c.json','final-regression.json']),
 'AGENT-SKILL-DISABLE-9A4766':('P06/P07：真实PATCH坏配置拒绝，正常whitelist/disable_model_invocation原回归及wire关闭通过；同session刷新F05仍OPEN',['final-four-c.json','final-regression.json']),
 'AGENT-SKILL-ENABLE-D5128A':('P06/P07：真实PATCH坏配置拒绝，正常目录/启用原回归及wire启用通过；同session刷新F05仍OPEN',['final-four-c.json','final-regression.json']),
 'AGENT-WORKSPACE-SKILL-DISABLE-778CBD':('P06/P07：workspace_disabled只改owned后重载及project wire禁用通过，原shadow/read-only回归通过',['final-four-c.json','final-regression.json']),
 'AGENT-WORKSPACE-SKILL-ENABLE-268C22':('P06/P07：workspace_disabled只改owned后重载及project wire正常控制通过，原shadow/read-only回归通过',['final-four-c.json','final-regression.json']),
 'AGENTS-ID-SKILLS-CLEANUP--6EA03A':('P08：真实preview正常共享技能不入orphan，badpeer拒绝/零写入；陌生文件保持，不据本条宣称整个助手删除已验',['final-family-R01/effects-probe-results.json','final-family-R02/effects-probe-results-v2.json','final-four-c.json']),
 'SKILL-INSTALL-AND-ENABLE-AGENT-0F7D12':('P11：生产Gateway正常installed且enabled、坏配置零安装、真实后置写入失败installed但Failed收据，原物理配置读回通过',['final-four-c.json']),
 'SKILLS-AGENTS-ID-SKILLS-PUT-B097BD':('P06：真实PUT坏配置拒绝并原件不变，正常full set与可见交集原回归通过；同session刷新F05仍OPEN',['final-four-c.json','final-regression.json']),
 'SKILLS-SKILLS-NAME-DELETE-2CAF7E':('P09：真实DELETE七家族，正常目录/引用移除，坏peer/bundle/IO前置保全，后置失败500真实已删及未清引用；陌生文件原件不变；原历史可能ok语义由F06明确错误收据修正',['final-four-c.json','logs/final-four-c.log']),
 'SKILLS-SKILLS-READ-8E11FA':('P06：实际runtime目录使用同解析器，原技能HTTP whitelist/shadow/read-only回归；完整原路由/UI身份投影未逐叶验收',['final-regression.json']),
 'TOOL-INSTALL-SKILL-3236B0':('P11：生产Gateway配置preflight及post-install失败收据，原风险token/TTL/安装边界回归通过；完整包格式EX03未关闭',['final-four-c.json','final-regression.json'])}
for record in scope['records']:
    previous=record['P03_configuration_share'];expected=record['original_clause'];actual='本条完整原义务未执行；无配置份额的实际逐条证据，不借Task级77项或邻近路径宣称通过';evidence=[];status='NOT_RUN_ORIGINAL_OWNER_RETAINS_SCOPE'
    if record['id'] in step_actual:
        actual,ev=step_actual[record['id']];evidence=[ev] if ev else [];status='EXECUTED_PARTIAL_CONFIG_REGRESSION' if evidence else 'NOT_RUN_ORIGINAL_OWNER_RETAINS_SCOPE'
    elif record['id'].startswith('R06-AR-F06:'):
        actual='本F配置修复全部四C和12真实路径最终冻结自检通过，等待全新独立Reviewer03';evidence=['four-c-self-check.json','family-repair-evidence-matrix.json'];status='SELF_CHECK_PASS_INDEPENDENT_NOT_RUN'
    elif record['kind']=='r00_leaf_assertion':
        for key,(message,ev) in leaf_map.items():
            if key in record['id']:actual=message;evidence=ev;status='EXECUTED_CONFIG_SHARE_FULL_ASSERTION_NOT_ACCEPTED';break
    elif record['id'].startswith('R06-AR-F08:'):
        actual='F08已由总控独立PASS并精确纳入basebc2；本配置修复只跑受影响hook回归，未另签F08验收';evidence=['final-regression.json'];status='INHERITED_UPSTREAM_F08_ACCEPTANCE_WITH_REGRESSION_ONLY'
    elif record['id'].startswith('R06-AR-F05:'):
        actual='同session下一Run旧prompt失败继续OPEN/STALE；新session重载仅证明F06，不代替本F缓存四C';evidence=['../W06-P02/initial-four-c-01.json'];status='OPEN_STALE_OTHER_OWNER_F05'
    record['P03_configuration_share']={**previous,'status':status,'expected':expected,'actual':actual,'evidence':evidence,'full_original_status':'UNCHANGED_NO_P03_FULL_ACCEPTANCE'}
for leaf in scope['leaf_groups']:
    related=[r['P03_configuration_share'] for r in scope['records'] if r['id'].startswith(leaf['id']+':ASSERT-')]
    leaf['P03_configuration_share']={'status':'EXECUTED_PARTIAL_CONFIG_SHARE' if any(x['evidence'] for x in related) else 'NOT_RUN_IN_P03','assertion_rows':[r['id'] for r in scope['records'] if r['id'].startswith(leaf['id']+':ASSERT-')],'full_leaf_status':'NOT_ACCEPTED','original_owner_deadline':'原字段完整保留，无延期或豁免'}
assert len(scope['records'])==354 and len(scope['leaf_groups'])==101
scope['P03_status_counts']=dict(collections.Counter(r['P03_configuration_share']['status'] for r in scope['records']))
scope['whole_original_acceptance']='NOT_ACCEPTED_FULL_TASKS_AND_LEAVES_REMAIN_WITH_ORIGINAL_OWNERS'
write('original-obligation-review.json',scope)
old=tomllib.loads(subprocess.check_output(['git','show','HEAD:rust/Cargo.lock'],cwd=CANDIDATE,text=True))['package'];new=tomllib.loads((CANDIDATE/'rust/Cargo.lock').read_text())['package']
registry=lambda data:{(p['name'],p['version'],p.get('source','')) for p in data if p.get('source')}
assert not (registry(old)-registry(new))
write('dependency-review.json',{'status':'PASS','old_registry_triples_removed_or_upgraded':[],'new_registry_triples':sorted(registry(new)-registry(old)),'incumbent_schema':'js-yaml 4.3.1 DEFAULT_SCHEMA via actual config-loader.ts loadConfig/saveConfig, no custom schema','mature_boundary':'granit-parser complete event syntax, serde_yaml_ng native typed scalar and apply_merge, serde_saphyr typed Deserialize plus public Tagged serializer; narrow compatibility adaptation applies only already parsed scalar nodes','schema_adapter':'timestamp uses exact installed resolver pattern on scalar output only; binary retains typed raw payload validated by exact original resolver; numbers use mature native scalar interfaces with original-schema string/negative-zero/explicit-overflow decisions proven through actual original load/save/load; no YAML source scan/regex preprocessing/self-written float or date constructor'})
raw=(ROOT/'logs/final-four-c.log').read_text()
receipts=[line for line in raw.splitlines() if line.startswith(('p03_delete_wire ','p03_delete input=','p02_install=','p03_late_install '))]
assert len([r for r in receipts if r.startswith('p03_delete_wire ')])==7
write('actual-side-effect-receipts.json',{'status':'ASSERTIONS_PASS','source':'logs/final-four-c.log','source_sha256':sha(ROOT/'logs/final-four-c.log'),'records':receipts,'claims':'记录实际HTTP/ToolOutcome与磁盘状态；后置失败只报部分完成，不假回滚。'})
processes=subprocess.check_output(['ps','-eo','pid,ppid,args'],text=True).splitlines()
own_live=[r for r in processes if '/target/debug/lingxi-service ' in r and any(s in r for s in ['lingxi-review-','p03-delete-','p02-install-switch-','lingxi-w06-'])]
assert not own_live,own_live
binary=CANDIDATE/'rust/target/debug/lingxi-service'
aggregate=hashlib.sha256(''.join(p+'\0'+owned[p]+'\n' for p in sorted(owned)).encode()).hexdigest()
assert aggregate=='ff5e2a83231ae6f6f306a7558d9c801fffdb66667d47da7e9fff986d1aa51e35'
write('binding-after-final.json',{'status':'FROZEN_SOURCE_UNCHANGED_ALL_FINAL_CHECKS_PASS','time_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'main':str(MAIN),'candidate':str(CANDIDATE),'HEAD':subprocess.check_output(['git','rev-parse','HEAD'],cwd=CANDIDATE,text=True).strip(),'owned_sha256':owned,'root_aggregate_algorithm':'sorted path + NUL + lowercase sha256 + LF','root_aggregate':aggregate,'executor_json_aggregate':freeze['working_tree_digest'],'service_binary_sha256':sha(binary),'target_not_symlink':not (CANDIDATE/'rust/target').is_symlink(),'protected_settings_sha256':sha(MAIN/'.vscode/settings.json'),'live_owned_fixture_processes':own_live,'all_final_run_sources_stable':True,'original_two_tests_exact_P02_bytes':True,'git_mutation':'P03 did not stage/commit/push; root owns exact Git'})
write('results.json',{'status':'READY_FOR_INDEPENDENT_REREVIEW','candidate':'W06-CANDIDATE-03','binding':'binding-after-final.json','checks':[{'name':r['tag'],'status':r['status'],'exit_code':r['exit_code'],'tests':r['test_count'],'evidence':r['tag']+'.json'} for r in runs],'four_c':'four-c-self-check.json','root_cause_paths':'family-repair-evidence-matrix.json','original_scope':'original-obligation-review.json','historical_integrity':'historical-integrity-final.json','independent_review':'NOT_RUN_NEW_REVIEWER03_REQUIRED','open':['F05 same-session prompt/cache refresh OPEN/STALE; new-session reload only F06','F01/F07/F13/EX03/EX12/F14/F15 and complete original tasks/leaf behavior retain original owners/deadlines; P03 does not self-exempt','LIVE providers, otherOS, packaging and R07 NOT_RUN; only external-model loopback and synthetic owned homes used'],'upstream_F08':'Independent accepted bc2 retained; original hooks regression PASS','platform':'macOS actual offline; no Daybreak Reviewer/platform workaround'})
print(json.dumps({'status':'READY_FOR_INDEPENDENT_REREVIEW','checks':len(runs),'scope_status_counts':scope['P03_status_counts'],'binary':sha(binary)},ensure_ascii=False))

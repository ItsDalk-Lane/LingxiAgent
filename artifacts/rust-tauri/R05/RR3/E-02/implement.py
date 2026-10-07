from capture import *
import copy,re
# 本轮仅精确替换现行字段；大账本旧条目原始字节保持不动。
def replace_key(text,key,value,indent=2):
 pat=re.compile(r'^'+ ' '*indent +json.dumps(key)+r': ',re.M); ms=list(pat.finditer(text)); assert len(ms)==1,(key,len(ms))
 start=ms[0].end();_,length=json.JSONDecoder().raw_decode(text[start:])
 rendered=json.dumps(value,ensure_ascii=False,indent=2);rendered=rendered.replace('\n','\n'+' '*indent)
 return text[:start]+rendered+text[start+length:]
def add_key(text,key,value,indent=2):
 assert key not in json.loads(text); tail=text.rfind('\n}')
 rendered=json.dumps(value,ensure_ascii=False,indent=2).replace('\n','\n'+' '*indent)
 return text[:tail]+',\n'+' '*indent+json.dumps(key)+': '+rendered+text[tail:]
def link(label,p):return f'[{label}](../../../{p})'
def read(p):return (ROOT/p).read_text()
def change_md(name,old,new):
 p=D/name;s=p.read_text();assert old in s,(name,old[:80]);p.write_text(s.replace(old,new,1))
old=load(D/'R05_HANDOFF.json');current=copy.deepcopy(old['rr3_current']);refs=current['evidence_refs']; t=now()
newrefs={'a_review':R+'A-REVIEW-02/REVIEW.md','a_manifest':R+'A-REVIEW-02/manifest.json','cf46_review':R+'C-F46-REVIEW-01/REVIEW.md','cf46_manifest':R+'C-F46-REVIEW-01/MANIFEST.json','cf46_result':R+'C-F46-REVIEW-01/RESULT.json','cf46_resource_receipt':R+'C-F46-REVIEW-01/resources-01/command.json','cf46_resource_raw':R+'C-F46-REVIEW-01/resources-01/f27-resource-series.json','cf46_analysis':R+'C-F46-REVIEW-01/RESOURCE_ANALYSIS.json','cf46_runtime_binding':R+'C-F46-REVIEW-01/resources-01/runtime-binding.json','cf46_negative':R+'C-F46-REVIEW-01/sampler-negative-isolated/result.json','cf46_ordinary_cancel':R+'C-F46-REVIEW-01/ordinary-subagent/command.json','cf46_late_result':R+'C-F46-REVIEW-01/ordinary-late/command.json','d_review':R+'D-REVIEW-01/REVIEW.md','d_manifest':R+'D-REVIEW-01/evidence-manifest.json','d_current_prepared':R+'D-REVIEW-01/PREPARED-ALLOW.md','d_current_object':R+'D-REVIEW-01/prepared-object.json','d_current_receipt':R+'D-REVIEW-01/r00-formal-01.json','e_first_fail':R+'E-REVIEW-01/REVIEW.md','e_first_manifest':R+'E-REVIEW-01/manifest.json','e_round2':R+'E-02/REPORT.md','e_consumption':R+'E-02/manifest-consumption.json'}
refs.update(newrefs)
current.update(recorded_by='rr3_e_impl_02（全新CLI exec --ephemeral；未派代理/外发）',recorded_at=t,status_snapshot='读取实际已完成独立报告、原始结果与逐文件manifest摘要；E-01派发截点仅为历史。本current消费A/B/C-F46 PASS及D新身份BLOCKED；G运行中，不预造结论。',silent_window='本轮仅限定生产输入前后相等；G真实运行并写evidence，不宣称全树静默或freeze。交付后E停止写现行文档；正式全链静默由总控另安排，新FINAL结果须新文档轮及新独立验收。')
p=current['packages'];p['A']={'status':'CLOSED','independent_review':'PASS','issues':['F42'],'mustFix':[],'note':'A-REVIEW-02 A1/A2同包独立PASS；首轮FAIL保留。完整正式嵌套全链stable/所有checkpoint仍待新FINAL，不作阶段accepted。','evidence_ref':refs['a_review'],'manifest_ref':refs['a_manifest'],'review_sha256':sha(ROOT/refs['a_review'])}
p['C_F46']={'status':'CLOSED','independent_review':'PASS','review_execution':'COMPLETED','issues':['F27-RR3','F46'],'mustFix':[],'note':'C-F46-REVIEW-01联合独立PASS：真实2/2 exit0、337.41s、160轮/54binary/61owner/15存活worker，全部原阈值；旧6轮超3红→字节还原绿。旧C漏检FAIL保留，完整阶段仍待FINAL。','evidence_ref':refs['cf46_review'],'manifest_ref':refs['cf46_manifest'],'review_sha256':sha(ROOT/refs['cf46_review'])}
obj=load(ROOT/refs['d_current_object']);p['D']={'status':'BLOCKED','independent_review':'PASS（仅定位及精确操作准备）','required_gate':'FAIL','exit_code':101,'test_summary':{'passed':0,'failed':1,'ignored':0,'filtered':0},'note':'D-REVIEW-01已完成：真实r00自然101/0通过1失败，非回环20s0字节，应用过滤线索；ALF permitted不代替真绿。未改系统。D-01旧SHA仅历史；FINAL若重链接须按真实新身份重准备并新复验。','binary_identity':{k:obj[k] for k in ['absolutePath','sha256','cdhash']},'prepared_ref':refs['d_current_prepared'],'evidence_ref':refs['d_review'],'manifest_ref':refs['d_manifest']}
p['E']={'status':'SELF_CHECKED','independent_review':'PENDING','round':2,'note':'E-REVIEW-01独立FAIL（MF-E01/MF-E02）永久保留；E-02修后仅自检，交另一全新E-REVIEW-02，不自签PASS。','evidence_ref':refs['e_round2'],'historical_reviews':[{'round':1,'verdict':'FAIL','mustFix':['MF-E01','MF-E02'],'evidence_ref':refs['e_first_fail'],'sha256':sha(ROOT/refs['e_first_fail'])}]}
p['G']={'status':'RUNNING','independent_review':'PENDING','result_ref':None,'tested_sha':None,'evidence_dir':R+'G-REVIEW-01/','note':'全新G-REVIEW-01正在真实运行默认N01–N16及恢复controls，无完整独立报告/结论；部分case不当16/16。交付前重新查完成状态。'}
# E-01已经保留的全部历史层、raw npm红、范围和原许可原样继承。
notice='rr3_current为E-02现行状态；rr3_current_history为E-01历史截点，旧pending不作当前阻断。RR1/RR2及全部历史FAIL/PASS、候选/尚未终审/uncommitted原文保留；当前NOT_ACCEPTED/R06_READY=false。'
for name in ['R05_HANDOFF.json','PROGRESS_LEDGER.json','R05_ACCEPTANCE_LEDGER.json','R05_TEST_MAP.json','R05_PERFORMANCE_RESULTS.json','R05_LIVE_VERIFICATION.json']:
 path=D/name;text=path.read_text();data=json.loads(text)
 text=replace_key(text,'rr3_current',current);text=replace_key(text,'historical_record_notice',notice)
 text=add_key(text,'rr3_current_history',[{'round':'E-01','historical_only':True,'superseded_by':'E-02','snapshot':data['rr3_current']}])
 if name=='R05_HANDOFF.json':
  for k,v in {'generated_by':'rr3_e_impl_02（RR3 E第二轮文档修复，仅自检）','generated_at':t,'unresolved_items':[{'id':k,**v} for k,v in p.items() if v['status']!='CLOSED'],'allowed_next_scope':{**data['allowed_next_scope'],'allowed':['D当前必需环境阻断处理；FINAL如重链接按CargoJSON真实身份重新准备并复验（系统操作按已有授权）','另一全新E-REVIEW-02独立文档验收','消费正在执行G默认N01–N16及新增反例的真实完整独立结果','全新正式FINAL四层亲跑（A/B/C-F46包级已关闭，不重开待验）','终审后另新文档轮真实补录与独立文档终审']},'artifact_hashes':{**data['artifact_hashes'],**{q:sha(ROOT/q) for q in newrefs.values() if (ROOT/q).is_file()}},'working_tree_digest_scope':{**data['working_tree_digest_scope'],'manifest':R+'E-02/inputhash-before.json'},'review_status':{**data['review_status'],'historical_status':'RR1/RR2旧值与E-01 pending截点均为历史；当前只读rr3_current。E首轮FAIL保留，新E独立复验PENDING。'}}.items():text=replace_key(text,k,v)
 elif name=='R05_PERFORMANCE_RESULTS.json':
  text=replace_key(text,'rr3_resource_selfcheck',{**data['rr3_resource_selfcheck'],'acceptance':'历史F46-01作者SELF_CHECKED原值；现行C/F27/F46已由C-F46-REVIEW-01独立PASS，见rr3_resource_independent_review。'})
  analysis=load(ROOT/refs['cf46_analysis']);receipt=load(ROOT/refs['cf46_resource_receipt'])
  block={'status':'PASS（仅C/F27/I10及F46包级）','stage_status':'NOT_ACCEPTED','R06_READY':False,'review_ref':refs['cf46_review'],'manifest_ref':refs['cf46_manifest'],'receipt':refs['cf46_resource_receipt'],'raw_series':refs['cf46_resource_raw'],'raw_sha256':sha(ROOT/refs['cf46_resource_raw']),'exit_code':receipt['exitCode'],'test_summary':{'passed':2,'failed':0,'ignored':0,'filtered':0,'test_duration_seconds':337.41},'cycles':160,'binary_points':54,'owner_points':61,'ranges':analysis['ranges'],'tree_ranges':analysis['treeRanges'],'owner_tree_ranges':analysis['ownerTreeRanges'],'binary_phases':analysis['seriesPhaseCounts'],'owner_phases':analysis['ownerPhaseCounts'],'original_thresholds':data['rr3_resource_selfcheck']['thresholds'],'checks':analysis['checks'],'cleanup':analysis['cleanup'],'negative_control':refs['cf46_negative'],'boundary':data['rr3_resource_selfcheck']['boundary']}
  text=add_key(text,'rr3_resource_independent_review',block)
 elif name=='R05_TEST_MAP.json':
  mapping=copy.deepcopy(data['rr3_I10_mapping']);mapping['ordinary_cancel_same_instance'][0]['receipt']=refs['cf46_ordinary_cancel'];mapping['ordinary_cancel_same_instance'][1]['receipt']=refs['cf46_late_result'];mapping['independent_review']=refs['cf46_review'];mapping['budget_408_restart_recovery']='C-F46-REVIEW-01 raw：60条running、cancelSettled=0、cancelDanglingActive=60，重启后消解且零重执行；不是普通取消同实例恢复。'
  text=add_key(text,'rr3_I10_mapping_history',data['rr3_I10_mapping']);text=replace_key(text,'rr3_I10_mapping',mapping)
 elif name=='R05_LIVE_VERIFICATION.json':
  platform=copy.deepcopy(data['rr3_platform_verification']);platform['macOS arm64']='PARTIAL：C/F27/F46已新独立PASS；D-REVIEW-01定位准备PASS但必需r00自然101/BLOCKED，G运行中、FINAL未跑。不能写全offline已通过。';text=replace_key(text,'rr3_platform_verification',platform)
 path.write_text(text)
path=ROOT/'docs/rust-tauri/ORCHESTRATOR_PROGRESS.json';text=path.read_text();orch=json.loads(text);stage=copy.deepcopy(orch['stages']['R05']);stage['rr3_current_history']=[{'round':'E-01','historical_only':True,'superseded_by':'E-02','snapshot':stage['rr3_current']}];stage['rr3_current']=current;stage['blockers']=['D必需r00非回环FAIL/BLOCKED，定位准备PASS不能解除环境','E首轮独立FAIL保留；E-02仅SELF_CHECKED，待另一新独立验收','G默认N01–N16 RUNNING/PENDING完整结论','RR3 FINAL NOT RUN；A/B/C-F46包级已CLOSED，不等于完整全链accepted']
text=replace_key(text,'R05',stage,4);text=replace_key(text,'current_task','R05 RR3 E-02修后SELF_CHECKED，待另一全新E独立验收；A/B/C-F46 CLOSED，D定位准备PASS但必需BLOCKED，G RUNNING，FINAL NOT RUN；不进入R06。');path.write_text(text)
# 当前入口只指向真正现行段；旧正文和历史层结果不改。
change_md('R05_REPORT.md','# R05_REPORT — 模型协议、凭证、流式处理与完整任务闭环（RR1 修复轮版本；§10 为 RR2 增补）','# R05_REPORT — RR3 当前状态与交接（§11；RR1/RR2历史保留）')
change_md('R05_REPORT.md','本轮 E 仅文档自检，独立 E 验收 PENDING。','A/B/C-F46包级CLOSED；D定位准备PASS但必需检查BLOCKED；G RUNNING，E-02仅修后SELF_CHECKED待另一新独立验收，E首轮独立FAIL保留。')
change_md('R05_REPORT.md','§10 由 R05 RR2 收口（WP-F）于 2026-10-06/07 增补——RR2 轮现状与阶段状态以 §10 为准，§1–§9 保留 RR1 轮历史原文。','§10 由 R05 RR2 收口（WP-F）于 2026-10-06/07 增补——§1–§10均为RR1/RR2历史原文；真正当前阶段状态以§11及HANDOFF rr3_current为准。')
change_md('R05_REPORT.md','| A/F42 | A-02仅shell查询异常拒绝补修，自检24/24绿；独立复验PENDING。A-REVIEW-01 FAIL永久保留 | '+link('A-02',R+'A-02/REPORT.md')+'；'+link('旧独立FAIL',R+'A-REVIEW-01/REVIEW.md')+' |','| A/F42 | CLOSED，A-REVIEW-02 A1/A2同包独立PASS，无mustFix；首轮FAIL保留，正式全链仍待FINAL | '+link('最新A独立PASS',refs['a_review'])+'；'+link('旧独立FAIL',R+'A-REVIEW-01/REVIEW.md')+' |')
change_md('R05_REPORT.md','| C/F27 + F46 | F46完整160自检2/2绿，新CLI联合独立验收RUNNING但结论PENDING | '+link('C说明',R+'C-01/REPORT.md')+'；'+link('F46自检',R+'F46-01/REPORT.md')+' |','| C/F27 + F46 | CLOSED，C-F46-REVIEW-01联合独立PASS，无mustFix；完整160与原阈值通过，不代阶段PASS | '+link('联合独立PASS',refs['cf46_review'])+'；'+link('原始结果',refs['cf46_resource_receipt'])+' |')
change_md('R05_REPORT.md','| D-01 | BLOCKED：真实非回环0通过/1失败/0ignored/0filtered，exit101；确切操作只准备未执行 | '+link('D实际测量',R+'D-01/REPORT.md')+'；'+link('准备单',R+'D-01/PREPARED-ALLOW.md')+' |','| D当前 | D-REVIEW-01定位/精准准备PASS，但必需r00自然exit101（0/1/0/0）仍BLOCKED；新身份准备未执行，FINAL重链接须重新核对 | '+link('D最新独立审查',refs['d_review'])+'；'+link('新身份准备',refs['d_current_prepared'])+' |')
change_md('R05_REPORT.md','| E/F28 | 当前文档实施SELF_CHECKED，独立验收PENDING，不自签PASS | '+link('本轮REPORT',R+'E-01/REPORT.md')+' |','| E/F28 | E-REVIEW-01独立FAIL保留；E-02修后SELF_CHECKED，待另一全新E-REVIEW-02，不自签PASS | '+link('本轮修复',refs['e_round2'])+'；'+link('首轮独立FAIL',refs['e_first_fail'])+' |')
change_md('R05_REPORT.md','| G默认16 + 新FINAL | 均NOT RUN；需冻结后新独立执行与四层有效证据 | 原N01–N16义务完整保留，历史绿不继承 |','| G默认16 | RUNNING，无完整独立结论；部分case不能当16/16，新FINAL仍NOT RUN | 原N01–N16及新增反例义务完整保留，历史绿不继承 |\n| 新FINAL | NOT RUN，结果与最终testedSha留空；包级PASS不等于完整正式四层通过 | 原§5.3及§6.1完整保留 |')
change_md('R05_REPORT.md','### 11.2 资源、自检与普通取消的边界','### 11.2 最新独立资源证据与普通取消的边界')
# 保留作者原335.40s自检数值并明确历史，与新独立337.41s不混。
old_resource='F46旧6次启动[1,2,3,4,4,4]真实红；修后[1,2,3,3,3,3]绿；新完整负载2/2、0ignored/0filtered、335.40s仅为自检。'
new_resource='F46旧6次启动[1,2,3,4,4,4]真实红；修后[1,2,3,3,3,3]绿；F46-01作者2/2、335.40s为历史自检。最新C-F46-REVIEW-01已亲跑2/2、0ignored/0filtered、exit0、337.41s，独立PASS。其完整160/54binary/61owner，服务RSS27408–49184KiB/FD15–19、树RSS27408–51760/FD15–22，115点日志≤3及34参与PID回收均有原始证据；正式binary与进程内owner边界不混。下面原27328–36656等数值仅为F46-01作者历史自检，保留不改为新运行数值。'
change_md('R05_REPORT.md',old_resource,new_resource)
change_md('R05_REPORT.md','N03维护缺口F45已独立关闭，不能作为维护债转R06；新G默认16仍未跑。','N03维护缺口F45已独立关闭，不能作为维护债转R06；新G默认16正在真实运行，无完整结论。')
start='D当前二进制SHA c5975a452505bd4f90fcf87509902f812148f809b21674302e6dbd34886daa6a、CDHash 6eadd46c232408547f08e305c792f1c4c2614a94；监听0.0.0.0:50220，192.168.3.5登录读0/20秒，真实exit101非supervisor timeout。'
replacement='D-01旧binary SHA c5975a452505bd4f90fcf87509902f812148f809b21674302e6dbd34886daa6a／CDHash 6eadd46c232408547f08e305c792f1c4c2614a94、旧端口50220仅为历史，不能用于当前许可。D-REVIEW-01新CargoJSON fresh=false对象SHA '+obj['sha256']+'／CDHash '+obj['cdhash']+'；实际0.0.0.0:60281、192.168.3.5登录20s0字节，真实exit101非supervisor timeout。定位和精准准备独立PASS，但必需环境仍BLOCKED。'
change_md('R05_REPORT.md',start,replacement)
change_md('R05_REPORT.md','正式静默窗口前本E停止所有文件写入；','G当前运行并写evidence，不宣称全树静默或freeze。本E-02交付后停止写现行文档；')
change_md('R05_BLOCKERS.md','stage_readiness=NOT_ACCEPTED／R06_READY=false。A2独立复验PENDING、C/F46新联合独立验收PENDING、D非回环BLOCKED且准备操作未执行、E新独立验收PENDING、G默认16及RR3 FINAL NOT RUN，均须在R05处理。历史“只剩ALF”不完整：RR2 FINAL同时有R04 8/8与R03 15/15来源漂移；C旧4日志超3也曾是真实缺口，F46虽自检绿尚未独立关闭。B/F45独立PASS不意味着默认全16通过。','stage_readiness=NOT_ACCEPTED／R06_READY=false。A/F42、B/F45、C/F27及F46均新独立PASS、包级CLOSED，无包级mustFix；不列为待验阻断。正式全链stable与全部checkpoint仍待新FINAL，历史RR2 FINAL的R04 8/8、R03 15/15来源漂移FAIL不覆盖。当前阻断为：D-REVIEW-01定位/精确准备PASS但必需r00自然101、非回环BLOCKED；E-REVIEW-01独立FAIL后E-02仅修后SELF_CHECKED，待另一新独立验收；G默认16 RUNNING无完整结论；RR3 FINAL NOT RUN。C旧4日志超3及F46旧红永久保留，已由C-F46-REVIEW-01联合独立关闭。不得把包级PASS推成阶段accepted。')
change_md('R05_INDEPENDENT_REVIEW.md','RR3 B独立PASS；A2、C/F46、D/E及新G/FINAL未最终独立签收。本注记由rr3_e_impl_01写，E自检不是独立验收，不能替审查者写PASS。','RR3 A-REVIEW-02、B-REVIEW-01、C-F46-REVIEW-01均包级独立PASS；D-REVIEW-01定位准备PASS但必需r00自然101/BLOCKED；G默认16 RUNNING无完整结论，FINAL NOT RUN。E-REVIEW-01独立FAIL（MF-E01/MF-E02）永久保留；E-02仅修后SELF_CHECKED，须另一全新E-REVIEW-02。本现行注记由rr3_e_impl_02写，不替审查者自签PASS。')
change_md('R05_NEGATIVE_GATE_REPORT.md','新默认N01–N16及新增反例由G冻结后执行，当前**NOT RUN**，历史16/16+6/6原文保留而不继承。A补修自检24/24不代替独立签收；C/F46资源负控各101及恢复0只是其自检，联合独立结论PENDING。','新默认N01–N16及新增反例由G-REVIEW-01正在真实执行，当前**RUNNING**，无完整独立结论，不将部分case当16/16；历史16/16+6/6保留。A-REVIEW-02已A1/A2同包独立PASS；C-F46-REVIEW-01联合独立PASS，资源测量假FD/TCP零负控各101、恢复0均亲跑，不再写仅自检/PENDING；两包CLOSED仍不代FINAL。E首轮独立FAIL后E-02仅SELF_CHECKED待另一新独立验收。')
change_md('MODEL_USAGE_SEMANTICS.md','新资源160自检绿、C/F46独立结论PENDING，','最新C-F46-REVIEW-01完整160及普通取消两具名回执均新亲验，联合独立PASS、包级CLOSED（旧自检/FAIL历史保留），')
change_md('WORKER_MODEL_BOUNDARY.md','行协议: kind=model.complete + cb_id + purpose + prompt + max_output_tokens','行协议: kind=callback + op=model.complete + cb_id + purpose + prompt + max_output_tokens')
wire='''
线格式包含两个独立字段：`kind="callback"`是读循环的消息分流字段，`op="model.complete"`是模型回调操作字段，不能把操作名写进`kind`。当前`WorkerCallbackLine`声明顺序为kind、cb_id、op、purpose、prompt、max_output_tokens；前3项必需，后3项有反序列化默认值，但正常调用必须满足宿主用途白名单与预算。真实`ask_model_tagged`夹具使用summarize和64 token，对应最小正常消息：

```json
{"kind":"callback","cb_id":"cb-1","op":"model.complete","purpose":"summarize","prompt":"Summarize the granted input.","max_output_tokens":64}
```

此消息以一行JSON和换行发送；前提是宿主已批准summarize并配置对应槽位，invocation、权限与截止时间由宿主管理。`workerrpc.rs`先按kind进入callback分支，再反序列化`WorkerCallbackLine`、检查身份主张/用途/预算并调用宿主模型端口；其他kind进入协议错误分支。当前分支没有另行按`cb.op`匹配操作字符串，本文不虚称新增了op校验。依据为[真实消息声明与分流](../../../rust/crates/lingxi-service/src/workerrpc.rs)、[真实worker夹具](../../../rust/crates/lingxi-service/src/bin/r04_t07_fixture.rs)。E-02只改文档，没有改接口、权限或预算，未重新运行worker进程。
'''
change_md('WORKER_MODEL_BOUNDARY.md','## 2. 信任规则（载荷不是权限）',wire+'\n## 2. 信任规则（载荷不是权限）')
dump(E/'implementation-result.json',{'utc':now(),'changed':[p for p in OWNED if sha(ROOT/p)!=load(E/'before.json')['owned'][p]],'current':current,'scope':'只现行E所有权；INTERFACE_EVOLUTION不变；历史E-01/E-REVIEW-01不改。'})
print('changed',len(load(E/'implementation-result.json')['changed']))

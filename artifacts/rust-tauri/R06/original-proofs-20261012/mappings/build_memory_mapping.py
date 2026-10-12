"""只登记本组原字面断言及实际测试位置，不签发验收结果。"""
import hashlib,json,re
from pathlib import Path
root=Path.cwd(); out=Path(__file__).resolve().parent
src=root/'artifacts/rust-tauri/R06/residual-repair-20261012/remaining-original-obligations.json'
all_items=json.loads(src.read_text())['items']
items=[i for i in all_items if i['featureId'].startswith('F-D07-') and 'R06-T05' not in i['r00TaskIds']]
new='r06_original_memory_skill_proofs'
old='r06_t06_memory'; adapters='agent_memory_r06t06'; tools='r06_t06_skills'
def proof(suite,test,lines,reason):
 package='lingxi-adapters' if suite==adapters else 'lingxi-service'
 path=f'rust/crates/{package}/tests/{suite}.rs';text=(root/path).read_text();split=text.splitlines()
 m=re.search(r'^(?:async )?fn '+re.escape(test)+r'\(',text,re.M);assert m, (suite,test)
 start=text[:m.start()].count('\n')+1
 selected=[]
 for lo,hi in lines:
  assert lo>=start,(test,lo,start)
  selected.extend({'line':n,'excerpt':'\n'.join(split[n-1:min(n+3,len(split))])} for n in range(lo,hi+1) if 'assert' in split[n-1])
 assert selected,(suite,test,lines)
 return {'package':package,'suite':suite,'test':test,'source':path,'sourceSHA256':hashlib.sha256(text.encode()).hexdigest(),'testStartLine':start,'assertionLocations':selected,'reason':reason,'executionStatus':'NOT_EXECUTED_FOR_THIS_PROOF_SET','argv':['cargo','test','--manifest-path','rust/Cargo.toml','-p',package,'--test',suite,test,'--','--exact','--nocapture']}
def n(test,lo,hi,reason):return proof(new,test,[(lo,hi)],reason)
def o(test,lo,hi,reason):return proof(old,test,[(lo,hi)],reason)
def a(test,lo,hi,reason):return proof(adapters,test,[(lo,hi)],reason)
def t(test,lo,hi,reason):return proof(tools,test,[(lo,hi)],reason)
def ready(*checks):return {'status':'READY','checks':list(checks),'missingClauses':[]}
def gap(why,*checks):return {'status':'GAP','checks':list(checks),'missingClauses':[why]}
facts='original_facts_v1_v2_import_export_and_rejections'
compiled='original_compiled_three_sections_live_legacy_and_invalid_no_write'
week='original_week_days_list_edit_sync_and_rejected_dates'
clear='original_clear_compiled_retains_raw_and_all_clear_removes_facts'
tenets='original_tenet_http_states_duplicate_limits_and_pinned_visibility'
decide='original_tenet_decisions_and_pinned_replacement_preserve_nonactive'
exp='original_experience_read_sync_paused_and_invalid_preserve_state'
feedback='original_feedback_paused_up_down_duplicate_excerpt_and_agent_isolation'
revision='original_revision_detail_reads_current_live_and_restore_keeps_agent_scope'
D={
'041F938C351F':[ready(n(revision,939,946,'真实restore响应给出所选修订，读回当前facts恢复且B不变')),gap('当前恢复没有忙状态/内容变化冲突保护的证明；未覆盖真实I/O失败500，不能由404或存储层恢复成功代替。',n(revision,913,961,'真实未知修订404、跨Agent404及拒绝后文件不变；测试还向同一restore入口发送只读凭证'))],
'04EE57C5D2BF':[gap('生产tenets_post_route不发布tenets-changed，成功广播原条款未实现。',n(tenets,528,540,'实际HTTP新增active、第二次duplicate且id相同')),ready(n(tenets,575,606,'空/非字符串400、原文件不变、指定priority可选、真实容量限制409，所属Agent范围与只读凭证拒绝'))],
'04F4C81BCBB7':[ready(n(facts,218,231,'真实v1 memories/content/date与v2 facts/time两条分别导入，核对实际计数与生产事实库和B隔离')),ready(n(facts,258,280,'无效导入400、只读凭证403、缺Agent/越界路径拒绝且已有事实数量保持'))],
'0779DAF64B03':[gap('已有具名测试没有调用unpin_memory并逐条验证active精确删除。'),gap('无实际unpin多命中/removedCount与删除项的专属断言；当前工具仅返回格式化文本。'),gap('现有工具绑定测试只覆盖search/pin，不覆盖unpin的B隔离。'),gap('未覆盖review权限和删除范围确认；现有unpin_manifest声明Execute，并非review确认链。')],
'1418E279D6FE':[ready(n(compiled,310,321,'真实循环含today，返回归一正文、today.md字节、live sections及memory.md同步')),ready(n(compiled,329,351,'同一today入口无效字段400，只读凭证403，非法Agent拒绝；前后逐文件快照不变，B视图为空'))],
'1685D5E08812':[gap('当前服务没有preferences/autolearn保存入口；原成功状态尚未实现。'),gap('包裹/扁平patch及无效body/平台拒绝均没有现行入口与专属行为证明。')],
'1CE6A92200DB':[ready(n(revision,898,904,'所选revision.before不变，连续改变实际compiled事实，两次HTTP current实时变化')),ready(n(revision,913,924,'真实未知修订与其它Agent同id均404，读取拒绝前后源文件不变；两次实时快照断言不能由缓存代替'))],
'1CFB84CEB4F8':[ready(n(tenets,544,555,'HTTP列表包含三个实际存储项，明确断言pending和rejected状态、active由相邻新增回读证明')),ready(n(tenets,582,583,'同一tenets GET未知助手404并给错误；A/B隔离由具名原测试逐助手精确内容断言'),o('a11_tenets_and_compiled_memory_scoped_per_agent',236,246,'A、B列表内容精确不同，未披露其他Agent原则'))],
'245D72DD4CEA':[ready(n(feedback,819,849,'经验暂停下up/down真实分类写入，added/duplicate/experienceEnabled逐响应断言，内容生产回读')),ready(n(feedback,832,869,'重复不新增、200字符截断、rating非法/缺excerpt400，已有文件不变；B经验空，只有读权限同接口被拒'))],
'38AD68BFB5B3':[gap('现有具名工具测试只覆盖暂停/损坏配置时tenet_propose，不覆盖合法提案经真实工具成为pending且未进入上下文。',a('a11_tenets_are_isolated_per_agent',127,137,'存储add_proposal及decide可用，不能代替真实提案工具返回pending/tenetId')),ready(n(decide,634,645,'真实decide入口批准为active、拒绝为rejected，active读回只含批准项')),gap('现有add_proposal存储隔离证明不等于真实tenet_propose工具在A/B视角不串用。',a('a11_tenets_are_isolated_per_agent',127,137,'存储层A提案和决定未进入B')),gap('未有合法提案前后空内容/重复/跨Agent工具全部不变的完整检查。',proof('r06_ar_f06_config','adversarial_real_memory_tool_rechecks_flow_and_corrupt_config',[(594,606)],'真实工具在关闭和损坏配置下不落pending，保留已有局部拒绝成果'))],
'396F817018F6':[gap('清除事实与五个编译产物及重置标记有真证据；总结来源清理没有实现（memory.rs说明属于尚未迁移summary store），不能将五文件清空冒充全部整理来源清空。',n(clear,480,495,'compiled-only不清原始事实；all-clear清事实且留下reset真实文件')),ready(n(clear,500,502,'实际ghost删除404、错误可见、现有编译文件保持；同一compiled删除只读权限拒绝不改文件'))],
'3C8CE915255E':[gap('memory-cache observation删除入口当前服务未实现。'),gap('无真实删除或拒绝后的观察记录回读证明；不能用其它记忆删除替代。')],
'4338B197B6DF':[gap('现有pin_memory工具写active和材料gather读取active分别可证明，但没有同一真实pin后立即采集/发送该条目上下文的专属检查。',t('a11_memory_tools_are_bound_to_one_agent_at_construction',598,621,'真实pin工具写B且PII脱敏，保留局部成果')),ready(t('a11_memory_tools_are_bound_to_one_agent_at_construction',606,621,'实际B tenets落盘经脱敏；额外PII存储函数金样覆盖敏感模式'),a('pii_scrub_matches_incumbent_patterns',388,410,'四种敏感模式逐个redacted且原密钥前缀不在输出')),ready(t('a11_memory_tools_are_bound_to_one_agent_at_construction',606,621,'真实工具B写入，A实际文件不含B内容')),gap('原pin工具空内容/满容量/跨Agent参数全部零写入的完整组合尚缺专属证明；不能用tenets HTTP容量检查替代工具层。')],
'476AB8B09402':[ready(n(compiled,310,321,'真实facts入口返回归一内容，facts.md与memory.md同时回读')),ready(n(compiled,329,351,'无效facts字段/只读权限/非法Agent，源快照不变且返回错误'))],
'506A6F6005C1':[ready(n(compiled,293,321,'首次GET真实迁移editable-facts并移除旧文件，实际facts.md字节与原事实一致，返回sections与Markdown；后续PUT再核对memory.md实际同步，不假设GET落盘组装文件')),ready(n(compiled,337,351,'B视图无A内容，非法Agent GET错误可见且A源文件快照不变'))],
'53B672CBEA46':[gap('原始facts保全及五个编译文件清空/重置标记可证；原要求的总结来源清理尚未迁移，不能标完整成功。',n(clear,480,489,'真实compiled DELETE返回成功，保留1原始事实、五文件空和reset marker')),ready(n(clear,500,502,'无效Agent拒绝后已有文件不变；compiled删除读权限凭证也被拒绝'))],
'628005EDFF37':[gap('现有context_notes HTTP检查不走工具append，追加只增新块未证明。'),gap('现有HTTP读回未重开会话/工具实例，不能替代重开会话断言。',o('context_notes_session_scoped_roundtrip',539,544,'已保留同会话立即读回正确正文')),gap('现有cap断言没有在超限后再读旧值，完整保全未证明。',a('context_notes_cap_and_roundtrip',574,587,'存储超限报CONTEXT_NOTES_OVERSIZE，读侧截断；并未读回原sess-1旧值')),gap('工具空写入返回Success文本拒绝，不是原要求的错误结果；无会话/16KiB需要真实工具面完整检查。')],
'7432B00DEC94':[ready(t('a11_memory_tools_are_bound_to_one_agent_at_construction',566,585,'B绑定真实search工具搜不到A，注入agentId经schema拒绝')),gap('当前search文本区分空库与无命中，但不输出实际semantic检索状态；not_configured被特意略去，缺未检索与无事实正式区分。'),gap('未配置模型/超时/覆盖不足的semantic降级当前无完整真实路径及状态输出；不能把FTS成功当模型检索完成。')],
'7563265D4241':[gap('PUT pinned缺agent-updated广播，未实现原成功整体条款。',n(decide,678,700,'实际替换删除已批准model提案，新增user_direct，空列表删除所有active')),ready(n(decide,679,711,'重复正文去重、非active pending/rejected保留、空数组清active、非数组400文件不变'))],
'76EE734926B4':[gap('真实当前修订列表200空库已存在，但仅空库具名测试，没有指定助手已有修订HTTP清单与隔离断言。',o('dream_faces_report_absence_and_revisions_serve_from_store',377,378,'保留真实空修订列表成功，不冒称ticker可用')),gap('原ticker不可用503与现行dream_revisions_route不依赖ticker、直接200的行为不同；不能伪造503通过。')],
'7D05B144AE21':[gap('recall_experience未注册为当前工具；只读经验HTTP不能替代该工具命中。'),gap('无真实recall工具前后文件不变证据。'),gap('无真实recall跨Agent/空结果专属证据。')],
'822AADC2B605':[ready(n(decide,643,650,'真实approve=false HTTP提案变rejected，active列表只含另一个批准项')),gap('不存在404和拒绝不变有真检查，活动/非pending/满容量限制路径原状态不变尚缺逐臂测试。',n(decide,661,663,'真实未知id404且tenets字节不变'))],
'89F253FD3567':[gap('缺省批准为active可证，尚未将该同一批准原则经真实上下文材料/编译采集证明进入提示词。',n(decide,634,635,'真实approve缺省HTTP返回active')),gap('explicit true、满model提案容量409、非pending拒绝后状态不变未逐臂覆盖；保留未知404和只读权限拒绝。',n(decide,661,663,'真实不存在404不改tenets字节'))],
'90975187F446':[ready(n(week,381,398,'真实按日PUT同步daily、week.md、memory.md，第二日保留')),ready(n(week,414,443,'不存在日404/无效日期与body400，同一入口读权限拒绝，文件不变，B隔离'))],
'99900EB5ACC9':[gap('当前tenets_delete_route不广播tenets-changed，原成功通知未实现；原具名测试也无HTTP删除后存储读回。'),gap('没有同一真实DELETE未知id404及越权零副作用专属检查。')],
'A17D742E55B7':[gap('record_experience未注册当前工具，存储写后合并不是工具写后recall证明。',a('experience_record_and_index_and_feedback_shape',273,288,'存储记录去重、正文和索引生成可证明局部')),gap('缺真实record_experience工具A/B隔离检查。'),gap('非法分类存储拒绝已有证据，写失败/跨Agent工具拒绝链未实现或未证明。',a('experience_record_and_index_and_feedback_shape',290,292,'存储../escape分类InvalidRequest'))],
'A9AD4C7F76AE':[gap('生产dream_run_route恒返503 dream_unavailable，未实现成功202启动手动dream。'),gap('ticker缺席503有证据；memory关闭/忙/已运行409路径没有当前ticker，无法证明。',o('dream_faces_report_absence_and_revisions_serve_from_store',358,367,'实际POST无ticker503；不是成功启动证明'))],
'AD0384FBE1F2':[gap('memory-cache observation读取入口未实现。'),gap('未有原观察记录范围/无效输入读回证据，不能借编译记忆GET替代。')],
'B45ECEC543C6':[gap('生产dream_status_route恒返503，无法返回ticker当前状态。'),gap('缺ticker503已证，未知Agent404和指定助手范围的完整状态检查缺失。',o('dream_faces_report_absence_and_revisions_serve_from_store',358,359,'原实际ticker不可用503错误码'))],
'C3FB70F9925E':[gap('preferences/autolearn GET当前服务未实现。'),gap('原全局自动学习包裹/扁平接口没有对应生产/原副作用检查。')],
'C75BF9A4D143':[ready(n(facts,243,249,'真实导出version=2/exportedAt/facts与完整GET列表相等')),ready(n(facts,278,280,'同一export入口未知或跨路径Agent拒绝、错误可见、已有事实数量不变'))],
'C8380CD5D08B':[ready(n(exp,726,757,'实际关闭403、开启无文件空正文，两分类以正式编号条目PUT后GET读回完整编号原文，真实索引包含两个分类')),ready(n(exp,739,795,'空正文字符串、暂停B的实际GET/PUT门控及B存储为空；同一经验写权限与非法body拒绝不改A'))],
'D38F6F02EDD7':[ready(n(compiled,310,321,'真实longterm入口返回正文，longterm.md与memory.md同步读回')),ready(n(compiled,329,351,'同一longterm无效字段400、读权限403、非法Agent拒绝，原文件保持且B不含A'))],
'D47D375E578B':[ready(n(tenets,544,564,'存储包含pending/rejected/active三种，pinned GET精确只含active正文')),ready(n(tenets,553,569,'rejected与pending没有进入pins，B pins为空且不披露A正文'))],
'DCF0836C3A16':[gap('经验分类正文与索引同步/空内容清空有证据，生产experience_put_route不刷新助手或发布刷新通知，原整体成功尚不完整。',n(exp,748,785,'真实分类文档同步、索引包含两个分类、空内容清空')),ready(n(exp,766,795,'content非字符串/非法分类400，文件不变；暂停B403，读权限拒绝'))],
'EA26288DA2DB':[ready(n(facts,222,246,'实际facts.db两条与HTTP memories全量长度和export相同，v1/v2字段回读')),ready(n(facts,278,280,'原memories读取的未知Agent与export路径形态拒绝，错误可见且已有事实保持'),o('a11_cross_agent_and_invalid_ids_are_rejected',297,312,'真实memories GET缺Agent/未知Agent/跨目录Agent拒绝'))],
'FD94F19E734D':[ready(n(week,369,370,'真实指定A days返回两个准确日期与正文，B为空')),ready(n(week,432,443,'B无A日期，ghost404错误可见，A文件快照不变'))],
}
# 只补原功能已实现且缺检查的四项；保留旧单断言依据。
new_tools='original_pin_and_propose_tools_publish_context_and_preserve_rejections'
new_decisions='original_tenet_decision_nonpending_capacity_errors_keep_store'
D['4338B197B6DF']=[
 ready(n(new_tools,1096,1111,'真实pin后同一个生产ContextCompiler立即采集发送文本，条目与脱敏结果出现，B不出现，实际存储来源为UserDirect'),n(new_tools,1123,1124,'相同正文再次调用真实pin，返回已存在且存储字节不变')),
 ready(n(new_tools,1101,1102,'同一次真实pin的实际编译文本为REDACTED、不含脱敏前密钥'),t('a11_memory_tools_are_bound_to_one_agent_at_construction',606,621,'实际tenets文件不保存原密钥，保留原工具证据')),
 ready(n(new_tools,1106,1106,'B编译文本不含A的新pin'),n(new_tools,1207,1223,'所有拒绝/容量路径后B实际记忆文件快照不变')),
 ready(n(new_tools,1131,1135,'空/空白/跨Agent参数真实工具失败或schema拒绝，Atenets字节不变'),n(new_tools,1218,1223,'真实pin工具超过200容量失败，A/B原文件均保全')),
]
D['38AD68BFB5B3']=[
 ready(n(new_tools,1149,1156,'真实tenet_propose返回实际pending和tenetId，pending内容未进入当前编译prompt')),
 ready(n(new_tools,1174,1207,'同一真实工具提案经HTTP批准active进入新prompt、旧冻结prompt不变；另一真实工具提案拒绝rejected且不入prompt')),
 ready(n(new_tools,1156,1165,'首次提案及重复/空白/跨Agent拒绝都回读B文件，与提案前一致'),n(new_tools,1207,1207,'拒绝后的B文件仍保持原值')),
 ready(n(new_tools,1164,1165,'重复提案、空白及跨Agent参数的真实工具请求不改变A/B已有原则'),proof('r06_ar_f06_config','adversarial_real_memory_tool_rechecks_flow_and_corrupt_config',[(594,606)],'原真实工具关闭/损坏配置拒绝不新增pending继续保留')),
]
D['822AADC2B605'][0]['checks'].append(n(new_tools,1201,1207,'真实提案拒绝后rejected，当前prompt无该原则，B不变'))
D['822AADC2B605'][1]=ready(n(new_decisions,1259,1263,'真实approve=false不存在404/非pending409，tenets字节及B不变；不伪造拒绝动作也受批准容量限制'))
D['89F253FD3567']=[
 ready(n(decide,634,635,'原HTTP缺省approve返回active'),n(new_tools,1174,1180,'实际explicit=true批准同一个工具提案后，当前生产编译prompt出现原则，旧冻结文本保持未批准状态')),
 ready(n(new_decisions,1259,1263,'真实model-active满容量409、非pending409、未知404均不修改存储，B为空'),n(decide,661,663,'原未知id和读权限拒绝路径保留')),
]
# 最后仅补四个指定叶中现有可证明行为；语义不符仍保留GAP。
unpin='original_unpin_exact_and_multi_match_keep_other_agent_and_nonactive'
notes='original_context_notes_append_reopen_and_oversize_preserve_old_value'
revision_list='original_nonempty_revision_lists_are_agent_scoped_and_read_only'
D['0779DAF64B03']=[
 ready(n(unpin,1555,1569,'真实unpin精确ID只移除所选active；关键词多命中只删两匹配项，未匹配和pending保留')),
 ready(n(unpin,1564,1569,'真实工具文本报告2项，包含两个实际已删正文；存储重读仅保留非匹配active与pending，实际数量准确')),
 ready(n(unpin,1570,1579,'两次真实删除及无匹配请求后，Btenets字节均保持')),
 gap('无匹配和空关键词零删除可证；原review权限与删除范围确认未实现，现有manifest为Execute；空库专属断言尚缺。',n(unpin,1577,1579,'真实无匹配/空关键词回读A/B字节不变')),
]
D['628005EDFF37']=[
 ready(n(notes,1640,1658,'真实工具write后append，工具再读及存储读回精确为原块加新块，只追加一块')),
 ready(n(notes,1665,1675,'销毁并重建工具/存储对象，以相同会话新Run读回原已落盘内容；其它会话实际为空')),
 ready(n(notes,1683,1690,'真实工具追加17KiB返回Failed；下一真实read仍为完整原值')),
 gap('超限Failed不覆盖与空写入不改文件均有实际证明；空写入生产返回Success拒绝文本而非原错误结果，无会话原工具边界尚缺，不能整项READY。',n(notes,1683,1704,'真实超限失败及随后读回原值；空写入实际拒绝文本且存储原值不变')),
]
D['76EE734926B4'][0]=ready(n(revision_list,1746,1782,'真实非空修订GET返回A两项与B一项，ID精确对应各助手落盘修订，不泄露before正文，两助手文件均不改写'))
# 仅补两项中的恢复失败证明；缺少原忙状态/内容冲突属于产品缺口，仍不通过。
restore_io='original_restore_real_io_failure_is_500_and_keeps_all_agent_files'
D['041F938C351F'][1]=gap(
 '实际HTTP恢复直接调用DreamRevisionStore.restore；当前没有原memory ticker每日维护/聚合编译忙状态或dream运行拒绝，也没有内容变化冲突判断。恢复只有当前快照/相同内容短路/pre_restore备份/文件写入；真实I/O失败500已补检查，不能以此替代原409忙/内容冲突。产品未修改，整叶保留GAP。',
 n(revision,913,961,'原未知修订404、跨Agent404、只读凭证拒绝及现有文件保全继续保留'),
 n(restore_io,1824,1833,'真实HTTP所选修订文件位置为目录，触发read_to_string I/O失败500及dream_restore_failed；当前正文及A/B所有实际文件字节保持'),
)
D['041F938C351F'][1]['gapKind']='IMPLEMENTATION'
# 生产事实只用于解释未通过，不作为执行成功凭证。
restore_implementation=[]
for source,lo,hi,reason in [
 ('rust/crates/lingxi-service/src/memory.rs',827,850,'HTTP恢复只校验Agent并调用存储restore，所有非404错误统一500；不读取忙状态。'),
 ('rust/crates/lingxi-adapters/src/storage/agent_memory.rs',1757,1809,'存储恢复没有busy/content-changed冲突条件，资料epoch是发布计数而非互斥/冲突保护。'),
 ('lib/memory/memory-ticker.ts',1101,1110,'原实现先拒绝_dailyRunning或_aggregateCompileInFlight状态，再交给dream runner。'),
 ('lib/memory/dream/runner.ts',362,367,'原runner正在运行时拒绝恢复。'),
]:
 data=(root/source).read_bytes()
 restore_implementation.append({'source':source,'sourceSHA256':hashlib.sha256(data).hexdigest(),'startLine':lo,'endLine':hi,'excerpt':'\n'.join(data.decode().splitlines()[lo-1:hi]),'reason':reason})
# 原预期结果独立于逐条断言检查；缺少结果中的额外动作也不能整项READY。
outcome_gaps={
 '04EE57C5D2BF':['尚未发布原预期tenets-changed广播。'],
 '0779DAF64B03':['真实精确ID/关键词多项删除和文本数量已有检查；当前工具仍只有格式化文本，没有原removedCount字段，不能将文本计数冒充该字段。'],
 '1685D5E08812':['自动学习设置保存入口未迁移。'],
 '396F817018F6':['总结来源清理尚未迁移，五个编译文件清空不能代替全部整理产物清空。'],
 '3C8CE915255E':['观察记录删除入口未迁移。'],
 '53B672CBEA46':['总结来源清理未迁移。'],
 '7432B00DEC94':['检索状态与semantic降级未完整实现或证明，not_configured状态被省略。'],
 '7563265D4241':['尚未发布原预期agent-updated广播。'],
 '7D05B144AE21':['recall_experience工具未注册。'],
 '99900EB5ACC9':['原预期tenets-changed广播未实现，真实HTTP删除后存储回读仍缺。'],
 'A17D742E55B7':['record_experience及后续recall工具未注册，存储局部写入和索引检查保留。'],
 'A9AD4C7F76AE':['成功202启动手动dream未实现，当前返回503不算成功。'],
 'AD0384FBE1F2':['观察记录读取入口未迁移。'],
 'B45ECEC543C6':['指定助手ticker状态未实现，当前返回503不算状态读取成功。'],
 'C3FB70F9925E':['自动学习设置读取入口未迁移。'],
 'DCF0836C3A16':['分类文档和索引同步可证，但原预期刷新助手未实现。'],
}
assert len(D)==len(items)==37
result=[]
for item in items:
 suffix=item['id'].split('-')[-1];groups=D[suffix];assert len(groups)==len(item['r00Assertions']),suffix
 enriched=[]
 for i,(assertion,g) in enumerate(zip(item['r00Assertions'],groups)):
  enriched.append({'assertionIndex':i,'assertion':assertion,**g})
 extra=outcome_gaps.get(suffix,[])
 result.append({**item,'status':'READY' if all(g['status']=='READY' for g in groups) and not extra else 'GAP','executionStatus':'NOT_EXECUTED_FOR_THIS_PROOF_SET','groups':enriched,'outcomeStatus':'GAP' if extra else 'READY','outcomeGaps':extra,'outcomeChecks':[c for g in groups for c in g['checks']],'outcomeReview':'已逐字对照r00Then，原预期结果中的额外动作单独保留缺口；READY仅指已有检查绑定，未执行也不代表通过。',**({'implementationEvidence':restore_implementation} if suffix=='041F938C351F' else {})})
report={'schemaVersion':1,'group':'memory-skills','source':str(src.relative_to(root)),'sourceSHA256':hashlib.sha256(src.read_bytes()).hexdigest(),'scope':'F-D07-* excluding r00TaskIds containing R06-T05','count':len(result),'readyCount':sum(i['status']=='READY' for i in result),'gapCount':sum(i['status']=='GAP' for i in result),'readyMeaning':'每条原断言已有准确专属检查绑定，尚待主代理统一实际执行；不是通过','formalAcceptanceComplete':False,'items':result}
path=out/'memory-skills.json';path.write_text(json.dumps(report,ensure_ascii=False,indent=2));print({k:report[k] for k in ('count','readyCount','gapCount')})

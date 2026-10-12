"""保留全部F-D21原断言；局部测试不代替完整界面或不存在的产品能力。"""
import hashlib,json
from pathlib import Path
root=Path.cwd(); out=Path(__file__).resolve().parent
# 复用同目录的定位函数，只执行定义前缀，不重写记忆映射。
helper=out/'build_memory_mapping.py'
exec(compile(helper.read_text().split('D={')[0],str(helper),'exec'))
items=[i for i in all_items if i['featureId'].startswith('F-D21-')]
new='r06_original_skills_bundle_proofs'
manual='original_global_manual_install_complete_package_without_agent_enable'
toggle='original_skill_enable_disable_runtime_and_workspace_shadow_boundaries'
external='original_external_paths_persist_readback_and_rejections_without_mutation'
bundles='original_bundles_agent_projection_update_order_delete_and_rejections'
faces='skills_http_faces_whitelist_shadow_readonly_and_bundles'
review='a12_layer1_install_review_token_protocol_end_to_end'
safe='a12_layer1_safe_review_installs_directly_marked_not_default_enabled'
ctx='r06_t06_skill_context'
D={
'0D8358645191':[ready(n(bundles,507,516,'真实按A/B视角返回每成员enabled/source/missing；另真实旧组合缺技能投影'),n(bundles,668,677,'缺文件旧组合GET显示missing=true、source=null、enabled=false')),ready(n(bundles,570,572,'同一bundles GET缺query.agentId返回400，不从焦点推断；组合持久状态不变'))],
'0F7D128E01B1':[gap('skills_install_route读取agentId但赋给_agent_id未使用，仅调用install_manual；带指定Agent的手动安装没有启用该助手/更新runtime。',n(manual,220,242,'全局安装完整目录且两个Agent enabled不变，这一证据不能冒充安装并启用')),gap('未有指定助手安装不存在/失败的真实错误和其它Agent不误启用检查；生产手动入口不验证所传Agent。')],
'268C221DAB82':[ready(n(toggle,329,345,'真实workspace enabled=true将名称移出所属workspace_disabled，B列表不变')),ready(n(toggle,359,385,'实际shadowed409、无技能404、enabled非bool400、只读凭证403，全部Agent配置字节不变'))],
'295F9F22981B':[ready(n(external,411,416,'真实GET返回PUT配置的精确绝对目录清单，与实际preferences字段一致')),ready(n(external,411,455,'无额外参数读取只返回明确配置目录；非法写与只读拒绝保持已存清单，外部资料仍只读'))],
'2CAF7E2573D8':[ready(n(bundles,646,652,'真实DELETE后目录消失，同时回读A/B enabled均移除、剩余组合skillNames删该项、无关技能保留')),ready(n(external,453,455,'真实readonly外部技能DELETE403且目录仍在'),n(bundles,657,659,'实际不存在技能404，错误明确且组合状态保持')),gap('后期引用写失败返回500和仍残留peer引用已有真实专属检查；尚未对所有助手/所有bundle引用逐个回读列风险，不能将局部peer读回说成完整引用审计。',proof('r06_ar_f06_binary','p03_delete_receipts_match_real_files_and_preflight',[(1275,1289)],'当前改进真实partial失败500、peer保留原引用；预检失败保全目录，陌生技能字节不变'))],
'3236B05B284F':[gap('真实安全审核服务安装、完整手动包文件、真实install_skill工具启用分别已有局部证明，但未有同一个经审核工具安装的完整多文件包与清单/Agent启用相符回读。',n(manual,223,233,'手动包SKILL.md与examples完整落盘；这一入口没有安全审核'),t(safe,277,285,'Safe guard结果真实安装且metadata标不默认启用'),proof('r06_ar_f06_config','p02_real_install_gateway_checks_config_before_install_and_enable',[(854,858),(861,865)],'真实工具已安装但启用晚期失败明确报告，不能把失败当完整启用')),ready(t(review,143,155,'风险首次请求要求确认且目标目录未写')),ready(t(review,173,254,'无token、内容改变、过期与重放拒绝，fresh token加显式risk_accepted成功')),gap('结构非法包/相对路径/symlink拒绝已有真实检查，工具级来源授权越权与安装I/O失败不产生可执行技能未完整逐臂证明。',a('install_package_validates_structure_and_rewrites_metadata',477,501,'缺SKILL.md、symlink、非绝对来源返回结构错误'),t(review,215,254,'所有确认失败目标不落盘；一次性确认重放明确拒绝'))],
'337738D5D233':[gap('当前未注册desk/delete-skill工作区目录删除路由；普通user技能DELETE不能代替。'),gap('无原工作区已知目录约束、递归删除失败状态与副作用专属证据。')],
'52055FF0CAC2':[gap('组合PATCH确可写入成员，但当前返回enabled而不是原要求changed，且set_enabled_skills替换全量名单可能移除无关已启用技能，原完整语义未证明。',t(faces,1001,1001,'旧真实启用组合仅检查HTTP200，保留局部不可当changed证明')),gap('不可见成员过滤后拒绝不变及changed准确值没有专属完整检查；不得由PUT白名单交集断言替代组合操作。')],
'52A5D1660088':[gap('原桌面系统目录选择接口在当前Rust/Tauri候选未迁移，暂无实际对话框路径证据。'),gap('取消空结果及选择不等于安装暂无候选原生界面专属行为证据。')],
'6E205433F474':[ready(n(bundles,548,562,'真实order PUT返回指定反序，BundlesStore实际重读同序落盘')),ready(n(bundles,585,607,'bundleIds非数组/未知id400、只读凭证403，实际组合内容不变'))],
'6EA03AC48851':[gap('已有shared预览为空与无写入证明；没有合法仅此Agent user候选的专属正对照及全清单精确回读。',proof('r06_ar_f06_binary','p01_owned_complex_keys_fail_closed_across_wire_and_shared_preview',[(667,668),(690,696)],'真实preview同伴仍启用技能不入候选，配置/技能保全；保留局部')),gap('内置/外部/plugin每来源候选排除尚无完整真实组合检查，不能用通用registry未驻或只读外部DELETE代替。')],
'771296A194E0':[gap('learn_lesson不是当前已注册Rust工具；已有install_skill不能代替课程内容创建。'),gap('未有真实learn_lesson风险未确认零写入检查。'),gap('未有真实课程索引/Agent启用状态及后续召回一致检查。'),gap('缺上下文、同名、写失败及一次性风险确认均无该原工具专属实现/证据。')],
'778CBDA01964':[gap('真实workspace disabled写入及目录enabled=false有检查，未对同一次停用后的实际模型可用技能列表/提示词采集验证不再提供。',n(toggle,329,345,'实际workspace_disabled与runtime投影enabled均跟随false/true，B不变')),ready(n(toggle,359,385,'未shadowed正臂与shadowed409/无效参数/无权限原配置不变'))],
'7803B231FC36':[gap('当前未注册skills/translate模型翻译接口，无法证明目标语言名称映射。'),gap('names/lang/agentId的原翻译路由边界没有现行专属检查。')],
'8A69F0CFFFA1':[gap('当前没有desk/install-skill至.agents/skills的路由；全局home/skills手动安装不能冒称工作区安装。'),gap('无该原desk入口越权/非法来源原目录保全的专属检查。')],
'8E11FAF4CC01':[ready(t(faces,755,804,'真实全量与runtime=1 GET分别包含user与workspace，source/enabled/shadowed等逐字段断言'),n(toggle,314,345,'修改后真实runtime列表按所属Agent反映状态')),ready(n(toggle,389,390,'skills GET无agentId真实400；不同Agent状态隔离与runtime视图由正臂证明'))],
'9A47666127D3':[gap('真实普通技能停用移出enabled且runtime=false有证明；同一次PATCH后的实际发送prompt刷新未专属证明。',n(toggle,299,315,'逐true/false真实PATCH、配置与runtime同步'),proof(ctx,'disabled_skill_never_reaches_the_production_system_prompt',[(319,333)],'空白名单初始态真实模型发送不含技能；不是对本次PATCH缓存刷新的证明')),ready(n(toggle,373,385,'未知技能/无效enabled/只读凭证，所有Agent实际配置不变'))],
'9CA8A47BE208':[gap('独立技能查看器的Rust/Tauri原生界面与列/读文件入口尚未迁移，旧Electron源码不能代替当前候选运行。'),gap('原绝对路径、目录、2MB及读取不等于安装无该候选原生接口专属证明。')],
'A3FE8FD26CB4':[ready(n(manual,220,242,'真实完整全局包存在可读，A/B enabled均维持原值，返回目录user未启用')),ready(n(manual,251,277,'相对来源/无SKILL.md包/只读权限拒绝，已有skill目录与两个Agent名单完全不变'),a('install_package_validates_structure_and_rewrites_metadata',489,501,'现有symlink结构拒绝与绝对路径合同有专属存储断言'))],
'A7B51C282C77':[gap('agent_skill_bundle_patch_route将enabled读入_enable后不使用，false也set_enabled_skills(bundle全部成员)，原停用操作未实现；没有changed结果。'),gap('原enabled=false/不可见过滤/拒绝零修改无法由当前true/false共用启用行为证明。')],
'B097BDDC1E10':[gap('实际PUT更新名单与模型发送目录已有证明，但skills-changed广播当前入口未发布。',t(faces,820,843,'真实名单PUT过滤ghost，实际配置A保留无关字段'),proof(ctx,'enabled_skill_catalog_reaches_the_production_system_prompt',[(266,300)],'真实service set_enabled后模型发送完整skill.catalog及同源观测，保留局部')),gap('交集已有真实证明，空数组停用所有可见普通技能的同一PUT接口及实际禁用投影检查未完整。',t(faces,820,830,'可见交集过滤ghost且原配置只含alpha'))],
'B0D4073E6AEB':[gap('真实DELETE删除组合且不删其技能已证；生产skills_bundle_delete_route未广播，原完整成功条款缺失。',n(bundles,615,619,'剩余组合数量1，两个成员技能文件仍在')),ready(n(bundles,624,626,'真实未知组合404、错误明确且组合存储保持'))],
'B383AB7B553C':[gap('原desk/skills GET返回workspace扫描与策略的入口当前未迁移；skills?runtime=1仅目录投影，不是desk策略接口。',proof(ctx,'runtime_skill_catalog_respects_whitelist_filter_and_workspace_shadow',[(456,477)],'已有真实runtime目录过滤/工作区来源断言，保留局部来源证明')),gap('该原desk/skills入口无实际拒绝/无效输入证据，不能以普通目录GET替代。')],
'BA490D930CA9':[gap('实际reload扫描返回数量有检查，当前skills_reload_route不发skills-changed，前端联动未证明。',t(faces,953,965,'真实reload与随后指定Agent bundles GET成功')),gap('源实现reload与install/delete共享mutation_lock；缺真实并发互斥检查及回传非Agent目录否定断言。')],
'BDEDEF118670':[gap('外部路径持久保存和重扫视图有证明，生产set_external_paths/路由无skills-changed广播。',n(external,406,424,'实际prefs清单与GET匹配，目录包含readonly外部技能')),ready(n(external,433,469,'非绝对/skillsDir本身/非数组400，原prefs不变；空数组移除全部路径与目录可见项'))],
'D5128ABAABCF':[gap('实际普通技能启用配置与runtime有证明，同一次PATCH之后实际发送prompt刷新未专属证明。',n(toggle,299,315,'真实ordinary true/false写所属enabled、live目录字段与B隔离'),proof(ctx,'enabled_skill_catalog_reaches_the_production_system_prompt',[(266,300)],'现有服务set_enabled真实发送技能目录，保留局部不代替PATCH缓存刷新')),ready(n(toggle,373,385,'真实未知/类型/只读权限拒绝，原Agent配置字节不变'))],
'D51B2A1FA68A':[gap('hana_card_guide未在当前Rust工具注册，不能产生原指南文本。'),gap('无该真实工具调用及会话卡片产物集合回读证明。'),gap('缺指南资源的显式失败未有原工具实现或专属证据。')],
'EE2ACB390FCE':[gap('show_card未在当前Rust工具注册，无原会话卡片记录/重开链。'),gap('当前候选没有该卡片iframe渲染隔离入口证据。'),gap('无真实show_card执行前后源文件不变证据。'),gap('权限拒绝不渲染/HTML记录而不假称完整语义校验没有该原工具与界面链专属证明。')],
'EFEEEFFCD60A':[gap('真实创建组合持久/返回成员状态有证据，当前create路由未广播skills-changed。',n(bundles,501,516,'真实创建两个组合后GET按A/B投影，name/skillNames实际存储后续order可重读')),gap('已证未知成员400局部；创建body.name/skillNames和缺query.agentId的同一POST拒绝不变尚未逐臂专属测试。',t(faces,974,974,'真实POST未知技能400，不能以此代替全部请求边界'))],
'F54ED990E939':[ready(n(bundles,524,539,'真实PUT分别只改name及只改skillNames，另一字段保留')),gap('未知技能PUT400不改存储已证明；同一PUT缺query.agentId及完整body边界未专属测试，不能由GET必填检查代替。',n(bundles,595,597,'真实未知成员更新400、错误明确、组合存储未变'))],
'FAE2BDEFDB04':[gap('当前未注册skills/bundles/{id}/export打包下载接口，无法构建原可下载包/元数据。'),gap('未知id/打包失败的明确错误没有该原导出入口实现和专属证据。')],
}
ui=next(i for i in items if i['id'].endswith('0DDB5AA05072'))
D['0DDB5AA05072']=[
 gap('当前候选缺该技能面板加载/empty/loading/noUser/错误toast/旧列表保留的界面证据；真实后端列表不证明页面状态。',n(bundles,507,516,'后端按Agent组合投影局部证据')),
 gap('指定Agent安装后自动启用当前未实现，桌面取消选择/toast/后端双GET重载未有候选界面测试。',n(manual,220,242,'全局安装后Agent仍不启用，不能代替面板安装启用')),
 gap('后端技能PATCH存在，UI无助手禁写/失败回滚/toast和重载投影未有当前候选证据；组合disabled还存在明确行为缺口。',n(toggle,299,385,'后端所属Agent启停与拒绝局部证据')),
 gap('后端组合CRUD有局部证据；当前候选缺UI组合树/导出showInFinder/取消/Promise.all部分成功后的实际页面证据，导出接口亦未迁移。',n(bundles,524,626,'后端改名/成员/order/delete的局部真实状态')),
]
assert len(D['0DDB5AA05072'])==len(ui['r00Assertions'])
ui=next(i for i in items if i['id'].endswith('99F8CD228733'))
ui_gaps=[
 '当前候选设置容器冷启动、ready/cache/background/15秒及provider摘要失败的界面生命周期证据缺失。',
 '按Agent加载/空助手清空/后台名称翻译及读取错误console/空态的实际UI未证明，翻译服务亦未迁移。',
 '全局完整包安装可证；文件选择/拖放path与base64分支、取消零请求、toast与runtime重载未有当前候选UI证据。',
 '实际删除与配置清理有后端局部证据，取消确认/无助手与重载和失败toast无当前候选界面证明。',
 '组合后端CRUD有局部证据，当前导出入口未迁移、showInFinder/非导出不IPC/弹窗取消/toast未证明。',
 '普通技能启停有后端证据，组合停用尚未正确实现；UI无助手禁用/失败回滚与提示未证明。',
 '外部目录PUT/GET及视图重扫有真实证据；原生选择取消不PUT/成功失败toast及重载失败catch的候选UI未证明。',
 '当前agent_config_patch_route只接T06布尔叶，不接受原learn_skills capability写入全局preferences；UI确认/autoSaveConfig后GET和保存部分失败分支未迁移或未证明。',
 '原全局goal/autolearn GET界面显示和undefined保留未有当前候选证明；此条确为只读，没有伪造PUT负控。',
 '目标预算与自动沉淀两个开关独立PUT/GET/失败回滚和toast未有该当前候选UI证明。',
 '当前候选原生技能查看器/iframe文件树、openSkillViewer/fire-and-forget/默认文件与installed真值均未迁移或未有真实界面专属证明。',
]
assert len(ui_gaps)==len(ui['r00Assertions'])
D['99F8CD228733']=[gap(reason) for reason in ui_gaps]
D['99F8CD228733'][2]['checks']=[n(manual,220,277,'真实全局包安装且不启用Agent，保留后端局部结果')]
D['99F8CD228733'][3]['checks']=[n(bundles,646,659,'真实删除目录、全部Agent和bundle引用回读，保留后端局部结果')]
D['99F8CD228733'][4]['checks']=[n(bundles,524,626,'后端组合改名/成员/order/delete局部结果')]
D['99F8CD228733'][5]['checks']=[n(toggle,299,385,'真实普通/workspace技能后端启停和拒绝局部结果')]
D['99F8CD228733'][6]['checks']=[n(external,406,469,'真实路径PUT/GET/持久化/拒绝与清空局部结果')]
# 后端HTTP变更后复用同一个正式编译器，补原提示词刷新和编辑必填的缺证据。
refresh='original_skill_patch_refreshes_same_compiler_and_frozen_previous_prompt'
D['9A47666127D3'][0]=ready(n(toggle,299,315,'原真实ordinary=false从所属enabled移出且runtime投影禁用'),n(refresh,745,750,'同一次HTTP停用后同一编译器下一Run不含该技能；旧启用文本保持原值'))
D['D5128ABAABCF'][0]=ready(n(toggle,299,315,'原真实ordinary=true进入所属enabled且runtime投影启用'),n(refresh,732,737,'同一次HTTP启用后同一个正式编译器prompt出现技能；旧未启用文本不改写'))
D['778CBDA01964'][0]=ready(n(toggle,329,345,'原真实workspace=false写workspace_disabled，B不变'),n(refresh,758,783,'同次HTTP停用后实际runtime_skill_catalog不再提供、正式prompt消失，B仍可用且旧文本保持'))
D['F54ED990E939'][1]=ready(n(bundles,595,597,'原未知技能PUT400不改组合存储'),n('original_bundle_edit_requires_agent_query_without_mutating',808,810,'实际同一组合PUT缺agentId400，error可见且组合原值不变'))
# 最后一个合并检查贯通原安装工具，不增加产品功能；外部审核结论为明确的合成边界。
install='original_install_tool_risk_token_complete_package_agent_enable_and_failures'
def ip(lo,hi,reason):return proof('r06_original_memory_skill_proofs',install,[(lo,hi)],reason)
D['3236B05B284F']=[
 ready(ip(1415,1446,'真实install_skill经显式风险确认后完整安装SKILL.md与配套examples文件，所属Agent配置及实际运行目录可见，B不启用且文件保持；外部审核结论由明确合成guard提供')),
 ready(ip(1379,1381,'真实工具首次风险提示后目标不存在且所属Agent配置不变'),t(review,143,155,'原正式审核服务requiresRiskConfirmation=true及风险提示后的零安装保留')),
 ready(ip(1391,1416,'真实工具不带token即使risk_accepted=true仍无安装，显式接受并带一次性token才安装'),ip(1459,1461,'同token真实工具重放再次拒绝，安装池和配置均保持'),t(review,173,254,'原真实审核服务内容变化/过期token拒绝和一次性确认原证明继续保留')),
 ready(ip(1470,1521,'真实工具相对/不存在来源、目录symlink越界、目标已有文件导致安装I/O失败均为Failed且不产生可执行技能，原技能池/Agent配置/其它Agent文件保全'),t(review,173,254,'正式审核服务安全未过时requiresRiskConfirmation、有效一次性token与显式确认原合同继续保留'),a('install_package_validates_structure_and_rewrites_metadata',477,501,'原包结构非法与symlink绝对来源校验继续保留')),
]
preview='original_nonempty_cleanup_preview_excludes_shared_and_readonly_sources'
D['6EA03AC48851']=[
 ready(n(preview,873,886,'真实HTTP预览A只返回一个实际仅A启用的用户技能完整清单，B空，不含shared和未启用技能'),n(preview,923,929,'相同生产预览方法结果与真实HTTP一致；技能池、助手配置、产品、外部及插件资料逐文件字节不变')),
 ready(n(preview,873,929,'真实HTTP及同源生产compute_orphan_skills排除B仍启用项、内置、外部、workspace；具名真实scan_external确认plugin输入已识别且readonly，生产预览结果仍精确只有A用户项；全部来源字节保全')),
]
# 原删除叶的全引用回读：完整清理和真正晚期写失败都检查全部助手/全部组合。
delete_refs='original_skill_delete_reads_every_agent_and_bundle_after_complete_or_partial_cleanup'
D['2CAF7E2573D8'][0]=ready(
 n(delete_refs,1023,1106,'真实HTTP删除后枚举合成HOME全部实际助手，逐一回读完整enabled/workspace_disabled和runtime投影；全部组合剩余引用精确匹配，只有目标成员的组合消失，不改无关技能资料'),
 n(delete_refs,1119,1140,'全部持久组合经HTTP再读，成员与存储逐一相等；完整清理无任何目标助手/bundle引用'),
)
D['2CAF7E2573D8'][2]=ready(
 n(delete_refs,1040,1143,'Unix真实目录写权限导致删除后首次引用atomic_write失败，HTTP明确500及清理失败码；逐项回读所有助手原字节、所有bundle完整原值和HTTP missing投影，实际残留助手/组合清单数量精确并打印，不将目录已删说成引用已清'),
 proof('r06_ar_f06_binary','p03_delete_receipts_match_real_files_and_preflight',[(1275,1289)],'保留原正常/晚期失败/预检拒绝具名证据；现行实现明确500而非伪造原可选ok，原可能返回ok条款不要求必须ok'),
)
outcome_gaps={
 '0DDB5AA05072':['当前候选缺原技能面板实际页面投影与失败状态证明；指定Agent手动安装不启用，组合停用及导出未实现。'],
 '0F7D128E01B1':['手动安装忽略指定Agent，未更新所属enabled和运行时列表。'],
 '337738D5D233':['原desk工作区技能删除及重新同步目录入口未迁移。'],
 '52055FF0CAC2':['当前返回enabled而非原changed，且全量名单替换可能移除无关已启用技能，不能视为原组合启用语义。'],
 '52A5D1660088':['原生目录选择对话框未迁移，暂无真实路径返回。'],
 '771296A194E0':['learn_lesson工具未注册，审核后写课程及后续检索链未实现。'],
 '7803B231FC36':['目标语言名称翻译入口未迁移。'],
 '8A69F0CFFFA1':['原desk/install-skill写.agents/skills入口未迁移，不能用全局技能安装替代。'],
 '99F8CD228733':['当前设置页面动作与投影缺完整证明；翻译、导出、原生查看器及全局目标/自动学习链存在未迁移功能。'],
 '9CA8A47BE208':['原生独立技能查看器列文件及读取所选文本未迁移。'],
 'A7B51C282C77':['enabled=false被忽略而写入全部成员，原组合停用未实现，也未返回changed。'],
 'B097BDDC1E10':['配置与prompt局部检查可用，但原skills-changed广播未实现。'],
 'B0D4073E6AEB':['实际删除组合且保留技能可证，但原广播未实现。'],
 'B383AB7B553C':['返回工作区扫描及策略的原desk入口未迁移。'],
 'BA490D930CA9':['扫描重载可证，但skills-changed广播及前端按Agent联动未实现或证明。'],
 'BDEDEF118670':['路径保存和重载来源可证，但skills-changed广播未实现。'],
 'D51B2A1FA68A':['hana_card_guide工具未注册，不能提供原指南文本。'],
 'EE2ACB390FCE':['show_card及当前候选隔离iframe卡片渲染链未迁移。'],
 'EFEEEFFCD60A':['组合持久保存及成员状态有局部证明，但原skills-changed广播未实现。'],
 'FAE2BDEFDB04':['组合打包下载及结果元数据入口未迁移。'],
}
assert len(items)==len(D)==33
result=[]
for item in items:
 suffix=item['id'].split('-')[-1];groups=D[suffix];assert len(groups)==len(item['r00Assertions']),suffix
 extra=outcome_gaps.get(suffix,[])
 result.append({**item,'status':'READY' if all(g['status']=='READY' for g in groups) and not extra else 'GAP','executionStatus':'NOT_EXECUTED_FOR_THIS_PROOF_SET','groups':[{'assertionIndex':i,'assertion':s,**g} for i,(s,g) in enumerate(zip(item['r00Assertions'],groups))],'outcomeStatus':'GAP' if extra else 'READY','outcomeGaps':extra,'outcomeChecks':[c for g in groups for c in g['checks']],'outcomeReview':'已逐字对照r00Then，原预期结果中的额外动作单独保留缺口；READY仅指已有检查绑定，未执行也不代表通过。'})
report={'schemaVersion':1,'group':'skills-bundles','source':str(src.relative_to(root)),'sourceSHA256':hashlib.sha256(src.read_bytes()).hexdigest(),'scope':'F-D21-*','count':len(result),'readyCount':sum(i['status']=='READY' for i in result),'gapCount':sum(i['status']=='GAP' for i in result),'readyMeaning':'每条原断言已有准确专属检查绑定，尚待主代理统一实际执行；不是通过','formalAcceptanceComplete':False,'items':result}
path=out/'skills-bundles.json';path.write_text(json.dumps(report,ensure_ascii=False,indent=2));print({k:report[k] for k in ('count','readyCount','gapCount')})

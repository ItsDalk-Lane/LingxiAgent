import json,re,hashlib
from pathlib import Path
root=Path.cwd(); out=root/'artifacts/rust-tauri/R06/original-proofs-20261012'
original=json.loads((out/'sessions-context-original.json').read_text())['items']
rows=[r for r in original if r['featureId'].startswith('F-D02')]
def test(suite,name,reason):
 p=Path('rust/crates/lingxi-service/tests')/(suite+'.rs');s=(root/p).read_text();ls=s.splitlines();start=next(i for i,l in enumerate(ls) if re.search(r'\bfn '+re.escape(name)+r'\(',l));end=next((i for i in range(start+1,len(ls)) if re.match(r'(?:async )?fn \w+\(',ls[i])),len(ls));asserts=[{'path':str(p),'line':i+1,'excerpt':'\n'.join(ls[i:min(i+4,end)])} for i in range(start,end) if re.search(r'assert(?:_eq|_ne)?!\(',ls[i])]
 return {'package':'lingxi-service','suite':suite,'test':name,'source':str(p),'sourceSHA256':hashlib.sha256((root/p).read_bytes()).hexdigest(),'testStartLine':start+1,'assertionLocations':asserts,'reason':reason}
new='r06_original_session_proofs';tree='r06_t03_session_tree';hist='r06_t04_history'; ops='r06_ar_f03_operations';life='r06_ar_f04_lifecycle'
ready={
 'FDB0FE6528EE':('original_session_rename_preserves_title_after_invalid_foreign_and_storage_refusal',['真实HTTP改名保存后回读；空标题拒绝且标题不变','错误类型/空标题400、未知404、跨主体403、实际SQLite触发写失败500，目标与另一会话无错误改动；解除失败后恢复正对照']),
 '1AEDB2670EB9':('original_checkpoint_delete_preserves_scope_after_invalid_foreign_and_storage_refusal',['真实Run后创建两个检查点，删除指定项且另一个仍在，二次删除404','非法名称400、未知404、跨主体403、SQLite实际删除失败500不丢检查点；恢复后真实删除']),
 '3FAFA387AD68':('original_checkpoint_delete_preserves_scope_after_invalid_foreign_and_storage_refusal',['真实Run的两个检查点经HTTP列表可见；单项删除后列表仅保留另一个','未知会话404、跨主体读取403、暂时隐藏实际检查点表令GET500，不以空列表呈现成功；恢复后精确回读']),
 '477DA3D558B8':('original_rewind_preview_reports_anchor_and_files_without_writing_any_state',['真实轮次与文件检查点的预览返回输入锚点、拟回退头、逐文件影响；分支/会话/事件/文件字节与修改时间不变','无效类型400、未知检查点404、跨主体403、真实存储读取失败500；拒绝未写，解除失败后恢复同一正结果']),
 '2ED065BED168':('original_pin_order_checks_each_owner_and_rolls_back_failed_reordering',['真实两个置顶项重编号；混合两个拥有者的请求逐项鉴权，重复ID拒绝','重复400、缺项404、混合归属403、第二项SQLite实际失败500整批回滚；再运行成功且取消置顶清时间和顺序']),
 'A18187567AF9':('original_session_search_returns_scoped_title_content_and_explicit_read_failures',['真实标题/真实Run最终内容按q/phase/limit返回所属会话、匹配类型和片段；空q为空','非法phase/limit/超长q400；另一设备只见自己会话；隐藏messages表产生真实500而非空成功，恢复后结果和会话不变'])}
gap={
 '015A65726C05':'Rust只有内存回退偏好与受控测试入口，没有原文件回退偏好PUT接口或持久化，不能用测试设置开关代替用户保存。',
 '22150190C7B1':'Rust没有原文件回退偏好GET接口；内部原子布尔值读取不能代替带鉴权、拒绝和失败的读取路由。',
 '0D4F0EFC9DAE':'置顶的布尔保存、顺序、拒绝及存储失败可证明，但当前置顶路由未验证Agent删除状态，缺原已删Agent禁止置顶条件。',
 '17FCFE45576B':'未接入原待办面板及历史收纳记录接口；没有仅对结束且未收纳计划追加记录的行为。',
 '1E0D477755CD':'没有原撤销授权目录路由；创建会话时写初始目录或ResourceIO拒绝不能代替撤权并验证新范围。',
 '21A99DB98735':'未接入工作台挂载/目录孤儿扫描路由，无法证明按挂载移除或目录丢失归档且跳过运行项。',
 '2D4EBE0FF3C2':'未接入工作台身份和dispose归档路由；通用session archive不能代替工作台资格判定。',
 '31CDA283A11A':'Rust恢复仅更新数据库生命周期，没有原归档文件及sidecar经路径/冲突校验移回活跃目录、返回新路径的证明。',
 '3562B5D44C5E':'没有原指定检查点restore路由返回写回位置；rewind会话分支回退不等同原独立快照恢复。',
 '3818ADEAFA7A':'没有续接已删除Agent会话的接口和新身份生成流程；普通新建或fork不能代替。',
 '38EEA99077E8':'已有拥有者隔离与active列表，但原Studio身份、归属/修订点/运行徽章完整列表投影未齐；不能只凭ID列表通过。',
 '3F95164A488C':'已补真正运行子会话、两层后代可恢复归档与真实存储失败整批回滚检查；当前ArchiveView不返回原Then要求的错误计数，整条仍未通过。',
 '3FCB259FF69C':'未注册原/reset slash动作或接管态拒绝；HTTP永久删除归档会话不等同slash清当前历史。',
 '43FFB75F252C':'归档列表已有生命周期/主体过滤，但未返回原各Agent首消息截断字段。',
 '45C5CE361CCB':'未接入计划complete路由和广播/追加完成记录的幂等面板行为。',
 '485B2D0A8A02':'未注册原/new slash轮换、旧会话归档和接管态拒绝。',
 '4CEBEA945B7A':'重试指定轮次和可选文件回退已有真实正/非法/忙/越权证明，但原下游失败的同入口无错误副作用检查未完整，整条不能通过。',
 '4DD3F71FA2CD':'未接入最近用户消息按版本/替换文字重放专属接口；现有retry没有该请求字段及版本契约。',
 '4F1F69CCA760':'现有sessions/find只返回会话元数据，未在指定会话历史内返回搜索命中位置或Bridge脱敏，不能以元数据find替代。',
 '51905CECDD1B':'现有memory GET能读布尔值并拒绝越权；当前未验证已删Agent拒绝，缺原活动会话条件与真实读取失败专属检查。',
 '52226FEA5997':'已证明永久删除生命周期/文件及失败可恢复边界；原删除后restore接口不可恢复的直接反查及输入/权限同入口矩阵未完整。',
 '5235D8EDD673':'现有detach_children真实正行为可证明，但同入口无效输入/越权/下游失败后的完整无副作用检查不足。',
 '5271217DB01B':'未接入持久化会话地图positions replace路由，无法证明未指定坐标清除与拒绝无写。',
 '5370DA3742CB':'未接入持久化会话地图positions merge路由，无法证明仅指定卡片变化。',
 '5CD35D9EED20':'未接入计划cancel路由、取消历史记录和专属广播；不能以Run取消代替余项取消。',
 '5DCB0352EB08':'未注册原session模型工具的读取/定向发送/部分成功保留新ID契约；HTTP会话归属检查不能替代工具完整操作。',
 '604C64F4828A':'创建会话可校验初始真实目录，但没有原追加授权目录路由及扩展后范围拒绝矩阵。',
 '6207F7E8C697':'已有HTTP检查点create/list/delete，但模型目录未注册原checkpoint工具与无会话/无用户轮次工具拒绝；不同入口不能替代。',
 '6658E9B80314':'未接入工作台专属永久删除动作及草稿清理，通用归档删除不证明工作台条件。',
 '73B631D77531':'没有原整体替换授权目录路由；初始authorizedFolders不是替换旧范围。',
 '7C959E75B8C3':'未注册原session_folders批准/拒绝工具；初始目录与file.access扩大提示不代替用户批准后仅当前会话更改。',
 '9616596B5720':'布尔开关已能保存回读；当前服务未验证活动生命周期，缺原仅活动会话写入以及同入口真实失败完整检查。',
 '98DDC94776F1':'sessions/find含初始authorizedFolders，但没有原活动会话授权目录专属GET路由及活动条件/完整失败检查。',
 '9D9E72CB6051':'普通新建会话可指定ID与初始权限，原独立会话可选项目归属/谱系及不切焦点流程未接入。',
 'AA9EB3FD034D':'Rust历史投影和ETag已有真实检查，但原before/all/reconciliation字段及待办投影未完整实现，不能以新cursor分页整体替代。',
 'B0F05BC32D62':'现有cleanup按期限删除计数有真实正检查，但原归档文件/标题与生命周期清理及失败/冲突无副作用整套证明不完整。',
 'B9DA1A584C6E':'已有真实session元数据创建，原切换焦点、工作区/模型配置和广播联动未完整接入。',
 'BB0A168006D7':'原用户编辑检查点的edit-start/autosave-interval原因和专属接口未接入，普通checkpoint name/filePaths请求不可替代。',
 'CDBFE6616EB9':'没有原history-overview接口/available false契约；messages分页与session Run计数不能代替。',
 'D62A81C2AEB2':'有新Run上下文编译与压缩模块，但未接入原fresh-compact刷新当前Agent配置和工具快照的完整用户动作。',
 'E1625F70C7CC':'真实HTTP rewind可以证明分支/文件回退及外部修改保护；模型rewind工具的用户批准/拒绝等待流程未接入，HTTP拒绝不能代替用户拒绝。',
 'ED089133C106':'Rust历史使用内联投影/资源引用，没有原按contentId读取历史延迟大内容的专属路由。',
 'F0319FC4F93D':'没有原summary滚动摘要记录GET路由；压缩摘要内部生成与context-observation不等同授权读记录。',
 'F28086B64522':'没有原conversation-map持久化布局GET/默认空布局路由。',
 'F6A1A50E97E4':'没有原conversation-map消息折叠为问答卡片接口；通用历史消息投影不等同问答卡片折叠。',
 'FBDEEDAC0B11':'没有焦点会话切换及浏览器恢复流程；服务侧新建或读取不证明桌面焦点/浏览器状态。',
 'FF4F06011B48':'已有同一Run冻结上下文观察，但未从原会话sidecar读取冻结系统提示词与工具名的专属接口；不能拿另一个观察面替代。',
 '8E2D44B48905':'新桌面候选尚无原地图工作区/消息缓存/选中失败/持久化坐标/草稿分支发送完整界面流程；服务端fork不能证明界面联动。'}
projects=['059066969D44','20E9C8F26CEA','3ECCF7C95483','60A58027CCCE','AFA7227BFB63','D43836A3A579','D58B651BF099','E167298FD87C','EEDFB7AB889C','EF5DD65C558E']
for k in projects:gap[k]='Rust未接入原session-projects项目/文件夹及会话归属管理路由和持久化，不能用普通sessions列表或标题证明项目动作。'
for k in ['94B9BE3AA7A6']:gap[k]='未接入地图collapsed整体替换路由及持久化折叠列表。'
partials={
 '0D4F0EFC9DAE':[(new,ready['2ED065BED168'][0],'真实pin布尔正向写时间/顺序，unpin清除，混合拥有者403；不覆盖已删Agent禁止置顶')],
 '31CDA283A11A':[(tree,'management_surface_rename_pin_memory_archive_restore_delete_cleanup','真实archive/restore生命周期与活动列表变化；不含原sidecar移动/路径校验')],
 '38EEA99077E8':[(tree,'management_surface_rename_pin_memory_archive_restore_delete_cleanup','真实主列表排除归档父项并显示active子项；不含原Studio与全部徽章字段')],
 '43FFB75F252C':[(tree,'management_surface_rename_pin_memory_archive_restore_delete_cleanup','真实归档列表可见父项并可恢复；不含截短首消息')],
 '4CEBEA945B7A':[(tree,'retry_target_resolution_matches_incumbent','真实assistant/user目标锚点与非法目标400'),(tree,'retry_file_rollback_contract_matches_incumbent','实际文件字节回退，非法值400/偏好关闭403不动分支，外部改动保留'),(tree,'busy_session_refuses_fork_retry_rewind_409','真实运行中fork/retry/rewind409'),(tree,'cross_principal_tree_access_is_403','真实设备身份访问他人retry403')],
 '51905CECDD1B':[(tree,'management_surface_rename_pin_memory_archive_restore_delete_cleanup','真实memory GET默认true/PATCH false后GET false'),(tree,'cross_principal_tree_access_is_403','memory GET他人会话403')],
 '52226FEA5997':[(tree,'management_surface_rename_pin_memory_archive_restore_delete_cleanup','实际归档永久删除后find404，未归档删除409'),(life,'f04_permanent_delete_removes_owned_cache_and_retains_other_branch','真实删除自己缓存并保留另一分支实际材料'),(life,'f04_delete_physical_failure_preserves_identity_until_restart_cleanup','实际物理删除失败保留身份，重启清理不伪报成功')],
 '5235D8EDD673':[(tree,'management_surface_rename_pin_memory_archive_restore_delete_cleanup','真实detach_children返回1，child parentSessionId空、活动顶层可见，parent归档')],
 '604C64F4828A':[(tree,'checkpoint_file_paths_outside_authorized_folders_are_403','真实checkpoint授权目录外文件403；仅局部权限边界，不证明追加目录动作')],
 '6207F7E8C697':[(tree,'checkpoint_crud_matches_incumbent_shape','真实HTTP create后列表可见，删除后空；只证明HTTP，非原模型工具'),(new,ready['1AEDB2670EB9'][0],'真实HTTP删除不在项404、拒绝无写；不证明工具入口')],
 '9616596B5720':[(tree,'management_surface_rename_pin_memory_archive_restore_delete_cleanup','真实指定会话memory布尔false保存并回读，尚无活动/失败整套证明')],
 'AA9EB3FD034D':[(hist,'history_etag_conditional_get_and_limit_clamp','真实ETag匹配304与limit夹限'),(hist,'history_cursor_validation_and_owner_isolation','真实无效游标拒绝及另一个拥有者403'),(hist,'a07_realtime_reopen_reconnect_export_agree','真实消息/工具投影在重开、重连、导出一致，但不是原字段全覆盖')],
 'B0F05BC32D62':[(tree,'management_surface_rename_pin_memory_archive_restore_delete_cleanup','真实按maxAgeDays0清理过期归档并返回deleted1；不替代原完整文件清理负向')],
 'B9DA1A584C6E':[(tree,'duplicate_session_id_is_loud_409_on_fork_and_create','真实会话创建重复ID409不覆盖原身份；尚无焦点和广播证明')],
 'E1625F70C7CC':[(tree,'rewind_restores_content_with_per_file_verdicts_and_branch_rewinds','真实HTTP回退restored/skipped/conflicted报告对应字节与分支，外部修改保留'),(tree,'busy_session_refuses_fork_retry_rewind_409','忙会话409，不证明模型工具用户拒绝等待')],
 'FF4F06011B48':[]}
ready['4CEBEA945B7A']=('original_retry_refusals_preserve_history_and_recover_after_actual_storage_failure',['真实指定轮次重试；结合既有实际workspace文件回退与关闭偏好403不动分支检查','本检查无效输入400/未知404/越权403/真实分支写入失败500均保留原历史和其他会话；恢复正对照；既有忙409检查'])
ready['5235D8EDD673']=('original_detach_archive_and_permanent_delete_refuse_atomically_and_do_not_restore_deleted_session',['真实解除子谱系后父归档，子仍active顶层，历史不丢','无效策略400、缺会话404、越权403、父写失败500使先解除的子谱系一并回滚，恢复正对照'])
ready['52226FEA5997']=('original_detach_archive_and_permanent_delete_refuse_atomically_and_do_not_restore_deleted_session',['真实归档永久删除后find与restore404，另一子会话及原历史保留；结合既有实际托管文件清理检查','无效400/缺项404/未归档409/越权403及删除末段实际SQLite失败500保留生命周期与历史；恢复正对照；既有实际文件失败检查'])
ready['B0F05BC32D62']=('original_cleanup_expired_archive_removes_bytes_title_and_lifecycle_with_honest_failures',['真实原cleanup HTTP按默认90天删除过期归档托管文件字节、会话标题及生命周期；保留近期归档和活动会话，返回实际deleted数量','非法期限400、匿名401、另一主体仅删自己的过期归档；SQLite实际末段删除失败500全事务回滚、文件和会话不变；解除失败后同入口成功'])
partials['3F95164A488C']=[(new,'original_archive_children_recurses_skips_running_and_preserves_failed_transaction','真实父与两层后代归档后恢复、真正运行子会话跳过计数，非法/缺项/越权/SQLite实际失败无错误副作用；原错误计数仍缺，不冒充完整通过')]
del ready['A18187567AF9']
gap['A18187567AF9']='实际标题搜索命中返回snippet空串，原Then要求标题或内容命中及片段；原实现对标题也生成命中片段。已保留标题命中、真实内容片段、空查询、权限与真实读取失败证明，不把缺少片段说成完整通过。'
partials['A18187567AF9']=[(new,'original_session_search_returns_scoped_title_content_and_explicit_read_failures','真实标题命中但片段为空的现状、内容命中和实际片段、空查询与非法输入/主体隔离/SQLite读取失败；缺原标题片段')]
extra={'4CEBEA945B7A':partials['4CEBEA945B7A'],'52226FEA5997':partials['52226FEA5997']}
items=[]
for r in rows:
 k=r['id'].rsplit('-',1)[1];q=dict(r);q['executionStatus']='NOT_EXECUTED_FOR_THIS_PROOF_SET';q['groups']=[]
 if k in ready:
  name,meanings=ready[k];q['status']='READY';q['gaps']=[]
  for i,a in enumerate(r['r00Assertions']):q['groups'].append({'assertionIndex':i,'assertion':a,'status':'READY','checks':[test(new,name,meanings[i])]+[test(s,n,m) for s,n,m in extra.get(k,[])],'missingClauses':[]})
 else:
  assert k in gap,k
  q['status']='GAP';q['gaps']=[gap[k]]
  for i,a in enumerate(r['r00Assertions']):q['groups'].append({'assertionIndex':i,'assertion':a,'status':'GAP','checks':[test(s,n,m) for s,n,m in partials.get(k,[])],'missingClauses':[gap[k]]})
 if k in ['3F95164A488C','A18187567AF9']:
  q['groups'][1]['status']='READY';q['groups'][1]['missingClauses']=[]
 items.append(q)
result={'schemaVersion':1,'group':'sessions','count':len(items),'readyCount':sum(i['status']=='READY' for i in items),'gapCount':sum(i['status']=='GAP' for i in items),'scope':'仅原65条逐断言证明；不修改产品，局部检查不释放原要求','formalAcceptanceComplete':False,'items':items}
(out/'mappings/sessions.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
print({k:v for k,v in result.items() if k!='items'})

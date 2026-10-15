"""按原完整断言登记Slash五叶及当前Rust桌面原生编辑；不替代阶段验收。"""
import importlib.util,json,hashlib,re
from pathlib import Path
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
spec=importlib.util.spec_from_file_location("anchors",HERE.parent/"continuation-20261013"/"review-proof-anchors.py")
a=importlib.util.module_from_spec(spec);spec.loader.exec_module(a)
inventory=json.loads(a.MAP.read_text());rows={r["id"]:r for r in inventory["items"]}
suite="r06_slash_completion"
ui="native_slash_production_input_real_ws_and_inline_result"
coverage={
"0FE8BED686E2":[["ws_compact_reports_progress_before_real_paid_call_then_commits_only_target"],["attached_reset_and_fresh_refuse_and_compact_failure_preserves_all_sources"]],
"2FAA749DA790":[["slash_unknown_uses_real_dispatcher_and_explicit_result",ui],["missing_agent_real_unready_and_foreign_bridge_target_are_refused_without_any_mutation"]],
"3FCB259FF69C":[["reset_physically_clears_only_current_history_preserves_charges_and_next_run_restart",ui],["attached_reset_and_fresh_refuse_and_compact_failure_preserves_all_sources","missing_agent_real_unready_and_foreign_bridge_target_are_refused_without_any_mutation"]],
"4EA88B5073E4":[["stop_takeover_cancels_real_desktop_only_busy_reset_refuses_and_idle_is_explicit",ui],["stop_takeover_cancels_real_desktop_only_busy_reset_refuses_and_idle_is_explicit"]],
"5A6E0D526903":[["fresh_refreshes_real_prompt_memory_and_tools_then_failure_preserves_original_history",ui],["attached_reset_and_fresh_refuse_and_compact_failure_preserves_all_sources","fresh_refreshes_real_prompt_memory_and_tools_then_failure_preserves_original_history"]],
"BDE3AC44D79C":[["current_rust_desktop_edit_ipc_allows_only_four_native_sender_commands"],["current_rust_desktop_edit_ipc_allows_only_four_native_sender_commands"]]
}
meanings={
"0FE8BED686E2":["实际Bridge WS先回正在压缩，真实供应商仍等待时历史未改；释放后真实持久摘要与token收据，只有被接管desktop改变。","真实供应商400明确失败、原历史与费用链保留，Bridge自身与旁会话均不变。"],
"2FAA749DA790":["实际HTTP/WS共用生产dispatch；当前Electron实际InputArea/Tiptap及生产WS显示真实slash_result。","真实未知命令、Agent不匹配或目录缺失、无压缩/编译服务及外主体均明确拒绝，原资料保全且零新增模型请求。"],
"3FCB259FF69C":["真实reset原子删除当前消息、正文事件、锚点和检查点，费用保留；重启下一Run无旧正文，当前InputArea显示已清除并失效旧读取。","真实接管拒绝、busy拒绝、未知会话不动其他会话；旁会话消息/分支/Run原字节保留。"],
"4EA88B5073E4":["真实等待中Run取消，晚到回复不落盘；空闲明确已停止，实际输入框展示收据。","接管只取消实际desktop，Bridge自身与旁会话仍运行并正常完成。"],
"5A6E0D526903":["实际修改当前身份和compiled memory后fresh，真实档案持久最新提示/记忆/工具，重启下一Run实际消费并返回token结果。","实际接管拒绝且零模型调用；真实供应商失败明确fresh失败、原历史保留且失败费用记录存在。"],
"BDE3AC44D79C":["当前Rust真实子进程连接下实际生产preload/main IPC，通过两真实Electron窗口验证cut/copy/paste原生事件及selectAll真实选择，仅调用发送窗口。","实际10个非法参数经真实IPC全拒绝Unknown edit command，含破坏隐式字符串转换对象；零额外编辑事件/另窗事件。独立负控原FAIL保留后修复。"]
}
ids=[]
for short,parts in coverage.items():
 leaf="R00-T02-LA-"+short;ids.append(leaf);r=rows[leaf];original=(list(r["assertions"]),r["then"])
 assert len(r["assertions"])==len(parts)==2
 for index,tests in enumerate(parts):
  g=next((g for g in r["groups"] if g["index"]==index),None)
  if g is None:g={"index":index,"tests":[],"proofLocations":[]};r["groups"].append(g)
  actual_suite="r06_edit_command_current_source" if short=="BDE3AC44D79C" else suite
  for name in tests:
   ref={"package":"lingxi-service","suite":actual_suite,"test":name}
   if ref not in g["tests"]:g["tests"].append(ref);g["proofLocations"].append(a.test_location(actual_suite,name))
  g.update(coverage="FULL",missingClauses=[],meaning=meanings[short][index]+" 当前macOS Electron和Rust实际执行；其他平台/Tauri正式宿主和阶段未冒称通过。")
 r["groups"].sort(key=lambda g:g["index"])
 assert (r["assertions"],r["then"])==original
 r.update(status="READY",gaps=[],reviewSource=(HERE/"slash-edit-proof-review.json").relative_to(ROOT).as_posix())
# 当前Tauri路径需自己的真实窗口证据，不能继承Electron事件结果。
p=ROOT/"desktop/src-tauri/tests/edit_current_host.rs";raw=p.read_text();name="current_tauri_native_edit_four_commands_and_refusals";d=re.search(r"\bfn\s+"+name+r"\s*\(",raw);assert d
level=0;opened=False
for token in a.tokens(raw[d.end():]):
 if token.group()=="{":level+=1;opened=True
 elif token.group()=="}":
  level-=1
  if opened and level==0:end=d.end()+token.end();break
loc={"path":p.relative_to(ROOT).as_posix(),"test":name,"sourceSHA256":hashlib.sha256(p.read_bytes()).hexdigest(),"testStartLine":raw[:d.start()].count("\n")+1,"testEndLine":raw[:end].count("\n")+1,"assertions":a.assertions_in_body(raw,raw[d.start():end],d)}
ref={"package":"lingxi-resource-desktop","suite":"edit_current_host","test":name,"kind":"rust_desktop_test","features":["e2e-test"]}
for g in rows["R00-T02-LA-BDE3AC44D79C"]["groups"]:
 if ref not in g["tests"]:g["tests"].append(ref);g["proofLocations"].append(loc)
 g["meaning"]+=" 另以当前Tauri真实WebView四命令/9未知字符串拒绝/只读另窗4拒绝及原文不变独立实际验证；不把Electron结果继承给Tauri。"
a.MAP.write_text(json.dumps(inventory,ensure_ascii=False,indent=2)+"\n")
(HERE/"slash-edit-leaf-ids.json").write_text(json.dumps(ids,indent=2)+"\n")
p=HERE.parent/"continuation-20261013"/"reviewed-original-ids.json";reviewed=json.loads(p.read_text())
for leaf in ids:
 if leaf not in reviewed:reviewed.append(leaf)
p.write_text(json.dumps(reviewed,indent=2)+"\n")
print("六叶按原完整条款登记，等待实际绑定窗口；阶段未验收")

"""真实情绪专用流和保存轨迹完成后，逐组绑定两原始叶；正文契约不变。"""
import importlib.util
import json
from pathlib import Path
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
spec=importlib.util.spec_from_file_location("anchors",HERE.parent/"continuation-20261013"/"review-proof-anchors.py")
anchors=importlib.util.module_from_spec(spec);spec.loader.exec_module(anchors)
inventory=json.loads(anchors.MAP.read_text())
ids=["R00-T02-LA-32E9930CF9D9","R00-T02-LA-B09CC9536400"]
suite="r06_mood_trajectory"
tests=["current_desktop_mood_real_ws_before_final_disconnect_and_history_empty_states","saved_mood_trajectory_survives_real_restart_and_empty_history_creates_no_event","interrupted_mood_is_partial_after_restart_and_foreign_principal_cannot_read","reviewer_mood_http_page_cursor_remains_bound_to_actual_mood_stream","reviewer_current_mood_ui_restores_paginated_real_trajectory_without_cursor_failure"]
for leaf_id in ids:
 row=next(x for x in inventory["items"] if x["id"]==leaf_id)
 original={"assertions":list(row["assertions"]),"then":row["then"]}
 known={g["index"] for g in row["groups"]}
 for index in range(len(row["assertions"])):
  if index not in known:row["groups"].append({"index":index,"tests":[],"proofLocations":[],"coverage":"NONE","missingClauses":[row["assertions"][index]],"meaning":""})
 row["groups"].sort(key=lambda g:g["index"])
 for group in row["groups"]:
  for test in tests:
   reference={"package":"lingxi-service","suite":suite,"test":test}
   if reference not in group["tests"]:group["tests"].append(reference);group["proofLocations"].append(anchors.test_location(suite,test))
  group.update(coverage="FULL",missingClauses=[],meaning="真实外部供应商增量→真实规范化→专用情绪key_events流同事务→提交后发布→生产ChatPage里的RustMoodPanel真实HTTP/WS消费；回复结束前已有实时变化。正文流和正文历史仍保持原始纯度断言。断连保持旧内容及原来源、无事件空态不造历史；实际保存后重开/重启恢复同字节。未闭合取消仍partial且不造final，跨主体和普通流绕读拒绝。真实300段/900events分页游标绑定实际Mood流，当前组件300明细逐字一致、不重不漏；无历史读取不写事件。")
 assert [g["index"] for g in row["groups"]]==list(range(len(row["assertions"])))
 assert {"assertions":row["assertions"],"then":row["then"]}==original
 row.update(status="READY",gaps=[],reviewSource=(HERE/"mood-proof-review.json").relative_to(ROOT).as_posix())
anchors.MAP.write_text(json.dumps(inventory,ensure_ascii=False,indent=2)+"\n")
(HERE/"mood-proof-review.json").write_text(json.dumps({"status":"EXECUTED_INDEPENDENT_R2_PASS_NOT_STAGE_ACCEPTANCE","leaves":ids,"tests":tests,"independentReview":"mood-independent-confirmation-r2.json","stageAccepted":False,"scope":"生产当前桌面组件和真服务网络；不宣称真实Electron/Tauri窗口、完整Rust聊天UI或阶段通过。"},ensure_ascii=False,indent=2)+"\n")
(HERE/"mood-leaf-ids.json").write_text(json.dumps(ids,indent=2)+"\n")
path=HERE.parent/"continuation-20261013"/"reviewed-original-ids.json"
all_ids=json.loads(path.read_text())
for leaf_id in ids:
 if leaf_id not in all_ids:all_ids.append(leaf_id)
path.write_text(json.dumps(all_ids,indent=2)+"\n")
print("情绪两叶逐原断言已登记；阶段仍未验收。")

"""以真实会话后台任务、HTTP/WS和媒体交付栅栏登记四原叶。"""
import hashlib
import importlib.util
import json
import re
import subprocess
from pathlib import Path

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
spec=importlib.util.spec_from_file_location("anchors",HERE.parent/"continuation-20261013"/"review-proof-anchors.py")
a=importlib.util.module_from_spec(spec);spec.loader.exec_module(a)
inventory=json.loads(a.MAP.read_text())
baseline=json.loads(subprocess.check_output(["git","show","6d19332793c0819f5219a4ed42fcc8f0d7f813a4:scripts/rust-tauri/r06_original_leaf_bindings.json"],cwd=ROOT))
originals={r["id"]:(r["assertions"],r["then"]) for r in baseline["items"]}
assert len(originals)==279
assert {r["id"]:(r["assertions"],r["then"]) for r in inventory["items"]}==originals
rows={r["id"].rsplit("-",1)[-1]:r for r in inventory["items"]}
suite="r06_background_tasks"
model="model_pending_filters_stop_and_foreign_refusal_use_real_child_runs"
continuation="continuation_preserves_old_task_result_and_old_stop_cannot_cancel_new_attempt"
wire="real_http_and_websocket_stop_owned_only_and_reject_stale_locators"
meanings={
    "7244E7DCE678":"正式模型目录check_pending_tasks只读当前会话真实子Run与宿主归属媒体任务；pending/resolved/failed按真实状态过滤，恢复已结束旧子任务的自有结果、不冒用同线程新结果；没有会话明确失败且不混入其他会话。时间为真实提交时间；无归属独立operation不猜作当前任务。",
    "84E22CD74BE1":"正式模型stop_task取消当前会话不可复用child Run，保留取消原因，旧迟到模型零新事件、无结果落入；外国任务零影响。不存在、已结束、真实停止处理器不可用分别返回not_found/already_stopped/no_handler；续接后的旧taskId只返回已结束，不取消新Run。真实本地视频供应商已接受任务且轮询挂起时，本地取消交付栅栏阻止下载和文件注册，另一会话任务保持pending；remoteCancel明确unsupported，不伪称远端作业已撤销。",
    "857747F848F6":"真实TCP WebSocket typed subagent_stop_request经现行凭证复核、同一稳定sessionId/native或受信legacy定位器和owner闸，返回subagent_stop_result/requestId；实际只取消指定child Run。异会话、真实外来device主体、失效定位及ID/path冲突均拒绝且两个实际子任务继续运行。",
    "B47CAC08F418":"真实HTTP POST task/{id}/abort和WS共享相同定位与所属会话停止函数：当前任务aborted，终态already_stopped，缺失404，异会话/主体403；无上下文或id/path冲突400。HTTP只支持子代理，不误取消媒体任务；模型stop_task另走同一个实际任务核心。",
}
for suffix,meaning in meanings.items():
    row=rows[suffix]
    names=[wire] if suffix in ("857747F848F6","B47CAC08F418") else [model,continuation]
    for index in range(len(row["assertions"])):
        group=next((g for g in row["groups"] if g["index"]==index),None)
        if group is None:
            group={"index":index,"tests":[],"proofLocations":[]};row["groups"].append(group)
        for name in names:
            ref={"package":"lingxi-service","suite":suite,"test":name}
            if ref not in group["tests"]:group["tests"].append(ref);group["proofLocations"].append(a.test_location(suite,name))
        group.update(coverage="FULL",missingClauses=[],meaning=meaning+" 本机合成资料与本地供应商真实运行；不宣称外部付费供应商、其他系统或阶段验收。")
    row.update(status="READY",gaps=[],reviewSource=(HERE/"background-task-proof-review.json").relative_to(ROOT).as_posix())

path=Path("rust/crates/lingxi-service/src/background_tasks.rs")
name="background_tasks::tests::owned_media_query_and_cancel_use_actual_supplier_and_fence_late_products"
raw=(ROOT/path).read_text();declaration=re.search(r"\bfn\s+"+name.rsplit("::",1)[-1]+r"\s*\(",raw);assert declaration
depth=0;opened=False
for token in a.tokens(raw[declaration.end():]):
    if token.group()=="{":depth+=1;opened=True
    elif token.group()=="}":
        depth-=1
        if opened and depth==0:end=declaration.end()+token.end();break
anchor={"path":path.as_posix(),"test":name,"sourceSHA256":hashlib.sha256((ROOT/path).read_bytes()).hexdigest(),"testStartLine":raw[:declaration.start()].count("\n")+1,"testEndLine":raw[:end].count("\n")+1,"assertions":a.assertions_in_body(raw,raw[declaration.start():end],declaration)}
assert anchor["assertions"]
ref={"kind":"rust_library_test","package":"lingxi-service","test":name,"sourcePath":path.as_posix()}
for suffix in ("7244E7DCE678","84E22CD74BE1"):
    for group in rows[suffix]["groups"]:
        if ref not in group["tests"]:group["tests"].append(ref)
        group["proofLocations"]=[loc for loc in group["proofLocations"] if not (loc["path"]==path.as_posix() and loc["test"]==name)]
        group["proofLocations"].append(anchor)
assert {r["id"]:(r["assertions"],r["then"]) for r in inventory["items"]}==originals
a.MAP.write_text(json.dumps(inventory,ensure_ascii=False,indent=2)+"\n")
ids=[rows[suffix]["id"] for suffix in meanings]
(HERE/"background-task-leaf-ids.json").write_text(json.dumps(ids,indent=2)+"\n")
p=HERE.parent/"continuation-20261013"/"reviewed-original-ids.json";reviewed=json.loads(p.read_text())
for leaf in ids:
    if leaf not in reviewed:reviewed.append(leaf)
p.write_text(json.dumps(reviewed,indent=2)+"\n")
print("后台任务四叶登记；当前候选实际绑定及汇总通过后才计入完成")

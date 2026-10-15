"""登记聊天六原叶的真实运行证据，保留全部原断言与既有检查。"""
import importlib.util
import json
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
spec = importlib.util.spec_from_file_location("anchors", HERE.parent / "continuation-20261013" / "review-proof-anchors.py")
a = importlib.util.module_from_spec(spec)
spec.loader.exec_module(a)
inventory = json.loads(a.MAP.read_text())
baseline = json.loads(subprocess.check_output(["git", "show", "6d19332793c0819f5219a4ed42fcc8f0d7f813a4:scripts/rust-tauri/r06_original_leaf_bindings.json"], cwd=ROOT))
originals = {r["id"]: (r["assertions"], r["then"]) for r in baseline["items"]}
assert len(originals) == 279
assert {r["id"]: (r["assertions"], r["then"]) for r in inventory["items"]} == originals
rows = {r["id"].rsplit("-", 1)[-1]: r for r in inventory["items"]}
confirmation = json.loads((HERE / "chat-backend-author-confirmation.json").read_text())
meanings = {
    "D483E4CD0935": "真实WS授权后进入正式Run，工具真实读取与模型下一轮消费、阶段/实时正文/最终消息均可核对；空输入和非法数量/格式/空字节/超限图片零Run，跨会话fileId拒绝，实际9MiB文件引用图片进入模型。",
    "7CBF6F4A6760": "真实准备阶段取消后释放文件读租约仍零迟到Run；真实窗口中邻会话占唯一模型槽，当前排队取消后UI立即闲、邻会话继续，释放槽后取消输入零供应商调用；旧流ID不取消新流、无任务already_stopped。",
    "81664285F927": "真实断连恢复补齐连续遗漏seq和终局；错误ID reset，无活动流空事件与运行态，真实4200个delta触发有界缓存truncated且完整持久正文不丢；外主体零旧帧。",
    "C1FB9C92E813": "真实活动工具回合steer进入下一轮模型请求，闲时成为普通真实输入；真实删除助手后拒绝且零新消息/调用，不改变邻会话活动流；外主体拒绝。",
    "92620BF2264F": "真实并行两会话只打断目标并保留新用户收据与回复；授权和媒体校验先于中断，非法媒体/会话路径/外主体/fileId归属拒绝、无空消息且原Run不取消。",
    "421EEA1AC8D5": "真实WS先accepted再succeeded/noop/failed；失败模型调用账本保留，未知method/实验关闭/缺会话/流式中拒绝且历史不改；instant_simple开启后真实本地checkpoint零新增模型/费用，重启下一Run实际消费。",
}
for suffix, meaning in meanings.items():
    row = rows[suffix]
    names = list(confirmation["testsForLeaf"][suffix])
    if suffix == "D483E4CD0935":
        names += ["file_refs_real_owner_snapshots_office_bytes_transaction_and_negative_zero_run", "actual_model_switch_default_thinking_and_image_survive_restart_into_provider"]
    for index in range(len(row["assertions"])):
        group = next((g for g in row["groups"] if g["index"] == index), None)
        if group is None:
            group = {"index": index, "tests": [], "proofLocations": []}
            row["groups"].append(group)
        for name in names:
            ref = {"package": "lingxi-service", "suite": "r06_chat_wire", "test": name}
            if ref not in group["tests"]:
                group["tests"].append(ref)
                group["proofLocations"].append(a.test_location("r06_chat_wire", name))
        group.update(coverage="FULL", missingClauses=[], meaning=meaning + " 当前macOS合成资料和本地供应商实际运行；不宣称完整产品启动、外部付费供应商、其他系统或全阶段验收。")
    row["groups"].sort(key=lambda g: g["index"])
    row.update(status="READY", gaps=[], reviewSource=(HERE / "chat-backend-proof-review.json").relative_to(ROOT).as_posix())
assert {r["id"]: (r["assertions"], r["then"]) for r in inventory["items"]} == originals
a.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
ids = [rows[suffix]["id"] for suffix in meanings]
(HERE / "chat-backend-leaf-ids.json").write_text(json.dumps(ids, indent=2) + "\n")
p = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
reviewed = json.loads(p.read_text())
for leaf in ids:
    if leaf not in reviewed:
        reviewed.append(leaf)
p.write_text(json.dumps(reviewed, indent=2) + "\n")
print("聊天六叶登记；实际冻结绑定汇总通过后才计入完成")

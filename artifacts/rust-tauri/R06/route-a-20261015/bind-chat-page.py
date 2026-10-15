"""将会话、交互与原生目录六叶绑定到当前真实检查，不改原验收文字。"""
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
entries = {
    "81B217A7E581": ("r06_chat_wire", ["native_current_chat_page_inputarea_tiptap_true_network_and_queue_interject", "native_current_app_pages_side_preview_attachment_and_model_response_generation", "model_switch_uses_real_session_admission_and_rejections_preserve_choice", "session_id_references_are_authorized_preserved_and_never_read_source_body_implicitly"], "当前生产Electron AppPages/ChatPage/InputArea/Tiptap真实操作原六动作：欢迎/消息投影、prompt/slash独立回执、中止插话与运行中队列无steer、侧边引用/文本图片附件/真实HTTP预览及失败提示/运行胶囊。已有消息切换与草稿设置分开，思考档位真实读写，busy/网络失败保留原值；迟到模型回复严格按当前连接与会话/请求令牌核对，不能解除新请求忙态。附件沿真实所属会话落库，下一HTTP模型实际收到正文和PNG像素；现有权威历史协调器实际补图解码2×2，不加冗余WS水合。侧栏引用持久且不暗读别会话正文。真实Run、存储、费用与窗口投影共同核对，另8文件148JS和三TS通过。旧后端父与27窗口原强断言保留，新AppPages34实际强断言通过。"),
}
for suffix, (suite, names, meaning) in entries.items():
    row = rows[suffix]
    for index in range(len(row["assertions"])):
        group = next((g for g in row["groups"] if g["index"] == index), None)
        if group is None:
            group = {"index": index, "tests": [], "proofLocations": []}
            row["groups"].append(group)
        for name in names:
            ref = {"package": "lingxi-service", "suite": suite, "test": name}
            if ref not in group["tests"]:
                group["tests"].append(ref)
                group["proofLocations"].append(a.test_location(suite, name))
        group.update(coverage="FULL", missingClauses=[], meaning=meaning + " 当前macOS合成资料实际运行；不声称外部付费供应商、其他系统或全阶段验收。")
    row["groups"].sort(key=lambda g: g["index"])
    row.update(status="READY", gaps=[], reviewSource=(HERE / "chat-page-proof-review.json").relative_to(ROOT).as_posix())
assert {r["id"]: (r["assertions"], r["then"]) for r in inventory["items"]} == originals
a.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
ids = [rows[suffix]["id"] for suffix in entries]
(HERE / "chat-page-leaf-ids.json").write_text(json.dumps(ids, indent=2) + "\n")
p = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
reviewed = json.loads(p.read_text())
for leaf in ids:
    if leaf not in reviewed:
        reviewed.append(leaf)
p.write_text(json.dumps(reviewed, indent=2) + "\n")
print("聊天页面一叶已登记，实际当前候选汇总通过后才计数")

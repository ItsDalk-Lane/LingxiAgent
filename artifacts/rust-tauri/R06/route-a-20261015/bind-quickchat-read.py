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
    "A2C385A33493": ("r06_quickchat_completion", ["original_quick_chat_visible_resize_focus_existing_missing_destroyed_and_late_ack"], "当前生产QuickChatApp/主聊天组件与双真实Electron窗口、CLI、真实HTTP模型共同核对：显示和尺寸改变；实际创建自有会话/一请求一费用账，真正owned switch和主renderer按请求ID/来源摘要/SID/path/连接代际回执确认，主窗真实聚焦后才隐藏。主窗DOM真实包含同一回复；相同路径仍真实服务focus，晚ACK/错owner/不存在会话/main.destroy均明确失败并保留快捷窗；不再无条件隐藏假成功。"),
    "93D5237ADE81": ("r06_read_rich", ["real_read_office_and_vision_reach_next_model_with_source_and_restart_ledger", "real_read_refusals_never_send_foreign_pixels_or_register_products", "real_read_vision_failure_and_cancel_keep_honest_fees_and_release_session_lease", "real_read_xlsx_keeps_eight_sheets_and_reports_the_ninth_without_silent_loss"], "真实Run的read保持文本分页/截断/去重；无链接非阻塞句柄、20MiB上限和前后实际身份/授权校验，Office有限解析和xlsx前8表/第9表明确truncated，无sheets显式失败。原始SHA和来源进入下一真实模型；真实PNG完整像素通过现有vision辅助槽/共享quota/认证/网络发送，实际描述由下一模型消费。未配置/供应商失败/取消各有真实诚实费用账（取消attempts=NULL，不猜零），同一SID取消后再read和重启不重复收费；越权路径/链接、坏Office/伪PNG、FIFO/过大源零视觉泄漏、零文件产物。直接图像工具块、旧GBK/XLS不以本轮声称支持。"),
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
    row.update(status="READY", gaps=[], reviewSource=(HERE / "quickchat-read-proof-review.json").relative_to(ROOT).as_posix())
assert {r["id"]: (r["assertions"], r["then"]) for r in inventory["items"]} == originals
a.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
ids = [rows[suffix]["id"] for suffix in entries]
(HERE / "quickchat-read-leaf-ids.json").write_text(json.dumps(ids, indent=2) + "\n")
p = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
reviewed = json.loads(p.read_text())
for leaf in ids:
    if leaf not in reviewed:
        reviewed.append(leaf)
p.write_text(json.dumps(reviewed, indent=2) + "\n")
print("快捷聊天与丰富读取两叶已登记，实际当前候选汇总通过后才计数")

"""真实手动整理、修订与状态逐原断言绑定；自动周期另叶仍保持开放。"""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
spec = importlib.util.spec_from_file_location(
    "anchors", HERE.parent / "continuation-20261013" / "review-proof-anchors.py"
)
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())
suite = "r06_dream_completion"
manual = (suite, "manual_five_stages_atomic_revision_usage_and_next_consumer")
refusals = (suite, "configured_refusals_disabled_and_unconfigured_are_real")
busy = (suite, "busy_is_same_agent_and_other_agent_can_finish")
lifecycle = (suite, "live_core_without_model_reads_revision_and_stopped_core_refuses_disclosure")
revisions = ("r06_original_memory_skill_proofs", "original_nonempty_revision_lists_are_agent_scoped_and_read_only")
definitions = {
    "R00-T02-LA-76EE734926B4": {
        0: ([manual, revisions], "真实五阶段整理生成修订，指定助手GET逐项返回真实修订ID；A/B独立，清单不披露before正文，两边文件均未改写。"),
        1: ([lifecycle], "真实核心stop_agent之后清单/详情均503并不含修订；B仍200且无额外供应商调用。无模型与核心停止分别明确报告，不把无配置当作核心不存在。"),
    },
    "R00-T02-LA-A9AD4C7F76AE": {
        0: ([manual], "真实POST202返回运行状态，五阶段供应商响应真实计费落账、发布修订与正文；同一Session下一Run消费新正文。"),
        1: ([refusals, busy, lifecycle], "settings.write权限拒绝403；实际关闭记忆409；同助手已有运行409且不能抢写清理，B独立完成；无配置503且无模型调用，核心真实停止503并不扩大披露。"),
    },
    "R00-T02-LA-B45ECEC543C6": {
        0: ([manual, busy], "真实GET返回指定助手当前running/succeeded、实际run/revision信息；跨助手任务独立，不以占位状态冒充成功。"),
        1: ([refusals, lifecycle], "真实未知助手404、无读取权限403；无模型503明确coreAvailable=true，真正stop_agent后503/coreAvailable=false，停止A不影响B且读状态不调用模型。"),
    },
}
ids = list(definitions)
for leaf_id, clauses in definitions.items():
    row = next(x for x in inventory["items"] if x["id"] == leaf_id)
    original = {"assertions": list(row["assertions"]), "then": row["then"]}
    assert set(clauses) == set(range(len(row["assertions"])))
    known = {g["index"] for g in row["groups"]}
    for index in clauses:
        if index not in known:
            row["groups"].append({"index": index, "tests": [], "proofLocations": []})
    row["groups"].sort(key=lambda g: g["index"])
    for group in row["groups"]:
        tests, meaning = clauses[group["index"]]
        for test_suite, test in tests:
            reference = {"package": "lingxi-service", "suite": test_suite, "test": test}
            if reference not in group["tests"]:
                group["tests"].append(reference)
                group["proofLocations"].append(anchors.test_location(test_suite, test))
        group.update(coverage="FULL", missingClauses=[], meaning=meaning)
    assert {"assertions": row["assertions"], "then": row["then"]} == original
    row.update(status="READY", gaps=[], reviewSource=(HERE / "dream-proof-review.json").relative_to(ROOT).as_posix())
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
(HERE / "dream-proof-review.json").write_text(json.dumps({
    "status": "INDEPENDENT_R2_AND_DELETE_R4_PASS_NOT_STAGE_ACCEPTANCE",
    "leaves": ids,
    "independentReview": ["dream-independent-review-r2.json", "agent-delete-independent-review-r4.json"],
    "stageAccepted": False,
    "scope": "三项手动整理核心、真实状态及修订；0B自动ticker与3516完整删除UI尚未登记，不替代全阶段门禁。",
}, ensure_ascii=False, indent=2) + "\n")
(HERE / "dream-leaf-ids.json").write_text(json.dumps(ids, indent=2) + "\n")
path = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
all_ids = json.loads(path.read_text())
for leaf_id in ids:
    if leaf_id not in all_ids:
        all_ids.append(leaf_id)
path.write_text(json.dumps(all_ids, indent=2) + "\n")
print("三项手动记忆整理逐原断言已登记；自动周期及阶段仍未验收。")

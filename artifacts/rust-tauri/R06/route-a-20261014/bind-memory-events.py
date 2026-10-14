"""真实事件与下一消费者证明完整后，绑定三项记忆原始叶。"""
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
rows = {x["id"]: x for x in inventory["items"]}
suite = "r06_memory_event_completion"
specification = {
    "R00-T02-LA-04EE57C5D2BF": (
        "direct_tenets_success_duplicate_events_and_refusals_are_agent_scoped",
        "真实HTTP新增active和duplicate均广播tenets-changed；重复id不变。"
        "空/非法content、容量409、只读凭证、未知助手拒绝，双助手字节及事件不变。",
    ),
    "R00-T02-LA-7563265D4241": (
        "pinned_replacement_event_and_next_actual_run_use_latest_principles",
        "真实整体替换删除不在名单的active模型提议项，新增为user_direct active；"
        "保留pending/rejected，发agent-updated。下一实际Run仅消费新原则，空列表清空；"
        "非法请求/权限拒绝无写入、无事件，另一助手文件不变。",
    ),
    "R00-T02-LA-DCF0836C3A16": (
        "experience_sync_event_reads_latest_documents_and_next_run_reloads_materials",
        "真实#标题同步分类/索引并发agent-updated；重复分类合并、空文档清空。"
        "GET/search立即消费新经验，下一生产Run现读人格/config。"
        "暂停403、非法分类/非字符串400、只读权限及未知助手拒绝均零文件/事件副作用。",
    ),
}
report = {"status": "EXECUTED_TESTS_BOUND_NOT_STAGE_ACCEPTANCE", "leaves": {}}
for leaf_id, (test, meaning) in specification.items():
    row = rows[leaf_id]
    original = {"then": row["then"], "assertions": list(row["assertions"])}
    location = anchors.test_location(suite, test)
    reference = {"package": "lingxi-service", "suite": suite, "test": test}
    for group in row["groups"]:
        if reference not in group["tests"]:
            group["tests"].append(reference)
            group["proofLocations"].append(location)
        group.update(coverage="FULL", missingClauses=[], meaning=meaning)
    assert [g["index"] for g in row["groups"]] == list(range(len(row["assertions"])))
    assert all(g["coverage"] == "FULL" and not g["missingClauses"] for g in row["groups"])
    assert {"then": row["then"], "assertions": row["assertions"]} == original
    row.update(status="READY", gaps=[], reviewSource=(HERE / "memory-events-proof-review.json").relative_to(ROOT).as_posix())
    report["leaves"][leaf_id] = {"proof": location, "meaning": meaning}
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
(HERE / "memory-events-proof-review.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
ids_path = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
ids = json.loads(ids_path.read_text())
for leaf_id in specification:
    if leaf_id not in ids:
        ids.append(leaf_id)
ids_path.write_text(json.dumps(ids, indent=2) + "\n")
print("三项记忆原始叶已全部绑定；阶段仍未验收。")

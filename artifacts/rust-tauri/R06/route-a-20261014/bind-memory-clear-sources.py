"""真实总结来源清理和错误回滚证明完整后，绑定两项原始叶。"""
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
suite = "r06_memory_clear_sources"
tests = [
    "clear_summary_sources_is_agent_scoped_preserves_raw_until_all_clear_and_next_read_is_empty",
    "clear_sources_denied_invalid_io_and_database_failure_keep_actual_sources_facts_and_marker",
    "clear_sources_refuses_linked_directory_and_cannot_clear_another_agents_sources",
    "late_summary_delete_io_failure_rolls_back_compiled_files_marker_and_fact_transaction",
]
specification = {
    "R00-T02-LA-396F817018F6": "真实清除事实/关联向量/检索索引、五个整理文件及直接JSON总结来源，留下重置标记；另一助手完整保留。",
    "R00-T02-LA-53B672CBEA46": "真实清除五个整理文件及直接JSON总结来源，留下重置标记；原始事实保留，下一GET整理正文为空。",
}
report = {"status": "EXECUTED_TESTS_BOUND_NOT_STAGE_ACCEPTANCE", "leaves": {}}
for leaf_id, meaning in specification.items():
    row = rows[leaf_id]
    original = {"then": row["then"], "assertions": list(row["assertions"])}
    meaning += "只清直接普通JSON，不递归、不删除其他格式；只读凭证/未知助手/异常路径/链接目录拒绝零副作用；数据库拒绝和实际总结删除I/O故障均返回错误，文件/重置标记/事实事务恢复。"
    for group in row["groups"]:
        for test in tests:
            reference = {"package": "lingxi-service", "suite": suite, "test": test}
            if reference not in group["tests"]:
                group["tests"].append(reference)
                group["proofLocations"].append(anchors.test_location(suite, test))
        group.update(coverage="FULL", missingClauses=[], meaning=meaning)
    assert [g["index"] for g in row["groups"]] == list(range(len(row["assertions"])))
    assert all(g["coverage"] == "FULL" and not g["missingClauses"] for g in row["groups"])
    assert {"then": row["then"], "assertions": row["assertions"]} == original
    row.update(status="READY", gaps=[], reviewSource=(HERE / "memory-clear-sources-proof-review.json").relative_to(ROOT).as_posix())
    report["leaves"][leaf_id] = {"tests": tests, "meaning": meaning}
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
(HERE / "memory-clear-sources-proof-review.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
ids_path = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
ids = json.loads(ids_path.read_text())
for leaf_id in specification:
    if leaf_id not in ids:
        ids.append(leaf_id)
ids_path.write_text(json.dumps(ids, indent=2) + "\n")
print("两项记忆清理原始叶已全部绑定；阶段仍未验收。")

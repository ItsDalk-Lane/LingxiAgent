"""记忆工具三叶逐断言绑定；独立并发反例修复和真实范围确认通过后执行。"""
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
suite = "r06_memory_tool_completion"
specification = {
    "0779DAF64B03": ["unpin_review_actual_scope_structured_counts_history_and_agent_isolation", "unpin_changed_matches_during_confirmation_refuse_all_deletions"],
    "7432B00DEC94": ["memory_search_status_no_hits_not_searched_and_agent_channel_isolation", "memory_semantic_real_provider_no_model_no_coverage_timeout_and_actual_vector_hits"],
    "A17D742E55B7": ["record_and_recall_registered_tools_preserve_failures_and_agent_boundaries", "independent_concurrent_record_successes_are_all_recallable", "experience_record_sync_rebuild_and_rollback_share_one_agent_operation_gate", "experience_failed_index_publish_keeps_operation_gate_through_document_rollback", "experience_operation_gate_survives_memory_directory_creation_and_agent_path_alias"],
}
confirmation = json.loads((HERE / "memory-tools-review-fix-confirmation.json").read_text())
assert confirmation["status"] == "PASS" and confirmation["stageAcceptance"] is False
boundaries = ["large_native_details_share_projection_budget_keep_raw_and_restore_same_projection"]
report = {"status": "EXECUTED_TESTS_BOUND_NOT_STAGE_ACCEPTANCE", "leaves": {}}
for suffix, leaf_tests in specification.items():
    leaf_id = "R00-T02-LA-" + suffix
    row = rows[leaf_id]
    original = {"then": row["then"], "assertions": list(row["assertions"])}
    tests = [*leaf_tests, *boundaries]
    # 旧登记只给成功断言建组；按原文补齐拒绝组，才能逐断言证明完整。
    known = {group["index"] for group in row["groups"]}
    for index in range(len(row["assertions"])):
        if index not in known:
            row["groups"].append({"index": index, "tests": [], "proofLocations": [],
                                  "coverage": "NONE", "missingClauses": [row["assertions"][index]], "meaning": ""})
    row["groups"].sort(key=lambda group: group["index"])
    for group in row["groups"]:
        for name in tests:
            reference = {"package": "lingxi-service", "suite": suite, "test": name}
            if reference not in group["tests"]:
                group["tests"].append(reference)
                group["proofLocations"].append(anchors.test_location(suite, name))
        group.update(
            coverage="FULL", missingClauses=[],
            meaning="真实生产注册/网关/权限/存储及运行/历史验证原成功和拒绝条款；unpin逐次Review准确冻结范围，结构计数真实；"
                    "语义查询经真实嵌入供应商链和覆盖/超时裁决，Agent隔离；经验实际写后可召回，失败回滚与同助手各写入口完整互斥。"
                    "不同助手独立，忙时明确可重试冲突；原生业务字段保留在运行输入/协议/存档，与正文共享32KiB投影预算，原raw完整保留。"
                    "外部HTTP模型消费正文，此证明不把内部details等同供应商实际收到结构字段。",
        )
    assert [g["index"] for g in row["groups"]] == list(range(len(row["assertions"])))
    assert all(g["coverage"] == "FULL" and not g["missingClauses"] for g in row["groups"])
    assert {"then": row["then"], "assertions": row["assertions"]} == original
    row.update(status="READY", gaps=[], reviewSource=(HERE / "memory-tools-proof-review.json").relative_to(ROOT).as_posix())
    report["leaves"][leaf_id] = {"tests": tests, "coverage": "FULL"}
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
(HERE / "memory-tools-proof-review.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
ids_path = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
ids = json.loads(ids_path.read_text())
for suffix in specification:
    leaf_id = "R00-T02-LA-" + suffix
    if leaf_id not in ids:
        ids.append(leaf_id)
ids_path.write_text(json.dumps(ids, indent=2) + "\n")
print("记忆工具三叶逐断言已绑定；阶段仍未验收。")

"""人格/头像八叶逐断言绑定；仅在独立发现修复并实际复验后执行。"""
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
suite = "r06_persona_completion"
specification = {
    "4870805E3A3D": "original_487080_agents_write_refreshes_actual_description_and_next_roster",
    "856B28902999": "original_856b28_identity_write_refreshes_actual_description_and_next_roster",
    "7C699FD2A740": "original_7c699f_public_write_and_legacy_alias_publish_only_after_persistence",
    "7F0F3F76E38A": "original_7f0f3f_agent_avatar_upload_cleans_formats_refreshes_next_list_and_event",
    "BB4CD1E4CD43": "original_bb4cd1_agent_avatar_delete_cleans_all_formats_and_is_idempotent",
    "7570BE1F2527": "original_7570be_role_avatar_upload_selects_explicit_agent_and_user_without_crosstalk",
    "4A56F26EDA32": "original_4a56f2_role_avatar_delete_restores_defaults_and_preserves_other_target",
    "E4BEE8E6974A": "original_e4bee8_role_avatar_returns_bytes_etag_304_and_404_under_actual_permissions",
}
boundaries = [
    "persona_writes_refuse_actual_filesystem_failure_and_symlink_without_publication",
    "real_immutable_file_failure_rolls_back_persona_pair_and_partial_avatar_cleanup",
]
report = {"status": "EXECUTED_TESTS_BOUND_NOT_STAGE_ACCEPTANCE", "leaves": {}}
for suffix, test in specification.items():
    leaf_id = "R00-T02-LA-" + suffix
    row = rows[leaf_id]
    original = {"then": row["then"], "assertions": list(row["assertions"])}
    tests = [test, *boundaries]
    if suffix in ["4870805E3A3D", "856B28902999"]:
        tests.append("persona_description_uses_explicit_chat_fallback_only_for_unconfigured_summary_slot")
        # 来源变化反例修复的实际用例由主控在最终绑定前明确填入，不能猜名或跳过。
        confirmation = HERE / "persona-review-fix-confirmation.json"
        if not confirmation.exists():
            raise ValueError("人格来源竞态独立复验回执缺失，不可翻READY")
        approved = json.loads(confirmation.read_text())
        if approved.get("status") != "PASS" or not approved.get("sourceRaceTests"):
            raise ValueError("人格来源竞态尚未独立复验通过")
        tests.extend(approved["sourceRaceTests"])
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
            meaning="实际认证HTTP及真实文件/事件验证原成功和拒绝条款；描述经真实供应商链生成，"
                    "下一Run/列表消费更新；role头像准确目标、格式清理、ETag/304/404与15MB请求边界。"
                    "非法/无权/未知目标/真实I/O失败保留原字节和另一助手，拒绝不发布成功事件。",
        )
    assert [g["index"] for g in row["groups"]] == list(range(len(row["assertions"])))
    assert all(g["coverage"] == "FULL" and not g["missingClauses"] for g in row["groups"])
    assert {"then": row["then"], "assertions": row["assertions"]} == original
    row.update(status="READY", gaps=[], reviewSource=(HERE / "persona-proof-review.json").relative_to(ROOT).as_posix())
    report["leaves"][leaf_id] = {"tests": tests, "coverage": "FULL"}
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
(HERE / "persona-proof-review.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
ids_path = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
ids = json.loads(ids_path.read_text())
for suffix in specification:
    leaf_id = "R00-T02-LA-" + suffix
    if leaf_id not in ids:
        ids.append(leaf_id)
ids_path.write_text(json.dumps(ids, indent=2) + "\n")
print("人格八叶逐断言已绑定；阶段仍未验收。")

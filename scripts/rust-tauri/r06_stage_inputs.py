#!/usr/bin/env python3
"""生成 R06 的真实阶段输入；缺少完整行为绑定时明确保持未完成。"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import signal
import subprocess
import time
from pathlib import Path


MAP_PATH = Path("rust/crates/xtask/src/stage_maps/R06.json")
INPUT_PATH = Path("rust/crates/xtask/src/stage_maps/R06_INPUTS.json")
TASK_PATH = Path("Lingxi_Rust_Tauri_Taskbooks_2026-09-23/task-catalog.json")
ACCEPTANCE_PATH = Path("docs/rust-tauri/R00/ACCEPTANCE_MAP.json")
FEATURE_PATH = Path("docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json")
ISSUE_PATH = Path("docs/rust-tauri/R06/repair-current/AR_ISSUE_MATRIX.json")
ORIGINAL_PROOF_PATH = Path("scripts/rust-tauri/r06_original_leaf_bindings.json")

# 仅本次明确获准修复的桌面原条款可引用独立宿主和现行界面检查。
DESKTOP_RESOURCE_LEAVES = {"R00-T02-LA-" + suffix for suffix in (
    "238267978DDA", "2FB382A137FC", "4173A08A51CC", "45627E4F8173",
    "ADB6B0580864", "AF18F5CEA14F", "BDE3AC44D79C", "CAA876503B1B",
    "CC738012B6EE", "D6509017C382", "EE18A57F7348",
)}
DESKTOP_MANIFEST = Path("desktop/src-tauri/Cargo.toml")
DESKTOP_PACKAGE = "lingxi-resource-desktop"


def proof_source_path(binding):
    if binding.get("kind") in ("rust_library_test", "vitest_test"):
        return Path(binding["sourcePath"])
    if binding.get("kind") == "rust_desktop_test":
        return Path("desktop/src-tauri/tests") / (binding["suite"] + ".rs")
    return Path("rust/crates") / binding["package"] / "tests" / (binding["suite"] + ".rs")


# 名称来自现有具名测试；它们是明确的局部证据，不代替完整逐叶语义验收。
ACCEPTANCE_TESTS = {
    "R06-A01": [("r06_t01_closed_loop", "sent_wire_system_bytes_match_the_observation_digests")],
    "R06-A02": [("r06_t01_context_compiler", "run_freeze_spans_turns_and_next_run_refreshes_material"),
                ("r06_t01_context_compiler", "budget_exceeded_fails_the_run_loudly")],
    "R06-A03": [("r06_t02_closed_loop", "a03_mid_run_compaction_lands_on_the_real_binary")],
    "R06-A04": [("r06_t02_closed_loop", "a04_summary_provider_failure_keeps_the_original_history")],
    "R06-A05": [("r06_t03_session_tree", "fork_copies_history_with_stable_ids_and_writes_stay_independent"),
                ("r06_t03_session_tree", "fork_beyond_depth_two_is_refused_409"),
                ("r06_t03_session_tree", "busy_session_refuses_fork_retry_rewind_409"),
                ("r06_t03_session_tree", "duplicate_session_id_is_loud_409_on_fork_and_create"),
                ("r06_t03_session_tree", "cross_principal_tree_access_is_403")],
    "R06-A06": [("r06_t03_session_tree", "rewind_restores_content_with_per_file_verdicts_and_branch_rewinds")],
    "R06-A07": [("r06_t04_history", "a07_realtime_reopen_reconnect_export_agree")],
    "R06-A08": [("r06_t04_history", "history_cursor_validation_and_owner_isolation"),
                ("r06_t04_history", "export_equals_paged_concat")],
    "R06-A09": [("r06_t05_session_files", "a09_import_legacy_sidecar_preserves_ids_and_readability")],
    "R06-A10": [("r06_t05_session_files", "a10_cleanup_keeps_delivered_external_and_cleans_cold_managed")],
    "R06-A11": [("r06_t06_memory", "a11_memory_read_write_isolated_between_agents"),
                ("r06_t06_memory", "a11_cross_agent_and_invalid_ids_are_rejected")],
    "R06-A12": [("r06_t06_skills", "a12_layer1_install_review_token_protocol_end_to_end"),
                ("r06_t06_skills", "a12_layer2_gateway_refuses_out_of_workspace_skill_action")],
    "R06-A13": [("r06_t07_knowledge_tools", "a13_search_within_scope_returns_only_allowed_sources"),
                ("r06_t07_knowledge_tools", "a13_citation_markdown_resolves_to_actual_text")],
    "R06-A14": [("r06_t07_knowledge", "a14_switching_to_incompatible_embedding_dimension_refuses_vector_channel_until_rebuild")],
    "R06-A15": [("r06_t08_behavior_eval", "be10_schema_valid_but_semantically_wrong_scores_fail_not_mixed")],
    "R06-A16": [("r06_t08_behavior_eval", "be04_disabled_tool_never_dispatched_and_prompt_change_never_relaxes"),
                ("r06_t08_behavior_eval", "be05_permission_outside_workspace_refused_and_explainable")],
}

ISSUE_SUITES = {
    "F01": [("lingxi-service", "r06_ar_f01_history")],
    "F02": [("lingxi-service", "r06_ar_f02_file_restore")],
    "F03": [("lingxi-service", "r06_ar_f03_operations")],
    "F04": [("lingxi-service", "r06_ar_f04_lifecycle")],
    "F05": [("lingxi-service", "r06_ar_f05_cache")],
    "F06": [("lingxi-service", "r06_ar_f06_config"), ("lingxi-service", "r06_ar_f06_binary")],
    "F07": [("lingxi-service", "r06_ar_f07_catalog")],
    "F08": [("lingxi-service", "r06_ar_f08_projection")],
    "F09": [("lingxi-service", "r06_ar_w09_embedding_batches")],
    "F10": [("lingxi-service", "r06_ar_w10_embedding_identity")],
    "F11": [("lingxi-service", "r06_ar_w11_embedding_lifecycle")],
    "F12": [("lingxi-service", "r06_ar_f12_cache_consumers"),
            ("lingxi-adapters", "r06_ar_f12_cache_preservation")],
    "F13": [("lingxi-service", "r06_ar_f13_submission")],
    "F14": [],
    "F15": [("lingxi-service", "r06_t08_behavior_eval")],
}

# 只登记已经核对过测试正文的逐断言语义。未覆盖的原始条款保持未完成，
# 不按任务家族、套件通过率或登记数量推算成功，也不后移原期限。
REVIEWED_LEAVES = {
    "R00-T02-LA-960668141E07": {
        "assertions": [
            "给定目标与相应权限，执行“从指定轮次分支会话”后核对：创建带谱系的新会话并广播；超过谱系深度或正在运行时拒绝",
            "输入无效、权限不足或下游失败时应返回明确错误，不呈现成功结果",
        ],
        "groups": [
            {"tests": [("r06_t03_session_tree", "fork_copies_history_with_stable_ids_and_writes_stay_independent"),
                       ("r06_t03_session_tree", "fork_beyond_depth_two_is_refused_409"),
                       ("r06_t03_session_tree", "busy_session_refuses_fork_retry_rewind_409"),
                       ("r06_ar_f03_operations", "completed_fork_replay_conflict_and_recovered_event_are_consistent")],
             "meaning": "真实HTTP分支及消息读回验证谱系/稳定ID/独立写入；持久事件接口恰好一条创建事件；深度第三层和运行中明确409。"},
            {"tests": [("r06_t03_session_tree", "duplicate_session_id_is_loud_409_on_fork_and_create"),
                       ("r06_t03_session_tree", "cross_principal_tree_access_is_403"),
                       ("r06_ar_f03_operations", "positive_and_fork_event_atomicity")],
             "meaning": "重复ID明确409、跨主体403；真实SQLite事件提交失败后分支不存在且重启后仍404、创建事件为0，成功分支保留。"},
        ],
    },
}


def load(root: Path, path: Path):
    return json.loads((root / path).read_text(encoding="utf-8"))


def sha(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True).encode()).hexdigest()


def named_tests(root: Path, package: str, suite: str):
    path = root / "rust/crates" / package / "tests" / (suite + ".rs")
    if not path.is_file():
        return []
    names = re.findall(r"#\[(?:tokio::)?test[^\]]*\]\s*(?:async\s+)?fn\s+(\w+)", path.read_text())
    # 子进程助手单独运行时可能直接返回；它们由所属真实父测试调用，不能冒充正向证据。
    return [{"package": package, "suite": suite, "test": name}
            for name in names if not name.endswith("_child")
            or (package, suite, name) == ("lingxi-service", "r03_t08_acceptance_matrix", "leaf_child_cancel_stops_only_the_child")]


def test_binding(suite, name):
    return {"package": "lingxi-service", "suite": suite, "test": name}


def toolchain(root: Path):
    match = re.search(r'^channel\s*=\s*"([^"]+)"', (root / "rust-toolchain.toml").read_text(), re.MULTILINE)
    if not match:
        raise ValueError("无法读取固定 Rust 工具链")
    return match.group(1)


def command_argv(root: Path, binding):
    if binding.get("kind") == "vitest_test":
        return ["node", "node_modules/vitest/vitest.mjs", "run", str(proof_source_path(binding)),
                "--config", "desktop/vite.tauri-resources.config.ts", "--reporter=json",
                "--no-file-parallelism", "-t", "^" + re.escape(binding["test"]) + "$"]
    if binding.get("kind") == "rust_desktop_test":
        features = binding.get("features", [])
        return ["rustup", "run", toolchain(root), "cargo", "test", "--manifest-path", str(DESKTOP_MANIFEST),
                "--locked", "-p", DESKTOP_PACKAGE, *(["--features", ",".join(features)] if features else []),
                "--test", binding["suite"], binding["test"], "--", "--exact", "--show-output"]
    if binding.get("kind") == "rust_library_test":
        return ["rustup", "run", toolchain(root), "cargo", "test", "--manifest-path", "rust/Cargo.toml", "--locked", "-p",
                binding["package"], "--lib", binding["test"], "--", "--exact", "--show-output"]
    kind = "--bin" if binding.get("kind") == "rust_unit_test" else "--test"
    return ["rustup", "run", toolchain(root), "cargo", "test", "--manifest-path", "rust/Cargo.toml", "--locked", "-p",
            binding["package"], kind, binding["suite"], binding["test"], "--", "--exact", "--show-output"]


def desktop_frontend_binding():
    return {"kind": "script",
            "argv": ["node", "node_modules/vite/bin/vite.js", "build",
                     "--config", "desktop/vite.tauri-resources.config.ts"],
            "stdoutPattern": r"built in \d", "timeoutSecs": 300}


def special_bindings(identifier):
    if identifier == "R06-AR-F14-C01":
        return [{"kind": "script", "argv": ["python3", "-B", "scripts/rust-tauri/r02_registry_consistency.py", "--root", "."],
                 "stdoutPattern": r"^PASS: migrations\.rs MIGRATIONS == registry \(", "timeoutSecs": 120,
                 "evidenceMeaning": "严格核对源码迁移与登记"},
                *[{"package": "lingxi-adapters", "suite": "r06_ar_f14_migration_receipts", "test": name}
                  for name in ("c01_fresh_database_receipts_equal_registry_and_inspector",
                               "c01_v7_upgrade_preserves_receipts_and_restarts_are_idempotent",
                               "c01_v8_upgrade_preserves_receipts_and_restarts_are_idempotent")]]
    if identifier == "R06-AR-F14-C04":
        return [{"kind": "script", "argv": ["python3", "-B", "scripts/rust-tauri/tests/test_r06_stage_inputs_contract.py"],
                 "stdoutPattern": r"^OK$", "timeoutSecs": 120,
                 "evidenceMeaning": "隔离实际生成器和真实登记命令：场景缺失、错绑、空匹配、忽略、旧候选、缺迁移、日志摘要和后移期限必须被拒绝，还原正对照通过。"},
                *[{"kind": "rust_unit_test", "package": "xtask", "suite": "xtask", "test": name}
                for name in ("stage_map::map_tests::empty_scenario_set_is_a_hard_error",
                             "stage_map::map_tests::unknown_command_ref_is_a_hard_error",
                             "verify::runner_tests::missing_evidence_is_a_failure_even_when_exit_is_zero",
                             "candidate::tests::a_changed_checkpoint_cannot_be_erased_by_restoring_final_bytes")]]
    return []


def binding_key(binding):
    """同一具名检查在本轮只执行一次；多个义务共享本轮结果，不复用旧证据。"""
    return "r06_test_" + sha(binding)[:20]


def validate_binding(root, binding):
    if binding["kind"] == "rust_integration_test":
        names = named_tests(root, binding["package"], binding["suite"])
        if not any(row["test"] == binding["test"] for row in names):
            raise ValueError("登记的具名测试不存在：" + binding["suite"] + "::" + binding["test"])
    elif binding["kind"] == "rust_library_test":
        path = Path(binding["sourcePath"])
        prefix = Path("rust/crates") / binding["package"] / "src"
        if not path.is_relative_to(prefix) or ".." in path.parts or not (root / path).is_file():
            raise ValueError("库内检查源码路径不合法")
        name = binding["test"].rsplit("::", 1)[-1]
        if not re.search(r"#\[(?:tokio::)?test[^\]]*\]\s*(?:async\s+)?fn\s+" + re.escape(name) + r"\s*\(", (root / path).read_text()):
            raise ValueError("登记的库内具名测试不存在：" + binding["test"])
    elif binding["kind"] == "rust_desktop_test":
        path = proof_source_path(binding)
        if (binding["package"] != DESKTOP_PACKAGE or ".." in path.parts
                or not (root / path).is_file() or not (root / DESKTOP_MANIFEST).is_file()
                or binding.get("features", []) not in ([], ["e2e-test"])):
            raise ValueError("独立桌面检查必须绑定已授权宿主和固定功能开关")
        declared = re.search(r'(?m)^name\s*=\s*"([^"]+)"', (root / DESKTOP_MANIFEST).read_text())
        if not declared or declared[1] != DESKTOP_PACKAGE:
            raise ValueError("桌面检查包与实际宿主不一致")
        names = re.findall(r"#\[(?:tokio::)?test[^\]]*\]\s*(?:async\s+)?fn\s+(\w+)", (root / path).read_text())
        if names.count(binding["test"]) != 1:
            raise ValueError("桌面具名检查必须在实际源码唯一存在")
    elif binding["kind"] == "vitest_test":
        path = proof_source_path(binding)
        if (binding["package"] != DESKTOP_PACKAGE or ".." in path.parts
                or not path.is_relative_to(Path("desktop/src/tauri-resources/__tests__"))
                or path.suffix not in (".ts", ".tsx") or not (root / path).is_file()):
            raise ValueError("桌面界面检查源码不在授权范围")
        title = binding.get("sourceTestTitle", binding["test"])
        declarations = re.findall(r"\b(?:it|test)\s*\(\s*(['\"])(.*?)\1\s*,", (root / path).read_text())
        if sum(name == title for _, name in declarations) != 1:
            raise ValueError("界面检查必须绑定唯一的真实具名用例")
    if not binding.get("argv"):
        raise ValueError("实际检查命令不能为空")


def original_proof_inventory(root, leaves):
    """完整保留本轮279项原文；局部证明和未实现项不能释放整条原义务。"""
    inventory = load(root, ORIGINAL_PROOF_PATH)
    if inventory.get("schema") != "lingxi.r06.original-proof-map.v1":
        raise ValueError("原义务证明登记版本不受支持")
    originals = {leaf["id"]: leaf for leaf in leaves if leaf["id"] not in REVIEWED_LEAVES}
    rows = inventory.get("items", [])
    if len(rows) != len(originals) or {row["id"] for row in rows} != set(originals):
        raise ValueError("279项原义务证明登记缺失、重复或扩大范围")
    for row in rows:
        leaf = originals[row["id"]]
        if (row.get("assertions") != leaf["r00Assertions"] or row.get("then") != leaf["r00Then"]
                or row.get("status") not in ("UNREVIEWED", "READY", "GAP")):
            raise ValueError(row["id"] + " 原断言或证明状态不合法")
        seen = set()
        for group in row.get("groups", []):
            index = group.get("index")
            if type(index) is not int or not 0 <= index < len(leaf["r00Assertions"]) or index in seen:
                raise ValueError(row["id"] + " 逐断言索引缺失、重复或越界")
            seen.add(index)
            if not group.get("tests") or not group.get("meaning"):
                raise ValueError(row["id"] + " 逐断言缺真实检查或证明范围说明")
            if group.get("coverage") not in ("FULL", "PARTIAL"):
                raise ValueError(row["id"] + " 必须明确完整断言或局部证明")
            if group["coverage"] == "PARTIAL" and not group.get("missingClauses"):
                raise ValueError(row["id"] + " 局部证明必须保留未满足子句")
            locations = group.get("proofLocations", [])
            for test in group["tests"]:
                desktop = (row["id"] in DESKTOP_RESOURCE_LEAVES and test.get("package") == DESKTOP_PACKAGE
                           and test.get("kind") in ("rust_desktop_test", "vitest_test"))
                if not desktop and test.get("package") not in ("lingxi-service", "lingxi-adapters", "lingxi-kernel"):
                    raise ValueError(row["id"] + " 检查包不在原范围")
                if test.get("kind") in ("rust_desktop_test", "vitest_test") and not desktop:
                    raise ValueError(row["id"] + " 桌面检查不能扩大到其他原条款")
                binding = dict(test, kind=test.get("kind", "rust_integration_test"))
                binding["argv"] = command_argv(root, binding)
                validate_binding(root, binding)
                path = proof_source_path(test)
                anchors = [anchor for anchor in locations if anchor.get("path") == str(path)
                           and anchor.get("test") == test["test"]]
                if not anchors or any(anchor.get("sourceSHA256") != file_sha(root / path) for anchor in anchors):
                    raise ValueError(row["id"] + " 证明定位缺失或测试源码改变，需要重新审阅映射")
        if row["status"] == "READY" and (seen != set(range(len(leaf["r00Assertions"]))) or row.get("gaps")
                                        or any(group["coverage"] != "FULL" for group in row.get("groups", []))):
            raise ValueError(row["id"] + " 尚有缺失原断言，不能登记完整证明")
        if row["status"] == "GAP" and not row.get("gaps"):
            raise ValueError(row["id"] + " 未通过项必须保留具体缺口")
    return inventory


def build(root: Path):
    acceptance = load(root, ACCEPTANCE_PATH)["scenarios"]
    features = {row["id"]: row for row in load(root, FEATURE_PATH)["supplemental_scenarios"]}
    tasks = [task for task in load(root, TASK_PATH)["tasks"] if task["stage_id"] == "R06"]
    issues = load(root, ISSUE_PATH)["issues"]
    expected_issues = {f"R06-AR-F{i:02}" for i in range(1, 16)}
    if {row["id"] for row in issues} != expected_issues or len(issues) != 15:
        raise ValueError("R06 的 15 项原问题集合不一致")
    leaves = []
    for identifier, original in sorted(acceptance.items()):
        if "R06" not in original.get("execution_stage_ids", []):
            continue
        original_feature = features[identifier]
        leaf = {"id": identifier, "featureId": original["feature_id"],
                "requirement": original["requirement"], "r00Kind": original["kind"],
                "r00TaskIds": original["task_ids"], "r00ExecutionStageIds": original["execution_stage_ids"],
                "r00LedgerStatus": original["ledger_status"], "r00ResultIds": original["result_ids"],
                "r00TestIds": original["test_ids"], "r00Then": original["then"],
                "r00Assertions": original_feature["assertions"], "r00Due": original_feature["due"],
                "basisKind": "required_behavior_pending", "evidenceCommandRefs": [],
                "evidenceRequired": "保持原条款和原到期阶段；当前修复未执行逐叶完整验收。必须为每条原断言绑定真实专属正反行为证据后才能改为完成；登记或套件通过不得代替。"}
        leaves.append(leaf)
    cases = []
    for identifier, tests in sorted(ACCEPTANCE_TESTS.items()):
        cases.append({"id": identifier, "requirement": "REQUIRED",
                      "supportingTests": [test_binding(suite, name) for suite, name in tests],
                      "bindingStatus": "SUPPORTING_TESTS_ONLY_NOT_FULL_ACCEPTANCE"})
    for issue in issues:
        issue_key = issue["id"].split("-")[-1]
        original_cases = issue["self_check"]["cases"]
        if {case["id"] for case in original_cases} != {issue["id"] + f"-C{i:02}" for i in range(1, 5)}:
            raise ValueError(issue["id"] + " 原四项基础检查集合不一致")
        supporting = [test for package, suite in ISSUE_SUITES[issue_key]
                      for test in named_tests(root, package, suite)]
        for case in original_cases:
            number = case["id"].split("-")[-1].lower()
            precise = [test for test in supporting if re.search(r"(?:^|_)" + number + r"(?:_|$)", test["test"])]
            selected = special_bindings(case["id"]) or precise or supporting
            cases.append({"id": case["id"], "issueId": issue["id"], "requirement": "REQUIRED",
                          "originalCheck": {key: case[key] for key in ("name", "procedure", "expected")},
                          "supportingTests": selected,
                          "bindingStatus": "SUPPORTING_TESTS_ONLY_NOT_FULL_ACCEPTANCE" if selected
                                           else "NO_COMPLETE_EXECUTION_BINDING"})
            if not selected and case["id"] not in ("R06-AR-F14-C02", "R06-AR-F14-C03"):
                cases[-1]["unresolvedBindingReason"] = "完整 R06 阶段/前序闭包的专属语义映射尚未完成；本 case 不递归调用 R06，也不以局部命令冒充完整验收。"
    for case in cases:
        for binding in case["supportingTests"]:
            if binding.get("kind") != "script":
                binding.setdefault("kind", "rust_integration_test")
                binding["argv"] = command_argv(root, binding)
            binding.setdefault("timeoutSecs", 900)
            validate_binding(root, binding)
        # 命令结果只回答是否真实执行成功；阶段验收还要消费全部原始逐叶断言。
        case["stageVerdictOwner"] = "xtask verify-stage R06"
    inputs = {"schema": "lingxi.r06.stage-inputs.v1", "stage": "R06", "originalTasks": tasks,
              "cases": cases, "requiredLeaves": leaves,
              "currentStatus": "IMPLEMENTATION_REGISTERED_ACCEPTANCE_NOT_RUN",
              "note": "只登记本轮 15 项及原 R06 范围；无完整逐断言绑定时保持未完成，未运行不写 PASS。"}
    commands = {"r06_inputs": {"argv": ["python3", "-B", "scripts/rust-tauri/r06_stage_inputs.py",
                                              "--check", "--evidence", "{EVIDENCE}/R06_INPUTS"],
                               "timeoutSecs": 120, "evidencePaths": ["{EVIDENCE}/R06_INPUTS/inputs.json"]},
                "r05_regression_gate": {"argv": ["cargo", "run", "--manifest-path", "rust/Cargo.toml", "--locked", "-p", "xtask", "--",
                                                      "verify-stage", "R05", "--evidence", "{EVIDENCE}/R05_REGRESSION"],
                                         "timeoutSecs": 14400, "evidencePaths": ["{EVIDENCE}/R05_REGRESSION/verify-stage-result.json"]}}
    scenarios = []
    bindings = {}
    for case in cases:
        refs = []
        for binding in case["supportingTests"]:
            key = binding_key(binding)
            if key in bindings and bindings[key] != binding:
                raise ValueError("检查命令标识冲突")
            bindings[key] = binding
            relative = "{EVIDENCE}/R06_BINDINGS/" + key
            commands[key] = {"argv": ["python3", "-B", "scripts/rust-tauri/r06_stage_inputs.py", "--binding", key, "--evidence", relative],
                             "timeoutSecs": binding["timeoutSecs"] + 120,
                             "evidencePaths": [relative + "/binding-result.json"]}
            refs.append(key)
        case["executionCommandRefs"] = refs
        scenarios.append({"id": case["id"], "requirement": "REQUIRED", "commandRefs": ["r06_inputs", *refs],
                          "note": "真实具名命令执行结果；逐叶原义务由同一个阶段汇总，命令全绿不代替阶段完成。"})
    inventory = original_proof_inventory(root, leaves)
    inputs["originalProofInventory"] = inventory
    proof_groups = {}
    for row in inventory["items"]:
        groups = []
        for group in row.get("groups", []):
            refs = []
            for test in group["tests"]:
                binding = dict(test, kind=test.get("kind", "rust_integration_test"), timeoutSecs=900)
                binding["argv"] = command_argv(root, binding)
                validate_binding(root, binding)
                key = binding_key(binding)
                if key in bindings and bindings[key] != binding:
                    raise ValueError("原义务检查命令标识冲突")
                bindings[key] = binding
                relative = "{EVIDENCE}/R06_BINDINGS/" + key
                commands[key] = {"argv": ["python3", "-B", "scripts/rust-tauri/r06_stage_inputs.py", "--binding", key, "--evidence", relative],
                                 "timeoutSecs": 1020, "evidencePaths": [relative + "/binding-result.json"]}
                refs.append(key)
            index = group["index"]
            groups.append({"case": row["id"] + f"-assertion-{index:02}", "expect": 1,
                           "assertion": row["assertions"][index], "commandRefs": sorted(set(refs)),
                           "evidenceMeaning": group["meaning"], "proofLocations": group.get("proofLocations", []),
                           "coverage": group["coverage"], "missingClauses": group.get("missingClauses", [])})
        proof_groups[row["id"]] = groups
    # C02 消费同一轮的实际 R06 执行全集，不递归启动包含自己的门禁。
    # C03 直接消费原 R05 门禁及其前序闭包，绝不以 Cargo 结果替代。
    for case, scenario in zip(cases, scenarios):
        if case["id"] == "R06-AR-F14-C02":
            scenario["commandRefs"] = ["r06_inputs", *sorted(bindings)]
            case["executionCommandRefs"] = scenario["commandRefs"]
            case["bindingStatus"] = "CURRENT_STAGE_AGGREGATE_EXECUTION"
        elif case["id"] == "R06-AR-F14-C03":
            scenario["commandRefs"] = ["r05_regression_gate"]
            case["executionCommandRefs"] = scenario["commandRefs"]
            case["bindingStatus"] = "ORIGINAL_PREDECESSOR_GATE_CLOSURE"
        elif not case["executionCommandRefs"]:
            raise ValueError(case["id"] + " 缺少实际执行绑定")
    inputs["executionBindings"] = bindings
    leaf_producer = "r06_z_leaf_evidence"
    inputs["leafBindings"] = {}
    inputs["partialLeafBindings"] = {}
    for leaf in leaves:
        reviewed = REVIEWED_LEAVES.get(leaf["id"])
        if reviewed is None:
            row = next(row for row in inventory["items"] if row["id"] == leaf["id"])
            groups = proof_groups[leaf["id"]]
            if row["status"] != "READY":
                if groups:
                    inputs["partialLeafBindings"][leaf["id"]] = groups
                continue
            reviewed = {"assertions": row["assertions"], "groups": []}
            # 中央登记按原索引绑定；仅全部原断言具备完整证明的条目进入正式合同。
            groups.sort(key=lambda group: int(group["case"].rsplit("-", 1)[1]))
            inputs["leafBindings"][leaf["id"]] = groups
            refs = sorted({key for group in groups for key in group["commandRefs"]})
            leaf.update({"basisKind": "full_original_behavior", "evidenceCommandRefs": [*refs, leaf_producer],
                         "assertionContract": {"producerCommand": leaf_producer, "evidencePath": "{EVIDENCE}/R06_LEAVES/leaf-cases.json",
                                               "cases": [{"case": group["case"], "expect": 1} for group in groups]},
                         "originalAssertionCases": [[group["case"]] for group in groups],
                         "evidenceRequired": "仅完整原断言登记可用于整叶验收；本轮实际具名检查及原始日志必须与同一候选一致。"})
            continue
        if reviewed["assertions"] != leaf["r00Assertions"]:
            raise ValueError(leaf["id"] + " 原断言改变，旧语义对应不能沿用")
        groups = []
        for index, group in enumerate(reviewed["groups"]):
            keys = []
            for suite, name in group["tests"]:
                matches = [key for key, binding in bindings.items()
                           if binding.get("package") == "lingxi-service" and binding.get("suite") == suite and binding.get("test") == name]
                if len(matches) != 1:
                    raise ValueError(leaf["id"] + " 逐断言证据没有唯一实际命令：" + suite + "::" + name)
                keys.append(matches[0])
            groups.append({"case": leaf["id"] + f"-assertion-{index:02}", "expect": 1,
                           "assertion": leaf["r00Assertions"][index], "commandRefs": keys,
                           "evidenceMeaning": group["meaning"]})
        if len(groups) != len(leaf["r00Assertions"]):
            raise ValueError(leaf["id"] + " 必须逐条覆盖全部原断言")
        inputs["leafBindings"][leaf["id"]] = groups
        refs = sorted({key for group in groups for key in group["commandRefs"]})
        leaf.update({"basisKind": "full_original_behavior", "evidenceCommandRefs": [*refs, leaf_producer],
                     "assertionContract": {"producerCommand": leaf_producer,
                                           "evidencePath": "{EVIDENCE}/R06_LEAVES/leaf-cases.json",
                                           "cases": [{"case": group["case"], "expect": group["expect"]} for group in groups]},
                     "originalAssertionCases": [[group["case"]] for group in groups],
                     "evidenceRequired": "每条原断言由列出的具名真实测试共同证明；生产者核对同候选、本轮命令与原始日志摘要后重算，失败或未执行均不能通过。"})
    commands[leaf_producer] = {"argv": ["python3", "-B", "scripts/rust-tauri/r06_stage_inputs.py", "--leaf-evidence",
                                       "--binding-evidence", "{EVIDENCE}/R06_BINDINGS", "--evidence", "{EVIDENCE}/R06_LEAVES"],
                               "timeoutSecs": 120, "evidencePaths": ["{EVIDENCE}/R06_LEAVES/leaf-cases.json"]}
    next(row for row in scenarios if row["id"] == "R06-AR-F14-C02")["commandRefs"].append(leaf_producer)
    scenarios.append({"id": "R06-SUP-R05REG", "requirement": "REQUIRED", "commandRefs": ["r05_regression_gate"]})
    graph = {"schemaVersion": 1, "stage": "R06", "resultVersion": "lingxi.xtask.verify-stage.v1",
             "defaultTimeoutSecs": 1800, "commands": commands, "scenarios": scenarios,
             "supplementalLeafScenarios": leaves, "inputDigestSha256": sha(inputs)}
    return inputs, graph


def save(path: Path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def file_sha(path: Path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def candidate_snapshot(root: Path, evidence: Path):
    """绑定实际源码字节及删除项；证据产物不冒充源码，也不进入自己的摘要。"""
    root = root.resolve()
    evidence = evidence.resolve()
    if evidence == root:
        raise ValueError("证据目录不能是仓库根目录")
    relative_evidence = evidence.relative_to(root) if evidence.is_relative_to(root) else None
    inventory = subprocess.run(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
                               cwd=root, capture_output=True, check=True).stdout
    head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, capture_output=True, check=True).stdout.decode().strip()
    entries = []
    for raw in sorted(set(inventory.split(b"\0")) - {b""}):
        relative = Path(os.fsdecode(raw))
        if relative.parts[0] == "artifacts" or (relative_evidence is not None and relative.is_relative_to(relative_evidence)):
            continue
        if relative.is_absolute() or any(part in (".", "..") for part in relative.parts):
            raise ValueError("候选源码路径不合法")
        path = root / relative
        for component in (path, *path.parents):
            if component == root:
                break
            if component.is_symlink():
                raise ValueError("源码绑定不允许符号链接：" + os.fsdecode(raw))
        entry = {"pathBytesHex": raw.hex(), "path": os.fsdecode(raw)}
        if not path.exists():
            entry.update({"kind": "deleted", "sha256": None})
        else:
            before = path.stat()
            if not path.is_file():
                raise ValueError("候选源码不是普通文件：" + os.fsdecode(raw))
            entry.update({"kind": "file", "sha256": file_sha(path), "mode": before.st_mode & 0o777})
            after = path.stat()
            if (before.st_ino, before.st_size, before.st_mtime_ns, before.st_mode) != (after.st_ino, after.st_size, after.st_mtime_ns, after.st_mode):
                raise ValueError("源码在摘要读取期间改变：" + os.fsdecode(raw))
        entries.append(entry)
    value = {"schema": "lingxi.r06.local-source-binding.v1", "gitHead": head, "files": entries,
             "policy": "已跟踪及非忽略新增文件的实际字节，保留删除项；只排除 artifacts 产物根及本次证据目录。正式阶段还须使用 xtask 自己的完整候选绑定。"}
    return {**value, "digestSha256": sha(value)}


def synthetic_env(root: Path, command_dir: Path):
    """每条命令独占合成资料与临时目录；保留宿主工具链缓存，避免复制依赖。"""
    env = os.environ.copy()
    runtime = command_dir / "runtime"
    home, temporary, raw = runtime / "home", runtime / "tmp", command_dir / "raw"
    for path in (home, temporary, raw):
        path.mkdir(parents=True, exist_ok=True)
    # HOME 保留给已有工具链；所有业务和测试资料只进入专项 LINGXI_HOME/TMPDIR。
    env.update({"LINGXI_HOME": str(home), "TMPDIR": str(temporary), "TMP": str(temporary), "TEMP": str(temporary),
                "LINGXI_PRODUCT_DIR": str(root / "lib"), "CARGO_TERM_COLOR": "never", "RUST_TEST_THREADS": "1",
                "LINGXI_F05_LOCAL_ROOT": str(runtime / "f05-local"), "R06_T08_RAW_OUT": str(raw / "t08"),
                "R06_T08_EVAL_OUT": str(raw / "t08-evaluations.jsonl")})
    for name in ("W01_EVIDENCE", "W09_EVIDENCE", "W10_EVIDENCE", "W11_EVIDENCE", "LINGXI_W03_EVIDENCE",
                 "LINGXI_F05_EVIDENCE", "LINGXI_F08_EVIDENCE"):
        target = raw / name.lower()
        target.mkdir(parents=True, exist_ok=True)
        env[name] = str(target)
    # 这些仅由具体父测试给子进程注入；宿主遗留变量不能启动故障助手。
    for name in ("LINGXI_W02_CRASH_HOME", "LINGXI_W02_CRASH_STAGE", "LINGXI_W02_KEEP_HOME_ALIAS",
                 "F12_IO_ROOT", "F12_IO_CACHE", "F12_IO_LIMIT", "F12_CRASH_DB", "F12_GUARD_ROOT", "F12_GUARD_CACHE"):
        env.pop(name, None)
    return env


def stop_process_tree(process):
    if os.name == "posix":
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            pass
        # 主进程可能先退出；组内仍存活的 Cargo/测试子进程也必须停止。
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=5)
    else:
        subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], capture_output=True, timeout=10)
        process.wait(timeout=5)


def inspect_output(binding, stdout: str, stderr: str):
    text = stdout + "\n" + stderr
    if binding["kind"] == "vitest_test":
        try:
            report = json.loads(stdout)
        except (ValueError, TypeError):
            return {"valid": False, "reason": "界面检查缺少真实框架结构化结果"}
        files = report.get("testResults", [])
        expected_path = proof_source_path(binding)
        matched_file = len(files) == 1 and Path(files[0].get("name", "")).as_posix().endswith("/" + expected_path.as_posix())
        assertions = files[0].get("assertionResults", []) if matched_file else []
        passed = [row for row in assertions if row.get("status") == "passed"]
        selected = [row for row in assertions if row.get("fullName") == binding["test"]]
        valid = (report.get("success") is True and report.get("numPassedTests") == 1
                 and report.get("numFailedTests") == 0 and len(passed) == len(selected) == 1
                 and selected[0].get("status") == "passed"
                 and passed[0].get("fullName") == binding["test"]
                 and all(row.get("status") != "failed" for row in assertions))
        return {"valid": valid, "expectedTest": binding["test"], "matchedExpectedSourceFile": matched_file,
                "passedTestNames": [row.get("fullName") for row in passed],
                "reason": "" if valid else "界面检查必须精确通过登记的一个实际用例；零匹配、跳过或错绑均拒绝"}
    if binding["kind"] == "script":
        found = re.search(binding["stdoutPattern"], stdout, re.MULTILINE) is not None
        return {"valid": found, "matchedExpectedScriptResult": found,
                "reason": "" if found else "脚本没有产生登记的实际检查结论"}
    running = re.findall(r"^running (\d+) tests?$", text, re.MULTILINE)
    summary = re.findall(r"^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored;", text, re.MULTILINE)
    passed = re.findall(r"^test (\S+) \.\.\. ok\s*$", text, re.MULTILINE)
    valid = running == ["1"] and summary == [("ok", "1", "0", "0")] and passed == [binding["test"]]
    return {"valid": valid, "expectedTest": binding["test"], "runningCounts": running,
            "resultSummaries": summary, "passedTestNames": passed,
            "reason": "" if valid else "必须精确执行登记的一个测试；0 匹配、忽略、失败或名称错绑均不能通过"}


def execute_binding(root: Path, evidence: Path, binding, index: int, timeout: float, initial_candidate):
    started = time.monotonic()
    command_dir = evidence / f"command-{index:03}"
    command_dir.mkdir()
    preparation = None
    if binding["kind"] == "rust_desktop_test":
        preparation_dir = command_dir / "frontend-preparation"
        preparation_dir.mkdir()
        preparation_binding = desktop_frontend_binding()
        preparation = execute_binding(root, preparation_dir, preparation_binding, 1,
                                      min(timeout, preparation_binding["timeoutSecs"]), initial_candidate)
    env = synthetic_env(root, command_dir)
    if binding.get("suite") == "r06_original_persona_proofs":
        # 人格模板只在本检查的合成目录准备，不能写入仓库产品材料。
        product = command_dir / "runtime" / "persona-product"
        product.mkdir()
        env["LINGXI_PRODUCT_DIR"] = str(product)
    stdout_path, stderr_path = command_dir / "stdout.log", command_dir / "stderr.log"
    before = candidate_snapshot(root, evidence)
    record = {"binding": binding, "argv": binding["argv"], "cwd": str(root),
              "sourceBeforeSha256": before["digestSha256"], "exitCode": None, "timedOut": False,
              "stdout": str(stdout_path), "stderr": str(stderr_path), "timeoutSecs": timeout,
              "syntheticEnvironment": {key: env[key] for key in ("LINGXI_HOME", "TMPDIR", "LINGXI_PRODUCT_DIR",
                                                                 "W01_EVIDENCE", "W09_EVIDENCE", "W10_EVIDENCE", "W11_EVIDENCE",
                                                                 "LINGXI_W03_EVIDENCE", "LINGXI_F05_EVIDENCE", "LINGXI_F08_EVIDENCE",
                                                                 "R06_T08_RAW_OUT", "R06_T08_EVAL_OUT")}}
    if preparation is not None:
        record["preparation"] = preparation
    with stdout_path.open("xb") as stdout, stderr_path.open("xb") as stderr:
        try:
            if preparation is not None and not preparation["localPassed"]:
                raise ValueError("真实桌面前端构建前置命令失败；不执行依赖旧产物的桌面测试")
            if before["digestSha256"] != initial_candidate["digestSha256"]:
                raise ValueError("开始下一命令前源码已改变")
            remaining = timeout - (time.monotonic() - started)
            if remaining <= 0:
                raise ValueError("前置命令后执行预算耗尽；不启动下一命令")
            executable = shutil.which(binding["argv"][0], path=env.get("PATH"))
            if executable is None and binding["argv"][0] == "rustup":
                bundled = Path.home() / ".cargo/bin/rustup"
                if bundled.is_file():
                    executable = str(bundled)
            if executable is None:
                raise FileNotFoundError("执行程序不存在：" + binding["argv"][0])
            actual_argv = [executable, *binding["argv"][1:]]
            record["actualArgv"] = actual_argv
            process = subprocess.Popen(actual_argv, cwd=root, env=env, stdout=stdout, stderr=stderr,
                                       start_new_session=os.name == "posix")
            record["pid"] = process.pid
            try:
                record["exitCode"] = process.wait(timeout=remaining)
            except subprocess.TimeoutExpired:
                record["timedOut"] = True
                stop_process_tree(process)
                record["exitCode"] = process.returncode
        except (OSError, ValueError, subprocess.SubprocessError) as error:
            record["executionError"] = str(error)
            stderr.write((str(error) + "\n").encode())
    record["elapsedSecs"] = round(time.monotonic() - started, 3)
    output = inspect_output(binding, stdout_path.read_text(errors="replace"), stderr_path.read_text(errors="replace"))
    record.update({"outputCheck": output, "stdoutSha256": file_sha(stdout_path), "stderrSha256": file_sha(stderr_path)})
    try:
        after = candidate_snapshot(root, evidence)
        record["sourceAfterSha256"] = after["digestSha256"]
        record["sourceStable"] = before["digestSha256"] == after["digestSha256"] == initial_candidate["digestSha256"]
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        record.update({"sourceAfterSha256": None, "sourceStable": False, "sourceBindingError": str(error)})
    record["localPassed"] = record["exitCode"] == 0 and not record["timedOut"] and output["valid"] and record["sourceStable"]
    save(command_dir / "command-result.json", record)
    return record


def execute_case(root: Path, evidence: Path, case):
    if case["bindingStatus"] in ("CURRENT_STAGE_AGGREGATE_EXECUTION", "ORIGINAL_PREDECESSOR_GATE_CLOSURE"):
        raise ValueError("该检查消费正式阶段的本轮命令全集，必须经 xtask verify-stage R06 执行，不能独立递归或伪造聚合结果")
    before = candidate_snapshot(root, evidence)
    save(evidence / "candidate-before.json", before)
    records = []
    deadline = time.monotonic() + 3500
    for index, binding in enumerate(case["supportingTests"], 1):
        remaining = deadline - time.monotonic()
        if remaining <= 0 or (records and not records[-1]["sourceStable"]):
            break
        records.append(execute_binding(root, evidence, binding, index, min(binding["timeoutSecs"], remaining), before))
    try:
        after = candidate_snapshot(root, evidence)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        after = {"digestSha256": None, "sourceBindingError": str(error)}
    save(evidence / "candidate-after.json", after)
    complete_execution = bool(case["supportingTests"]) and len(records) == len(case["supportingTests"])
    local_pass = complete_execution and all(record["localPassed"] for record in records) and before["digestSha256"] == after["digestSha256"]
    local_status = "LOCAL_SUPPORTING_TESTS_PASS" if local_pass else "LOCAL_SUPPORTING_TESTS_FAIL" if records else "NOT_EXECUTED_NO_SUPPORTING_BINDING"
    # 本入口仅给实际命令的执行结论。完整阶段由 xtask 的全部原义务共同决定，
    # 不再用固定 False 让真实成功永远无法结算，也不以单个检查给整个阶段放行。
    exit_code = 0 if local_pass else 1
    result = {"id": case["id"], "status": local_status, "localPassed": local_pass,
              "formalAcceptanceStatus": "NOT_A_STAGE_VERDICT",
              "supportingTests": case["supportingTests"], "commands": records, "commandsAttempted": len(records),
              "commandsExecuted": any("pid" in record for record in records),
              "testsExecuted": any(any(int(count) > 0 for count in record["outputCheck"].get("runningCounts", [])) for record in records),
              "completeSupportingExecution": complete_execution, "sourceCandidateSha256": before["digestSha256"],
              "sourceStable": before["digestSha256"] == after["digestSha256"], "exitCode": exit_code,
              "reason": "本次只反映真实命令执行；正式阶段还必须逐条通过原始义务、证据与候选绑定检查。"}
    save(evidence / "case-result.json", result)
    print(case["id"] + "：" + local_status + "；完整语义状态=" + result["formalAcceptanceStatus"])
    return exit_code


def execute_registered_binding(root, evidence, key, binding):
    before = candidate_snapshot(root, evidence)
    save(evidence / "candidate-before.json", before)
    record = execute_binding(root, evidence, binding, 1, binding["timeoutSecs"], before)
    result = {"schema": "lingxi.r06.executed-binding.v1", "key": key,
              "sourceCandidateSha256": before["digestSha256"], "binding": binding,
              "command": record, "passed": record["localPassed"],
              "stageVerdict": "NOT_A_STAGE_VERDICT"}
    save(evidence / "binding-result.json", result)
    print(key + "：" + ("PASS" if result["passed"] else "FAIL") + "；正式阶段仍核对全部原义务")
    return 0 if result["passed"] else 1


def read_executed_binding(root, binding_evidence, key, binding, candidate):
    raw_directory = binding_evidence / key
    if raw_directory.is_symlink():
        raise ValueError("实际执行证据不允许符号链接")
    directory = raw_directory.resolve()
    result_path = directory / "binding-result.json"
    if directory.is_symlink() or result_path.is_symlink():
        raise ValueError("实际执行证据不允许符号链接")
    result = json.loads(result_path.read_text())
    if result.get("schema") != "lingxi.r06.executed-binding.v1" or result.get("key") != key or result.get("binding") != binding:
        raise ValueError(key + " 实际命令身份错绑")
    if result.get("sourceCandidateSha256") != candidate["digestSha256"]:
        raise ValueError(key + " 不是当前候选的实际执行证据")
    command = result["command"]
    if result.get("passed") is not True:
        raise ValueError(key + " 没有同一候选的完整真实成功结算")
    validate_executed_command(directory, command, binding, candidate, key)
    return {"key": key, "resultPath": str(result_path), "resultSha256": file_sha(result_path),
            "stdoutSha256": command["stdoutSha256"], "stderrSha256": command["stderrSha256"],
            "observedTest": binding.get("test"), "sourceCandidateSha256": candidate["digestSha256"]}


def validate_executed_command(directory, command, binding, candidate, key):
    if command.get("binding") != binding or command.get("argv") != binding["argv"]:
        raise ValueError(key + " 实际执行命令与登记不一致")
    if binding["kind"] == "rust_desktop_test":
        preparation = command.get("preparation")
        if not isinstance(preparation, dict):
            raise ValueError(key + " 缺少实际桌面前端构建前置命令")
        validate_executed_command(directory, preparation, desktop_frontend_binding(), candidate,
                                  key + " frontend preparation")
    elif "preparation" in command:
        raise ValueError(key + " 出现未登记的前置命令")
    logs = []
    for label in ("stdout", "stderr"):
        path = Path(command[label])
        if path.is_symlink() or not path.resolve().is_relative_to(directory):
            raise ValueError(key + " 原始日志越出本轮命令目录")
        if file_sha(path) != command[label + "Sha256"]:
            raise ValueError(key + " 原始日志摘要改变")
        logs.append(path.read_text(errors="replace"))
    output = inspect_output(binding, *logs)
    if (command.get("localPassed") is not True
            or command.get("exitCode") != 0 or command.get("timedOut") is not False
            or not isinstance(command.get("pid"), int) or command["pid"] <= 0
            or command.get("sourceStable") is not True
            or command.get("sourceBeforeSha256") != candidate["digestSha256"]
            or command.get("sourceAfterSha256") != candidate["digestSha256"]
            or not output["valid"] or sha(output) != sha(command.get("outputCheck"))):
        raise ValueError(key + " 没有同一候选的完整真实成功结算")


def produce_leaf_evidence(root, evidence, binding_evidence, inputs, proof_leaf_ids=None):
    selected = None
    if proof_leaf_ids is not None:
        original_ids = {row["id"] for row in inputs["originalProofInventory"]["items"]}
        if (not isinstance(proof_leaf_ids, list) or not proof_leaf_ids
                or not all(isinstance(value, str) for value in proof_leaf_ids)
                or len(set(proof_leaf_ids)) != len(proof_leaf_ids)
                or not set(proof_leaf_ids).issubset(original_ids)):
            raise ValueError("证明范围必须是非空、不重复的原要求id清单")
        selected = set(proof_leaf_ids)
    candidate = candidate_snapshot(root, evidence)
    observed, errors = {}, {}
    # 先验证本目录存在的每个登记结果，避免不被当前叶消费的坏日志被掩盖。
    if not binding_evidence.is_dir() or binding_evidence.is_symlink():
        raise ValueError("缺少本轮实际执行证据目录")
    for directory in sorted(binding_evidence.iterdir()):
        key = directory.name
        binding = inputs["executionBindings"].get(key)
        if binding is None:
            raise ValueError("本轮出现未登记的实际执行证据：" + key)
        try:
            observed[key] = read_executed_binding(root, binding_evidence, key, binding, candidate)
        except (OSError, ValueError, KeyError, TypeError) as error:
            errors[key] = str(error)
    cases, partial_proofs = [], []
    for collection, scope in ((inputs["leafBindings"], "COMPLETE_ORIGINAL_LEAF"),
                              (inputs.get("partialLeafBindings", {}), "PARTIAL_ASSERTION_ONLY_NOT_LEAF_ACCEPTANCE")):
        for leaf_id, groups in collection.items():
            if selected is not None and leaf_id not in selected:
                continue
            for group in groups:
                missing = [key for key in group["commandRefs"] if key not in observed]
                if group.get("coverage") == "PARTIAL":
                    partial_proofs.append({"case": group["case"], "leafId": leaf_id,
                                           "originalAssertion": group["assertion"],
                                           "originalAssertionSatisfied": False,
                                           "checkedSubsetPassed": not missing,
                                           "acceptanceScope": "CHECKED_SUBSET_ONLY_NOT_ORIGINAL_ASSERTION_ACCEPTANCE",
                                           "evidenceMeaning": group["evidenceMeaning"],
                                           "missingClauses": group["missingClauses"],
                                           "executions": [observed[key] for key in group["commandRefs"] if key in observed],
                                           "missingOrInvalidCommandRefs": missing})
                    continue
                cases.append({"case": group["case"], "expect": group["expect"],
                              "actual": 1 if not missing else None, "ok": not missing,
                              "leafId": leaf_id, "originalAssertion": group["assertion"],
                              "acceptanceScope": scope, "evidenceMeaning": group["evidenceMeaning"],
                              "executions": [observed[key] for key in group["commandRefs"] if key in observed],
                              "missingOrInvalidCommandRefs": missing})
    after = candidate_snapshot(root, evidence)
    stable = candidate["digestSha256"] == after["digestSha256"]
    passed = (bool(cases or partial_proofs) and all(row["ok"] for row in cases)
              and all(row["checkedSubsetPassed"] for row in partial_proofs) and not errors and stable)
    unbound = [leaf["id"] for leaf in inputs["requiredLeaves"] if leaf["id"] not in inputs["leafBindings"]]
    result = {"schema": "lingxi.leaf-case-results.v1", "producedBy": "r06_stage_inputs.py --leaf-evidence",
              "sourceCandidateSha256": candidate["digestSha256"], "sourceStable": stable,
              "cases": cases, "partialProofs": partial_proofs, "executionErrors": errors, "unboundOriginalLeaves": unbound,
              "stageVerdict": "NOT_A_STAGE_VERDICT", "passedRegisteredAssertions": passed,
              "executionScope": "SELECTED_ORIGINAL_REQUIREMENTS" if selected is not None else "ALL_REGISTERED_ASSERTIONS",
              "selectedOriginalLeaves": sorted(selected) if selected is not None else None,
              "excludedFromExecutionScope": [leaf["id"] for leaf in inputs["requiredLeaves"]
                                             if selected is not None and leaf["id"] not in selected],
              "note": "仅已审阅的逐断言证据由本轮原始日志重算；缺少映射的原义务仍由阶段门禁保持BLOCKED。"}
    save(evidence / "leaf-cases.json", result)
    print("R06 已登记逐断言证据：" + ("PASS" if passed else "FAIL") + f"；未绑定原义务 {len(unbound)} 项仍未完成")
    return 0 if passed else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--generate", action="store_true")
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--case")
    mode.add_argument("--binding")
    mode.add_argument("--leaf-evidence", action="store_true")
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--binding-evidence", type=Path)
    parser.add_argument("--proof-leaf-ids", type=Path)
    args = parser.parse_args()
    try:
        args.root = args.root.resolve()
        if args.proof_leaf_ids is not None and not args.leaf_evidence:
            raise ValueError("证明范围只能用于局部逐断言证明，不能改变阶段验收")
        inputs, graph = build(args.root)
        if args.generate:
            save(args.root / INPUT_PATH, inputs)
            save(args.root / MAP_PATH, graph)
            print("R06 输入与阶段图已登记；本操作没有执行验收。")
            return 0
        if load(args.root, INPUT_PATH) != inputs or load(args.root, MAP_PATH) != graph:
            raise ValueError("R06 图或镜像输入与原义务/当前具体测试不一致；先重新生成，不得沿用旧证据")
        if args.evidence is None:
            raise ValueError("执行检查必须指定新的证据目录")
        if args.evidence.is_symlink():
            raise ValueError("证据目录不能是符号链接")
        args.evidence = args.evidence.resolve()
        if args.evidence.exists() and any(args.evidence.iterdir()):
            raise ValueError("证据目录已有内容，拒绝复用旧结果")
        if args.check:
            save(args.evidence / "inputs.json", {"schema": inputs["schema"], "inputsDigestSha256": sha(inputs),
                                               "inputSnapshot": inputs, "status": "INPUTS_MATCH_NOT_STAGE_VERDICT"})
            return 0
        if args.binding:
            binding = inputs["executionBindings"].get(args.binding)
            if binding is None:
                raise ValueError("未知实际检查绑定：" + args.binding)
            return execute_registered_binding(args.root, args.evidence, args.binding, binding)
        if args.leaf_evidence:
            if args.binding_evidence is None:
                raise ValueError("逐断言汇总必须指定本轮实际执行目录")
            proof_ids = json.loads(args.proof_leaf_ids.read_text()) if args.proof_leaf_ids is not None else None
            return produce_leaf_evidence(args.root, args.evidence, args.binding_evidence, inputs, proof_ids)
        case = next((case for case in inputs["cases"] if case["id"] == args.case), None)
        if case is None:
            raise ValueError("未知 R06 检查：" + args.case)
        return execute_case(args.root, args.evidence, case)
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print("R06 输入拒绝：" + str(error))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

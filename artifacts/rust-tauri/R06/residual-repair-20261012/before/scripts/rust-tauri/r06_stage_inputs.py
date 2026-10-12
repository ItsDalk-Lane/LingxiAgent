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


# 名称来自现有具名测试；它们是明确的局部证据，不代替完整逐叶语义验收。
ACCEPTANCE_TESTS = {
    "R06-A01": [("r06_t01_closed_loop", "sent_wire_system_bytes_match_the_observation_digests")],
    "R06-A02": [("r06_t01_context_compiler", "run_freeze_spans_turns_and_next_run_refreshes_material"),
                ("r06_t01_context_compiler", "budget_exceeded_fails_the_run_loudly")],
    "R06-A03": [("r06_t02_closed_loop", "a03_mid_run_compaction_lands_on_the_real_binary")],
    "R06-A04": [("r06_t02_closed_loop", "a04_summary_provider_failure_keeps_the_original_history")],
    "R06-A05": [("r06_t03_session_tree", "fork_copies_history_with_stable_ids_and_writes_stay_independent")],
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
            for name in names if not name.endswith("_child")]


def test_binding(suite, name):
    return {"package": "lingxi-service", "suite": suite, "test": name}


def toolchain(root: Path):
    match = re.search(r'^channel\s*=\s*"([^"]+)"', (root / "rust-toolchain.toml").read_text(), re.MULTILINE)
    if not match:
        raise ValueError("无法读取固定 Rust 工具链")
    return match.group(1)


def command_argv(root: Path, binding):
    kind = "--bin" if binding.get("kind") == "rust_unit_test" else "--test"
    return ["rustup", "run", toolchain(root), "cargo", "test", "--manifest-path", "rust/Cargo.toml", "--locked", "-p",
            binding["package"], kind, binding["suite"], binding["test"], "--", "--exact", "--nocapture"]


def special_bindings(identifier):
    if identifier == "R06-AR-F14-C01":
        return [{"kind": "script", "argv": ["python3", "-B", "scripts/rust-tauri/r02_registry_consistency.py", "--root", "."],
                 "stdoutPattern": r"^PASS: migrations\.rs MIGRATIONS == registry \(", "timeoutSecs": 120,
                 "evidenceMeaning": "严格核对源码迁移与登记；真实数据库升级、inspector 和幂等恢复仍未由此命令证明"}]
    if identifier == "R06-AR-F14-C04":
        return [{"kind": "rust_unit_test", "package": "xtask", "suite": "xtask", "test": name}
                for name in ("stage_map::map_tests::empty_scenario_set_is_a_hard_error",
                             "stage_map::map_tests::unknown_command_ref_is_a_hard_error",
                             "verify::runner_tests::missing_evidence_is_a_failure_even_when_exit_is_zero",
                             "candidate::tests::a_changed_checkpoint_cannot_be_erased_by_restoring_final_bytes")]
    return []


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
            if not selected:
                cases[-1]["unresolvedBindingReason"] = "完整 R06 阶段/前序闭包的专属语义映射尚未完成；本 case 不递归调用 R06，也不以局部命令冒充完整验收。"
    for case in cases:
        for binding in case["supportingTests"]:
            if binding.get("kind") != "script":
                binding.setdefault("kind", "rust_integration_test")
                binding["argv"] = command_argv(root, binding)
            binding.setdefault("timeoutSecs", 900)
        # 完整逐断言正反证据尚未绑定，局部实际执行结果不得自行升级。
        case["formalAcceptanceComplete"] = False
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
    for case in cases:
        key = "r06_case_" + case["id"].lower().replace("-", "_")
        relative = "{EVIDENCE}/R06_CASES/" + case["id"]
        commands[key] = {"argv": ["python3", "-B", "scripts/rust-tauri/r06_stage_inputs.py", "--case", case["id"], "--evidence", relative],
                         "timeoutSecs": 3600, "evidencePaths": [relative + "/case-result.json"]}
        scenarios.append({"id": case["id"], "requirement": "REQUIRED", "commandRefs": ["r06_inputs", key],
                          "note": "登记的是支持测试；完整行为绑定未确认时执行器明确返回非零，不能形成阶段 PASS。"})
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
    command_dir = evidence / f"command-{index:03}"
    command_dir.mkdir()
    env = synthetic_env(root, command_dir)
    stdout_path, stderr_path = command_dir / "stdout.log", command_dir / "stderr.log"
    before = candidate_snapshot(root, evidence)
    record = {"binding": binding, "argv": binding["argv"], "cwd": str(root),
              "sourceBeforeSha256": before["digestSha256"], "exitCode": None, "timedOut": False,
              "stdout": str(stdout_path), "stderr": str(stderr_path), "timeoutSecs": timeout,
              "syntheticEnvironment": {key: env[key] for key in ("LINGXI_HOME", "TMPDIR", "LINGXI_PRODUCT_DIR",
                                                                 "W01_EVIDENCE", "W09_EVIDENCE", "W10_EVIDENCE", "W11_EVIDENCE",
                                                                 "LINGXI_W03_EVIDENCE", "LINGXI_F05_EVIDENCE", "LINGXI_F08_EVIDENCE",
                                                                 "R06_T08_RAW_OUT", "R06_T08_EVAL_OUT")}}
    started = time.monotonic()
    with stdout_path.open("xb") as stdout, stderr_path.open("xb") as stderr:
        try:
            if before["digestSha256"] != initial_candidate["digestSha256"]:
                raise ValueError("开始下一命令前源码已改变")
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
                record["exitCode"] = process.wait(timeout=timeout)
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
    formal_complete = case["formalAcceptanceComplete"]
    # 退出 2 表示支持检查实际通过，但完整语义映射尚缺；阶段不得将它当正式 PASS。
    exit_code = 0 if local_pass and formal_complete else 2 if local_pass or not records else 1
    result = {"id": case["id"], "status": local_status, "localPassed": local_pass,
              "formalAcceptanceComplete": formal_complete, "formalAcceptanceStatus": "PASS" if local_pass and formal_complete else "BLOCKED_COMPLETE_BINDING_MISSING",
              "supportingTests": case["supportingTests"], "commands": records, "commandsAttempted": len(records),
              "commandsExecuted": any("pid" in record for record in records),
              "testsExecuted": any(any(int(count) > 0 for count in record["outputCheck"].get("runningCounts", [])) for record in records),
              "completeSupportingExecution": complete_execution, "sourceCandidateSha256": before["digestSha256"],
              "sourceStable": before["digestSha256"] == after["digestSha256"], "exitCode": exit_code,
              "reason": "本次只反映实际支持命令；原完整逐断言正反行为与正式验收仍须独立证明。"}
    save(evidence / "case-result.json", result)
    print(case["id"] + "：" + local_status + "；完整语义状态=" + result["formalAcceptanceStatus"])
    return exit_code


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--generate", action="store_true")
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--case")
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    try:
        args.root = args.root.resolve()
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
        case = next((case for case in inputs["cases"] if case["id"] == args.case), None)
        if case is None:
            raise ValueError("未知 R06 检查：" + args.case)
        return execute_case(args.root, args.evidence, case)
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print("R06 输入拒绝：" + str(error))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""F14-C04：隔离真实R06输入，验证生成、绑定、输出与候选拒绝行为。"""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True


REPOSITORY = Path(__file__).resolve().parents[3]
SCRIPT = Path("scripts/rust-tauri/r06_stage_inputs.py")
REGISTRY_SCRIPT = Path("scripts/rust-tauri/r02_registry_consistency.py")
MIGRATIONS = Path("rust/crates/lingxi-adapters/src/storage/migrations.rs")
REGISTRY = Path("docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json")


def import_stage(path):
    spec = importlib.util.spec_from_file_location("r06_stage_inputs_under_test", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class R06StageInputsContract(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="lingxi-f14-inputs-")
        self.root = Path(self.temporary.name)
        source = import_stage(REPOSITORY / SCRIPT)
        paths = {SCRIPT, REGISTRY_SCRIPT, MIGRATIONS, REGISTRY, Path("rust-toolchain.toml")}
        paths.update(value for key, value in vars(source).items()
                     if key.endswith("_PATH") and isinstance(value, Path))
        suites = {("lingxi-service", suite) for tests in source.ACCEPTANCE_TESTS.values()
                  for suite, _ in tests}
        suites.update(pair for pairs in source.ISSUE_SUITES.values() for pair in pairs)
        suites.add(("lingxi-adapters", "r06_ar_f14_migration_receipts"))
        inventory = json.loads((REPOSITORY / source.ORIGINAL_PROOF_PATH).read_text())
        tests = [test for row in inventory["items"]
                 for group in row.get("groups", []) for test in group["tests"]]
        paths.update(source.proof_source_path(test) for test in tests)
        if any(test.get("kind") == "rust_desktop_test" for test in tests):
            paths.add(source.DESKTOP_MANIFEST)
        paths.update(Path("rust/crates") / package / "tests" / (suite + ".rs")
                     for package, suite in suites)
        paths.update(path.relative_to(REPOSITORY)
                     for path in (REPOSITORY / "rust/crates/xtask/src").rglob("*.rs"))
        paths.add(Path(__file__).resolve().relative_to(REPOSITORY))
        for relative in paths:
            original = REPOSITORY / relative
            self.assertTrue(original.is_file(), f"真实输入不存在：{relative}")
            destination = self.root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(original, destination)
        self.stage = import_stage(self.root / SCRIPT)
        self.inputs, self.graph = self.stage.build(self.root)
        self.stage.save(self.root / self.stage.INPUT_PATH, self.inputs)
        self.stage.save(self.root / self.stage.MAP_PATH, self.graph)
        # 只在合成目录建立候选仓库，使真实git清单/删除项参与被测摘要。
        for argv in (["git", "init", "-q"], ["git", "add", "."],
                     ["git", "-c", "user.name=F14 Fixture", "-c", "user.email=f14@example.invalid",
                      "commit", "-qm", "isolated stage input fixture"]):
            subprocess.run(argv, cwd=self.root, capture_output=True, check=True)
        self.observations = []
        self.sequence = 0

    def tearDown(self):
        destination = os.environ.get("R06_STAGE_INPUT_TEST_EVIDENCE")
        if destination:
            destination = Path(destination) / self._testMethodName
            destination.mkdir(parents=True, exist_ok=True)
            (destination / "observations.json").write_text(
                json.dumps(self.observations, ensure_ascii=False, indent=2) + "\n",
                encoding="utf-8")
            if (self.root / "artifacts").is_dir():
                shutil.copytree(self.root / "artifacts", destination / "raw", dirs_exist_ok=True)
        self.temporary.cleanup()

    def cli_check(self, label, evidence=None):
        self.sequence += 1
        evidence = evidence or self.root / "artifacts" / f"{self.sequence:02}-{label}"
        argv = [sys.executable, "-B", str(self.root / SCRIPT), "--root", str(self.root),
                "--check", "--evidence", str(evidence)]
        result = subprocess.run(argv, cwd=self.root, capture_output=True, text=True, timeout=30)
        self.observations.append({"label": label, "argv": argv, "exitCode": result.returncode,
                                  "stdout": result.stdout, "stderr": result.stderr})
        return result, evidence

    def assert_positive(self, label="restored-positive"):
        result, evidence = self.cli_check(label)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        actual = json.loads((evidence / "inputs.json").read_text())
        self.assertEqual(actual["status"], "INPUTS_MATCH_NOT_STAGE_VERDICT")
        self.assertEqual(actual["inputSnapshot"], self.inputs)
        return evidence

    def registry_binding(self):
        return {"kind": "script", "argv": [sys.executable, "-B", str(REGISTRY_SCRIPT), "--root", "."],
                "stdoutPattern": r"^PASS: migrations\.rs MIGRATIONS == registry \(", "timeoutSecs": 30}

    def execute_registry(self, label, candidate=None):
        evidence = self.root / "artifacts" / label
        evidence.mkdir(parents=True)
        candidate = candidate or self.stage.candidate_snapshot(self.root, evidence)
        actual = self.stage.execute_binding(self.root, evidence, self.registry_binding(), 1, 30, candidate)
        self.observations.append({"label": label, "record": actual})
        return actual

    def cli_leaf_evidence(self, label, bindings):
        self.sequence += 1
        evidence = self.root / "artifacts" / f"{self.sequence:02}-{label}"
        argv = [sys.executable, "-B", str(self.root / SCRIPT), "--root", str(self.root),
                "--leaf-evidence", "--binding-evidence", str(bindings), "--evidence", str(evidence)]
        result = subprocess.run(argv, cwd=self.root, capture_output=True, text=True, timeout=30)
        self.observations.append({"label": label, "argv": argv, "exitCode": result.returncode,
                                  "stdout": result.stdout, "stderr": result.stderr})
        return result, evidence

    def test_build_preserves_required_cases_and_shares_one_execution_binding(self):
        expected = {f"R06-A{i:02}" for i in range(1, 17)}
        expected.update(f"R06-AR-F{i:02}-C{case:02}" for i in range(1, 16) for case in range(1, 5))
        self.assertEqual({case["id"] for case in self.inputs["cases"]}, expected)
        self.assertEqual({row["id"] for row in self.graph["scenarios"]}, expected | {"R06-SUP-R05REG"})
        self.assertTrue(all(row["requirement"] == "REQUIRED" for row in self.graph["scenarios"]))
        bindings = self.inputs["executionBindings"]
        unique = {self.stage.binding_key(binding): binding for case in self.inputs["cases"]
                  for binding in case["supportingTests"]}
        for row in self.inputs["originalProofInventory"]["items"]:
            for group in row.get("groups", []):
                if group.get("manualEvidence", {}).get("kind") == "human_in_loop_gui_receipt":
                    continue  # 人工交互绑定不进自动命令集（阶段裁定 a）
                for test in group["tests"]:
                    binding = dict(test, kind=test.get("kind", "rust_integration_test"), timeoutSecs=900)
                    binding["argv"] = self.stage.command_argv(self.root, binding)
                    unique[self.stage.binding_key(binding)] = binding
        self.assertEqual(bindings, unique)
        scenarios = {row["id"]: row for row in self.graph["scenarios"]}
        self.assertEqual(scenarios["R06-AR-F14-C02"]["commandRefs"], ["r06_inputs", *sorted(bindings), "r06_z_leaf_evidence"])
        self.assertEqual(scenarios["R06-AR-F14-C03"]["commandRefs"], ["r05_regression_gate"])
        self.assert_positive("unchanged-positive")

    def test_deleting_required_scenario_is_rejected_and_restoration_passes(self):
        path = self.root / self.stage.MAP_PATH
        original = path.read_bytes()
        altered = json.loads(original)
        missing = "R06-AR-F03-C02"
        altered["scenarios"] = [row for row in altered["scenarios"] if row["id"] != missing]
        self.stage.save(path, altered)
        result, _ = self.cli_check("missing-required-scenario")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("不一致", result.stdout)
        path.write_bytes(original)
        self.assert_positive()

    def test_deleting_original_required_case_is_rejected_and_restoration_passes(self):
        path = self.root / self.stage.ISSUE_PATH
        original = path.read_bytes()
        altered = json.loads(original)
        issue = next(row for row in altered["issues"] if row["id"] == "R06-AR-F03")
        issue["self_check"]["cases"].pop()
        self.stage.save(path, altered)
        with self.assertRaisesRegex(ValueError, "R06-AR-F03.*集合不一致"):
            self.stage.build(self.root)
        result, _ = self.cli_check("missing-original-case")
        self.assertNotEqual(result.returncode, 0)
        path.write_bytes(original)
        self.assert_positive()

    def test_original_proof_inventory_cannot_drop_or_rewrite_original_requirements(self):
        path = self.root / self.stage.ORIGINAL_PROOF_PATH
        original = path.read_bytes()
        for label in ("missing-original", "rewritten-original", "rewritten-original-outcome"):
            altered = json.loads(original)
            if label == "missing-original":
                altered["items"].pop()
            elif label == "rewritten-original":
                altered["items"][0]["assertions"][0] = "仅保留成功路径"
            else:
                altered["items"][0]["then"] = "删除原有额外结果要求"
            self.stage.save(path, altered)
            with self.assertRaises(ValueError):
                self.stage.build(self.root)
            result, _ = self.cli_check(label)
            self.assertNotEqual(result.returncode, 0)
        path.write_bytes(original)
        self.assert_positive()

    def test_partial_proof_cannot_release_a_complete_original_requirement(self):
        path = self.root / self.stage.ORIGINAL_PROOF_PATH
        original = path.read_bytes()
        altered = json.loads(original)
        row = next(row for row in altered["items"] if row["status"] == "READY")
        row["groups"][0]["coverage"] = "PARTIAL"
        row["groups"][0]["missingClauses"] = ["合成反例：原拒绝路径尚未覆盖"]
        self.stage.save(path, altered)
        with self.assertRaisesRegex(ValueError, "尚有缺失原断言"):
            self.stage.build(self.root)
        result, _ = self.cli_check("partial-cannot-release-full-leaf")
        self.assertNotEqual(result.returncode, 0)
        path.write_bytes(original)
        self.assert_positive()

    def test_missing_original_assertion_cannot_release_a_complete_requirement(self):
        path = self.root / self.stage.ORIGINAL_PROOF_PATH
        original = path.read_bytes()
        altered = json.loads(original)
        row = next(row for row in altered["items"] if row["status"] == "READY")
        row["groups"].pop()
        self.stage.save(path, altered)
        with self.assertRaisesRegex(ValueError, "尚有缺失原断言"):
            self.stage.build(self.root)
        result, _ = self.cli_check("missing-original-assertion")
        self.assertNotEqual(result.returncode, 0)
        path.write_bytes(original)
        self.assert_positive()

    def test_postponed_leaf_due_mirror_is_rejected_and_original_due_is_preserved(self):
        path = self.root / self.stage.INPUT_PATH
        original = path.read_bytes()
        altered = json.loads(original)
        self.assertTrue(altered["requiredLeaves"])
        leaf = altered["requiredLeaves"][0]
        original_due = leaf["r00Due"]
        leaf["r00Due"] = {"forgedStage": "R99", "originalDue": original_due}
        self.stage.save(path, altered)
        result, _ = self.cli_check("forged-postponed-due")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("不一致", result.stdout)
        fresh, _ = self.stage.build(self.root)
        self.assertEqual(fresh["requiredLeaves"][0]["r00Due"], original_due)
        path.write_bytes(original)
        self.assert_positive()

    def test_wrong_test_binding_is_rejected_and_restoration_passes(self):
        binding = self.inputs["cases"][0]["supportingTests"][0]
        path = self.root / "rust/crates" / binding["package"] / "tests" / (binding["suite"] + ".rs")
        original = path.read_bytes()
        text = original.decode()
        needle = "fn " + binding["test"] + "("
        self.assertIn(needle, text)
        path.write_text(text.replace(needle, "fn wrong_binding_test(", 1), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "具名测试不存在"):
            self.stage.build(self.root)
        result, _ = self.cli_check("wrong-binding")
        self.assertNotEqual(result.returncode, 0)
        path.write_bytes(original)
        self.assert_positive()

    def test_zero_match_wrong_name_and_ignored_output_cannot_pass(self):
        binding = {"kind": "rust_integration_test", "test": "registered_actual_test"}
        positive = "running 1 test\ntest registered_actual_test ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n"
        variants = {
            "zero-match": "running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s\n",
            "wrong-name": positive.replace("registered_actual_test", "different_test"),
            "ignored": "running 1 test\ntest registered_actual_test ... ignored\ntest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
            "multiple-runs": positive + positive,
        }
        self.assertTrue(self.stage.inspect_output(binding, positive, "")["valid"])
        for label, stdout in variants.items():
            with self.subTest(label=label):
                check = self.stage.inspect_output(binding, stdout, "")
                self.assertFalse(check["valid"], check)
                self.observations.append({"label": label, "stdout": stdout, "outputCheck": check})
        self.assertTrue(self.stage.inspect_output(binding, positive, "")["valid"])

    def test_real_registry_missing_migration_fails_and_restoration_passes(self):
        first = self.execute_registry("registry-positive")
        self.assertTrue(first["localPassed"], first)
        path = self.root / REGISTRY
        original = path.read_bytes()
        altered = json.loads(original)
        removed = altered["new_persistence_points"][0]["migrations"].pop()
        self.stage.save(path, altered)
        negative = self.execute_registry("registry-missing-migration")
        self.assertNotEqual(negative["exitCode"], 0, negative)
        self.assertFalse(negative["localPassed"], negative)
        self.assertIn(f"v{removed['version']}", Path(negative["stderr"]).read_text())
        path.write_bytes(original)
        self.assertTrue(self.execute_registry("registry-restored-positive")["localPassed"])

    def test_native_binding_requires_actual_frontend_preparation(self):
        binding = next(binding for binding in self.inputs["executionBindings"].values()
                       if binding["kind"] == "rust_desktop_test")
        candidate = self.stage.candidate_snapshot(self.root, self.root / "artifacts")
        command = {"binding": binding, "argv": binding["argv"]}
        with self.assertRaisesRegex(ValueError, "缺少实际桌面前端构建前置命令"):
            self.stage.validate_executed_command(self.root, command, binding, candidate, "native")
        command["preparation"] = {"binding": self.registry_binding(),
                                  "argv": self.registry_binding()["argv"]}
        with self.assertRaisesRegex(ValueError, "实际执行命令与登记不一致"):
            self.stage.validate_executed_command(self.root, command, binding, candidate, "native")

    def test_failed_frontend_preparation_never_starts_native_test(self):
        path = self.root / REGISTRY
        original = path.read_bytes()
        altered = json.loads(original)
        altered["new_persistence_points"][0]["migrations"].pop()
        self.stage.save(path, altered)
        evidence = self.root / "artifacts" / "failed-frontend-preparation"
        evidence.mkdir(parents=True)
        candidate = self.stage.candidate_snapshot(self.root, evidence)
        binding = next(binding for binding in self.inputs["executionBindings"].values()
                       if binding["kind"] == "rust_desktop_test")
        with patch.object(self.stage, "desktop_frontend_binding", return_value=self.registry_binding()):
            actual = self.stage.execute_binding(self.root, evidence, binding, 1, 30, candidate)
        self.assertFalse(actual["localPassed"], actual)
        self.assertFalse(actual["preparation"]["localPassed"], actual)
        self.assertIsInstance(actual["preparation"]["pid"], int)
        self.assertNotIn("pid", actual, "前置命令失败后不得启动依赖旧产物的桌面检查")
        self.assertIn("前端构建前置命令失败", actual["executionError"])
        self.observations.append({"label": "failed-preparation-zero-native-dispatch", "record": actual})
        path.write_bytes(original)
        self.assertTrue(self.execute_registry("preparation-registry-restored")["localPassed"])

    def test_old_candidate_is_rejected_before_execution_and_restoration_passes(self):
        first = self.execute_registry("candidate-positive")
        self.assertTrue(first["localPassed"], first)
        candidate = self.stage.candidate_snapshot(self.root, self.root / "artifacts")
        path = self.root / "rust-toolchain.toml"
        original = path.read_bytes()
        path.write_bytes(original + b"\n# isolated candidate mutation\n")
        negative = self.execute_registry("old-candidate", candidate)
        self.assertFalse(negative["localPassed"], negative)
        self.assertFalse(negative["sourceStable"], negative)
        self.assertNotIn("pid", negative, "旧候选应在启动命令之前被拒绝")
        self.assertIn("源码已改变", negative["executionError"])
        path.write_bytes(original)
        restored = self.execute_registry("candidate-restored-positive", candidate)
        self.assertTrue(restored["localPassed"], restored)

    def test_existing_evidence_is_rejected_and_fresh_evidence_passes(self):
        evidence = self.assert_positive("first-real-input-check")
        result, _ = self.cli_check("reuse-old-result", evidence)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("拒绝复用旧结果", result.stdout)
        self.assert_positive("fresh-restored-positive")

    def test_tampered_binding_log_is_rejected_and_restored_evidence_is_consumed(self):
        # 先真实执行原登记检查，由生产入口生成收据，不手填成功结果。
        key = next(key for key, binding in self.inputs["executionBindings"].items()
                   if binding.get("kind") == "script" and str(REGISTRY_SCRIPT) in binding["argv"])
        bindings = self.root / "artifacts" / "R06_BINDINGS"
        evidence = bindings / key
        argv = [sys.executable, "-B", str(self.root / SCRIPT), "--root", str(self.root),
                "--binding", key, "--evidence", str(evidence)]
        executed = subprocess.run(argv, cwd=self.root, capture_output=True, text=True, timeout=30)
        self.observations.append({"label": "actual-registry-binding", "argv": argv,
                                  "exitCode": executed.returncode, "stdout": executed.stdout,
                                  "stderr": executed.stderr})
        self.assertEqual(executed.returncode, 0, executed.stdout + executed.stderr)
        binding = self.inputs["executionBindings"][key]
        candidate = self.stage.candidate_snapshot(self.root, evidence)
        positive = self.stage.read_executed_binding(self.root, bindings, key, binding, candidate)
        self.observations.append({"label": "actual-binding-consumed", "observed": positive})
        result, leaf_dir = self.cli_leaf_evidence("incomplete-leaf-refused", bindings)
        self.assertNotEqual(result.returncode, 0, "登记校验不能代替分支行为的逐断言证据")
        leaves = json.loads((leaf_dir / "leaf-cases.json").read_text())
        self.assertFalse(leaves["passedRegisteredAssertions"])
        # 人工回执绑定的 GUI 叶不受「单绑定已跑」影响（其义务由回执自身满足）
        self.assertTrue(all(row["actual"] is None and not row["ok"] for row in leaves["cases"]
                            if "manualEvidence" not in row))
        self.assertTrue(leaves["partialProofs"])
        for row in leaves["partialProofs"]:
            self.assertFalse(row["originalAssertionSatisfied"])
            self.assertNotIn("actual", row)
            self.assertNotIn("expect", row)
        logs = list(evidence.rglob("stdout.log"))
        self.assertEqual(len(logs), 1)
        original = logs[0].read_bytes()
        logs[0].write_bytes(original + b"\nforged evidence bytes\n")
        with self.assertRaisesRegex(ValueError, "原始日志摘要改变"):
            self.stage.read_executed_binding(self.root, bindings, key, binding, candidate)
        negative, bad_leaf_dir = self.cli_leaf_evidence("tampered-binding-log", bindings)
        self.assertNotEqual(negative.returncode, 0, negative.stdout + negative.stderr)
        bad = json.loads((bad_leaf_dir / "leaf-cases.json").read_text())
        self.assertIn("原始日志摘要改变", bad["executionErrors"][key])
        logs[0].write_bytes(original)
        restored = self.stage.read_executed_binding(self.root, bindings, key, binding, candidate)
        self.assertEqual(restored, positive)
        self.observations.append({"label": "restored-binding-consumed", "observed": restored})

    def test_snapshot_tracks_deleted_and_untracked_source_and_restoration(self):
        evidence = self.root / "artifacts" / "snapshot-evidence"
        before = self.stage.candidate_snapshot(self.root, evidence)
        path = self.root / "rust-toolchain.toml"
        original = path.read_bytes()
        path.unlink()
        deleted = self.stage.candidate_snapshot(self.root, evidence)
        self.assertNotEqual(deleted["digestSha256"], before["digestSha256"])
        entry = next(row for row in deleted["files"] if row["path"] == "rust-toolchain.toml")
        self.assertEqual(entry["kind"], "deleted")
        self.assertIsNone(entry["sha256"])
        path.write_bytes(original)
        self.assertEqual(self.stage.candidate_snapshot(self.root, evidence), before)
        added = self.root / "actual-new-source.txt"
        added.write_text("first candidate bytes", encoding="utf-8")
        first = self.stage.candidate_snapshot(self.root, evidence)
        self.assertNotEqual(first["digestSha256"], before["digestSha256"])
        added.write_text("changed candidate bytes", encoding="utf-8")
        self.assertNotEqual(self.stage.candidate_snapshot(self.root, evidence)["digestSha256"], first["digestSha256"])
        added.unlink()
        self.assertEqual(self.stage.candidate_snapshot(self.root, evidence), before)
        self.observations.append({"before": before, "deleted": deleted, "added": first})

    def test_scoped_proof_rejects_unknown_duplicate_and_empty_original_ids(self):
        known = self.inputs["originalProofInventory"]["items"][0]["id"]
        for invalid in ([], [known, known], ["NOT_AN_ORIGINAL_REQUIREMENT"], known):
            with self.assertRaisesRegex(ValueError, "原要求id清单"):
                self.stage.produce_leaf_evidence(self.root, self.root / "artifacts" / "bad-scope",
                                                self.root / "artifacts" / "bindings", self.inputs, invalid)

    def test_scoped_proof_cannot_accept_missing_execution_or_release_other_leaves(self):
        selected = next(row["id"] for row in self.inputs["originalProofInventory"]["items"]
                        if row["status"] == "READY")
        bindings = self.root / "artifacts" / "scoped-empty-bindings"
        bindings.mkdir(parents=True)
        evidence = self.root / "artifacts" / "scoped-missing-proof"
        code = self.stage.produce_leaf_evidence(self.root, evidence, bindings, self.inputs, [selected])
        self.assertEqual(code, 1)
        result = json.loads((evidence / "leaf-cases.json").read_text())
        self.assertFalse(result["passedRegisteredAssertions"])
        self.assertEqual(result["stageVerdict"], "NOT_A_STAGE_VERDICT")
        self.assertEqual(result["selectedOriginalLeaves"], [selected])
        self.assertEqual(len(result["excludedFromExecutionScope"]), 279)
        self.assertTrue(result["cases"])
        self.assertTrue(all(row["leafId"] == selected and not row["ok"]
                            and row["actual"] is None for row in result["cases"]))
        self.assertTrue(result["unboundOriginalLeaves"])
        self.observations.append({"scope": selected, "missingExecutionRejected": result})

    def test_desktop_framework_requires_one_real_passed_target_and_file(self):
        binding = {"kind": "vitest_test", "package": "lingxi-resource-desktop", "suite": "resource-desktop",
                   "sourcePath": "desktop/src/tauri-resources/__tests__/resource-desktop.test.tsx",
                   "test": "original save preserves external changes"}
        positive = {"success": True, "numPassedTests": 1, "numFailedTests": 0,
                    "testResults": [{"name": str(self.root / binding["sourcePath"]),
                                     "assertionResults": [{"fullName": binding["test"], "status": "passed"}]}]}
        self.assertTrue(self.stage.inspect_output(binding, json.dumps(positive), "")["valid"])
        for mode in ("zero", "skipped", "wrong-name", "wrong-file", "two-passes", "failed"):
            report = json.loads(json.dumps(positive))
            rows = report["testResults"][0]["assertionResults"]
            if mode == "zero":report["numPassedTests"] = 0; rows.clear()
            elif mode == "skipped":report["numPassedTests"] = 0; rows[0]["status"] = "pending"
            elif mode == "wrong-name":rows[0]["fullName"] = "unrelated test"
            elif mode == "wrong-file":report["testResults"][0]["name"] = str(self.root / "unrelated.test.tsx")
            elif mode == "two-passes":report["numPassedTests"] = 2; rows.append({"fullName": "other", "status": "passed"})
            else:report["numFailedTests"] = 1; report["success"] = False; rows[0]["status"] = "failed"
            self.assertFalse(self.stage.inspect_output(binding, json.dumps(report), "")["valid"], mode)
        self.assertFalse(self.stage.inspect_output(binding, "PASS", "")["valid"])

    def test_desktop_bindings_cannot_be_applied_to_unrelated_original_requirements(self):
        path = self.root / self.stage.ORIGINAL_PROOF_PATH
        original = path.read_bytes();inventory = json.loads(original)
        row = next(row for row in inventory["items"] if row["id"] not in self.stage.DESKTOP_RESOURCE_LEAVES and row["groups"])
        row["groups"][0]["tests"][0].update(package=self.stage.DESKTOP_PACKAGE, kind="rust_desktop_test")
        self.stage.save(path, inventory)
        with self.assertRaisesRegex(ValueError, "检查包不在原范围"):
            self.stage.build(self.root)
        path.write_bytes(original)
        self.assert_positive("desktop-scope-restored-positive")


if __name__ == "__main__":
    unittest.main(testRunner=unittest.TextTestRunner(stream=sys.stdout), verbosity=2)

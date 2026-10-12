"""只核对最终两项真实凭证，复用现行核验器；不代替完整十项或阶段验收。"""
import ast
import copy
import importlib.util
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True
root = Path.cwd()
output = Path(__file__).resolve().parent
source_path = root / "scripts/rust-tauri/r06_evaluation_audit.py"
source_bytes = source_path.read_bytes()
spec = importlib.util.spec_from_file_location("current_audit", source_path)
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)
metrics = next(node for node in ast.parse(source_bytes).body
               if isinstance(node, ast.FunctionDef) and node.name == "audit_metrics")
# 原完整入口要求十项。本局部凭证核对原样执行程序绑定和每项完整分支，明确不签发完整入口通过。
program_loop = next(node for node in metrics.body if isinstance(node, ast.For)
                    and ast.unparse(node.target) == "ready")
case_loop = next(node for node in metrics.body if isinstance(node, ast.For)
                 and ast.unparse(node.target) == "(task_id, record)")
scoped_code = compile(ast.Module(body=[program_loop, case_loop], type_ignores=[]),
                      str(source_path), "exec")
base = root / "artifacts/rust-tauri/R06/residual-repair-20261012/final-validation"
cases = {"BE-02": "be02", "BE-07": "be07-final"}
report = {"inputKind": "ACTUAL_FINAL_SCOPED_RECORDS", "scope": list(cases),
          "auditSHA256": audit.sha_bytes(source_bytes), "cases": [], "budgetControls": [],
          "fullAuditExecuted": False, "originalFullBatteryReduced": False,
          "formalAcceptanceComplete": False}


def reject_budget(record, task_id, label, exact_conflict):
    try:
        audit.enforce_call_budgets({task_id: record})
    except audit.BudgetConflict as error:
        result = {"taskId": task_id, "control": label, "rejected": True,
                  "rejectionKind": "ORIGINAL_CALL_BUDGET_EXCEEDED", "detail": str(error)}
    except ValueError as error:
        audit.require(not exact_conflict, label + "应为真实原预算冲突")
        result = {"taskId": task_id, "control": label, "rejected": True,
                  "rejectionKind": "BUDGET_FIELDS_TAMPERED", "detail": str(error)}
    else:
        raise AssertionError(label + "不应获准")
    report["budgetControls"].append(result)
    return result


for task_id, directory in cases.items():
    records_path = base / directory / "raw/t08-evaluations.jsonl"
    facts_path = base / directory / "raw/t08/facts.jsonl"
    records_bytes, facts_bytes = records_path.read_bytes(), facts_path.read_bytes()
    records = [audit.parse_json(line) for line in records_bytes.decode().splitlines()]
    facts = [audit.parse_json(line) for line in facts_bytes.decode().splitlines()]
    record = audit.one(records, task_id + "真实评测记录")
    audit.require(record["taskId"] == task_id, task_id + "任务身份不符")
    launches, readies, exits = [audit.values(facts, kind) for kind in
                               ("service_launch", "service_ready", "service_exit")]
    launch, ready, stopped = [audit.one(items, task_id + label) for items, label in
                             zip((launches, readies, exits), ("启动", "READY", "物理退出"))]
    accepted = audit.one(audit.values(facts, "run_accepted"), task_id + "真实提交")
    final = audit.one(audit.values(facts, "world_final"), task_id + "最终文件")
    home = accepted["databaseHome"]
    allowed_dirs = [Path(home).resolve(), Path(final["workspace"]).resolve()]
    tag = "be02" if task_id == "BE-02" else "be07"
    audit.require(all(path.name.startswith("lingxi-r06t08-" + tag + "-") for path in allowed_dirs),
                  task_id + "必须是本次专属合成资料")
    allowed_files = {Path(launch[key]).resolve() for key in ("binaryPath", "testBinaryPath")}
    expected_target = Path("/Users/study_superior/.codex/r06-w05-r01-runtime/target/debug").resolve()
    audit.require(Path(launch["binaryPath"]).resolve() == expected_target / "lingxi-service"
                  and Path(launch["testBinaryPath"]).resolve().parent == expected_target / "deps",
                  task_id + "实际程序来源不符")

    def scoped_path(path, label):
        resolved = Path(path).resolve()
        audit.require(resolved in allowed_files or any(resolved.is_relative_to(parent) for parent in allowed_dirs),
                      label + "超出本项专属合成资料与明确程序")
        return resolved

    audit.owned_path = scoped_path
    audit.require(launch["argv"] == ready["argv"]
                  and ready["pid"] == stopped["pid"] == accepted["servicePid"],
                  task_id + "实际启动/提交/退出身份不符")
    scope = dict(audit.__dict__)
    scope.update({"facts": facts, "launches": launches, "readies": readies, "exits": exits,
                  "indexed": {task_id: record}, "used_runs": set()})
    # 正式分支同时验证完整记录、wire、实际SQL、用量、目标收据、schema、文件/引用。
    exec(scoped_code, scope, scope)
    wires, actuals, derived = [scope[key] for key in ("wires", "actuals", "derived")]
    invocations = [row for actual in actuals for row in actual["invocations"]]
    if task_id == "BE-02":
        audit.enforce_call_budgets({task_id: record})
        budget_result, budget_detail = "PASS", None
        report["budgetControls"].append({"taskId": task_id, "control": "实际2次零额外原预算正对照",
                                         "inputKind": "ACTUAL_FINAL_RECORD", "accepted": True})
    else:
        failure = reject_budget(record, task_id, "实际3次比原2次多1次", True)
        budget_result, budget_detail = "FAIL", failure["detail"]
        for mutation in ("hide_extra", "inflate_budget"):
            changed = copy.deepcopy(record)
            changed["extraModelCalls"] = 0
            if mutation == "inflate_budget":
                changed["modelCallsExpected"] = changed["modelCallsObserved"]
            reject_budget(changed, task_id, mutation, False)
    report["cases"].append({"taskId": task_id, "recordsPath": str(records_path), "factsPath": str(facts_path),
                            "recordsSHA256": audit.sha_bytes(records_bytes), "factsSHA256": audit.sha_bytes(facts_bytes),
                            "binarySHA256": launch["binarySha256Before"], "testBinarySHA256": launch["testBinarySha256"],
                            "runId": accepted["runId"], "businessAndEvidenceResult": "PASS_FOR_THIS_CASE_ONLY",
                            "physicalModelRequests": len(wires), "ledgerRows": len(derived),
                            "actualInvocationTargets": [row["target"] for row in invocations],
                            "inputTokenCoverage": record["tokens"]["inputCoverage"],
                            "outputTokenCoverage": record["tokens"]["outputCoverage"],
                            "originalCallBudget": 2, "extraModelCalls": record["extraModelCalls"],
                            "originalBudgetResult": budget_result, "budgetFailure": budget_detail})
    audit.require(records_path.read_bytes() == records_bytes and facts_path.read_bytes() == facts_bytes,
                  task_id + "核对不能改写真实凭证")

# 旧四次记录只作为历史预算负控，不混入最终二进制/当前业务凭证。
historical = root / "artifacts/rust-tauri/R06/residual-repair-20261012/evaluation-round2/be02_on_demand_catalog_declared_per_turn_not_in_prompt/raw/t08-evaluations.jsonl"
historical_bytes = historical.read_bytes()
old = audit.one([audit.parse_json(line) for line in historical_bytes.decode().splitlines()], "旧BE02真实四次记录")
audit.require(old["taskId"] == "BE-02" and old["modelCallsExpected"] == 2
              and old["modelCallsObserved"] == 4 and old["extraModelCalls"] == 2,
              "旧真实四次预算记录身份不符")
rejected = reject_budget(old, "BE-02", "旧真实4次比原2次多2次仍须拒绝", True)
rejected.update({"inputKind": "ACTUAL_HISTORICAL_BUDGET_RECORD_ONLY", "recordPath": str(historical),
                 "recordSHA256": audit.sha_bytes(historical_bytes)})
for mutation in ("hide_extra", "inflate_budget"):
    changed = copy.deepcopy(old)
    changed["extraModelCalls"] = 0
    if mutation == "inflate_budget":
        changed["modelCallsExpected"] = changed["modelCallsObserved"]
    reject_budget(changed, "BE-02", "历史预算" + mutation, False)
audit.require(historical.read_bytes() == historical_bytes and source_path.read_bytes() == source_bytes,
              "核对期间原记录或核验器发生变化")
report["scopeResult"] = "ACTUAL_BE02_BE07_EVIDENCE_CONSISTENT_BE02_BUDGET_PASS_BE07_BUDGET_FAIL"
report["budgetControlResult"] = "PASS"
destination = output / "final-actual-evidence-check.json"
destination.write_text(json.dumps(report, ensure_ascii=False, indent=2))
print(json.dumps({"scopeResult": report["scopeResult"], "budgetControlResult": "PASS",
                  "budgetControls": len(report["budgetControls"]), "fullAuditExecuted": False,
                  "formalAcceptanceComplete": False, "evidence": str(destination)}, ensure_ascii=False))

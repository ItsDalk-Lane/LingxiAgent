"""Persist actual selected-original scope, not a stage verdict."""
import json
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
execution = HERE / "registered-ready-session-lifecycle-execution"
rollup = json.loads((execution / "R06_LEAVES/leaf-cases.json").read_text())
readback = json.loads((HERE / "registered-ready-session-lifecycle-readback/leaf-cases.json").read_text())
freshness = json.loads((HERE / "stage-input-session-lifecycle-final-check/inputs.json").read_text())
assert freshness["status"] == "INPUTS_MATCH_NOT_STAGE_VERDICT"
assert rollup["sourceStable"] is True and rollup["passedRegisteredAssertions"] is True
assert readback["sourceStable"] is True and readback["passedRegisteredAssertions"] is True
assert rollup["stageVerdict"] == "NOT_A_STAGE_VERDICT"
assert rollup["executionScope"] == "SELECTED_ORIGINAL_REQUIREMENTS"
assert not rollup["executionErrors"] and not rollup["partialProofs"]
for key in ["sourceCandidateSha256", "cases", "selectedOriginalLeaves", "executionErrors"]:
    assert rollup[key] == readback[key], key
assert all(case["ok"] is True and not case["missingOrInvalidCommandRefs"] for case in rollup["cases"])
ids = json.loads((HERE / "registered-ready-session-lifecycle-ids.json").read_text())
assert set(ids) == set(rollup["selectedOriginalLeaves"])
keys = {item["key"] for case in rollup["cases"] for item in case["executions"]}
for key in keys:
    result = json.loads((execution / "R06_BINDINGS" / key / "binding-result.json").read_text())
    assert result["passed"] is True and result["sourceCandidateSha256"] == rollup["sourceCandidateSha256"]
inventory_path = "scripts/rust-tauri/r06_original_leaf_bindings.json"
inventory = json.loads((ROOT / inventory_path).read_text())
original = json.loads(subprocess.check_output(["git", "show", "HEAD:" + inventory_path], cwd=ROOT))
by_id = {item["id"]: item for item in original["items"]}
assert len(inventory["items"]) == 279 == len(by_id)
assert all(all(item[key] == by_id[item["id"]][key] for key in ["then", "assertions"]) for item in inventory["items"])
ready = sum(item["status"] == "READY" for item in inventory["items"])
gap = len(inventory["items"]) - ready
assert ready == len(ids) and gap == len(rollup["unboundOriginalLeaves"])
receipt = json.loads((HERE / "CONTINUATION_RECEIPT_WORKSPACE_SKILLS_LOCAL.json").read_text())
receipt.update({
    "status": "FORMAL_PIN_AND_ACTIVE_MEMORY_LIFECYCLE_VERIFIED_R06_NOT_ACCEPTED",
    "supersedesLocalSummary": "CONTINUATION_RECEIPT_WORKSPACE_SKILLS_LOCAL.json",
    "gitHead": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
    "branch": subprocess.check_output(["git", "branch", "--show-current"], cwd=ROOT, text=True).strip(),
    "currentScopedExecution": {
        "directory": execution.name,
        "sourceCandidateSha256": rollup["sourceCandidateSha256"], "sourceStable": rollup["sourceStable"],
        "registeredBindingsExecuted": len(keys), "registeredBindingsPassed": len(keys),
        "selectedOriginalRequirements": len(ids),
        "selectedAssertions": len(rollup["cases"]),
        "selectedAssertionsPassed": sum(case["ok"] for case in rollup["cases"]),
        "executionErrors": rollup["executionErrors"],
        "rollup": execution.name + "/R06_LEAVES/leaf-cases.json",
        "rawReceiptReadback": "registered-ready-session-lifecycle-readback/leaf-cases.json",
        "executionScope": rollup["executionScope"], "excludedOriginalRequirements": gap,
        "additionalExcludedRequiredSupplementalLeaf": "R00-T02-LA-960668141E07",
        "excludedRegisteredGraphLeaves": len(rollup["excludedFromExecutionScope"]),
        "stageVerdict": rollup["stageVerdict"],
    },
    "originalRequirementRegistration": {
        "total": len(inventory["items"]), "ready": ready, "gap": gap,
        "registrationIsNotStageAcceptance": True,
    },
    "newlyCompletedOriginalLeaves": [
        "R00-T02-LA-0D4F0EFC9DAE", "R00-T02-LA-51905CECDD1B", "R00-T02-LA-9616596B5720",
    ],
    "newProductRepair": [
        "Formal authorized pin rejects409 agent_deleted for actual .deleted-agent.json marker; corrupted marker remains deleted. Unpin remains allowed and another agent is unaffected.",
        "Checked deletion-state IO fails500 rather than enabling pin; invalid bool400, missing auth401 and foreign403 precede any state disclosure or write.",
        "Pin-order only updates already-pinned sessions, cannot COALESCE an unpinned session into a pin; mixed list400 rolls back the transaction, closing actual same-family deleted-agent bypass.",
        "Formal owned memory GET/PATCH require live agent and active session; deleted-agent409, archived403, invalid400, foreign403, invalid auth401 and missing404 remain explicit.",
        "Memory write transaction checks archived state inside the same transaction; direct storage rejects archived session, not only route precheck.",
        "True/false save/read cycles and genuine service restart preserve chosen session memory; unrelated sessions unchanged. Real triggers, directory permissions and missing sessions-table500 preserve original state, repairs restore same reads/writes.",
    ],
    "supportingLocalRuns": [
        {"log": "session-lifecycle-coupled-final.log", "passed": 44, "failed": 0},
        {"log": "session-lifecycle-route-policy.log", "passed": 1, "failed": 0},
        {"log": "session-lifecycle-clippy-final.log", "status": "PASS", "scope": "workspace --all-targets -- -D warnings"},
        {"log": "workspace-format-session-lifecycle-final.log", "status": "PASS"},
        {"log": "check-contracts-session-lifecycle.log", "status": "PASS"},
        {"log": "check-boundaries-session-lifecycle.log", "status": "PASS"},
        {"log": "stage-input-contract-tests-session-lifecycle.log", "passed": 20, "failed": 0},
        {"log": "runner-identity-session-lifecycle.log", "passed": 1, "failed": 0},
        {"directory": "stage-input-session-lifecycle-final-check", "status": "INPUTS_MATCH_NOT_STAGE_VERDICT"},
        {"report": "proof-anchor-review-session-lifecycle-applied.json", "unchangedOriginalAnchorsRefreshed": 48, "missingAssertions": 0},
    ],
    "preservedFailures": [
        {"log": "session-pin-deleted-agent-red.log", "reason": "Deleted-agent pin incorrectly succeeded200 and changed time/order; fixed at authorized official route."},
        {"log": "session-memory-lifecycle-red.log", "reason": "Archived-session memory GET incorrectly succeeded200; active-only GET/PATCH and transaction-local state check now enforced."},
        {"log": "session-pin-order-bypass-red.log", "reason": "Pin-order wrongly created unpinned deleted-agent pins; storage updates only already-pinned rows, rolling back whole list on refusal."},
        {"log": "session-lifecycle-coupled-r2.log", "reason": "Wrong SessionView vs actual SessionFacts helper type and wrong error-match insertion; actual compiler facts corrected, no unsafe casts or wildcard errors."},
    ],
    "intentionalBehaviorDifferences": [
        "Deleted-agent pin now refuses explicitly; unpin keeps original permissive semantics and no marker JSON parse can revive the agent.",
        "Memory GET/PATCH refuse nonactive or deleted-agent session instead of success-shaped reads/writes.",
        "Pin-order reorders existing pins only, rather than silently creating new pins through COALESCE.",
    ],
})
receipt["unresolved"]["originalRequirementsWithGaps"] = gap
assert receipt["r06Accepted"] is False and receipt["r07Ready"] is False
assert receipt["userRequestedEndpointAchieved"] is False and receipt["taskMarkedComplete"] is False
output = HERE / "CONTINUATION_RECEIPT_SESSION_LIFECYCLE_LOCAL.json"
assert not output.exists()
output.write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + "\n")
print(json.dumps({"receipt": output.name, "originals": ready, "bindings": len(keys), "assertions": len(rollup["cases"]), "r07Ready": False}))

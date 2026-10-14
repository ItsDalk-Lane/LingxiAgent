"""Record only the completed, reconsumed selected-original execution window."""
import json
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
execution = HERE / "registered-ready-workspace-skills-primary-execution"
rollup = json.loads((execution / "R06_LEAVES/leaf-cases.json").read_text())
readback = json.loads((HERE / "registered-ready-workspace-skills-primary-readback/leaf-cases.json").read_text())
assert rollup["sourceStable"] is True and rollup["passedRegisteredAssertions"] is True
assert rollup["stageVerdict"] == "NOT_A_STAGE_VERDICT"
assert rollup["executionScope"] == "SELECTED_ORIGINAL_REQUIREMENTS"
assert not rollup["executionErrors"] and not rollup["partialProofs"]
freshness = json.loads((HERE / "stage-input-workspace-skills-primary-final-check/inputs.json").read_text())
assert freshness["status"] == "INPUTS_MATCH_NOT_STAGE_VERDICT"
assert readback["sourceStable"] is True and readback["passedRegisteredAssertions"] is True
for key in ["sourceCandidateSha256", "cases", "selectedOriginalLeaves", "executionErrors"]:
    assert rollup[key] == readback[key], key
assert all(case["ok"] is True and not case["missingOrInvalidCommandRefs"] for case in rollup["cases"])
ids = json.loads((HERE / "registered-ready-workspace-skills-ids.json").read_text())
assert set(rollup["selectedOriginalLeaves"]) == set(ids)
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
receipt = json.loads((HERE / "CONTINUATION_RECEIPT_INPUT_DRAFTS_LOCAL.json").read_text())
receipt.update({
    "status": "FORMAL_WORKSPACE_SKILLS_AND_CHECKED_PRIMARY_VERIFIED_R06_NOT_ACCEPTED",
    "supersedesLocalSummary": "CONTINUATION_RECEIPT_INPUT_DRAFTS_LOCAL.json",
    "gitHead": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
    "branch": subprocess.check_output(["git", "branch", "--show-current"], cwd=ROOT, text=True).strip(),
    "currentScopedExecution": {
        "directory": execution.name,
        "sourceCandidateSha256": rollup["sourceCandidateSha256"],
        "sourceStable": rollup["sourceStable"],
        "registeredBindingsExecuted": len(keys),
        "registeredBindingsPassed": len(keys),
        "selectedOriginalRequirements": len(ids),
        "selectedAssertions": len(rollup["cases"]),
        "selectedAssertionsPassed": sum(case["ok"] for case in rollup["cases"]),
        "executionErrors": rollup["executionErrors"],
        "rollup": execution.name + "/R06_LEAVES/leaf-cases.json",
        "rawReceiptReadback": "registered-ready-workspace-skills-primary-readback/leaf-cases.json",
        "executionScope": rollup["executionScope"],
        "excludedOriginalRequirements": gap,
        "additionalExcludedRequiredSupplementalLeaf": "R00-T02-LA-960668141E07",
        "excludedRegisteredGraphLeaves": len(rollup["excludedFromExecutionScope"]),
        "stageVerdict": rollup["stageVerdict"],
    },
    "originalRequirementRegistration": {
        "total": len(inventory["items"]), "ready": ready, "gap": gap,
        "registrationIsNotStageAcceptance": True,
    },
    "newlyCompletedOriginalLeaves": ["R00-T02-LA-B383AB7B553C"],
    "newProductRepair": [
        "Real LocalOnly GET /lingxi/v1/desk/skills scans four original workspace roots and returns actual metadata, paths, sourceIdentity, per-agent policy and object-shaped shadow winners.",
        "Global skills stay excluded; policy-disabled entries remain visible inactive; an empty URL-decoded approved subdirectory returns the same actual policy without writes.",
        "Invalid queries/relative or file dirs/unsupported mounts reject400, missing agents and missing roots404, invalid credentials401, devices/outside roots and symlink escapes403, actual unreadable documents/preferences and invalid stored-primary data500.",
        "Configured non-directory workspace roots fail500 rather than successful empty results. Permissions/links/root restoration permits the same scan and snapshots preserve user bytes.",
        "Implicit primary selection uses checked preferences and actual agent configuration, never silently substitutes another agent after corrupted or inaccessible preferences.",
        "A real preferences false200 RED was fixed; explicit agentId does not read unrelated preferences, and a real stored agentB chooses agentB policy rather than id-sorted agentA.",
    ],
    "supportingLocalRuns": [
        {"log": "workspace-skills-coupled-final-r5.log", "passed": 22, "failed": 0},
        {"log": "workspace-skills-primary-route-policy.log", "passed": 1, "failed": 0},
        {"log": "workspace-skills-primary-clippy-final.log", "status": "PASS", "scope": "workspace --all-targets -- -D warnings"},
        {"log": "workspace-format-workspace-skills-primary-final.log", "status": "PASS"},
        {"log": "check-contracts-workspace-skills-primary.log", "status": "PASS"},
        {"log": "check-boundaries-workspace-skills-primary.log", "status": "PASS"},
        {"log": "stage-input-contract-tests-workspace-skills-primary.log", "passed": 20, "failed": 0},
        {"log": "runner-identity-workspace-skills-primary.log", "passed": 1, "failed": 0},
        {"directory": "stage-input-workspace-skills-primary-final-check", "status": "INPUTS_MATCH_NOT_STAGE_VERDICT"},
        {"report": "proof-anchor-review-workspace-skills-primary-applied.json", "unchangedOriginalAnchorsRefreshed": 54, "missingAssertions": 0},
    ],
    "preservedFailures": [
        {"log": "workspace-skills-http-r1.log", "reason": "New tests initially used wrong helper/flag type; fixed using actual repository helpers/types."},
        {"log": "workspace-skills-coupled-final.log", "reason": "Wrong regression suite name; rerun with actual r06_t06_skill_context."},
        {"log": "workspace-skills-primary-resolution-red.log", "reason": "Actual malformed preferences incorrectly produced success200; core checked primary resolution now fails explicitly."},
        {"directory": "registered-ready-workspace-skills-execution", "reason": "Stopped incomplete pre-primary-repair window, not accepted."},
        {"log": "workspace-skills-coupled-final-r4.log", "reason": "New missing-root assertion incorrectly expected500 instead of repository-standard404; corrected, no error handling weakened."},
    ],
    "intentionalBehaviorDifferences": [
        "Unreadable workspace roots/documents and invalid/unreadable implicit-primary preferences fail explicitly rather than silently returning empty or substituting another agent.",
        "Unapproved dirs, invalid/unknown/duplicate queries and unsupported mount IDs visibly reject instead of success-shaped empty scans.",
    ],
})
receipt["unresolved"]["originalRequirementsWithGaps"] = gap
assert receipt["r06Accepted"] is False and receipt["r07Ready"] is False
assert receipt["userRequestedEndpointAchieved"] is False and receipt["taskMarkedComplete"] is False
output = HERE / "CONTINUATION_RECEIPT_WORKSPACE_SKILLS_LOCAL.json"
assert not output.exists(), "Do not overwrite a completed historical receipt"
output.write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + "\n")
print(json.dumps({"receipt": output.name, "originals": ready, "bindings": len(keys), "assertions": len(rollup["cases"]), "r07Ready": False}))

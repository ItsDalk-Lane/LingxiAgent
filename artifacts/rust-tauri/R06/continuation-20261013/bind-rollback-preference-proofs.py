"""Bind the actual persistent preference API without changing original clauses."""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("anchors", HERE / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())
row = next(row for row in inventory["items"] if row["id"] == "R00-T02-LA-015A65726C05")
original = {key: row[key] for key in ["then", "assertions", "gaps"]}
suite = "r06_original_session_proofs"
tests = [
    "original_workspace_rollback_preference_persists_without_restoring_files_and_controls_real_gate",
    "original_workspace_rollback_preference_refusals_keep_saved_and_live_state_and_retry",
]
assert len(row["assertions"]) == 2
assert row["status"] == "GAP" and row["groups"] == []
meanings = [
    "Real GET/PUT persist strict boolean rollback_file_changes while preserving unrelated preferences. "
    "Writes do not alter checkpointed file bytes/mtime, branch state or snapshot rows. A real service "
    "restart loads the saved value; authorized chat scope and concurrent setters retain disk/live "
    "consistency. The HTTP switch directly controls the production rewind preview preference gate.",
    "Missing or invalid enabled and malformed bodies reject 400 without saved/live changes; no "
    "credential rejects 401 and a real device missing chat scope rejects 403. Actual filesystem read "
    "failure or corrupt saved JSON/type rejects 500 without claiming success or mutating the runtime "
    "preference. Repairing the backing file permits retry. Corrupt startup storage fails explicitly, "
    "not a successful bootstrap with a default preference.",
]
row["groups"] = [{
    "index": index, "meaning": meaning, "coverage": "FULL", "missingClauses": [],
    "tests": [{"package": "lingxi-service", "suite": suite, "test": test}],
    "proofLocations": [anchors.test_location(suite, test)],
} for index, (meaning, test) in enumerate(zip(meanings, tests))]
report = HERE / "rollback-preference-proof-review.json"
row.update(status="READY", gaps=[], reviewSource=report.relative_to(anchors.ROOT).as_posix())
assert row["then"] == original["then"] and row["assertions"] == original["assertions"]
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
report.write_text(json.dumps({
    "status": "EXECUTED_TESTS_BOUND_NOT_STAGE_ACCEPTANCE",
    "id": row["id"], "original": original, "addedTests": tests,
    "executionLog": "workspace-rollback-coupled-regression-r3.log",
    "relatedRegression": {"originalSessions": 11, "sessionTree": 13, "fileRestore": 16},
    "remainingStageRequirementsUnaffected": True,
}, ensure_ascii=False, indent=2) + "\n")
ids = HERE / "reviewed-original-ids.json"
selected = json.loads(ids.read_text())
assert row["id"] not in selected
selected.append(row["id"])
ids.write_text(json.dumps(selected, indent=2) + "\n")
print("Bound persistent rollback preference leaf; no R06 or R07 acceptance.")

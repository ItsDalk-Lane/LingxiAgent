"""Bind only the reviewed, executed missing knowledge management clauses."""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("anchors", HERE / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())
row = next(row for row in inventory["items"] if row["id"] == "R00-T02-LA-1AA3F1311F8A")
original = {"then": row["then"], "assertions": row["assertions"], "gaps": row["gaps"]}
suite = "r06_original_knowledge_proofs"
tests = [
    "original_manage_interactive_unapproved_and_cancelled_leave_no_sources_or_jobs",
    "original_manage_add_preserves_source_and_reports_background_index_failure_separately",
]
group = next(group for group in row["groups"] if group["index"] == 3)
for test in tests:
    reference = {"package": "lingxi-service", "suite": suite, "test": test}
    if reference not in group["tests"]:
        group["tests"].append(reference)
        group["proofLocations"].append(anchors.test_location(suite, test))
group.update(
    coverage="FULL", missingClauses=[],
    meaning=(
        "Actual model-directed knowledge_manage invocation in an ask session parks in durable "
        "waiting_approval with zero sources/jobs before approval and after cancellation; the same "
        "arguments in an operate session persist one source/job. Existing real calls prove input/path "
        "and foreign-studio refusals preserve facts. A configured embedding transport returning HTTP "
        "502 after a real authorized management add yields a separate failed ingestion job with its "
        "explicit reason and failed=1/done=0 notebook status, while original bytes remain stored."
    ),
)
assert len(row["groups"]) == len(row["assertions"]) == 4
assert all(group["coverage"] == "FULL" and not group["missingClauses"] for group in row["groups"])
report = HERE / "knowledge-management-proof-review.json"
row.update(status="READY", gaps=[], reviewSource=report.relative_to(anchors.ROOT).as_posix())
assert row["then"] == original["then"] and row["assertions"] == original["assertions"]
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
report.write_text(json.dumps({
    "status": "EXECUTED_TESTS_BOUND_NOT_STAGE_ACCEPTANCE",
    "id": row["id"], "original": original, "addedTests": tests,
    "executionLog": "knowledge-management-completion-r2.log",
    "remainingStageRequirementsUnaffected": True,
}, ensure_ascii=False, indent=2) + "\n")
ids = HERE / "reviewed-original-ids.json"
selected = json.loads(ids.read_text())
if row["id"] not in selected:
    selected.append(row["id"])
ids.write_text(json.dumps(selected, indent=2) + "\n")
print("Bound one complete knowledge management leaf; no R06 or R07 verdict.")

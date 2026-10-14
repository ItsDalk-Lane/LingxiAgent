"""Bind the actual archived-list projection and refusal proofs, not a stage verdict."""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("anchors", HERE / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())
identifier = "R00-T02-LA-43FFB75F252C"
row = next(row for row in inventory["items"] if row["id"] == identifier)
assert row["status"] == "GAP" and len(row["groups"]) == len(row["assertions"]) == 2
original = {key: row[key] for key in ["then", "assertions", "gaps"]}
tests = [
    "original_archived_list_aggregates_agents_and_projects_first_visible_user_text",
    "original_archived_list_refusals_and_corrupt_history_do_not_return_empty_success",
]
meanings = [
    "Actual HTTP creates and archives five sessions across three agents, excluding an active session. "
    "The real execute pipeline persists first and later user inputs and assistant responses; preview selects "
    "the first visible user input, not the latest or assistant, and returns firstMessage. Chinese and emoji "
    "previews meet the original100 UTF16-unit limit, preserving whole Unicode scalars at an astral boundary. "
    "Reference/reminder/knowledge/research envelopes (including an unterminated block) are stripped without "
    "rewriting stored history; a truly empty session returns an empty string.",
    "Actual HTTP root aggregates authorized agents while a device sees only its owner-scoped archived row. "
    "Missing/invalid credentials reject401 and a real settings-only credential rejects403 with no sessions "
    "success payload. A renamed real history table, malformed stored JSON and text:null reject500; restoring "
    "storage returns the identical list. Metadata-only reads used by cleanup retain their prior independent "
    "behavior when the history table is unavailable; no lifecycle or history mutation is introduced by the read.",
]
for group in row["groups"]:
    assert group["coverage"] == "PARTIAL"
    test = tests[group["index"]]
    group["tests"].append({"package": "lingxi-service", "suite": "r06_original_session_proofs", "test": test})
    group["proofLocations"].append(anchors.test_location("r06_original_session_proofs", test))
    group.update(coverage="FULL", missingClauses=[], meaning=meanings[group["index"]])
report = HERE / "archived-projection-proof-review.json"
row.update(status="READY", gaps=[], reviewSource=report.relative_to(anchors.ROOT).as_posix())
assert row["then"] == original["then"] and row["assertions"] == original["assertions"]
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
report.write_text(json.dumps({
    "status": "EXECUTED_ORIGINAL_PROOFS_BOUND_NOT_STAGE_ACCEPTANCE",
    "id": identifier, "original": original,
    "logs": ["archived-projection-coupled-final.log", "archived-adapter-regression.log", "workspace-clippy-archived-final.log"],
    "coupledServiceTestsPassed": 42, "adapterTestsPassed": 18,
    "preservedFailures": ["archived-projection-red.log", "archived-projection-green-r1.log", "archived-projection-green-r2.log"],
    "storageShape": "Actual T03 user-message writer stores {text}, not an assistant NormalizedMessage or bare content-block array.",
    "unicodeBoundary": "At most100 UTF16 units; a scalar crossing unit100 is omitted whole, rather than emitting invalid surrogate/UTF8.",
    "stageAcceptance": False,
}, ensure_ascii=False, indent=2) + "\n")
ids = HERE / "reviewed-original-ids.json"
selected = json.loads(ids.read_text())
assert identifier not in selected
selected.append(identifier)
ids.write_text(json.dumps(selected, indent=2) + "\n")
print("Bound complete actual archived-list original leaf; no R06 or R07 verdict.")

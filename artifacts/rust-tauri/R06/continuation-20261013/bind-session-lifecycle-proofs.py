"""Bind actual pin/deleted-agent and active-memory requirements without stage claims."""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("anchors", HERE / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())
suite = "r06_original_session_proofs"
tests_by_id = {
    "R00-T02-LA-0D4F0EFC9DAE": [
        "original_pin_deleted_agent_refuses_without_writes_and_allows_unpin",
        "original_pin_order_checks_each_owner_and_rolls_back_failed_reordering",
    ],
    "R00-T02-LA-51905CECDD1B": [
        "original_session_memory_only_active_live_agents_persist_and_refuse_explicitly",
    ],
    "R00-T02-LA-9616596B5720": [
        "original_session_memory_only_active_live_agents_persist_and_refuse_explicitly",
    ],
}
meanings = {
    "R00-T02-LA-0D4F0EFC9DAE": (
        "Actual HTTP boolean pin stores time/order, unpin clears both. Existing .deleted-agent.json "
        "rejects pin409 agent_deleted before writes, even if marker JSON is corrupt; unpin is allowed. "
        "Another agent can still pin. Invalid/missing/null booleans400, missing auth401, foreign403 "
        "without deleted-agent disclosure; refused operations preserve full session rows and marker bytes. "
        "Actual inaccessible agent directory500, pin UPDATE trigger500, restored directory/trigger "
        "permits live pin. Actual pin-order mixed pinned/unpinned input400 rolls back all, cannot create "
        "a deleted-agent pin. Existing duplicate400/missing404/mixed-owner403/second-write500 rollback "
        "and successful reorder proofs remain. This does not claim complete agent deletion UI."
    ),
    "memory": (
        "Actual owned active session memory GET returns the same saved boolean as PATCH; true/false "
        "cycles leave unrelated sessions unchanged. Real archive makes GET/PATCH403 with no writes, "
        "and direct storage write also rejects in its transaction, not only a route precheck. "
        "Restore reopens the same session; actual deleted-agent marker makes GET/PATCH409 agent_deleted "
        "and leaves marker/session unchanged. Foreign403, invalid auth401, invalid/null/missing/extra "
        "body400, missing session404. Actual write-trigger500, unreadable agent-directory500 and missing "
        "sessions-table500 do not return successful memoryEnabled, preserve saved state. Repairs restore "
        "read, genuine service restart reads saved false, then saves/reads true. No UI/ticker claim."
    ),
}
review = []
report = HERE / "session-lifecycle-proof-review.json"
for identifier, tests in tests_by_id.items():
    row = next(row for row in inventory["items"] if row["id"] == identifier)
    assert row["status"] == "GAP" and len(row["assertions"]) == 2
    original = {key: row[key] for key in ["then", "assertions", "groups", "gaps"]}
    row["groups"] = [{
        "index": index,
        "tests": [{"package": "lingxi-service", "suite": suite, "test": test} for test in tests],
        "proofLocations": [anchors.test_location(suite, test) for test in tests],
        "coverage": "FULL", "missingClauses": [],
        "meaning": meanings.get(identifier, meanings["memory"]),
    } for index in range(2)]
    row.update(status="READY", gaps=[], reviewSource=report.relative_to(anchors.ROOT).as_posix())
    assert all(row[key] == original[key] for key in ["then", "assertions"])
    review.append({"id": identifier, "original": original})
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
report.write_text(json.dumps({
    "status": "ACTUAL_SESSION_LIFECYCLE_BOUND_NOT_STAGE_ACCEPTANCE",
    "review": review,
    "redLogs": ["session-pin-deleted-agent-red.log", "session-memory-lifecycle-red.log", "session-pin-order-bypass-red.log"],
    "coupledLog": "session-lifecycle-coupled-final.log",
    "stageAcceptance": False,
}, ensure_ascii=False, indent=2) + "\n")
ids = HERE / "reviewed-original-ids.json"
selected = json.loads(ids.read_text())
assert not set(tests_by_id).intersection(selected)
selected.extend(tests_by_id)
ids.write_text(json.dumps(selected, indent=2) + "\n")
ready = [row["id"] for row in inventory["items"] if row["status"] == "READY"]
(HERE / "registered-ready-session-lifecycle-ids.json").write_text(json.dumps(ready, indent=2) + "\n")
print(f"{len(ready)} READY originals registered; full R06/R07 not accepted.")

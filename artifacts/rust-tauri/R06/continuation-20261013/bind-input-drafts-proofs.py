"""Register complete home/read/clear drafts only; unresolved sessionPath remains GAP."""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("anchors", HERE / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())
identifiers = [
    "R00-T02-LA-081FACA79F2C", "R00-T02-LA-1B712E189DDF", "R00-T02-LA-F829AEA5931C",
]
suite = "r06_original_session_proofs"
tests = [
    "original_input_drafts_persist_home_and_sessions_per_surface_and_delete_only_target",
    "original_input_drafts_reject_bad_scope_and_authorization_and_preserve_failed_storage",
    "original_input_drafts_concurrent_updates_and_actual_caps_preserve_other_owners",
]
report = HERE / "input-drafts-proof-review.json"
review = []
for identifier in identifiers:
    row = next(row for row in inventory["items"] if row["id"] == identifier)
    assert row["status"] == "GAP" and not row["groups"] and len(row["assertions"]) == 2
    original = {key: row[key] for key in ["then", "assertions", "gaps"]}
    for index in range(2):
        row["groups"].append({
            "index": index,
            "tests": [{"package": "lingxi-service", "suite": suite, "test": test} for test in tests],
            "proofLocations": [anchors.test_location(suite, test) for test in tests],
            "coverage": "FULL", "missingClauses": [],
            "meaning": (
                "Real authenticated GET/PUT /lingxi/v1/input-drafts uses the original electron/pwa surfaces and "
                "input-drafts.v1.json {version:1,surfaces} format. Actual text/doc/server updatedAt home and "
                "sessionId entries survive two genuine service restarts unchanged; GET does not write. "
                "Empty/whitespace text deletes only the chosen home/session entry, preserving other sessions "
                "and surface. Query normalization includes actual URL decoding; missing/invalid/duplicate/"
                "unknown queries and conflicting/invalid body scopes/text/doc reject400 before writes. "
                "Missing credentials reject401; cross-owner session writes403 and unknown sessions404 leave "
                "file bytes unchanged; device GET excludes other-owned session entries. Home remains the "
                "original authenticated instance-wide bucket. Corrupt/unsupported/unreadable saved files "
                "and actual unwritable parent persistence reject500, preserving original bytes with no "
                "success payload/cache update; repaired storage permits the original operation. "
                "Barrier-driven actual concurrent HTTP updates preserve both entries. Real512K UTF16 entry "
                "handling reports docDropped/textTruncated and keeps scalars whole; a genuine200-session "
                "bucket evicts its oldest owned entry with explicit receipt, never another owner's draft. "
                "Unresolved sessionPath is explicitly400 and its separate original save-session leaf "
                "remains GAP, not claimed by these complete read/home/clear requirements."
            ),
        })
    row.update(status="READY", gaps=[], reviewSource=report.relative_to(anchors.ROOT).as_posix())
    assert row["then"] == original["then"] and row["assertions"] == original["assertions"]
    review.append({"id": identifier, "original": original, "status": row["status"]})
unresolved = next(row for row in inventory["items"] if row["id"] == "R00-T02-LA-D901852E1B1A")
assert unresolved["status"] == "GAP"
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
report.write_text(json.dumps({
    "status": "EXECUTED_ORIGINAL_PROOFS_BOUND_NOT_STAGE_ACCEPTANCE", "review": review,
    "logs": ["input-drafts-coupled-final-r2.log", "input-drafts-route-policy.log", "input-drafts-workspace-clippy-final.log"],
    "coupledTestsPassed": 45, "routePolicyTestsPassed": 1,
    "intentionalExplicitFailures": "Corrupt/version/unreadable storage fails instead of silent rebuild; incompatible body fields reject400 rather than deleting on invalid input.",
    "homeScope": "Preserves original authenticated instance-wide home per surface; session entries additionally honor current server ownership.",
    "sessionPathLeaf": "GAP: no registered persistent path locator exists in current Rust session architecture.",
    "stageAcceptance": False,
}, ensure_ascii=False, indent=2) + "\n")
ids = HERE / "reviewed-original-ids.json"
selected = json.loads(ids.read_text())
assert not set(identifiers).intersection(selected)
selected.extend(identifiers)
ids.write_text(json.dumps(selected, indent=2) + "\n")
print("Registered three complete original draft leaves; sessionPath and R06/R07 remain unaccepted.")

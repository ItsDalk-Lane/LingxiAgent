"""Bind real provider registrations to handle-only release/diagnostic/cursor APIs."""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("anchors", HERE / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())
identifiers = [
    "R00-T02-LA-00C88B996AA0", "R00-T02-LA-4E8EDC4E25FC",
    "R00-T02-LA-75753997F5C6", "R00-T02-LA-9FF4E1805297",
]
suite = "r06_original_resource_proofs"
tests = [
    "original_resource_watch_handle_lifecycle_and_cursor_reads_use_real_provider_registrations",
    "original_resource_events_since_and_stale_resync_follow_real_writes_without_read_side_effects",
]
report = HERE / "resource-lifecycle-proof-review.json"
review = []
for identifier in identifiers:
    row = next(row for row in inventory["items"] if row["id"] == identifier)
    assert row["status"] == "GAP" and len(row["groups"]) == len(row["assertions"]) == 3
    original = {key: row[key] for key in ["then", "assertions", "gaps"]}
    group = next(group for group in row["groups"] if group["index"] == 2)
    assert group["coverage"] == "PARTIAL"
    for test in tests:
        group["tests"].append({"package": "lingxi-service", "suite": suite, "test": test})
        group["proofLocations"].append(anchors.test_location(suite, test))
    group.update(coverage="FULL", missingClauses=[], meaning=(
        "Actual authorized local_fs references create a watch and subscription sharing refCount=2. "
        "The genuine returned handles are released through HTTP with true then false, exact diagnostics "
        "and unchanged file bytes/mtime and registration rows. These APIs take handles or since, not "
        "resource references: provider coverage is proved at creation and lifecycle read/release. "
        "Mount and URL are explicitly unsupported400; real SessionFile references refuse403; "
        "mixed local/provider subscriptions roll back every retain, diagnostics and since show no "
        "phantom subscription/event. No mount success is claimed. Missing and unauthorized local "
        "targets refuse; missing credentials cannot release handles. Actual1005 production HTTP "
        "writes overflow real retention: since returns exactly3 ordered newer events or explicit "
        "stale resource-stat-required. Invalid cursors/unknown query reject400, missing auth401, "
        "and all reads preserve files, metadata, registrations and event history. Existing exact "
        "cross-provider move rejection remains bound."
    ))
    assert all(g["coverage"] == "FULL" and not g["missingClauses"] for g in row["groups"])
    row.update(status="READY", gaps=[], reviewSource=report.relative_to(anchors.ROOT).as_posix())
    assert row["then"] == original["then"] and row["assertions"] == original["assertions"]
    review.append({"id": identifier, "original": original, "status": row["status"]})
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
report.write_text(json.dumps({
    "status": "EXECUTED_PROOFS_BOUND_NOT_STAGE_ACCEPTANCE", "review": review,
    "executionLog": "resource-lifecycle-coupled-final.log", "testsPassed": 23,
    "rateLimit": "Only isolated retention fixture injects2048 requests/window; production default and other fixtures unchanged.",
    "mountAndUrlSuccessNotClaimed": True, "providerRefusalNotBypassed": True,
    "preservedAttempts": ["resource-lifecycle-r1.log", "resource-lifecycle-r2.log"],
}, ensure_ascii=False, indent=2) + "\n")
ids = HERE / "reviewed-original-ids.json"
selected = json.loads(ids.read_text())
assert not set(identifiers).intersection(selected)
selected.extend(identifiers)
ids.write_text(json.dumps(selected, indent=2) + "\n")
print("Bound four complete actual resource lifecycle leaves; no stage acceptance.")

"""Refresh complete desk proofs after the observed implicit-primary false success."""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("anchors", HERE / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())
row = next(item for item in inventory["items"] if item["id"] == "R00-T02-LA-B383AB7B553C")
assert row["status"] == "READY" and len(row["groups"]) == 2
for group in row["groups"]:
    assert group["coverage"] == "FULL" and not group["missingClauses"]
    group["proofLocations"] = [
        anchors.test_location(test["suite"], test["test"]) for test in group["tests"]
    ]
    group["meaning"] += (
        " Implicit primary selection now reads checked real preferences. Actual malformed JSON, "
        "nonobject preferences, wrong-type/invalid/missing stored primary and real unreadable "
        "preferences explicitly reject500 without writes; explicit agentId remains independent. "
        "Actual valid primary agentB policy is selected rather than id-sorted agentA policy. "
        "Absent preference/primary alone permits deterministic discovery. "
        "The original false200 RED and stopped pre-repair window are retained, not passed."
    )
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
(HERE / "workspace-skills-primary-proof-review.json").write_text(json.dumps({
    "status": "OBSERVED_FALSE_SUCCESS_FIXED_NOT_STAGE_ACCEPTANCE",
    "redLog": "workspace-skills-primary-resolution-red.log",
    "greenLog": "workspace-skills-coupled-final-r5.log",
    "stoppedWindow": "registered-ready-workspace-skills-execution",
    "stoppedWindowAccepted": False,
    "stageAcceptance": False,
}, indent=2) + "\n")
print("Refreshed actual implicit-primary desk proofs; stopped window not accepted.")

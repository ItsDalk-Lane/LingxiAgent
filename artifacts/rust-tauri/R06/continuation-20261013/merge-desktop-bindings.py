"""Merge only executed desktop proofs and the verified partial studio repair."""
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
MAP = ROOT / "scripts/rust-tauri/r06_original_leaf_bindings.json"
inventory = json.loads(MAP.read_text())
fragment = json.loads((HERE / "desktop-full-bindings.json").read_text())
expected = {
    "R00-T02-LA-45627E4F8173", "R00-T02-LA-ADB6B0580864",
    "R00-T02-LA-CC738012B6EE", "R00-T02-LA-D6509017C382",
}
assert {item["id"] for item in fragment["items"]} == expected
rows = {row["id"]: row for row in inventory["items"]}
reviewed = []
for item in fragment["items"]:
    row = rows[item["id"]]
    assert row["then"] == item["then"]
    assert row["assertions"] == [
        assertion["originalAssertion"] for assertion in item["assertions"]
    ]
    assert {group["index"] for group in item["groups"]} == set(range(len(row["assertions"])))
    for group in item["groups"]:
        assert group["coverage"] == "FULL" and group["tests"] and group["meaning"]
        for anchor in group["proofLocations"]:
            assert hashlib.sha256((ROOT / anchor["path"]).read_bytes()).hexdigest() == anchor["sourceSHA256"]
        group["missingClauses"] = []
    reviewed.append({"id": row["id"], "previousGaps": row["gaps"]})
    row.update(
        groups=item["groups"], status="READY", gaps=[],
        reviewSource="artifacts/rust-tauri/R06/continuation-20261013/desktop-full-bindings.json",
    )

row = rows["R00-T02-LA-1AA3F1311F8A"]
remaining = (
    "尚缺interactive审批未批准时的专属检查和真实后台索引失败经管理工具返回后独立状态检查。"
)
for group in row["groups"]:
    if group["index"] == 2:
        group.update(
            coverage="FULL", missingClauses=[],
            meaning="Real model tool calls reject foreign-studio add/remove/refresh/reindex and malformed arguments; authenticated fact/source/job readback proves no foreign or own-library changes.",
        )
    elif group["index"] == 3:
        group["missingClauses"] = [remaining]
row["gaps"] = [{"assertionIndex": 3, "kind": "PROOF_GAP", "reason": remaining}, remaining]
assert row["status"] == "GAP"

MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
(HERE / "desktop-binding-merge-review.json").write_text(json.dumps({
    "status": "EXECUTED_PROOFS_BOUND_NOT_STAGE_ACCEPTANCE",
    "reviewed": reviewed,
    "partialKnowledgeRepair": {
        "id": row["id"], "executionLog": "knowledge-studio-regression.log",
        "remainingMissingClauses": [remaining],
    },
}, ensure_ascii=False, indent=2) + "\n")
print("Bound four native desktop leaves; knowledge management remains PARTIAL.")

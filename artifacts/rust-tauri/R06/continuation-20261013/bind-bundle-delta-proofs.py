"""Bind actual visible-member enable/disable deltas, retaining UI/broadcast gaps."""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("anchors", HERE / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())
suite = "r06_original_skills_bundle_proofs"
tests = [
    "original_bundle_enable_disable_apply_visible_delta_and_preserve_unrelated_selection",
    "original_bundle_toggle_refusals_and_backing_storage_failures_preserve_selection",
]
report = HERE / "bundle-delta-proof-review.json"
review = []
identifiers = ["R00-T02-LA-52055FF0CAC2", "R00-T02-LA-A7B51C282C77"]
for identifier in identifiers:
    row = next(row for row in inventory["items"] if row["id"] == identifier)
    assert row["status"] == "GAP" and len(row["assertions"]) == 2
    original = {key: row[key] for key in ["then", "assertions", "groups", "gaps"]}
    meanings = [
        "Real HTTP true and false apply deduplicated bundle-order visible member deltas to the "
        "requested agent only, preserve unrelated enabled skills and workspace opt-outs, and return "
        "accurate changed and enabled. Repeated toggles, missing members, no visible members, runtime "
        "enabled flags and per-agent bundle projection are read from actual files. Concurrent "
        "bundle/single-skill and two-bundle writers use barriers and preserve both updates.",
        "Both true/false reject read-only credentials, invalid enabled/body, missing bundle and "
        "missing agent without changing either agent or the bundle file. Actual corrupt/unreadable "
        "bundle storage returns an explicit error rather than pretending no bundle exists. Actual "
        "unwritable config parent refuses persistence without successful selection; repairing it "
        "permits enable and disable retries. Missing members are excluded from changed.",
    ]
    row["groups"] = [{
        "index": index, "coverage": "FULL", "missingClauses": [],
        "meaning": meaning,
        "tests": [{"package": "lingxi-service", "suite": suite, "test": test} for test in selected],
        "proofLocations": [anchors.test_location(suite, test) for test in selected],
    } for index, (meaning, selected) in enumerate([
        (meanings[0], tests[:1]), (meanings[1], tests),
    ])]
    row.update(status="READY", gaps=[], reviewSource=report.relative_to(anchors.ROOT).as_posix())
    assert row["then"] == original["then"] and row["assertions"] == original["assertions"]
    review.append({"id": identifier, "original": original, "status": row["status"]})
panel = next(row for row in inventory["items"] if row["id"] == "R00-T02-LA-0DDB5AA05072")
panel["gaps"] = [
    gap.replace("组合停用及导出未实现", "组合停用已接入真实增量并验证，导出仍未实现")
       .replace("组合disabled还存在明确行为缺口", "组合disabled后端已修复，UI仍缺现行候选证明")
    if isinstance(gap, str) else gap for gap in panel["gaps"]
]
assert panel["status"] == "GAP"
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
report.write_text(json.dumps({
    "status": "EXECUTED_TESTS_BOUND_NOT_STAGE_ACCEPTANCE",
    "review": review, "executionLog": "bundle-toggle-coupled-final.log",
    "testsPassed": 20, "failed": 0, "uiAndBroadcastGapsRetained": True,
    "preservedRepro": "bundle-toggle-red.log",
    "diskWriteRefusalPlatformActuallyExecuted": "macOS",
}, ensure_ascii=False, indent=2) + "\n")
ids = HERE / "reviewed-original-ids.json"
selected = json.loads(ids.read_text())
assert not set(identifiers).intersection(selected)
selected.extend(identifiers)
ids.write_text(json.dumps(selected, indent=2) + "\n")
print("Bound two original bundle delta leaves; UI and broadcast gaps remain.")

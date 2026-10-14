"""Bind the actual desk scan; do not claim installation, broadcasts or stage acceptance."""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("anchors", HERE / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())
identifier = "R00-T02-LA-B383AB7B553C"
row = next(row for row in inventory["items"] if row["id"] == identifier)
assert row["status"] == "GAP" and len(row["assertions"]) == 2
original = {key: row[key] for key in ["then", "assertions", "groups", "gaps"]}
suite = "r06_original_skills_bundle_proofs"
tests = [
    "original_desk_skills_returns_actual_workspace_sources_and_agent_policy_without_writes",
    "original_desk_skills_refuses_invalid_outside_and_unreadable_targets_explicitly",
]
report = HERE / "workspace-skills-proof-review.json"
row["groups"] = [{
    "index": index,
    "tests": [{"package": "lingxi-service", "suite": suite, "test": test} for test in tests],
    "proofLocations": [anchors.test_location(suite, test) for test in tests],
    "coverage": "FULL",
    "missingClauses": [],
    "meaning": (
        "Actual LocalOnly GET /lingxi/v1/desk/skills scans all four original workspace roots "
        "and returns real skill metadata, paths, source identity and per-agent discovery policy. "
        "Global skills are excluded; policy-disabled entries remain visible inactive, and an "
        "enabled duplicate returns the actual standard winner source/sourceIdentity object. "
        "Policy changes and another agent are read from actual separate configurations. "
        "An empty URL-decoded approved subdirectory returns empty skills with the same policy. "
        "Invalid/duplicate/unknown queries, relative/file dirs and unsupported mounts reject400; "
        "missing agents404, invalid credentials401, nonowner devices and outside roots403. "
        "Actual unreadable SKILL.md rejects500; root and document symlink escapes reject403. "
        "Refusals preserve workspace/config and outside bytes; restoring permissions/links "
        "restores the same real scan. Successful reads also leave snapshots unchanged. "
        "Mount support, skill installation/deletion, UI and app broadcasts are not claimed."
    ),
} for index in range(2)]
row.update(status="READY", gaps=[], reviewSource=report.relative_to(anchors.ROOT).as_posix())
assert row["then"] == original["then"] and row["assertions"] == original["assertions"]
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
report.write_text(json.dumps({
    "status": "ACTUAL_DESK_SCAN_BOUND_NOT_STAGE_ACCEPTANCE",
    "id": identifier,
    "original": original,
    "logs": ["workspace-skills-http-r2.log", "workspace-skills-coupled-final-r2.log"],
    "coupledTestsPassed": 22,
    "stageAcceptance": False,
}, ensure_ascii=False, indent=2) + "\n")
ids = HERE / "reviewed-original-ids.json"
selected = json.loads(ids.read_text())
assert identifier not in selected
selected.append(identifier)
ids.write_text(json.dumps(selected, indent=2) + "\n")
ready = [item["id"] for item in inventory["items"] if item["status"] == "READY"]
(HERE / "registered-ready-workspace-skills-ids.json").write_text(json.dumps(ready, indent=2) + "\n")
print(f"Registered desk scan; {len(ready)} READY originals, not R06/R07 acceptance.")

"""Bind the 6 B7a server-side leaves (conversation-map layout family, card
guide tool, checkpoint restore). Registration is not stage acceptance."""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("anchors", HERE / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())

S = "lingxi-service"
SUITE = "r06_conversation_map_and_checkpoint"
LAYOUT = "conversation_map_layout_get_merge_replace_collapsed"
RESTORE = "checkpoint_restore_route"
GUIDE = "hana_card_guide_tool_through_gateway"

BATCH = {
    "R00-T02-LA-5271217DB01B": {"tests": {0: [(SUITE, LAYOUT)], 1: [(SUITE, LAYOUT)]},
        "meaning": {0: "replacePositions:true replaces the whole coordinate table (unspecified card coordinates are removed) while untouched sections are preserved.", 1: "Invalid bodies 400 invalid_body; unknown session 404; missing credential 401; the layout file is byte-identical across rejected requests."}},
    "R00-T02-LA-5370DA3742CB": {"tests": {0: [(SUITE, LAYOUT)], 1: [(SUITE, LAYOUT)]},
        "meaning": {0: "Default merge updates only the named cards (JS Math.round half-up rounding, ±100000 clamp, non-object entries dropped, 5000-entry cap — all ported from sanitizePositionsInput).", 1: "Same explicit rejection vocabulary with no persisted drift."}},
    "R00-T02-LA-94B9BE3AA7A6": {"tests": {0: [(SUITE, LAYOUT)], 1: [(SUITE, LAYOUT)]},
        "meaning": {0: "A present collapsed array replaces the whole fold list (empty clears it) without touching positions.", 1: "Same explicit rejection vocabulary with no persisted drift."}},
    "R00-T02-LA-F28086B64522": {"tests": {0: [(SUITE, LAYOUT)], 1: [(SUITE, LAYOUT)]},
        "meaning": {0: "GET returns the persisted layout or the default empty layout; the defensive default-on-read-failure behavior is the incumbent contract itself.", 1: "Unknown session 404 / unauthenticated 401 remain explicit (read failures degrade to the default layout, never to a fabricated error-free success)."}},
    "R00-T02-LA-D51B2A1FA68A": {"tests": {0: [(SUITE, GUIDE)], 1: [(SUITE, GUIDE)], 2: [(SUITE, GUIDE)]},
        "meaning": {0: "The tool returns the full incumbent guide text (byte-identical port) through the real gateway prepare/execute chain.", 1: "The response is a single text block with zero resource_refs — no card artifact is created by this call.", 2: "A missing guide resource fails loudly (port-side fail-loud hardening, unit-covered) instead of returning an empty guide."}},
    "R00-T02-LA-3562B5D44C5E": {"tests": {0: [(SUITE, RESTORE)], 1: [(SUITE, RESTORE)]},
        "meaning": {0: "POST /lingxi/v1/checkpoints/{id}/restore writes the archived bytes back to the recorded path (mkdir -p, base64 lossless) and returns the restored location; a missing checkpoint is 404.", 1: "Corrupt archives fail with an explicit 500 (no silent degradation); unauthorized requests are refused."}},
}

review = []
report = HERE / "b7a-leaves-batch4-proof-review.json"
ready_ids = []
for identifier, spec_row in BATCH.items():
    row = next(row for row in inventory["items"] if row["id"] == identifier)
    assert row["status"] == "GAP", identifier
    original = {key: row[key] for key in ["then", "assertions", "groups", "gaps"]}
    assert set(spec_row["tests"]) == set(range(len(row["assertions"]))), identifier
    groups = [{
        "index": index,
        "tests": [{"package": S, "suite": SUITE, "test": test} for _s, test in spec_row["tests"][index]],
        "proofLocations": [anchors.test_location(SUITE, test) for _s, test in spec_row["tests"][index]],
        "coverage": "FULL",
        "missingClauses": [],
        "meaning": spec_row["meaning"][index],
    } for index in sorted(spec_row["tests"])]
    row["groups"] = groups
    row.update(status="READY", gaps=[],
               reviewSource=report.relative_to(anchors.ROOT).as_posix())
    ready_ids.append(identifier)
    assert row["then"] == original["then"] and row["assertions"] == original["assertions"], identifier
    review.append({"id": identifier, "original": original, "became": row["status"]})

anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
report.write_text(json.dumps({
    "status": "B7A_LEAVES_BOUND_NOT_STAGE_ACCEPTANCE",
    "ready": ready_ids,
    "review": review,
    "stageAcceptance": False,
}, ensure_ascii=False, indent=2) + "\n")
ids = HERE / "reviewed-original-ids.json"
selected = json.loads(ids.read_text())
assert not set(ready_ids).intersection(selected)
selected.extend(ready_ids)
ids.write_text(json.dumps(selected, indent=2) + "\n")
(HERE / "registered-ready-b7a-batch4-ids.json").write_text(json.dumps(ready_ids, indent=2) + "\n")
ready_total = sum(1 for row in inventory["items"] if row["status"] == "READY")
print(f"{len(ready_ids)} READY registered this batch ({ready_total} total); full R06/R07 not accepted.")

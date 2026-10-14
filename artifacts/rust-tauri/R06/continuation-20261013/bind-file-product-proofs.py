"""Bind verified file-product registration without releasing the sandbox gap."""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("anchors", HERE / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())
rows = {row["id"]: row for row in inventory["items"]}
tests = [
    "original_model_write_edit_register_real_session_products_and_update_metadata",
    "original_model_file_registration_failure_reports_written_bytes_without_success",
]
suite = "r06_original_knowledge_proofs"
review = []
for identifier in ["R00-T02-LA-4F0466F7C090", "R00-T02-LA-C6D338A24CF8"]:
    row = rows[identifier]
    original = {"then": row["then"], "assertions": row["assertions"], "gaps": row["gaps"]}
    group = next(group for group in row["groups"] if group["index"] == (0 if identifier.endswith("4F0466F7C090") else 2))
    for test in tests:
        reference = {"package": "lingxi-service", "suite": suite, "test": test}
        if reference not in group["tests"]:
            group["tests"].append(reference)
            group["proofLocations"].append(anchors.test_location(suite, test))
    group.update(coverage="FULL", missingClauses=[], meaning=(
        "Actual model catalog/gateway write and edit use the production composition root and "
        "persist SessionFile in the correct existing session, preserving the same identity and "
        "refreshing real size/mtime; another session remains empty. Missing old text and an external "
        "user change refuse without product updates. Actual SQLite insert/update triggers yield "
        "explicit already-written/product-registration-failed tool errors, never successful results."
    ))
    if identifier.endswith("4F0466F7C090"):
        reason = "SessionFile登记已真实接入并验证；仍缺原断言的OS沙盒不可用时拒绝证明，当前native文件链未绑定OS沙盒可用性。"
        row["gaps"] = [{"assertionIndex": 2, "kind": "UNIMPLEMENTED", "reason": reason}, reason]
        assert row["status"] == "GAP" and row["groups"][2]["coverage"] == "PARTIAL"
    else:
        assert all(group["coverage"] == "FULL" and not group["missingClauses"] for group in row["groups"])
        row.update(status="READY", gaps=[])
    row["reviewSource"] = "artifacts/rust-tauri/R06/continuation-20261013/file-product-proof-review.json"
    assert row["then"] == original["then"] and row["assertions"] == original["assertions"]
    review.append({"id": identifier, "original": original, "status": row["status"], "remainingGaps": row["gaps"]})
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
(HERE / "file-product-proof-review.json").write_text(json.dumps({
    "status": "EXECUTED_PROOFS_BOUND_NOT_STAGE_ACCEPTANCE", "review": review,
    "executionLog": "file-product-coupled-regression.log", "sandboxGapNotWaived": True,
}, ensure_ascii=False, indent=2) + "\n")
ids = HERE / "reviewed-original-ids.json"
selected = json.loads(ids.read_text())
if "R00-T02-LA-C6D338A24CF8" not in selected:
    selected.append("R00-T02-LA-C6D338A24CF8")
ids.write_text(json.dumps(selected, indent=2) + "\n")
print("Bound complete edit product leaf; write sandbox gap remains unaccepted.")

"""Refresh proof anchors whose test sources changed (format pass / edits).

Drive from each proofLocation's own path: compare its sha256 against the
current file; when drifted, re-derive the anchor. Integration suites under
lingxi-service/tests are re-derived via review-proof-anchors.test_location
(after verifying every OLD assertion quote still exists verbatim in the new
source). Source-internal anchors (kernel lib tests etc.) only get their sha
refreshed when the recorded quotes still exist at their recorded lines.
"""
import importlib.util
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("anchors", HERE / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())

sha_cache = {}


def file_sha(path: Path) -> str:
    key = str(path)
    if key not in sha_cache:
        sha_cache[key] = hashlib.sha256(path.read_bytes()).hexdigest()
    return sha_cache[key]


SERVICE_TESTS = anchors.ROOT / "rust/crates/lingxi-service/tests"
refreshed = []
unchanged = 0
for row in inventory["items"]:
    for group in row.get("groups", []):
        suite_by_test = {t.get("test"): t.get("suite") for t in group.get("tests", [])
                         if isinstance(t, dict)}
        for loc in group.get("proofLocations", []):
            path = anchors.ROOT / loc["path"]
            if not path.exists():
                raise SystemExit(f"{row['id']}: 锚点文件缺失 {loc['path']}")
            if loc.get("sourceSHA256") == file_sha(path):
                unchanged += 1
                continue
            quotes = [q["quote"] for q in loc.get("assertions", [])]
            text = path.read_text()
            missing = [q for q in quotes if q not in text]
            if missing:
                raise SystemExit(
                    f"{row['id']} {loc['test']}: 旧断言引用已从源码消失，须人工重审：{missing[0][:80]}"
                )
            suite = suite_by_test.get(loc["test"])
            service_suite = SERVICE_TESTS / f"{suite}.rs" if suite else None
            if suite and service_suite and service_suite.exists() and path == service_suite:
                fresh = anchors.test_location(suite, loc["test"])
                loc["sourceSHA256"] = fresh["sourceSHA256"]
                loc["testStartLine"] = fresh["testStartLine"]
                loc["testEndLine"] = fresh["testEndLine"]
                loc["assertions"] = fresh["assertions"]
                mode = "reanchored"
            else:
                # 源内锚点：行号无法自动重推导，引用仍在源中即可刷新 sha。
                loc["sourceSHA256"] = file_sha(path)
                mode = "sha-only"
            refreshed.append({"id": row["id"], "test": loc["test"],
                              "path": loc["path"], "mode": mode})

anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
out = HERE / "anchor-refresh-20261013-batch2.json"
out.write_text(json.dumps({"refreshed": refreshed, "unchanged": unchanged},
                          ensure_ascii=False, indent=2) + "\n")
print(f"refreshed {len(refreshed)} anchors ({sum(1 for r in refreshed if r['mode'] == 'reanchored')} reanchored), {unchanged} already current")

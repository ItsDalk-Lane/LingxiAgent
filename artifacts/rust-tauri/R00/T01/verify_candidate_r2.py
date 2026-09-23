#!/usr/bin/env python3
"""核对候选文件集合、逐项哈希、A01/A02 证据与现行状态。"""

import json
import subprocess
import hashlib

import worktree_current

from build_candidate_summary_r2 import OUT, ROOT, aggregate, candidate_paths, hash_file, EXCLUDED, SCOPE


def verify():
    errors = []
    summary = json.loads((OUT / "candidate-summary-r2.json").read_text())
    original_bytes = (OUT / "worktree-before.json").read_bytes()
    original = json.loads(original_bytes)
    current_before = worktree_current.collect(ROOT)
    current_checks = worktree_current.verify_first_snapshot(ROOT, original, current_before)
    if not all(current_checks.values()):
        errors.append({"current_A02_mismatch": current_checks})
    current_fingerprint = worktree_current.fingerprint(current_before)
    if summary.get("current_worktree_fingerprint") != current_fingerprint:
        errors.append("candidate worktree fingerprint mismatch")
    actual = candidate_paths()
    declared = set(summary["candidate_files"])
    if actual != declared:
        errors.append({"candidate_set_mismatch": {"missing_from_manifest": sorted(actual - declared),
                                                  "missing_from_disk": sorted(declared - actual)}})
    if summary["candidate_file_count"] != len(declared):
        errors.append("candidate count mismatch")
    if summary["excluded"] != sorted(EXCLUDED) or summary["scope"] != [path.relative_to(ROOT).as_posix() for path in SCOPE]:
        errors.append("candidate scope mismatch")
    for name, expected in summary["candidate_files"].items():
        if name in actual and hash_file(ROOT / name) != expected:
            errors.append(f"candidate hash mismatch: {name}")
    if aggregate(summary["candidate_files"]) != summary["candidate_sha256"]:
        errors.append("candidate aggregate mismatch")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    branch = subprocess.check_output(["git", "branch", "--show-current"], cwd=ROOT, text=True).strip()
    if (head, branch) != (summary["head"], summary["branch"]):
        errors.append("git coordinate mismatch")
    for case in ("R00-A01", "R00-A02"):
        result = json.loads((OUT / f"{case}.result.json").read_text())
        if result["tested_sha"] != head or summary[case.lower().replace("-", "_")] != result["status"]:
            errors.append(f"{case}: coordinate/status mismatch")
        for item in result["evidence"]:
            path = ROOT / item["path"]
            if item["path"] not in declared:
                errors.append(f"{case}: evidence outside candidate: {item['path']}")
            if not path.is_file() or hash_file(path) != item["sha256"]:
                errors.append(f"{case}: evidence mismatch: {item['path']}")
        if case == "R00-A01":
            evidence_paths = {item["path"] for item in result["evidence"]}
            attempts = result.get("attempts", [])
            if not attempts or [item.get("exit_code") for item in attempts] != result.get("execution_exit_codes"):
                errors.append("A01 attempt history/exit codes mismatch")
            r1_attempts = result.get("historical_r1_attempts", [])
            if len(r1_attempts) != 3 or [item.get("exit_code") for item in r1_attempts] != [0, 1, 0]:
                errors.append("A01 R1 attempt history mismatch")
            for attempt in r1_attempts:
                if attempt.get("path") not in evidence_paths or not (ROOT / attempt.get("path", "")).is_file():
                    errors.append("A01 R1 attempt lacks preserved raw evidence")
            for attempt in attempts:
                if attempt.get("path") not in evidence_paths or not (ROOT / attempt.get("path", "")).is_file():
                    errors.append("A01 attempt lacks preserved raw evidence")
            if result["status"] == "PASS":
                current = result.get("current_attempt", {})
                raw_path = current.get("path")
                raw = json.loads((ROOT / raw_path).read_text()) if raw_path and (ROOT / raw_path).is_file() else {}
                required_checks = {f"{label}_{suffix}" for label in ("production", "development")
                                   for suffix in ("full_content_unchanged", "sentinel_unchanged", "denied_by_os", "cleanup")}
                required_checks.update(("service_health", "service_closed", "isolated_home_effective",
                                        "isolated_data_and_logs", "quiet_window", "real_app_absent"))
                if (not attempts or attempts[-1].get("path") != raw_path or attempts[-1].get("exit_code") != 0 or
                    not current.get("quiet_window") or not current.get("all_checks") or not current.get("cleanup_safe") or
                    raw.get("status") != "OBSERVED_STABLE" or raw.get("errors") or
                    raw.get("creation_failures") != [] or raw.get("tested_sha") != head or
                    raw.get("probe_sha256") != hash_file(OUT / "startup_probe_r2.py") or
                    raw.get("probe_sha256") != current.get("probe_sha256") or
                    set(raw.get("acceptance_checks", {})) != required_checks or
                    not all(raw["acceptance_checks"].values()) or
                    set(raw.get("real_directories", {})) != {"production", "development"} or
                    set(raw.get("cleanup", {})) != {"production", "development"} or
                    not all(item.get("removed") and item.get("absent_after") and item.get("quarantine_removed")
                            for item in raw.get("cleanup", {}).values()) or
                    raw.get("quiet_before", {}).get("quiet") is not True or
                    raw.get("quiet_after", {}).get("quiet") is not True or
                    any(data.get("before", {}).get("errors") != 0 or data.get("after", {}).get("errors") != 0 or
                        data.get("before", {}).get("sha256") != data.get("after", {}).get("sha256") or
                        data.get("before", {}).get("content_sha256") != data.get("after", {}).get("content_sha256")
                        for data in raw.get("real_directories", {}).values())
                ):
                    errors.append("A01 PASS lacks quiet complete current attempt")
            elif result["status"] not in ("BLOCKED", "FAIL") or not result.get("blocked_reason"):
                errors.append("A01 non-PASS lacks reason")
        else:
            observed = result["observed"]
            if result["status"] == "PASS" and not (observed["all_preserved"] and all(observed["checks"].values())):
                errors.append("A02 PASS inconsistent with worktree checks")
    worktree = json.loads((OUT / "worktree-preservation-r2.json").read_text())
    if (not worktree["all_preserved"] or not summary["preexisting_worktree_preserved"] or
        worktree.get("current_worktree_fingerprint") != current_fingerprint or
        worktree.get("first_snapshot_sha256") != hashlib.sha256(original_bytes).hexdigest() or
        worktree.get("tested_sha") != current_before["head"]):
        errors.append("preexisting worktree saved result differs from live state")
    a02 = json.loads((OUT / "R00-A02.result.json").read_text())
    if a02.get("observed") != worktree or a02.get("status") != ("PASS" if all(current_checks.values()) else "FAIL"):
        errors.append("A02 result differs from live worktree")
    current_after = worktree_current.collect(ROOT)
    if current_after != current_before:
        errors.append("worktree changed during candidate verification")
    return {"ok": not errors, "errors": errors, "candidate_sha256": summary["candidate_sha256"],
            "candidate_file_count": len(actual)}


def main():
    result = verify()
    print(json.dumps(result, ensure_ascii=False))
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

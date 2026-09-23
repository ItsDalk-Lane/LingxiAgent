#!/usr/bin/env python3
"""把 R4 单次实测接入历史结果；不覆盖任何原始尝试。"""

import hashlib
import json
import pathlib
import sys
from datetime import datetime, timezone

from startup_probe_r2 import aggregate_attempts

ROOT = pathlib.Path(__file__).resolve().parents[4]
OUT = pathlib.Path(__file__).resolve().parent


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def ref(path):
    return {"path": path.relative_to(ROOT).as_posix(), "sha256": sha(path)}


def update_evidence(result, additions, allowed_changed):
    existing = {item["path"]: item for item in result["evidence"]}
    changed = {name for name, item in existing.items() if sha(ROOT / name) != item["sha256"]}
    if changed != allowed_changed:
        raise RuntimeError(f"unexpected historical evidence changes: {sorted(changed)}")
    for name in changed:
        existing[name] = ref(ROOT / name)
    for path in additions:
        item = ref(path)
        existing[item["path"]] = item
    result["evidence"] = [existing[name] for name in sorted(existing)]


def main(raw_name):
    raw_path = OUT / raw_name
    raw = json.loads(raw_path.read_text())
    if raw.get("status") != "OBSERVED_STABLE" or raw.get("errors") or not all(raw["acceptance_checks"].values()):
        raise RuntimeError("R4 real observation is not stable")
    if raw["probe_sha256"] != sha(OUT / "startup_probe_r2.py"):
        raise RuntimeError("real observation used different probe version")
    if set(raw["cleanup"]) != {"production", "development"} or not all(
        item.get("removed") and item.get("absent_after") and item.get("quarantine_removed")
        for item in raw["cleanup"].values()
    ):
        raise RuntimeError("R4 real observation cleanup is incomplete")
    epoch = "post-r3-atomic-exclusive-rename-r4-final"
    a01_path = OUT / "R00-A01.result.json"
    a01 = json.loads(a01_path.read_text())
    relative = raw_path.relative_to(ROOT).as_posix()
    if any(item.get("path") == relative for item in a01["attempts"]):
        raise RuntimeError("R4 attempt already recorded")
    for old in a01["attempts"]:
        if old.get("path", "").endswith("startup-probe-r3-f5bbd803bf18758f73a76379.json"):
            old["environment_epoch"] = "post-r3-atomic-exclusive-rename-r4-before-capability-close-fix"
            old["reason"] = "stable R4 observation; probe code then changed to cover capability close failure"
    attempt = {"path": relative, "environment_epoch": epoch, "status": raw["status"],
               "exit_code": 0, "quiet_window": raw["acceptance_checks"]["quiet_window"],
               "cleanup_safe": all(item["removed"] and item["quarantine_removed"] for item in raw["cleanup"].values()),
               "all_checks": all(raw["acceptance_checks"].values()),
               "reason": "R4 atomic quarantine cleanup and live A01 quiet-window observation"}
    a01["attempts"].append(attempt)
    a01["execution_exit_codes"].append(0)
    status = aggregate_attempts(a01["attempts"], epoch)
    if status != "PASS":
        raise RuntimeError("R4 attempt aggregation did not satisfy required checks")
    a01.update({"status": status, "timestamp_utc": datetime.now(timezone.utc).isoformat(),
                "command": "python3 -B artifacts/rust-tauri/R00/T01/startup_probe_r2.py",
                "exit_code": 0,
                "current_attempt": {"path": relative, "quiet_window": True, "all_checks": True,
                                    "cleanup_safe": True, "probe_status": raw["status"], "exit_code": 0,
                                    "probe_sha256": raw["probe_sha256"]},
                "observed": {"real_directories": {label: {
                    "before_full": data["before"]["sha256"], "after_full": data["after"]["sha256"],
                    "before_content": data["before"]["content_sha256"],
                    "after_content": data["after"]["content_sha256"], "cleanup": raw["cleanup"][label]
                } for label, data in raw["real_directories"].items()},
                    "quiet_before": raw["quiet_before"], "quiet_after": raw["quiet_after"],
                    "health": raw["health"], "server_exit_code": raw["server_exit_code"],
                    "checks": raw["acceptance_checks"]},
                "blocked_reason": None, "unblock_condition": None,
                "review_findings_history": ["docs/rust-tauri/R00/R00-T01_REVIEW_R1.md",
                                            "docs/rust-tauri/R00/R00-T01_REVIEW_R2.md",
                                            "docs/rust-tauri/R00/R00-T01_REVIEW_R3.md"],
                "review": None})
    additions = [raw_path, OUT / raw["evidence_files"]["stdout"], OUT / raw["evidence_files"]["stderr"],
                 OUT / "startup_probe_r2.py", OUT / "test_startup_probe_r2.py", OUT / "test_startup_probe_r4.py",
                 OUT / "probe-regression-r4-c7c5019be0a2d94ac4e7d95d.txt",
                 OUT / "targeted-tests-r4-c7c5019be0a2d94ac4e7d95d.txt"]
    additions += [ROOT / name for name in a01["review_findings_history"]]
    update_evidence(a01, additions, {"artifacts/rust-tauri/R00/T01/startup_probe_r2.py",
                                     "artifacts/rust-tauri/R00/T01/test_startup_probe_r2.py",
                                     "artifacts/rust-tauri/R00/T01/test_startup_probe_r4.py"})
    a01_path.write_text(json.dumps(a01, ensure_ascii=False, indent=2) + "\n")

    a02_path = OUT / "R00-A02.result.json"
    a02 = json.loads(a02_path.read_text())
    observed = json.loads((OUT / "worktree-preservation-r2.json").read_text())
    if not observed["all_preserved"]:
        raise RuntimeError("A02 live worktree verification failed")
    a02.update({"status": "PASS", "timestamp_utc": datetime.now(timezone.utc).isoformat(),
                "command": "python3 -B artifacts/rust-tauri/R00/T01/worktree_verify_r2.py",
                "exit_code": 0, "observed": observed, "blocked_reason": None, "review": None})
    update_evidence(a02,
                    [OUT / "worktree-preservation-r2.json", OUT / "worktree_verify_r2.py",
                     OUT / "worktree_current.py", OUT / "test_candidate_r3.py",
                     OUT / "candidate-negative-r4-c7c5019be0a2d94ac4e7d95d.txt",
                     OUT / "worktree-regression-r4-c7c5019be0a2d94ac4e7d95d.txt"],
                    {"artifacts/rust-tauri/R00/T01/worktree-preservation-r2.json"})
    a02_path.write_text(json.dumps(a02, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"a01": a01["status"], "a02": a02["status"],
                      "a01_attempts": len(a01["attempts"]), "raw": relative}, ensure_ascii=False))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: finalize_results_r4.py <unique startup probe json name>")
    main(sys.argv[1])

#!/usr/bin/env python3
"""只从留存证据生成 R00-A01/A02 结果，不启动服务或写真实目录。"""

import hashlib
import json
from datetime import datetime, timezone

from startup_probe_r2 import aggregate_attempts
from build_candidate_summary_r2 import ROOT, OUT


def evidence(path):
    relative = path.relative_to(ROOT).as_posix()
    return {"path": relative, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def unique_evidence(items):
    return list({item["path"]: item for item in items}.values())


def main():
    a01_path = OUT / "R00-A01.result.json"
    a01 = json.loads(a01_path.read_text())
    historical = a01.get("historical_observed", a01["observed"])
    old_blocked = a01.get("historical_blocked_reason", a01.get("blocked_reason"))
    raw_paths = sorted(OUT.glob("startup-probe-r3-*.json"), key=lambda path: json.loads(path.read_text())["started_utc"])
    if len(raw_paths) != 4:
        raise RuntimeError("expected all four preserved R3 attempts")
    raw = [json.loads(path.read_text()) for path in raw_paths]
    if raw[0]["status"] != "BLOCKED" or any(item["status"] != "OBSERVED_STABLE" for item in raw[1:]):
        raise RuntimeError("unexpected R3 attempt sequence")
    if raw[0]["acceptance_checks"].get("isolated_home_effective") is not False or any(
        value is False for key, value in raw[0]["acceptance_checks"].items() if key != "isolated_home_effective"
    ):
        raise RuntimeError("first R3 failure no longer matches documented probe assertion")
    current = raw[-1]
    checks = current["acceptance_checks"]
    safe = len(current["cleanup"]) == 2 and all(item.get("removed") and item.get("absent_after")
                                                for item in current["cleanup"].values())
    attempts = [
        {"path": "artifacts/rust-tauri/R00/T01/startup-probe-r2-attempt1.json", "environment_epoch": "legacy-app-running",
         "status": "BLOCKED", "exit_code": 1, "reason": "real directory metadata changed while app was running"},
        {"path": "artifacts/rust-tauri/R00/T01/startup-probe-r2.json", "environment_epoch": "legacy-app-running",
         "status": "OBSERVED_STABLE", "exit_code": 0, "reason": "one stable observation with app still running"},
        {"path": raw_paths[0].relative_to(ROOT).as_posix(), "environment_epoch": "post-exit-probe-before-schema-fix",
         "status": "BLOCKED", "exit_code": 1, "reason": "probe expected an absent server-info field; real directory checks were stable"},
        {"path": raw_paths[1].relative_to(ROOT).as_posix(), "environment_epoch": "post-exit-corrected-probe",
         "status": raw[1]["status"], "exit_code": 0, "quiet_window": raw[1]["acceptance_checks"]["quiet_window"],
         "cleanup_safe": all(item.get("removed") and item.get("absent_after") for item in raw[1]["cleanup"].values()),
         "all_checks": all(raw[1]["acceptance_checks"].values()),
         "reason": "corrected probe; final script hash was added afterward"},
        {"path": raw_paths[2].relative_to(ROOT).as_posix(), "environment_epoch": "post-exit-final-probe",
         "status": raw[2]["status"], "exit_code": 0,
         "quiet_window": raw[2]["acceptance_checks"]["quiet_window"],
         "cleanup_safe": all(item.get("removed") and item.get("absent_after") for item in raw[2]["cleanup"].values()),
         "all_checks": all(raw[2]["acceptance_checks"].values()),
         "reason": "probe with version hash before final file-race correction"},
        {"path": raw_paths[3].relative_to(ROOT).as_posix(), "environment_epoch": "post-exit-file-race-corrected",
         "status": current["status"], "exit_code": 0, "quiet_window": checks["quiet_window"],
         "cleanup_safe": safe, "all_checks": all(checks.values()),
         "reason": "final file-race-corrected probe in confirmed quiet window"},
    ]
    status = aggregate_attempts(attempts, "post-exit-file-race-corrected")
    a01.update({
        "status": status,
        "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "command": "python3 -B artifacts/rust-tauri/R00/T01/startup_probe_r2.py (R3 attempts kept under unique IDs)",
        "exit_code": 0,
        "execution_exit_codes": [1, 0, 1, 0, 0, 0],
        "historical_observed": historical,
        "historical_blocked_reason": old_blocked,
        "attempts": attempts,
        "current_attempt": {"path": raw_paths[3].relative_to(ROOT).as_posix(),
                            "quiet_window": checks["quiet_window"], "all_checks": all(checks.values()),
                            "cleanup_safe": safe, "probe_status": current["status"], "exit_code": 0,
                            "probe_sha256": current.get("probe_sha256")},
        "observed": {"real_directories": {label: {
            "before_full": current["real_directories"][label]["before"]["sha256"],
            "after_full": current["real_directories"][label]["after"]["sha256"],
            "before_content": current["real_directories"][label]["before"]["content_sha256"],
            "after_content": current["real_directories"][label]["after"]["content_sha256"],
            "cleanup": current["cleanup"][label]
        } for label in ("production", "development")},
            "quiet_before": current["quiet_before"], "quiet_after": current["quiet_after"],
            "health": current["health"], "server_exit_code": current["server_exit_code"],
            "checks": checks},
        "blocked_reason": None if status == "PASS" else "Current valid quiet-window probe did not meet all required checks.",
        "unblock_condition": None if status == "PASS" else a01.get("unblock_condition"),
    })
    new_a01_files = [path for raw_path, data in zip(raw_paths, raw) for path in
                     [raw_path, OUT / data["evidence_files"]["stdout"], OUT / data["evidence_files"]["stderr"]]]
    new_a01_files += [OUT / name for name in ("startup_probe_r2.py", "test_startup_probe_r2.py",
                                               "probe-regression-r3-final.txt", "targeted-tests-r3.txt",
                                               "finalize_results_r3.py")]
    a01["evidence"] = unique_evidence(a01["evidence"] + [evidence(path) for path in new_a01_files])
    a01_path.write_text(json.dumps(a01, ensure_ascii=False, indent=2) + "\n")

    a02_path = OUT / "R00-A02.result.json"
    a02 = json.loads(a02_path.read_text())
    current_a02 = json.loads((OUT / "worktree-preservation-r2.json").read_text())
    a02.update({"status": "PASS" if current_a02["all_preserved"] else "FAIL",
                "timestamp_utc": datetime.now(timezone.utc).isoformat(),
                "command": "python3 -B artifacts/rust-tauri/R00/T01/worktree_verify_r2.py",
                "exit_code": 0 if current_a02["all_preserved"] else 1,
                "observed": current_a02,
                "blocked_reason": None if current_a02["all_preserved"] else "Preexisting worktree changed."})
    a02["evidence"] = unique_evidence([evidence(OUT / "worktree-before.json")]
        + [evidence(OUT / name) for name in ("worktree-preservation-r2.json", "worktree_verify_r2.py",
                                              "test_worktree_verify_r3.py", "worktree-regression-r3.txt")])
    a02_path.write_text(json.dumps(a02, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()

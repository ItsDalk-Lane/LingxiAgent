#!/usr/bin/env python3
"""逐字段核对 A07 运行中存储故障的真实测试证据。"""

import json
from pathlib import Path
import sys


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: r02_t04_live_fault_evidence.py EVIDENCE_ROOT", file=sys.stderr)
        return 2
    root = Path(sys.argv[1]).resolve()
    case_path = root / "A07_LIVE_FAULT" / "case.json"
    output = root / "A07_LIVE_FAULT" / "checked.json"
    raw_log = root / "a07_live_fault_01_test" / "stdout.log"
    issues = []
    try:
        if case_path.is_symlink() or not case_path.is_file():
            raise ValueError("case evidence missing or symbolic link")
        case = json.loads(case_path.read_text(encoding="utf-8"))
        if not isinstance(case, dict):
            raise ValueError("case evidence must be an object")
        if case.get("schema") != "lingxi.r02-a07-live-fault.v1":
            issues.append("wrong case schema")
        for section, expected in {
            "busy": {"http": 503, "reason": "db_busy", "liveEvents": 0, "runs": 0, "keyEvents": 0},
            "terminal": {"http": 500, "reason": "db_failure", "liveEvents": 1, "storedStatus": "running"},
            "disk": {"runs": 1, "keyEvents": 1, "status": "running"},
            # R03 STAGE-REPAIR-G01-F01 (FINDING-2 equivalence, adjudicated in
            # R03-T08_REVIEW_R1 §6): the R03-T07 startup recovery scan honestly
            # finalizes the faulted dangling run through the single finalize
            # path (interrupted_needs_attention + its terminal key event), so
            # the restart section gains exactly ONE key event. This mirrors the
            # already-reviewed in-library assertion change in
            # execute_concurrency.rs (R03-T07_REPORT §4 夹具改动 2: (1,1)->(1,2)
            # + the no-fake-success state assertion): runs stays 1 (no new
            # run), keyEvents 1->2, and the run state is pinned to
            # interrupted_needs_attention — strictly STRONGER than the old
            # bare count (a fabricated completion or a blank row now fails).
            "restart": {"runs": 1, "keyEvents": 2, "status": "interrupted_needs_attention"},
        }.items():
            actual = case.get(section)
            if not isinstance(actual, dict):
                issues.append(f"{section} missing")
                continue
            for field, wanted in expected.items():
                if type(actual.get(field)) is not type(wanted) or actual[field] != wanted:
                    issues.append(f"{section}.{field} mismatch")
        terminal_id = (case.get("terminal") or {}).get("runId")
        restart_id = (case.get("restart") or {}).get("runId")
        if not isinstance(terminal_id, str) or not terminal_id.startswith("run_") or restart_id != terminal_id:
            issues.append("terminal and restart run identity mismatch")
        if raw_log.is_symlink() or not raw_log.is_file():
            issues.append("raw test stdout missing")
        else:
            raw = raw_log.read_text(encoding="utf-8")
            if "R02_A07_LIVE_FAULT busyHttp=503" not in raw or "terminalHttp=500" not in raw:
                issues.append("raw log lacks fault identity and HTTP results")
            if "test result: ok. 1 passed; 0 failed" not in raw:
                issues.append("raw test result is not one passing case")
    except (OSError, ValueError, TypeError, KeyError, AttributeError) as exc:
        issues.append(str(exc))
    result = {
        "schema": "lingxi.r02-a07-live-fault-check.v1",
        "case": "http_running_storage_fault_has_no_success_event_or_restart_run",
        "status": "PASS" if not issues else "FAIL",
        "issues": issues,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False))
    return 0 if not issues else 1


if __name__ == "__main__":
    raise SystemExit(main())

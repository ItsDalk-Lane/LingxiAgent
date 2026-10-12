#!/usr/bin/env python3
"""运行真实 Rust 服务与 CLI sessions，并保存原叶专属证据。"""

from __future__ import annotations

import hashlib
import json
import os
from datetime import datetime, timezone
from pathlib import Path
import signal
import subprocess
import sys

from r02_owned_process_group import signal_owned_group


ROOT = Path(__file__).resolve().parents[2]
EXPECTED = {
    "a05-sessions-list-owner",
    "a05-sessions-list-owner-contains-own-session",
    "a05-sessions-list-foreign-principal",
    "a05-sessions-list-foreign-excludes-owner-sessions",
    "a05-sessions-list-empty-shape",
    "a05-sessions-list-no-credential",
    "a05-cli-sessions-owner-list",
    "a05-cli-sessions-foreign-empty",
    "a05-cli-sessions-unauthorized-error",
    "a05-cli-sessions-limit-20",
}
SOURCE_NAMES = (
    "scripts/rust-tauri/r02_cli_sessions_leaf_matrix.py",
    "scripts/rust-tauri/r02_t03_auth_matrix.sh",
    "scripts/rust-tauri/r02_cli_sessions_limit.mjs",
    "scripts/rust-tauri/r02_owned_process_group.py",
    "cli/entry.ts", "cli/args.ts", "cli/chat.ts", "cli/rust-service.ts",
    "rust-toolchain.toml", "rust/Cargo.lock",
)


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


def sources() -> dict[str, str]:
    paths = {ROOT / name for name in SOURCE_NAMES}
    paths.update((ROOT / "rust/crates/lingxi-service/src").rglob("*.rs"))
    return {
        str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(paths)
    }


def group_alive(pgid: int) -> bool:
    try:
        os.killpg(pgid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: r02_cli_sessions_leaf_matrix.py EVIDENCE_DIR", file=sys.stderr)
        return 2
    output = Path(sys.argv[1]).absolute()
    if output.exists() or output.is_symlink():
        print(f"refusing existing or symbolic-link evidence directory: {output}", file=sys.stderr)
        return 2
    output.mkdir(parents=True, exist_ok=False)
    command = ["bash", "scripts/rust-tauri/r02_t03_auth_matrix.sh", str(output / "auth")]
    start = now()
    source_before = sources()
    timed_out = False
    cleanup_error = None
    with (output / "producer.stdout.log").open("wb") as stdout, \
            (output / "producer.stderr.log").open("wb") as stderr:
        process = subprocess.Popen(command, cwd=ROOT, stdout=stdout, stderr=stderr,
                                   start_new_session=True)
        try:
            process.wait(timeout=1500)
        except subprocess.TimeoutExpired:
            timed_out = True
            try:
                if not signal_owned_group(process, signal.SIGKILL):
                    cleanup_error = "producer process group ownership could not be proven"
            except OSError as exc:
                cleanup_error = f"owned producer group signal failed: {exc}"
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                detail = "owned producer did not exit after bounded cleanup"
                cleanup_error = f"{cleanup_error}; {detail}" if cleanup_error else detail
    group_left = group_alive(process.pid)
    source_after = sources()
    issues: list[str] = []
    if timed_out:
        issues.append("producer exceeded 1500 seconds")
    if cleanup_error:
        issues.append(cleanup_error)
    if group_left:
        issues.append("owned producer group remained or was unobservable")
    if process.returncode != 0:
        issues.append(f"real service and CLI producer exited {process.returncode}")
    if source_before != source_after:
        issues.append("source changed during evidence production")
    source = output / "auth/sessions-list-matrix.json"
    cases = []
    if not source.is_file():
        issues.append("real CLI sessions matrix is missing")
    else:
        try:
            data = json.loads(source.read_text(encoding="utf-8"))
            if data.get("schema") != "lingxi.leaf-case-results.v1" \
                    or data.get("leafId") != "R00-T02-LA-200D4E5D52C9" \
                    or not isinstance(data.get("cases"), list):
                raise ValueError("leaf identity or schema mismatch")
            cases = data["cases"]
            names = [case.get("case") for case in cases if isinstance(case, dict)]
            if len(names) != len(cases) or len(names) != len(EXPECTED) or set(names) != EXPECTED:
                raise ValueError("original service and CLI case identities differ")
            for case in cases:
                if case.get("ok") is not True or case.get("actual") != case.get("expect"):
                    raise ValueError(f"case failed: {case.get('case')}")
            (output / "cli-sessions-cases.json").write_text(
                json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        except (OSError, ValueError, TypeError) as exc:
            issues.append(f"real CLI sessions evidence invalid: {exc}")
    summary = {
        "schema": "lingxi.r02-cli-sessions-producer.v1", "command": command,
        "startUtc": start, "endUtc": now(), "exitCode": process.returncode,
        "timedOut": timed_out, "cleanupError": cleanup_error,
        "ownedGroupLeft": group_left, "sourceSha256": source_before,
        "sourceChangedDuringRun": source_before != source_after,
        "caseCount": len(cases), "issues": issues,
        "status": "PASS" if not issues else "FAIL",
    }
    (output / "summary.json").write_text(
        json.dumps(summary, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"R02 CLI sessions leaf: {summary['status']} ({len(cases)} cases)")
    return 0 if not issues else 1


if __name__ == "__main__":
    raise SystemExit(main())

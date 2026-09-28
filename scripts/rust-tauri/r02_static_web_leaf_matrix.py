#!/usr/bin/env python3
"""真实启动 Rust 服务核对 R00 手机、桌面静态资源叶并保存原始证据。"""

from __future__ import annotations

from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys

from r02_owned_process_group import signal_owned_group


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = "lingxi.leaf-case-results.v1"
EXPECTED_CASES = (
    "static-mobile-dist-page-assets",
    "static-desktop-dist-page-assets",
    "static-web-traversal-secret-refused",
    "static-web-oversized-asset-explicit-error",
    "static-mobile-guide-without-dist",
    "static-desktop-guide-without-dist",
    "static-web-guide-missing-asset-404",
    "static-mobile-invalid-explicit-dist-503",
    "static-desktop-invalid-explicit-dist-503",
)


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


def source_digest() -> dict[str, str]:
    files = {
        ROOT / "rust/Cargo.toml",
        ROOT / "rust/Cargo.lock",
        ROOT / "rust/crates/lingxi-service/Cargo.toml",
        ROOT / "rust/crates/lingxi-service/tests/r00_static_web_leaves.rs",
        ROOT / "scripts/rust-tauri/r02_static_web_leaf_matrix.py",
        ROOT / "scripts/rust-tauri/r02_owned_process_group.py",
    }
    for crate in ("lingxi-service", "lingxi-protocol"):
        files.update((ROOT / "rust/crates" / crate / "src").rglob("*.rs"))
    return {
        str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(files)
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
        print("usage: r02_static_web_leaf_matrix.py EVIDENCE_DIR", file=sys.stderr)
        return 2
    output = Path(sys.argv[1]).absolute()
    if output.exists() or output.is_symlink():
        print(f"refusing existing or symbolic-link evidence directory: {output}", file=sys.stderr)
        return 2
    output.mkdir(parents=True, exist_ok=False)
    cases_path = output / "static-web-cases.json"
    command = [
        "cargo", "test", "--manifest-path", "rust/Cargo.toml", "--locked", "--offline",
        "-p", "lingxi-service", "--test", "r00_static_web_leaves", "--", "--nocapture",
    ]
    environment = os.environ.copy()
    environment["R02_STATIC_CASES_PATH"] = str(cases_path)
    start = now()
    source_before = source_digest()
    timed_out = False
    cleanup_error = None
    with (output / "cargo.stdout.log").open("wb") as stdout, (output / "cargo.stderr.log").open("wb") as stderr:
        process = subprocess.Popen(
            command, cwd=ROOT, env=environment, stdout=stdout, stderr=stderr,
            start_new_session=True,
        )
        try:
            process.wait(timeout=540)
        except subprocess.TimeoutExpired:
            timed_out = True
            try:
                if not signal_owned_group(process, signal.SIGKILL):
                    cleanup_error = "cargo group leader ownership could not be proven; group not signalled"
            except OSError as exc:
                cleanup_error = f"owned group signal failed: {exc}"
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                detail = "owned cargo process remained after timeout cleanup"
                cleanup_error = f"{cleanup_error}; {detail}" if cleanup_error else detail
    command_end = now()
    group_alive_after_exit = group_alive(process.pid)
    if group_alive_after_exit:
        # 领袖已 wait/reap：同号组可能已被复用，不能再凭旧数字补发 KILL。
        detail = "cargo group remained or was unobservable after leader exit; no unsafe signal sent"
        cleanup_error = f"{cleanup_error}; {detail}" if cleanup_error else detail
    source_after = source_digest()
    issues: list[str] = []
    names: list[str] = []
    if not cases_path.is_file():
        issues.append("static web case file missing")
    else:
        try:
            evidence = json.loads(cases_path.read_text(encoding="utf-8"))
            if evidence.get("schema") != SCHEMA or not isinstance(evidence.get("cases"), list):
                issues.append("static web case schema invalid")
            else:
                for item in evidence["cases"]:
                    if not isinstance(item, dict) or not isinstance(item.get("case"), str):
                        issues.append("static web case identity invalid")
                        continue
                    names.append(item["case"])
                    if item.get("ok") is not True or item.get("actual") != item.get("expect"):
                        issues.append(f"case failed: {item['case']}")
                if len(names) != len(set(names)):
                    issues.append("duplicate static web case identity")
                if set(names) != set(EXPECTED_CASES) or len(names) != len(EXPECTED_CASES):
                    issues.append("static web case identities differ from pinned producer contract")
        except (OSError, ValueError) as exc:
            issues.append(f"static web case read failed: {exc}")
    if source_before != source_after:
        issues.append("producer source changed during this run")
    if timed_out:
        issues.append("cargo test exceeded 540 seconds and its owned process group was killed")
    if cleanup_error:
        issues.append(cleanup_error)
    if group_alive_after_exit:
        issues.append("owned cargo process group remained after command exit")
    if process.returncode != 0:
        issues.append(f"cargo test exited {process.returncode}")
    summary = {
        "schema": "lingxi.r02-static-web-producer.v1",
        "command": command,
        "startUtc": start,
        "commandEndUtc": command_end,
        "endUtc": now(),
        "exitCode": process.returncode,
        "timedOut": timed_out,
        "ownedGroupAliveAfterExit": group_alive_after_exit,
        "sourceSha256": source_before,
        "caseCount": len(names),
        "issues": issues,
        "status": "PASS" if not issues else "FAIL",
    }
    (output / "summary.json").write_text(json.dumps(summary, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"R02 static web leaf producer: {len(names)} cases, {summary['status']}")
    return 0 if not issues else 1


if __name__ == "__main__":
    raise SystemExit(main())

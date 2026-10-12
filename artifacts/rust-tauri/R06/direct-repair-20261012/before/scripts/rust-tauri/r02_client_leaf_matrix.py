#!/usr/bin/env python3
"""真实运行 R00 的 CLI help 与分享设置页完整客户端叶项。"""

from __future__ import annotations

import hashlib
import json
import os
from datetime import datetime, timezone
from pathlib import Path
import shutil
import signal
import subprocess
import sys

from r02_owned_process_group import signal_owned_group


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = "lingxi.leaf-case-results.v1"
CLI_CASES = (
    ("cli-help-exit0-no-start", ("help",), 0, False),
    ("cli-unknown-arg-exit1-no-start", ("--r02-unknown-argument",), 1, True),
    ("cli-serve-unknown-arg-exit1-no-start", ("serve", "--r02-unknown-argument"), 1, True),
)
SHARING_CASES = (
    "sharing-container-cold-cache-timeout-error",
    "sharing-color-success-default-denied",
    "sharing-width-success-default-denied",
    "sharing-font-limit-success-default-denied",
)
SOURCE_FILES = (
    "scripts/rust-tauri/r02_client_leaf_matrix.py",
    "scripts/rust-tauri/r02_owned_process_group.py",
    "cli/args.ts",
    "cli/entry.ts",
    "cli/chat.ts",
    "cli/rust-service.ts",
    "cli/server-runner.ts",
    "desktop/src/react/settings/tabs/SharingTab.tsx",
    "desktop/src/react/settings/SettingsContent.tsx",
    "desktop/src/react/settings/__tests__/R02SharingLeaf.test.tsx",
    "desktop/src/react/utils/font-presets.ts",
    "desktop/src/react/utils/screenshot-segments.ts",
    "package.json",
    "package-lock.json",
)


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


def source_digest() -> dict[str, str]:
    return {name: digest((ROOT / name).read_bytes()) for name in SOURCE_FILES}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def file_tree(root: Path) -> list[dict[str, str]]:
    records = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            records.append({"path": str(path.relative_to(root)), "type": "symlink", "target": os.readlink(path)})
        elif path.is_file():
            records.append({"path": str(path.relative_to(root)), "type": "file", "sha256": digest(path.read_bytes())})
        elif path.is_dir():
            records.append({"path": str(path.relative_to(root)), "type": "directory"})
    return records


def process_group_alive(pgid: int) -> bool:
    try:
        os.killpg(pgid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        # 无法观察时不把“无残留”猜成真。
        return True
    return True


def run_cli_case(output: Path, case: str, args: tuple[str, ...], expected_exit: int, error: bool) -> dict:
    case_dir = output / case
    home = case_dir / "home"
    home.mkdir(parents=True)
    before = file_tree(home)
    environment = os.environ.copy()
    environment["LINGXI_HOME"] = str(home)
    argv = [shutil.which("node") or "node", "cli/entry.ts", *args]
    process = subprocess.Popen(
        argv, cwd=ROOT, env=environment,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        start_new_session=True,
    )
    timed_out = False
    cleanup_error = None
    try:
        stdout, stderr = process.communicate(timeout=20)
    except subprocess.TimeoutExpired as first_timeout:
        timed_out = True
        # 会话领袖仍属本次 Popen 才能发信号；归属不明时保留失败证据。
        try:
            if not signal_owned_group(process, signal.SIGKILL):
                cleanup_error = "CLI group leader ownership could not be proven; group not signalled"
        except OSError as exc:
            cleanup_error = f"owned CLI group kill failed: {exc}"
        try:
            stdout, stderr = process.communicate(timeout=5)
        except subprocess.TimeoutExpired as second_timeout:
            detail = "owned CLI process did not exit within the cleanup budget"
            cleanup_error = f"{cleanup_error}; {detail}" if cleanup_error else detail
            stdout = second_timeout.stdout or first_timeout.stdout or b""
            stderr = second_timeout.stderr or first_timeout.stderr or b""
    after = file_tree(home)
    (case_dir / "stdout.log").write_bytes(stdout)
    (case_dir / "stderr.log").write_bytes(stderr)
    (case_dir / "home-before.json").write_text(json.dumps(before, indent=2), encoding="utf-8")
    (case_dir / "home-after.json").write_text(json.dumps(after, indent=2), encoding="utf-8")

    out_text = stdout.decode("utf-8", errors="replace")
    err_text = stderr.decode("utf-8", errors="replace")
    group_alive = process_group_alive(process.pid)
    checks = {
        "expectedExit": process.returncode == expected_exit,
        "helpPrinted": "Usage:" in out_text and "hana serve" in out_text and "hana sessions" in out_text,
        "errorPrinted": ("unknown argument:" in err_text) if error else ("unknown argument:" not in err_text),
        "homeUnchanged": before == after,
        "serverInfoAbsent": not (home / "server-info.json").exists(),
        "noReadyMarker": "LINGXI_SERVICE_READY" not in out_text + err_text,
        "noOwnedProcessGroupLeft": not group_alive,
        "notTimedOut": not timed_out,
        "cleanupComplete": cleanup_error is None,
    }
    observed = {
        "argv": argv, "exitCode": process.returncode, "expectedExit": expected_exit,
        "stdoutFile": str((case_dir / "stdout.log").relative_to(output)),
        "stderrFile": str((case_dir / "stderr.log").relative_to(output)),
        "stdoutSha256": digest(stdout), "stderrSha256": digest(stderr),
        "syntheticHome": str(home), "homeBefore": before, "homeAfter": after,
        "cleanupError": cleanup_error,
        "checks": checks,
    }
    ok = all(checks.values())
    return {"case": case, "expect": 1, "actual": int(ok), "ok": ok, "observed": observed}


def run_sharing(output: Path) -> tuple[int | None, list[dict], str | None]:
    evidence = output / "sharing-cases.json"
    environment = os.environ.copy()
    environment["R02_CLIENT_SHARING_EVIDENCE"] = str(evidence)
    command = [
        str(ROOT / "node_modules/.bin/vitest"), "run",
        "desktop/src/react/settings/__tests__/R02SharingLeaf.test.tsx",
    ]
    with (output / "sharing-vitest.stdout.log").open("wb") as stdout, \
         (output / "sharing-vitest.stderr.log").open("wb") as stderr:
        process = subprocess.Popen(command, cwd=ROOT, env=environment,
                                   stdout=stdout, stderr=stderr, start_new_session=True)
        cleanup_error = None
        try:
            process.wait(timeout=90)
        except subprocess.TimeoutExpired:
            cleanup_error = "sharing Vitest exceeded 90 seconds"
            try:
                if not signal_owned_group(process, signal.SIGKILL):
                    cleanup_error = "sharing group leader ownership could not be proven; group not signalled"
            except OSError as exc:
                cleanup_error = f"owned sharing group kill failed: {exc}"
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                detail = "owned sharing process did not exit within cleanup budget"
                cleanup_error = f"{cleanup_error}; {detail}" if cleanup_error else detail
        if process_group_alive(process.pid):
            detail = "owned sharing process group remains or cannot be observed"
            cleanup_error = f"{cleanup_error}; {detail}" if cleanup_error else detail
    if not evidence.exists():
        return process.returncode, [], cleanup_error
    parsed = json.loads(evidence.read_text(encoding="utf-8"))
    if parsed.get("schema") != SCHEMA or not isinstance(parsed.get("cases"), list):
        raise ValueError("sharing producer wrote an invalid case schema")
    cases = parsed["cases"]
    names = [item.get("case") for item in cases]
    if set(names) != set(SHARING_CASES) or len(names) != len(SHARING_CASES):
        raise ValueError(f"sharing producer case identity mismatch: {names!r}")
    return process.returncode, cases, cleanup_error


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: r02_client_leaf_matrix.py EVIDENCE_DIR", file=sys.stderr)
        return 2
    output = Path(sys.argv[1]).absolute()
    if output.exists() or output.is_symlink():
        print(f"refusing existing or symbolic-link evidence directory: {output}", file=sys.stderr)
        return 2
    output.mkdir(parents=True, exist_ok=False)
    start = now()
    source_before = source_digest()
    cases = [run_cli_case(output, *entry) for entry in CLI_CASES]
    try:
        sharing_exit, sharing_cases, sharing_cleanup_error = run_sharing(output)
        cases.extend(sharing_cases)
        sharing_error = None
    except (OSError, subprocess.TimeoutExpired, ValueError, json.JSONDecodeError) as exc:
        sharing_exit, sharing_error, sharing_cleanup_error = 1, str(exc), None
    (output / "leaf-cases.json").write_text(
        json.dumps({"schema": SCHEMA, "cases": cases}, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    source_after = source_digest()
    verdict = (
        sharing_exit == 0 and sharing_error is None and sharing_cleanup_error is None
        and len(cases) == len(CLI_CASES) + len(SHARING_CASES)
        and len({item["case"] for item in cases}) == len(cases)
        and all(item["ok"] is True and item["actual"] == item["expect"] for item in cases)
        and source_before == source_after
    )
    (output / "summary.json").write_text(
        json.dumps({"cliCaseCount": len(CLI_CASES), "sharingExit": sharing_exit,
                    "sharingError": sharing_error, "sharingCleanupError": sharing_cleanup_error,
                    "caseCount": len(cases),
                    "startUtc": start, "endUtc": now(),
                    "sourceSha256": source_before,
                    "sourceChangedDuringRun": source_before != source_after,
                    "allCasesPass": verdict}, indent=2) + "\n",
        encoding="utf-8",
    )
    print(f"R02 client leaves: {len(cases)} recorded cases, verdict={'PASS' if verdict else 'FAIL'}")
    return 0 if verdict else 1


if __name__ == "__main__":
    raise SystemExit(main())

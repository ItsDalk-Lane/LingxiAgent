#!/usr/bin/env python3
"""Record R02 static-only checks without launching a service."""

import ast
import datetime
import hashlib
import json
import pathlib
import re
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[4]
OUT = pathlib.Path(__file__).resolve().parent / "static-final.json"


def command(argv):
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    done = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True)
    ended = datetime.datetime.now(datetime.timezone.utc).isoformat()
    return {
        "argv": argv,
        "startedUtc": started,
        "endedUtc": ended,
        "exitCode": done.returncode,
        "stdout": done.stdout,
        "stderr": done.stderr,
    }


def main():
    checks = []
    for path in sorted((ROOT / "scripts/rust-tauri").glob("r02_*.sh")):
        checks.append(command(["bash", "-n", str(path)]))
    for path in sorted((ROOT / "scripts/rust-tauri").glob("r02_*.py")):
        try:
            ast.parse(path.read_text(), filename=str(path))
            checks.append({"path": str(path.relative_to(ROOT)), "kind": "python-ast", "exitCode": 0})
        except Exception as error:
            checks.append({"path": str(path.relative_to(ROOT)), "kind": "python-ast", "exitCode": 1, "error": str(error)})
    for name in (
        "R02_HANDOFF.json",
        "R02_ACCEPTANCE_LEDGER.json",
        "R02_IMPLEMENTATION_MAP.json",
        "RISK_REGISTER.json",
    ):
        path = ROOT / "docs/rust-tauri/R02" / name
        try:
            json.loads(path.read_text())
            checks.append({"path": str(path.relative_to(ROOT)), "kind": "json-parse", "exitCode": 0})
        except Exception as error:
            checks.append({"path": str(path.relative_to(ROOT)), "kind": "json-parse", "exitCode": 1, "error": str(error)})
    path = ROOT / "docs/rust-tauri/ORCHESTRATOR_PROGRESS.json"
    try:
        json.loads(path.read_text())
        checks.append({"path": str(path.relative_to(ROOT)), "kind": "json-parse", "exitCode": 0})
    except Exception as error:
        checks.append({"path": str(path.relative_to(ROOT)), "kind": "json-parse", "exitCode": 1, "error": str(error)})
    checks.append(command(["git", "diff", "--check"]))
    report = ROOT / "docs/rust-tauri/R02/R02_FULL_SCOPE_HANDOFF_R18.md"
    links = []
    for target in re.findall(r"\]\(([^)#]+)(?:#[^)]+)?\)", report.read_text()):
        if "://" not in target:
            checked = (report.parent / target).resolve()
            links.append({"target": target, "exists": checked.exists()})
    checks.append({"kind": "report-local-links", "exitCode": 0 if all(item["exists"] for item in links) else 1, "links": links})
    result = {
        "recordedUtc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "head": command(["git", "rev-parse", "HEAD"])["stdout"].strip(),
        "runnerSha256": hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
        "checks": checks,
        "exitCode": 0 if all(item["exitCode"] == 0 for item in checks) else 1,
        "scope": "static syntax, JSON, whitespace and local links only",
    }
    OUT.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"checks": len(checks), "exitCode": result["exitCode"], "record": str(OUT)}, ensure_ascii=False))
    raise SystemExit(result["exitCode"])


if __name__ == "__main__":
    main()

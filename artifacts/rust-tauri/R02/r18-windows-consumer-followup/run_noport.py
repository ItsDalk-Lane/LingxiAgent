#!/usr/bin/env python3
"""保存本轮 Windows CLI 分发修正的无端口定向检查原件。"""

import datetime
import hashlib
import json
import os
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[4]
OUT = pathlib.Path(__file__).resolve().parent / os.environ.get("LINGXI_EVIDENCE_SET", "noport-current")
OUT.mkdir(parents=True, exist_ok=True)
FILES = [
    ".github/workflows/build.yml",
    "package.json",
    "cli/rust-service.ts",
    "scripts/build-server.mjs",
    "scripts/build-server-artifact.mjs",
    "scripts/build-standalone-server-artifact.mjs",
    "scripts/verify-standalone-server-artifact.mjs",
    "tests/build-server-artifact.test.ts",
    "tests/build-standalone-server-artifact.test.ts",
    "tests/desktop-rust-local-service.test.cjs",
]


def hashes():
    return {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in FILES}


CHECKS = [
    ("typecheck", ["npm", "run", "typecheck"]),
    ("cli-artifact-tests", ["./node_modules/.bin/vitest", "run", "tests/cli-rust-service.test.ts", "tests/build-server-artifact.test.ts", "tests/build-standalone-server-artifact.test.ts"]),
    ("desktop-tests", ["node", "--test", "tests/desktop-rust-local-service.test.cjs"]),
    ("build-server-syntax", ["node", "--check", "scripts/build-server.mjs"]),
    ("server-artifact-syntax", ["node", "--check", "scripts/build-server-artifact.mjs"]),
    ("standalone-builder-syntax", ["node", "--check", "scripts/build-standalone-server-artifact.mjs"]),
    ("standalone-verifier-syntax", ["node", "--check", "scripts/verify-standalone-server-artifact.mjs"]),
    ("cli-bundle", ["./node_modules/.bin/esbuild", "cli/entry.ts", "--bundle", "--platform=node", "--format=esm", "--target=node24", "--external:ws", f"--outfile={OUT / 'cli-bundle.mjs'}"]),
    ("diff-check", ["git", "diff", "--check"]),
]

before = hashes()
records = []
for name, command in CHECKS:
    start = datetime.datetime.now(datetime.timezone.utc).isoformat()
    try:
        result = subprocess.run(command, cwd=ROOT, capture_output=True, timeout=180)
        code, stdout, stderr = result.returncode, result.stdout, result.stderr
    except subprocess.TimeoutExpired as err:
        code, stdout, stderr = "TIMEOUT", err.stdout or b"", err.stderr or b""
    end = datetime.datetime.now(datetime.timezone.utc).isoformat()
    (OUT / f"{name}.stdout.log").write_bytes(stdout)
    (OUT / f"{name}.stderr.log").write_bytes(stderr)
    records.append({"name": name, "argv": command, "cwd": str(ROOT),
                    "started_utc": start, "ended_utc": end, "exit": code,
                    "stdout_sha256": hashlib.sha256(stdout).hexdigest(),
                    "stderr_sha256": hashlib.sha256(stderr).hexdigest()})
after = hashes()
summary = {"platform": os.uname().sysname, "checks": records,
           "source_before": before, "source_after": after,
           "source_unchanged": before == after}
(OUT / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
print(json.dumps({"source_unchanged": summary["source_unchanged"],
                  "checks": [(item["name"], item["exit"]) for item in records]}))

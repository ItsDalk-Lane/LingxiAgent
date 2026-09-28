#!/usr/bin/env python3
"""为 Windows 令牌消费端修复保存本机无端口检查的原始记录。"""

import datetime
import hashlib
import json
import os
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[4]
OUT = pathlib.Path(__file__).resolve().parent / os.environ.get("LINGXI_EVIDENCE_SET", "noport-final")
OUT.mkdir(parents=True, exist_ok=True)
FILES = [
    "rust/crates/lingxi-adapters/src/storage/windows_acl.rs",
    "rust/crates/lingxi-service/src/main.rs",
    "desktop/src/shared/rust-local-service.cjs",
    "desktop/main.cjs",
    "cli/rust-service.ts",
    "scripts/build-standalone-server-artifact.mjs",
    "scripts/verify-standalone-server-artifact.mjs",
    "tests/build-standalone-server-artifact.test.ts",
]


def hashes():
    return {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in FILES}


CHECKS = [
    ("rust-fmt", ["/Users/study_superior/.cargo/bin/rustup", "run", "1.98.1", "cargo", "fmt", "--all", "--", "--check"], ROOT / "rust"),
    ("rust-check", ["/Users/study_superior/.cargo/bin/rustup", "run", "1.98.1", "cargo", "check", "--workspace", "--all-targets", "--locked", "--offline"], ROOT / "rust"),
    ("rust-clippy", ["/Users/study_superior/.cargo/bin/rustup", "run", "1.98.1", "cargo", "clippy", "-p", "lingxi-service", "-p", "lingxi-adapters", "--all-targets", "--locked", "--offline", "--", "-D", "warnings"], ROOT / "rust"),
    ("typecheck", ["npm", "run", "typecheck"], ROOT),
    ("cli-and-standalone-tests", ["./node_modules/.bin/vitest", "run", "tests/cli-rust-service.test.ts", "tests/build-standalone-server-artifact.test.ts"], ROOT),
    ("desktop-tests", ["node", "--test", "tests/desktop-rust-local-service.test.cjs"], ROOT),
    ("cli-bundle", ["./node_modules/.bin/esbuild", "cli/entry.ts", "--bundle", "--platform=node", "--format=esm", "--target=node24", "--external:ws", "--outfile=/tmp/r02-win-consumer-cli-bundle.mjs"], ROOT),
    ("diff-check", ["git", "diff", "--check"], ROOT),
]

before = hashes()
records = []
for name, command, cwd in CHECKS:
    start = datetime.datetime.now(datetime.timezone.utc).isoformat()
    try:
        result = subprocess.run(command, cwd=cwd, env={**os.environ, "CARGO_NET_OFFLINE": "true"}, capture_output=True, timeout=180)
        code, stdout, stderr = result.returncode, result.stdout, result.stderr
    except subprocess.TimeoutExpired as err:
        code, stdout, stderr = "TIMEOUT", err.stdout or b"", err.stderr or b""
    end = datetime.datetime.now(datetime.timezone.utc).isoformat()
    (OUT / f"{name}.stdout.log").write_bytes(stdout)
    (OUT / f"{name}.stderr.log").write_bytes(stderr)
    records.append({"name": name, "argv": command, "cwd": str(cwd), "started_utc": start,
                    "ended_utc": end, "exit": code, "stdout_sha256": hashlib.sha256(stdout).hexdigest(),
                    "stderr_sha256": hashlib.sha256(stderr).hexdigest()})
after = hashes()
summary = {"platform": os.uname().sysname, "checks": records, "source_before": before,
           "source_after": after, "source_unchanged": before == after}
(OUT / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
print(json.dumps({"source_unchanged": summary["source_unchanged"],
                  "checks": [(item["name"], item["exit"]) for item in records]}))

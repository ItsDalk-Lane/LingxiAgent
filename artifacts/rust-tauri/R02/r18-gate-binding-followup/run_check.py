#!/usr/bin/env python3
"""记录本组不启动服务的实际检查。"""
import datetime
import hashlib
import json
import pathlib
import subprocess
import sys

root = pathlib.Path(__file__).resolve().parents[4]
out = pathlib.Path(__file__).resolve().parent
name = sys.argv[1]
command = sys.argv[2:]
files = [
    "rust/Cargo.lock",
    "rust/Cargo.toml",
    "rust-toolchain.toml",
    "rust/crates/xtask/Cargo.toml",
    "rust/crates/xtask/src/main.rs",
    "rust/crates/xtask/src/verify.rs",
    "rust/crates/xtask/src/candidate.rs",
    "rust/crates/xtask/src/candidate/tests.rs",
    "rust/crates/xtask/src/runner_identity.rs",
    "rust/crates/xtask/src/stage_maps/R02.json",
    "rust/crates/xtask/src/verify/runner_tests.rs",
]


def source_hashes():
    return {path: hashlib.sha256((root / path).read_bytes()).hexdigest() for path in files}


start = datetime.datetime.now(datetime.timezone.utc).isoformat()
before = source_hashes()
with (out / f"{name}.stdout.log").open("wb") as stdout, (out / f"{name}.stderr.log").open("wb") as stderr:
    result = subprocess.run(command, cwd=root, stdout=stdout, stderr=stderr, check=False)
end = datetime.datetime.now(datetime.timezone.utc).isoformat()
record = {
    "command": command,
    "cwd": str(root),
    "startedAtUtc": start,
    "finishedAtUtc": end,
    "exitCode": result.returncode,
    "sourceSha256Before": before,
    "sourceSha256After": source_hashes(),
    "stdout": f"{name}.stdout.log",
    "stderr": f"{name}.stderr.log",
}
(out / f"{name}.json").write_text(json.dumps(record, ensure_ascii=False, indent=2) + "\n")
print(json.dumps({"name": name, "exitCode": result.returncode, "record": str(out / f"{name}.json")}))
sys.exit(result.returncode)

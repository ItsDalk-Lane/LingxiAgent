#!/usr/bin/env python3
"""只运行不会启动 R02 真服务的当前候选检查，并保存每次原始输出。"""
import datetime as dt
import hashlib
import json
import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[4]
OUT = pathlib.Path(__file__).resolve().parent / "final-noport-1"
RUST = ROOT / "rust"
RUSTUP = "/Users/study_superior/.cargo/bin/rustup"
SOURCE_PREFIXES = ("rust/", "desktop/", "cli/", "scripts/", "build/", "tests/")
SOURCE_FILES = {"package.json", "package-lock.json", "rust-toolchain.toml"}


def utc():
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec="microseconds")


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def manifest(label):
    paths = subprocess.check_output(
        ["git", "ls-files", "-co", "--exclude-standard", "-z"], cwd=ROOT
    ).decode().split("\0")
    rows = []
    for name in sorted(filter(None, paths)):
        if name.startswith(SOURCE_PREFIXES) or name in SOURCE_FILES:
            file = ROOT / name
            if file.is_file():
                rows.append(f"{sha(file)}  {name}\n")
    target = OUT / f"source-{label}-sha256.txt"
    target.write_text("".join(rows), encoding="utf-8")
    return {"path": str(target.relative_to(ROOT)), "count": len(rows), "sha256": sha(target)}


def run(name, argv, cwd, env):
    stdout = OUT / f"{name}.stdout.log"
    stderr = OUT / f"{name}.stderr.log"
    started = utc()
    print(f"START {name} {started}", flush=True)
    with stdout.open("wb") as out, stderr.open("wb") as err:
        result = subprocess.run(argv, cwd=cwd, env=env, stdout=out, stderr=err, check=False)
    ended = utc()
    record = {
        "name": name, "command": argv, "cwd": str(cwd), "startedUtc": started,
        "endedUtc": ended, "exitCode": result.returncode,
        "stdout": str(stdout.relative_to(ROOT)), "stdoutSha256": sha(stdout),
        "stderr": str(stderr.relative_to(ROOT)), "stderrSha256": sha(stderr),
    }
    (OUT / f"{name}.json").write_text(json.dumps(record, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"END {name} exit={result.returncode} {ended}", flush=True)
    return record


def main():
    if OUT.exists():
        raise SystemExit(f"refuse to overwrite evidence: {OUT}")
    OUT.mkdir(parents=True)
    env = os.environ.copy()
    env["CARGO_NET_OFFLINE"] = "true"
    env["RUSTUP_TOOLCHAIN"] = "1.98.1"
    env["CARGO_TARGET_DIR"] = "/private/tmp/lingxi-r02-r18-final-target"
    initial = manifest("before")
    checks = [
        ("rust-version", [RUSTUP, "run", "1.98.1", "rustc", "--version"], ROOT),
        ("cargo-fmt", [RUSTUP, "run", "1.98.1", "cargo", "fmt", "--all", "--", "--check"], RUST),
        ("cargo-check", [RUSTUP, "run", "1.98.1", "cargo", "check", "--workspace", "--all-targets", "--locked", "--offline"], RUST),
        ("cargo-clippy", [RUSTUP, "run", "1.98.1", "cargo", "clippy", "--workspace", "--all-targets", "--locked", "--offline", "--", "-D", "warnings"], RUST),
        ("xtask-unit", [RUSTUP, "run", "1.98.1", "cargo", "test", "-p", "xtask", "--locked", "--offline"], RUST),
        ("management-compile", [RUSTUP, "run", "1.98.1", "cargo", "test", "-p", "lingxi-service", "--test", "r00_management_leaves", "--no-run", "--locked", "--offline"], RUST),
        ("static-web-compile", [RUSTUP, "run", "1.98.1", "cargo", "test", "-p", "lingxi-service", "--test", "r00_static_web_leaves", "--no-run", "--locked", "--offline"], RUST),
        ("npm-typecheck", ["npm", "run", "typecheck"], ROOT),
        ("desktop-rust-helper", ["node", "--test", "tests/desktop-rust-local-service.test.cjs"], ROOT),
        ("targeted-vitest", ["node_modules/.bin/vitest", "run",
            "tests/cli-rust-service.test.ts", "tests/cli-chat-runtime.test.ts", "tests/cli-args.test.ts", "tests/cli-chat.test.ts",
            "desktop/src/react/__tests__/mobile/MobileApp.test.tsx",
            "desktop/src/react/settings/__tests__/SettingsContent.test.tsx",
            "desktop/src/react/__tests__/services/R02RustWebSocket.test.ts",
            "desktop/src/react/__tests__/components/R02StatusBar.test.tsx",
            "desktop/src/react/settings/__tests__/R02AccessRoute.test.tsx",
            "desktop/src/react/settings/__tests__/R02AccessCredentialLeaf.test.tsx",
            "desktop/src/react/settings/__tests__/R02SharingLeaf.test.tsx"], ROOT),
        ("build-client", ["npm", "run", "build:client"], ROOT),
        ("build-server", ["npm", "run", "build:server"], ROOT),
        ("build-rust-service", ["npm", "run", "build:rust-service"], ROOT),
        ("verify-rust-package", ["npm", "run", "verify:rust-service-package"], ROOT),
    ]
    records = []
    for name, argv, cwd in checks:
        records.append(run(name, argv, cwd, env))
    final = manifest("after")
    summary = {
        "candidateBefore": initial, "candidateAfter": final,
        "sourceUnchanged": initial["sha256"] == final["sha256"],
        "platform": sys.platform, "node": subprocess.getoutput("node --version"),
        "npm": subprocess.getoutput("npm --version"),
        "checks": [{"name": r["name"], "exitCode": r["exitCode"], "record": f"{r['name']}.json"} for r in records],
        "scope": "no true service bind, no full npm test, no verify-stage R02",
    }
    (OUT / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"sourceUnchanged": summary["sourceUnchanged"], "checks": summary["checks"]}), flush=True)


if __name__ == "__main__":
    main()

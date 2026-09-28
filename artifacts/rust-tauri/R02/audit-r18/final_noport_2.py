#!/usr/bin/env python3
"""Current R18 no-port checks after Gate and Windows consumer repairs."""

import datetime as dt
import hashlib
import json
import os
import pathlib
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[4]
OUT = pathlib.Path(__file__).resolve().parent / "final-noport-2"
RUST = ROOT / "rust"
RUSTUP = "/Users/study_superior/.cargo/bin/rustup"
SOURCE_PREFIXES = ("rust/", "desktop/", "cli/", "scripts/", "build/", "tests/")
SOURCE_FILES = {"package.json", "package-lock.json", "rust-toolchain.toml"}


def utc():
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec="microseconds")


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def manifest(label):
    raw = subprocess.check_output(["git", "ls-files", "-co", "--exclude-standard", "-z"], cwd=ROOT)
    rows = []
    for value in sorted(filter(None, raw.split(b"\0"))):
        name = value.decode("utf-8", "surrogateescape")
        if name.startswith(SOURCE_PREFIXES) or name in SOURCE_FILES:
            path = ROOT / name
            if path.is_file():
                rows.append(f"{sha(path)}  {name}\n")
    target = OUT / f"source-{label}-sha256.txt"
    target.write_text("".join(rows))
    return {"path": str(target.relative_to(ROOT)), "count": len(rows), "sha256": sha(target)}


def run(name, argv, cwd, env):
    stdout = OUT / f"{name}.stdout.log"
    stderr = OUT / f"{name}.stderr.log"
    started = utc()
    print(f"START {name} {started}", flush=True)
    with stdout.open("wb") as out, stderr.open("wb") as err:
        done = subprocess.run([str(item) for item in argv], cwd=cwd, env=env, stdout=out, stderr=err)
    ended = utc()
    record = {
        "name": name, "argv": [str(item) for item in argv], "cwd": str(cwd),
        "startedUtc": started, "endedUtc": ended, "exitCode": done.returncode,
        "stdout": str(stdout.relative_to(ROOT)), "stdoutSha256": sha(stdout),
        "stderr": str(stderr.relative_to(ROOT)), "stderrSha256": sha(stderr),
    }
    (OUT / f"{name}.json").write_text(json.dumps(record, ensure_ascii=False, indent=2) + "\n")
    print(f"END {name} exit={done.returncode} {ended}", flush=True)
    return record


def main():
    if OUT.exists():
        raise SystemExit(f"refuse to overwrite prior evidence: {OUT}")
    OUT.mkdir(parents=True)
    env = os.environ.copy()
    env.update({
        "CARGO_NET_OFFLINE": "true",
        "RUSTUP_TOOLCHAIN": "1.98.1",
        "CARGO_TARGET_DIR": "/private/tmp/lingxi-r02-r18-final-target",
    })
    before = manifest("before")
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
            "tests/cli-rust-service.test.ts", "tests/build-standalone-server-artifact.test.ts",
            "tests/cli-chat-runtime.test.ts", "tests/cli-args.test.ts", "tests/cli-chat.test.ts",
            "desktop/src/react/__tests__/mobile/MobileApp.test.tsx",
            "desktop/src/react/settings/__tests__/SettingsContent.test.tsx",
            "desktop/src/react/__tests__/services/R02RustWebSocket.test.ts",
            "desktop/src/react/__tests__/components/R02StatusBar.test.tsx",
            "desktop/src/react/settings/__tests__/R02AccessRoute.test.tsx",
            "desktop/src/react/settings/__tests__/R02AccessCredentialLeaf.test.tsx",
            "desktop/src/react/settings/__tests__/R02SharingLeaf.test.tsx"], ROOT),
        ("build-client", ["npm", "run", "build:client"], ROOT),
    ]
    records = [run(name, argv, cwd, env) for name, argv, cwd in checks]
    with tempfile.TemporaryDirectory(prefix="lingxi-r02-r18-local-sign-") as temp:
        private = pathlib.Path(temp) / "private.pem"
        public = pathlib.Path(temp) / "public-keyset.json"
        generated = run("local-keygen", ["node", "scripts/artifact-keygen.mjs", "--out", private], ROOT, env)
        records.append(generated)
        if generated["exitCode"] == 0:
            key = json.loads((OUT / "local-keygen.stdout.log").read_text())
            public.write_text(json.dumps([key]) + "\n")
            signed_env = dict(env, LINGXI_SIGN_KEY=str(private), LINGXI_SIGN_KEYSET=str(public))
            server = run("build-server-local-sign", ["npm", "run", "build:server"], ROOT, signed_env)
            records.append(server)
            rust = run("build-rust-service", ["npm", "run", "build:rust-service"], ROOT, env)
            records.append(rust)
            verify = run("verify-rust-package", ["npm", "run", "verify:rust-service-package"], ROOT, env)
            records.append(verify)
            if server["exitCode"] == rust["exitCode"] == verify["exitCode"] == 0:
                shell_env = dict(env, LINGXI_SIGN_KEYSET=str(public),
                    CSC_IDENTITY_AUTO_DISCOVERY="false", SKIP_NOTARIZE="true")
                records.append(run("build-shell-local-structure", ["npm", "run", "build:shell"], ROOT, shell_env))
    after = manifest("after")
    summary = {
        "startedUtc": records[0]["startedUtc"], "endedUtc": utc(),
        "sourceBefore": before, "sourceAfter": after, "sourceUnchanged": before["sha256"] == after["sha256"],
        "platform": sys.platform, "node": subprocess.getoutput("node --version"),
        "npm": subprocess.getoutput("npm --version"),
        "checks": [{"name": item["name"], "exitCode": item["exitCode"], "record": f"{item['name']}.json"} for item in records],
        "temporaryPrivateKeyRemoved": not private.exists(),
        "scope": "no real service bind, no full npm test, no A16, no verify-stage R02; local test signing/public keyset and skipped notarization only",
    }
    (OUT / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"sourceUnchanged": summary["sourceUnchanged"], "checks": summary["checks"]}), flush=True)


if __name__ == "__main__":
    main()

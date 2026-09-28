#!/usr/bin/env python3
"""R02 CLI 到真实 Rust 服务的补充场景证据生产者。"""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from urllib.error import URLError
from urllib.request import ProxyHandler, Request, build_opener

from r02_owned_process_group import signal_owned_group


REPO = Path(__file__).resolve().parents[2]
CLI = REPO / "cli/entry.ts"
HTTP = build_opener(ProxyHandler({}))
SOURCE_DIRS = (
    "rust/crates/lingxi-service/src",
    "rust/crates/lingxi-adapters/src",
    "rust/crates/lingxi-kernel/src",
    "rust/crates/lingxi-protocol/src",
)
SOURCE_FILES = (
    "rust/Cargo.lock", "rust/Cargo.toml", "rust-toolchain.toml", "package.json",
    "package-lock.json", "shared/server-info-probe.cjs",
    "scripts/rust-tauri/r02_cli_rust_matrix.py",
    "scripts/rust-tauri/r02_owned_process_group.py",
)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def source_manifest() -> dict[str, str]:
    paths = [REPO / relative for relative in SOURCE_FILES]
    paths.extend((REPO / "rust/crates").glob("*/Cargo.toml"))
    for dirname in SOURCE_DIRS:
        paths.extend(path for path in (REPO / dirname).rglob("*.rs") if path.is_file())
    paths.extend((REPO / "cli").rglob("*.ts"))
    paths.extend((REPO / "shared/artifact-core").rglob("*.cjs"))
    paths.extend((REPO / "tests").glob("cli-*.test.ts"))
    return {str(path.relative_to(REPO)): sha256(path) for path in sorted(set(paths))}


def snapshot(root: Path) -> dict[str, str]:
    if not root.exists():
        return {}
    result: dict[str, str] = {}
    for path in sorted(root.rglob("*")):
        key = str(path.relative_to(root))
        if path.is_symlink():
            result[key] = "SYMLINK:" + os.readlink(path)
        elif path.is_file():
            result[key] = "SHA256:" + sha256(path)
        elif path.is_dir():
            result[key] = "DIR"
        else:
            result[key] = "OTHER"
    return result


class Matrix:
    def __init__(self, evidence: Path):
        self.evidence = evidence
        self.cases: list[dict] = []
        self.commands: list[dict] = []
        self.source_start = source_manifest()

    def run(self, name: str, argv: list[str], *, env: dict[str, str] | None = None,
            timeout: int = 20) -> subprocess.CompletedProcess[str]:
        start = utc_now()
        timed_out = False
        cleanup_error = None
        process = None
        try:
            process = subprocess.Popen(argv, cwd=REPO, env=env, stdout=subprocess.PIPE,
                                       stderr=subprocess.PIPE, text=True, start_new_session=True)
            stdout, stderr = process.communicate(timeout=timeout)
            exit_code = process.returncode
        except subprocess.TimeoutExpired:
            timed_out = True
            # 会话领袖仍属本次 Popen 才能发信号；否则保留失败证据。
            try:
                if not signal_owned_group(process, signal.SIGTERM):
                    cleanup_error = "CLI group leader ownership could not be proven before TERM"
            except OSError as exc:
                cleanup_error = f"CLI group TERM failed: {exc}"
            try:
                stdout, stderr = process.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                try:
                    if not signal_owned_group(process, signal.SIGKILL):
                        detail = "CLI group leader ownership could not be proven before KILL"
                        cleanup_error = f"{cleanup_error}; {detail}" if cleanup_error else detail
                except OSError as exc:
                    detail = f"CLI group KILL failed: {exc}"
                    cleanup_error = f"{cleanup_error}; {detail}" if cleanup_error else detail
                stdout, stderr = process.communicate(timeout=5)
            exit_code = 124
            stderr += f"\nCLI command timed out after {timeout}s\n"
        except BaseException:
            if process is not None and process.poll() is None:
                try:
                    signal_owned_group(process, signal.SIGTERM)
                    process.communicate(timeout=5)
                except (OSError, subprocess.TimeoutExpired):
                    if process.poll() is None:
                        signal_owned_group(process, signal.SIGKILL)
                        process.communicate(timeout=5)
            raise
        end = utc_now()
        (self.evidence / f"{name}.stdout.log").write_text(stdout, encoding="utf-8")
        (self.evidence / f"{name}.stderr.log").write_text(stderr, encoding="utf-8")
        record = {"name": name, "argv": argv, "startedUtc": start, "endedUtc": end,
                  "exit": exit_code, "timedOut": timed_out, "cleanupError": cleanup_error,
                  "stdout": f"{name}.stdout.log", "stderr": f"{name}.stderr.log",
                  "environment": {key: env[key] for key in (
                      "LINGXI_HOME", "LINGXI_SERVICE_BIN", "CARGO_TARGET_DIR", "CARGO_NET_OFFLINE",
                      "HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME") if env and key in env}}
        self.commands.append(record)
        return subprocess.CompletedProcess(argv, exit_code, stdout, stderr)

    def case(self, name: str, passed: bool, observed: dict) -> None:
        self.cases.append({"case": name, "expect": 1, "actual": int(passed),
                           "ok": passed, "observed": observed})

    def save(self, *, binary: Path | None = None, homes: dict | None = None) -> None:
        manifest = source_manifest()
        cleanup_failures = [item for item in self.commands if item.get("cleanupError")]
        if cleanup_failures:
            self.case("cli-rust-producer-cleanup", False, {"commands": cleanup_failures})
        self.case("cli-rust-candidate-unchanged-during-probe", manifest == self.source_start,
                  {"sourceFiles": len(manifest), "changedPaths": sorted(
                      key for key in set(manifest) | set(self.source_start)
                      if manifest.get(key) != self.source_start.get(key))})
        document = {
            "schema": "lingxi.leaf-case-results.v1",
            "producer": "r02_cli_rust_matrix.py",
            "producedUtc": utc_now(),
            "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
            "candidateSources": self.source_start,
            "candidateSourcesSha256": hashlib.sha256(json.dumps(self.source_start, sort_keys=True).encode()).hexdigest(),
            "binaryPath": str(binary) if binary else None,
            "binarySha256": sha256(binary) if binary and binary.is_file() else None,
            "commands": self.commands,
            "homes": homes or {},
            "cases": self.cases,
        }
        (self.evidence / "cli-rust-cases.json").write_text(
            json.dumps(document, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def cli_args(command: str, *parts: str) -> list[str]:
    return ["node", str(CLI), command, "--runtime", "rust", *parts]


def health_ok(addr: str) -> bool:
    try:
        with HTTP.open(f"http://{addr}/lingxi/v1/health", timeout=2) as response:
            body = json.load(response)
            return response.status == 200 and body.get("serverKind") == "lingxi-service"
    except (OSError, URLError, ValueError):
        return False


def table_counts(db: Path) -> dict[str, int]:
    with sqlite3.connect(f"file:{db}?mode=ro", uri=True, timeout=2) as connection:
        return {name: connection.execute(f"SELECT COUNT(*) FROM {name}").fetchone()[0]
                for name in ("sessions", "runs", "key_events")}


def session_titles(db: Path) -> set[str]:
    with sqlite3.connect(f"file:{db}?mode=ro", uri=True, timeout=2) as connection:
        return {row[0] for row in connection.execute("SELECT title FROM sessions")}


def api_sessions(addr: str, token_path: Path) -> list[dict]:
    # 短令牌只在内存中用于本机认证请求；原始值不落证据文件。
    token = json.loads(token_path.read_text(encoding="utf-8"))["token"]
    request = Request(f"http://{addr}/lingxi/v1/sessions",
                      headers={"Authorization": f"Bearer {token}"})
    with HTTP.open(request, timeout=3) as response:
        body = json.load(response)
    if not isinstance(body, dict) or not isinstance(body.get("sessions"), list):
        raise RuntimeError("Rust sessions API returned an invalid list")
    return body["sessions"]


def process_field(pid: int, field: str) -> str:
    result = subprocess.run(["ps", "-o", f"{field}=", "-p", str(pid)],
                            capture_output=True, text=True, check=False)
    return result.stdout.strip() if result.returncode == 0 else ""


def process_state(pid: int, birth: str) -> str:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return "exited"
    except PermissionError:
        return "unobservable"
    now = process_field(pid, "lstart")
    if not now:
        return "unobservable"
    return "owned-live" if now == birth else "recycled-pid"


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: r02_cli_rust_matrix.py NEW_EVIDENCE_DIR", file=sys.stderr)
        return 2
    evidence = Path(sys.argv[1]).absolute()
    if evidence.exists() or evidence.is_symlink():
        print(f"FAIL: evidence directory already exists or is a symlink: {evidence}", file=sys.stderr)
        return 1
    evidence.mkdir(parents=True, exist_ok=False)
    matrix = Matrix(evidence)
    interrupted = False

    def stop_requested(signum: int, _frame) -> None:
        nonlocal interrupted
        interrupted = True
        raise InterruptedError(f"producer received signal {signum}")

    signal.signal(signal.SIGTERM, stop_requested)
    signal.signal(signal.SIGINT, stop_requested)
    homes = {name: evidence / name for name in (
        "home-main", "home-bad-bind", "home-newer", "home-fallback", "home-beta", "home-downgrade")}
    for path in homes.values():
        path.mkdir(mode=0o700)
    home_notes: dict = {}
    binary: Path | None = None
    service: subprocess.Popen | None = None
    serve_stdout = None
    serve_stderr = None
    addr = ""
    rust_pid = 0
    rust_birth = ""
    rust_parent = ""
    instance_transport = ""
    clean_exit = False
    try:
        toolchain_text = (REPO / "rust-toolchain.toml").read_text(encoding="utf-8")
        match = re.search(r'^channel\s*=\s*"([^"]+)"', toolchain_text, re.MULTILINE)
        if not match:
            raise RuntimeError("cannot determine locked Rust toolchain")
        # 独立全新 target：已有二进制不能冒充当前源码的本轮构建。
        target = Path(tempfile.mkdtemp(prefix="r02-cli-rust-target-"))
        rustup = shutil.which("rustup") or str(Path.home() / ".cargo/bin/rustup")
        if not Path(rustup).is_file():
            raise RuntimeError("locked Rust toolchain launcher rustup is unavailable")
        env = os.environ.copy()
        for key in ("all_proxy", "ALL_PROXY", "http_proxy", "HTTP_PROXY", "https_proxy", "HTTPS_PROXY"):
            env.pop(key, None)
        env["CARGO_NET_OFFLINE"] = "true"
        env["CARGO_TARGET_DIR"] = str(target)
        build = matrix.run("build-current-service", [rustup, "run", match.group(1), "cargo",
            "build", "--manifest-path", "rust/Cargo.toml", "--locked", "--offline", "-p", "lingxi-service"],
            env=env, timeout=1200)
        binary = target / "debug/lingxi-service"
        matrix.case("cli-rust-current-binary-build", build.returncode == 0 and binary.is_file(),
                    {"exit": build.returncode, "binary": str(binary)})
        if build.returncode != 0 or not binary.is_file():
            binary = None
            raise RuntimeError("current Rust service build failed")
        if source_manifest() != matrix.source_start:
            binary = None
            raise RuntimeError("candidate source changed during Rust service build")
        cli_env = os.environ.copy()
        cli_env["LINGXI_SERVICE_BIN"] = str(binary)
        cli_env["LINGXI_HOME"] = str(homes["home-main"])
        argv = cli_args("serve", "--channel", "stable", "--", "--home", str(homes["home-main"]), "--bind", "127.0.0.1:0")
        start = utc_now()
        serve_stdout = (evidence / "serve.stdout.log").open("w", encoding="utf-8")
        serve_stderr = (evidence / "serve.stderr.log").open("w", encoding="utf-8")
        service = subprocess.Popen(argv, cwd=REPO, env=cli_env, stdout=serve_stdout,
                                   stderr=serve_stderr, start_new_session=True, text=True)
        record = {"name": "serve-foreground", "argv": argv, "startedUtc": start,
                  "pid": service.pid, "stdout": "serve.stdout.log", "stderr": "serve.stderr.log",
                  "environment": {"LINGXI_HOME": cli_env["LINGXI_HOME"],
                                  "LINGXI_SERVICE_BIN": cli_env["LINGXI_SERVICE_BIN"]}}
        matrix.commands.append(record)
        instance_path = homes["home-main"] / "lingxi-service/instance.json"
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline and service.poll() is None:
            if instance_path.is_file():
                try:
                    instance = json.loads(instance_path.read_text(encoding="utf-8"))
                    addr = instance.get("bindAddr", "")
                    instance_transport = instance.get("transport", "")
                    raw_pid = instance.get("pid", 0)
                    rust_pid = raw_pid if isinstance(raw_pid, int) and raw_pid > 0 else 0
                except (OSError, ValueError):
                    addr = ""
                if addr and health_ok(addr):
                    break
            time.sleep(0.1)
        ready = bool(addr and health_ok(addr) and service.poll() is None)
        record["readyUtc"] = utc_now()
        record["bindAddr"] = addr
        if isinstance(rust_pid, int) and rust_pid > 0:
            rust_birth = process_field(rust_pid, "lstart")
            rust_parent = process_field(rust_pid, "ppid")
        matrix.case("cli-rust-serve-ready", ready and bool(rust_birth)
                    and rust_parent == str(service.pid) and instance_transport == "http",
                    {"cliPid": service.pid, "rustPid": rust_pid, "rustBirth": rust_birth,
                     "rustParent": rust_parent, "bindAddr": addr,
                     "instanceTransport": instance_transport, "instanceFile": str(instance_path)})
        matrix.case("cli-rust-channel-stable-selected", ready and argv[5:7] == ["--channel", "stable"]
                    and instance_path.is_file() and instance_transport == "http",
                    {"cliArgv": argv, "ready": ready, "instanceTransport": instance_transport})
        if not ready:
            raise RuntimeError("CLI did not start a healthy foreground Rust service")
        db = homes["home-main"] / "lingxi-service/data/runs.db"
        token_path = homes["home-main"] / "lingxi-service/local-token.json"
        before = table_counts(db)
        with HTTP.open(f"http://{addr}/lingxi/v1/health", timeout=2) as response:
            health = json.load(response)
        health_version = health.get("serverVersion")
        status = matrix.run("status-owner", cli_args("status"), env=cli_env)
        matrix.case("cli-rust-status-real-health-identity", status.returncode == 1
                    and isinstance(health_version, str) and bool(health_version)
                    and f"http://{addr}" in status.stdout and health_version in status.stdout
                    and "loopback_token" in status.stdout
                    and "LingxiAgent Rust service" in status.stdout
                    and "Agent and model are not available yet" in status.stderr,
                    {"exit": status.returncode, "healthServerVersion": health_version,
                     "dbCounts": before})
        owner = matrix.run("sessions-owner", cli_args("sessions"), env=cli_env)
        owner_api = api_sessions(addr, token_path)
        (evidence / "sessions-owner-api.json").write_text(
            json.dumps({"sessions": owner_api}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        matrix.case("cli-rust-sessions-owner", owner.returncode == 0
                    and "Synthetic session alpha" in owner.stdout
                    and "Synthetic session beta" in owner.stdout
                    and {row["sessionId"] for row in owner_api} == {"sess_local_alpha", "sess_local_beta"},
                    {"exit": owner.returncode, "apiSessionIds": [row["sessionId"] for row in owner_api],
                     "dbCounts": before})
        missing = matrix.run("continue-missing", cli_args("continue", "does-not-exist"), env=cli_env)
        after_missing = table_counts(db)
        matrix.case("cli-rust-continue-missing-no-create", missing.returncode not in (0, 124)
                    and "Session not found" in missing.stderr and before == after_missing,
                    {"exit": missing.returncode, "before": before, "after": after_missing})
        existing = matrix.run("continue-existing", cli_args("continue", "sess_local_alpha"), env=cli_env)
        after_existing = table_counts(db)
        matrix.case("cli-rust-continue-existing-no-fake-reply", existing.returncode not in (0, 124)
                    and "model/tool reply streaming are unavailable" in existing.stderr
                    and before == after_existing,
                    {"exit": existing.returncode, "before": before, "after": after_existing})
        chat = matrix.run("chat-no-kernel", cli_args("chat"), env=cli_env)
        after_chat = table_counts(db)
        matrix.case("cli-rust-chat-no-fake-run", chat.returncode not in (0, 124)
                    and "model/tool reply streaming are unavailable" in chat.stderr
                    and before == after_chat,
                    {"exit": chat.returncode, "before": before, "after": after_chat})
        bad_env = {**cli_env, "LINGXI_HOME": str(homes["home-main"])}
        unauthorized = matrix.run("sessions-unauthorized", cli_args("sessions", "--url", f"http://{addr}",
            "--token", "intentionally-invalid-token"), env=bad_env)
        matrix.case("cli-rust-sessions-unauthorized", unauthorized.returncode not in (0, 124)
                    and "HTTP 401" in unauthorized.stderr
                    and "Synthetic session" not in unauthorized.stdout,
                    {"exit": unauthorized.returncode})
        identity_failure = matrix.run("status-identity-failure", cli_args("status", "--url", f"http://{addr}",
            "--token", "intentionally-invalid-token"), env=bad_env)
        matrix.case("cli-rust-status-no-fake-auth", identity_failure.returncode == 1
                    and "unavailable (identity check failed)" in identity_failure.stdout
                    and "loopback_token" not in identity_failure.stdout
                    and "Rust status is incomplete" in identity_failure.stderr,
                    {"exit": identity_failure.returncode})
        # 只改本轮合成库，分别制造 >20 与空列表，验证 CLI 真服务响应的两端。
        with sqlite3.connect(db, timeout=5) as connection:
            latest_existing = connection.execute(
                "SELECT COALESCE(MAX(created_at_unix_ms), 0) FROM sessions"
            ).fetchone()[0]
            seeded = [
                (f"sess_cli_{index:02d}", "lingxi", "user_local", f"CLI fixture {index:02d}",
                 latest_existing + index + 1)
                for index in range(25)
            ]
            connection.executemany("INSERT INTO sessions VALUES (?, ?, ?, ?, ?)", seeded)
        (evidence / "sessions-fixture-ledger.json").write_text(json.dumps({
            "source": "direct SQLite insert into this run's isolated synthetic home",
            "columns": ["session_id", "agent_id", "owner_user_id", "title", "created_at_unix_ms"],
            "latestExistingCreatedAtUnixMs": latest_existing,
            "rows": seeded,
            "note": "This seeds server state to test CLI listing; CLI did not create these sessions.",
        }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        many = matrix.run("sessions-limit", cli_args("sessions"), env=cli_env)
        rows = re.findall(r"^\s*\d+\.\s+(.+?) · ", many.stdout, re.MULTILINE)
        many_api = api_sessions(addr, token_path)
        (evidence / "sessions-limit-api.json").write_text(
            json.dumps({"sessions": many_api}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        known_titles = session_titles(db)
        expected_newest = [f"sess_cli_{index:02d}" for index in range(24, 4, -1)]
        matrix.case("cli-rust-sessions-limit20", many.returncode == 0 and len(rows) == 20
                    and rows == [row["title"] for row in many_api[:20]]
                    and [row["sessionId"] for row in many_api[:20]] == expected_newest
                    and set(rows).issubset(known_titles) and len(known_titles) > 20,
                    {"exit": many.returncode, "printedRows": rows,
                     "apiSessionIds": [row["sessionId"] for row in many_api],
                     "expectedNewestSessionIds": expected_newest,
                     "storedTitles": len(known_titles), "dbCounts": table_counts(db)})
        with sqlite3.connect(db, timeout=5) as connection:
            connection.execute("DELETE FROM sessions")
        empty = matrix.run("sessions-empty", cli_args("sessions"), env=cli_env)
        empty_api = api_sessions(addr, token_path)
        (evidence / "sessions-empty-api.json").write_text(
            json.dumps({"sessions": empty_api}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        matrix.case("cli-rust-sessions-empty", empty.returncode == 0
                    and "No sessions yet." in empty.stdout and empty_api == [],
                    {"exit": empty.returncode, "apiSessions": empty_api, "dbCounts": table_counts(db)})
    except Exception as error:
        matrix.case("cli-rust-producer-internal", False, {"error": str(error)})
    finally:
        if service is not None:
            try:
                if service.poll() is None:
                    try:
                        service.send_signal(signal.SIGTERM)
                    except ProcessLookupError:
                        pass
                service.wait(timeout=12)
                clean_exit = service.returncode == 0
            except subprocess.TimeoutExpired:
                # 进程组由本生产者创建；仅在前台 CLI 仍未退出时有界强杀。
                if service.poll() is None:
                    try:
                        if not signal_owned_group(service, signal.SIGKILL):
                            matrix.case("cli-rust-serve-cleanup-ownership", False,
                                        {"reason": "serve group leader ownership unproven before KILL"})
                    except OSError as exc:
                        matrix.case("cli-rust-serve-cleanup-ownership", False,
                                    {"reason": f"serve group KILL failed: {exc}"})
                try:
                    service.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    pass
                clean_exit = False
            record["endedUtc"] = utc_now()
            child_state = process_state(rust_pid, rust_birth) if rust_pid > 0 and rust_birth else "unobservable"
            observed_child_state = child_state
            if child_state == "owned-live":
                # PID 与出生时间都与本轮记录一致，才允许回收残留的本轮 Rust 子进程。
                try:
                    os.kill(rust_pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline and process_state(rust_pid, rust_birth) == "owned-live":
                    time.sleep(0.1)
                if process_state(rust_pid, rust_birth) == "owned-live":
                    try:
                        os.kill(rust_pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                child_state = process_state(rust_pid, rust_birth)
            if serve_stdout:
                serve_stdout.close()
            if serve_stderr:
                serve_stderr.close()
            output = (evidence / "serve.stdout.log").read_text(encoding="utf-8")
            matrix.case("cli-rust-serve-sigterm-clean", clean_exit and "LINGXI_SERVICE_READY " in output
                        and bool(addr) and not health_ok(addr) and observed_child_state == "exited",
                        {"exit": service.returncode, "bindAddr": addr, "rustPid": rust_pid,
                         "rustChildStateBeforeCleanup": observed_child_state,
                         "rustChildStateAfterCleanup": child_state,
                         "portClosed": not health_ok(addr) if addr else None})
        if binary is not None and not interrupted:
            cli_env = os.environ.copy()
            cli_env["LINGXI_SERVICE_BIN"] = str(binary)
            bad_bind_before = snapshot(homes["home-bad-bind"])
            bad_bind = matrix.run("serve-invalid-bind", cli_args("serve", "--", "--home", str(homes["home-bad-bind"]),
                "--bind", "not-an-address"), env=cli_env)
            bad_bind_after = snapshot(homes["home-bad-bind"])
            matrix.case("cli-rust-serve-startup-error", bad_bind.returncode not in (0, 124)
                        and "invalid --bind" in bad_bind.stderr
                        and "LINGXI_SERVICE_READY " not in bad_bind.stdout
                        and bad_bind_before == bad_bind_after,
                        {"exit": bad_bind.returncode, "before": bad_bind_before,
                         "after": bad_bind_after})
            beta_before = snapshot(homes["home-beta"])
            beta = matrix.run("serve-beta-unavailable", cli_args("serve", "--channel", "beta", "--",
                "--home", str(homes["home-beta"]), "--bind", "127.0.0.1:0"), env=cli_env)
            matrix.case("cli-rust-serve-beta-no-silent-stable", beta.returncode not in (0, 124)
                        and "No activated beta frontend" in beta.stderr
                        and "LINGXI_SERVICE_READY " not in beta.stdout
                        and snapshot(homes["home-beta"]) == beta_before,
                        {"exit": beta.returncode, "before": beta_before,
                         "after": snapshot(homes["home-beta"])})
            downgrade_before = snapshot(homes["home-downgrade"])
            downgrade = matrix.run("serve-downgrade-refused", cli_args("serve", "--allow-data-downgrade", "--",
                "--home", str(homes["home-downgrade"]), "--bind", "127.0.0.1:0"), env=cli_env)
            matrix.case("cli-rust-serve-downgrade-refused", downgrade.returncode not in (0, 124)
                        and "does not support --allow-data-downgrade" in downgrade.stderr
                        and "LINGXI_SERVICE_READY " not in downgrade.stdout
                        and snapshot(homes["home-downgrade"]) == downgrade_before,
                        {"exit": downgrade.returncode, "before": downgrade_before,
                         "after": snapshot(homes["home-downgrade"])})
            newer = homes["home-newer"]
            fallback = homes["home-fallback"]
            stamp = newer / "data-epoch.json"
            stamp.write_text(json.dumps({"schemaVersion": 2, "epoch": 2, "minimumReaderEpoch": 2,
                "committedDataEpoch": 2, "lastVersion": "9.9.9", "updatedAt": "2026-09-26T08:00:00.000Z"}), encoding="utf-8")
            before_stamp = sha256(stamp)
            fallback_before = snapshot(fallback)
            epoch_env = {**cli_env, "LINGXI_HOME": str(fallback), "HOME": str(fallback),
                         "XDG_DATA_HOME": str(fallback / "xdg-data"),
                         "XDG_CONFIG_HOME": str(fallback / "xdg-config")}
            epoch = matrix.run("serve-newer-epoch", cli_args("serve", "--", "--home", str(newer),
                "--bind", "127.0.0.1:0"), env=epoch_env)
            matrix.case("cli-rust-serve-newer-epoch-no-root-switch", epoch.returncode not in (0, 124)
                        and "LINGXI_DATA_EPOCH_BLOCKED" in epoch.stderr
                        and "LINGXI_SERVICE_READY " not in epoch.stdout
                        and sha256(stamp) == before_stamp and snapshot(fallback) == fallback_before
                        and not (newer / "lingxi-service/data").exists()
                        and not (fallback / "lingxi-service/data").exists(),
                        {"exit": epoch.returncode, "stampSha256": before_stamp,
                         "fallbackBefore": fallback_before, "fallbackAfter": snapshot(fallback)})
            unavailable = matrix.run("sessions-after-shutdown", cli_args("sessions", "--url",
                f"http://{addr}", "--token", "intentionally-invalid-token"), env=cli_env)
            matrix.case("cli-rust-sessions-disconnected", bool(addr) and unavailable.returncode not in (0, 124)
                        and "unreachable" in unavailable.stderr and "Synthetic session" not in unavailable.stdout,
                        {"exit": unavailable.returncode})
        home_notes = {name: snapshot(path) for name, path in homes.items()}
        matrix.save(binary=binary, homes=home_notes)
    return 0 if matrix.cases and all(case["ok"] for case in matrix.cases) else 1


if __name__ == "__main__":
    raise SystemExit(main())

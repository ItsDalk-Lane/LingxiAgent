#!/usr/bin/env python3
"""真实运行 Rust 管理入口 R00 补充叶测试并保存原始证据。"""

from __future__ import annotations

from collections import Counter
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
SOURCE_FILES = (
    "rust/Cargo.toml",
    "rust/Cargo.lock",
    "rust/crates/lingxi-service/Cargo.toml",
    "rust/crates/lingxi-service/tests/r00_management_leaves.rs",
    "rust/crates/lingxi-service/tests/tls_web_login.rs",
    "scripts/rust-tauri/r02_management_leaf_matrix.py",
    "scripts/rust-tauri/r02_owned_process_group.py",
)
EXPECTED_CASES = (
    "management-me-denied-no-state-change",
    "management-me-owner-version-identity-capabilities",
    "management-me-forged-header-ignored",
    "management-me-device-version-identity-capabilities",
    "management-summary-initial-real-state",
    "management-summary-populated-matches-stores",
    "management-summary-unauthorized-no-state-change",
    "management-summary-registry-failure-not-empty",
    "management-profile-success-account-audit",
    "management-profile-invalid-and-unauthorized-unchanged",
    "management-profile-store-failure-unchanged",
    "management-network-success-summary-audit",
    "management-network-invalid-and-unauthorized-unchanged",
    "management-network-store-failure-unchanged",
    "management-mobile-qr-svg-present",
    "management-mobile-qr-unauthorized-no-svg",
    "management-mobile-qr-lan-unavailable-refused",
    "management-password-set-account-audit",
    "management-password-invalid-keeps-prior",
    "management-password-set-store-failure-unchanged",
    "management-password-clear-account-audit",
    "management-password-clear-unauthorized-keeps-prior",
    "management-password-clear-store-failure-unchanged",
    "management-mobile-credential-secret-address-audit",
    "management-mobile-credential-store-failure-no-secret",
    "management-desktop-credential-secret-address-audit",
    "management-desktop-credential-store-failure-no-secret",
    "management-mobile-credential-invalid-unauthorized-unchanged",
    "management-desktop-credential-invalid-unauthorized-unchanged",
    "management-device-list-redacts-secrets",
    "management-device-list-unauthorized-refused",
    "management-device-list-includes-pending-pairing-redacted",
    "management-device-list-registry-failure-not-empty",
    "management-credential-revoke-target-only",
    "management-credential-revoke-store-failure-target-still-active",
    "management-credential-revoke-missing-no-side-effect",
    "management-device-revoke-target-auth-rejected",
    "management-device-revoke-store-failure-target-still-active",
    "management-device-revoke-missing-no-side-effect",
    "management-pairing-create-invalid-unauthorized-unchanged",
    "management-pairing-create-store-failure-no-code",
    "management-pairing-create-code-expiry-audit",
    "management-pairing-approve-wrong-code-scope-auth-unchanged",
    "management-pairing-approve-store-failure-still-pending",
    "management-pairing-approve-once-scope-secret-audit",
    "management-pairing-replay-does-not-sign",
    "management-pairing-concurrent-only-one-credential",
    "management-pairing-expired-code-does-not-sign",
    "management-web-login-password-cookie-14day-desktop-scope",
    "management-web-login-token-positive-14day-mobile-scope",
    "management-web-login-credential-priority-denies-invalid-token",
    "management-web-session-valid-sanitized",
    "management-web-session-invalid-false",
    "management-web-session-expired-false",
    "management-web-session-mobile-scope-no-write",
    "management-web-session-connection-mismatch-false",
    "management-web-login-mobile-scope-restricted",
    "management-web-login-store-failure-no-session",
    "management-web-logout-store-failure-session-alive",
    "management-web-logout-only-current-cookie",
    "management-web-logout-invalid-cookie-no-other-revoke",
    "management-standard-audit-path-failure-no-write-recovery",
    "management-standard-audit-all-actions-identities-no-secrets",
    "management-network-saved-restart-real-bind-no-audit-duplicate",
    "management-network-cli-override-real-bind",
    "management-network-corrupt-saved-settings-refused",
    "management-lan-browser-origin-login-session-logout",
)
TLS_WEB_LOGIN_CASES = (
    "web-login-https-password-secure-14day-cookie",
    "web-login-https-session-logout-revoked",
    "web-login-foreign-origin-denied",
    "web-login-http-forwarded-proto-denied",
)
WEB_MANAGEMENT_CASES = (
    "management-web-login-password-cookie-14day-desktop-scope",
    "management-web-login-token-positive-14day-mobile-scope",
    "management-web-login-credential-priority-denies-invalid-token",
    "management-web-session-mobile-scope-no-write",
    "management-web-login-store-failure-no-session",
    "management-lan-browser-origin-login-session-logout",
)


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


def source_digest() -> dict[str, str]:
    # 每次重新列举源码，运行途中新增或修改生产者代码都会使候选摘要不一致。
    sources = {ROOT / name for name in SOURCE_FILES}
    for crate in ("lingxi-service", "lingxi-protocol"):
        sources.update((ROOT / "rust/crates" / crate / "src").rglob("*.rs"))
    return {
        str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(sources)
    }


def owned_group_alive(pgid: int) -> bool:
    try:
        os.killpg(pgid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        # 无法证明无残留时按未清理处理，不推断为成功。
        return True
    return True


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: r02_management_leaf_matrix.py EVIDENCE_DIR", file=sys.stderr)
        return 2
    output = Path(sys.argv[1]).absolute()
    if output.exists() or output.is_symlink():
        print(f"refusing existing or symbolic-link evidence directory: {output}", file=sys.stderr)
        return 2
    output.mkdir(parents=True, exist_ok=False)
    cases_path = output / "management-cases.json"
    command = [
        "cargo", "test", "--manifest-path", "rust/Cargo.toml", "--locked", "--offline",
        "-p", "lingxi-service", "--test", "r00_management_leaves", "--", "--nocapture",
    ]
    environment = os.environ.copy()
    environment["R02_MANAGEMENT_CASES_PATH"] = str(cases_path)
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
            # 会话领袖仍属本次 Popen 才能发信号；归属不明时保留失败证据。
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
    group_alive_after_exit = owned_group_alive(process.pid)
    if group_alive_after_exit:
        # 领袖已 wait/reap：同号组可能已被复用，不能再凭旧数字补发 KILL。
        detail = "cargo group remained or was unobservable after leader exit; no unsafe signal sent"
        cleanup_error = f"{cleanup_error}; {detail}" if cleanup_error else detail
    tls_cases_path = output / "tls-web-login-cases.json"
    tls_command = [
        "cargo", "test", "--manifest-path", "rust/Cargo.toml", "--locked", "--offline",
        "-p", "lingxi-service", "--test", "tls_web_login", "--", "--nocapture",
    ]
    tls_environment = environment.copy()
    tls_environment["R02_TLS_WEB_LOGIN_CASES_PATH"] = str(tls_cases_path)
    tls_timed_out = False
    tls_cleanup_error = None
    with (output / "tls-cargo.stdout.log").open("wb") as stdout, \
         (output / "tls-cargo.stderr.log").open("wb") as stderr:
        tls_process = subprocess.Popen(
            tls_command, cwd=ROOT, env=tls_environment, stdout=stdout, stderr=stderr,
            start_new_session=True,
        )
        try:
            tls_process.wait(timeout=540)
        except subprocess.TimeoutExpired:
            tls_timed_out = True
            try:
                if not signal_owned_group(tls_process, signal.SIGKILL):
                    tls_cleanup_error = "TLS cargo group leader ownership could not be proven"
            except OSError as exc:
                tls_cleanup_error = f"owned TLS group signal failed: {exc}"
            try:
                tls_process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                detail = "owned TLS cargo process remained after timeout cleanup"
                tls_cleanup_error = f"{tls_cleanup_error}; {detail}" if tls_cleanup_error else detail
    tls_command_end = now()
    tls_group_alive = owned_group_alive(tls_process.pid)
    if tls_group_alive:
        detail = "TLS cargo group remained or was unobservable after leader exit"
        tls_cleanup_error = f"{tls_cleanup_error}; {detail}" if tls_cleanup_error else detail
    source_after = source_digest()
    issues: list[str] = []
    names: list[str] = []
    management_cases: list[dict] = []
    audit_event_count = 0
    if not cases_path.is_file():
        issues.append("management case file missing")
    else:
        try:
            evidence = json.loads(cases_path.read_text(encoding="utf-8"))
            if evidence.get("schema") != SCHEMA or not isinstance(evidence.get("cases"), list):
                issues.append("management case schema invalid")
            else:
                management_cases = evidence["cases"]
                for item in evidence["cases"]:
                    if not isinstance(item, dict) or not isinstance(item.get("case"), str):
                        issues.append("management case identity invalid")
                        continue
                    names.append(item["case"])
                    if item.get("ok") is not True or item.get("actual") != item.get("expect"):
                        issues.append(f"case failed: {item['case']}")
                if len(names) != len(set(names)):
                    issues.append("duplicate management case identity")
                if set(names) != set(EXPECTED_CASES) or len(names) != len(EXPECTED_CASES):
                    issues.append("management case identities differ from pinned producer contract")
        except (OSError, ValueError) as exc:
            issues.append(f"management case read failed: {exc}")
    tls_cases: list[dict] = []
    if not tls_cases_path.is_file():
        issues.append("TLS Web login case file missing")
    else:
        try:
            tls_evidence = json.loads(tls_cases_path.read_text(encoding="utf-8"))
            if tls_evidence.get("schema") != SCHEMA or not isinstance(tls_evidence.get("cases"), list):
                raise ValueError("TLS Web login case schema invalid")
            tls_cases = tls_evidence["cases"]
            tls_names = [item.get("case") for item in tls_cases if isinstance(item, dict)]
            if len(tls_names) != len(tls_cases) or len(tls_names) != len(TLS_WEB_LOGIN_CASES) \
                    or set(tls_names) != set(TLS_WEB_LOGIN_CASES):
                raise ValueError("TLS Web login case identities differ from pinned producer contract")
            if any(item.get("ok") is not True or item.get("actual") != item.get("expect")
                   for item in tls_cases):
                raise ValueError("TLS Web login case failed")
        except (OSError, ValueError, TypeError) as exc:
            issues.append(f"TLS Web login evidence invalid: {exc}")
    selected = [item for item in management_cases if isinstance(item, dict)
                and item.get("case") in WEB_MANAGEMENT_CASES]
    if len(selected) != len(WEB_MANAGEMENT_CASES) \
            or {item.get("case") for item in selected} != set(WEB_MANAGEMENT_CASES):
        issues.append("Web login management case identities are incomplete")
    if len(tls_cases) == len(TLS_WEB_LOGIN_CASES) and len(selected) == len(WEB_MANAGEMENT_CASES):
        (output / "web-login-cases.json").write_text(
            json.dumps({"schema": SCHEMA, "leafId": "R00-T02-LA-C4E6F27D7873",
                        "cases": selected + tls_cases}, indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
    source_path = output / "security-audit-sources.json"
    audit_path = output / "security-audit.jsonl"
    if not source_path.is_file() or not audit_path.is_file():
        issues.append("standard security audit source or JSONL evidence missing")
    else:
        try:
            source_evidence = json.loads(source_path.read_text(encoding="utf-8"))
            management_intents = source_evidence["managementIntents"]
            device_intents = source_evidence["deviceIntents"]
            if not isinstance(management_intents, list) or not isinstance(device_intents, list):
                raise ValueError("standard audit intent lists invalid")
            events = [json.loads(line) for line in audit_path.read_text(encoding="utf-8").splitlines() if line]
            audit_event_count = len(events)
            expected = Counter(
                (item["action"], item["target"], json.dumps(item.get("metadata") or {}, sort_keys=True), item["atUnixMs"])
                for item in management_intents + device_intents
            )
            actual = Counter(
                (item["action"], item["target"], json.dumps(item["metadata"], sort_keys=True),
                 round(datetime.fromisoformat(item["timestamp"].replace("Z", "+00:00")).timestamp() * 1000))
                for item in events
            )
            if not events or expected != actual:
                issues.append("standard audit JSONL does not exactly match committed source intents")
            ids = [item.get("eventId") for item in events]
            if any(not isinstance(value, str) or not value.startswith("sec_") for value in ids) or len(ids) != len(set(ids)):
                issues.append("standard audit event identities invalid or duplicate")
            for event in events:
                actor = event.get("actor") or {}
                if event.get("schemaVersion") != 1 or event.get("result") != "success" or actor.get("kind") != "local_user" or actor.get("userId") != "user_local" or actor.get("connectionKind") != "local" or actor.get("credentialKind") != "loopback_token":
                    issues.append("standard audit schema or owner identity mismatch")
                    break
                observed_time = datetime.fromisoformat(event["timestamp"].replace("Z", "+00:00"))
                if observed_time.tzinfo is None:
                    issues.append("standard audit timestamp lacks timezone")
                    break
                if "secret" in event.get("metadata", {}) or "password" in event.get("metadata", {}):
                    issues.append("standard audit metadata contains a secret field")
                    break
        except (OSError, ValueError, KeyError, TypeError) as exc:
            issues.append(f"standard audit evidence read failed: {exc}")
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
    if tls_timed_out:
        issues.append("TLS cargo test exceeded 540 seconds")
    if tls_cleanup_error:
        issues.append(tls_cleanup_error)
    if tls_group_alive:
        issues.append("owned TLS cargo group remained after command exit")
    if tls_process.returncode != 0:
        issues.append(f"TLS cargo test exited {tls_process.returncode}")
    summary = {
        "schema": "lingxi.r02-management-producer.v1",
        "command": command,
        "startUtc": start,
        "commandEndUtc": command_end,
        "tlsCommand": tls_command,
        "tlsCommandEndUtc": tls_command_end,
        "tlsExitCode": tls_process.returncode,
        "tlsTimedOut": tls_timed_out,
        "tlsOwnedGroupAliveAfterExit": tls_group_alive,
        "endUtc": now(),
        "exitCode": process.returncode,
        "timedOut": timed_out,
        "ownedGroupAliveAfterExit": group_alive_after_exit,
        "sourceSha256": source_before,
        "caseCount": len(names),
        "tlsCaseCount": len(tls_cases),
        "auditEventCount": audit_event_count,
        "issues": issues,
        "status": "PASS" if not issues else "FAIL",
    }
    (output / "summary.json").write_text(json.dumps(summary, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"R02 management leaf producer: {len(names)} cases, {summary['status']}")
    return 0 if not issues else 1


if __name__ == "__main__":
    raise SystemExit(main())

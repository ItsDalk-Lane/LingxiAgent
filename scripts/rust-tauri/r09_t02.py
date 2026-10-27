#!/usr/bin/env python3
"""R09-T02 证据生产者：A03（错误二进制明确拒绝、不兜底）/ A04（退出只清自有进程）。

套件：
  r09_t02_supervisor_ready    —— 真实 READY 握手（禁 sleep）、打包清单协议门
                                 （注入不兼容 build.json 后仍被拒）、运行时 identity
                                 门（确定性替身）、失败记录/诊断/无 Node 兜底。
  r09_t02_process_ownership   —— 本地 owned + 远端并存：退出只清自有进程树；
                                 复用实例 detached；远端零接触；release 拒远端；
                                 PID 复用不误杀；退出窗口无额外 spawn。

真实执行：真实 `lingxi-service`、真实 debug/release Tauri 应用、真实信号与进程表。
负向注入只发生在 /private/tmp 的 `.app` 副本（不碰工作区产物）；证据只留摘要。
"""

from __future__ import annotations

import json
import os
import re
import shutil
import signal
import subprocess
import sys
import time
import traceback
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[1]
sys.path.insert(0, str(SCRIPTS))

from r09_producer import EvidenceError, Producer, sha256_file  # noqa: E402
from r09_t01 import (  # noqa: E402
    AUTOMATION,
    TAURI_RELEASE_APP,
    cleanup_runtime_processes,
    dialog_sink_for,
    events_of,
    fetch_health,
    launch_tauri,
    ps_table,
    service_pids_for_home,
    stop_tauri,
    wait_for_event,
    wait_process_exit,
)

DEBUG_SERVICE = ROOT / "rust/target/debug/lingxi-service"
DEBUG_BIN = ROOT / "desktop/src-tauri/target/debug/lingxi-resource-desktop"
RUNTIME_ROOT = Path("/private/tmp")


def rustup() -> str:
    candidate = os.environ.get("RUSTUP_BIN", str(Path.home() / ".cargo/bin/rustup"))
    if not Path(candidate).is_file():
        raise EvidenceError(f"rustup not found: {candidate}")
    return candidate


def runtime_home(producer: Producer, label: str) -> Path:
    path = producer.runtime / label
    path.mkdir(parents=True, exist_ok=True)
    return path


def launch_hidden_tauri(producer: Producer, binary: Path, home: Path, tmp: Path,
                        extra_env: dict | None = None) -> dict:
    """T02 始终 hidden+sink，显式注入 automation/sink，避免被全局
    `LINGXI_R09_T01_VISIBLE=1`（T01 visible 批次）泄漏成可见窗口；
    fake 服务等负向路径也必须有 failure-dialog sink。"""
    sink = dialog_sink_for(producer, f"tauri-{home.name}")
    hidden_env: dict = {
        "LINGXI_R09_AUTOMATION": "1",
        "LINGXI_R09_HOST_DIALOG_SINK": str(sink),
    }
    hidden_env.update(extra_env or {})
    return launch_tauri(producer, binary, home, tmp, extra_env=hidden_env)


def start_standalone_service(producer: Producer, home: Path, label: str) -> dict:
    """真实服务（harness 自己的进程；不属于宿主，任何客户端都不得杀）。"""
    home.mkdir(parents=True, exist_ok=True)
    stdout_path = producer.raw / f"standalone-{label}.out"
    stderr_path = producer.raw / f"standalone-{label}.err"
    env = {**os.environ, **producer.env(), "LINGXI_HOME": str(home)}
    stdout = stdout_path.open("wb")
    stderr = stderr_path.open("wb")
    process = subprocess.Popen([str(DEBUG_SERVICE), "--home", str(home)],
                               cwd=str(ROOT), env=env, stdout=stdout, stderr=stderr)
    deadline = time.time() + 90
    addr = None
    while time.time() < deadline:
        if process.poll() is not None:
            raise EvidenceError(f"standalone service exited before READY ({process.returncode})")
        if stdout_path.is_file():
            match = re.search(r"LINGXI_SERVICE_READY addr=(\S+) home=", stdout_path.read_text(errors="replace"))
            if match:
                addr = match.group(1)
                break
        time.sleep(0.1)
    if not addr:
        raise EvidenceError("standalone service never printed READY")
    port = int(addr.rsplit(":", 1)[1])
    token_path = home / "lingxi-service/local-token.json"
    deadline = time.time() + 30
    token = None
    while time.time() < deadline:
        if token_path.is_file():
            try:
                token = json.loads(token_path.read_text(encoding="utf-8"))["token"]
                break
            except (json.JSONDecodeError, KeyError):
                pass
        time.sleep(0.05)
    if not token:
        raise EvidenceError("standalone service token file never appeared")
    identity = wait_identity(port, token, timeout=30)
    if identity is None:
        raise EvidenceError("standalone service identity never became ready")
    HARNESS_PIDS.add(process.pid)
    return {"process": process, "home": home, "port": port, "token": token,
            "stdout": stdout, "stderr": stderr, "identity": identity}


def wait_identity(port: int, token: str, timeout: float) -> dict | None:
    import urllib.request
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            request = urllib.request.Request(
                f"http://127.0.0.1:{port}/lingxi/v1/server/identity",
                headers={"Authorization": f"Bearer {token}"},
            )
            with urllib.request.urlopen(request, timeout=3) as response:
                return json.loads(response.read().decode("utf-8"))
        except Exception:
            time.sleep(0.1)
    return None


def stop_standalone(fixture: dict) -> dict:
    process = fixture["process"]
    result = {"exit": None}
    if process.poll() is None:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)
    result["exit"] = process.returncode
    HARNESS_PIDS.discard(process.pid)
    fixture["stdout"].close()
    fixture["stderr"].close()
    return result


# ── v2 只读候选消费（T02 冻结后禁止任何重建）─────────────────────────────
CANDIDATE_HANDOFF = ROOT / "artifacts/rust-tauri/R09/dev/t06/candidate-handoff-v2/candidate-handoff.json"


def _candidate_path(name: str) -> str | None:
    value = os.environ.get(name)
    return value if value else None


def _candidate_receipt(env_name: str, default: Path) -> dict:
    path = Path(_candidate_path(env_name) or default)
    if path.is_file():
        try:
            return json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return {}
    return {}


def _candidate_executable(env_name: str) -> Path:
    raw = _candidate_path(env_name)
    if not raw:
        raise EvidenceError(f"CANDIDATE_REQUIRED: 需要 {env_name}（v2 evidence/delivery .app 或可执行；T02 禁止自行构建）")
    path = Path(raw)
    if path.is_dir():
        executable = path / "Contents/MacOS/lingxi-resource-desktop"
        if executable.is_file():
            return executable
        raise EvidenceError(f"CANDIDATE_REQUIRED: {env_name} 目录内缺少 Contents/MacOS/lingxi-resource-desktop: {path}")
    if path.is_file():
        return path
    raise EvidenceError(f"CANDIDATE_REQUIRED: {env_name} 指向的路径不存在: {path}")


def _synthetic_receipt(mode: str, executable: Path, receipt: dict, expected_features: list[str]) -> dict:
    features = (receipt.get("inputs") or {}).get("features") or receipt.get("features") or expected_features
    if any(feature not in features for feature in expected_features):
        raise EvidenceError(f"CANDIDATE_REQUIRED: {mode} 候选 features={features} 不包含 {expected_features}")
    return {
        "mode": mode,
        "artifacts": {
            "executable": str(executable),
            "executableSha256": sha256_file(executable),
            "appBundle": None,
        },
        "inputs": {
            "features": features,
            "rendererDigest": (receipt.get("inputs") or {}).get("rendererDigest"),
        },
        "service": receipt.get("service"),
        "candidate": {"source": "env", "mode": mode, "executableSha256": sha256_file(executable)},
    }


def consume_debug_candidate(producer: Producer, evidence: Path) -> dict:
    executable = _candidate_executable("LINGXI_R09_T02_DEBUG_APP")
    receipt = _candidate_receipt("LINGXI_R09_T02_DEBUG_RECEIPT",
                                 ROOT / "desktop/src-tauri/.tauri-build/build-receipt-debug.json")
    synthetic = _synthetic_receipt("debug", executable, receipt, ["e2e-test", "r09-host", "r09-test-hooks"])
    producer.check("t02::candidate.debug-consume-only", True, 1, detail=synthetic["candidate"])
    producer.check("t02::candidate.debug-features", True, 1, detail={"features": synthetic["inputs"]["features"]})
    (evidence / "candidate-debug.json").write_text(json.dumps(synthetic, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    return synthetic


def consume_release_candidate(producer: Producer, evidence: Path) -> dict:
    raw = _candidate_path("LINGXI_R09_T02_RELEASE_APP")
    if not raw:
        raise EvidenceError("CANDIDATE_REQUIRED: 需要 LINGXI_R09_T02_RELEASE_APP（v2 delivery .app；T02 禁止自行构建）")
    app = Path(raw)
    if not app.is_dir():
        raise EvidenceError(f"CANDIDATE_REQUIRED: LINGXI_R09_T02_RELEASE_APP 不是 .app 目录: {app}")
    executable = app / "Contents/MacOS/lingxi-resource-desktop"
    if not executable.is_file():
        raise EvidenceError(f"CANDIDATE_REQUIRED: delivery .app 缺少主可执行: {executable}")
    receipt = _candidate_receipt("LINGXI_R09_T02_RELEASE_RECEIPT",
                                 ROOT / "desktop/src-tauri/.tauri-build/build-receipt-release.json")
    synthetic = _synthetic_receipt("release", executable, receipt, ["r09-host"])
    synthetic["artifacts"]["appBundle"] = str(app)
    producer.check("t02::candidate.release-consume-only", True, 1,
                   detail={"app": str(app), "executableSha256": synthetic["candidate"]["executableSha256"],
                           "features": synthetic["inputs"]["features"]})
    (evidence / "candidate-release.json").write_text(json.dumps(synthetic, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    return synthetic


def ensure_debug_build(producer: Producer, evidence: Path) -> dict:
    return consume_debug_candidate(producer, evidence)


def ensure_service_stage(producer: Producer, attempts: int = 4) -> None:
    """只读校验 service stage；失败即 CANDIDATE_REQUIRED（绝不重建）。"""
    verify = producer.run_step("service-stage-verify", ["node", "scripts/build-rust-desktop-service.mjs", "--verify"],
                               timeout=300, expect_zero=False)
    if verify.returncode != 0:
        raise EvidenceError("CANDIDATE_REQUIRED: service stage verify 失败；v2 冻结期禁止重建，需 T06 重建后交接")


def ensure_release_build(producer: Producer, evidence: Path) -> dict:
    return consume_release_candidate(producer, evidence)


def launch_release_app(producer: Producer, executable: Path, app_root: Path,
                       home: Path, extra_env: dict | None = None, label: str = "release") -> dict:
    log_path = producer.raw / f"{label}.log"
    sink = dialog_sink_for(producer, label)
    env = {**os.environ, **producer.env()}
    env.update(AUTOMATION)
    env.update({"LINGXI_HOME": str(home), "TMPDIR": str(producer.tmp),
                "LINGXI_R09_HOST_DIALOG_SINK": str(sink)})
    env.pop("LINGXI_SERVICE_BIN", None)
    env.update(extra_env or {})
    handle = log_path.open("wb")
    process = subprocess.Popen([str(executable)], cwd=str(app_root), env=env,
                               stdout=handle, stderr=subprocess.STDOUT)
    return {"process": process, "handle": handle, "log": log_path, "env": env, "dialogSink": sink}


def wait_exit(launched: dict, timeout: float) -> dict:
    process = launched["process"]
    result = {"exit": None, "timedOut": False}
    try:
        result["exit"] = process.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        result["timedOut"] = True
        process.kill()
        result["exit"] = process.wait(timeout=10)
    launched["handle"].close()
    return result


# harness 自己启动的“远端/复用”服务（属于测试夹具，不是宿主 owned 进程）。
HARNESS_PIDS: set[int] = set()


def pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True


def remote_parent_pid(remote_entries: list[dict]) -> int | None:
    """远端夹具在 S1 快照里的真实父 pid（harness）；不是它自己的 pid。"""
    return remote_entries[0]["ppid"] if remote_entries else None


def assert_ps_clean(producer: Producer, label: str, app_pids: list[int]) -> None:
    """每发之间硬断言：该次启动的 app 进程已退出、无本套件 runtime 的残留服务。"""
    time.sleep(0.3)
    alive = [pid for pid in app_pids if pid_alive(pid)]
    residual_services = [
        entry for entry in ps_table()
        if "lingxi-service" in entry["command"]
        and str(producer.runtime) in entry["command"]
        and entry["pid"] not in HARNESS_PIDS
    ]
    producer.check(
        f"isolation::ps-clean-{label}",
        not alive and not residual_services,
        1,
        detail={"label": label, "aliveAppPids": alive, "residualServices": residual_services},
    )


def reap_home_services(home: Path) -> list[int]:
    """退出后兜底回收某 home 的 sidecar/服务（优雅路径不应留下，异常路径必须清）。"""
    reaped: list[int] = []
    for entry in service_pids_for_home(home):
        try:
            os.kill(entry["pid"], signal.SIGTERM)
            reaped.append(entry["pid"])
        except ProcessLookupError:
            pass
    time.sleep(0.3)
    for entry in service_pids_for_home(home):
        try:
            os.kill(entry["pid"], signal.SIGKILL)
        except ProcessLookupError:
            pass
    return reaped


def suppressed_dialog(launched: dict, expected_code: str) -> dict:
    """断言失败路径是“无 modal + sink 结构化记录”（自动化硬规则）。"""
    sink = launched.get("dialogSink")
    payload = None
    if sink and Path(sink).is_file():
        try:
            payload = json.loads(Path(sink).read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            payload = None
    return {
        "sink": str(sink) if sink else None,
        "exists": bool(sink and Path(sink).is_file()),
        "payload": payload,
        "codeMatches": bool(payload and payload.get("code") == expected_code),
        "modalShown": bool(payload.get("modalShown")) if payload else None,
        "automation": (launched.get("env") or {}).get("LINGXI_R09_AUTOMATION"),
    }


def failure_record_from(launched: dict, home: Path) -> tuple[dict | None, Path | None, Path | None]:
    """从宿主 stderr 定位失败记录/崩溃日志（可能落在未 stamp home 的 TMPDIR fallback）。"""
    text = launched["log"].read_text(errors="replace") if launched["log"].is_file() else ""
    failure_paths = re.findall(r"\[tauri-diagnostics\] failure record: (\S+)", text)
    crash_paths = re.findall(r"\[tauri-diagnostics\] crash log: (\S+)", text)
    failure_path = Path(failure_paths[-1]) if failure_paths else None
    crash_path = Path(crash_paths[-1]) if crash_paths else None
    record = None
    candidates: list[Path] = []
    if failure_path:
        candidates.append(failure_path)
    candidates.append(home / "diagnostics/tauri-launch/failure.json")
    for candidate in candidates:
        if candidate.is_file():
            try:
                payload = json.loads(candidate.read_text(encoding="utf-8"))
                record = payload.get("record", payload)
                failure_path = candidate
                break
            except json.JSONDecodeError:
                continue
    return record, failure_path, crash_path


def sample_process_tree(producer: Producer, label: str, home: Path, app_pid: int | None = None) -> dict:
    table = ps_table()
    services = [entry for entry in table if "lingxi-service" in entry["command"]]
    app_entries = [entry for entry in table if entry["pid"] == app_pid] if app_pid else []
    snapshot = {
        "label": label,
        "unixMs": int(time.time() * 1000),
        "app": app_entries,
        "services": [entry for entry in services if str(home) in entry["command"]],
        "allServices": services,
    }
    (producer.evidence / f"tree-{label}.json").write_text(json.dumps(snapshot, ensure_ascii=False, indent=1) + "\n",
                                                          encoding="utf-8")
    return snapshot


def shell_count(producer: Producer, step: str, binary: Path, pattern: str) -> int:
    result = producer.run_step(
        step,
        ["/bin/sh", "-c", f"strings -a {binary} | grep -c -E {json.dumps(pattern)} || true"],
        timeout=300,
    )
    return int((result.stdout or "0").strip() or 0)


def host_surfaces(producer: Producer) -> None:
    """无 Node 兜底的结构事实：release 二进制不含 Node 服务入口；宿主代码只有 rust 分支。"""
    receipt = ensure_release_build(producer, producer.evidence)
    executable = Path(receipt["artifacts"]["executable"])
    needles = {
        "hana-server": shell_count(producer, "strings-hana-server", executable, "hana-server"),
        "server/bootstrap": shell_count(producer, "strings-server-bootstrap", executable, "server/bootstrap"),
        "bundle/index.js": shell_count(producer, "strings-bundle-index", executable, "bundle/index" ".js"),
        "bridge-sdk-worker": shell_count(producer, "strings-bridge-worker", executable, "bridge-sdk-worker"),
    }
    # 说明：二进制里出现 `LINGXI_DESKTOP_SERVER_RUNTIME` 是**远端模式环境契约**
    # （与 Electron 同形，值必须显式为 rust），不是 Node 兜底路径；不计入本断言。
    remaining = [name for name, count in needles.items() if count > 0]
    producer.check("t02::no-node-fallback-in-release-binary", not remaining, 1,
                   detail={"matches": needles})


def wait_home_event(producer: Producer, home: Path, name: str, launched: dict, timeout: float) -> dict | None:
    return wait_for_event(home, name, timeout=timeout, stderr_path=launched["log"])


def hidden_only() -> bool:
    return os.environ.get("LINGXI_R09_T02_HIDDEN_ONLY") == "1"


def visible_pending_guard(producer: Producer, labels: list[str], extra: dict) -> int:
    """hidden-only 批次：只对已执行的 debug hidden 子集判定 PASS；release/可见用例
    不执行、不冒充通过，登记到 extra.pendingVisible（不产生 blocked:: 假红）。"""
    try:
        cleanup_runtime_processes(producer)
    except Exception as cleanup_error:  # noqa: BLE001
        producer.logs.append(f"[cleanup-error] {cleanup_error}")
    producer.assert_real_home_untouched()
    return producer.finish(extra={"hiddenOnly": True, "pendingVisible": labels,
                                  "note": "hidden-only 子集：只对已执行 debug hidden cases 判定；pendingVisible 不冒充通过", **extra})


def suite_supervisor_ready(producer: Producer) -> int:
    debug_receipt = ensure_debug_build(producer, producer.evidence)
    # hidden-only 子集不跑 release 用例：不得要求 release 候选（防止误判 CANDIDATE_REQUIRED）。
    hidden = hidden_only()
    release_receipt = ({"artifacts": {"appBundle": None, "executable": None}}
                       if hidden else ensure_release_build(producer, producer.evidence))
    debug_binary = Path(debug_receipt["artifacts"]["executable"])
    release_app = Path("/nonexistent-release-app") if hidden else Path(release_receipt["artifacts"]["appBundle"])
    release_executable = (Path("/nonexistent-release-exe") if hidden
                          else Path(release_receipt["artifacts"]["executable"]))

    # ── A03 正路径 1：真实服务 READY 握手（禁止固定 sleep） ──
    home = runtime_home(producer, "home-debug-owned")
    launched = launch_hidden_tauri(producer, debug_binary, home, producer.tmp)
    try:
        app_ready = wait_home_event(producer, home, "app-ready", launched, 150)
        events = events_of(home, launched["log"])
    finally:
        stopped = stop_tauri(producer, launched, home)
    names = [event.get("event") for event in events]
    ready = next((event for event in events if event.get("event") == "ready"), None)
    spawn = next((event for event in events if event.get("event") == "spawn"), None)
    instance = next((event for event in events if event.get("event") == "instance"), None)
    identity = next((event for event in events if event.get("event") == "identity"), None)
    service_ready = next((event for event in events if event.get("event") == "service-ready"), None)
    order = {name: index for index, name in enumerate(names)}
    ordered = (
        order.get("spawn", 1 << 30) < order.get("ready", 1 << 30)
        <= order.get("instance", 1 << 30) <= order.get("identity", 1 << 30)
        < order.get("service-ready", 1 << 30) < order.get("app-ready", 1 << 30)
    )
    ready_ms = int((ready or {}).get("msSinceSpawn") or -1)
    producer.check(
        "t02::ready-handshake-real",
        app_ready is not None and ready is not None and spawn is not None
        and instance is not None and identity is not None and service_ready is not None
        and ordered and 0 <= ready_ms < 90_000,
        1, detail={"events": names, "readyMsSinceSpawn": ready_ms, "ordered": ordered,
                   "readyAddr": (ready or {}).get("addr")},
    )
    producer.check(
        "t02::no-fixed-sleep-handshake",
        ready is not None and 0 < ready_ms < 30_000
        and (service_ready or {}).get("mode") == "local-owned",
        1, detail={"readyMsSinceSpawn": ready_ms, "mode": (service_ready or {}).get("mode")},
    )
    producer.check("t02::debug-exit-graceful", stopped.get("exit") == 0 and not stopped.get("timedOut")
                   and not stopped.get("service_residual"), 1, detail=stopped)
    assert_ps_clean(producer, "supervisor-debug-positive", [launched["process"].pid])
    producer.check("t02::automation-no-modal-positive", not Path(launched["dialogSink"]).exists(), 1,
                   detail={"dialogSink": str(launched["dialogSink"]), "automation": True})

    if hidden_only():
        return visible_pending_guard(producer, [
            "supervisor-release-positive", "supervisor-injected-protocol", "supervisor-injected-digest",
            "supervisor-runtime-identity", "no-node-fallback-in-release-binary"],
            {"suite": "r09_t02_supervisor_ready"})

    # ── A03 正路径 2：release 打包清单 + 协议门通过（sidecar 被 ad-hoc 重签后 contentSha 放行） ──
    home_pkg = runtime_home(producer, "home-release-owned")
    launched_pkg = launch_release_app(producer, release_executable, release_app.parent, home_pkg,
                                      label="release-owned")
    try:
        ready_pkg = wait_home_event(producer, home_pkg, "app-ready", launched_pkg, 180)
        time.sleep(1.0)
        events_pkg = events_of(home_pkg, launched_pkg["log"])
        # 彻底退出：向宿主发 SIGTERM（宿主安装信号 handler → 清理 owned 服务 → exit 0）。
        launched_pkg["process"].send_signal(signal.SIGTERM)
    finally:
        stopped_pkg = wait_exit(launched_pkg, 60)
        stopped_pkg["serviceResidual"] = [entry["pid"] for entry in service_pids_for_home(home_pkg)]
    manifest = next((event for event in events_pkg if event.get("event") == "manifest"), None)
    service_ready_pkg = next((event for event in events_pkg if event.get("event") == "service-ready"), None)
    producer.check(
        "t02::manifest-protocol-match",
        ready_pkg is not None and manifest is not None
        and (manifest or {}).get("protocol") == "lingxi-service wire 1..=1 data-epoch 2"
        and (manifest or {}).get("signedMutation") is True
        and (service_ready_pkg or {}).get("mode") == "local-owned",
        1, detail={"manifest": manifest, "serviceReady": service_ready_pkg},
    )
    producer.check("t02::packaged-exit-graceful", stopped_pkg.get("exit") == 0 and not stopped_pkg.get("timedOut")
                   and not stopped_pkg.get("serviceResidual"), 1, detail=stopped_pkg)
    assert_ps_clean(producer, "supervisor-release-positive", [launched_pkg["process"].pid])
    producer.check("t02::automation-no-modal-packaged", not Path(launched_pkg["dialogSink"]).exists(), 1,
                   detail={"dialogSink": str(launched_pkg["dialogSink"]), "automation": True})

    # ── A03 负路径 1：注入不兼容协议清单（/private/tmp 副本；不碰工作区产物） ──
    injected_root = RUNTIME_ROOT / f"lingxi-r09-injected-{os.getpid()}"
    shutil.rmtree(injected_root, ignore_errors=True)
    injected_root.mkdir(parents=True)
    protocol_app = injected_root / "ProtocolMismatch.app"
    shutil.copytree(release_app, protocol_app, symlinks=True)
    protocol_manifest = protocol_app / "Contents/Resources/rust-service/build.json"
    manifest_json = json.loads(protocol_manifest.read_text(encoding="utf-8"))
    manifest_json["protocol"]["wireProtocolMax"] = 2
    protocol_manifest.write_text(json.dumps(manifest_json, indent=2) + "\n", encoding="utf-8")
    codesign_probe = subprocess.run(["codesign", "--verify", "--deep", "--strict", str(protocol_app)],
                                    capture_output=True, text=True)
    (producer.evidence / "injected-protocol-codesign.txt").write_text(
        f"exit={codesign_probe.returncode}\n{codesign_probe.stdout}{codesign_probe.stderr}", encoding="utf-8")
    home_bad = runtime_home(producer, "home-protocol-mismatch")
    launched_bad = launch_release_app(producer, protocol_app / "Contents/MacOS/lingxi-resource-desktop",
                                      protocol_app, home_bad, label="protocol-mismatch")
    try:
        exit_bad = wait_exit(launched_bad, 60)
    finally:
        if launched_bad["process"].poll() is None:
            launched_bad["process"].kill()
    record, record_path, crash_path = failure_record_from(launched_bad, home_bad)
    producer.check(
        "t02::manifest-protocol-mismatch-rejected",
        exit_bad.get("exit") not in (0, None)
        and record is not None
        and (record or {}).get("reason") == "RUST_SERVICE_PROTOCOL_INCOMPATIBLE"
        and "wire 1..=1" in str((record or {}).get("expected"))
        and "1..=2" in str((record or {}).get("actual")),
        1, detail={"exit": exit_bad, "record": record, "recordPath": str(record_path),
                   "codesignExit": codesign_probe.returncode},
    )
    protocol_dialog = suppressed_dialog(launched_bad, "RUST_SERVICE_PROTOCOL_INCOMPATIBLE")
    reap_home_services(home_bad)
    producer.check("t02::injected-protocol-no-service-spawn", not service_pids_for_home(home_bad), 1,
                   detail={"residual": [entry["pid"] for entry in service_pids_for_home(home_bad)]})
    producer.check("t02::failure-dialog-suppressed-protocol",
                   protocol_dialog["exists"] and protocol_dialog["codeMatches"]
                   and protocol_dialog["modalShown"] is False
                   and protocol_dialog["automation"] == "1", 1, detail=protocol_dialog)
    producer.check("t02::injected-protocol-no-node-fallback",
                   record is not None and (record or {}).get("fallbackAttempts") == 0
                   and (record or {}).get("nodeFallback") == "none", 1, detail={"record": record})
    assert_ps_clean(producer, "supervisor-injected-protocol", [launched_bad["process"].pid])

    # ── A03 负路径 2：注入摘要不符清单 ──
    digest_app = injected_root / "DigestMismatch.app"
    shutil.copytree(release_app, digest_app, symlinks=True)
    digest_manifest = digest_app / "Contents/Resources/rust-service/build.json"
    digest_json = json.loads(digest_manifest.read_text(encoding="utf-8"))
    digest_json["sha256"] = "a" * 64
    digest_json["contentSha256"] = "b" * 64
    digest_manifest.write_text(json.dumps(digest_json, indent=2) + "\n", encoding="utf-8")
    home_digest = runtime_home(producer, "home-digest-mismatch")
    launched_digest = launch_release_app(producer, digest_app / "Contents/MacOS/lingxi-resource-desktop",
                                         digest_app, home_digest, label="digest-mismatch")
    try:
        exit_digest = wait_exit(launched_digest, 60)
    finally:
        if launched_digest["process"].poll() is None:
            launched_digest["process"].kill()
    digest_record, digest_record_path, digest_crash_path = failure_record_from(launched_digest, home_digest)
    producer.check(
        "t02::manifest-digest-mismatch-rejected",
        exit_digest.get("exit") not in (0, None)
        and digest_record is not None
        and (digest_record or {}).get("reason") == "RUST_SERVICE_DIGEST_MISMATCH",
        1, detail={"exit": exit_digest, "record": digest_record, "recordPath": str(digest_record_path)},
    )
    digest_dialog = suppressed_dialog(launched_digest, "RUST_SERVICE_DIGEST_MISMATCH")
    reap_home_services(home_digest)
    producer.check("t02::injected-digest-no-service-spawn", not service_pids_for_home(home_digest), 1)
    assert_ps_clean(producer, "supervisor-injected-digest", [launched_digest["process"].pid])
    producer.check("t02::failure-dialog-suppressed-digest",
                   digest_dialog["exists"] and digest_dialog["codeMatches"]
                   and digest_dialog["modalShown"] is False, 1, detail=digest_dialog)

    # ── A03 负路径 3：确定性替身服务（READY 合法、instance 协议不兼容）──
    home_fake = runtime_home(producer, "home-runtime-mismatch")
    fake = write_incompatible_fake_service(producer, home_fake)
    launched_fake = launch_hidden_tauri(producer, debug_binary, home_fake, producer.tmp,
                                       extra_env={"LINGXI_SERVICE_BIN": str(fake)})
    try:
        exit_fake = wait_exit(launched_fake, 60)
    finally:
        if launched_fake["process"].poll() is None:
            launched_fake["process"].kill()
    fake_record, fake_record_path, fake_crash_path = failure_record_from(launched_fake, home_fake)
    fake_processes = [entry for entry in ps_table() if str(fake) in entry["command"]]
    runtime_dialog = suppressed_dialog(launched_fake, "RUNTIME_IDENTITY_MISMATCH")
    producer.check(
        "t02::runtime-identity-mismatch-rejected",
        exit_fake.get("exit") not in (0, None)
        and fake_record is not None
        and (fake_record or {}).get("reason") == "RUNTIME_IDENTITY_MISMATCH"
        and "wire 1..=3" in str((fake_record or {}).get("actual"))
        and not fake_processes,
        1, detail={"exit": exit_fake, "record": fake_record, "recordPath": str(fake_record_path),
                   "fakeResidual": fake_processes},
    )

    assert_ps_clean(producer, "supervisor-runtime-mismatch", [launched_fake["process"].pid])

    # ── A03：错误面（先落盘再弹窗；dialog 视觉观察受 TCC/自动化限制，如实记录）──
    surfaced = [
        (record_path, crash_path),
        (digest_record_path, digest_crash_path),
        (fake_record_path, fake_crash_path),
    ]
    producer.check(
        "t02::error-surfaced-records",
        all(path is not None and Path(path).is_file() for path, _ in surfaced)
        and all(crash is not None and Path(crash).is_file() for _, crash in surfaced),
        1, detail={"failureRecords": [str(path) for path, _ in surfaced],
                   "crashLogs": [str(crash) for _, crash in surfaced]},
    )
    dialogs = [protocol_dialog, digest_dialog, runtime_dialog]
    producer.check(
        "t02::failure-dialog-suppressed-all",
        all(dialog["exists"] and dialog["codeMatches"] and dialog["modalShown"] is False
            and dialog["automation"] == "1" for dialog in dialogs),
        1,
        detail={
            "dialogs": dialogs,
            "note": "自动化模式（LINGXI_R09_AUTOMATION=1）不弹原生 modal，失败结构化写入 "
                    "LINGXI_R09_HOST_DIALOG_SINK；生产默认仍弹 dialog（TCC 像素观察为 "
                    "BLOCKED-SYSTEM-PERMISSIONS，不冒充）",
        },
    )

    # ── A03：READY 超时界限（Rust 集成测试注入短 Duration，不跑 90s 真实等待）──
    producer.run_step(
        "cargo-supervisor-tests",
        [rustup(), "run", "1.98.1", "cargo", "test",
         "--manifest-path", "desktop/src-tauri/Cargo.toml", "--locked",
         "--features", "r09-host,e2e-test",
         "--test", "service_supervisor", "--", "--nocapture"],
        timeout=1800,
        env={"CARGO_TARGET_DIR": str(producer.runtime / "t02-supervisor-cargo-target")},
    )
    producer.check("t02::ready-timeout-bounded", True, 1, detail={
        "evidence": "cargo test service_supervisor::ready_timeout_kills_spawned_child_and_records_failure "
                    "(injected 600ms duration; asserts child reaped + failure record)",
    })
    host_surfaces(producer)
    cleanup_runtime_processes(producer)
    producer.assert_real_home_untouched()
    return producer.finish(extra={
        "failures": {
            "protocolMismatch": record,
            "digestMismatch": digest_record,
            "runtimeIdentityMismatch": fake_record,
        },
        "releaseManifest": release_receipt.get("service"),
        "platform": "macos-arm64",
        "injectedRoot": str(injected_root),
    })


def write_incompatible_fake_service(producer: Producer, home: Path) -> Path:
    script = producer.raw / "fake-incompatible-service.py"
    script.write_text(
        r'''#!/usr/bin/env python3
import json, os, signal, socket, sys, threading

def main():
    home = None
    args = sys.argv[1:]
    for index, arg in enumerate(args):
        if arg == "--home" and index + 1 < len(args):
            home = os.path.realpath(args[index + 1])
    if home is None:
        return 2
    runtime = os.path.join(home, "lingxi-service")
    os.makedirs(runtime, exist_ok=True)
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(16)
    addr = "127.0.0.1:%d" % listener.getsockname()[1]
    instance_id = "0123456789abcdef0123456789abcdef"
    record = {
        "schemaVersion": 1, "serverKind": "lingxi-service", "serverVersion": "0.0.0",
        "wireProtocolMin": 1, "wireProtocolMax": 3, "dataEpoch": 2,
        "instanceId": instance_id, "startNonce": "fake", "pid": os.getpid(),
        "bindAddr": addr, "transport": "http", "startedAtUnixMs": 1, "homePath": home,
    }
    with open(os.path.join(runtime, "instance.json"), "w", encoding="utf-8") as handle:
        json.dump(record, handle)
    token = {"schemaVersion": 1, "kind": "local_token",
             "token": "fedcba9876543210fedcba9876543210",
             "instanceId": instance_id, "createdAtUnixMs": 1}
    token_path = os.path.join(runtime, "local-token.json")
    with open(token_path, "w", encoding="utf-8") as handle:
        json.dump(token, handle)
    os.chmod(token_path, 0o600)
    print("LINGXI_SERVICE_READY addr=%s home=%s source=--home" % (addr, home), flush=True)
    def stop(_signum, _frame):
        listener.close()
        sys.exit(0)
    signal.signal(signal.SIGTERM, stop)
    def serve():
        while True:
            try:
                connection, _ = listener.accept()
                connection.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                connection.close()
            except OSError:
                return
    threading.Thread(target=serve, daemon=True).start()
    while True:
        signal.pause()

main()
''', encoding="utf-8")
    script.chmod(0o755)
    return script


def suite_process_ownership(producer: Producer) -> int:
    debug_receipt = ensure_debug_build(producer, producer.evidence)
    hidden = hidden_only()
    release_receipt = ({"artifacts": {"appBundle": None, "executable": None}}
                       if hidden else ensure_release_build(producer, producer.evidence))
    debug_binary = Path(debug_receipt["artifacts"]["executable"])
    release_app = Path("/nonexistent-release-app") if hidden else Path(release_receipt["artifacts"]["appBundle"])
    release_executable = (Path("/nonexistent-release-exe") if hidden
                          else Path(release_receipt["artifacts"]["executable"]))

    remote = start_standalone_service(producer, runtime_home(producer, "home-remote"), "remote")
    try:
        # ── A04 核心：本地 owned + 远端并存，退出只清自有树 ──
        local_home = runtime_home(producer, "home-own-local")
        launched = launch_hidden_tauri(producer, debug_binary, local_home, producer.tmp)
        try:
            ready_app = wait_home_event(producer, local_home, "app-ready", launched, 150)
            s1 = sample_process_tree(producer, "s1", local_home, launched["process"].pid)
            events = events_of(local_home, launched["log"])
            service_ready = next((event for event in events if event.get("event") == "service-ready"), None)
            local_pid = (service_ready or {}).get("pid")
            local_service_entries = [entry for entry in s1["allServices"] if str(local_home) in entry["command"]]
            remote_service_entries = [entry for entry in s1["allServices"] if str(remote["home"]) in entry["command"]]
            app_pid = launched["process"].pid
            # 远端夹具是 harness 的直接子进程：S1 里记录它的真实 ppid（不是自身 pid）。
            remote_ppid_before = remote_parent_pid(remote_service_entries)
            remote_instance_before = sha256_file(remote["home"] / "lingxi-service/instance.json")
            producer.check(
                "t02::two-services-coexist",
                ready_app is not None and len(local_service_entries) == 1
                and len(remote_service_entries) == 1
                and local_service_entries[0]["ppid"] == app_pid,
                1, detail={"local": local_service_entries, "remote": remote_service_entries,
                           "appPid": app_pid},
            )
            # 退出窗口采样：无额外 spawn。
            monitoring = {"samples": [], "new_services": []}
            launched["process"].send_signal(signal.SIGTERM)
            deadline = time.time() + 2.0
            while time.time() < deadline:
                current = [entry for entry in ps_table() if str(local_home) in entry["command"]]
                monitoring["samples"].append([entry["pid"] for entry in current])
                for entry in current:
                    if entry["pid"] != local_pid and entry["pid"] not in monitoring["new_services"]:
                        monitoring["new_services"].append(entry["pid"])
                time.sleep(0.1)
            exit_stopped = wait_exit(launched, 45)
            time.sleep(1.0)
            s3 = sample_process_tree(producer, "s3", local_home, app_pid)
            events_after = events_of(local_home, launched["log"])
            shutdown = next((event for event in events_after if event.get("event") == "shutdown"), None)
            local_after = [entry for entry in s3["allServices"] if str(local_home) in entry["command"]]
            remote_after = [entry for entry in s3["allServices"] if str(remote["home"]) in entry["command"]]
            remote_identity_after = wait_identity(remote["port"], remote["token"], timeout=10)
            remote_instance_after = sha256_file(remote["home"] / "lingxi-service/instance.json")
        finally:
            if launched["process"].poll() is None:
                launched["process"].kill()
                launched["process"].wait(timeout=10)
            launched["handle"].close()
        producer.check("t02::exit-owned-tree-only",
                       exit_stopped.get("exit") == 0 and not exit_stopped.get("timedOut")
                       and not local_after and not monitoring["new_services"]
                       and shutdown is not None and (shutdown or {}).get("mode") == "local-owned"
                       and (shutdown or {}).get("confirmed") is True
                       and (shutdown or {}).get("residual") == []
                       and local_pid in (shutdown or {}).get("ownedPids", []),
                       1, detail={"exit": exit_stopped, "shutdown": shutdown, "localAfter": local_after,
                                  "newServices": monitoring["new_services"]})
        assert_ps_clean(producer, "ownership-owned+remote", [app_pid])
        producer.check("t02::remote-untouched",
                       len(remote_after) == 1 and remote_after[0]["pid"] == remote["process"].pid
                       and remote_after[0]["ppid"] == remote_ppid_before
                       and remote_identity_after is not None
                       and remote_instance_after == remote_instance_before,
                       1, detail={"remotePid": remote["process"].pid, "remoteAfter": remote_after,
                                  "ppidBefore": remote_ppid_before, "instanceShaBefore": remote_instance_before,
                                  "instanceShaAfter": remote_instance_after})

        # ── A04：复用实例 detached（dev 允许复用，退出不杀） ──
        reuse_home = runtime_home(producer, "home-reused")
        reuse = start_standalone_service(producer, reuse_home, "reused")
        try:
            launched_reuse = launch_hidden_tauri(producer, debug_binary, reuse_home, producer.tmp)
            try:
                ready_reuse = wait_home_event(producer, reuse_home, "app-ready", launched_reuse, 150)
                events_reuse = events_of(reuse_home, launched_reuse["log"])
                ready_mode = next((event for event in events_reuse if event.get("event") == "service-ready"), None)
                launched_reuse["process"].send_signal(signal.SIGTERM)
                exit_reuse = wait_exit(launched_reuse, 45)
                time.sleep(0.5)
                events_reuse = events_of(reuse_home, launched_reuse["log"])
                shutdown_reuse = next((event for event in events_reuse if event.get("event") == "shutdown"), None)
                reuse_alive = reuse["process"].poll() is None
            finally:
                if launched_reuse["process"].poll() is None:
                    launched_reuse["process"].kill()
                    launched_reuse["process"].wait(timeout=10)
                launched_reuse["handle"].close()
            assert_ps_clean(producer, "ownership-reused", [launched_reuse["process"].pid])
            producer.check("t02::reused-is-detached",
                           ready_reuse is not None
                           and (ready_mode or {}).get("mode") == "local-reused"
                           and exit_reuse.get("exit") == 0
                           and reuse_alive
                           and shutdown_reuse is not None
                           and (shutdown_reuse or {}).get("mode") == "local-reused"
                           and (shutdown_reuse or {}).get("ownedPids") == [],
                           1, detail={"serviceReady": ready_mode, "shutdown": shutdown_reuse,
                                      "reusedAlive": reuse_alive, "exit": exit_reuse})
        finally:
            stop_standalone(reuse)

        # ── A04：remote 模式结构性零接触（debug 构建允许远端） ──
        token_file = producer.raw / "remote-token.json"
        token_file.write_text(json.dumps({"token": remote["token"]}) + "\n", encoding="utf-8")
        token_file.chmod(0o600)
        remote_local_home = runtime_home(producer, "home-remote-client")
        remote_env = {
            "LINGXI_DESKTOP_SERVER_RUNTIME": "rust",
            "LINGXI_DESKTOP_RUST_REMOTE_URL": f"http://127.0.0.1:{remote['port']}",
            "LINGXI_DESKTOP_RUST_REMOTE_TOKEN_FILE": str(token_file),
        }
        launched_remote = launch_hidden_tauri(producer, debug_binary, remote_local_home, producer.tmp,
                                          extra_env=remote_env)
        try:
            ready_remote = wait_home_event(producer, remote_local_home, "app-ready", launched_remote, 150)
            events_remote = events_of(remote_local_home, launched_remote["log"])
            ready_mode = next((event for event in events_remote if event.get("event") == "service-ready"), None)
            local_instance_before_exit = (remote_local_home / "lingxi-service/instance.json").exists()
            launched_remote["process"].send_signal(signal.SIGTERM)
            exit_remote = wait_exit(launched_remote, 45)
            events_remote = events_of(remote_local_home, launched_remote["log"])
            shutdown_remote = next((event for event in events_remote if event.get("event") == "shutdown"), None)
            remote_alive_after = remote["process"].poll() is None
            identity_after = wait_identity(remote["port"], remote["token"], timeout=10)
        finally:
            if launched_remote["process"].poll() is None:
                launched_remote["process"].kill()
                launched_remote["process"].wait(timeout=10)
            launched_remote["handle"].close()
        assert_ps_clean(producer, "ownership-remote", [launched_remote["process"].pid])
        producer.check("t02::remote-mode-zero-touch",
                       ready_remote is not None and (ready_mode or {}).get("mode") == "remote"
                       and not local_instance_before_exit
                       and exit_remote.get("exit") == 0
                       and shutdown_remote is not None
                       and (shutdown_remote or {}).get("mode") == "remote"
                       and (shutdown_remote or {}).get("touchedRemote") is False
                       and (shutdown_remote or {}).get("ownedPids") == []
                       and remote_alive_after and identity_after is not None,
                       1, detail={"serviceReady": ready_mode, "shutdown": shutdown_remote,
                                  "remoteAlive": remote_alive_after, "exit": exit_remote})

        if hidden_only():
            return visible_pending_guard(producer, [
                "ownership-release-refuses-remote", "ownership-pid-reuse", "ownership-packaged-exit-graceful"],
                {"suite": "r09_t02_process_ownership"})

        # ── A04：release 拒绝远端（dev-only 边界），零连接/零 spawn ──
        release_home = runtime_home(producer, "home-release-remote-refusal")
        launched_refuse = launch_release_app(producer, release_executable, release_app.parent, release_home,
                                             extra_env=remote_env, label="release-refuses-remote")
        exit_refuse = wait_exit(launched_refuse, 60)
        refuse_record, refuse_path, _ = failure_record_from(launched_refuse, release_home)
        reap_home_services(release_home)
        assert_ps_clean(producer, "ownership-package-refuses-remote", [launched_refuse["process"].pid])
        producer.check("t02::packaged-refuses-remote",
                       exit_refuse.get("exit") not in (0, None)
                       and refuse_record is not None
                       and (refuse_record or {}).get("reason") == "REMOTE_REQUIRES_DEV_BUILD"
                       and not service_pids_for_home(release_home),
                       1, detail={"exit": exit_refuse, "record": refuse_record, "recordPath": str(refuse_path)})

        # ── A04：PID 复用安全（伪造记录指向无害存活进程，不误杀） ──
        pid_home = runtime_home(producer, "home-pid-reuse")
        sleeper = subprocess.Popen(["/bin/sleep", "120"])
        try:
            runtime_dir = pid_home / "lingxi-service"
            runtime_dir.mkdir(parents=True, exist_ok=True)
            forged = {
                "schemaVersion": 1, "serverKind": "lingxi-service", "serverVersion": "0.0.0",
                "wireProtocolMin": 1, "wireProtocolMax": 1, "dataEpoch": 2,
                "instanceId": "0123456789abcdef0123456789abcdef", "startNonce": "forged",
                "pid": sleeper.pid, "bindAddr": "127.0.0.1:43199", "transport": "http",
                "startedAtUnixMs": 1, "homePath": str(pid_home),
            }
            (runtime_dir / "instance.json").write_text(json.dumps(forged) + "\n", encoding="utf-8")
            launched_pid = launch_release_app(producer, release_executable, release_app.parent, pid_home,
                                              label="pid-reuse")
            exit_pid = wait_exit(launched_pid, 60)
            pid_record, pid_record_path, _ = failure_record_from(launched_pid, pid_home)
            sleeper_alive = sleeper.poll() is None
        finally:
            if sleeper.poll() is None:
                sleeper.kill()
                sleeper.wait(timeout=10)
        reap_home_services(pid_home)
        assert_ps_clean(producer, "ownership-pid-reuse", [launched_pid["process"].pid])
        producer.check("t02::pid-reuse-safe",
                       exit_pid.get("exit") not in (0, None)
                       and sleeper_alive
                       and pid_record is not None
                       and (pid_record or {}).get("reason") == "RUST_SERVICE_INSTANCE_INVALID",
                       1, detail={"exit": exit_pid, "record": pid_record, "sleeperAlive": sleeper_alive,
                                  "recordPath": str(pid_record_path)})
    finally:
        stop_standalone(remote)
        for pid in [entry["pid"] for entry in ps_table() if "lingxi-r09" in entry["command"] and "lingxi-service" in entry["command"]]:
            try:
                os.kill(pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
    cleanup_runtime_processes(producer)
    producer.assert_real_home_untouched()
    orphan_probe = [entry for entry in ps_table()
                    if "lingxi-service" in entry["command"] and str(producer.runtime) in entry["command"]]
    producer.check("t02::no-orphan-owned-services", not orphan_probe, 1,
                   detail={"orphans": orphan_probe, "marker": str(producer.runtime),
                           "note": "finally 级 kill/SIGKILL 后按进程表断言零残留（含 release sidecar）；"
                                   "宿主被 SIGKILL 时 macOS 无父死子亡原语，属已知边界——"
                                   "优雅退出路径由 shutdown 报告 residual=[] 覆盖"})
    return producer.finish(extra={
        "platform": "macos-arm64",
        "remoteFixture": {"pid": remote["process"].pid, "port": remote["port"], "home": str(remote["home"])},
    })


SUITES = {
    "r09_t02_supervisor_ready": suite_supervisor_ready,
    "r09_t02_process_ownership": suite_process_ownership,
}


def run_suite(suite: str, evidence: Path) -> int:
    producer = Producer(suite, evidence)
    implementation = SUITES.get(suite)
    if implementation is None:
        return producer.fail(f"unknown r09_t02 suite: {suite}")
    try:
        return implementation(producer)
    except EvidenceError as error:
        producer.check(f"{suite}::producer", False, detail={"error": str(error)})
        producer.logs.append(f"\n[producer-error] {error}\n")
        try:
            cleanup_runtime_processes(producer)
        except Exception as cleanup_error:  # noqa: BLE001
            producer.logs.append(f"[cleanup-error] {cleanup_error}")
        return producer.finish("FAIL", extra={"failureDetail": str(error)})
    except Exception as error:  # noqa: BLE001 - 生产者的任何异常都必须变成红
        detail = f"{type(error).__name__}: {error}"
        producer.check(f"{suite}::producer", False, detail={"error": detail, "traceback": traceback.format_exc()[-4000:]})
        producer.logs.append(f"\n[producer-exception] {detail}\n{traceback.format_exc()}\n")
        try:
            cleanup_runtime_processes(producer)
        except Exception as cleanup_error:  # noqa: BLE001
            producer.logs.append(f"[cleanup-error] {cleanup_error}")
        return producer.finish("FAIL", extra={"failureDetail": detail})


if __name__ == "__main__":
    raise SystemExit("r09_t02.py 由 r09_suites.py 调度")

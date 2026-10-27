#!/usr/bin/env python3
"""R09-T04 证据生产者：语音/屏幕/辅助功能权限（A07 撤销后不采集 / A08 远端边界）。

套件（由 `r09_suites.py --suite <name>` 分派）：
  - `r09_t04_permission_matrix`：权限状态机（TCC 查询/请求、helper 租约、
    语音六态与采集会话取消/撤销/设备缺失）+ **真实 lingxi-service HTTP**：
    computer-use 偏好/批准/撤销/读取 + app-events 事件 + 负向分支。
  - `r09_t04_remote_boundary`：A08 远端设备边界（远端主体无本机授权不能读设备）
    + 真实服务鉴权边（未认证设备相关路由拒绝）。

不做（如实阻塞，套件 exit 2 门真红）：
  - 真实 TCC 授权弹窗/撤销-恢复（BLOCKED-SYSTEM-PERMISSIONS，需人工在系统设置
    操作；本生产者只跑只读查询与不触弹窗的负向分支，绝不自动触发 TCC）；
  - `request-permissions` 正向（mac 会弹真实 TCC）/真实输入注入 helper（服务当前
    明确 `computer_control_engine_unavailable`，无 helper）；
  - Tauri 宿主注册后的端到端（BLOCKED_PENDING_REGISTRATION）；
  - 页截图捕获（OQ-04）；真实 ASR 识别结果（BLOCKED_REAL_PROVIDER）；
  - Linux Preview 负向真机（BLOCKED_PLATFORM_LINUX_X64）。

用法：
  python3 -B scripts/rust-tauri/r09_t04.py --suite r09_t04_permission_matrix \
      --evidence /private/tmp/r09-t04-perm-01
"""

from __future__ import annotations

import json
import os
import re
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[1]
sys.path.insert(0, str(SCRIPTS))

from r09_producer import (  # noqa: E402
    Producer,
    run,
    rustup,
    sha256_file,
    toolchain,
)

NATIVE_DIR = ROOT / "desktop/src-tauri/src/native"
SERVICE = ROOT / "rust/target/debug/lingxi-service"
HARNESS_CRATE = "native_host_harness"
COMPUTER_USE_ROUTE = "/lingxi/v1/preferences/computer-use"
APPROVALS_ROUTE = "/lingxi/v1/preferences/computer-use/approvals"
REQUEST_PERMISSIONS_ROUTE = "/lingxi/v1/preferences/computer-use/request-permissions"
APP_EVENTS_ROUTE = "/lingxi/v1/app-events?since=0"


# ── 通用工具（与 r09_t03.py 同构；每个生产者自带以便独立运行）──────────


def build_native_harness(producer: Producer) -> Path:
    target = producer.evidence / "harness"
    target.mkdir(parents=True, exist_ok=True)
    root_src = target / "lib.rs"
    root_src.write_text(
        f'#[path = "{(NATIVE_DIR / "mod.rs").resolve()}"]\npub mod native;\n',
        encoding="utf-8")
    binary = target / HARNESS_CRATE
    producer.run_step(
        "build::native-harness",
        [rustup(), "run", toolchain(), "rustc", "--test", "--edition", "2021",
         "--cfg", "r09_standalone_test",
         "--crate-name", HARNESS_CRATE, str(root_src), "-o", str(binary)],
        timeout=600,
        env={"CARGO_TARGET_DIR": str(target / "target")},
    )
    return binary


def run_rust_tests(producer: Producer, binary: Path, filter_: str) -> str:
    process = producer.run_step(
        f"test::{filter_}", [str(binary), "--test-threads", "1", filter_], timeout=600)
    return process.stdout + process.stderr


def rust_case(producer: Producer, binary: Path, case: str, test: str) -> bool:
    process = run([str(binary), "--exact", test, "--test-threads", "1"],
                  timeout=600, env=producer.env())
    output = process.stdout + process.stderr
    ok = process.returncode == 0 and f"test {test} ... ok" in output
    return producer.check(case, ok, detail={"test": test, "exitCode": process.returncode})


def http(method: str, url: str, headers: dict | None = None,
         body: dict | None = None, timeout: float = 8.0) -> dict:
    data = None
    merged = dict(headers or {})
    if body is not None:
        data = json.dumps(body).encode()
        merged["Content-Type"] = "application/json"
    request = urllib.request.Request(url, method=method, headers=merged, data=data)
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return {
                "status": response.status,
                "body": response.read().decode("utf-8", "replace"),
            }
    except urllib.error.HTTPError as error:
        return {
            "status": error.code,
            "body": error.read().decode("utf-8", "replace"),
        }
    except urllib.error.URLError as error:
        return {"status": 0, "body": f"URLError: {error.reason}"}


def wait_ready(process: subprocess.Popen, log_path: Path, timeout: float = 90.0) -> str:
    deadline = time.time() + timeout
    buffer = ""
    while time.time() < deadline:
        line = process.stdout.readline()
        if not line:
            if process.poll() is not None:
                break
            continue
        buffer += line
        log_path.write_text(buffer, encoding="utf-8")
        if line.startswith("LINGXI_SERVICE_READY "):
            return buffer
    raise RuntimeError(f"service did not emit READY within {timeout}s: {buffer[-2000:]}")


def start_isolated_service(producer: Producer):
    """隔离 home 启动真实 lingxi-service（空 home；返回 base/bearer/process/日志）。"""
    home = producer.home
    log_path = producer.evidence / "service-stdout.log"
    stderr_path = producer.evidence / "service-stderr.log"
    env = producer.env({"RUST_BACKTRACE": "1"})
    if not SERVICE.is_file():
        raise RuntimeError(f"服务二进制不存在: {SERVICE}（先构建 rust/target/debug/lingxi-service）")
    with log_path.open("w") as out, stderr_path.open("w") as err:
        process = subprocess.Popen(
            [str(SERVICE), "--home", str(home)],
            cwd=str(SERVICE.parent), env=env,
            stdout=subprocess.PIPE, stderr=err, text=True, bufsize=1)
    ready = wait_ready(process, log_path)
    address = None
    for token in ready.replace("=", " ").split():
        if ":" in token and token.split(":")[0] in ("127.0.0.1", "localhost"):
            address = token
    if address is None:
        raise RuntimeError(f"无法从 READY 解析地址: {ready!r}")
    instance = json.loads((home / "lingxi-service/instance.json").read_text(encoding="utf-8"))
    token_record = json.loads(
        (home / "lingxi-service/local-token.json").read_text(encoding="utf-8"))
    bearer = token_record.get("token") or token_record.get("accessToken")
    if not bearer:
        raise RuntimeError(f"local-token.json 无 bearer: {token_record}")
    return process, f"http://{address}", bearer, instance


def stop_service(producer: Producer, process: subprocess.Popen) -> bool:
    if process.poll() is not None:
        return True
    process.send_signal(signal.SIGTERM)
    try:
        exit_code = process.wait(timeout=40)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=10)
        exit_code = None
    time.sleep(0.4)
    alive = False
    try:
        os.kill(process.pid, 0)
        alive = True
    except ProcessLookupError:
        alive = False
    producer.check(
        "service::terminated-no-residual",
        not alive,
        detail={"exitCode": exit_code, "pid": process.pid},
    )
    return not alive


def auth_headers(bearer: str) -> dict:
    return {"Authorization": f"Bearer {bearer}", "Accept": "application/json"}


def app_events_since(base: str, bearer: str, since: int) -> list[dict]:
    response = http("GET", f"{base}/lingxi/v1/app-events?since={since}",
                    auth_headers(bearer))
    if response["status"] != 200:
        return []
    try:
        payload = json.loads(response["body"])
    except json.JSONDecodeError:
        return []
    events = payload.get("events")
    return events if isinstance(events, list) else []


def find_event(events: list[dict], event_type: str, predicate=None) -> dict | None:
    for event in events:
        if event.get("type") != event_type:
            continue
        if predicate is None or predicate(event.get("payload") or {}):
            return event
    return None


# ── 套件：权限矩阵 ────────────────────────────────────────────────────


LEAF_UNIT_CASES: dict[str, str] = {
    "leaf::computer-use.lease-requires-approval":
        "native::permissions::tests::lease_requires_real_approval_and_permissions",
    "leaf::computer-use.remote-session-denied":
        "native::permissions::tests::remote_sessions_can_never_start_leases",
    "leaf::computer-use.lease-expiry-and-controls":
        "native::permissions::tests::actions_are_gated_by_lease_expiry_permissions_and_controls",
    "leaf::computer-use.revoke-invalidates-lease":
        "native::permissions::tests::stop_revoke_and_app_exit_invalidate_leases",
    "leaf::computer-use.permission-query-read-only":
        "native::permissions::tests::query_is_read_only_and_window_gated",
    "leaf::computer-use.request-terminal-no-reprompt":
        "native::permissions::tests::request_only_prompts_in_not_determined_and_user_gesture_window",
    "leaf::computer-use.platform-linux-preview":
        "native::permissions::tests::platform_and_caller_gates",
    "leaf::speech.permission-state-mapping":
        "native::speech::tests::raw_status_mapping_matches_electron",
    "leaf::speech.request-single-flight":
        "native::speech::tests::request_policy_first_terminal_devshell_and_single_flight",
    "leaf::speech.dispose-fail-closed":
        "native::speech::tests::dispose_settles_as_bridge_unavailable_and_late_callbacks_are_dropped",
    "leaf::speech.revoked-during-recording":
        "native::speech::tests::revocation_during_recording_stops_and_reports",
    "leaf::speech.cancel-timeout-not-success":
        "native::speech::tests::cancel_and_timeout_never_count_as_success",
    "leaf::speech.ui-states":
        "native::speech::tests::ui_states_cover_first_denied_restricted_missing_cancel",
    "leaf::screenshot.theme-and-save-policy":
        "native::screenshot::tests::theme_table_matches_incumbent_six_themes",
    "leaf::screenshot.preview-vs-save-shape":
        "native::screenshot::tests::file_names_and_result_shapes",
}



# ── T04 最终逐叶绑定计划（Lead 要求 §5.1 最终形式）──────────────────────
# 9 叶：4 叶可绑定（真实服务 HTTP + app-events 证据已 PASS）；其余 5 叶按原因阻塞/人工。
# 每条原断言 → case 列表；`status` = bindable | manual | blocked。
T04_BINDING_PLAN = {
    "R00-T02-LA-EF9EEE38D8F6": {
        "title": "读取电脑操作偏好",
        "assertions": [
            ["leaf::computer-use.prefs-read", "leaf::computer-use.permission-subject-and-names"],
            ["leaf::computer-use.prefs-update-invalid-bounded",
             "leaf::computer-use.unauthenticated-denied", "leaf::computer-use.platform-linux-preview"],
        ],
        "status": "bindable",
    },
    "R00-T02-LA-6E8307F57F42": {
        "title": "更新电脑操作偏好",
        "assertions": [
            ["leaf::computer-use.prefs-update", "leaf::computer-use.engine-unavailable-honest"],
            ["leaf::computer-use.prefs-update-invalid-bounded",
             "leaf::computer-use.platform-linux-preview"],
        ],
        "status": "bindable",
    },
    "R00-T02-LA-3FA31D4C3100": {
        "title": "批准电脑操作应用",
        "assertions": [
            ["leaf::computer-use.approve-app"],
            ["leaf::computer-use.approval-invalid-body", "leaf::computer-use.unauthenticated-denied"],
        ],
        "status": "bindable",
    },
    "R00-T02-LA-38B14B7B34BD": {
        "title": "撤销电脑操作应用批准",
        "assertions": [
            ["leaf::computer-use.revoke-app"],
            ["leaf::computer-use.approval-invalid-body", "leaf::computer-use.unauthenticated-denied"],
        ],
        "status": "bindable",
    },
    "R00-T02-LA-CA446FE1DB71": {
        "title": "申请电脑操作系统权限",
        "assertions": [
            [],
            ["leaf::computer-use.request-permissions-negative",
             "leaf::computer-use.platform-linux-preview"],
        ],
        "status": "manual",
        "manualSteps": "正向请求会触发真实 TCC 弹窗（BLOCKED-SYSTEM-PERMISSIONS）："
                       "人工在系统设置中授权/拒绝，并记录屏幕录制 + tccutil/系统设置截图；"
                       "负向分支已由真实服务 HTTP 自动覆盖。",
    },
    "R00-T02-LA-D34F2D718CF3": {
        "title": "打包壳显示系统授权弹窗（Speech）",
        "assertions": [
            ["leaf::speech.permission-state-mapping"],
            ["leaf::speech.request-single-flight", "leaf::speech.dispose-fail-closed"],
        ],
        "status": "blocked",
        "blockedReason": "真实 Speech TCC 授权弹窗需正式签名身份 + 人工操作"
                         "（BLOCKED-SIGNING-IDENTITY / BLOCKED-SYSTEM-PERMISSIONS）；"
                         "状态机/单飞/fail-closed 已由 Rust 单测覆盖。",
    },
    "R00-T02-LA-FD5B420182C6": {
        "title": "语音识别结果（取消/超时不能记成功）",
        "assertions": [
            ["leaf::speech.cancel-timeout-not-success"],
            ["leaf::speech.cancel-timeout-not-success", "leaf::speech.ui-states"],
        ],
        "status": "blocked",
        "blockedReason": "真实语音识别结果需 ASR 供应商/本地识别链（BLOCKED-REAL-PROVIDER；"
                         "Rust 服务 system-speech-recognition 显式 refusal）。",
    },
    "R00-T02-LA-D2575672A18D": {
        "title": "computer-use helper 租约与真实控制",
        "assertions": [
            ["leaf::computer-use.lease-requires-approval"],
            ["leaf::computer-use.lease-expiry-and-controls"],
            ["leaf::computer-use.revoke-invalidates-lease"],
            ["leaf::computer-use.remote-session-denied"],
        ],
        "status": "blocked",
        "blockedReason": "服务报告 computer_control_engine_unavailable 且无输入注入 helper；"
                         "「获批应用发生预期改变/撤权后控制失败」需 helper 落地后才能真实观察。",
    },
    "R00-T02-LA-54DC0A05A0DE": {
        "title": "页截图渲染（preview base64 / 保存 PNG）",
        "assertions": [
            ["leaf::screenshot.preview-vs-save-shape"],
            ["leaf::screenshot.theme-and-save-policy"],
        ],
        "status": "blocked",
        "blockedReason": "Tauri/Wry 无稳定页面捕获 API（OQ-04）；策略层已实现，捕获执行 BLOCKED。",
    },
}


def validate_t04_binding_plan(producer: Producer) -> int:
    """校验 T04 最终绑定计划的 case 真实存在且 PASS（bindable 叶）/原因完整（manual/blocked）。"""
    if not producer.cases:
        producer.check("t04::binding-plan-has-evidence", False)
        return 1
    observed = {case["case"]: case["ok"] for case in producer.cases}
    missing = []
    failed = []
    for leaf_id, plan in T04_BINDING_PLAN.items():
        for group in plan["assertions"]:
            for case in group:
                if case not in observed:
                    missing.append({"leaf": leaf_id, "case": case})
                elif not observed[case]:
                    failed.append({"leaf": leaf_id, "case": case})
        if plan["status"] == "manual" and not plan.get("manualSteps"):
            missing.append({"leaf": leaf_id, "reason": "manual-without-steps"})
        if plan["status"] == "blocked" and not plan.get("blockedReason"):
            missing.append({"leaf": leaf_id, "reason": "blocked-without-reason"})
    producer.check("t04::binding-plan-cases-observed-and-pass",
                   not missing and not failed,
                   detail={"missing": missing[:10], "failed": failed[:10]})
    bindable = [leaf for leaf, plan in T04_BINDING_PLAN.items() if plan["status"] == "bindable"]
    total_groups = sum(len(plan["assertions"]) for plan in T04_BINDING_PLAN.values())
    producer.check("t04::binding-plan-shape",
                   len(T04_BINDING_PLAN) == 9 and len(bindable) == 4
                   and total_groups == 20,
                   detail={"leaves": len(T04_BINDING_PLAN), "bindable": bindable,
                           "assertionGroups": total_groups})
    return 0

def run_permission_matrix(producer: Producer) -> int:
    binary = build_native_harness(producer)

    # 1) T04 相关模块全量单测真实退出码
    module_tests = {
        "permissions": "native::permissions::tests",
        "speech": "native::speech::tests",
        "remote_boundary": "native::remote_boundary::tests",
        "screenshot": "native::screenshot::tests",
    }
    for name, filter_ in module_tests.items():
        output = run_rust_tests(producer, binary, filter_)
        match = re.search(r"test result: ok\. (\d+) passed; 0 failed", output)
        producer.check(
            f"native::{name}-unit-tests",
            match is not None and int(match.group(1)) > 0,
            detail={"passed": int(match.group(1)) if match else 0},
        )

    # 2) 逐 leaf case（供 Lead 登记 FULL_BINDINGS）
    for case, test in LEAF_UNIT_CASES.items():
        rust_case(producer, binary, case, test)

    # 3) 真实服务 HTTP：computer-use 偏好/批准/撤销/读取 + app-events
    process, base, bearer, instance = start_isolated_service(producer)
    try:
        producer.check("service::identity", bool(instance.get("pid")),
                       detail={"instance": {k: instance.get(k) for k in
                                            ("serverKind", "pid", "bindAddr", "instanceId")}})

        # 3.1 读取偏好（不改动设置）
        get_before = http("GET", base + COMPUTER_USE_ROUTE, auth_headers(bearer))
        body_before = json.loads(get_before["body"]) if get_before["status"] == 200 else {}
        settings_before = body_before.get("settings") or {}
        producer.check(
            "leaf::computer-use.prefs-read",
            get_before["status"] == 200
            and settings_before.get("enabled") is False
            and isinstance(settings_before.get("app_approvals"), list)
            and settings_before.get("app_approvals") == []
            and body_before.get("selectedProviderId") == "macos:cua"
            and (body_before.get("status") or {}).get("permissionSubject") is None
            or (body_before.get("status") or {}).get("providers", [{}])[0]
            .get("status", {}).get("permissionSubject") == "service_process",
            detail={"status": get_before["status"], "bodyHead": get_before["body"][:400]},
        )
        providers = ((body_before.get("status") or {}).get("providers") or [{}])
        provider_status = (providers[0].get("status") or {}) if providers else {}
        permission_names = {item.get("name") for item in provider_status.get("permissions", [])}
        producer.check(
            "leaf::computer-use.permission-subject-and-names",
            provider_status.get("permissionSubject") == "service_process"
            and permission_names == {"accessibility", "screenRecording"},
            detail={"permissionNames": sorted(permission_names),
                    "subject": provider_status.get("permissionSubject")},
        )

        # 3.2 更新偏好（PUT 布尔白名单 + 回读）
        put = http("PUT", base + COMPUTER_USE_ROUTE,
                   {**auth_headers(bearer), "Content-Type": "application/json"},
                   {"settings": {"enabled": True}})
        get_after = http("GET", base + COMPUTER_USE_ROUTE, auth_headers(bearer))
        get_after_body = json.loads(get_after["body"]) if get_after["status"] == 200 else {}
        producer.check(
            "leaf::computer-use.prefs-update",
            put["status"] == 200
            and json.loads(put["body"]).get("ok") is True
            and (get_after_body.get("settings") or {}).get("enabled") is True,
            detail={"putStatus": put["status"], "putBody": put["body"][:300]},
        )
        # 无效 body / 扩大写入 → 400（不写入）
        invalid_values = [
            {"settings": {"enabled": "yes"}},
            {"settings": {"enabled": True, "app_approvals": []}},
            {"settings": {}},
        ]
        invalid_ok = True
        invalid_detail = []
        for payload in invalid_values:
            response = http("PUT", base + COMPUTER_USE_ROUTE,
                            {**auth_headers(bearer), "Content-Type": "application/json"},
                            payload)
            invalid_detail.append({"payload": payload, "status": response["status"]})
            invalid_ok = invalid_ok and response["status"] == 400
        producer.check("leaf::computer-use.prefs-update-invalid-bounded", invalid_ok,
                       detail=invalid_detail)
        # 复原 enabled=false（避免副作用污染后续断言）
        restore = http("PUT", base + COMPUTER_USE_ROUTE,
                       {**auth_headers(bearer), "Content-Type": "application/json"},
                       {"settings": {"enabled": False}})
        producer.check("leaf::computer-use.prefs-restore", restore["status"] == 200)

        # 3.3 批准应用 → 设置持久化 + 设置变更事件
        events_before = len(app_events_since(base, bearer, 0))
        approve = http("POST", base + APPROVALS_ROUTE,
                       {**auth_headers(bearer), "Content-Type": "application/json"},
                       {"providerId": "macos:cua", "appId": "com.apple.TextEdit",
                        "appName": "TextEdit"})
        approve_body = json.loads(approve["body"]) if approve["status"] == 200 else {}
        approvals_after = ((approve_body.get("settings") or {}).get("app_approvals") or [])
        get_after_approve = http("GET", base + COMPUTER_USE_ROUTE, auth_headers(bearer))
        persisted_approvals = (
            ((json.loads(get_after_approve["body"]).get("settings") or {})
             .get("app_approvals") or [])
            if get_after_approve["status"] == 200 else []
        )
        events_after_approve = app_events_since(base, bearer, events_before)
        approve_event = find_event(
            events_after_approve, "computer-use-settings-changed",
            lambda payload: payload.get("appId") == "com.apple.TextEdit")
        producer.check(
            "leaf::computer-use.approve-app",
            approve["status"] == 200
            and any(item.get("appId") == "com.apple.TextEdit" for item in approvals_after)
            and any(item.get("appId") == "com.apple.TextEdit" for item in persisted_approvals)
            and approve_event is not None,
            detail={"approveStatus": approve["status"],
                    "approvals": persisted_approvals,
                    "event": approve_event},
        )

        # 3.4 撤销应用批准 → 移除 + 设置变更事件
        events_before_revoke = len(app_events_since(base, bearer, 0))
        revoke = http("DELETE", base + APPROVALS_ROUTE,
                      {**auth_headers(bearer), "Content-Type": "application/json"},
                      {"providerId": "macos:cua", "appId": "com.apple.TextEdit"})
        get_after_revoke = http("GET", base + COMPUTER_USE_ROUTE, auth_headers(bearer))
        approvals_after_revoke = (
            ((json.loads(get_after_revoke["body"]).get("settings") or {})
             .get("app_approvals") or [])
            if get_after_revoke["status"] == 200 else []
        )
        events_after_revoke = app_events_since(base, bearer, events_before_revoke)
        revoke_event = find_event(
            events_after_revoke, "computer-use-settings-changed",
            lambda payload: payload.get("appId") == "com.apple.TextEdit")
        producer.check(
            "leaf::computer-use.revoke-app",
            revoke["status"] == 200
            and all(item.get("appId") != "com.apple.TextEdit"
                    for item in approvals_after_revoke)
            and revoke_event is not None,
            detail={"revokeStatus": revoke["status"],
                    "approvals": approvals_after_revoke,
                    "event": revoke_event},
        )

        # 3.4b 批准/撤销的无效 body 负向（不写入、不删错记录）
        approve_invalid = [
            http("POST", base + APPROVALS_ROUTE,
                 {**auth_headers(bearer), "Content-Type": "application/json"}, {}),
            http("DELETE", base + APPROVALS_ROUTE,
                 {**auth_headers(bearer), "Content-Type": "application/json"}, {}),
        ]
        producer.check(
            "leaf::computer-use.approval-invalid-body",
            all(response["status"] == 400 for response in approve_invalid),
            detail=[{"status": response["status"]} for response in approve_invalid],
        )

        # 3.5 申请权限的负向分支（不触发 TCC 弹窗）
        mismatch = http("POST", base + REQUEST_PERMISSIONS_ROUTE,
                        {**auth_headers(bearer), "Content-Type": "application/json"},
                        {"providerId": "not-the-selected-provider"})
        mismatch_body = mismatch["body"]
        array_body = http("POST", base + REQUEST_PERMISSIONS_ROUTE,
                          {**auth_headers(bearer), "Content-Type": "application/json"},
                          [1, 2, 3])
        events_after_request = app_events_since(
            base, bearer, len(app_events_since(base, bearer, 0)))
        request_event = find_event(events_after_request,
                                   "computer-use-permissions-requested")
        producer.check(
            "leaf::computer-use.request-permissions-negative",
            mismatch["status"] == 400
            and "selected provider mismatch" in mismatch_body
            and array_body["status"] == 400
            and request_event is None,
            detail={"mismatchStatus": mismatch["status"],
                    "arrayStatus": array_body["status"],
                    "unexpectedEvent": request_event},
        )

        # 3.6 未认证访问拒绝（设备/偏好面不可匿名读）
        unauth = http("GET", base + COMPUTER_USE_ROUTE)
        producer.check("leaf::computer-use.unauthenticated-denied",
                       unauth["status"] in (401, 403),
                       detail={"status": unauth["status"]})

        # 3.7 服务对电脑操作引擎的诚实声明（helper 未实现 → 不得假装可用）
        engine = provider_status
        producer.check(
            "leaf::computer-use.engine-unavailable-honest",
            engine.get("available") is False
            and engine.get("reason") == "computer_control_engine_unavailable",
            detail={"providerStatus": engine},
        )
    finally:
        stop_service(producer, process)

    # 3.8) T04 最终绑定计划与真实证据一致性
    validate_t04_binding_plan(producer)

    # 4) 真实系统权限动作 → 显式阻塞（门真红，不冒充）
    blocked = [
        ("blocked::tcc.real-prompt-and-revoke",
         "真实 TCC 授权弹窗与撤销-恢复演练需人工在系统设置操作（BLOCKED-SYSTEM-PERMISSIONS）"),
        ("blocked::computer-use.helper-real-input",
         "服务报告 computer_control_engine_unavailable 且无输入注入 helper：点击/输入/按键/滚动真实生效在 helper 实现前不可验"),
        ("blocked::screenshot.capture-execution",
         "Tauri/Wry 无稳定页面捕获 API（OQ-04），捕获执行 BLOCKED；仅策略层可验"),
        ("blocked::asr.real-recognition-result",
         "真实语音识别结果需 ASR 供应商/本地识别链（BLOCKED-REAL-PROVIDER；服务 system-speech-recognition 显式 refusal）"),
        ("blocked::speech.packaged-tcc-prompt",
         "打包壳真实 Speech TCC 弹窗需正式签名身份 + 人工操作（BLOCKED-SIGNING-IDENTITY / BLOCKED-SYSTEM-PERMISSIONS）"),
        ("blocked::permission-matrix.linux-preview",
         "Linux Preview 负向真机不可用（BLOCKED-PLATFORM-LINUX-X64）；本机只验证 Rust 状态机返回 unsupported"),
        ("blocked::permission-matrix.registered-host-e2e",
         "T04 命令已注册（T06 收口）；端到端 invoke + TCC 桥需真实 GUI 会话 + 人工 TCC（Lead GUI 禁令 / BLOCKED_SYSTEM_PERMISSIONS）"),
    ]
    for case, reason in blocked:
        producer.check(case, False, detail={"blocked": True, "reason": reason})
    producer.assert_real_home_untouched()
    return producer.finish(status="FAIL", extra={
        "blockedPendingRegistration": [reason for _, reason in blocked],
        "blockedKind": "BLOCKED_SYSTEM_PERMISSIONS|BLOCKED_GUI_SMOKE|BLOCKED_REAL_PROVIDER",
        "serviceBinary": str(SERVICE.relative_to(ROOT)),
        "serviceSha256": sha256_file(SERVICE) if SERVICE.is_file() else None,
        "note": "真实服务 HTTP（approve/revoke/prefs/负向/app-events）与 Rust 状态机单测已真实执行；"
                "TCC/helper/截图/ASR/注册后 E2E 如实阻塞",
    })


# ── 套件：远端设备边界（A08）──────────────────────────────────────────

REMOTE_UNIT_CASES: dict[str, str] = {
    "leaf::remote-boundary.no-grant-requires-local-auth":
        "native::remote_boundary::tests::remote_request_without_grant_requires_local_authorization",
    "leaf::remote-boundary.remote-cannot-self-grant":
        "native::remote_boundary::tests::remote_cannot_grant_itself",
    "leaf::remote-boundary.grant-scope-and-ttl":
        "native::remote_boundary::tests::grant_scopes_device_subject_and_ttl",
    "leaf::remote-boundary.expired-grant-and-tcc-conjunctive":
        "native::remote_boundary::tests::expired_grant_does_not_allow_and_local_tcc_is_conjunctive",
    "leaf::remote-boundary.local-principals-follow-tcc":
        "native::remote_boundary::tests::local_principals_follow_host_permission_only",
    "leaf::remote-boundary.revoke-grant":
        "native::remote_boundary::tests::revoke_removes_grant_and_device_tcc_mapping_is_complete",
    "leaf::remote-boundary.helper-remote-denied":
        "native::permissions::tests::remote_sessions_can_never_start_leases",
    "leaf::remote-boundary.speech-caller-gated":
        "native::speech::tests::caller_and_platform_errors",
}


def run_remote_boundary(producer: Producer) -> int:
    binary = build_native_harness(producer)
    for name, filter_ in {
        "remote_boundary": "native::remote_boundary::tests",
        "permissions": "native::permissions::tests",
        "speech": "native::speech::tests",
    }.items():
        output = run_rust_tests(producer, binary, filter_)
        match = re.search(r"test result: ok\. (\d+) passed; 0 failed", output)
        producer.check(f"native::{name}-unit-tests", match is not None,
                       detail={"passed": int(match.group(1)) if match else 0})
    for case, test in REMOTE_UNIT_CASES.items():
        rust_case(producer, binary, case, test)

    process, base, bearer, instance = start_isolated_service(producer)
    try:
        # A08 服务边：设备/权限相关路由未认证一律拒绝（远端不能匿名读本机设备面）
        unauth_checks = {
            "leaf::remote-boundary.unauth-computer-use-read":
                http("GET", base + COMPUTER_USE_ROUTE),
            "leaf::remote-boundary.unauth-request-permissions":
                http("POST", base + REQUEST_PERMISSIONS_ROUTE,
                     {"Content-Type": "application/json"}, {"providerId": "x"}),
            "leaf::remote-boundary.unauth-app-events":
                http("GET", base + APP_EVENTS_ROUTE),
            "leaf::remote-boundary.unauth-media-transcribe":
                http("POST", base + "/lingxi/v1/media/asr/transcribe",
                     {"Content-Type": "application/json"}, {"audio": "AAAA"}),
        }
        for case, response in unauth_checks.items():
            producer.check(case, response["status"] in (401, 403),
                           detail={"status": response["status"]})
        # 本机 owner 仍可读 app-events（本地/远端边界的分界证据）
        local_events = http("GET", base + APP_EVENTS_ROUTE, auth_headers(bearer))
        producer.check("leaf::remote-boundary.local-owner-allowed",
                       local_events["status"] == 200,
                       detail={"status": local_events["status"]})
        # 远端 `request-permissions` 未认证时不得触发 TCC 事件（无权限请求副作用）
        events = app_events_since(base, bearer, 0)
        producer.check(
            "leaf::remote-boundary.no-remote-permission-request-event",
            find_event(events, "computer-use-permissions-requested") is None,
            detail={"events": [event.get("type") for event in events][:20]},
        )
        producer.check("service::identity", bool(instance.get("pid")))
    finally:
        stop_service(producer, process)

    blocked = [
        ("blocked::remote-boundary.two-host-e2e",
         "宿主已注册 + T02 远端模式已实现；双宿主端到端拒绝需真实 GUI 会话（Lead GUI 禁令）"),
        ("blocked::remote-boundary.remote-device-read-chain",
         "远端任务实际调用本机麦克风/屏幕的完整链（服务→宿主命令→设备）需 helper 与宿主注册；当前仅策略/鉴权边可验"),
        ("blocked::remote-boundary.tcc-revoke-mid-task",
         "任务进行中真实撤销屏幕/麦克风权限并观察采集停止需人工 TCC 操作（BLOCKED-SYSTEM-PERMISSIONS）"),
    ]
    for case, reason in blocked:
        producer.check(case, False, detail={"blocked": True, "reason": reason})
    producer.assert_real_home_untouched()
    return producer.finish(status="FAIL", extra={
        "blockedPendingRegistration": [reason for _, reason in blocked],
        "blockedKind": "BLOCKED_GUI_SMOKE|BLOCKED_SYSTEM_PERMISSIONS",
        "serviceBinary": str(SERVICE.relative_to(ROOT)),
        "serviceSha256": sha256_file(SERVICE) if SERVICE.is_file() else None,
    })


# ── 分派 ────────────────────────────────────────────────────────────────


def run_suite(suite: str, evidence: Path) -> int:
    producer = Producer(suite, evidence)
    try:
        if suite == "r09_t04_permission_matrix":
            code = run_permission_matrix(producer)
        elif suite == "r09_t04_remote_boundary":
            code = run_remote_boundary(producer)
        else:
            code = producer.fail(f"未知 T04 套件: {suite}")
        (producer.evidence / "runtime.json").write_text(json.dumps({
            "schema": "lingxi.r09.runtime.v1",
            "home": str(producer.home),
            "tmp": str(producer.tmp),
            "serviceBinary": str(SERVICE.relative_to(ROOT)),
            "serviceSha256": sha256_file(SERVICE) if SERVICE.is_file() else None,
            "t04Modules": ["permissions.rs", "speech.rs", "remote_boundary.rs", "screenshot.rs"],
        }, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        return code
    except Exception as error:  # noqa: BLE001 - 生产者必须真实失败并写回执
        return producer.fail(f"{type(error).__name__}: {error}")


if __name__ == "__main__":
    args = sys.argv[1:]
    if "--suite" not in args or "--evidence" not in args:
        print("usage: r09_t04.py --suite <name> --evidence <dir>", file=sys.stderr)
        sys.exit(64)
    suite_name = args[args.index("--suite") + 1]
    evidence_dir = Path(args[args.index("--evidence") + 1])
    if not evidence_dir.is_absolute():
        evidence_dir = ROOT / evidence_dir
    sys.exit(run_suite(suite_name, evidence_dir))

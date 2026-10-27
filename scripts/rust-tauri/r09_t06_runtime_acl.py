#!/usr/bin/env python3
"""R09-T06 运行期 ACL 负向矩阵 harness（A11 运行期半边；不启动 GUI 的自检模式）。

用 wdio WebDriver HTTP 协议驱动 T07 evidence 候选
（`--features e2e-test,r09-host,r09-test-hooks`，automation hidden + dialog sink），
对 policy 声明的每个窗口 label 执行：
  - allowedCommands 正路径：invoke 不得被 ACL 拒绝（领域错误允许并记录）；
  - 白名单外命令负向：invoke 必须返回 ACL 拒绝（Tauri 文案 `Command <cmd> not allowed by ACL`）；
  - `acl::union-equals-policy::<window>`：该窗口正/负矩阵全部符合 policy 并集（多 capability 叠加不扩权）；
  - token 不出现在 URL/DOM/localStorage（只记录布尔/哈希，不落 token）；
  - release 侧 canary/debug 命令不存在：读交付 release 二进制静态断言（不启动 GUI）。

窗口获取策略（当前 host 命令面，desktop/src-tauri 已冻结）：
  - main：启动后已存在；
  - quick-chat：main invoke `quick_chat_show`；
  - onboarding：main invoke `debug_open_onboarding`（仅 evidence/hooks 构建存在）；
  - viewer-*：main invoke `spawn_viewer`（隔离 fixture 文件）；
  - browser-viewer：main `set_browser_connection` + `open_browser_viewer`（best-effort）；
  - skill-viewer-*：main `open_skill_viewer`（best-effort）；
  - settings / splash：当前 host 无“主窗存活时创建该 label”的命令 → 记
    `blocked::window-open::<label>` 并给出所需 host patch（转 T06_GUI_BATCH_NEEDS.json）。

安全/纪律：
  - **无 clearance 时绝不启动 app**：要求显式 `LINGXI_R09_T06_RUNTIME_ACL_CLEARED=1` 且
    `--app` 指向 T07 evidence 候选；否则写 `RUNTIME_ACL_NOT_CLEARED`、exit 2，不 spawn 任何进程。
  - 隔离 `LINGXI_HOME`/`TMPDIR`（/private/tmp）；真实 ~/.lingxi 前后摘要断言；
  - 启动 app 时 `LINGXI_R09_AUTOMATION=1` + `LINGXI_R09_HOST_DIALOG_SINK`（hidden + sink，不弹窗）；
  - 结束 SIGTERM 进程组 → 等待 → SIGKILL；断言无 app/sidecar 残留、无窗口。

用法：
  # 无 GUI 自检（当前允许的唯一模式）
  python3 -B scripts/rust-tauri/r09_t06_runtime_acl.py --evidence <dir>
  # visible clearance 批次（Lead 明确 cleared 后）：
  LINGXI_R09_T06_RUNTIME_ACL_CLEARED=1 python3 -B scripts/rust-tauri/r09_t06_runtime_acl.py \
      --evidence <dir> --app <g1-evidence-exe> [--wdio-port 19465]

退出码：0 PASS；2 FAIL/BLOCKED（含未 cleared 的 `RUNTIME_ACL_NOT_CLEARED`）。
"""

from __future__ import annotations

import hashlib
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
POLICY_PATH = ROOT / "docs/rust-tauri/R09/R09_COMMAND_POLICY.json"
def default_handoff_path() -> Path:
    """最新冻结候选 handoff（禁止硬编码旧候选；env 优先便于显式绑定）。"""
    env = os.environ.get("LINGXI_R09_T06_HANDOFF")
    if env:
        return Path(env)
    base = ROOT / "artifacts/rust-tauri/R09/dev/t06"
    found: list[tuple[tuple[int, ...], Path]] = []
    for candidate in base.glob("candidate-handoff-v*/candidate-handoff.json"):
        match = re.search(r"v(\d+(?:\.\d+)?)", candidate.parent.name)
        version = tuple(int(part) for part in match.group(1).split(".")) if match else (0,)
        found.append((version, candidate))
    if found:
        return max(found, key=lambda item: item[0])[1]
    return base / "candidate-handoff-02/candidate-handoff.json"
WINDOW_LABELS_SOURCE = ROOT / "desktop/src-tauri/src/native/window_labels.rs"
CLEARANCE_ENV = "LINGXI_R09_T06_RUNTIME_ACL_CLEARED"
HIDDEN_ONLY_ENV = "LINGXI_R09_T06_RUNTIME_ACL_HIDDEN_ONLY"

# ── hang 加固（Lead 2026-10-26 23:3x 裁决）────────────────────────────
# 1) 每个 wdio HTTP 调用硬超时 <=10s；2) 每窗口/每 case 超时；3) 整体 deadline
# 5min，到点写部分结果并非零退出；4) 每 20 case 落盘 progress.jsonl；
# 5) session/handles 失败快速失败；6) hidden-only 只跑 main 的 hidden-safe 子集，
# window-open 项直接 blocked（不尝试）；7) 任何结束/异常都 killpg + ps/端口断言。
WDIO_CALL_TIMEOUT = 10.0
WDIO_READY_TIMEOUT = 30.0
SESSION_TIMEOUT = 10.0
WINDOW_WAIT_TIMEOUT = 45.0
INVOKE_TIMEOUT = 8.0
OVERALL_DEADLINE = 300.0
PROGRESS_EVERY = 20

PROGRESS_PATH: Path | None = None
RUN_DEADLINE: float = 0.0

# 这些窗口的创建命令会调用 window.show() 或 WebviewWindowBuilder 默认 visible=true，
# 在 hidden-only clearance 下打开会产生**可见窗口**（Lead 明令：出现可见窗口迹象立即停）。
# 因此 hidden-only 批次不打开它们，如实记 blocked，并给出所需 visible 批次/host patch。
# main 窗口 allowedCommands 中在 hidden-only 批次**禁止实际执行**的命令：
# 会创建可见窗口/弹窗、重启/reload、真实系统状态变更、写入文件或触发 TCC。
# 记 blocked::allow-side-effect::main::<cmd>，如实披露而不是假通过。
RISKY_ALLOW_COMMANDS = {
    "quick_chat_show", "spawn_viewer", "open_browser_viewer", "open_skill_viewer",
    "debug_open_onboarding", "debug_open_onboarding_preview",
    "select_files", "select_folder", "select_skill",
    "show_notification", "speech_permission_request",
    "restart_app", "reload_main_window", "window_close", "window_minimize", "window_maximize",
    "set_keep_awake_enabled", "set_auto_launch_enabled",
    "start_drag", "trash_item", "write_file", "write_file_binary",
    "write_file_if_unchanged", "copy_file",
    "open_file", "open_folder", "show_in_finder",
}

VISIBLE_RISK_WINDOWS = {
    "quick-chat": "quick_chat_show → install::show_quick_chat → window.show()/set_focus（可见）",
    "onboarding": "debug_open_onboarding: builder visible(false) 后立即 window.show()（可见）",
    "viewer-*": "spawn_viewer: WebviewWindowBuilder 默认 visible=true（创建即可见）",
    "browser-viewer": "open_browser_viewer: 创建控制面板窗口后 show（可见）",
    "skill-viewer-*": "open_skill_viewer: WebviewWindowBuilder 默认 visible=true（创建即可见）",
}
CASES: list[dict] = []
LOGS: list[str] = []


# ── 基础 ────────────────────────────────────────────────────────────────


def case(name: str, ok: bool, detail=None) -> bool:
    CASES.append({"case": name, "ok": bool(ok), "actual": 1 if ok else 0, "detail": detail})
    if PROGRESS_PATH is not None and len(CASES) % PROGRESS_EVERY == 0:
        try:
            with PROGRESS_PATH.open("a", encoding="utf-8") as handle:
                handle.write(json.dumps({"n": len(CASES), "case": name, "ok": bool(ok),
                                         "at": time.time()}, ensure_ascii=False) + "\n")
        except OSError:
            pass
    return bool(ok)


def check_deadline(stage: str) -> None:
    if RUN_DEADLINE and time.time() > RUN_DEADLINE:
        raise TimeoutError(f"overall deadline exceeded at {stage}")


def log(message: str) -> None:
    print(message, flush=True)
    LOGS.append(message)


def read_json(path: Path) -> dict | None:
    try:
        return json.loads(path.read_text(encoding="utf-8")) if path.is_file() else None
    except json.JSONDecodeError:
        return None


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def real_home_snapshot() -> dict:
    home = Path.home() / ".lingxi"
    if not home.is_dir():
        return {"exists": False, "digest": None, "fileCount": 0}
    digest = hashlib.sha256()
    count = 0
    for path in sorted(home.rglob("*")):
        if not path.is_file():
            continue
        relative = path.relative_to(home)
        digest.update(str(relative).encode())
        digest.update(b"\0")
        digest.update(str(path.stat().st_size).encode())
        digest.update(b"\0")
        count += 1
    return {"exists": True, "digest": digest.hexdigest(), "fileCount": count}


def http_call(method: str, url: str, payload=None, timeout: float = 320.0):
    data = None
    headers = {}
    if payload is not None:
        data = json.dumps(payload).encode()
        headers["Content-Type"] = "application/json"
    request = urllib.request.Request(url, data=data, headers=headers, method=method)
    with urllib.request.urlopen(request, timeout=timeout) as response:
        body = response.read().decode("utf-8")
    return json.loads(body) if body else {}


# ── policy / 矩阵计划 ───────────────────────────────────────────────────


def expected_window_patterns() -> set[str]:
    """窗口 label 模式唯一来源：冻结的 `window_labels.rs`（只读）。"""
    fallback = {"main", "settings", "quick-chat", "browser-viewer", "splash",
                "onboarding", "viewer-*", "skill-viewer-*"}
    text = WINDOW_LABELS_SOURCE.read_text(encoding="utf-8") if WINDOW_LABELS_SOURCE.is_file() else ""
    if not text:
        return fallback
    patterns = set()
    for const in ("MAIN", "SETTINGS", "QUICK_CHAT", "BROWSER_VIEWER", "SPLASH", "ONBOARDING"):
        match = re.search(rf'pub const {const}\s*:\s*&str\s*=\s*"([^"]+)"', text)
        if match:
            patterns.add(match.group(1))
    for const in ("VIEWER_PREFIX", "SKILL_VIEWER_PREFIX"):
        match = re.search(rf'pub const {const}\s*:\s*&str\s*=\s*"([^"]+)"', text)
        if match:
            patterns.add(match.group(1) + "*")
    return patterns or fallback


def effective_windows(policy: dict) -> dict:
    """构建运行期矩阵：release policy 面 + dev-only（仅 dev capability 覆盖的窗口）。"""
    release_surface: set[str] = set()
    for spec in (policy.get("windows") or {}).values():
        release_surface.update(spec.get("allowedCommands") or [])
    dev_commands = set(policy.get("devOnlyCommands") or [])
    dev_windows = set(((policy.get("devOnlyCapabilities") or {}).get("windows")) or [])
    windows: dict[str, dict] = {}
    for label, spec in (policy.get("windows") or {}).items():
        allowed = set(spec.get("allowedCommands") or [])
        if label in dev_windows:
            allowed |= dev_commands
        denied = sorted((release_surface | dev_commands) - allowed)
        windows[label] = {
            "label": label,
            "allowed": sorted(allowed),
            "denied": denied,
            "devOnlyGranted": label in dev_windows,
            "negativeCount": len(denied),
        }
    return windows


def build_plan(policy: dict, windows: dict) -> dict:
    return {
        "schema": "lingxi.r09.t06-runtime-acl-plan.v1",
        "source": str(POLICY_PATH.relative_to(ROOT)),
        "windowPatterns": sorted(windows),
        "perWindow": windows,
        "summary": {
            window: {"allow": len(spec["allowed"]), "deny": len(spec["denied"]),
                     "devOnlyGranted": spec["devOnlyGranted"]}
            for window, spec in windows.items()
        },
        "negativeMatrix": [
            {"window": window, "command": command, "expect": "deny"}
            for window, spec in windows.items() for command in spec["denied"]
        ],
        "assertion": "Tauri ACL 拒绝文案：`Command <cmd> not allowed by ACL`（tauri 2.12 webview/mod.rs:2107）",
    }


# ── preflight（无 GUI） ─────────────────────────────────────────────────


def preflight(policy: dict | None, handoff: dict | None, policy_path: Path,
              handoff_path: Path, windows: dict) -> list[str]:
    errors: list[str] = []
    if policy is None:
        errors.append("policy 缺失/非法")
        return errors
    case("acl::preflight::policy-valid", policy.get("schema") == "lingxi.r09.command-policy.v1",
         {"schema": policy.get("schema"), "path": str(policy_path.relative_to(ROOT))})
    case("acl::preflight::window-labels-match-policy",
         set(windows) == expected_window_patterns(),
         {"policy": sorted(windows), "native": sorted(expected_window_patterns())})
    total_allow = sum(len(spec["allowed"]) for spec in windows.values())
    total_deny = sum(len(spec["denied"]) for spec in windows.values())
    case("acl::preflight::matrix-plan-generated", total_allow > 0 and total_deny > 0,
         {"allowCases": total_allow, "denyCases": total_deny,
          "perWindow": {w: spec["negativeCount"] for w, spec in windows.items()}})

    if handoff is None:
        case("acl::preflight::candidate-handoff-present", False,
             {"expected": str(handoff_path.relative_to(ROOT)),
              "note": "需要 candidate-handoff JSON（含 delivery/G1 evidence 候选）"})
        return errors
    case("acl::preflight::candidate-handoff-present", True,
         {"path": str(handoff_path.relative_to(ROOT))})
    delivery = handoff.get("delivery") or {}
    g1 = handoff.get("g1Evidence") or {}
    case("acl::preflight::g1-evidence-features",
         g1.get("evidenceBuild") is True
         and {"r09-host", "r09-test-hooks", "e2e-test"} <= set((g1.get("featureList") or g1.get("features") or [])),
         {"features": g1.get("featureList") or g1.get("features"),
          "evidenceBuild": g1.get("evidenceBuild"),
          "executable": g1.get("executable")})
    case("acl::preflight::delivery-release-not-evidence",
         (delivery.get("evidenceBuild") is False)
         and "r09-test-hooks" not in (delivery.get("featureList") or []),
         {"features": delivery.get("featureList"), "evidenceBuild": delivery.get("evidenceBuild")})
    case("acl::preflight::renderer-binding-equal",
         bool(delivery.get("rendererDigest")) and delivery.get("rendererDigest") == g1.get("rendererDigest"),
         {"delivery": delivery.get("rendererDigest"), "g1": g1.get("rendererDigest"),
          "note": "同一 renderer 候选（digest 相同），两个生成实例"})
    scan = handoff.get("releaseScan")
    if not scan:
        # v3.3+ handoff: 静态门收在 staticGates 下（releaseScan 为 v1/v2 字段）。
        scan = ((handoff.get("staticGates") or {}).get("releaseAttackSurface")) or {}
    case("acl::preflight::delivery-scan-pass",
         scan.get("status") == "PASS" or scan.get("overall") == "PASS",
         {"status": scan.get("status"), "overall": scan.get("overall"),
          "failedCases": scan.get("failedCases")})

    # release 无 canary/debug 命令：读交付 release 二进制（只读，不启动）。
    delivery_exe = delivery.get("executable")
    debug_commands = list(policy.get("devOnlyCommands") or []) + ["e2e_canary", "r09_e2e_canary"]
    if delivery_exe and Path(delivery_exe).is_file():
        data = Path(delivery_exe).read_bytes()
        hits = {token: data.count(token.encode()) for token in debug_commands
                if data.count(token.encode())}
        case("acl::preflight::release-no-canary-debug-command", not hits,
             {"binary": delivery_exe, "hits": hits,
              "note": "A12：release 不得含 test canary/debug 命令名"})
    else:
        case("acl::preflight::release-no-canary-debug-command", False,
             {"binary": delivery_exe, "note": "交付 release 二进制缺失"})

    cleared = os.environ.get(CLEARANCE_ENV) == "1"
    hidden_only = os.environ.get(HIDDEN_ONLY_ENV) == "1"
    case("acl::preflight::gui-clearance", cleared,
         {"env": CLEARANCE_ENV, "value": os.environ.get(CLEARANCE_ENV),
          "note": "运行期矩阵未 cleared：不会启动 app（无 GUI 自检模式）"})
    case("acl::preflight::hidden-only-clearance", hidden_only,
         {"env": HIDDEN_ONLY_ENV, "value": os.environ.get(HIDDEN_ONLY_ENV),
          "visibleRiskWindows": VISIBLE_RISK_WINDOWS,
          "note": "hidden-only 批次只启动 hidden 主窗；可见风险窗口记 blocked，避免任何可见窗口"})
    return errors


# ── wdio 驱动（清理后才走） ─────────────────────────────────────────────


class WdioDriver:
    def __init__(self, base: str, call_timeout: float = WDIO_CALL_TIMEOUT):
        self.base = base.rstrip("/")
        self.session: str | None = None
        self.call_timeout = call_timeout

    def request(self, method: str, path: str, payload=None):
        # 单次 wdio 调用必须有上限：v2 重跑前实测曾因端口参数错误 + 无超时导致
        # session/handles 长时间挂起（工具 600s 超时转后台，需人工清理）。
        payload_value = http_call(method, self.base + path, payload, timeout=self.call_timeout)
        value = payload_value.get("value")
        # W3C：value 可能是对象/数组/字符串；只有对象且 error 非空才是 WebDriver 错误。
        if isinstance(value, dict) and value.get("error"):
            raise RuntimeError(f"WebDriver error: {payload_value}")
        return value

    def start(self) -> None:
        value = self.request("POST", "/session", {"capabilities": {"alwaysMatch": {}}})
        self.session = value["sessionId"]
        self.set_script_timeout(8000)

    def stop(self) -> None:
        if self.session:
            try:
                self.request("DELETE", f"/session/{self.session}")
            except Exception:  # noqa: BLE001 - 会话清理失败不掩盖主断言
                pass

    def handles(self) -> list[str]:
        value = self.request("GET", f"/session/{self.session}/window/handles")
        return list(value or [])

    def focus(self, handle: str) -> None:
        self.request("POST", f"/session/{self.session}/window", {"handle": handle})

    def set_script_timeout(self, milliseconds: int) -> None:
        self.request("POST", f"/session/{self.session}/timeouts",
                     {"script": milliseconds})

    def script(self, script: str, args: list | None = None):
        return self.request("POST", f"/session/{self.session}/execute/sync",
                            {"script": script, "args": args or []})

    def async_script(self, script: str, args: list | None = None):
        # W3C execute/async：脚本最后拿到 done 回调；WDIO 插件用 callAsyncJavaScript
        # 实现，Promise 在页面 runloop 正常推进时 resolve（execute/sync + 轮询 probe
        # 在该插件下不会 resolve，v3.2 首次实测每个命令耗尽 8s probe deadline）。
        #
        # 注意：不能用 request() 的“dict 带 error 即 WebDriver 错误”规则——命令结果
        # 本身可能带非空 error（ACL 拒绝/参数错误），只有 value 不是 `{done:true,...}`
        # 的才是脚本层失败（如 script timeout / javascript error）。
        payload_value = http_call("POST", f"{self.base}/session/{self.session}/execute/async",
                                  {"script": script, "args": args or []},
                                  timeout=self.call_timeout)
        value = payload_value.get("value")
        if isinstance(value, dict) and value.get("done") is True:
            return value
        raise RuntimeError(f"WebDriver error: {payload_value}")

    def wait_script(self, script: str, timeout: float = 120.0, interval: float = 0.15):
        started = time.time()
        while time.time() - started < timeout:
            value = self.script(script)
            if value not in (None, False):
                return value
            time.sleep(interval)
        raise TimeoutError(f"界面未到达要求状态: {script}")


LABEL_SCRIPT = (
    "return (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.metadata && "
    "window.__TAURI_INTERNALS__.metadata.currentWindow && "
    "window.__TAURI_INTERNALS__.metadata.currentWindow.label) || null;"
)
INVOKE_SCRIPT = """
return (function(){
  if (!window.__TAURI__ || !window.__TAURI__.core || typeof window.__TAURI__.core.invoke !== 'function') {
    return Promise.reject('tauri-bridge-missing');
  }
  return window.__TAURI__.core.invoke(CMD, ARGS);
})();
"""
PROBE_BEGIN = """
(function(){
  var id = 'p' + Math.random().toString(36).slice(2);
  window.__r09AclProbe = window.__r09AclProbe || {};
  window.__r09AclProbe[id] = {done:false, ok:false, error:null, result:null};
  window.__TAURI__.core.invoke(CMD, ARGS).then(function(result){
    window.__r09AclProbe[id] = {done:true, ok:true, error:null, result:(result === undefined ? null : result)};
  }).catch(function(error){
    var message = String((error && (error.message || error)) || error);
    window.__r09AclProbe[id] = {done:true, ok:false, error:message, result:null};
  });
  return id;
})()
"""
PROBE_READ = """
(function(){
  var id = ID;
  var probe = window.__r09AclProbe && window.__r09AclProbe[id];
  if (!probe) return {done:false, missing:true};
  return probe;
})()
"""
ACL_DENY_MARKERS = (
    "not allowed by acl",
    "not allowed on any window",
    "not allowed on window",   # Tauri 2.12: `X not allowed on window "main", webview "main"...`
    "not allowed on webview",
    "not allowed by acl",      # 兼容旧文案
)


def is_acl_denied(message: str | None) -> bool:
    lowered = (message or "").lower()
    return any(marker in lowered for marker in ACL_DENY_MARKERS)


def invoke_async(driver: WdioDriver, command: str, args: dict | None = None,
                 timeout: float = INVOKE_TIMEOUT) -> dict:
    """单次 W3C execute/async 调用一个 Tauri 命令。

    为什么不用 execute/sync + 轮询 probe（v3.2 实测教训）：该 wdio 插件的
    execute/script 期间页面 IPC 的 Promise 不推进，probe 永远 pending，每个命令
    都要耗尽 INVOOKE_TIMEOUT 预算（72 命令会超过整体 deadline），而且旧逻辑把
    probe-timeout 当 domain-error 误判为 allow 通过。execute/async 由插件用
    callAsyncJavaScript 实现，Promise resolve 后 done(result) 直接返回；失败则
    显式返回 done=False（调用方必须记红，不能算 allow 通过）。
    """
    script = (
        "var done = arguments[arguments.length - 1];\n"
        "var cmd = CMD, cmdArgs = ARGS;\n"
        "if (!window.__TAURI__ || !window.__TAURI__.core || "
        "typeof window.__TAURI__.core.invoke !== 'function') {\n"
        "  done({done: true, ok: false, error: 'tauri-bridge-missing', result: null});\n"
        "} else {\n"
        "  window.__TAURI__.core.invoke(cmd, cmdArgs).then(function(result) {\n"
        "    done({done: true, ok: true, error: null, "
        "result: (result === undefined ? null : result)});\n"
        "  }).catch(function(error) {\n"
        "    done({done: true, ok: false, "
        "error: String((error && (error.message || error)) || error), result: null});\n"
        "  });\n"
        "}\n"
    ).replace("CMD", json.dumps(command)).replace("ARGS", json.dumps(args or {}))
    try:
        value = driver.async_script(script, [])
    except Exception as error:  # noqa: BLE001 - wdio 脚本超时/断连 = 该命令无结果
        return {"done": False, "ok": False,
                "error": f"wdio-async-failed: {type(error).__name__}: {error}", "result": None}
    if not isinstance(value, dict):
        return {"done": False, "ok": False,
                "error": f"wdio-async-bad-value: {value!r}", "result": None}
    return value


def window_label_of(driver: WdioDriver, handle: str) -> str | None:
    driver.focus(handle)
    return driver.script(LABEL_SCRIPT)


def wait_for_label(driver: WdioDriver, label_pattern: str, timeout: float = 60.0):
    started = time.time()
    errors = 0
    while time.time() - started < timeout:
        try:
            handles = driver.handles()
        except Exception as error:  # noqa: BLE001 - wdio 无响应时快速失败，不挂起
            errors += 1
            if errors >= 5:
                raise RuntimeError(f"wdio unresponsive while listing handles: {error}") from error
            time.sleep(1.0)
            continue
        errors = 0
        for handle in handles:
            try:
                label = window_label_of(driver, handle)
            except Exception:  # noqa: BLE001 - 单 handle 失败继续下一个
                continue
            if label is None:
                continue
            if label_pattern.endswith("*"):
                if label.startswith(label_pattern[:-1]):
                    return handle, label
            elif label == label_pattern:
                return handle, label
        time.sleep(0.2)
    return None, None


WINDOW_OPENERS = {
    "quick-chat": ("quick_chat_show", {}),
    "onboarding": ("debug_open_onboarding", {}),
    "viewer-*": ("spawn_viewer", {"path": "{fixture}"}),
    "browser-viewer": ("open_browser_viewer", None),  # 两步特殊处理
    "skill-viewer-*": ("open_skill_viewer", None),    # 特殊 payload
}
WINDOW_UNAVAILABLE_NOTE = {
    "settings": "主窗存活时 open_settings 走 MainModal（不创建 settings 窗口）；"
                "host 无独立创建入口 → 需 host patch（T06_GUI_BATCH_NEEDS.json）",
    "splash": "host 当前只在 windows() 声明 splash 规格，没有创建/显示 splash 窗口的命令 → "
              "需 host patch（T06_GUI_BATCH_NEEDS.json）",
}


def acquire_window(wdio: WdioDriver, main_handle: str, label: str,
                   fixture: Path) -> tuple[str | None, str, list[dict]]:
    """best-effort 打开窗口 label；返回 (handle, note, attempts)。"""
    attempts: list[dict] = []
    wdio.focus(main_handle)

    def attempt(command: str, args: dict):
        result = invoke_async(wdio, command, args, timeout=90)
        attempts.append({"command": command,
                         "ok": bool(result.get("done") and result.get("ok")),
                         "error": result.get("error")})
        return result

    if label == "main":
        return main_handle, "startup", attempts
    if label in WINDOW_UNAVAILABLE_NOTE:
        return None, WINDOW_UNAVAILABLE_NOTE[label], attempts
    if label == "browser-viewer":
        connection = {
            "connectionId": "runtime-acl-probe",
            "baseUrl": "http://127.0.0.1:1",
            "serverId": "runtime-acl-probe",
        }
        created = attempt("set_browser_connection", {"connection": connection})
        key = (created.get("result") or {}).get("connectionKey") if created.get("ok") else None
        if not created.get("ok"):
            # set_browser_connection 返回 Option<String>；适配纯字符串返回形状
            if isinstance(created.get("result"), str):
                key = created.get("result")
        if key:
            attempt("open_browser_viewer",
                    {"target": {"connectionKey": key, "sessionPath": "native:runtime-acl"}})
    elif label == "skill-viewer-*":
        skills_root = fixture.parent / "acl-skill"
        skills_root.mkdir(parents=True, exist_ok=True)
        (skills_root / "SKILL.md").write_text("# runtime acl probe\n", encoding="utf-8")
        attempt("open_skill_viewer", {"data": {"name": "runtime-acl-probe",
                                              "baseDir": str(skills_root),
                                              "filePath": None,
                                              "installed": False}})
    else:
        command, args = WINDOW_OPENERS.get(label, (None, None))
        if command:
            materialized = json.loads(json.dumps(args).replace("{fixture}", str(fixture)))
            attempt(command, materialized)

    handle, actual = wait_for_label(wdio, label, timeout=45)
    if handle:
        return handle, f"opened:{actual}", attempts
    return None, "window-open-failed", attempts


def window_storage_probe(token: str | None) -> str:
    token_js = json.dumps(token or "")
    return f"""
(function(){{
  var token = {token_js};
  var storage = [];
  try {{ for (var i=0;i<localStorage.length;i++) storage.push(localStorage.getItem(localStorage.key(i))||''); }} catch (e) {{}}
  try {{ for (var i=0;i<sessionStorage.length;i++) storage.push(sessionStorage.getItem(sessionStorage.key(i))||''); }} catch (e) {{}}
  var text = (document.body && document.body.innerText) || '';
  return {{
    hrefHasToken: !!token && String(location.href).indexOf(token) >= 0,
    domHasToken: !!token && text.indexOf(token) >= 0,
    storageHasToken: !!token && storage.some(function(value) {{ return value.indexOf(token) >= 0; }})
  }};
}})()
"""


# ── 运行期执行 ──────────────────────────────────────────────────────────


class AppProcess:
    def __init__(self, argv: list[str], env: dict, log_path: Path):
        self.log_path = log_path
        self.log = log_path.open("w", encoding="utf-8")
        self.proc = subprocess.Popen(argv, stdout=self.log, stderr=subprocess.STDOUT,
                                     env=env, start_new_session=True)

    def terminate(self) -> dict:
        residual = 0
        try:
            os.killpg(os.getpgid(self.proc.pid), signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            self.proc.wait(timeout=20)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(os.getpgid(self.proc.pid), signal.SIGKILL)
            except ProcessLookupError:
                pass
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                residual += 1
        self.log.close()
        return {"exitCode": self.proc.returncode, "residual": residual}


def prepare_app_bundle(app: Path, runtime_root: Path) -> tuple[Path, dict]:
    """macOS 的 UNUserNotificationCenter 等 API 要求进程有 .app bundle；T01 debug
    构建是未打包的裸二进制（--no-bundle），直接运行会抛 bundleProxyForCurrentProcess
    异常。这里在隔离运行目录构造最小 debug .app（只复制现有 exe，不改候选 sha），
    用于 hidden wdio 批次；不改 frozen desktop 产物。"""
    if ".app" in app.parts:
        return app, {"wrapped": False, "bundle": str(app)}
    bundle = runtime_root / "LingxiRuntimeAcl.app"
    macos = bundle / "Contents" / "MacOS"
    resources = bundle / "Contents" / "Resources"
    macos.mkdir(parents=True, exist_ok=True)
    resources.mkdir(parents=True, exist_ok=True)
    executable_name = app.name
    launch = macos / executable_name
    launch.write_bytes(app.read_bytes())
    launch.chmod(0o755)
    plist = """<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>com.lingxi.app.runtime-acl</string>
  <key>CFBundleExecutable</key><string>{executable}</string>
  <key>CFBundleName</key><string>Lingxi Runtime ACL</string>
  <key>CFBundleDisplayName</key><string>Lingxi Runtime ACL</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.43</string>
  <key>CFBundleVersion</key><string>0.1.43</string>
  <key>LSMinimumSystemVersion</key><string>10.13</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSpeechRecognitionUsageDescription</key><string>runtime acl batch</string>
  <key>NSMicrophoneUsageDescription</key><string>runtime acl batch</string>
</dict>
</plist>
""".format(executable=executable_name)
    (bundle / "Contents" / "Info.plist").write_text(plist, encoding="utf-8")
    return launch, {"wrapped": True, "bundle": str(bundle), "sourceExe": str(app),
                    "sourceSha256": sha256_file(app), "launchExe": str(launch),
                    "launchSha256": sha256_file(launch)}


def run_runtime(policy: dict, handoff: dict, windows: dict, app: Path,
                wdio_port: int, evidence: Path, hidden_only: bool = True) -> int:
    runtime_root = Path("/private/tmp") / f"lingxi-r09-t06-acl-{os.getpid()}-{int(time.time())}"
    home = runtime_root / "home"
    tmp = runtime_root / "tmp"
    fixtures = runtime_root / "fixtures"
    for path in (home, tmp, fixtures):
        path.mkdir(parents=True, exist_ok=True)
    launch_app_path, bundle_info = prepare_app_bundle(app, runtime_root)
    case("acl::runtime::app-bundle-ready", launch_app_path.is_file(),
         {"sourceExe": str(app), "bundle": bundle_info,
          "note": "debug 裸二进制包最小 .app 以满足 macOS NSBundle API；候选 sha 未改"})
    fixture = fixtures / "runtime-acl-fixture.txt"
    fixture.write_text("runtime acl matrix fixture\n", encoding="utf-8")
    sink = runtime_root / "dialog-sink.json"
    real_home_before = real_home_snapshot()
    global RUN_DEADLINE
    RUN_DEADLINE = time.time() + OVERALL_DEADLINE
    env = dict(os.environ)
    env.update({
        "LINGXI_HOME": str(home),
        "TMPDIR": str(tmp), "TMP": str(tmp), "TEMP": str(tmp),
        "LINGXI_R09_AUTOMATION": "1",
        "LINGXI_R09_HOST_DIALOG_SINK": str(sink),
        "TAURI_WEBDRIVER_PORT": str(wdio_port),
    })
    service_binary = ROOT / "rust/target/debug/lingxi-service"
    if service_binary.is_file():
        env["LINGXI_SERVICE_BIN"] = str(service_binary)
    log(f"runtime: launching evidence candidate hidden+automation: {launch_app_path}")
    app_process = AppProcess([str(launch_app_path)], env, runtime_root / "native-app.log")
    wdio = WdioDriver(f"http://127.0.0.1:{wdio_port}")
    results: dict[str, dict] = {}
    status = "FAIL"
    try:
        started = time.time()
        while time.time() - started < WDIO_READY_TIMEOUT:
            check_deadline("wdio-ready")
            try:
                http_call("GET", f"http://127.0.0.1:{wdio_port}/status", timeout=1.0)
                break
            except Exception:  # noqa: BLE001 - 等待 wdio 服务起来
                time.sleep(0.2)
        else:
            case("acl::runtime::wdio-available", False, {"port": wdio_port})
            return finish_runtime(evidence, runtime_root, "FAIL", 2,
                                  {"error": "wdio-not-available"})
        case("acl::runtime::wdio-available", True, {"port": wdio_port})
        try:
            wdio.start()
        except Exception as error:  # noqa: BLE001 - session 失败快速返回
            case("acl::runtime::wdio-session-created", False,
                 {"port": wdio_port, "error": f"{type(error).__name__}: {error}"})
            return finish_runtime(evidence, runtime_root, "FAIL", 2,
                                  {"error": "wdio-session-failed"})
        case("acl::runtime::wdio-session-created", bool(wdio.session),
             {"port": wdio_port, "session": bool(wdio.session)})
        try:
            main_handle, main_label = wait_for_label(wdio, "main", timeout=WINDOW_WAIT_TIMEOUT)
        except Exception as error:  # noqa: BLE001 - wdio 无响应 = 本批次红
            case("acl::runtime::wdio-responsive", False,
                 {"port": wdio_port, "error": f"{type(error).__name__}: {error}"})
            return finish_runtime(evidence, runtime_root, "FAIL", 2,
                                  {"error": "wdio-unresponsive"})
        case("acl::runtime::main-window", bool(main_handle),
             {"handle": main_handle, "label": main_label})
        if not main_handle:
            return finish_runtime(evidence, runtime_root, "FAIL", 2,
                                  {"error": "main-window-missing"})

        token_result = invoke_async(wdio, "get_server_token", {}, timeout=30)
        token = token_result.get("result") if token_result.get("ok") else None
        token = token if isinstance(token, str) and token else None

        blocked_windows: list[dict] = []
        hidden_safe_labels = {"main"} if hidden_only else set(windows)
        case("acl::runtime::hidden-safe-window-subset", True,
             {"attempted": sorted(hidden_safe_labels),
              "blockedByPolicy": sorted(set(windows) - hidden_safe_labels),
              "note": "hidden-only 批次只对 main 执行矩阵；其余窗口不尝试打开（避免 window.show() 产生可见窗口/挂起）"})
        for label, spec in windows.items():
            check_deadline(f"window:{label}")
            wdio.focus(main_handle)
            if hidden_only and label not in hidden_safe_labels:
                if label in VISIBLE_RISK_WINDOWS:
                    reason = ("hidden-only clearance：该 label 的创建路径会显示窗口（"
                              + VISIBLE_RISK_WINDOWS[label] + "），本批次禁止打开；需 visible 批次或 host automation 抑制 patch")
                else:
                    reason = WINDOW_UNAVAILABLE_NOTE.get(
                        label, "hidden-only clearance：本批次不尝试打开该窗口")
                blocked_windows.append({"window": label, "note": reason, "attempts": [],
                                        "kind": "hidden-only-blocked"})
                case(f"blocked::window-open::{label}", False,
                     {"blocked": True, "reason": reason})
                continue
            if hidden_only and label in VISIBLE_RISK_WINDOWS:
                note = ("hidden-only clearance：该 label 的创建路径会显示窗口（"
                        + VISIBLE_RISK_WINDOWS[label] + "），本批次禁止打开；需 visible 批次或 host automation 抑制 patch")
                blocked_windows.append({"window": label, "note": note, "attempts": [],
                                        "kind": "visible-risk-blocked"})
                case(f"blocked::window-open::{label}", False,
                     {"blocked": True, "visibilityRisk": True, "reason": note})
                continue
            handle, note, attempts = acquire_window(wdio, main_handle, label, fixture)
            if not handle:
                blocked_windows.append({"window": label, "note": note, "attempts": attempts})
                case(f"blocked::window-open::{label}", False,
                     {"blocked": True, "reason": note, "attempts": attempts})
                continue
            wdio.focus(handle)
            case(f"acl::window-available::{label}", True,
                 {"handle": handle, "note": note, "attempts": attempts})
            observed_allow: list[str] = []
            acl_violations: list[str] = []
            domain_errors: list[str] = []
            skipped_side_effect: list[str] = []
            for command in spec["allowed"]:
                check_deadline(f"allow:{label}:{command}")
                if hidden_only and command in RISKY_ALLOW_COMMANDS:
                    skipped_side_effect.append(command)
                    case(f"blocked::allow-side-effect::{label}::{command}", False,
                         {"blocked": True,
                          "reason": "hidden-only 批次禁止执行：会创建可见窗口/弹窗、重启、写文件、系统状态变更或 TCC",
                          "requires": "visible clearance 批次或 host automation 抑制 patch"})
                    continue
                probe = invoke_async(wdio, command, {})
                if not probe.get("done"):
                    case(f"acl::{label}::{command}::allow", False,
                         {"reason": "probe-no-result（不能当作 allow 通过）",
                          "error": probe.get("error")})
                elif probe.get("ok"):
                    observed_allow.append(command)
                    case(f"acl::{label}::{command}::allow", True,
                         {"result": "ok"})
                elif not is_acl_denied(probe.get("error")):
                    observed_allow.append(command)
                    domain_errors.append({"command": command, "error": probe.get("error")})
                    case(f"acl::{label}::{command}::allow", True,
                         {"result": "domain-error-not-acl", "error": probe.get("error")})
                else:
                    acl_violations.append(command)
                    case(f"acl::{label}::{command}::allow", False,
                         {"error": probe.get("error"), "reason": "allowed command was ACL-denied"})
            deny_violations: list[str] = []
            for command in spec["denied"]:
                check_deadline(f"deny:{label}:{command}")
                probe = invoke_async(wdio, command, {})
                if not probe.get("done"):
                    case(f"acl::{label}::{command}::deny", False,
                         {"reason": "probe-no-result（无法证明 ACL 拒绝）",
                          "error": probe.get("error")})
                elif is_acl_denied(probe.get("error")):
                    case(f"acl::{label}::{command}::deny", True,
                         {"error": probe.get("error")})
                else:
                    deny_violations.append(command)
                    case(f"acl::{label}::{command}::deny", False,
                         {"result": "not-acl-denied", "ok": probe.get("ok"),
                          "error": probe.get("error"), "reason": "command outside policy was not ACL-denied"})
            expected_allow = [c for c in spec["allowed"] if c not in skipped_side_effect]
            union_ok = (not acl_violations and not deny_violations
                        and sorted(observed_allow) == sorted(expected_allow))
            case(f"acl::union-equals-policy::{label}", union_ok,
                 {"scope": "executed-subset" if skipped_side_effect else "full",
                  "allowedCount": len(spec["allowed"]),
                  "executedAllowCount": len(expected_allow),
                  "observedAllow": len(observed_allow),
                  "skippedSideEffect": skipped_side_effect,
                  "aclViolations": acl_violations, "denyViolations": deny_violations,
                  "domainErrors": domain_errors})
            if skipped_side_effect:
                case(f"blocked::union-equals-policy::{label}::full-side-effect", False,
                     {"blocked": True, "skippedSideEffect": skipped_side_effect,
                      "reason": "可见/副作用命令需 visible 批次才能纳入 full 并集断言"})
            if token is None:
                case(f"blocked::token-not-in-url-dom::{label}", False,
                     {"blocked": True, "reason": "host service token 未就绪（get_server_token 返回空），token 探针留 visible/服务就绪批次"})
            if token is not None:
                check_deadline(f"token:{label}")
                probe = wdio.script(window_storage_probe(token)) or {}
                if not isinstance(probe, dict):
                    probe = {}
                token_ok = not any(probe.get(key) for key in
                                   ("hrefHasToken", "domHasToken", "storageHasToken"))
                case(f"acl::token-not-in-url-dom::{label}", token_ok,
                     {"probe": probe, "tokenRecorded": False})
            results[label] = {"handle": handle, "attempts": attempts,
                              "domainErrors": domain_errors}
        # T03 ipc::no-child-process-observed 的可复用接口：运行期采样 app 进程树，
        # 期望只有 app 自身 + 固定 sidecar（lingxi-service），无 shell/任意子进程。
        tree = process_tree(app_process.proc.pid)
        unexpected = [row for row in tree
                      if row["pid"] != app_process.proc.pid
                      and not row["comm"].endswith("lingxi-service")
                      and "lingxi-service" not in row["comm"]]
        case("acl::runtime::no-unexpected-child-process", not unexpected, {"processTree": tree,
             "unexpected": unexpected,
             "note": "复用 T03 blocked::ipc::no-child-process-observed 的接口（进程表探针）"})
        wdio.stop()
        case("acl::runtime::overall-deadline-met", True,
             {"limitSecs": OVERALL_DEADLINE, "elapsedSecs": round(time.time() - (RUN_DEADLINE - OVERALL_DEADLINE), 1)})
        failed = [row["case"] for row in CASES if not row["ok"] and not row["case"].startswith("blocked::")]
        status = "PASS" if not failed and not blocked_windows else (
            "PASS_WITH_BLOCKED_WINDOWS" if not failed else "FAIL")
        return finish_runtime(evidence, runtime_root, status,
                              0 if status.startswith("PASS") else 2,
                              {"windows": results, "blockedWindows": blocked_windows,
                               "realHomeBefore": real_home_before})
    except TimeoutError as error:
        # 整体 deadline 到点：写部分结果并非零退出（不挂起）。
        case("acl::runtime::overall-deadline", False,
             {"limitSecs": OVERALL_DEADLINE, "error": str(error)})
        return finish_runtime(evidence, runtime_root, "FAIL", 2,
                              {"error": "overall-deadline-exceeded",
                               "partialCases": len(CASES)})
    finally:
        shutdown = app_process.terminate()
        real_home_after = real_home_snapshot()
        untouched = real_home_after == real_home_before
        case("acl::runtime::real-home-untouched", untouched,
             {"before": real_home_before, "after": real_home_after})
        case("acl::runtime::no-process-residual", shutdown["residual"] == 0,
             {"shutdown": shutdown})
        cleanup = cleanup_residual_processes(app, home)
        residual = process_residual(app, home)
        # app 退出后 sidecar/wrapper 可能还剩几帧才完全消失：1s 内重试，避免 timing 假红。
        for _ in range(10):
            if residual == 0:
                break
            time.sleep(0.2)
            cleanup += cleanup_residual_processes(app, home)
            residual = process_residual(app, home)
        case("acl::runtime::no-app-sidecar-residual", residual == 0,
             {"appPath": str(app), "matchingProcesses": residual, "cleaned": cleanup})
        port_open = wdio_port_open(wdio_port)
        case("acl::runtime::wdio-port-released", not port_open,
             {"port": wdio_port, "leftOpen": port_open})
        sink_payload = read_sink(sink)
        case("acl::runtime::dialog-sink-clean", sink_payload is None,
             {"sink": str(sink), "payload": sink_payload,
              "note": "sink 只在宿主启动失败/自动化失败路径写入；clean = 文件不存在或空"})
        diagnostics = hidden_window_evidence(home, runtime_root / "native-app.log")
        # 归档诊断证据到 evidence（评审要求：app_ready windowVisible 证据不能只留在 /tmp）。
        (evidence / "hidden-window-evidence.json").write_text(
            json.dumps(diagnostics, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        raw_events = []
        for diag in sorted(home.rglob("*.jsonl")):
            try:
                raw_events.append({"path": str(diag.relative_to(home)),
                                   "content": diag.read_text(encoding="utf-8", errors="replace")})
            except OSError:
                continue
        (evidence / "hidden-automation-events.json").write_text(
            json.dumps(raw_events, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        case("acl::runtime::hidden-window-evidence",
             diagnostics.get("automation") is True
             and diagnostics.get("windowVisible") is False,
             diagnostics)
        log(f"runtime: app shutdown {shutdown}")


def process_tree(root_pid: int) -> list[dict]:
    result = subprocess.run(["/bin/ps", "-axo", "pid=,ppid=,comm="],
                            capture_output=True, text=True, timeout=30)
    rows = []
    by_parent: dict[int, list[int]] = {}
    for line in result.stdout.splitlines():
        parts = line.split(None, 2)
        if len(parts) != 3:
            continue
        pid, ppid, comm = int(parts[0]), int(parts[1]), parts[2]
        rows.append({"pid": pid, "ppid": ppid, "comm": comm})
        by_parent.setdefault(ppid, []).append(pid)
    allowed = {root_pid}
    changed = True
    while changed:
        changed = False
        for pid in list(allowed):
            for child in by_parent.get(pid, []):
                if child not in allowed:
                    allowed.add(child)
                    changed = True
    return [row for row in rows if row["pid"] in allowed]


def _belongs_to_run(command: str, app: Path, home: Path) -> bool:
    if str(app) in command:
        return True
    return "lingxi-service" in command and str(home) in command


def process_residual(app: Path, home: Path) -> int:
    result = subprocess.run(["/bin/ps", "-axo", "command="],
                            capture_output=True, text=True, timeout=30)
    return sum(1 for line in result.stdout.splitlines()
               if _belongs_to_run(line, app, home))


def cleanup_residual_processes(app: Path, home: Path) -> list[dict]:
    """兜底清理：只匹配本候选 app 路径/隔离 home 的 lingxi-service；记录被清理进程。"""
    result = subprocess.run(["/bin/ps", "-axo", "pid=,command="],
                            capture_output=True, text=True, timeout=30)
    killed = []
    for line in result.stdout.splitlines():
        parts = line.strip().split(None, 1)
        if len(parts) != 2:
            continue
        pid, command = parts
        if int(pid) in (os.getpid(), os.getppid()):
            continue
        if _belongs_to_run(command, app, home):
            try:
                os.kill(int(pid), signal.SIGTERM)
                killed.append({"pid": int(pid), "command": command[:300]})
            except ProcessLookupError:
                pass
    if killed:
        time.sleep(1.0)
        for row in killed:
            try:
                os.kill(row["pid"], signal.SIGKILL)
            except ProcessLookupError:
                pass
    return killed


def wdio_port_open(port: int) -> bool:
    import socket
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.settimeout(1.0)
    try:
        return sock.connect_ex(("127.0.0.1", port)) == 0
    finally:
        sock.close()


def read_sink(path: Path):
    if not path.is_file():
        return None
    text = path.read_text(encoding="utf-8").strip()
    if not text:
        return None
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return {"raw": text[:2000]}


def hidden_window_evidence(home: Path, app_log: Path | None = None) -> dict:
    """从宿主诊断（隔离 home 的 jsonl + app stdout 的 [tauri-diag] 行）提取
    app-ready 的 hidden/automation 证据（证明主窗未显示、未弹 modal）。"""
    evidence = {"automation": None, "windowVisible": None, "diagnostics": [],
                "modalSuppressed": None}
    events: list[tuple[str, dict]] = []
    for path in sorted(home.rglob("*.jsonl")):
        try:
            for line in path.read_text(encoding="utf-8").splitlines():
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if isinstance(event, dict):
                    events.append((str(path.relative_to(home)), event))
        except OSError:
            continue
    if app_log is not None and app_log.is_file():
        for line in app_log.read_text(encoding="utf-8", errors="replace").splitlines():
            marker = "[tauri-diag] "
            if marker in line:
                try:
                    event = json.loads(line.split(marker, 1)[1])
                except json.JSONDecodeError:
                    continue
                events.append(("native-app.log", event))
    for source, event in events:
        name = event.get("event") or event.get("name")
        detail = event.get("detail") or event.get("payload") or event
        if not isinstance(detail, dict):
            detail = {}
        if name in ("app-ready", "app-ready-window-suppressed"):
            automation = detail.get("automation")
            if automation is None and name == "app-ready-window-suppressed":
                automation = True
            evidence["automation"] = bool(automation)
            evidence["windowVisible"] = bool(detail.get("windowVisible", False))
            evidence["diagnostics"].append({"source": source, "event": name,
                                            "detail": {k: detail.get(k) for k in
                                                       ("automation", "windowVisible",
                                                        "alreadyReady", "label")}})
        if name == "failure-dialog-suppressed":
            evidence["modalSuppressed"] = bool(detail.get("modalShown") is False)
    return evidence


def finish_runtime(evidence: Path, runtime_root: Path, status: str, exit_code: int,
                   extra: dict) -> int:
    extra = dict(extra)
    extra["runtimeRoot"] = str(runtime_root)
    extra["hiddenOnly"] = os.environ.get(HIDDEN_ONLY_ENV) == "1"
    extra["riskyAllowCommandsSkipped"] = sorted(RISKY_ALLOW_COMMANDS)
    return finish(evidence, status, exit_code, extra)


# ── 输出 ────────────────────────────────────────────────────────────────


def finish(evidence: Path, status: str, exit_code: int, extra: dict | None = None) -> int:
    failed = [row["case"] for row in CASES if not row["ok"]]
    blocked = [row["case"] for row in CASES if row["case"].startswith("blocked::")]
    evidence.mkdir(parents=True, exist_ok=True)
    payload = {
        "schema": "lingxi.r09.t06-runtime-acl.v1",
        "suite": "r09_t06_runtime_acl",
        "status": status,
        "exitCode": exit_code,
        "caseCount": len(CASES),
        "failedCases": failed,
        "blockedCases": blocked,
        "clearanceEnv": CLEARANCE_ENV,
        "guiLaunched": any("runtime::wdio-available" == row["case"] for row in CASES),
    }
    if extra:
        payload.update(extra)
    (evidence / "cases.json").write_text(json.dumps(
        {"schema": "lingxi.leaf-case-results.v1", "suite": "r09_t06_runtime_acl",
         "cases": CASES}, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    (evidence / "output.log").write_text("\n".join(LOGS) + "\n", encoding="utf-8")
    if PROGRESS_PATH is not None:
        try:
            with PROGRESS_PATH.open("a", encoding="utf-8") as handle:
                handle.write(json.dumps({"n": len(CASES), "final": True, "status": status,
                                         "at": time.time()}, ensure_ascii=False) + "\n")
        except OSError:
            pass
    (evidence / "result.json").write_text(json.dumps({
        "schema": "lingxi.r09.suite-receipt.v1",
        "suite": "r09_t06_runtime_acl",
        "status": status,
        "exitCode": exit_code,
        "caseCount": len(CASES),
        "failedCases": failed,
        "blockedCases": blocked,
        "clearanceEnv": CLEARANCE_ENV,
    }, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"r09_t06_runtime_acl: status={status} exit={exit_code} cases={len(CASES)} "
          f"failed={failed[:8]}{'…' if len(failed) > 8 else ''}")
    return exit_code


# ── 主流程 ──────────────────────────────────────────────────────────────


def arg_value(name: str, default=None):
    argv = sys.argv
    return Path(argv[argv.index(name) + 1]) if name in argv else default


def main() -> int:
    evidence = arg_value("--evidence")
    if evidence is None:
        print("usage: r09_t06_runtime_acl.py --evidence <dir> "
              "[--app <g1-evidence-exe>] [--wdio-port 19465] [--policy FILE] [--handoff FILE]")
        return 64
    evidence = evidence if evidence.is_absolute() else ROOT / evidence
    policy_path = arg_value("--policy", POLICY_PATH) or POLICY_PATH
    handoff_path = arg_value("--handoff", None) or default_handoff_path()
    policy = read_json(policy_path)
    handoff = read_json(handoff_path)
    windows = effective_windows(policy) if policy else {}
    preflight(policy, handoff, policy_path, handoff_path, windows)
    plan = build_plan(policy, windows) if policy else {}
    evidence.mkdir(parents=True, exist_ok=True)
    global PROGRESS_PATH
    PROGRESS_PATH = evidence / "progress.jsonl"
    PROGRESS_PATH.write_text("", encoding="utf-8")
    (evidence / "runtime-acl-plan.json").write_text(
        json.dumps(plan, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")

    cleared = os.environ.get(CLEARANCE_ENV) == "1"
    app_override = arg_value("--app")
    app_raw = app_override or Path((handoff or {}).get("g1Evidence", {}).get("executable", ""))
    app_candidate = Path(app_raw).resolve() if str(app_raw) else app_raw
    if not cleared:
        log("no GUI clearance: runtime ACL matrix not executed (RUNTIME_ACL_NOT_CLEARED)")
        for label in sorted(windows):
            case(f"blocked::runtime::{label}::matrix", False,
                 {"blocked": True, "reason": "RUNTIME_ACL_NOT_CLEARED：需 Lead 明确 GUI cleared 批次",
                  "plan": {"allow": len(windows[label]["allowed"]),
                           "deny": len(windows[label]["denied"])}})
        for group in ("remote-webview-zero-acl", "canary-debug-command-absent",
                      "debug-command-release-denied"):
            case(f"blocked::runtime::{group}", False,
                 {"blocked": True, "reason": "RUNTIME_ACL_NOT_CLEARED：需真实 GUI/wdio"})
        return finish(evidence, "RUNTIME_ACL_NOT_CLEARED", 2,
                      {"plan": "runtime-acl-plan.json",
                       "next": f"Lead cleared 后：{CLEARANCE_ENV}=1 --app <g1-evidence-exe>"})
    if not app_candidate or not Path(app_candidate).is_file():
        case("acl::runtime::evidence-app-present", False,
             {"app": str(app_candidate), "note": "需要 T07 evidence 候选 exe（e2e-test+r09-test-hooks）"})
        return finish(evidence, "FAIL", 2, {"error": "evidence-app-missing"})
    # CLI 优先；其次 env；最后默认 19465（T07 helper 的端口约定）。
    port_arg = arg_value("--wdio-port")
    # arg_value 统一返回 Path（便于 --app/--evidence）；数值参数必须先转成字符串。
    port = int(str(port_arg)) if port_arg is not None else int(os.environ.get("TAURI_WEBDRIVER_PORT", "19465"))
    hidden_only = os.environ.get(HIDDEN_ONLY_ENV) == "1"
    try:
        return run_runtime(policy, handoff or {}, windows, app_candidate, port, evidence,
                           hidden_only=hidden_only)
    except Exception as error:  # noqa: BLE001 - harness 自身失败必须红并写回执
        import traceback
        case("acl::runtime::harness-exception", False,
             {"error": f"{type(error).__name__}: {error}",
              "traceback": traceback.format_exc()[-3000:]})
        return finish(evidence, "FAIL", 2,
                      {"error": f"{type(error).__name__}: {error}"})


if __name__ == "__main__":
    sys.exit(main())

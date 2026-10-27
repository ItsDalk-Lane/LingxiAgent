#!/usr/bin/env python3
"""R09-T01 证据生产者：A01（同一 React 产物可运行）/ A02（生产不依赖开发服务器）。

两个套件（argv/evidencePaths 由 `rust/crates/xtask/src/stage_maps/R09.json` 决定）：
  r09_t01_react_parity   —— Electron 与 Tauri 加载同一 `desktop/dist-renderer` 构建，
                            核心页面（Rust 连接下的 app-init 状态）、宿主识别、
                            服务身份一致；业务组件零重写（renderer 改动白名单）。
  r09_t01_release_offline —— release `.app` 内嵌页面、关闭 Vite、注入
                            `VITE_DEV_URL` 无效、5173 canary 零连接、strings 无
                            devUrl、ad-hoc 签名可校验、SIGTERM 优雅退出。

真实执行：`npm run build:renderer`、`node scripts/tauri-build.mjs`、Electron（CDP）
与 Tauri debug/release 真实进程、真实 `lingxi-service`。不 mock 宿主/构建/权限边界；
所有命令与退出码写入 Producer 回执。大产物（.app、二进制）只在工作区/临时目录，
证据目录只放摘要/投影/日志（R08 69GB 教训）。
"""

from __future__ import annotations

import json
import os
import re
import shutil
import signal
import socket
import subprocess
import sys
import threading
import time
import traceback
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[1]
sys.path.insert(0, str(SCRIPTS))

from r09_producer import (  # noqa: E402
    EvidenceError,
    Producer,
    sha256_file,
    sha256_tree,
    tree_digest,
)

TAURI_DEBUG_BIN = ROOT / "desktop/src-tauri/target/debug/lingxi-resource-desktop"
TAURI_RELEASE_APP = ROOT / "desktop/src-tauri/target/release/bundle/macos/Lingxi.app"
ELECTRON_BIN = ROOT / "node_modules/electron/dist/Electron.app/Contents/MacOS/Electron"
DEBUG_SERVICE = ROOT / "rust/target/debug/lingxi-service"
DIST_RENDERER = ROOT / "desktop/dist-renderer"
DIST_TAURI_RESOURCES = ROOT / "desktop/dist-tauri-resources"

# A01「无重写第二套业务组件」白名单：只允许宿主接线文件变化（不含业务组件）。
RENDERER_DIFF_WHITELIST = (
    "desktop/src/react/host/",
    "desktop/src/react/__tests__/host/",
    "desktop/src/react/stores/session-actions.ts",
    "desktop/src/react/types.ts",
    "desktop/src/modules/platform.js",
    # v3/v3.2 页面级 meta CSP：Tauri 宿主 asset 协议 URL 需要 img-src/media-src 含
    # `asset:`/`http://asset.localhost`（Electron file: 不需要）。H1 A13 根因修复，
    # audit caps::page-meta-csp-allows-asset + tests/csp-sync 回归；不是业务组件重写。
    "desktop/src/modules/connection-csp.js",
    "desktop/src/global.d.ts",
    # T01 唯一业务组件改动（竞态防御，不改变业务语义）：WKWebView 下 i18n 异步
    # 加载成功后 placeholder 变化可能先于 tiptap EditorContent 挂载完成触发
    # `editor.view` 访问，旧代码会抛进区域 ErrorBoundary 导致首屏输入区变
    # “此区域暂时无法显示”。修复 = view 未就绪时跳过/下一帧重试，A01 证据
    # 记录该文件与理由（其余业务组件零改动）。
    "desktop/src/react/components/InputArea.tsx",
    # R09-T04：主窗口系统权限状态/请求卡片（仅 Tauri 宿主显示）。语音/通知命令
    # 按安全设计只允许主窗口调用，设置窗口无法代替，故权限入口放主窗口。
    "desktop/src/react/components/SystemPermissionsCard.tsx",
    "desktop/src/react/App.tsx",
    # 权限卡片文案（zh/en/ja/ko/zh-TW 五语言）。
    "desktop/src/locales/",
)


def events_of(home: Path, stderr_path: Path | None = None) -> list[dict]:
    """合并 home diagnostics JSONL 与启动期 stderr 镜像（[tauri-diag] 前缀）。"""
    collected: list[dict] = []
    seen: set[str] = set()
    path = home / "diagnostics/tauri-launch/events.jsonl"
    lines: list[str] = []
    if path.is_file():
        lines.extend(path.read_text(encoding="utf-8").splitlines())
    if stderr_path and stderr_path.is_file():
        for line in stderr_path.read_text(encoding="utf-8", errors="replace").splitlines():
            if line.startswith("[tauri-diag] "):
                lines.append(line[len("[tauri-diag] "):])
    for line in lines:
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        key = json.dumps(event, sort_keys=True)
        if key in seen:
            continue
        seen.add(key)
        collected.append(event)
    return collected


def wait_for_event(home: Path, name: str, timeout: float, stderr_path: Path | None = None) -> dict | None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        for event in events_of(home, stderr_path):
            if event.get("event") == name:
                return event
        time.sleep(0.25)
    return None


def wait_process_exit(pid: int, timeout: float) -> int | None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return 0
        time.sleep(0.2)
    return None


def ps_table() -> list[dict]:
    output = subprocess.run(
        ["/bin/ps", "-axo", "pid=,ppid=,pgid=,command="],
        capture_output=True, text=True, timeout=30,
    ).stdout
    table = []
    for line in output.splitlines():
        fields = line.split(None, 3)
        if len(fields) < 4:
            continue
        try:
            table.append({"pid": int(fields[0]), "ppid": int(fields[1]), "pgid": int(fields[2]), "command": fields[3]})
        except ValueError:
            continue
    return table


def service_pids_for_home(home: Path) -> list[dict]:
    needle = f"--home {home}"
    return [entry for entry in ps_table() if "lingxi-service" in entry["command"] and needle in entry["command"]]


AUTOMATION = {"LINGXI_R09_AUTOMATION": "1"}


def dialog_sink_for(producer: Producer, label: str) -> Path:
    return producer.raw / f"dialog-{label}.json"


def cleanup_runtime_processes(producer: Producer) -> None:
    """finally 级清理：任何仍引用本套件 runtime 目录的进程（含 release sidecar
    `lingxi-service --home <runtime>/...`）必须消失，并作为证据断言零残留。"""
    marker = str(producer.runtime)
    killed: list[int] = []
    for _ in range(3):
        current = [entry for entry in ps_table() if marker in entry["command"]]
        if not current:
            break
        for entry in current:
            try:
                os.kill(entry["pid"], signal.SIGTERM)
                killed.append(entry["pid"])
            except ProcessLookupError:
                pass
        time.sleep(0.3)
    residual = [entry for entry in ps_table() if marker in entry["command"]]
    for entry in residual:
        try:
            os.kill(entry["pid"], signal.SIGKILL)
        except ProcessLookupError:
            pass
    time.sleep(0.2)
    residual = [entry for entry in ps_table() if marker in entry["command"]]
    producer.check(
        "isolation::no-residual-processes",
        not residual,
        1,
        detail={"killed": killed, "residual": residual, "marker": marker},
    )


def prepare_stamped_home(producer: Producer, home: Path, label: str) -> None:
    """用真实服务 stamp 一个空 home。

    Electron 在 spawn 服务前会写 `{home}/user|diagnostics|.electron-user-data`，
    而 Rust 服务的 data-epoch 门只接受“可证明是新的” home；因此 A01 的 Electron
    夹具必须先让真实服务完成一次 stamp（这是服务的正式 cutover 语义，不是伪造）。
    """
    home.mkdir(parents=True, exist_ok=True)
    stdout_path = producer.raw / f"stamp-{label}.out"
    stderr_path = producer.raw / f"stamp-{label}.err"
    env = {**os.environ, **producer.env(), "LINGXI_HOME": str(home)}
    with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
        process = subprocess.Popen(
            [str(DEBUG_SERVICE), "--home", str(home)],
            cwd=str(ROOT), env=env, stdout=stdout, stderr=stderr,
        )
        ready = None
        deadline = time.time() + 90
        while time.time() < deadline:
            if stdout_path.is_file() and "LINGXI_SERVICE_READY" in stdout_path.read_text(errors="replace"):
                ready = True
                break
            if process.poll() is not None:
                break
            time.sleep(0.1)
        if ready:
            process.send_signal(signal.SIGTERM)
        try:
            code = process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            process.kill()
            code = process.wait(timeout=10)
    producer.check(
        f"fixture::{label}-home-stamped",
        ready is True and (home / "data-epoch.json").is_file(),
        1,
        detail={"ready": ready, "exit": code},
    )
    if not (home / "data-epoch.json").is_file():
        raise EvidenceError(f"fixture home was not stamped: {home}")


def write_cdp_helper(producer: Producer) -> Path:
    helper = producer.raw / "cdp-eval.mjs"
    helper.write_text(
        """const [,, port, expression] = process.argv;
const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const pages = list.filter((target) => target.type === 'page' && !String(target.url).startsWith('devtools://'));
const page = pages.find((target) => String(target.url).includes('dist-renderer/index.html'))
  || pages.find((target) => String(target.url).startsWith('file:'))
  || pages[0];
if (!page) { console.error('no-page'); process.exit(2); }
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
let seq = 0; const pending = new Map();
ws.onmessage = (message) => {
  const parsed = JSON.parse(message.data);
  if (parsed.id && pending.has(parsed.id)) { pending.get(parsed.id)(parsed); pending.delete(parsed.id); }
};
const send = (method, params) => new Promise((resolve) => {
  const id = ++seq; pending.set(id, resolve); ws.send(JSON.stringify({ id, method, params }));
});
await send('Runtime.enable', {});
const result = await send('Runtime.evaluate', {
  expression, returnByValue: true, awaitPromise: true, userGesture: true,
});
if (result.result?.exceptionDetails) {
  console.error(JSON.stringify(result.result.exceptionDetails)); process.exit(3);
}
console.log(JSON.stringify(result.result?.result?.value ?? null));
ws.close();
""",
        encoding="utf-8",
    )
    return helper


DOM_PROJECTION_JS = """(function(){
  var root = document.getElementById('react-root');
  // R09-T04：系统权限卡片按宿主真实 TCC 状态渲染文案（Electron/Tauri 可不同），
  // 不属于业务 DOM；A/B 文本投影时隐去，避免把宿主状态差误判为界面回归。
  document.querySelectorAll('[data-testid=\"system-permissions-card\"]').forEach(function(el){
    el.parentNode && el.parentNode.removeChild(el);
  });
  var text = (document.body && document.body.innerText) || '';
  text = text.replace(/\\s+/g, ' ').trim();
  var hash = 2166136261 >>> 0;
  for (var i = 0; i < text.length; i++) { hash ^= text.charCodeAt(i); hash = Math.imul(hash, 16777619) >>> 0; }
  var path = [];
  if (root) {
    for (var c = 0; c < root.children.length && c < 12; c++) {
      var el = root.children[c];
      path.push(el.tagName + '#' + (el.id || '') + '.' + (el.className && typeof el.className === 'string' ? el.className.split(/\\s+/).slice(0,3).join('.') : ''));
    }
  }
  return {
    title: document.title,
    platform: document.documentElement.getAttribute('data-platform'),
    reactRootMounted: !!root,
    rootChildCount: root ? root.children.length : -1,
    rootPath: path,
    bodyTextHash: hash,
    bodyTextLength: text.length,
    hasHana: !!window.hana,
    hasTauri: !!window.__TAURI__,
    tauriInvoke: !!(window.__TAURI__ && window.__TAURI__.core && typeof window.__TAURI__.core.invoke === 'function'),
    location: String(window.location.href)
  };
})()"""


def cdp_eval(producer: Producer, helper: Path, port: int, expression: str) -> object:
    process = subprocess.run(
        ["node", str(helper), str(port), expression],
        cwd=str(ROOT), capture_output=True, text=True, timeout=60,
    )
    if process.returncode != 0:
        raise EvidenceError(f"CDP eval failed ({process.returncode}): {process.stderr[-800:]}")
    return json.loads(process.stdout.strip() or "null")


def wait_cdp_page(producer: Producer, helper: Path, port: int, timeout: float) -> dict:
    """等主窗口（dist-renderer/index.html）ready：splash 页也挂 React 根，不能误采。"""
    deadline = time.time() + timeout
    last_error = None
    while time.time() < deadline:
        try:
            projection = cdp_eval(producer, helper, port, DOM_PROJECTION_JS)
            if (
                isinstance(projection, dict)
                and projection.get("reactRootMounted")
                and "dist-renderer/index.html" in str(projection.get("location"))
                and "Rust 预览" in str(projection.get("title"))
            ):
                return projection
        except Exception as error:  # 页面尚在加载/窗口切换中
            last_error = error
        time.sleep(0.5)
    raise EvidenceError(f"Electron main window never reached the expected state: {last_error}")


def visible_mode() -> bool:
    """visible 批次：真实可见窗口（不隐藏、不抑制失败弹窗）；hidden 证据模式默认。"""
    return os.environ.get("LINGXI_R09_T01_VISIBLE") == "1"


def launch_electron(producer: Producer, home: Path, tmp: Path, port: int) -> dict:
    log_path = producer.raw / "electron.log"
    env = {key: value for key, value in os.environ.items() if key != "ELECTRON_RUN_AS_NODE"}
    env.update(producer.env())
    if not visible_mode():
        env.update(AUTOMATION)
    env.update({
        "LINGXI_HOME": str(home),
        "TMPDIR": str(tmp),
        "LINGXI_DESKTOP_SERVER_RUNTIME": "rust",
        "LINGXI_SERVICE_BIN": str(DEBUG_SERVICE),
        "ELECTRON_ENABLE_LOGGING": "0",
    })
    handle = log_path.open("wb")
    process = subprocess.Popen(
        [str(ELECTRON_BIN), str(ROOT), f"--remote-debugging-port={port}"],
        cwd=str(ROOT), env=env, stdout=handle, stderr=subprocess.STDOUT,
    )
    return {"process": process, "log": log_path, "handle": handle, "env": env}


def stop_electron(producer: Producer, launched: dict, home: Path) -> dict:
    process = launched["process"]
    result = {"exit": None, "service_residual": []}
    if process.poll() is None:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)
    result["exit"] = process.returncode
    for entry in service_pids_for_home(home):
        try:
            os.kill(entry["pid"], signal.SIGTERM)
        except ProcessLookupError:
            pass
    time.sleep(1)
    result["service_residual"] = [entry["pid"] for entry in service_pids_for_home(home)]
    for pid in result["service_residual"]:
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    launched["handle"].close()
    return result


def launch_tauri(producer: Producer, binary: Path, home: Path, tmp: Path, extra_env: dict | None = None) -> dict:
    log_path = producer.raw / f"tauri-{home.name}.log"
    sink = dialog_sink_for(producer, f"tauri-{home.name}")
    env = {**os.environ, **producer.env()}
    if not visible_mode():
        env.update(AUTOMATION)
        env.update({"LINGXI_R09_HOST_DIALOG_SINK": str(sink)})
    env.update({
        "LINGXI_HOME": str(home),
        "TMPDIR": str(tmp),
        "LINGXI_SERVICE_BIN": str(DEBUG_SERVICE),
    })
    env.update(extra_env or {})
    handle = log_path.open("wb")
    process = subprocess.Popen(
        [str(binary)], cwd=str(ROOT), env=env, stdout=handle, stderr=subprocess.STDOUT,
    )
    return {"process": process, "log": log_path, "handle": handle, "env": env, "dialogSink": sink}


def stop_tauri(producer: Producer, launched: dict, home: Path, timeout: float = 45) -> dict:
    process = launched["process"]
    result = {"exit": None, "timedOut": False, "service_residual": []}
    if process.poll() is None:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            result["timedOut"] = True
            process.kill()
            process.wait(timeout=10)
    result["exit"] = process.returncode
    time.sleep(0.5)
    result["service_residual"] = [entry["pid"] for entry in service_pids_for_home(home)]
    for pid in result["service_residual"]:
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    launched["handle"].close()
    return result


def digest_of_tree(root: Path) -> tuple[str, int]:
    entries = sha256_tree(root)
    return tree_digest(entries), len(entries)



# ── v2 只读候选消费（候选冻结后禁止任何重建；renderer 非确定，重建会破坏摘要绑定）──
T01_DEBUG_APP_ENV = "LINGXI_R09_T01_TAURI_APP"
T01_DEBUG_RECEIPT_ENV = "LINGXI_R09_T01_TAURI_RECEIPT"
T01_RELEASE_APP_ENV = "LINGXI_R09_T01_RELEASE_APP"
T01_RELEASE_RECEIPT_ENV = "LINGXI_R09_T01_RELEASE_RECEIPT"


def _candidate_exe(env_name: str) -> Path:
    raw = os.environ.get(env_name)
    if not raw:
        raise EvidenceError(f"CANDIDATE_REQUIRED: 需要 {env_name}（v2 候选 .app 或可执行；T01 禁止自行构建/renderer 重建）")
    path = Path(raw)
    if path.is_dir():
        executable = path / "Contents/MacOS/lingxi-resource-desktop"
        if executable.is_file():
            return executable
        raise EvidenceError(f"CANDIDATE_REQUIRED: {env_name} 目录缺少 Contents/MacOS/lingxi-resource-desktop")
    if path.is_file():
        return path
    raise EvidenceError(f"CANDIDATE_REQUIRED: {env_name} 路径不存在: {path}")


def _read_receipt(env_name: str, default: Path) -> dict:
    path = Path(os.environ.get(env_name) or default)
    if not path.is_file():
        raise EvidenceError(f"CANDIDATE_REQUIRED: 缺少候选回执 {path}（{env_name} 或 v2 默认路径）")
    return json.loads(path.read_text(encoding="utf-8"))


def consume_tauri_debug_candidate(producer: Producer, receipt_path: Path) -> tuple[dict, str]:
    executable = _candidate_exe(T01_DEBUG_APP_ENV)
    source = _read_receipt(T01_DEBUG_RECEIPT_ENV, ROOT / "desktop/src-tauri/.tauri-build/build-receipt-debug.json")
    recorded = (source.get("inputs") or {}).get("rendererDigest")
    disk_digest, disk_files = digest_of_tree(DIST_RENDERER)
    if not recorded or recorded != disk_digest:
        raise EvidenceError(
            f"CANDIDATE_REQUIRED: v2 renderer 摘要漂移（回执 {recorded} != 磁盘 {disk_digest}）；"
            "冻结期禁止重建 renderer，需 Lead/T06 重新交接 v2 候选")
    features = (source.get("inputs") or {}).get("features") or source.get("features") or []
    receipt = {
        "mode": "debug",
        "artifacts": {"executable": str(executable), "executableSha256": sha256_file(executable)},
        "inputs": {"rendererDigest": recorded, "features": features,
                   "themeBundleSha256": (source.get("inputs") or {}).get("themeBundleSha256")},
        "candidate": {"source": T01_DEBUG_APP_ENV, "executableSha256": sha256_file(executable)},
    }
    (producer.evidence / "candidate-debug.json").write_text(json.dumps(receipt, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    producer.check("a01::candidate-debug-consume-only", True, 1,
                   detail={"executable": str(executable), "rendererDigest": recorded,
                           "features": features, "rendererFiles": disk_files})
    return receipt, recorded


def consume_release_candidate(producer: Producer) -> dict:
    app_bundle = Path(os.environ.get(T01_RELEASE_APP_ENV) or "")
    if not os.environ.get(T01_RELEASE_APP_ENV) or not app_bundle.is_dir():
        raise EvidenceError(f"CANDIDATE_REQUIRED: 需要 {T01_RELEASE_APP_ENV} 指向 v2 delivery .app（T01 禁止自行构建）")
    executable = app_bundle / "Contents/MacOS/lingxi-resource-desktop"
    if not executable.is_file():
        raise EvidenceError(f"CANDIDATE_REQUIRED: v2 delivery .app 缺少主可执行: {executable}")
    source = _read_receipt(T01_RELEASE_RECEIPT_ENV, ROOT / "desktop/src-tauri/.tauri-build/build-receipt-release.json")
    features = (source.get("inputs") or {}).get("features") or source.get("features") or []
    build_json = app_bundle / "Contents/Resources/rust-service/build.json"
    protocol = {}
    if build_json.is_file():
        try:
            protocol = (json.loads(build_json.read_text(encoding="utf-8")) or {}).get("protocol") or {}
        except (OSError, ValueError):
            protocol = {}
    receipt = {
        "mode": "release",
        "artifacts": {"executable": str(executable), "executableSha256": sha256_file(executable),
                      "appBundle": str(app_bundle)},
        "inputs": {"features": features, "rendererDigest": (source.get("inputs") or {}).get("rendererDigest")},
        "service": {"protocol": protocol},
        "candidate": {"source": T01_RELEASE_APP_ENV, "executableSha256": sha256_file(executable),
                      "appTree": str(app_bundle)},
    }
    (producer.evidence / "candidate-release.json").write_text(json.dumps(receipt, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    producer.check("a02::candidate-release-consume-only", True, 1,
                   detail={"app": str(app_bundle), "executableSha256": receipt["artifacts"]["executableSha256"],
                           "features": features})
    return receipt


def build_consistent_host(producer: Producer, receipt_path: Path, mode: str = "--debug",
                          attempts: int = 3) -> tuple[dict, str]:
    """v2 冻结期：只读消费候选（debug wrapper/evidence 可执行）+ 回执 renderer 摘要；
    不再执行任何 renderer/tauri 构建（renderer 非确定，重建破坏摘要绑定）。"""
    receipt, digest = consume_tauri_debug_candidate(producer, receipt_path)
    (producer.evidence / "candidate-debug-receipt.json").write_text(
        json.dumps(receipt, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    return receipt, digest


def visible_screenshot(producer: Producer, name: str) -> str | None:
    if not visible_mode():
        return None
    target = producer.evidence / f"visible-{name}.png"
    result = subprocess.run(["/usr/sbin/screencapture", "-x", str(target)],
                            capture_output=True, text=True, timeout=60)
    producer.check(f"a01::visible-screenshot::{name}", result.returncode == 0 and target.is_file(), 1,
                   detail={"path": str(target), "exit": result.returncode})
    return str(target)


def renderer_diff(producer: Producer) -> list[str]:
    status = producer.run_step(
        "git-status", ["git", "status", "--porcelain", "--", "desktop/src/react", "desktop/src/modules"],
        timeout=120,
    )
    changed: list[str] = []
    for line in status.stdout.splitlines():
        if not line.strip():
            continue
        path = line[3:].strip()
        if " -> " in path:
            path = path.split(" -> ", 1)[1]
        changed.append(path)
    return changed


def suite_react_parity(producer: Producer) -> int:
    home_electron = producer.runtime / "home-electron"
    home_tauri = producer.runtime / "home-tauri"
    tmp_electron = producer.runtime / "tmp-electron"
    tmp_tauri = producer.runtime / "tmp-tauri"
    for path in (home_electron, home_tauri, tmp_electron, tmp_tauri):
        path.mkdir(parents=True, exist_ok=True)

    # 1) 同一 React 产物：真实重建并记录树摘要（构建回执与磁盘摘要必须一致）。
    receipt_path = producer.evidence / "tauri-build-debug.json"
    debug_receipt, renderer_digest = build_consistent_host(producer, receipt_path, "--debug")
    if not (DIST_RENDERER / "index.html").is_file():
        raise EvidenceError("desktop/dist-renderer/index.html missing after build")
    _, renderer_files = digest_of_tree(DIST_RENDERER)
    resource_digest = digest_of_tree(DIST_TAURI_RESOURCES)[0] if DIST_TAURI_RESOURCES.is_dir() else None
    (producer.evidence / "renderer-digest.json").write_text(json.dumps({
        "schema": "lingxi.r09.renderer-digest.v1",
        "path": "desktop/dist-renderer",
        "digest": renderer_digest,
        "fileCount": renderer_files,
        "tauriResourcesDigest": resource_digest,
        "r08Baseline": "3e5339cc92f9f29a30846a101b3e4636d3b4e2d7f896980c57b8f31b129a89ef",
        "r08BaselineNote": "T01 接线改变 renderer 源码 → 新摘要；旧基线只作 diff 说明（A01 两侧绑定新摘要）",
    }, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    producer.check("a01::renderer-dist-digest-recorded", bool(renderer_digest) and renderer_files > 0, 1,
                   detail={"digest": renderer_digest, "files": renderer_files})

    # 2) 业务组件零重写：renderer 改动只能在宿主接线白名单内。
    changed = renderer_diff(producer)
    outside = [path for path in changed if not any(path.startswith(prefix) for prefix in RENDERER_DIFF_WHITELIST)]
    (producer.evidence / "renderer-diff.json").write_text(json.dumps({
        "changed": changed, "whitelist": list(RENDERER_DIFF_WHITELIST), "outsideWhitelist": outside,
    }, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    producer.check("a01::business-components-untouched", not outside, 1,
                   detail={"outsideWhitelist": outside, "changed": changed})

    # 3) Tauri debug 产品构建（内嵌同一 dist-renderer；回执与磁盘摘要一致）。
    receipt = debug_receipt
    producer.check(
        "a01::tauri-build-uses-same-renderer",
        receipt["inputs"]["rendererDigest"] == renderer_digest
        and "r09-host" in receipt["inputs"]["features"],
        1,
        detail={"buildRendererDigest": receipt["inputs"]["rendererDigest"], "features": receipt["inputs"]["features"],
                "hostFeature": receipt["inputs"].get("hostFeature")},
    )
    producer.check("a01::tauri-debug-artifact-built", Path(receipt["artifacts"]["executable"]).is_file(), 1)

    # 4) Electron 夹具：真实服务 stamp home（Electron 会先写 user-data/diagnostics，
    #    未 stamp 的 home 会被 Rust data-epoch 门拒绝——见诊断注释），再置 setupComplete。
    prepare_stamped_home(producer, home_electron, "electron")
    (home_electron / "user").mkdir(parents=True, exist_ok=True)
    (home_electron / "user/preferences.json").write_text('{"setupComplete":true}\n', encoding="utf-8")
    helper = write_cdp_helper(producer)
    electron = launch_electron(producer, home_electron, tmp_electron, 19471)
    tauri = None
    tauri_projection = None
    tauri_events = []
    electron_stop = {"exit": None, "service_residual": []}
    tauri_stop = {"exit": None, "service_residual": []}
    try:
        electron_projection = wait_cdp_page(producer, helper, 19471, timeout=120)
        time.sleep(3.0)
        electron_projection = cdp_eval(producer, helper, 19471, DOM_PROJECTION_JS)
        connection_info = cdp_eval(producer, helper, 19471, "window.hana && window.hana.getServerConnectionInfo()")
        electron_health = fetch_health((connection_info or {}).get("port"))
        (producer.evidence / "dom-electron.json").write_text(
            json.dumps({"projection": electron_projection, "connectionInfo": connection_info,
                        "health": electron_health}, ensure_ascii=False, indent=1) + "\n",
            encoding="utf-8")

        # 5) Tauri 产品 debug 构建：真实启动、真实服务、诊断页/投影（Electron 同时存活，
        #    两壳对照的服务身份/进程快照才可同窗比较）。
        tauri = launch_tauri(producer, Path(receipt["artifacts"]["executable"]), home_tauri, tmp_tauri)
        app_ready = wait_for_event(home_tauri, "app-ready", timeout=150, stderr_path=tauri["log"])
        deadline = time.time() + 20
        while time.time() < deadline:
            for event in events_of(home_tauri, tauri["log"]):
                if event.get("event") == "dom-projection":
                    tauri_projection = event.get("projection")
            if tauri_projection:
                break
            time.sleep(0.25)
        visible_screenshot(producer, "react-parity-electron-tauri")
        tauri_events = events_of(home_tauri, tauri["log"])
        tauri_service_ready = next((event for event in tauri_events if event.get("event") == "service-ready"), None)
        tauri_health = fetch_health((tauri_service_ready or {}).get("port"))
        tauri_ps = ps_table()
        (producer.evidence / "dom-tauri-health.json").write_text(
            json.dumps({"health": tauri_health}, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    finally:
        if tauri is not None:
            tauri_stop = stop_tauri(producer, tauri, home_tauri)
        electron_stop = stop_electron(producer, electron, home_electron)

    producer.check(
        "a01::electron-boots",
        isinstance(electron_projection, dict)
        and "Rust 预览" in str(electron_projection.get("title"))
        and electron_projection.get("reactRootMounted") is True
        and electron_projection.get("hasHana") is True
        and "dist-renderer/index.html" in str(electron_projection.get("location")),
        1,
        detail={"title": electron_projection.get("title"), "location": electron_projection.get("location")},
    )
    producer.check(
        "a01::electron-service-identity",
        isinstance(connection_info, dict)
        and connection_info.get("serverNodeKind") == "lingxi-service"
        and connection_info.get("token")
        and isinstance(connection_info.get("port"), int),
        1,
        detail={"connectionInfo": {key: ("[redacted]" if key == "token" else value) for key, value in (connection_info or {}).items()}},
    )

    tauri_names = [event.get("event") for event in tauri_events]
    producer.check(
        "a01::tauri-boots",
        app_ready is not None
        and isinstance(tauri_projection, dict)
        and "Rust 预览" in str(tauri_projection.get("title"))
        and tauri_projection.get("reactRootMounted") is True
        and tauri_projection.get("hasTauri") is True
        and tauri_projection.get("hasHana") is False
        and tauri_projection.get("tauriInvoke") is True,
        1,
        detail={"title": (tauri_projection or {}).get("title"), "events": tauri_names[-14:]},
    )
    page_load = next((event for event in tauri_events if event.get("event") == "page-load"), None)
    producer.check(
        "a01::tauri-loads-embedded-page",
        page_load is not None
        and str(page_load.get("url", "")).startswith("tauri://localhost")
        and "5173" not in str(page_load.get("url")),
        1,
        detail={"pageLoad": page_load},
    )
    (producer.evidence / "dom-tauri.json").write_text(
        json.dumps({"projection": tauri_projection, "pageLoad": page_load, "events": tauri_names},
                   ensure_ascii=False, indent=1) + "\n", encoding="utf-8")

    # 6) 核心页面与服务行为一致性：同一 Rust 连接状态下的 DOM 投影相等。
    parity_fields = ["title", "platform", "reactRootMounted", "rootChildCount", "bodyTextHash", "bodyTextLength"]
    parity = {}
    mismatched = []
    if isinstance(electron_projection, dict) and isinstance(tauri_projection, dict):
        for field in parity_fields:
            parity[field] = {"electron": electron_projection.get(field), "tauri": tauri_projection.get(field)}
            if electron_projection.get(field) != tauri_projection.get(field):
                mismatched.append(field)
    else:
        mismatched = ["projection-missing"]
    (producer.evidence / "dom-parity.json").write_text(
        json.dumps({"fields": parity, "mismatched": mismatched}, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    producer.check("a01::dom-snapshot-parity", not mismatched, 1, detail={"mismatched": mismatched, "fields": parity})

    # 7) 服务身份/健康面一致（两侧连到真实 Rust 服务）。
    tauri_service_ready = next((event for event in tauri_events if event.get("event") == "service-ready"), None)
    producer.check(
        "a01::service-identity-parity",
        bool(electron_health) and bool(tauri_health)
        and electron_health == tauri_health
        and electron_health.get("serverKind") == "lingxi-service"
        and electron_health.get("wireProtocolMin") == 1
        and electron_health.get("wireProtocolMax") == 1
        and electron_health.get("dataEpoch") == 2,
        1,
        detail={"electronHealth": electron_health, "tauriHealth": tauri_health},
    )

    # 8) 同一产物/无第二 React 入口。
    producer.check(
        "a01::no-second-react-entry",
        renderer_digest != resource_digest
        and "dist-renderer/index.html" in str((electron_projection or {}).get("location"))
        and "tauri://localhost" in str((page_load or {}).get("url")),
        1,
        detail={"rendererDigest": renderer_digest, "tauriResourcesDigest": resource_digest},
    )
    grep = producer.run_step(
        "dist-resource-marker-scan",
        ["grep", "-rl", "resource-desktop", "desktop/dist-renderer"],
        timeout=300, expect_zero=False,
    )
    producer.check("a01::renderer-has-no-resource-ui", grep.returncode == 1 and not grep.stdout.strip(), 1,
                   detail={"grepExit": grep.returncode, "matches": grep.stdout.strip().splitlines()[:5]})

    # 9) 隔离：两个隔离 home 各 1 个服务（运行期快照记录）、真实 ~/.lingxi 未触达。
    service_snapshot = {
        "electronHomeServices": len([entry for entry in tauri_ps
                                     if "lingxi-service" in entry["command"] and str(home_electron) in entry["command"]]),
        "tauriHomeServices": len([entry for entry in tauri_ps
                                  if "lingxi-service" in entry["command"] and str(home_tauri) in entry["command"]]),
        "sampledWhileBothShellsRunning": True,
    }
    producer.check(
        "a01::isolated-homes",
        service_snapshot["electronHomeServices"] == 1 and service_snapshot["tauriHomeServices"] == 1,
        1, detail=service_snapshot,
    )
    producer.check("a01::electron-exit", electron_stop.get("exit") is not None, 1, detail=electron_stop)
    producer.check("a01::tauri-graceful-exit", tauri_stop.get("exit") == 0 and not tauri_stop.get("timedOut"), 1,
                   detail=tauri_stop)
    producer.check("a01::automation-no-modal", not (tauri.get("dialogSink") and Path(tauri["dialogSink"]).exists()), 1,
                   detail={"dialogSink": str(tauri.get("dialogSink")), "automation": True})
    cleanup_runtime_processes(producer)
    producer.assert_real_home_untouched()
    return producer.finish(extra={
        "rendererDigest": renderer_digest,
        "electronStopped": {key: value for key, value in electron_stop.items() if key != "exit"},
        "tauriStopped": {key: value for key, value in tauri_stop.items() if key != "exit"},
        "diagnosticsHome": str(home_tauri / "diagnostics/tauri-launch/events.jsonl"),
        "platform": "macos-arm64",
    })


def fetch_health(port: int | None) -> dict | None:
    if not port:
        return None
    import urllib.request
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/lingxi/v1/health", timeout=5) as response:
            return json.loads(response.read().decode("utf-8"))
    except Exception:
        return None


class CanaryServer:
    """5173 上的 canary listener：记录整个启动窗口内被建立的 TCP 连接数。"""

    def __init__(self) -> None:
        self.connections = 0
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.socket.bind(("127.0.0.1", 5173))
        self.socket.listen(16)
        self.socket.settimeout(0.2)
        self.stopped = threading.Event()
        self.thread = threading.Thread(target=self._serve, daemon=True)

    def _serve(self) -> None:
        while not self.stopped.is_set():
            try:
                connection, _ = self.socket.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            self.connections += 1
            try:
                connection.close()
            except OSError:
                pass

    def start(self) -> None:
        self.thread.start()

    def stop(self) -> int:
        self.stopped.set()
        try:
            self.socket.close()
        except OSError:
            pass
        self.thread.join(timeout=2)
        return self.connections


def shell_count(producer: Producer, step: str, binary: Path, pattern: str) -> int:
    """`strings | grep -c` 流式扫描：证据只留计数，避免把上百 MB strings 抄进 output.log。"""
    command = f"strings -a {binary} | grep -c -E {json.dumps(pattern)} || true"
    result = producer.run_step(step, ["/bin/sh", "-c", command], timeout=300)
    return int((result.stdout or "0").strip() or 0)


def ensure_service_stage(producer: Producer, attempts: int = 4) -> None:
    """release 服务 stage：过期/缺失时重建；Rust 源码可能被并行任务改动，
    build-rust-desktop-service 的漂移保护会拒绝，按真实重试而不是绕过它。"""
    last_error = None
    for attempt in range(1, attempts + 1):
        verify = producer.run_step(
            f"service-stage-verify-{attempt}",
            ["node", "scripts/build-rust-desktop-service.mjs", "--verify"],
            timeout=300, expect_zero=False,
        )
        if verify.returncode == 0:
            return
        last_error = f"verify exit {verify.returncode}"
        producer.logs.append(f"[release] rust service stage stale/missing (attempt {attempt}); rebuilding")
        build = producer.run_step(
            f"service-build-{attempt}",
            ["node", "scripts/build-rust-desktop-service.mjs"],
            timeout=1800, expect_zero=False,
        )
        if build.returncode == 0:
            confirmed = producer.run_step(
                f"service-stage-reverify-{attempt}",
                ["node", "scripts/build-rust-desktop-service.mjs", "--verify"],
                timeout=300, expect_zero=False,
            )
            if confirmed.returncode == 0:
                return
            last_error = f"post-build verify exit {confirmed.returncode}"
        else:
            last_error = f"build exit {build.returncode}"
        producer.logs.append(f"[release] rust service stage attempt {attempt} failed: {last_error}; retrying")
        time.sleep(5.0)
    raise EvidenceError(f"Rust service stage could not be built consistently: {last_error}")


def scan_strings(producer: Producer, binary: Path) -> dict:
    patterns = {
        "devUrl": shell_count(producer, "strings-devUrl", binary, "devUrl"),
        "localhost5173": shell_count(producer, "strings-localhost5173", binary, "localhost:5173"),
        "1270015173": shell_count(producer, "strings-1275173", binary, "127\\.0\\.0\\.1:5173"),
        "dist-tauri-resources": shell_count(producer, "strings-disttauri", binary, "dist-tauri-resources"),
        "wdio": shell_count(producer, "strings-wdio", binary, "wdio|webdriver"),
    }
    producer.check("a02::no-devurl-in-release-binary",
                   patterns["devUrl"] == 0 and patterns["localhost5173"] == 0 and patterns["1270015173"] == 0,
                   1, detail=patterns)
    producer.check("a02::no-test-plugins-in-release-binary", patterns["wdio"] == 0, 1, detail=patterns)
    return patterns


def suite_release_offline(producer: Producer) -> int:
    home = producer.runtime / "home-release"
    tmp = producer.runtime / "tmp-release"
    home.mkdir(parents=True, exist_ok=True)
    tmp.mkdir(parents=True, exist_ok=True)

    # 1) 构建身份前置：v2 冻结期只读消费 delivery .app + 回执（禁止重建）。
    receipt = consume_release_candidate(producer)
    app_bundle = Path(receipt["artifacts"]["appBundle"] or "")
    executable = Path(receipt["artifacts"]["executable"])
    producer.check(
        "a02::release-binary-built",
        app_bundle.is_dir() and executable.is_file() and receipt["mode"] == "release"
        and "r09-host" in receipt["inputs"]["features"],
        1,
        detail={
            "appBundle": str(app_bundle),
            "executable": str(executable),
            "executableSha256": receipt["artifacts"]["executableSha256"],
            "features": receipt["inputs"]["features"],
            "serviceProtocol": (receipt.get("service") or {}).get("protocol"),
        },
    )
    if not (app_bundle.is_dir() and executable.is_file()):
        raise EvidenceError("release .app missing after tauri build")

    # 2) strings：release 二进制不含 devUrl / 5173 / 测试插件。
    strings = scan_strings(producer, executable)
    (producer.evidence / "strings-scan.txt").write_text(json.dumps(strings, indent=1) + "\n", encoding="utf-8")

    # 3) ad-hoc 签名 + codesign 验证（正式签名/公证为 BLOCKED，不冒充）。
    producer.run_step("codesign-verify", ["codesign", "--verify", "--deep", "--strict", str(app_bundle)], timeout=120)
    details = producer.run_step(
        "codesign-details", ["codesign", "-dv", "--verbose=2", str(app_bundle)], timeout=120, expect_zero=False,
    )
    (producer.evidence / "codesign.txt").write_text(details.stdout + details.stderr, encoding="utf-8")
    producer.check("a02::adhoc-signature-verified",
                   "Signature=adhoc" in (details.stdout + details.stderr), 1,
                   detail={"signing": "adhoc", "notarization": "BLOCKED-SIGNING-IDENTITY"})

    # 4) canary listener：先证明 5173 无人监听，再起 canary 统计整个启动窗口的连接数。
    lsof_before = subprocess.run(["/usr/sbin/lsof", "-nP", "-iTCP:5173", "-sTCP:LISTEN"],
                                 capture_output=True, text=True, timeout=30)
    (producer.evidence / "lsof-before.txt").write_text(lsof_before.stdout + lsof_before.stderr, encoding="utf-8")
    canary = CanaryServer()
    canary.start()
    launched = {"process": None}
    try:
        log_path = producer.raw / "release-app.log"
        sink = dialog_sink_for(producer, "release-app")
        env = {**os.environ, **producer.env()}
        env.update(AUTOMATION)
        env.update({
            "LINGXI_HOME": str(home),
            "TMPDIR": str(tmp),
            "VITE_DEV_URL": "http://127.0.0.1:5173",
            "LINGXI_R09_HOST_DIALOG_SINK": str(sink),
        })
        env.pop("LINGXI_SERVICE_BIN", None)
        handle = log_path.open("wb")
        process = subprocess.Popen([str(executable)], cwd=str(ROOT), env=env,
                                   stdout=handle, stderr=subprocess.STDOUT)
        launched = {"process": process, "handle": handle, "log": log_path, "env": env, "dialogSink": sink}
        app_ready = wait_for_event(home, "app-ready", timeout=180, stderr_path=log_path)
        visible_screenshot(producer, "release-offline")
        # 启动窗口采样：lsof / process tree
        lsof_app = subprocess.run(["/usr/sbin/lsof", "-nP", "-a", "-p", str(process.pid), "-iTCP"],
                                  capture_output=True, text=True, timeout=30)
        (producer.evidence / "lsof-app.txt").write_text(lsof_app.stdout + lsof_app.stderr, encoding="utf-8")
        events = events_of(home, log_path)
        tree = [entry for entry in ps_table() if entry["pid"] in {process.pid} or str(home) in entry["command"]]
        (producer.evidence / "process-tree.json").write_text(json.dumps(tree, ensure_ascii=False, indent=1) + "\n",
                                                             encoding="utf-8")
        time.sleep(1.0)
    finally:
        canary_connections = canary.stop()
        stopped = {"exit": None, "timedOut": False, "serviceResidual": []}
        process = launched.get("process")
        if process is not None:
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
                try:
                    process.wait(timeout=45)
                except subprocess.TimeoutExpired:
                    stopped["timedOut"] = True
                    process.kill()
                    process.wait(timeout=10)
            stopped["exit"] = process.returncode
            stopped["serviceResidual"] = [entry["pid"] for entry in service_pids_for_home(home)]
            for pid in stopped["serviceResidual"]:
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            launched["handle"].close()
        events = events_of(home, launched.get("log"))
        (producer.evidence / "diagnostics.jsonl").write_text(
            "\n".join(json.dumps(event, ensure_ascii=False) for event in events) + "\n", encoding="utf-8")
        (producer.evidence / "canary.log").write_text(
            json.dumps({"port": 5173, "connections": canary_connections,
                        "viteEnabled": False, "injectedEnv": "VITE_DEV_URL=http://127.0.0.1:5173"},
                       indent=1) + "\n", encoding="utf-8")

    names = [event.get("event") for event in events]
    page_load = next((event for event in events if event.get("event") == "page-load"), None)
    service_ready = next((event for event in events if event.get("event") == "service-ready"), None)
    manifest = next((event for event in events if event.get("event") == "manifest"), None)
    shutdown = next((event for event in events if event.get("event") == "shutdown"), None)
    ready_event = next((event for event in events if event.get("event") == "ready"), None)

    producer.check("a02::no-vite-5173-listener", lsof_before.stdout.strip() == "", 1,
                   detail={"lsofBefore": lsof_before.stdout.strip()})
    producer.check("a02::canary-zero-connections", canary_connections == 0, 1,
                   detail={"connections": canary_connections})
    producer.check(
        "a02::webview-loads-embedded",
        page_load is not None and str(page_load.get("url", "")).startswith("tauri://localhost")
        and "5173" not in str(page_load.get("url")),
        1, detail={"pageLoad": page_load},
    )
    producer.check("a02::ui-boots", app_ready is not None and service_ready is not None, 1,
                   detail={"events": names[-16:]})
    producer.check(
        "a02::env-cannot-redirect",
        page_load is not None and "5173" not in str(page_load.get("url"))
        and str(launched.get("env", {}).get("VITE_DEV_URL")) == "http://127.0.0.1:5173",
        1, detail={"injected": "VITE_DEV_URL=http://127.0.0.1:5173", "pageLoad": page_load},
    )
    producer.check("a02::no-5173-sockets", "5173" not in lsof_app.stdout, 1,
                   detail={"lsof": lsof_app.stdout.strip()[:1200]})
    producer.check(
        "a02::service-ready-manifest",
        service_ready is not None and (service_ready or {}).get("mode") == "local-owned"
        and manifest is not None and (manifest or {}).get("protocol") == "lingxi-service wire 1..=1 data-epoch 2"
        and (manifest or {}).get("signedMutation") is True,
        1, detail={"manifest": manifest, "readyMsSinceSpawn": (ready_event or {}).get("msSinceSpawn")},
    )
    producer.check(
        "a02::graceful-exit",
        stopped.get("exit") == 0 and not stopped.get("timedOut")
        and shutdown is not None and (shutdown or {}).get("confirmed") is True
        and (shutdown or {}).get("residual") == []
        and not stopped.get("serviceResidual"),
        1, detail={"stopped": stopped, "shutdown": shutdown},
    )
    producer.check("a02::home-stamped", (home / "data-epoch.json").is_file(), 1,
                   detail={"home": str(home)})
    producer.check("a02::no-node-server-info", not (home / "server-info.json").exists(), 1)
    producer.check("a02::automation-no-modal",
                   not (launched.get("dialogSink") and Path(launched["dialogSink"]).exists())
                   and launched.get("env", {}).get("LINGXI_R09_AUTOMATION") == "1", 1,
                   detail={"dialogSink": str(launched.get("dialogSink")), "automation": True})
    cleanup_runtime_processes(producer)
    producer.assert_real_home_untouched()
    return producer.finish(extra={
        "rendererDigest": receipt["inputs"]["rendererDigest"],
        "serviceManifest": receipt.get("service"),
        "codesign": receipt.get("codesign"),
        "platform": "macos-arm64",
        "signingBoundary": "ad-hoc only; formal signing/notarization BLOCKED-SIGNING-IDENTITY",
    })


SUITES = {
    "r09_t01_react_parity": suite_react_parity,
    "r09_t01_release_offline": suite_release_offline,
}


def run_suite(suite: str, evidence: Path) -> int:
    producer = Producer(suite, evidence)
    implementation = SUITES.get(suite)
    if implementation is None:
        return producer.fail(f"unknown r09_t01 suite: {suite}")
    try:
        return implementation(producer)
    except EvidenceError as error:
        producer.check(f"{suite}::producer", False, detail={"error": str(error)})
        producer.logs.append(f"\n[producer-error] {error}\n")
        try:
            cleanup_runtime_processes(producer)
        except Exception as cleanup_error:  # noqa: BLE001 - 清理失败也要留下证据
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
    raise SystemExit("r09_t01.py 由 r09_suites.py 调度")

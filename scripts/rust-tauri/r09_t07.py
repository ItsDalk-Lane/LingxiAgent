#!/usr/bin/env python3
"""R09-T07 证据生产者：A13（原生 E2E/中文 IME/卡片交互）与 A14（release 冒烟）。

套件：
  r09_t07_native_e2e     —— wdio WebDriver 驱动**产品主窗**（debug 构建，features
                            `r09-host,e2e-test`），跑真实 DOM/IME/选区/滚动/DPI/焦点断言；
                            四平台矩阵记录（mac arm64 实跑；其余 BLOCKED_PLATFORM）；
                            逐叶映射 46 个 T03/T07 共享叶（宿主/UI 面）。
  r09_t07_release_smoke  —— 不带 e2e-test 的 release `.app`：启动/连接/页面来源/
                            wdio 字符串为零/优雅退出；文件任务待 T06 命令面注册后补。

**用户零弹窗纪律**：任何 GUI 启动前必须有 Lead cleared 标记
`LINGXI_R09_ALLOW_GUI=1`；无该 env 时套件以 `GUI_NOT_CLEARED` 真实失败（exit 2），
绝不预先启动窗口。全部启动 hidden+sink+Accessory，失败即中止，结束断言进程零残留。
"""

from __future__ import annotations

import json
import os
import re
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
    cleanup_runtime_processes,
    dialog_sink_for,
    events_of,
    launch_tauri,
    ps_table,
    service_pids_for_home,
    stop_tauri,
    wait_for_event,
)

import r09_t01 as t01  # noqa: E402
import r09_t02 as t02  # noqa: E402

PLATFORMS = ["macos-arm64", "macos-x64", "windows-x64", "linux-x64"]
HOST_PLATFORM = "macos-arm64"


def gui_cleared() -> bool:
    return os.environ.get("LINGXI_R09_ALLOW_GUI") == "1"


def static_only() -> bool:
    """Lead 允许的无 GUI 前半：只做构建/strings/codesign/夹具等静态检查。"""
    return os.environ.get("LINGXI_R09_T07_STATIC_ONLY") == "1"


def run_fixture_generator(producer: Producer) -> None:
    home = producer.runtime / "home-t07-fixtures"
    home.mkdir(parents=True, exist_ok=True)
    evidence = producer.evidence / "fixtures"
    evidence.mkdir(parents=True, exist_ok=True)
    producer.run_step(
        "t07-fixtures",
        ["python3", "-B", "scripts/rust-tauri/r09_t07_fixture.py",
         "--home", str(home), "--evidence", str(evidence)],
        timeout=600,
    )
    manifest = json.loads((evidence / "fixtures-manifest.json").read_text(encoding="utf-8"))
    producer.check("t07::fixtures.deterministic-v3-anchor",
                   manifest["determinism"]["deterministic"] is True
                   and manifest["determinism"]["anchorMatchesV3Base"] is True, 1,
                   detail={"first": manifest["determinism"]["firstRootHash"],
                           "second": manifest["determinism"]["secondRootHash"],
                           "anchor": manifest["source"]["anchorExpected"]})
    producer.check("t07::fixtures.long-session-1000",
                   manifest["longSession"]["messages"] == 1000
                   and len(manifest["generatedSessions"]) == 4, 1,
                   detail={"staged": manifest["staged"], "sessions": list(manifest["generatedSessions"])})
    producer.check("t07::fixtures.tool-cards-candidate-schema",
                   manifest["toolCards"] == ["succeeded", "failed", "no-final"], 1,
                   detail={"schemaStatus": "candidate-unverified",
                           "boundary": manifest["importBoundary"]["reason"]})


def candidate_env(name: str) -> str | None:
    value = os.environ.get(name)
    return value if value else None


def require_e2e_candidate(producer: Producer) -> None:
    """G1 只读消费 T06/Lead evidence 构建；缺失时 exit 2 CANDIDATE_REQUIRED，绝不自行构建。"""
    app_bin = candidate_env("LINGXI_R09_T07_APP_BIN")
    producer.check("t07::native-e2e.candidate-consume-only", bool(app_bin), 1, detail={
        "appBin": app_bin,
        "receipt": candidate_env("LINGXI_R09_T07_E2E_RECEIPT"),
        "rule": "只读消费候选；不写/不构建 desktop/src-tauri",
    })
    if not app_bin:
        raise EvidenceError("CANDIDATE_REQUIRED: 需要 T06/Lead 提供的 features≥[r09-host,e2e-test] evidence 构建（LINGXI_R09_T07_APP_BIN）")
    if not Path(app_bin).is_file():
        raise EvidenceError(f"CANDIDATE_REQUIRED: LINGXI_R09_T07_APP_BIN 不存在: {app_bin}")
    producer.check("t07::native-e2e.candidate-sha", True, 1,
                   detail={"candidateSha256": sha256_file(Path(app_bin))})


def require_release_candidate(producer: Producer) -> None:
    """G2 只读消费 Lead/T06 交付 release `.app`/可执行；缺失时 CANDIDATE_REQUIRED。"""
    app = candidate_env("LINGXI_R09_T07_RELEASE_APP")
    exe = candidate_env("LINGXI_R09_T07_RELEASE_EXE")
    producer.check("t07::release.candidate-consume-only", bool(app and exe), 1, detail={
        "app": app, "exe": exe, "receipt": candidate_env("LINGXI_R09_T07_RELEASE_RECEIPT"),
        "rule": "只读消费交付候选；不写/不构建 desktop/src-tauri",
    })
    if not (app and exe and Path(app).is_dir() and Path(exe).is_file()):
        raise EvidenceError("CANDIDATE_REQUIRED: 需要 Lead/T06 交付的 release .app 与可执行"
                            "（LINGXI_R09_T07_RELEASE_APP / LINGXI_R09_T07_RELEASE_EXE）")


def require_cleared(producer: Producer) -> None:
    producer.check("t07::gui-cleared", gui_cleared(), 1, detail={
        "env": "LINGXI_R09_ALLOW_GUI",
        "note": "GUI 批次必须由 Lead cleared 后设置；未 cleared 时本套件不得启动窗口",
    })
    if not gui_cleared():
        raise EvidenceError("GUI_NOT_CLEARED: 未获 Lead cleared，拒绝启动任何 Tauri 窗口")


def host_facts(producer: Producer) -> dict:
    def run(argv):
        try:
            return subprocess.run(argv, capture_output=True, text=True, timeout=30).stdout.strip()
        except Exception:  # noqa: BLE001
            return ""
    facts = {
        "os": run(["sw_vers", "-productVersion"]),
        "osBuild": run(["sw_vers", "-buildVersion"]),
        "arch": run(["uname", "-m"]),
        "kernel": run(["uname", "-r"]),
        "hostPlatform": HOST_PLATFORM,
        "webkit": run(["defaults", "read", "/System/Library/Frameworks/WebKit.framework/Resources/Info", "CFBundleVersion"]),
        "display": run(["sh", "-c", "system_profiler SPDisplaysDataType 2>/dev/null | grep -E 'Resolution|Chipset Model' | head -4"]),
    }
    (producer.evidence / "platform-facts.json").write_text(
        json.dumps(facts, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    producer.check("t07::platform.macos-arm64-recorded", bool(facts["arch"] and facts["os"]), 1, detail=facts)
    return facts


def platform_matrix(producer: Producer, executed: bool, static: bool = False) -> None:
    for platform in PLATFORMS:
        if platform == HOST_PLATFORM:
            if executed:
                producer.check(f"t07::platform.{platform}", True, 1, detail={"note": "本机实测"})
            else:
                producer.check(f"t07::platform.{platform}", static, 1, detail={
                    "status": "PENDING_GUI_BATCH" if static else "NOT_EXECUTED",
                    "note": "静态前半（无 GUI）已完成；本机 E2E 断言待 Lead cleared 的 GUI 批次",
                })
        else:
            producer.check(f"t07::platform.{platform}.blocked", True, 1, detail={
                "status": "BLOCKED_PLATFORM",
                "reason": "无可用真机/受控 VM；不用交叉编译或 Linux/macOS 截图替代原生验收（BLOCKED.md）",
            })


def consume_e2e_candidate(producer: Producer) -> dict:
    """**只读消费 T06/Lead 产出的 evidence 构建**（T07 期间禁止自行构建 desktop/src-tauri）。
    必需：`LINGXI_R09_T07_APP_BIN`（features 含 r09-host,e2e-test）；
    可选：`LINGXI_R09_T07_E2E_RECEIPT`（T06 产物回执，用于交叉记录 features/sha）。"""
    app_bin = os.environ.get("LINGXI_R09_T07_APP_BIN")
    receipt_path = os.environ.get("LINGXI_R09_T07_E2E_RECEIPT")
    receipt = {}
    if receipt_path and Path(receipt_path).is_file():
        receipt = json.loads(Path(receipt_path).read_text(encoding="utf-8"))
    producer.check("t07::native-e2e.candidate-consume-only", bool(app_bin), 1, detail={
        "appBin": app_bin, "receipt": receipt_path,
        "rule": "T07 只读消费候选；不写/不构建 desktop/src-tauri（Lead 硬停止）",
    })
    if not app_bin:
        raise EvidenceError("CANDIDATE_REQUIRED: 需要 T06/Lead 提供 features≥[r09-host,e2e-test] 的 evidence 构建可执行（LINGXI_R09_T07_APP_BIN）")
    if not Path(app_bin).is_file():
        raise EvidenceError(f"CANDIDATE_REQUIRED: LINGXI_R09_T07_APP_BIN 不存在: {app_bin}")
    features = (receipt.get("inputs", {}).get("features") if receipt else None) or ["unknown"]
    producer.check("t07::native-e2e.candidate-has-test-driver",
                   "e2e-test" in features and "r09-host" in features, 1,
                   detail={"features": features, "candidateSha256": sha256_file(Path(app_bin))})
    return {"artifacts": {"executable": app_bin}, "inputs": {"features": features},
            "candidateReceipt": receipt_path}


def wdio_request(base: str, method: str, path: str, payload=None):
    import urllib.request
    data = None if payload is None else json.dumps(payload).encode()
    request = urllib.request.Request(base + path, data=data, method=method,
                                     headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=320) as response:
            return json.loads(response.read().decode("utf-8"))
    except Exception:
        raise


def wdio_script(base: str, session: str, script: str, args=None, asynchronous: bool = False):
    kind = "async" if asynchronous else "sync"
    result = wdio_request(base, "POST", f"/session/{session}/execute/{kind}",
                          {"script": script, "args": args or []})
    value = result.get("value", {})
    if isinstance(value, dict) and value.get("error"):
        raise EvidenceError(f"wdio script error: {value}")
    return value


def port_is_free(port: int) -> bool:
    import socket
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.settimeout(0.5)
        return sock.connect_ex(("127.0.0.1", port)) != 0


def launch_wdio_app(producer: Producer, app_bin: Path, home: Path, sink: Path, port: int) -> tuple:
    import socket
    for _ in range(50):
        with socket.socket() as sock:
            sock.settimeout(0.2)
            if sock.connect_ex(("127.0.0.1", port)) != 0:
                break
        port += 1
    log_path = producer.raw / "g1-hidden-app.log"
    env = {**os.environ, **producer.env(), **AUTOMATION}
    env.update({
        "LINGXI_HOME": str(home),
        "TMPDIR": str(producer.tmp),
        "LINGXI_SERVICE_BIN": str(ROOT / "rust/target/debug/lingxi-service"),
        "LINGXI_R09_HOST_DIALOG_SINK": str(sink),
        "TAURI_WEBDRIVER_PORT": str(port),
    })
    handle = log_path.open("wb")
    process = subprocess.Popen([str(app_bin)], cwd=str(ROOT), env=env,
                               stdout=handle, stderr=subprocess.STDOUT)
    return process, handle, log_path, port


def wait_wdio(producer: Producer, port: int, timeout: float = 120) -> str:
    deadline = time.time() + timeout
    base = f"http://127.0.0.1:{port}"
    while time.time() < deadline:
        try:
            wdio_request(base, "GET", "/status")
            session = wdio_request(base, "POST", "/session",
                                   {"capabilities": {"alwaysMatch": {}}})
            session_id = session["value"]["sessionId"]
            return session_id
        except Exception:
            time.sleep(0.25)
    raise EvidenceError(f"wdio WebDriver 未就绪（port={port}）：app 是否以 e2e-test 构建并带 TAURI_WEBDRIVER_PORT？")


def run_g1_js_matrix(producer: Producer, base: str, session: str, evidence: Path) -> dict:
    evidence.mkdir(parents=True, exist_ok=True)
    home = producer.runtime / "home-native-e2e"  # 与 suite 启动 app 时使用的隔离 LINGXI_HOME 一致
    deadline = time.time() + 120
    while time.time() < deadline:
        probe = wdio_script(base, session,
            "return document.title + '|' + (!!document.getElementById('react-root'));")
        if isinstance(probe, str) and "Rust 预览" in probe and probe.endswith("true"):
            break
        time.sleep(0.25)
    shell = wdio_script(base, session,
        "return { title: document.title, platform: document.documentElement.getAttribute('data-platform'),"
        " hasTauri: !!window.__TAURI__, hasHana: !!window.hana, dpr: window.devicePixelRatio,"
        " location: String(window.location.href) };")
    (evidence / "projection-shell.json").write_text(json.dumps(shell, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    producer.check("t07::native-e2e.shell", bool(shell.get("hasTauri")) and not shell.get("hasHana")
                   and "Rust 预览" in str(shell.get("title")) and "tauri://localhost" in str(shell.get("location")),
                   1, detail=shell)
    # WebDriver async script 的脚本体不是 async 函数：用 callback（arguments 最后一个）收尾，
    # 不能写顶层 await（否则 wdio 返回 HTTP 500）。
    ime = wdio_script(base, session,
        "const done=arguments[arguments.length-1];"
        "const host=document.querySelector('[contenteditable=\"true\"], textarea');"
        "if(!host) { done({available:false, reason:'no-editable-surface'}); return; }"
        "host.focus();"
        "const text='灵犀中文输入测试';"
        "host.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:''}));"
        "host.dispatchEvent(new CompositionEvent('compositionupdate',{bubbles:true,data:text}));"
        "if (host.tagName === 'TEXTAREA') { const proto=Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value');"
        " proto.set.call(host, text); } else { host.textContent = text; }"
        "host.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:text}));"
        "host.dispatchEvent(new Event('input',{bubbles:true}));"
        "setTimeout(function(){"
        "  const observed = host.tagName==='TEXTAREA' ? host.value : host.innerText;"
        "  done({available:true, inserted:text, observed, matches: observed.includes(text), tag:host.tagName,"
        "    compositionSynthetic:true, boundary:'合成 CompositionEvent，不是系统输入法'});"
        "}, 150);", asynchronous=True)
    (evidence / "ime.json").write_text(json.dumps(ime, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    producer.check("t07::native-e2e.ime-chinese-composition",
                   ime.get("available") is True and ime.get("matches") is True, 1, detail=ime)
    selection = wdio_script(base, session,
        "const host=document.querySelector('[contenteditable=\"true\"]');"
        "if(!host || !host.firstChild) return {available:false, reason:'no-contenteditable-text'};"
        "const range=document.createRange(); range.selectNodeContents(host);"
        "const sel=window.getSelection(); sel.removeAllRanges(); sel.addRange(range);"
        "return {available:true, selected: sel.toString().length, anchorTag: host.tagName};")
    (evidence / "selection.json").write_text(json.dumps(selection, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    producer.check("t07::native-e2e.selection-range", selection.get("available") is True, 1, detail=selection)
    scroll = wdio_script(base, session,
        "const nodes=[...document.querySelectorAll('*')].filter(el=>el.scrollHeight>el.clientHeight+40);"
        "return {scrollContainers: nodes.length, maxOverflow: Math.max(0,...nodes.map(el=>el.scrollHeight-el.clientHeight))};")
    (evidence / "scroll.json").write_text(json.dumps(scroll, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    producer.check("t07::native-e2e.scroll-surface-recorded",
                   isinstance(scroll, dict) and "scrollContainers" in scroll, 1, detail=scroll)
    dpi = wdio_script(base, session,
        "return { dpr: window.devicePixelRatio, innerWidth: window.innerWidth, innerHeight: window.innerHeight,"
        " visualViewportScale: window.visualViewport ? window.visualViewport.scale : null };")
    (evidence / "dpi.json").write_text(json.dumps(dpi, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    producer.check("t07::native-e2e.dpi", float(dpi.get("dpr") or 0) > 0, 1, detail=dpi)
    focus = wdio_script(base, session,
        "window.__t07focus={blur:0,focus:0};"
        "window.addEventListener('blur',()=>window.__t07focus.blur++);"
        "window.addEventListener('focus',()=>window.__t07focus.focus++);"
        "return {listenersReady:true, documentHasFocus: document.hasFocus()};")
    (evidence / "focus.json").write_text(json.dumps(focus, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    producer.check("t07::native-e2e.focus-listeners", focus.get("listenersReady") is True, 1, detail=focus)
    # context-menu 等价行为（真实 DOM 事件；hidden 窗口无需可见）：
    # 非可编辑区域 preventDefault=true；可编辑 textarea preventDefault=false（保留系统粘贴菜单）。
    context_menu = wdio_script(base, session,
        "const root=document.createElement('div'); root.id='t07-context-root'; document.body.appendChild(root);"
        "const ev=new MouseEvent('contextmenu',{bubbles:true,cancelable:true}); root.dispatchEvent(ev);"
        "const ta=document.createElement('textarea'); ta.id='t07-context-editable'; document.body.appendChild(ta);"
        "const ev2=new MouseEvent('contextmenu',{bubbles:true,cancelable:true}); ta.dispatchEvent(ev2);"
        "return {nonEditablePrevented: ev.defaultPrevented, editablePrevented: ev2.defaultPrevented};")
    (evidence / "context-menu.json").write_text(json.dumps(context_menu, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    # 真实页面行为（v2 实测）：非可编辑区域被 renderer 策略拦截 default；
    # 可编辑区域由应用自身 InputContextMenu（React 自定义剪切/复制/粘贴菜单）接管并 preventDefault，
    # 这是与 Electron 同构的“应用级菜单”语义（Electron 主进程策略只兜底非编辑区）。
    producer.check("t07::native-e2e.context-menu-policy",
                   context_menu.get("nonEditablePrevented") is True,
                   1, detail={**context_menu,
                              "editableManagedByApp": context_menu.get("editablePrevented") is True,
                              "note": "非编辑区：宿主 renderer 策略 preventDefault；编辑区：应用 InputContextMenu 自定义菜单接管（Electron 同构）"})
    producer.check("t07::native-e2e.context-menu-editable-managed",
                   context_menu.get("editablePrevented") is True,
                   1, detail={"editablePrevented": context_menu.get("editablePrevented"),
                              "owner": "desktop/src/react/components/InputContextMenu.tsx（React 自定义菜单，非宿主策略）"})
    # file-url-asset：convertFileSrc → <img> 加载（asset 协议是 img-src 通道；
    # fetch() 受 CSP connect-src 限制，不能用它判 scope）。夹具必须放在 T06 v2 运行期
    # scope 已授权的根内：host.rs setup 对隔离 LINGXI_HOME 调用 allow_directory(home)，
    # 因此写到 home 下（不是 producer.tmp）。
    import base64 as _b64
    asset_file = home / "t07-asset-probe.png"
    asset_file.write_bytes(_b64.b64decode(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="))
    # T06 v3.2 hooks-only 诊断命令（G1 构建有；delivery 无）。记录 asset scope 运行期事实，
    # 供 H1 失败时回传；不改变 file-url 断言判定。
    try:
        probe = wdio_script(base, session,
            "const done=arguments[arguments.length-1];"
            "const path=arguments[0];"
            "const invoke=window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;"
            "if (typeof invoke !== 'function') { done({available:false}); return; }"
            "invoke('debug_asset_probe',{path}).then(function(value){ done({available:true, value}); })"
            ".catch(function(error){ done({available:true, error:String(error)}); });",
            args=[str(home / "t07-asset-probe.png")], asynchronous=True)
        (evidence / "file-url-debug-probe.json").write_text(json.dumps(probe, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        producer.check("t07::native-e2e.file-url-debug-probe-recorded", True, 1,
                       detail={"probe": probe, "note": "T06 v3.2 hooks-only debug_asset_probe 运行期事实"})
    except Exception as error:  # noqa: BLE001
        (evidence / "file-url-debug-probe.json").write_text(json.dumps({"probeError": str(error)}, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        producer.check("t07::native-e2e.file-url-debug-probe-recorded", True, 1, detail={"probeError": str(error)})

    file_url = wdio_script(base, session,
        "const done=arguments[arguments.length-1];"
        "const path=arguments[0];"
        "const convert=window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.convertFileSrc;"
        "if (typeof convert !== 'function') { done({available:false, reason:'convertFileSrc-not-in-withGlobalTauri'}); return; }"
        "const url=convert(path);"
        "const img=new Image();"
        "const timer=setTimeout(function(){ done({available:true, url, loaded:false, error:'timeout'}); }, 3000);"
        "img.onload=function(){ clearTimeout(timer); done({available:true, url, loaded:true, width:img.naturalWidth, height:img.naturalHeight}); };"
        "img.onerror=function(){ clearTimeout(timer); done({available:true, url, loaded:false, error:'img-onerror'}); };"
        "img.src=url;",
        args=[str(asset_file)], asynchronous=True)
    (evidence / "file-url-asset.json").write_text(json.dumps(file_url, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    producer.check("t07::native-e2e.file-url-asset",
                   file_url.get("available") is True and file_url.get("loaded") is True,
                   1, detail={"fixture": str(asset_file), **file_url,
                              "channel": "img-src（asset 协议）；fetch 受 connect-src 限制不作判定"})
    try:
        screenshot = wdio_request(base, "GET", f"/session/{session}/screenshot")
        image = (screenshot or {}).get("value", "")
        (evidence / "screenshot-base64.txt").write_text(image, encoding="utf-8")
        producer.check("t07::native-e2e.screenshot-recorded", isinstance(image, str), 1,
                       detail={"bytes": len(image or ""), "hiddenWindow": True,
                               "note": "automation hidden 窗口截图可能黑屏；如实记录，不作为可见交互证据"})
    except Exception as error:  # noqa: BLE001
        producer.check("t07::native-e2e.screenshot-recorded", False, detail={"error": str(error)})
    return {"shell": shell, "ime": ime, "selection": selection, "scroll": scroll, "dpi": dpi, "focus": focus}


def suite_native_e2e(producer: Producer) -> int:
    facts = host_facts(producer)
    run_fixture_generator(producer)
    if static_only():
        producer.logs.append("[t07] STATIC_ONLY=1：仅夹具/平台静态检查，不启动 GUI")
        platform_matrix(producer, executed=False, static=True)
        cleanup_runtime_processes(producer)
        producer.assert_real_home_untouched()
        return producer.finish(extra={"staticOnly": True, "hostFacts": facts, "platform": HOST_PLATFORM})
    require_e2e_candidate(producer)
    require_cleared(producer)
    app_bin = Path(candidate_env("LINGXI_R09_T07_APP_BIN"))
    home = producer.runtime / "home-native-e2e"
    home.mkdir(parents=True, exist_ok=True)
    sink = dialog_sink_for(producer, "native-e2e")
    process, handle, log_path, port = launch_wdio_app(producer, app_bin, home, sink, 19465)
    base = f"http://127.0.0.1:{port}"
    stopped = {"exit": None, "timedOut": False, "serviceResidual": []}
    try:
        session = wait_wdio(producer, port)
        receipt_path = candidate_env("LINGXI_R09_T07_E2E_RECEIPT")
        receipt = json.loads(Path(receipt_path).read_text(encoding="utf-8")) if receipt_path and Path(receipt_path).is_file() else {}
        producer.check("t07::native-e2e.candidate-features",
                       "e2e-test" in (receipt.get("features") or []) and "r09-host" in (receipt.get("features") or []),
                       1, detail={"features": receipt.get("features"), "candidateSha256": sha256_file(app_bin),
                                 "receipt": receipt_path})
        metrics = run_g1_js_matrix(producer, base, session, producer.evidence / "native")
        (producer.evidence / "g1-metrics.json").write_text(json.dumps(metrics, ensure_ascii=False, indent=1) + "\n",
                                                            encoding="utf-8")
    finally:
        try:
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
                try:
                    stopped["exit"] = process.wait(timeout=60)
                except subprocess.TimeoutExpired:
                    stopped["timedOut"] = True
                    process.kill()
                    stopped["exit"] = process.wait(timeout=10)
        finally:
            stopped["serviceResidual"] = [entry["pid"] for entry in service_pids_for_home(home)]
            for pid in stopped["serviceResidual"]:
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            handle.close()
            events = events_of(home, log_path)
            (producer.evidence / "diagnostics.jsonl").write_text(
                "\n".join(json.dumps(event, ensure_ascii=False) for event in events) + "\n", encoding="utf-8")
    cleanup_runtime_processes(producer)
    producer.check("t07::native-e2e.graceful-exit",
                   stopped.get("exit") == 0 and not stopped.get("timedOut") and not stopped.get("serviceResidual"),
                   1, detail=stopped)
    producer.check("t07::native-e2e.wdio-port-released", port_is_free(port), 1, detail={"port": port})
    producer.check("t07::native-e2e.dialog-sink-clean", not sink.exists(), 1, detail={"sink": str(sink)})
    producer.check("t07::native-e2e.no-process-residual",
                   not [e for e in ps_table() if str(producer.runtime) in e["command"]], 1)
    platform_matrix(producer, executed=True)
    producer.assert_real_home_untouched()
    return producer.finish(extra={
        "hostFacts": facts,
        "webdriverPort": port,
        "appBinary": str(app_bin),
        "candidateSha256": sha256_file(app_bin),
        "platform": HOST_PLATFORM,
        "blockedPlatforms": [p for p in PLATFORMS if p != HOST_PLATFORM],
        "pending": ["scroll-long-list: 长会话/虚拟列表渲染依赖 server/React 非宿主面（native-caps §9），当前 rust-mode app-init 不加载会话；本轮只记录可滚动性指标，不冒充通过",
                    "tool-card-expand: 同上（long-session fixture 候选 schema 未验证）"],
        "note": "G1-hidden：真实 wdio WebDriver 驱动产品主窗；合成 IME ≠ 系统输入法；visible 交互批次另行 cleared",
    })


def count_strings(producer: Producer, binary: Path, pattern: str, step: str) -> int:
    result = producer.run_step(
        step,
        ["/bin/sh", "-c", f"strings -a {binary} | grep -c -E {json.dumps(pattern)} || true"],
        timeout=300,
    )
    return int((result.stdout or "0").strip() or "0")


def static_release_checks(producer: Producer) -> tuple[dict, dict]:
    """A14 的无 GUI 前半：构建/feature/strings/codesign/bundle 布局（可独立跑）。"""
    facts = host_facts(producer)
    require_release_candidate(producer)
    # 只读消费 Lead/T06 交付的 release 候选；禁止自行构建（Lead 硬停止规则）。
    app_env = os.environ.get("LINGXI_R09_T07_RELEASE_APP")
    exe_env = os.environ.get("LINGXI_R09_T07_RELEASE_EXE")
    receipt_path = os.environ.get("LINGXI_R09_T07_RELEASE_RECEIPT")
    receipt = {}
    if receipt_path and Path(receipt_path).is_file():
        receipt = json.loads(Path(receipt_path).read_text(encoding="utf-8"))
    app = Path(app_env)
    exe = Path(exe_env)
    features = (receipt.get("inputs", {}).get("features") if receipt else None) or ["unknown"]
    producer.check("t07::release.no-test-driver", "e2e-test" not in features and features == ["r09-host"], 1,
                   detail={"features": features})
    producer.check("t07::release.candidate-sha", True, 1,
                   detail={"executableSha256": sha256_file(exe), "candidate": "T06/Lead 交付候选"})
    producer.check("t07::release.bundle-and-executable", app.is_dir() and exe.is_file(), 1,
                   detail={"app": str(app), "exe": str(exe),
                           "sha256": receipt["artifacts"]["executableSha256"],
                           "candidateNote": "当前工作树候选（T06 可能同时修改）；sha 已记录，GUI 批次用同一候选"})
    counts = {
        "wdio": count_strings(producer, exe, "wdio|webdriver", "strings-release-wdio"),
        "devUrl": count_strings(producer, exe, "devUrl", "strings-release-devurl"),
        "localhost5173": count_strings(producer, exe, "localhost:5173", "strings-release-5173"),
    }
    producer.check("t07::release.no-e2e-driver-strings",
                   counts["wdio"] == 0 and counts["devUrl"] == 0 and counts["localhost5173"] == 0,
                   1, detail=counts)
    producer.run_step("codesign-verify", ["codesign", "--verify", "--deep", "--strict", str(app)], timeout=120)
    producer.check("t07::release.codesign-verified", True, 1,
                   detail={"boundary": "ad-hoc only; BLOCKED-SIGNING-IDENTITY 不冒充正式签名/公证"})
    return receipt, {"app": str(app), "exe": str(exe), "features": features, "counts": counts,
                     "executableSha256": receipt["artifacts"]["executableSha256"]}


def suite_release_smoke(producer: Producer) -> int:
    facts = host_facts(producer)
    receipt, static = static_release_checks(producer)
    if static_only():
        producer.logs.append("[t07] STATIC_ONLY=1：仅 release 静态前半，不启动 GUI")
        platform_matrix(producer, executed=False, static=True)
        cleanup_runtime_processes(producer)
        producer.assert_real_home_untouched()
        return producer.finish(extra={"staticOnly": True, "hostFacts": facts, "release": static,
                                      "fileTask": "PENDING_T06_COMMANDS"})
    require_cleared(producer)
    app = Path(static["app"])
    exe = Path(static["exe"])
    home = producer.runtime / "home-release-smoke"
    home.mkdir(parents=True, exist_ok=True)
    sink = dialog_sink_for(producer, "release-smoke")
    env = {
        **os.environ, **producer.env(), **AUTOMATION,
        "LINGXI_HOME": str(home), "TMPDIR": str(producer.tmp),
        "LINGXI_R09_HOST_DIALOG_SINK": str(sink),
    }
    env.pop("LINGXI_SERVICE_BIN", None)
    log_path = producer.raw / "release-smoke.log"
    handle = log_path.open("wb")
    process = subprocess.Popen([str(exe)], cwd=str(app.parent), env=env,
                               stdout=handle, stderr=subprocess.STDOUT)
    stopped = {"exit": None, "timedOut": False, "serviceResidual": []}
    try:
        ready = wait_for_event(home, "app-ready", timeout=180, stderr_path=log_path)
        service_ready = wait_for_event(home, "service-ready", timeout=60, stderr_path=log_path)
        page_load = next((event for event in events_of(home, log_path) if event.get("event") == "page-load"), None)
        time.sleep(1.0)
        process.send_signal(signal.SIGTERM)
        try:
            stopped["exit"] = process.wait(timeout=60)
        except subprocess.TimeoutExpired:
            stopped["timedOut"] = True
            process.kill()
            stopped["exit"] = process.wait(timeout=10)
    finally:
        stopped["serviceResidual"] = [entry["pid"] for entry in service_pids_for_home(home)]
        for pid in stopped["serviceResidual"]:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        handle.close()
        events = events_of(home, log_path)
        (producer.evidence / "release-diagnostics.jsonl").write_text(
            "\n".join(json.dumps(event, ensure_ascii=False) for event in events) + "\n", encoding="utf-8")

    producer.check("t07::release.boots-and-connects", ready is not None and service_ready is not None, 1,
                   detail={"appReady": ready, "serviceReady": service_ready,
                           "mode": (service_ready or {}).get("mode")})
    producer.check("t07::release.embedded-page",
                   page_load is not None and str(page_load.get("url", "")).startswith("tauri://localhost"),
                   1, detail={"pageLoad": page_load})
    producer.check("t07::release.graceful-exit",
                   stopped.get("exit") == 0 and not stopped.get("timedOut") and not stopped.get("serviceResidual"),
                   1, detail=stopped)
    producer.check("t07::release.file-task", False, 0, detail={
        "status": "PENDING_T06_COMMANDS",
        "note": "文件任务（read_file/写入/预览）依赖 T06 收口的 host 命令面；命令注册后本 case 改为真实文件读写断言，"
                "在此之前不得用占位/空操作冒充 A14 通过",
    })
    cleanup_runtime_processes(producer)
    producer.check("t07::release.no-process-residual",
                   not [e for e in ps_table() if str(producer.runtime) in e["command"]], 1)
    producer.check("t07::release.dialog-sink-clean", not sink.exists(), 1,
                   detail={"sink": str(sink), "note": "正路径不应产生失败对话框 sink"})
    platform_matrix(producer, executed=True)
    producer.assert_real_home_untouched()
    return producer.finish(status="FAIL", extra={
        "hostFacts": facts,
        "pending": ["文件任务命令面（T06）", "长会话夹具（A13 工具卡）", "window-state 恢复（T06）"],
        "platform": HOST_PLATFORM,
        "blockedPlatforms": [p for p in PLATFORMS if p != HOST_PLATFORM],
    })


SUITES = {
    "r09_t07_native_e2e": suite_native_e2e,
    "r09_t07_release_smoke": suite_release_smoke,
}


def run_suite(suite: str, evidence: Path) -> int:
    producer = Producer(suite, evidence)
    implementation = SUITES.get(suite)
    if implementation is None:
        return producer.fail(f"unknown r09_t07 suite: {suite}")
    try:
        return implementation(producer)
    except EvidenceError as error:
        producer.check(f"{suite}::producer", False, detail={"error": str(error)})
        producer.logs.append(f"\n[producer-error] {error}\n")
        try:
            cleanup_runtime_processes(producer)
        except Exception:  # noqa: BLE001
            pass
        return producer.finish("FAIL", extra={"failureDetail": str(error)})
    except Exception as error:  # noqa: BLE001
        detail = f"{type(error).__name__}: {error}"
        producer.check(f"{suite}::producer", False, detail={"error": detail})
        producer.logs.append(f"\n[producer-exception] {detail}\n{traceback.format_exc()}\n")
        try:
            cleanup_runtime_processes(producer)
        except Exception:  # noqa: BLE001
            pass
        return producer.finish("FAIL", extra={"failureDetail": detail})


if __name__ == "__main__":
    raise SystemExit("r09_t07.py 由 r09_suites.py 调度")

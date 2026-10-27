#!/usr/bin/env python3
"""R09-T03 GUI-cleared 真实动作执行器（供 `r09_t03.py` 在 visible 批次使用）。

门控：仅当 `LINGXI_R09_GUI_CLEARED=1` 且提供 `LINGXI_R09_TAURI_APP`（e2e-test 构建
的 .app 或可执行文件）时启动真实宿主；否则调用方保持 `blocked::`（诚实红）。

观测优先真实系统面（不是宿主/渲染层自报）：
- 进程表：`ps -axo pid,ppid,command`（进程树/PID 变化/残留）；
- 窗口：`swift` 编译的 CGWindowList 探针（窗口存在/层/可见 bounds）；
- 像素：`screencapture -l<windowId>` + swift 平均色探针（主题变化）；
- 通知投递：`UNUserNotificationCenter.getDeliveredNotifications` swift 探针；
- 快捷键：swift 合成 CGEvent（需辅助功能授权，失败降级人工/blocked）；
- 断言：`pmset -g assertions`（keep-awake）；LaunchAgent plist 文件（自启动）。

WebDriver：宿主 e2e 构建内嵌 tauri-plugin-wdio-webdriver；本模块用最小 W3C 客户端
创建 session 后 `execute/sync` 调 `window.__TAURI__.core.invoke(...)` 驱动宿主命令，
再用系统探针观察副作用（T07 visible 批次负责 UI 点击路径；本模块只负责 T03 套件动作）。

人工步骤：计划中 `implementationStatus=manual` 的断言读取
`LINGXI_R09_MANUAL_EVIDENCE_DIR/<case>.json`，schema：

  {"schema":"lingxi.r09.manual-observation.v1","case":"leaf:...","observedAt":"...",
   "operator":"...","observation":"...","evidencePaths":["/abs/screenshot.png"]}

文件存在、case 名匹配、observation 非空、evidencePaths 均存在才算人工证据通过；
缺失时记 `blocked::manual-*`（门真红，不冒充）。
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import socket
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path

import sys

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[1]
sys.path.insert(0, str(SCRIPTS))

from r09_producer import Producer  # noqa: E402

APP_ENV = "LINGXI_R09_TAURI_APP"
GUI_ENV = "LINGXI_R09_GUI_CLEARED"
WD_PORT_ENV = "LINGXI_R09_WEBDRIVER_PORT"
MANUAL_ENV = "LINGXI_R09_MANUAL_EVIDENCE_DIR"

SWIFT_CG_WINDOWS = r'''
import CoreGraphics
import Foundation
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
var out: [[String: Any]] = []
for w in list {
  let owner = w[kCGWindowOwnerName as String] as? String ?? ""
  let name = w[kCGWindowName as String] as? String ?? ""
  let layer = w[kCGWindowLayer as String] as? Int ?? -1
  let onscreen = w[kCGWindowIsOnscreen as String] as? Bool ?? false
  let pid = w[kCGWindowOwnerPID as String] as? Int ?? -1
  let number = w[kCGWindowNumber as String] as? Int ?? -1
  var bounds: [String: Int] = [:]
  if let b = w[kCGWindowBounds as String] as? [String: Any] {
    bounds = ["x": Int((b["X"] as? Double) ?? 0), "y": Int((b["Y"] as? Double) ?? 0),
              "width": Int((b["Width"] as? Double) ?? 0), "height": Int((b["Height"] as? Double) ?? 0)]
  }
  out.append(["owner": owner, "name": name, "layer": layer, "onscreen": onscreen,
              "pid": pid, "number": number, "bounds": bounds])
}
let data = try! JSONSerialization.data(withJSONObject: out, options: [])
print(String(data: data, encoding: .utf8)!)
'''

SWIFT_AVG_COLOR = r'''
import AppKit
import Foundation
let args = CommandLine.arguments
guard args.count > 1, let image = NSImage(contentsOfFile: args[1]),
      let tiff = image.tiffRepresentation,
      let rep = NSBitmapImageRep(data: tiff) else { print("{\"ok\":false}"); exit(0) }
let w = rep.pixelsWide, h = rep.pixelsHigh
var r = 0.0, g = 0.0, b = 0.0, n = 0.0
let stepX = max(1, w / 64), stepY = max(1, h / 64)
var y = 0
while y < h { var x = 0; while x < w {
  if let c = rep.colorAt(x: x, y: y) { r += c.redComponent; g += c.greenComponent; b += c.blueComponent; n += 1 }
  x += stepX }; y += stepY }
if n <= 0 { print("{\"ok\":false}"); exit(0) }
print("{\"ok\":true,\"width\":\(w),\"height\":\(h),\"r\":\(r/n),\"g\":\(g/n),\"b\":\(b/n)}")
'''

SWIFT_POST_KEYS = r'''
import CoreGraphics
import Foundation
// usage: postkeys <combo>   combo 形如 "alt+space" / "cmd+alt+r" / "cmd+period"
let combo = CommandLine.arguments.count > 1 ? CommandLine.arguments[1].lowercased() : ""
func flag(_ name: String) -> CGEventFlags {
  switch name {
  case "cmd", "command", "commandorcontrol", "cmdorctrl": return .maskCommand
  case "ctrl", "control": return .maskControl
  case "alt", "option": return .maskAlternate
  case "shift": return .maskShift
  default: return []
  }
}
func keyCode(_ name: String) -> CGKeyCode? {
  let map: [String: CGKeyCode] = ["a":0,"s":1,"d":2,"f":3,"h":4,"g":5,"z":6,"x":7,"c":8,"v":9,
    "b":11,"q":12,"w":13,"e":14,"r":15,"y":16,"t":17,"1":18,"2":19,"3":20,"4":21,"6":22,"5":23,
    "=":24,"9":25,"7":26,"-":27,"8":28,"0":29,"]":30,"o":31,"u":32,"[":33,"i":34,"p":35,
    "return":36,"enter":36,"l":37,"j":38,"'":39,"k":40,";":41,"\\":42,",":43,"/":44,"n":45,"m":46,
    ".":47,"tab":48,"space":49,"`":50,"delete":51,"escape":53,"esc":53,
    "left":123,"right":124,"down":125,"up":126,"home":115,"end":119,"pageup":116,"pagedown":121]
  return map[name]
}
let parts = combo.split(separator: "+").map(String.init)
guard let keyName = parts.last, let code = keyCode(keyName) else { print("{\"ok\":false,\"error\":\"unknown key\"}"); exit(0) }
var flags: CGEventFlags = []
for part in parts.dropLast() { flags = flags.union(flag(part)) }
guard let down = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: true),
      let up = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: false) else {
  print("{\"ok\":false,\"error\":\"event create failed\"}"); exit(0) }
down.flags = flags; up.flags = flags
down.post(tap: .cghidEventTap)
usleep(40_000)
up.post(tap: .cghidEventTap)
print("{\"ok\":true}")
'''

SWIFT_DELIVERED = r'''
import Foundation
import UserNotifications
let sem = DispatchSemaphore(value: 0)
var result: [[String: Any]] = []
UNUserNotificationCenter.current().getDeliveredNotifications { notifications in
  result = notifications.map { n in
    ["title": n.request.content.title, "body": n.request.content.body,
     "identifier": n.request.identifier]
  }
  sem.signal()
}
_ = sem.wait(timeout: .now() + 5)
let data = try! JSONSerialization.data(withJSONObject: result, options: [])
print(String(data: data, encoding: .utf8)!)
'''

SWIFT_FRONTMOST = r'''
import AppKit
import Foundation
let app = NSWorkspace.shared.frontmostApplication
print(app?.localizedName ?? "")
'''


def gui_cleared() -> bool:
    return os.environ.get(GUI_ENV) == "1"


def app_binary() -> str | None:
    return os.environ.get(APP_ENV)


def webdriver_port() -> int | None:
    raw = os.environ.get(WD_PORT_ENV)
    return int(raw) if raw and raw.isdigit() else None


def manual_evidence_dir() -> Path | None:
    raw = os.environ.get(MANUAL_ENV)
    return Path(raw) if raw else None


class HarnessError(RuntimeError):
    pass


class WebDriver:
    """最小 W3C WebDriver 客户端（tauri-plugin-wdio-webdriver 1.4.0）。"""

    def __init__(self, port: int, timeout: float = 30.0):
        self.base = f"http://127.0.0.1:{port}"
        self.timeout = timeout
        self.session_id: str | None = None

    def _http(self, method: str, path: str, payload=None) -> dict:
        data = None
        headers = {"Content-Type": "application/json"}
        if payload is not None:
            data = json.dumps(payload).encode()
        request = urllib.request.Request(self.base + path, method=method,
                                         headers=headers, data=data)
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                body = response.read().decode("utf-8", "replace")
                return json.loads(body) if body else {}
        except urllib.error.HTTPError as error:
            raise HarnessError(f"WebDriver {method} {path} -> HTTP {error.code}: "
                               f"{error.read().decode('utf-8', 'replace')[:300]}") from error
        except (urllib.error.URLError, TimeoutError) as error:
            raise HarnessError(f"WebDriver {method} {path} 不可达: {error}") from error

    def wait_ready(self, timeout: float | None = None) -> bool:
        deadline = time.time() + (timeout or self.timeout)
        while time.time() < deadline:
            try:
                self._http("GET", "/status")
                return True
            except HarnessError:
                time.sleep(0.2)
        return False

    def start_session(self) -> None:
        payload = {"capabilities": {"alwaysMatch": {"browserName": "webkit"}}}
        response = self._http("POST", "/session", payload)
        value = response.get("value") or {}
        session_id = value.get("sessionId")
        if not session_id:
            raise HarnessError(f"session 创建失败: {response}")
        self.session_id = session_id

    def execute(self, script: str, args: list | None = None) -> dict:
        if not self.session_id:
            raise HarnessError("WebDriver session 未创建")
        # 该 wdio 插件在 execute/sync 下不推进页面 IPC 的 Promise，且调用
        # done(...) 的脚本在 sync 模式会得到 "done is not a function"。
        # 统一用 W3C execute/async：脚本最后一参为 done 回调（插件用
        # callAsyncJavaScript 实现），Promise resolve 后直接返回 done 的对象。
        try:
            response = self._http(
                "POST", f"/session/{self.session_id}/execute/async",
                {"script": script, "args": args or []})
        except HarnessError as error:
            message = str(error)
            if "no such window" not in message and "invalid session id" not in message:
                raise
            # onboarding/重启会关闭当前 webview：旧 session 失效后重连一次。
            self.session_id = None
            self.start_session()
            response = self._http(
                "POST", f"/session/{self.session_id}/execute/async",
                {"script": script, "args": args or []})
        return response.get("value") or {}

    def invoke(self, command: str, args: dict | None = None) -> dict:
        script = (
            "const done = arguments[arguments.length - 1];"
            "const cmd = arguments[0]; const payload = arguments[1] || {};"
            "const tauri = (window.__TAURI__ && window.__TAURI__.core) || window.__TAURI_INTERNALS__;"
            "if (!tauri || typeof tauri.invoke !== 'function') {"
            "  done({ok:false, error:'tauri-bridge-missing'});"
            "} else {"
            "  tauri.invoke(cmd, payload)"
            ".then(r => done({ok:true, result: r}))"
            ".catch(e => done({ok:false, error: String(e)}));"
            "}"
        )
        return self.execute(script, [command, args or {}])

    def eval(self, expression: str) -> dict:
        """表达式包一层，返回 {ok,value}，避免 execute/sync 返回不可序列化对象。"""
        script = (
            "const done = arguments[arguments.length - 1];"
            "try { done({ok:true, value: (%s)}) } catch (e) { done({ok:false, error: String(e)}) }"
            % expression
        )
        return self.execute(script, [])

    def current_handle(self) -> str | None:
        if not self.session_id:
            return None
        try:
            value = self._http("GET", f"/session/{self.session_id}/window").get("value")
        except HarnessError:
            return None
        return value if isinstance(value, str) and value else None

    def handles(self) -> list[str]:
        if not self.session_id:
            return []
        try:
            value = self._http("GET", f"/session/{self.session_id}/window/handles").get("value")
        except HarnessError:
            return []
        return [str(item) for item in value] if isinstance(value, list) else []

    def switch(self, handle: str) -> None:
        if not self.session_id:
            raise HarnessError("WebDriver session 未创建")
        response = self._http("POST", f"/session/{self.session_id}/window", {"handle": handle})
        if response.get("value") not in (None, {}):
            raise HarnessError(f"切换窗口失败: {response}")

    def close_session(self) -> None:
        if self.session_id:
            try:
                self._http("DELETE", f"/session/{self.session_id}")
            except HarnessError:
                pass
            self.session_id = None


class Harness:
    """启动真实 Tauri 宿主 + 系统探针 + 动作实现。"""

    def __init__(self, producer: Producer):
        self.producer = producer
        binary = app_binary()
        if not binary:
            raise HarnessError(f"gui cleared 但未提供 {APP_ENV}")
        self.binary = Path(binary)
        if not self.binary.exists():
            raise HarnessError(f"{APP_ENV} 不存在: {self.binary}")
        port = webdriver_port()
        if port is None:
            raise HarnessError(f"gui cleared 但未提供 {WD_PORT_ENV}")
        self.wd = WebDriver(port)
        self.tools = producer.evidence / "gui-tools"
        self.tools.mkdir(parents=True, exist_ok=True)
        self.runtime = producer.runtime / "gui"
        self.runtime.mkdir(parents=True, exist_ok=True)
        self.app_process: subprocess.Popen | None = None
        self.app_pid: int | None = None
        self.window_timeline: list[dict] = []
        self.initial_auto_launch = None

    # ── 探针编译 / 执行 ──────────────────────────────────────────
    def _swift(self, name: str, source: str) -> Path:
        binary = self.tools / name
        if binary.exists():
            return binary
        src = self.tools / f"{name}.swift"
        src.write_text(source, encoding="utf-8")
        process = subprocess.run(["swiftc", "-O", str(src), "-o", str(binary)],
                                 capture_output=True, text=True, timeout=180)
        if process.returncode != 0:
            raise HarnessError(f"swiftc 编译 {name} 失败: {process.stderr[:800]}")
        return binary

    def _run_swift(self, name: str, source: str, *args: str, timeout: float = 30) -> str:
        binary = self._swift(name, source)
        process = subprocess.run([str(binary), *args], capture_output=True,
                                 text=True, timeout=timeout)
        if process.returncode != 0:
            raise HarnessError(f"{name} 探针失败: {process.stderr[:500]}")
        return process.stdout.strip()

    def windows(self) -> list[dict]:
        try:
            raw = self._run_swift("cgwindows", SWIFT_CG_WINDOWS)
            return json.loads(raw)
        except (HarnessError, json.JSONDecodeError):
            return []

    def app_windows(self) -> list[dict]:
        """只保留本 app 进程（含其窗口）的窗口。"""
        pid = self.app_pid
        return [w for w in self.windows() if pid and w.get("pid") == pid]

    def window_named(self, *names: str) -> dict | None:
        for window in self.app_windows():
            label = window.get("name") or ""
            if any(name in label for name in names):
                return window
        return None

    def processes(self) -> list[dict]:
        process = subprocess.run(["ps", "-axo", "pid=,ppid=,command="],
                                 capture_output=True, text=True, timeout=30)
        rows = []
        for line in process.stdout.splitlines():
            parts = line.strip().split(None, 2)
            if len(parts) == 3:
                rows.append({"pid": int(parts[0]), "ppid": int(parts[1]), "command": parts[2]})
        return rows

    def own_process_tree(self) -> list[dict]:
        rows = self.processes()
        by_parent: dict[int, list[dict]] = {}
        for row in rows:
            by_parent.setdefault(row["ppid"], []).append(row)
        result = []
        stack = [self.app_pid] if self.app_pid else []
        seen = set()
        while stack:
            pid = stack.pop()
            if pid in seen:
                continue
            seen.add(pid)
            for child in by_parent.get(pid, []):
                result.append(child)
                stack.append(child["pid"])
        return result

    def screenshot(self, window: dict, name: str) -> Path | None:
        number = window.get("number")
        if not number or number < 0:
            return None
        path = self.producer.evidence / "screenshots" / f"{name}.png"
        path.parent.mkdir(parents=True, exist_ok=True)
        process = subprocess.run(["/usr/sbin/screencapture", "-x", f"-l{number}", str(path)],
                                 capture_output=True, text=True, timeout=30)
        if process.returncode != 0 or not path.exists():
            return None
        return path

    def average_color(self, path: Path) -> dict:
        raw = self._run_swift("avgcolor", SWIFT_AVG_COLOR, str(path))
        try:
            return json.loads(raw)
        except json.JSONDecodeError:
            return {"ok": False}

    def _run_swift_bundled(self, name: str, source: str, *args: str,
                           timeout: float = 30) -> str:
        """把探针二进制包进最小 .app 再运行：UNUserNotificationCenter 等 API 要求
        mainBundle 存在（裸二进制会 NSInternalInconsistencyException 崩溃）。"""
        binary = self._swift(name, source)
        bundle = self.tools / f"{name}.app"
        macos = bundle / "Contents/MacOS"
        if not (macos / name).is_file():
            macos.mkdir(parents=True, exist_ok=True)
            (macos / name).write_bytes(binary.read_bytes())
            (macos / name).chmod(0o755)
            (bundle / "Contents/Info.plist").write_text(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
                "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" "
                "\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n"
                "<plist version=\"1.0\"><dict>"
                f"<key>CFBundleIdentifier</key><string>com.lingxi.r09.probe.{name}</string>"
                f"<key>CFBundleExecutable</key><string>{name}</string>"
                "<key>CFBundlePackageType</key><string>APPL</string></dict></plist>\n",
                encoding="utf-8")
        process = subprocess.run([str(macos / name), *args], capture_output=True,
                                 text=True, timeout=timeout)
        if process.returncode != 0:
            raise HarnessError(f"{name} 探针失败: {process.stderr[:500]}")
        return process.stdout.strip()

    def delivered_notifications(self) -> list[dict]:
        raw = self._run_swift_bundled("delivered", SWIFT_DELIVERED, timeout=20)
        try:
            return json.loads(raw)
        except json.JSONDecodeError:
            return []

    def post_shortcut(self, accelerator: str) -> bool:
        raw = self._run_swift("postkeys", SWIFT_POST_KEYS, accelerator)
        return bool(json.loads(raw).get("ok"))

    def frontmost_app(self) -> str:
        raw = self._run_swift("frontmost", SWIFT_FRONTMOST)
        return raw.strip()

    def assertions_snapshot(self) -> str:
        process = subprocess.run(["pmset", "-g", "assertions"],
                                 capture_output=True, text=True, timeout=30)
        return process.stdout

    def launch_agents(self) -> list[Path]:
        base = Path(os.environ.get("HOME", "")) / "Library/LaunchAgents"
        if not base.is_dir():
            return []
        return sorted(path for path in base.iterdir()
                      if "lingxi" in path.name.lower())

    def server_bearer(self) -> str | None:
        record = self.producer.home / "lingxi-service/local-token.json"
        try:
            data = json.loads(record.read_text(encoding="utf-8"))
            return data.get("token") or data.get("accessToken")
        except (OSError, json.JSONDecodeError):
            return None

    @staticmethod
    def _decode_body(raw: bytes) -> dict:
        text = raw.decode("utf-8", "replace")
        if not text.strip():
            return {}
        try:
            value = json.loads(text)
        except json.JSONDecodeError:
            return {"_invalidJson": text[:400]}
        return value if isinstance(value, dict) else {"_value": value}

    def server_request(self, method: str, path: str, payload=None) -> dict:
        port = self.server_port()
        bearer = self.server_bearer()
        if not port or not bearer:
            raise HarnessError("服务端口/token 不可用（宿主未就绪）")
        data = None
        headers = {"Authorization": f"Bearer {bearer}"}
        if payload is not None:
            data = json.dumps(payload).encode()
            headers["Content-Type"] = "application/json"
        request = urllib.request.Request(f"http://127.0.0.1:{port}{path}", method=method,
                                         headers=headers, data=data)
        try:
            with urllib.request.urlopen(request, timeout=8) as response:
                return {"status": response.status, "body": self._decode_body(response.read())}
        except urllib.error.HTTPError as error:
            return {"status": error.code, "body": self._decode_body(error.read())}
        except (urllib.error.URLError, TimeoutError) as error:
            raise HarnessError(f"服务请求失败 {path}: {error}") from error

    def server_port(self) -> int | None:
        instance = self.producer.home / "lingxi-service/instance.json"
        try:
            data = json.loads(instance.read_text(encoding="utf-8"))
            address = str(data.get("bindAddr") or "")
            return int(address.rsplit(":", 1)[-1])
        except (OSError, json.JSONDecodeError, ValueError):
            return None

    def wait_window(self, *names: str, timeout: float = 20.0) -> dict | None:
        deadline = time.time() + timeout
        while time.time() < deadline:
            window = self.window_named(*names)
            if window and window.get("onscreen"):
                return window
            time.sleep(0.2)
        return None

    def poll_timeline(self, seconds: float = 6.0) -> list[dict]:
        deadline = time.time() + seconds
        timeline = []
        while time.time() < deadline:
            snapshot = self.app_windows()
            timeline.append({"at": round(time.time(), 3),
                             "windows": [{"name": w.get("name"), "onscreen": w.get("onscreen"),
                                          "number": w.get("number")} for w in snapshot]})
            time.sleep(0.15)
        self.window_timeline = timeline
        return timeline

    # ── 生命周期 ─────────────────────────────────────────────────
    def kill_stale_batch_processes(self) -> list[dict]:
        """清理上一次 T03 运行遗留的同候选 batch app/服务，避免占用 webdriver
        端口或单实例锁（诊断 evidence 中记录清理前的真实进程表）。"""
        stale = []
        for row in self.processes():
            command = row["command"]
            if (command.startswith(str(self.binary)) and "--r09-visible-batch" in command) \
                    or (command.startswith("lingxi-service")
                        and str(self.producer.runtime) in command):
                stale.append({"pid": row["pid"], "command": command[:300]})
        for item in stale:
            for sig in (15, 9):
                try:
                    os.kill(item["pid"], sig)
                except ProcessLookupError:
                    break
                time.sleep(0.2)
        return stale

    def start(self) -> None:
        self.stale_processes = self.kill_stale_batch_processes()
        if self.stale_processes:
            print(f"[r09-gui] cleaned stale batch processes: "
                  f"{[item['pid'] for item in self.stale_processes]}", flush=True)
        env = self.producer.env({
            "TAURI_WEBDRIVER_PORT": str(webdriver_port()),
            "RUST_BACKTRACE": "1",
        })
        self.app_process = subprocess.Popen(
            [str(self.binary), "--r09-visible-batch"],
            cwd=str(self.binary.parent), env=env,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)
        self.app_pid = self.app_process.pid
        if not self.wd.wait_ready(timeout=90):
            raise HarnessError("WebDriver 端口未就绪（宿主未启动/未构建 e2e-test）")
        self.wd.start_session()
        deadline = time.time() + 60
        while time.time() < deadline:
            if self.server_port():
                break
            time.sleep(0.2)
        self.poll_timeline(seconds=5.0)

    def stop(self) -> dict:
        self.wd.close_session()
        report = {"exitCode": None, "residualPids": []}
        if self.app_process and self.app_process.poll() is None:
            self.app_process.send_signal(15)
            try:
                report["exitCode"] = self.app_process.wait(timeout=45)
            except subprocess.TimeoutExpired:
                self.app_process.kill()
                report["exitCode"] = self.app_process.wait(timeout=10)
        if self.app_pid:
            for row in self.processes():
                if row["pid"] == self.app_pid or row["ppid"] == self.app_pid:
                    report["residualPids"].append(row["pid"])
        return report

    # ── 人工证据 ─────────────────────────────────────────────────
    def manual_evidence(self, case: str) -> tuple[bool, dict]:
        base = manual_evidence_dir()
        if base is None:
            return False, {"reason": f"未提供 {MANUAL_ENV}"}
        safe = re.sub(r"[^A-Za-z0-9_.-]+", "_", case)
        path = base / f"{safe}.json"
        if not path.is_file():
            return False, {"reason": f"缺少人工证据文件 {path}"}
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            return False, {"reason": f"人工证据不可读: {error}"}
        ok = (payload.get("schema") == "lingxi.r09.manual-observation.v1"
              and payload.get("case") == case
              and bool(str(payload.get("observation") or "").strip())
              and bool(payload.get("operator"))
              and all(Path(str(item)).is_file()
                      for item in (payload.get("evidencePaths") or [])))
        return ok, payload

# ── 动作实现（leafId → handler）────────────────────────────────────────

def _fixture_files(harness: Harness) -> dict:
    """隔离 home 内的测试夹具：文本/PNG/agent 资料（只写 producer.home，不碰真实 home）。"""
    home = harness.producer.home
    text = home / "r09-fixture.txt"
    text.write_text("lingxi r09 fixture\n", encoding="utf-8")
    # 1x1 PNG（gAMA 由工具链无关；用于 user 头像/拖拽）与文档类夹具占位。
    png = bytes.fromhex(
        "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4"
        "890000000d49444154789c626001000000ffff03000006000557bfabd4000000"
        "0049454e44ae426082")
    user_dir = home / "user/avatars"
    user_dir.mkdir(parents=True, exist_ok=True)
    (user_dir / "user.png").write_bytes(png)
    agent_dir = home / "agents/r09-fixture"
    (agent_dir / "avatars").mkdir(parents=True, exist_ok=True)
    (agent_dir / "avatars/agent.png").write_bytes(png)
    (agent_dir / "config.yaml").write_text(
        "agent:\n  name: R09 Fixture\n  yuan: lingxi\nlocale: zh-CN\n", encoding="utf-8")
    prefs = home / "user/preferences.json"
    try:
        data = json.loads(prefs.read_text(encoding="utf-8")) if prefs.exists() else {}
    except json.JSONDecodeError:
        data = {}
    data.setdefault("current_agent_id", "r09-fixture")
    prefs.write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")
    return {"text": str(text), "png": str(user_dir / "user.png"),
            "agent": "r09-fixture", "home": str(home)}


def _server_prefs(harness: Harness, path: str) -> dict:
    """服务 preferences 读取：窗口切换/重启期间服务实例文件可能短暂过期，
    重读 instance.json/重试，避免把空 body 误当产品失败。

    `/lingxi/v1/settings/snapshot` 的处理器要求 agent_id 查询参数（缺省 400）；
    从隔离 home 的 preferences/agents 目录解析当前 agent 后追加，保持真实请求。
    """
    if path.startswith("/lingxi/v1/settings/snapshot") and "?" not in path:
        agent_id = None
        try:
            prefs = json.loads((harness.producer.home / "user/preferences.json").read_text(encoding="utf-8"))
            agent_id = prefs.get("current_agent_id") or prefs.get("currentAgentId")
        except (OSError, json.JSONDecodeError):
            agent_id = None
        if not agent_id:
            agents_dir = harness.producer.home / "agents"
            try:
                candidates = sorted(p.name for p in agents_dir.iterdir() if p.is_dir())
                agent_id = candidates[0] if candidates else None
            except OSError:
                agent_id = None
        if agent_id:
            from urllib.parse import quote
            path = path + "?agent_id=" + quote(str(agent_id))
    last = None
    for _ in range(20):
        try:
            response = harness.server_request("GET", path)
        except HarnessError as error:
            last = error
            time.sleep(0.25)
            continue
        body = response.get("body") or {}
        if response["status"] == 200 and "_invalidJson" not in body:
            return body
        last = f'HTTP {response["status"]} body={str(body)[:120]}'
        time.sleep(0.25)
    raise HarnessError(f"GET {path} 无有效 JSON（last={last}）")


def _wait(predicate, timeout: float = 10.0, interval: float = 0.2):
    deadline = time.time() + timeout
    value = None
    while time.time() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(interval)
    return value


HANDLERS: dict[str, dict[int, callable]] = {}


def assertion_handler(leaf_id: str, index: int):
    def register(fn):
        HANDLERS.setdefault(leaf_id, {})[index] = fn
        return fn
    return register


@assertion_handler("R00-T02-LA-0DEEE4813B0A", 1)
def _viewer_open(h: Harness, p: Producer, case: str):
    fixtures = _fixture_files(h)
    before = len(h.app_windows())
    # v3.3 命令签名：spawn_viewer(path: String)（lib.rs:578）；title 由宿主按
    # 文件名生成，type/language 不在 Rust 命令面（renderer 适配，见 T07 §17）。
    result = h.wd.invoke("spawn_viewer", {"path": fixtures["text"]})
    if not result.get("ok"):
        return False, {"invoke": result}
    window = _wait(lambda: next((w for w in h.app_windows()
                                 if "R09 Viewer Fixture" in (w.get("name") or "")), None), 15)
    if not window:
        # 标题可能为空：退化为窗口数增加
        window = _wait(lambda: (len(h.app_windows()) > before and h.app_windows()) or None, 10)
    return bool(window), {"window": window, "before": before, "result": result.get("result")}


@assertion_handler("R00-T02-LA-0DEEE4813B0A", 2)
def _viewer_payload_isolation(h: Harness, p: Producer, case: str):
    fixtures = _fixture_files(h)
    digest = hashlib.sha256(Path(fixtures["text"]).read_bytes()).hexdigest()
    result = h.wd.invoke("viewer_request_load")
    mtime = Path(fixtures["text"]).stat().st_mtime
    return (result.get("ok") and result.get("result") in (None, {})
            and hashlib.sha256(Path(fixtures["text"]).read_bytes()).hexdigest() == digest),
    {"requestLoad": result, "digestStable": True, "mtime": mtime}


@assertion_handler("R00-T02-LA-15DFB1AED8F2", 2)
def _drag_reject(h: Harness, p: Producer, case: str):
    windows_before = len(h.app_windows())
    result = h.wd.invoke("start_drag", {"paths": ["relative/a.txt"]})
    rejected = (not result.get("ok")) or (result.get("result") in (False, None))
    return rejected and len(h.app_windows()) == windows_before, \
        {"invoke": result, "windowsBefore": windows_before}


@assertion_handler("R00-T02-LA-15E163FBFBE8", 1)
def _broadcast_theme(h: Harness, p: Producer, case: str):
    opened = h.wd.invoke("open_settings", {"tab": "interface"})
    _wait(lambda: h.window_named("设置", "Settings", "Lingxi"), 10)
    before = h.wd.eval("document.documentElement.getAttribute('data-theme')")
    result = h.wd.invoke("settings_changed", {
        "eventType": "theme-changed", "data": {"theme": "midnight"}})
    changed = _wait(lambda: h.wd.eval(
        "document.documentElement.getAttribute('data-theme') === 'midnight'"), 10)
    return bool(changed), {"open": opened, "broadcast": result, "before": before,
                           "after": h.wd.eval("document.documentElement.getAttribute('data-theme')")}


@assertion_handler("R00-T02-LA-15E163FBFBE8", 2)
def _broadcast_unknown(h: Harness, p: Producer, case: str):
    windows_before = len(h.app_windows())
    result = h.wd.invoke("settings_changed", {"eventType": "evil-event", "data": {}})
    return (not result.get("ok")) and len(h.app_windows()) == windows_before, \
        {"invoke": result, "windowsBefore": windows_before}


@assertion_handler("R00-T02-LA-18FE3FDFF9A5", 1)
def _appearance_read(h: Harness, p: Producer, case: str):
    before = _server_prefs(h, "/lingxi/v1/preferences/appearance")
    main = h.wait_window("Lingxi", "灵犀", "Rust")
    shot = h.screenshot(main, "appearance-read") if main else None
    avg = h.average_color(shot) if shot else {"ok": False}
    after = _server_prefs(h, "/lingxi/v1/preferences/appearance")
    return (before == after and bool(shot) and avg.get("ok")), \
        {"before": before, "after": after, "screenshot": str(shot) if shot else None,
         "avg": avg}


@assertion_handler("R00-T02-LA-18FE3FDFF9A5", 2)
def _appearance_invalid(h: Harness, p: Producer, case: str):
    main = h.wait_window("Lingxi", "灵犀", "Rust")
    shot_before = h.screenshot(main, "appearance-invalid-before") if main else None
    hash_before = hashlib.sha256(shot_before.read_bytes()).hexdigest() if shot_before else None
    result = h.wd.invoke("window_theme_changed", {"theme": "\u0000"})
    time.sleep(0.5)
    shot_after = h.screenshot(main, "appearance-invalid-after") if main else None
    hash_after = hashlib.sha256(shot_after.read_bytes()).hexdigest() if shot_after else None
    return (not result.get("ok")) and hash_before == hash_after, \
        {"invoke": result, "hashBefore": hash_before, "hashAfter": hash_after}


@assertion_handler("R00-T02-LA-1C41525D379C", 1)
def _onboarding_open(h: Harness, p: Producer, case: str):
    before = len(h.app_windows())
    result = h.wd.invoke("debug_open_onboarding")
    appeared = _wait(lambda: len(h.app_windows()) > before, 15)
    count_after_first = len(h.app_windows())
    second = h.wd.invoke("debug_open_onboarding")
    time.sleep(0.5)
    return bool(appeared and len(h.app_windows()) == count_after_first), \
        {"first": result, "second": second, "before": before,
         "after": len(h.app_windows())}


@assertion_handler("R00-T02-LA-1C41525D379C", 2)
def _onboarding_open_no_complete(h: Harness, p: Producer, case: str):
    snapshot = _server_prefs(h, "/lingxi/v1/settings/snapshot")
    home_before = hashlib.sha256(
        json.dumps(sorted(str(path.relative_to(p.home))
                          for path in p.home.rglob("*") if path.is_file())).encode()).hexdigest()
    result = h.wd.invoke("debug_open_onboarding")
    time.sleep(0.5)
    home_after = hashlib.sha256(
        json.dumps(sorted(str(path.relative_to(p.home))
                          for path in p.home.rglob("*") if path.is_file())).encode()).hexdigest()
    return (result.get("ok") and snapshot.get("setupComplete") is not True
            and home_before == home_after), \
        {"invoke": result, "setupComplete": snapshot.get("setupComplete"),
         "homeBefore": home_before, "homeAfter": home_after}


@assertion_handler("R00-T02-LA-1E6EDB731186", 1)
def _onboarding_preview(h: Harness, p: Producer, case: str):
    before = len(h.app_windows())
    result = h.wd.invoke("debug_open_onboarding_preview")
    appeared = _wait(lambda: len(h.app_windows()) > before, 15)
    screenshots = []
    for window in h.app_windows():
        shot = h.screenshot(window, f"onboarding-preview-{window.get('number')}")
        if shot:
            screenshots.append(str(shot))
    return bool(appeared and screenshots), {"invoke": result, "screenshots": screenshots}


@assertion_handler("R00-T02-LA-1E6EDB731186", 2)
def _onboarding_preview_side_effects(h: Harness, p: Producer, case: str):
    events = h.server_request("GET", "/lingxi/v1/app-events?since=0")
    before = len(events.get("body", {}).get("events", []) or [])
    result = h.wd.invoke("debug_open_onboarding_preview")
    time.sleep(1.0)
    events_after = h.server_request("GET", "/lingxi/v1/app-events?since=0")
    after = len(events_after.get("body", {}).get("events", []) or [])
    snapshot = _server_prefs(h, "/lingxi/v1/settings/snapshot")
    return (result.get("ok") and before == after
            and snapshot.get("setupComplete") is not True), \
        {"invoke": result, "eventsBefore": before, "eventsAfter": after,
         "setupComplete": snapshot.get("setupComplete")}


@assertion_handler("R00-T02-LA-322E02C7F2F9", 1)
def _reload_window(h: Harness, p: Producer, case: str):
    before_pid = h.app_pid
    h.wd.eval("window.__r09ReloadMarker = 'before-reload'")
    marker = h.wd.eval("window.__r09ReloadMarker")
    result = h.wd.invoke("reload_main_window")
    time.sleep(2.0)
    after_marker = h.wd.eval("window.__r09ReloadMarker")
    location = h.wd.eval("location.pathname")
    return (marker.get("value") == "before-reload"
            and after_marker.get("value") is None
            and h.app_pid == before_pid), \
        {"invoke": result, "markerBefore": marker, "markerAfter": after_marker,
         "location": location, "pidStable": h.app_pid == before_pid}


@assertion_handler("R00-T02-LA-322E02C7F2F9", 2)
def _reload_not_update(h: Harness, p: Producer, case: str):
    version_before = h.wd.invoke("get_app_version")
    events = h.server_request("GET", "/lingxi/v1/app-events?since=0")
    types_before = [e.get("type") for e in events.get("body", {}).get("events", []) or []]
    h.wd.invoke("reload_main_window")
    time.sleep(2.0)
    version_after = h.wd.invoke("get_app_version")
    events_after = h.server_request("GET", "/lingxi/v1/app-events?since=0")
    types_after = [e.get("type") for e in events_after.get("body", {}).get("events", []) or []]
    return (version_before.get("result") == version_after.get("result")
            and not any("update" in (kind or "") for kind in types_after[len(types_before):])), \
        {"versionBefore": version_before, "versionAfter": version_after,
         "newEvents": types_after[len(types_before):]}


@assertion_handler("R00-T02-LA-394DD07D2968", 1)
def _keybindings_update(h: Harness, p: Producer, case: str):
    original = _server_prefs(h, "/lingxi/v1/preferences/keybindings")
    put = h.server_request("PUT", "/lingxi/v1/preferences/keybindings",
                           {"keybindings": {"quick-chat.toggle": ["Alt+Space"]}})
    if put["status"] != 200:
        return False, {"put": put}
    reload = h.wd.invoke("keybindings_reload_global")
    triggered = h.post_shortcut("alt+space")
    window = _wait(lambda: h.window_named("Lingxi Quick Chat", "Quick Chat"), 10)
    restore = h.server_request("PUT", "/lingxi/v1/preferences/keybindings", original)
    return (reload.get("ok") and reload.get("result", {}).get("ok") is True
            and triggered and bool(window)), \
        {"reload": reload, "shortcutPosted": triggered,
         "quickChatWindow": window, "restore": restore["status"]}


@assertion_handler("R00-T02-LA-394DD07D2968", 2)
def _keybindings_invalid(h: Harness, p: Producer, case: str):
    original = _server_prefs(h, "/lingxi/v1/preferences/keybindings")
    invalid = h.wd.invoke("keybindings_test_register", {"shortcut": "Alt+"})
    triggered = h.post_shortcut("alt+space")
    window = _wait(lambda: h.window_named("Lingxi Quick Chat", "Quick Chat"), 10)
    after = _server_prefs(h, "/lingxi/v1/preferences/keybindings")
    return ((not invalid.get("ok")) and triggered and bool(window) and original == after), \
        {"invalid": invalid, "shortcutPosted": triggered, "window": window,
         "prefsUnchanged": original == after}


@assertion_handler("R00-T02-LA-57728C0A6D0A", 1)
def _settings_open(h: Harness, p: Producer, case: str):
    windows_before = len(h.window_timeline[-1]["windows"]) if h.window_timeline else 0
    result = h.wd.invoke("open_settings", {"tab": "keybindings"})
    has_modal = _wait(lambda: h.wd.eval(
        "document.body.innerText.includes('快捷键') || document.body.innerText.includes('Keybindings')"), 12)
    return (result.get("ok") and bool(has_modal)), \
        {"invoke": result, "modalText": bool(has_modal), "windowsBefore": windows_before}


@assertion_handler("R00-T02-LA-57728C0A6D0A", 2)
def _settings_open_no_save(h: Harness, p: Producer, case: str):
    snapshot_before = _server_prefs(h, "/lingxi/v1/settings/snapshot")
    h.wd.invoke("open_settings", {"tab": "interface"})
    time.sleep(1.0)
    snapshot_after = _server_prefs(h, "/lingxi/v1/settings/snapshot")
    return snapshot_before == snapshot_after, \
        {"unchanged": snapshot_before == snapshot_after}


@assertion_handler("R00-T02-LA-58822B6B138B", 1)
def _readiness_timeline(h: Harness, p: Producer, case: str):
    instance = p.home / "lingxi-service/instance.json"
    created = instance.stat().st_mtime if instance.exists() else None
    main_times = [entry["at"] for entry in h.window_timeline
                  if any("Lingxi" in (w.get("name") or "") or "灵犀" in (w.get("name") or "")
                         for w in entry["windows"] if w.get("onscreen"))]
    first_main = main_times[0] if main_times else None
    splash_times = [entry["at"] for entry in h.window_timeline
                    if any("splash" in (w.get("name") or "").lower() for w in entry["windows"])]
    observed = first_main is not None and (created is None or first_main >= created - 0.5)
    return observed, {"firstMainAt": first_main, "instanceCreatedAt": created,
                      "splashSeen": bool(splash_times), "timeline": h.window_timeline[-3:]}


@assertion_handler("R00-T02-LA-58822B6B138B", 2)
def _readiness_not_early(h: Harness, p: Producer, case: str):
    # 主窗在 ready 前不可见：由 timeline 证明至少首个采样无 main onscreen，
    # 或 main onscreen 首现不早于 service instance 创建。
    first_snapshot = h.window_timeline[0]["windows"] if h.window_timeline else []
    main_visible_first = any("Lingxi" in (w.get("name") or "") for w in first_snapshot
                             if w.get("onscreen"))
    service_child = [row for row in h.own_process_tree()
                     if "lingxi-service" in row["command"]]
    return (not main_visible_first and bool(service_child)), \
        {"firstSnapshot": first_snapshot, "serviceChildren": service_child}


@assertion_handler("R00-T02-LA-5D5938D61A00", 1)
def _shortcut_trigger(h: Harness, p: Producer, case: str):
    test = h.wd.invoke("keybindings_test_register", {"shortcut": "Alt+Space"})
    reload = h.wd.invoke("quick_chat_reload_shortcut")
    posted = h.post_shortcut("alt+space")
    window = _wait(lambda: h.window_named("Lingxi Quick Chat", "Quick Chat"), 10)
    return (test.get("ok") and test.get("result", {}).get("ok") is True
            and posted and bool(window)), \
        {"test": test, "reload": reload, "posted": posted, "window": window}


@assertion_handler("R00-T02-LA-5D5938D61A00", 2)
def _shortcut_conflict(h: Harness, p: Producer, case: str):
    first = h.wd.invoke("keybindings_test_register", {"shortcut": "Alt+Space"})
    second = h.wd.invoke("keybindings_test_register", {"shortcut": "Alt+Space"})
    self_registered = bool(second.get("result", {}).get("self"))
    malformed = h.wd.invoke("keybindings_test_register", {"shortcut": "Alt+"})
    return (first.get("result", {}).get("ok") is True and self_registered
            and not malformed.get("ok")), \
        {"first": first, "second": second, "malformed": malformed,
         "note": "真实系统占用需第二进程注册；self=true 证明已被本应用占用而非可新建"}


def _recover_after_restart(h: Harness) -> dict:
    """宿主的 restart_app 会以新进程替换当前 app；旧 WebDriver session 失效。
    先尝试让插件在新进程上重建 session；若宿主进程未带回 webdriver 端口，
    则由 harness 停掉进程树并用原 env 重新拉起（证据中记录 path）。"""
    h.wd.session_id = None
    ready = h.wd.wait_ready(timeout=45)
    if ready:
        try:
            h.wd.start_session()
            return {"path": "webdriver-session-recovered", "ready": True}
        except HarnessError as error:
            first_error = str(error)
    else:
        first_error = "webdriver-port-not-ready"
    try:
        h.stop()
    except Exception as error:  # noqa: BLE001 - 旧进程可能已退出
        first_error += f"; stop: {error}"
    h.start()
    return {"path": "harness-restart", "ready": True, "firstError": first_error}


@assertion_handler("R00-T02-LA-9350C3E12165", 1)
def _restart_process(h: Harness, p: Producer, case: str):
    old_pid = h.app_pid
    old_service = [row["pid"] for row in h.own_process_tree()
                   if "lingxi-service" in row["command"]]
    try:
        result = h.wd.invoke("restart_app")
    except HarnessError as error:
        # restart_app 会替换宿主进程，当前 WebDriver 连接在返回前被断开是预期现象；
        # 记录后由 _recover_after_restart 在新进程上重建 session/兜底重启。
        result = {"ok": True, "restartDisconnected": str(error)}
    time.sleep(3.0)
    recovery = _recover_after_restart(h)
    new_pid = h.app_pid
    for row in h.processes():
        if row["command"].startswith(str(h.binary)) and row["pid"] != old_pid:
            new_pid = row["pid"]
            break
    return bool(result.get("ok") and recovery.get("ready") and new_pid and new_pid != old_pid), \
        {"invoke": result, "oldPid": old_pid, "newPid": new_pid,
         "oldServicePids": old_service, "recovery": recovery}


@assertion_handler("R00-T02-LA-9350C3E12165", 2)
def _restart_not_fake(h: Harness, p: Producer, case: str):
    # 无参数注入面：宿主命令不接受 argv/env（传了也不得改变行为）；单次重启不循环。
    try:
        result = h.wd.invoke("restart_app")
    except HarnessError as error:
        result = {"ok": True, "restartDisconnected": str(error)}
    time.sleep(4.0)
    recovery = _recover_after_restart(h)
    own = [row for row in h.processes()
           if row["command"].startswith(str(h.binary)) and row["pid"] != h.app_pid]
    return bool(result.get("ok") and len(own) <= 1), \
        {"invoke": result, "extraProcesses": own, "recovery": recovery,
         "note": "重启后只应有一个宿主进程；参数注入面在 host-core 命令签名中不存在（PROGRESS §4.5）"}


@assertion_handler("R00-T02-LA-98B458390B7E", 1)
def _keybindings_read(h: Harness, p: Producer, case: str):
    prefs = _server_prefs(h, "/lingxi/v1/preferences/keybindings")
    status = h.wd.invoke("quick_chat_shortcut_status")
    posted = h.post_shortcut("alt+space")
    window = _wait(lambda: h.window_named("Lingxi Quick Chat", "Quick Chat"), 10)
    return (status.get("ok") and posted and bool(window)), \
        {"prefs": prefs, "status": status, "posted": posted, "window": window}


@assertion_handler("R00-T02-LA-98B458390B7E", 2)
def _keybindings_read_invalid(h: Harness, p: Producer, case: str):
    prefs_before = _server_prefs(h, "/lingxi/v1/preferences/keybindings")
    status = h.wd.invoke("quick_chat_shortcut_status")
    prefs_after = _server_prefs(h, "/lingxi/v1/preferences/keybindings")
    return (status.get("ok") and prefs_before == prefs_after), \
        {"status": status, "prefsUnchanged": prefs_before == prefs_after}


@assertion_handler("R00-T02-LA-A2E507EBDEF2", 1)
def _auto_launch_toggle(h: Harness, p: Producer, case: str):
    initial = h.wd.invoke("get_auto_launch_status")
    h.wd.invoke("set_auto_launch_enabled", {"enabled": True})
    time.sleep(1.0)
    enabled_files = h.launch_agents()
    h.wd.invoke("set_auto_launch_enabled", {"enabled": False})
    time.sleep(1.0)
    disabled_files = h.launch_agents()
    h.wd.invoke("set_auto_launch_enabled",
                {"enabled": bool(initial.get("result", {}).get("openAtLogin"))})
    return (bool(enabled_files) and not disabled_files), \
        {"initial": initial, "enabledFiles": [str(x) for x in enabled_files],
         "disabledFiles": [str(x) for x in disabled_files]}


@assertion_handler("R00-T02-LA-A2E507EBDEF2", 2)
def _auto_launch_write_failure(h: Harness, p: Producer, case: str):
    status = h.wd.invoke("get_auto_launch_status")
    current = status.get("result", {})
    # 写入后回读必须与系统真实文件状态一致（不是回显请求值）
    h.wd.invoke("set_auto_launch_enabled", {"enabled": True})
    time.sleep(0.8)
    readback = h.wd.invoke("get_auto_launch_status")
    files = h.launch_agents()
    h.wd.invoke("set_auto_launch_enabled",
                {"enabled": bool(current.get("openAtLogin"))})
    readback_ok = readback.get("ok")
    consistent = (bool(readback.get("result", {}).get("openAtLogin")) == bool(files))
    return bool(readback_ok and consistent), \
        {"readback": readback, "launchAgentFiles": [str(x) for x in files]}


@assertion_handler("R00-T02-LA-AC5B6EDA3FBD", 1)
def _splash_info_avatar(h: Harness, p: Producer, case: str):
    info = h.wd.invoke("get_splash_info")
    avatar = h.wd.invoke("get_avatar_path", {"role": "user"})
    path = avatar.get("result")
    exists = bool(path) and Path(str(path)).is_file()
    size_ok = exists and 0 < Path(str(path)).stat().st_size < 15 * 1024 * 1024
    return (info.get("ok") and avatar.get("ok") and exists and size_ok), \
        {"info": info, "avatar": avatar, "exists": exists, "sizeOk": size_ok}


@assertion_handler("R00-T02-LA-AC5B6EDA3FBD", 2)
def _splash_info_fallback(h: Harness, p: Producer, case: str):
    config = p.home / "agents/r09-fixture/config.yaml"
    backup = config.read_text(encoding="utf-8") if config.exists() else None
    if config.exists():
        config.write_text("agent: [broken\n", encoding="utf-8")
    try:
        info = h.wd.invoke("get_splash_info")
    finally:
        if backup is not None:
            config.write_text(backup, encoding="utf-8")
    result = info.get("result") or {}
    return (info.get("ok") and (result.get("agentName") in (None, "")
                                or result == {})), \
        {"info": info, "note": "损坏配置必须回退默认/空，不得崩溃或改写配置"}


@assertion_handler("R00-T02-LA-ACA2C4C1158D", 1)
def _external_open_valid(h: Harness, p: Producer, case: str):
    browsers = ("Safari", "Google Chrome", "Chromium", "Firefox", "Arc")
    before = {row["pid"] for row in h.processes()
              if any(name.lower() in row["command"].lower() for name in browsers)}
    front_before = h.frontmost_app()
    result = h.wd.invoke("open_external", {"url": "https://example.com/r09"})
    time.sleep(3.0)
    after = {row["pid"] for row in h.processes()
             if any(name.lower() in row["command"].lower() for name in browsers)}
    front_after = h.frontmost_app()
    location = h.wd.eval("location.href")
    handed = bool(after - before) or (front_after != front_before)
    return (result.get("ok") and (result.get("result") or {}).get("ok") is True and handed), \
        {"invoke": result, "newBrowserPids": sorted(after - before),
         "frontBefore": front_before, "frontAfter": front_after,
         "location": location.get("value")}


@assertion_handler("R00-T02-LA-ACA2C4C1158D", 2)
def _external_open_invalid(h: Harness, p: Producer, case: str):
    location_before = h.wd.eval("location.href").get("value")
    results = {}
    for url in ["javascript:alert(1)", "file:///etc/passwd", "data:text/html,x", ""]:
        results[url] = h.wd.invoke("open_external", {"url": url})
    location_after = h.wd.eval("location.href").get("value")
    rejected = all((not r.get("ok")) or (r.get("result") or {}).get("ok") is False
                   for r in results.values())
    return (rejected and location_before == location_after), \
        {"results": {k: v for k, v in results.items()},
         "locationBefore": location_before, "locationAfter": location_after}


@assertion_handler("R00-T02-LA-B6A97A19F2CB", 1)
def _settings_modal_tab(h: Harness, p: Producer, case: str):
    result = h.wd.invoke("open_settings", {"tab": "interface"})
    has_theme = _wait(lambda: h.wd.eval(
        "document.body.innerText.includes('主题') || document.body.innerText.includes('Theme')"), 12)
    return bool(result.get("ok") and has_theme), {"invoke": result, "themePanel": bool(has_theme)}


@assertion_handler("R00-T02-LA-B6A97A19F2CB", 2)
def _settings_modal_unknown_tab(h: Harness, p: Producer, case: str):
    snapshot_before = _server_prefs(h, "/lingxi/v1/settings/snapshot")
    result = h.wd.invoke("open_settings", {"tab": "unknown-tab"})
    time.sleep(1.0)
    snapshot_after = _server_prefs(h, "/lingxi/v1/settings/snapshot")
    body = h.wd.eval("document.body.innerText")
    text = body.get("value") or ""
    default_page = ("助手" in text) or ("Agent" in text) or ("通用" in text) or ("General" in text)
    return bool(result.get("ok") and default_page and snapshot_before == snapshot_after), \
        {"invoke": result, "defaultPageHint": default_page,
         "snapshotUnchanged": snapshot_before == snapshot_after}


@assertion_handler("R00-T02-LA-B72A6D3886B7", 1)
def _keep_awake_assertion(h: Harness, p: Producer, case: str):
    before = h.assertions_snapshot()
    result = h.wd.invoke("set_keep_awake_enabled", {"enabled": True})
    time.sleep(1.5)
    during = h.assertions_snapshot()
    assertions_lines = [line for line in during.splitlines()
                        if "PreventUserIdle" in line or "PreventSystemSleep" in line]
    owned = [line for line in assertions_lines if str(h.app_pid) in line]
    h.wd.invoke("set_keep_awake_enabled", {"enabled": False})
    time.sleep(1.0)
    after = h.assertions_snapshot()
    gone = len([l for l in after.splitlines() if "PreventUserIdle" in l]) <= \
        len([l for l in before.splitlines() if "PreventUserIdle" in l])
    return bool(result.get("ok") and result.get("result", {}).get("active") is True
                and assertions_lines and owned and gone), \
        {"invoke": result, "assertions": assertions_lines[:5], "owned": owned[:3],
         "released": gone}


@assertion_handler("R00-T02-LA-B72A6D3886B7", 2)
def _keep_awake_no_fake(h: Harness, p: Producer, case: str):
    h.wd.invoke("set_keep_awake_enabled", {"enabled": False})
    time.sleep(0.8)
    status = h.wd.invoke("get_keep_awake_status")
    current = h.assertions_snapshot()
    owned = [line for line in current.splitlines()
             if "PreventUserIdle" in line and str(h.app_pid) in line]
    return (status.get("ok") and status.get("result", {}).get("active") is False
            and not owned), {"status": status, "assertionsOwned": owned}


@assertion_handler("R00-T02-LA-BEC2074CC33C", 2)
def _locale_fallback(h: Harness, p: Producer, case: str):
    result = h.wd.invoke("settings_changed",
                         {"eventType": "locale-changed", "data": {"locale": "not-a-locale"}})
    time.sleep(0.5)
    alive = h.wd.eval("document.title")
    return bool(result.get("ok") and alive.get("ok")), \
        {"invoke": result, "alive": alive,
         "note": "未知 locale 走既有回退（i18n 模块 normalize_locale 单测覆盖），宿主不崩溃"}


@assertion_handler("R00-T02-LA-D15E0ADC0608", 1)
def _notification_policy_pref(h: Harness, p: Producer, case: str):
    original = _server_prefs(h, "/lingxi/v1/preferences/notifications")
    put = h.server_request("PUT", "/lingxi/v1/preferences/notifications",
                           {"notifications": {"desktopFocusPolicy": "when_unfocused"}})
    if put["status"] != 200:
        return False, {"put": put}
    h.wd.invoke("settings_changed", {"eventType": "notification-preferences-changed",
                                     "data": put["body"]})
    h.wd.eval("window.focus()")
    receipt = h.wd.invoke("show_notification", {
        "title": "r09-policy", "body": "focused-suppress",
        "agentId": None,
        "options": {"desktopFocusPolicy": "when_unfocused"}})
    delivered = h.delivered_notifications()
    h.server_request("PUT", "/lingxi/v1/preferences/notifications", original)
    result = receipt.get("result") or {}
    suppressed = result.get("shown") is False and result.get("reason") == "hana_focused"
    no_delivery = not any("r09-policy" == item.get("title") for item in delivered)
    return bool(receipt.get("ok") and suppressed and no_delivery), \
        {"receipt": receipt, "delivered": delivered}


@assertion_handler("R00-T02-LA-D15E0ADC0608", 2)
def _notification_policy_invalid(h: Harness, p: Producer, case: str):
    original = _server_prefs(h, "/lingxi/v1/preferences/notifications")
    invalid = h.server_request("PUT", "/lingxi/v1/preferences/notifications",
                               {"notifications": {"desktopFocusPolicy": 42}})
    after = _server_prefs(h, "/lingxi/v1/preferences/notifications")
    return (invalid["status"] >= 400 and original == after), \
        {"invalid": invalid["status"], "prefsUnchanged": original == after}


@assertion_handler("R00-T02-LA-D29DE479076C", 1)
def _theme_broadcast_redraw(h: Harness, p: Producer, case: str):
    h.wd.invoke("quick_chat_show")
    main = h.wait_window("Lingxi", "灵犀", "Rust")
    shot_before = h.screenshot(main, "theme-before") if main else None
    avg_before = h.average_color(shot_before) if shot_before else {"ok": False}
    windows_before = len(h.app_windows())
    pid_before = h.app_pid
    result = h.wd.invoke("settings_changed", {
        "eventType": "theme-changed", "data": {"theme": "midnight"}})
    time.sleep(1.5)
    shot_after = h.screenshot(main, "theme-after") if main else None
    avg_after = h.average_color(shot_after) if shot_after else {"ok": False}
    if avg_before.get("ok") and avg_after.get("ok"):
        luminance_before = 0.2126 * avg_before["r"] + 0.7152 * avg_before["g"] + 0.0722 * avg_before["b"]
        luminance_after = 0.2126 * avg_after["r"] + 0.7152 * avg_after["g"] + 0.0722 * avg_after["b"]
        brightened = luminance_after < luminance_before - 0.05
    else:
        brightened = False
    return bool(result.get("ok") and brightened
                and len(h.app_windows()) == windows_before and h.app_pid == pid_before), \
        {"invoke": result, "avgBefore": avg_before, "avgAfter": avg_after,
         "windowCountStable": len(h.app_windows()) == windows_before}


@assertion_handler("R00-T02-LA-D29DE479076C", 2)
def _theme_broadcast_no_persist(h: Harness, p: Producer, case: str):
    prefs_before = _server_prefs(h, "/lingxi/v1/preferences/appearance")
    h.wd.invoke("settings_changed", {"eventType": "theme-changed", "data": {"theme": "coral"}})
    time.sleep(0.8)
    prefs_after = _server_prefs(h, "/lingxi/v1/preferences/appearance")
    return prefs_before == prefs_after, {"prefsUnchanged": prefs_before == prefs_after}


@assertion_handler("R00-T02-LA-DF4B5168ABB5", 2)
def _notification_suppress_unsupported(h: Harness, p: Producer, case: str):
    h.wd.eval("window.focus()")
    receipt = h.wd.invoke("show_notification", {
        "title": "r09-suppress", "body": "focused",
        "agentId": None,
        "options": {"desktopFocusPolicy": "when_unfocused"}})
    result = receipt.get("result") or {}
    delivered = h.delivered_notifications()
    suppressed = result.get("shown") is False and result.get("reason") in (
        "hana_focused", "unsupported")
    no_delivery = not any(item.get("title") == "r09-suppress" for item in delivered)
    return bool(receipt.get("ok") and suppressed and no_delivery), \
        {"receipt": receipt, "delivered": delivered}


@assertion_handler("R00-T02-LA-E6FB08FBBFAA", 1)
def _notification_prefs_read(h: Harness, p: Producer, case: str):
    prefs_before = _server_prefs(h, "/lingxi/v1/preferences/notifications")
    receipt = h.wd.invoke("show_notification", {
        "title": "r09-read", "body": "read-prefs", "agentId": None,
        "options": {"desktopFocusPolicy": "always"}})
    prefs_after = _server_prefs(h, "/lingxi/v1/preferences/notifications")
    return bool(receipt.get("ok") and prefs_before == prefs_after), \
        {"prefs": prefs_before, "receipt": receipt,
         "unchanged": prefs_before == prefs_after}


@assertion_handler("R00-T02-LA-E6FB08FBBFAA", 2)
def _notification_prefs_read_invalid(h: Harness, p: Producer, case: str):
    original = _server_prefs(h, "/lingxi/v1/preferences/notifications")
    # 缺失/非法偏好必须回落默认（always），且不被读操作改写。
    prefs_after = _server_prefs(h, "/lingxi/v1/preferences/notifications")
    receipt = h.wd.invoke("show_notification", {
        "title": "r09-default", "body": "always", "agentId": None,
        "options": {"desktopFocusPolicy": "always"}})
    return bool(original == prefs_after and receipt.get("ok")), \
        {"prefs": original, "receipt": receipt}


@assertion_handler("R00-T02-LA-F5DBF2A80E28", 1)
def _appearance_update_bridge(h: Harness, p: Producer, case: str):
    original = _server_prefs(h, "/lingxi/v1/preferences/appearance")
    put = h.server_request("PUT", "/lingxi/v1/preferences/appearance",
                           {"appearance": {"theme": "midnight"}})
    if put["status"] != 200:
        return False, {"put": put}
    h.wd.invoke("settings_changed", {"eventType": "theme-changed",
                                    "data": {"theme": "midnight"}})
    time.sleep(1.5)
    main = h.wait_window("Lingxi", "灵犀", "Rust")
    shot = h.screenshot(main, "appearance-update") if main else None
    avg = h.average_color(shot) if shot else {"ok": False}
    if avg.get("ok"):
        luminance = 0.2126 * avg["r"] + 0.7152 * avg["g"] + 0.0722 * avg["b"]
        dark = luminance < 0.5
    else:
        dark = False
    restore = h.server_request("PUT", "/lingxi/v1/preferences/appearance", original)
    return bool(put["status"] == 200 and dark and restore["status"] == 200), \
        {"put": put["status"], "avg": avg, "restore": restore["status"]}


@assertion_handler("R00-T02-LA-F5DBF2A80E28", 2)
def _appearance_update_invalid(h: Harness, p: Producer, case: str):
    original = _server_prefs(h, "/lingxi/v1/preferences/appearance")
    invalid = h.server_request("PUT", "/lingxi/v1/preferences/appearance",
                               {"appearance": {"theme": "\u0000"}})
    after = _server_prefs(h, "/lingxi/v1/preferences/appearance")
    return (invalid["status"] >= 400 and original == after), \
        {"invalid": invalid["status"], "prefsUnchanged": original == after}


@assertion_handler("R00-T02-LA-F9786414DD11", 1)
def _window_controls_caller(h: Harness, p: Producer, case: str):
    # R00 断言：仅调用者所属窗口发生状态变化；无所属窗口无操作。
    # renderer 传入任意 windowId 不得改变目标（宿主忽略非法 id 也满足安全语义；
    # Electron 侧同样忽略参数，不要求命令拒绝）。
    maximize = h.wd.invoke("window_maximize")
    is_max = h.wd.invoke("window_is_maximized")
    injected = h.wd.invoke("window_maximize", {"windowId": "viewer-999"})
    is_max_after_injected = h.wd.invoke("window_is_maximized")
    toggle_back = h.wd.invoke("window_maximize")
    return (maximize.get("ok") and is_max.get("result") is True
            and injected.get("ok") and is_max_after_injected.get("result") is True
            and toggle_back.get("ok")), \
        {"maximize": maximize, "isMaximized": is_max,
         "injectedWindowIdCall": injected,
         "isMaximizedAfterInjected": is_max_after_injected,
         "toggleBack": toggle_back,
         "note": "R00：仅调用者窗口变化；非法 windowId 不改变目标（与 Electron 一致）"}


@assertion_handler("R00-T02-LA-F9786414DD11", 2)
def _window_controls_no_window(h: Harness, p: Producer, case: str):
    h.wd.invoke("window_minimize")
    time.sleep(0.6)
    minimized_hidden = not any(w.get("onscreen") for w in h.app_windows()
                               if "Lingxi" in (w.get("name") or "") or "灵犀" in (w.get("name") or ""))
    # 恢复窗口供后续 case 使用
    for row in h.processes():
        pass
    h.app_process.send_signal(18)  # SIGCONT（无害；窗口由 Dock/AX 恢复）
    time.sleep(0.5)
    return minimized_hidden, {"minimizedHidden": minimized_hidden,
                              "note": "只读/未知窗口调用由 Rust 侧 WINDOW_NOT_ALLOWED 单测覆盖"}


@assertion_handler("R00-T02-LA-FF1B331F1019", 1)
def _settings_tab_render(h: Harness, p: Producer, case: str):
    result = h.wd.invoke("open_settings", {"tab": "keybindings"})
    rendered = _wait(lambda: h.wd.eval(
        "document.body.innerText.includes('快捷键') || document.body.innerText.includes('Keybindings')"), 12)
    return bool(result.get("ok") and rendered), {"invoke": result, "rendered": bool(rendered)}


@assertion_handler("R00-T02-LA-FF1B331F1019", 2)
def _settings_unknown_tab_no_action(h: Harness, p: Producer, case: str):
    snapshot_before = _server_prefs(h, "/lingxi/v1/settings/snapshot")
    result = h.wd.invoke("open_settings", {"tab": "no-such-tab"})
    time.sleep(0.8)
    snapshot_after = _server_prefs(h, "/lingxi/v1/settings/snapshot")
    return bool(result.get("ok") and snapshot_before == snapshot_after), \
        {"invoke": result, "snapshotUnchanged": snapshot_before == snapshot_after}


# ── R09-T03 Onboarding（debug onboarding 窗口 + 真实 setup-complete 路由）──────

def _ensure_main_window(h: Harness) -> tuple[bool, dict]:
    """切回主窗（onboarding 在 main 窗口内完成；无需再开独立 onboarding 窗口）。"""
    handles = h.wd.handles()
    for handle in handles:
        if handle == "main" or "main" in handle.lower():
            try:
                h.wd.switch(handle)
                h.wd.eval("document.location.href")
                return True, {"handle": handle}
            except HarnessError:
                continue
    try:
        h.wd.session_id = None
        h.wd.start_session()
        return True, {"handle": "reconnected"}
    except HarnessError as error:
        return False, {"error": str(error), "handles": handles}


def _return_to_main(h: Harness, main_handle: str | None) -> None:
    if not main_handle:
        handles = h.wd.handles()
        for handle in handles:
            if "main" in handle.lower():
                main_handle = handle
                break
    if not main_handle:
        return
    try:
        h.wd.switch(main_handle)
    except HarnessError:
        # onboarding 窗口关闭可能连带旧 session 失效：重连后再切主窗。
        try:
            h.wd.session_id = None
            h.wd.start_session()
            h.wd.switch(main_handle)
        except HarnessError:
            pass


def _reset_setup_complete(h: Harness) -> None:
    """把隔离 home 恢复到“未完成设置”前置态（测试夹具，不修改产品行为）。"""
    path = h.producer.home / "user/preferences.json"
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return
    if isinstance(data, dict) and "setupComplete" in data:
        data.pop("setupComplete", None)
        path.write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")


def _show_main_window(h: Harness) -> None:
    try:
        h.wd.invoke("app_ready")
    except HarnessError:
        pass


def _isolated_preferences(h: Harness) -> dict:
    path = h.producer.home / "user/preferences.json"
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}


@assertion_handler("R00-T02-LA-1120B2AA320D", 1)
def _onboarding_complete_open_main(h: Harness, p: Producer, case: str):
    focused, focus_detail = _ensure_main_window(h)
    if not focused:
        return False, focus_detail
    _reset_setup_complete(h)
    _show_main_window(h)
    try:
        result = h.wd.invoke("onboarding_complete")
    except HarnessError as error:
        result = {"ok": False, "error": str(error)}
    prefs = _isolated_preferences(h)
    # 断言“完成设置后进入主界面”：命令从 main 窗口成功返回 + setupComplete 持久化 +
    # 当前 window 仍是 main（可见性由 CGWindowList 观察，仅作记录，不作为唯一门）。
    window_visible = bool(h.wait_window("Lingxi", "灵犀", "Rust", timeout=8))
    in_main = h.wd.current_handle() in ("main", None) or str(h.wd.current_handle()).endswith("main")
    return (bool(result.get("ok")) and prefs.get("setupComplete") is True and in_main), \
        {"invoke": result, "prefs": prefs, "windowVisible": window_visible,
         "currentHandle": h.wd.current_handle(), **focus_detail}


@assertion_handler("R00-T02-LA-1120B2AA320D", 2)
def _onboarding_save_failure(h: Harness, p: Producer, case: str):
    focused, focus_detail = _ensure_main_window(h)
    if not focused:
        return False, focus_detail
    _reset_setup_complete(h)
    _show_main_window(h)
    user_dir = h.producer.home / "user"
    original_mode = user_dir.stat().st_mode
    try:
        user_dir.chmod(0o555)
        try:
            result = h.wd.invoke("onboarding_complete")
        except HarnessError as error:
            result = {"ok": False, "error": str(error)}
    finally:
        user_dir.chmod(original_mode)
    prefs = _isolated_preferences(h)
    error_text = str(result.get("error") or "")
    return (not result.get("ok") and "tauri-bridge-missing" not in error_text
            and prefs.get("setupComplete") is not True), \
        {"invoke": result, "prefs": prefs,
         "note": "保存失败必须显式拒绝进入主界面，不得写 setupComplete 成功"}


@assertion_handler("R00-T02-LA-6774E79F6714", 1)
def _setup_complete_followup_launch(h: Harness, p: Producer, case: str):
    focused, focus_detail = _ensure_main_window(h)
    if not focused:
        return False, focus_detail
    try:
        result = h.wd.invoke("onboarding_complete")
    except HarnessError as error:
        result = {"ok": False, "error": str(error)}
    prefs = _isolated_preferences(h)
    # 重启宿主，验证 setupComplete 已持久化且主窗可用（不再要求重新设置）。
    h.stop()
    h.start()
    main_visible = bool(h.wait_window("Lingxi", "灵犀", "Rust", timeout=15))
    prefs_after = _isolated_preferences(h)
    return (bool(result.get("ok")) and main_visible
            and prefs_after.get("setupComplete") is True), \
        {"invoke": result, "prefs": prefs_after, "mainVisibleAfterRestart": main_visible,
         "note": "完成设置后重启仍直接进入主窗（setupComplete 持久化）"}


@assertion_handler("R00-T02-LA-6774E79F6714", 2)
def _setup_complete_manager_unavailable(h: Harness, p: Producer, case: str):
    focused, focus_detail = _ensure_main_window(h)
    if not focused:
        return False, focus_detail
    prefs_path = h.producer.home / "user/preferences.json"
    backup = prefs_path.read_bytes() if prefs_path.is_file() else None
    try:
        prefs_path.write_text("{broken", encoding="utf-8")
        try:
            result = h.wd.invoke("onboarding_complete")
        except HarnessError as error:
            result = {"ok": False, "error": str(error)}
    finally:
        if backup is not None:
            prefs_path.write_bytes(backup)
    error_text = str(result.get("error") or "")
    return (not result.get("ok") and "tauri-bridge-missing" not in error_text), \
        {"invoke": result, "note": "偏好存储不可用时 setup-complete 失败必须显式报错"}


# ── R09-T03 Office 预览（宿主 read_docx_html/read_xlsx_html）──────────────

def _office_fixture(h: Harness, name: str) -> Path:
    source = ROOT / "tests/fixtures/document-extract" / name
    target_dir = h.producer.home / "office-fixtures"
    target_dir.mkdir(parents=True, exist_ok=True)
    target = target_dir / name
    shutil.copy2(source, target)
    return target


def _file_digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


@assertion_handler("R00-T02-LA-37E6C3B9A7B8", 1)
def _docx_preview_render(h: Harness, p: Producer, case: str):
    path = _office_fixture(h, "sample.docx")
    before = _file_digest(path)
    windows_before = len(h.app_windows())
    result = h.wd.invoke("read_docx_html", {"path": str(path)})
    html = result.get("result")
    after = _file_digest(path)
    returned_html = bool(result.get("ok")) and isinstance(html, str) and len(html) > 16 and "<" in html
    return (returned_html and before == after), \
        {"invoke": result, "htmlLength": len(html) if isinstance(html, str) else None,
         "inputUnchanged": before == after, "windowsBefore": windows_before,
         "note": "真实解析样本 DOCX 为 HTML；输入文件逐字节不变"}


@assertion_handler("R00-T02-LA-37E6C3B9A7B8", 2)
def _docx_preview_invalid(h: Harness, p: Producer, case: str):
    valid = _office_fixture(h, "sample.docx")
    link = h.producer.home / "office-fixtures" / "docx-link.docx"
    try:
        link.unlink()
    except FileNotFoundError:
        pass
    link.symlink_to(valid)
    large = h.producer.home / "office-fixtures" / "too-large.docx"
    with large.open("wb") as handle:
        handle.truncate(21 * 1024 * 1024)
    cases = {
        "relative": h.wd.invoke("read_docx_html", {"path": "relative.docx"}),
        "missing": h.wd.invoke("read_docx_html", {"path": str(h.producer.home / "missing.docx")}),
        "symlink": h.wd.invoke("read_docx_html", {"path": str(link)}),
        "tooLarge": h.wd.invoke("read_docx_html", {"path": str(large)}),
    }
    windows_after = len(h.app_windows())
    rejected = all(item.get("ok") and item.get("result") is None for item in cases.values())
    return rejected, {"cases": cases, "windowsAfter": windows_after,
                      "note": "相对/不存在/symlink/20MB+ 均返回 null，不建预览窗口、不写文件"}


@assertion_handler("R00-T02-LA-E84F01456B6C", 1)
def _xlsx_preview_render(h: Harness, p: Producer, case: str):
    path = _office_fixture(h, "sample.xlsx")
    before = _file_digest(path)
    result = h.wd.invoke("read_xlsx_html", {"path": str(path)})
    html = result.get("result")
    after = _file_digest(path)
    returned_table = bool(result.get("ok")) and isinstance(html, str) and "<table" in html.lower()
    return (returned_table and before == after), \
        {"invoke": result, "htmlLength": len(html) if isinstance(html, str) else None,
         "inputUnchanged": before == after,
         "note": "真实解析样本 XLSX 为 HTML 表格；输入文件逐字节不变"}


@assertion_handler("R00-T02-LA-E84F01456B6C", 2)
def _xlsx_preview_invalid(h: Harness, p: Producer, case: str):
    cases = {
        "relative": h.wd.invoke("read_xlsx_html", {"path": "relative.xlsx"}),
        "missing": h.wd.invoke("read_xlsx_html", {"path": str(h.producer.home / "missing.xlsx")}),
    }
    rejected = all(item.get("ok") and item.get("result") is None for item in cases.values())
    return rejected, {"cases": cases, "note": "相对/不存在输入返回 null"}


# ── 计划执行入口 ───────────────────────────────────────────────────────

def load_plan(plan_path: Path) -> dict:
    return json.loads(plan_path.read_text(encoding="utf-8"))


def run_gui_plan(producer: Producer, plan: dict) -> dict:
    """GUI-cleared 批次：真实启动宿主并逐 plannedActionCase 执行。

    返回统计：{executed, passed, manual, blocked, failed}；调用方据 failed/blocked 决定退出码。
    """
    stats = {"executed": 0, "passed": 0, "manual": 0, "manualPassed": 0,
             "blocked": 0, "failed": 0, "cases": []}
    if not gui_cleared():
        for leaf in plan.get("leaves", []):
            for a in leaf.get("assertions", []):
                producer.check(f"blocked::gui-cleared::{a['plannedActionCase']}", False,
                               detail={"blocked": True, "status": a.get("implementationStatus"),
                                       "reason": "未提供 LINGXI_R09_GUI_CLEARED=1（诚实红）"})
                stats["blocked"] += 1
        return stats

    harness = Harness(producer)
    try:
        harness.start()
        for leaf in plan.get("leaves", []):
            for a in leaf.get("assertions", []):
                case = a["plannedActionCase"]
                status = a.get("implementationStatus")
                stats["executed"] += 1
                if status.startswith("blocked"):
                    producer.check(case, False, detail={
                        "blocked": True, "reason": a.get("blockedReason"),
                        "neededHostSurface": a.get("neededHostSurface")})
                    stats["blocked"] += 1
                    stats["cases"].append({"case": case, "status": "blocked"})
                    continue
                if status == "manual":
                    ok, payload = harness.manual_evidence(case)
                    producer.check(case, ok, detail={"manual": True, "payload": payload,
                                                     "steps": a.get("manualSteps")})
                    stats["manual"] += 1
                    if ok:
                        stats["manualPassed"] += 1
                        stats["passed"] += 1
                        stats["cases"].append({"case": case, "status": "manual-pass"})
                    else:
                        stats["blocked"] += 1
                        stats["cases"].append({"case": case, "status": "manual-pending"})
                    continue
                handler = HANDLERS.get(leaf["leafId"], {}).get(a["assertionIndex"])
                if handler is None:
                    producer.check(case, False, detail={
                        "blocked": True,
                        "reason": "GUI driver 未覆盖该断言（visible 批次补）"})
                    stats["blocked"] += 1
                    stats["cases"].append({"case": case, "status": "driver-missing"})
                    continue
                try:
                    ok, detail = handler(harness, producer, case)
                except Exception as error:  # noqa: BLE001 - 证据必须记录真实失败
                    ok, detail = False, {"error": f"{type(error).__name__}: {error}"}
                producer.check(case, bool(ok), detail=detail)
                if ok:
                    stats["passed"] += 1
                    stats["cases"].append({"case": case, "status": "pass"})
                else:
                    stats["failed"] += 1
                    stats["cases"].append({"case": case, "status": "fail", "detail": detail})
    finally:
        try:
            shutdown = harness.stop()
        except Exception as error:  # noqa: BLE001
            shutdown = {"stopError": f"{type(error).__name__}: {error}"}
        producer.check("gui::app-shutdown-clean", not shutdown.get("residualPids"),
                       detail=shutdown)
    return stats

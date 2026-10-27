#!/usr/bin/env python3
"""真实服务 Origin/CSP 接缝探针（R09 D6，Lead 证据生产者）。

用隔离 LINGXI_HOME 启动真实 lingxi-service 二进制，验证：
  1) Tauri 三来源精确放行（`tauri://localhost` / `http://tauri.localhost` /
     `https://tauri.localhost`）；
  2) 近似伪造来源拒绝（`tauri://evil` / 子域 / 带端口），错误为 403 bad_origin；
  3) 预检 OPTIONS 对放行来源返回 204 + ACAO 回显 + Vary: Origin；
  4) 认证面：Bearer token 在放行来源下可用，在伪造来源下仍被 Origin 闸拒绝；
  5) 退出清理：SIGTERM 后进程消失（不残留）。
证据写 <evidence>/result.json + output.log + 响应抓取。
"""

from __future__ import annotations

import json
import os
import signal
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[1]
SERVICE = ROOT / "rust/target/debug/lingxi-service"
RUNTIME = Path("/private/tmp/lingxi-r09-origin-probe")


def http(method: str, url: str, headers: dict | None = None, timeout: float = 5):
    request = urllib.request.Request(url, method=method, headers=headers or {})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return {
                "status": response.status,
                "headers": {k.lower(): v for k, v in response.headers.items()},
                "body": response.read().decode("utf-8", "replace"),
            }
    except urllib.error.HTTPError as error:
        return {
            "status": error.code,
            "headers": {k.lower(): v for k, v in error.headers.items()},
            "body": error.read().decode("utf-8", "replace"),
        }


def wait_ready(process: subprocess.Popen, log_path: Path, timeout: float = 60.0) -> str:
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


def main() -> int:
    evidence = Path(sys.argv[sys.argv.index("--evidence") + 1]) \
        if "--evidence" in sys.argv else None
    if evidence is None:
        print("usage: r09_service_seam_probe.py --evidence <dir>", file=sys.stderr)
        return 64
    evidence = evidence if evidence.is_absolute() else ROOT / evidence
    evidence.mkdir(parents=True, exist_ok=True)
    if not SERVICE.is_file():
        print(f"error: service binary missing at {SERVICE}; build it first "
              f"(rustup run 1.98.1 cargo build --manifest-path rust/Cargo.toml --locked -p lingxi-service)",
              file=sys.stderr)
        return 64
    stamp = f"{os.getpid()}-{int(time.time() * 1000)}"
    home = RUNTIME / f"home-{stamp}"
    home.mkdir(parents=True, exist_ok=True)
    stdout_log = evidence / "service-stdout.log"
    stderr_log = evidence / "service-stderr.log"
    cases: list[dict] = []
    transcript: list[dict] = []

    # 注意：home 必须保持“全新空目录”，否则数据 epoch 闸会把未知数据当作
    # 未标定旧库拒绝启动（unstamped-home-with-data，真实安全行为）。临时目录
    # 放在 home 之外的兄弟目录。
    scratch = RUNTIME / f"tmp-{stamp}"
    scratch.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    env.update({"LINGXI_HOME": str(home), "TMPDIR": str(scratch), "RUST_BACKTRACE": "1"})
    with stdout_log.open("w") as out, stderr_log.open("w") as err:
        process = subprocess.Popen(
            [str(SERVICE), "--home", str(home)],
            cwd=str(SERVICE.parent), env=env,
            stdout=subprocess.PIPE, stderr=err, text=True, bufsize=1)
        try:
            ready = wait_ready(process, stdout_log)
            address = None
            for token in ready.replace("=", " ").split():
                if ":" in token and token.split(":")[0] in ("127.0.0.1", "localhost"):
                    address = token
            if address is None:
                raise RuntimeError(f"cannot parse READY address from {ready!r}")
            base = f"http://{address}"
            instance = json.loads((home / "lingxi-service/instance.json").read_text())
            token_record = json.loads((home / "lingxi-service/local-token.json").read_text())
            bearer = token_record.get("token") or token_record.get("accessToken")
            if not bearer:
                raise RuntimeError(f"no bearer token in local-token.json: {token_record}")

            def check(name: str, method: str, path: str, origin: str | None,
                      expected_status: int, auth: bool = False, expect_acao: bool = False):
                headers = {}
                if origin is not None:
                    headers["Origin"] = origin
                if auth:
                    headers["Authorization"] = f"Bearer {bearer}"
                response = http(method, base + path, headers)
                okay = response["status"] == expected_status
                if expect_acao and origin is not None:
                    okay = okay and response["headers"].get("access-control-allow-origin") == origin
                    okay = okay and "origin" in response["headers"].get("vary", "").lower()
                body_head = response["body"][:300]
                if expected_status == 403:
                    okay = okay and "bad_origin" in response["body"]
                cases.append({"case": name, "ok": okay,
                              "expectedStatus": expected_status,
                              "actualStatus": response["status"],
                              "detail": {"origin": origin, "bodyHead": body_head,
                                         "acao": response["headers"].get("access-control-allow-origin")}})
                transcript.append({"case": name, "method": method, "path": path,
                                   "origin": origin, "status": response["status"],
                                   "headers": {k: v for k, v in response["headers"].items()
                                               if k in ("access-control-allow-origin",
                                                        "access-control-allow-credentials",
                                                        "access-control-allow-methods",
                                                        "vary")},
                                   "bodyHead": body_head})
                print(f"[{'PASS' if okay else 'FAIL'}] {name}: "
                      f"origin={origin} expected={expected_status} got={response['status']}")

            check("seam::health-no-origin", "GET", "/lingxi/v1/health", None, 200)
            for origin in ["tauri://localhost", "http://tauri.localhost",
                           "https://tauri.localhost"]:
                check(f"seam::health-allowed-{origin.replace('://', '-')}",
                      "GET", "/lingxi/v1/health", origin, 200, expect_acao=True)
            for origin in ["tauri://evil", "http://evil.tauri.localhost",
                           "http://tauri.localhost:8080", "http://tauri.localhost.evil.example"]:
                check(f"seam::health-forbidden-{origin.replace('://', '-').replace('/', '_')}",
                      "GET", "/lingxi/v1/health", origin, 403)
            check("seam::preflight-allowed", "OPTIONS", "/lingxi/v1/health",
                  "tauri://localhost", 204, expect_acao=True)
            check("seam::preflight-forbidden", "OPTIONS", "/lingxi/v1/health",
                  "tauri://evil", 403)
            check("seam::identity-authenticated-allowed", "GET",
                  "/lingxi/v1/server/identity", "tauri://localhost", 200, auth=True)
            check("seam::identity-authenticated-forbidden-origin", "GET",
                  "/lingxi/v1/server/identity", "tauri://evil", 403, auth=True)

            # 退出清理：SIGTERM → 进程必须消失。
            process.send_signal(signal.SIGTERM)
            try:
                exit_code = process.wait(timeout=40)
            except subprocess.TimeoutExpired:
                process.kill()
                exit_code = None
            time.sleep(0.5)
            alive = False
            try:
                os.kill(process.pid, 0)
                alive = True
            except ProcessLookupError:
                alive = False
            cases.append({"case": "seam::shutdown-no-residual", "ok": not alive,
                          "actualStatus": 1 if not alive else 0,
                          "detail": {"exitCode": exit_code, "pid": process.pid}})
            print(f"[{'PASS' if not alive else 'FAIL'}] seam::shutdown-no-residual "
                  f"exit={exit_code} alive={alive}")

            failed = [row["case"] for row in cases if not row["ok"]]
            status = "PASS" if not failed else "FAIL"
            result = {
                "schema": "lingxi.r09.service-seam-probe.v1",
                "serviceBinary": str(SERVICE.relative_to(ROOT)),
                "serviceSha256": __import__("hashlib").sha256(SERVICE.read_bytes()).hexdigest(),
                "home": str(home),
                "instance": {k: instance.get(k) for k in
                             ("serverKind", "pid", "bindAddr", "instanceId", "homePath")},
                "cases": cases,
                "failedCases": failed,
                "status": status,
                "transcript": transcript,
                "note": "真实服务进程 + 真实 HTTP；不使用 mock。CSP frame-ancestors 由 preview.rs 单测与 T05 浏览器探针覆盖。",
            }
            (evidence / "result.json").write_text(
                json.dumps(result, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
            # 地图 evidencePaths 要求 output.log；把逐 case 观察写成真实日志文件。
            (evidence / "output.log").write_text(
                "\n".join(
                    f"[{'PASS' if row['ok'] else 'FAIL'}] {row['case']}"
                    for row in cases) + "\n",
                encoding="utf-8")
            print(f"origin seam probe: status={status} cases={len(cases)} failed={failed}")
            return 0 if status == "PASS" else 2
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=10)


if __name__ == "__main__":
    sys.exit(main())

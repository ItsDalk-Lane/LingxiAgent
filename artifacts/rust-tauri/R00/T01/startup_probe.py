#!/usr/bin/env python3
"""R00-T01：在独立目录启动真实服务，并记录隔离证据。"""

import hashlib
import json
import os
import pathlib
import shutil
import signal
import stat
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone


ROOT = pathlib.Path(__file__).resolve().parents[4]
OUT = pathlib.Path(__file__).resolve().parent
REAL_HOME = pathlib.Path.home()
PRODUCTION = REAL_HOME / ".lingxi"
DEVELOPMENT = REAL_HOME / ".lingxi-dev"


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def directory_inventory(path):
    """只读记录相对路径、类型、大小、权限和时间的整体摘要，不保存用户文件名。"""
    digest = hashlib.sha256()
    counts = {"files": 0, "directories": 0, "symlinks": 0, "errors": 0}
    if not path.exists():
        return {"exists": False, "inventory_sha256": digest.hexdigest(), **counts}
    for base, dirs, files in os.walk(path, topdown=True, followlinks=False):
        entries = sorted(dirs + files)
        for name in entries:
            item = pathlib.Path(base) / name
            relative = item.relative_to(path).as_posix()
            try:
                info = item.lstat()
            except OSError as error:
                counts["errors"] += 1
                digest.update(f"ERROR\0{relative}\0{error.errno}\n".encode())
                continue
            kind = "symlink" if stat.S_ISLNK(info.st_mode) else "directory" if stat.S_ISDIR(info.st_mode) else "file"
            counts[{"symlink": "symlinks", "directory": "directories", "file": "files"}[kind]] += 1
            digest.update(f"{relative}\0{kind}\0{info.st_mode}\0{info.st_size}\0{info.st_mtime_ns}\n".encode())
    return {"exists": True, "inventory_sha256": digest.hexdigest(), **counts}


def health_probe(port, token):
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/api/health",
        headers={"Authorization": f"Bearer {token}"},
    )
    with urllib.request.urlopen(request, timeout=3) as response:
        payload = json.load(response)
        return {"http_status": response.status, "body_status": payload.get("status"), "version": payload.get("version")}


def safe_process_args(pid):
    result = subprocess.run(["ps", "-p", str(pid), "-o", "pid=,ppid=,command="], capture_output=True, text=True)
    return result.stdout.strip()


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    test_root = pathlib.Path(tempfile.mkdtemp(prefix="lingxi-r00-t01-", dir="/tmp"))
    isolated = test_root / "isolated"
    protected = test_root / "protected-production"
    for directory in (isolated, protected, isolated / "tmp"):
        directory.mkdir(parents=True, exist_ok=True)
    protected_sentinel = protected / "R00-READ-ONLY-SENTINEL"
    protected_sentinel.write_text("R00-T01 protected production sentinel\n", encoding="utf-8")
    protected_sentinel.chmod(0o444)
    protected.chmod(0o555)
    actual_home = isolated / "lingxi-home"
    before = {
        "production": directory_inventory(PRODUCTION),
        "development": directory_inventory(DEVELOPMENT),
        "protected": directory_inventory(protected),
        "protected_sentinel_sha256": sha256_file(protected_sentinel),
        "real_production_sentinel_sha256": sha256_file(PRODUCTION / "data-epoch.json") if (PRODUCTION / "data-epoch.json").is_file() else None,
    }
    profile = test_root / "probe.sb"
    profile.write_text(
        '(version 1)\n(allow default)\n'
        f'(deny file-write* (subpath "{REAL_HOME}"))\n'
        f'(deny file-write* (subpath "{protected}"))\n',
        encoding="utf-8",
    )
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "LANG": os.environ.get("LANG", "en_US.UTF-8"),
        "HOME": str(isolated),
        "TMPDIR": str(isolated / "tmp"),
        "LINGXI_HOME": str(actual_home),
        "LINGXI_ROOT": str(ROOT),
        "LINGXI_SERVER_ENTRY": str(ROOT / "server" / "main-full.ts"),
        "LINGXI_PORT": "0",
        "LINGXI_CREATE_STARTUP_SESSION": "0",
    }
    command = ["sandbox-exec", "-f", str(profile), shutil.which("node") or "node", "server/bootstrap.ts"]
    started = datetime.now(timezone.utc).isoformat()
    with (OUT / "server-stdout.txt").open("w", encoding="utf-8") as stdout, (OUT / "server-stderr.txt").open("w", encoding="utf-8") as stderr:
        child = subprocess.Popen(command, cwd=ROOT, env=env, stdout=stdout, stderr=stderr, start_new_session=True)
        info_path = actual_home / "server-info.json"
        result = {
            "started_utc": started,
            "candidate_sha": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
            "command": command,
            "pid": child.pid,
            "process_args": safe_process_args(child.pid),
            "test_root": str(test_root),
            "isolated_home_env": str(actual_home),
            "home_env": str(isolated),
            "sandbox_profile": str(profile),
            "before": before,
            "startup": {},
        }
        try:
            deadline = time.monotonic() + 90
            info = None
            while time.monotonic() < deadline:
                if child.poll() is not None:
                    result["startup"]["error"] = f"server exited before ready: {child.returncode}"
                    break
                try:
                    info = json.loads(info_path.read_text(encoding="utf-8"))
                    break
                except (OSError, json.JSONDecodeError):
                    time.sleep(0.2)
            if info is None and "error" not in result["startup"]:
                result["startup"]["error"] = "server-info.json timeout after 90 seconds"
            if info is not None:
                result["startup"].update({
                    "server_info_path": str(info_path),
                    "server_info_pid": info.get("pid"),
                    "server_info_port": info.get("port"),
                    "server_info_token_present": bool(info.get("token")),
                    "server_info_home": info.get("lingxiHome"),
                    "server_process_args": safe_process_args(info.get("pid")) if isinstance(info.get("pid"), int) else "",
                })
                try:
                    result["startup"]["health"] = health_probe(info["port"], info["token"])
                except (OSError, urllib.error.HTTPError, KeyError) as error:
                    result["startup"]["health_error"] = f"{type(error).__name__}: {error}"
                result["startup"]["isolated_inventory_during"] = directory_inventory(isolated)
        finally:
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
            try:
                child.wait(timeout=20)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait(timeout=5)
                result["shutdown_forced"] = True
            result["exit_code"] = child.returncode
            result["server_info_removed"] = not info_path.exists()
    result["after"] = {
        "production": directory_inventory(PRODUCTION),
        "development": directory_inventory(DEVELOPMENT),
        "protected": directory_inventory(protected),
        "protected_sentinel_sha256": sha256_file(protected_sentinel),
        "real_production_sentinel_sha256": sha256_file(PRODUCTION / "data-epoch.json") if (PRODUCTION / "data-epoch.json").is_file() else None,
        "isolated": directory_inventory(isolated),
    }
    result["controlled_production_unchanged"] = (
        before["protected"] == result["after"]["protected"]
        and before["protected_sentinel_sha256"] == result["after"]["protected_sentinel_sha256"]
    )
    result["real_production_inventory_unchanged"] = before["production"] == result["after"]["production"]
    result["real_development_inventory_unchanged"] = before["development"] == result["after"]["development"]
    result["real_production_sentinel_unchanged"] = before["real_production_sentinel_sha256"] == result["after"]["real_production_sentinel_sha256"]
    result["ended_utc"] = datetime.now(timezone.utc).isoformat()
    result["acceptance_checks"] = {
        "health_ok": result["startup"].get("health", {}).get("http_status") == 200
        and result["startup"].get("health", {}).get("body_status") == "ok",
        "closed_cleanly": result["exit_code"] == 0 and result["server_info_removed"],
        "actual_home_logged": f"lingxiHome={actual_home}" in (OUT / "server-stdout.txt").read_text(errors="replace"),
        "isolated_data_and_logs_present": result["after"]["isolated"]["files"] > 0
        and (actual_home / "logs").is_dir(),
        "protected_production_unchanged": result["controlled_production_unchanged"],
        "real_production_inventory_unchanged": result["real_production_inventory_unchanged"],
        "real_development_inventory_unchanged": result["real_development_inventory_unchanged"],
        "real_production_sentinel_unchanged": result["real_production_sentinel_unchanged"],
    }
    (OUT / "startup-probe.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return 0 if all(result["acceptance_checks"].values()) else 1


if __name__ == "__main__":
    raise SystemExit(main())

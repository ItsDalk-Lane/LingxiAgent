#!/usr/bin/env python3
"""F47 永久回归：两个外层日志级别均执行完整正式启动检查。"""
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    root = Path(__file__).resolve().parents[2]
    evidence = Path(sys.argv[1]).resolve()
    # 不混用旧证据；临时服务数据由原正式脚本自己创建和有界回收。
    evidence.mkdir(parents=True, exist_ok=False)
    script = root / "scripts/rust-tauri/r02_t01_service_smoke.sh"
    expected = {
        "a01-build-locked-offline", "a01-deptree-desktop-free",
        "a01-service-start-ready", "a01-health-200",
        "a01-sigterm-clean-exit-0", "a01-no-leftover-children",
        "a01-port-closed-after-shutdown", "a01-startup-failure-no-ready",
    }
    tags = ["a01-newer-data", "a01-newer-env-data", "a01-newer-config-data"]
    for tag in tags:
        expected.update(f"{tag}-epoch-{suffix}" for suffix in ["refused", "explicit-error", "no-root-switch"])
    rows = []
    for level in ["warn", "info"]:
        env = os.environ.copy()
        env["RUST_LOG"] = level
        # 短段合成路径让原精确选根断言有效；不改变个人路径遮盖规则。
        env["TMPDIR"] = "/tmp"
        out = evidence / level
        argv = ["bash", str(script), str(out)]
        before = digest(script)
        started = datetime.datetime.now(datetime.timezone.utc).isoformat()
        with (evidence / f"{level}.stdout.log").open("wb") as stdout, (evidence / f"{level}.stderr.log").open("wb") as stderr:
            result = subprocess.run(argv, cwd=root, env=env, stdout=stdout, stderr=stderr)
        record = {"argv": argv, "cwd": str(root), "UTC_start": started,
                  "UTC_end": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  "exit": result.returncode, "outerRUST_LOG": level,
                  "scriptBefore": before, "scriptAfter": digest(script)}
        (evidence / f"{level}.command.json").write_text(json.dumps(record, indent=2))
        assert result.returncode == 0, f"{level} 完整正式脚本失败，详见原始输出"
        assert before == digest(script), "运行期间检查脚本变化"
        doc = json.loads((out / "a01-leaf-cases.json").read_text())
        cases = doc["cases"]
        assert len(cases) == 17 and {c["case"] for c in cases} == expected
        assert all(c["ok"] and c["actual"] == c["expect"] for c in cases)
        for tag, source in zip(tags, ["cli", "env", "config-file"]):
            err = (out / f"{tag}.stderr.log").read_text()
            originals = (out / f"{tag}-original-before.sha256").read_bytes()
            assert originals == (out / f"{tag}-original-after.sha256").read_bytes()
            assert (out / f"{tag}-fallback-before.txt").read_bytes() == (out / f"{tag}-fallback-after.txt").read_bytes()
            selected = originals.decode().splitlines()[0].split("  ", 1)[1].removesuffix("/data-epoch.json")
            assert f"effective_home={selected}" in err and f"source={source}" in err
            assert "LINGXI_DATA_EPOCH_BLOCKED reason=epoch-downgrade-blocked" in err
            assert "epoch 2 or newer" in err
            assert "LINGXI_SERVICE_READY " not in (out / f"{tag}.stdout.log").read_text()
        rows.append({"outerRUST_LOG": level, "actual": len(cases), "failed": 0, "sources": ["cli", "env", "config-file"]})
    (evidence / "result.json").write_text(json.dumps(rows, indent=2))
    print(json.dumps(rows))


if __name__ == "__main__":
    main()

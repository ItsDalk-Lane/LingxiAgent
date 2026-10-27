#!/usr/bin/env python3
"""R09 生产者套件统一入口（阶段门内被真实执行）。

用法：
  python3 -B scripts/rust-tauri/r09_suites.py --suite <name> --evidence <dir>

按套件名前缀分派到同目录任务模块 `r09_t01.py` … `r09_t08.py`（按需创建，每个
任务模块只写自己的套件）。**未实现套件必须真实失败**：写
result.json（status=NOT_IMPLEMENTED）与 output.log，退出码 2——门据此判红，
绝不以“注册即通过”冒充验收。

本文件不包含任何验收语义；语义在任务模块内。
"""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[1]
TASK_MODULES = {
    "r09_t01": "r09_t01",
    "r09_t02": "r09_t02",
    "r09_t03": "r09_t03",
    "r09_t04": "r09_t04",
    "r09_t05": "r09_t05",
    "r09_t06": "r09_t06",
    "r09_t07": "r09_t07",
    "r09_t08": "r09_t08",
}


def _arg(name: str) -> str | None:
    argv = sys.argv
    if name in argv:
        index = argv.index(name) + 1
        if index < len(argv):
            return argv[index]
    return None


def _receipt(evidence: Path, suite: str, status: str, exit_code: int,
             detail: str, extra: dict | None = None) -> int:
    evidence.mkdir(parents=True, exist_ok=True)
    log = f"suite={suite} status={status} exit={exit_code}\n{detail}\n"
    (evidence / "output.log").write_text(log, encoding="utf-8")
    payload = {
        "schema": "lingxi.r09.suite-receipt.v1",
        "suite": suite,
        "status": status,
        "exitCode": exit_code,
        "detail": detail,
        "finishedAtUnixMs": int(time.time() * 1000),
    }
    if extra:
        payload.update(extra)
    (evidence / "result.json").write_text(
        json.dumps(payload, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(log, end="")
    return exit_code


def main() -> int:
    suite = _arg("--suite")
    evidence_raw = _arg("--evidence")
    if not suite or not evidence_raw:
        print("usage: r09_suites.py --suite <name> --evidence <dir>", file=sys.stderr)
        return 64
    evidence = Path(evidence_raw)
    if not evidence.is_absolute():
        evidence = ROOT / evidence
    prefix = "_".join(suite.split("_")[:2])
    module_name = TASK_MODULES.get(prefix)
    if module_name is None:
        return _receipt(evidence, suite, "UNKNOWN_SUITE", 64,
                        f"套件 {suite} 不属于已注册任务前缀 {sorted(TASK_MODULES)}")
    module_path = SCRIPTS / f"{module_name}.py"
    if not module_path.is_file():
        return _receipt(evidence, suite, "NOT_IMPLEMENTED", 2,
                        f"任务模块 {module_path.name} 尚未实现（R09 进行中）")
    sys.path.insert(0, str(SCRIPTS))
    import importlib
    module = importlib.import_module(module_name)
    run_suite = getattr(module, "run_suite", None)
    if run_suite is None:
        return _receipt(evidence, suite, "NOT_IMPLEMENTED", 2,
                        f"{module_path.name} 未导出 run_suite()")
    started = time.time()
    code = run_suite(suite, evidence)
    if not (evidence / "result.json").is_file():
        return _receipt(evidence, suite, "PRODUCER_WROTE_NO_RECEIPT", 3,
                        f"{module_path.name} ran for {time.time() - started:.1f}s but wrote no result.json")
    return code


if __name__ == "__main__":
    sys.exit(main())

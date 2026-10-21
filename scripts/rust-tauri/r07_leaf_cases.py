#!/usr/bin/env python3
"""R07 叶级案例生产者：把本轮各套件的真实 libtest 输出转成
lingxi.leaf-case-results.v1 案例文件，供补充叶 assertionContract 钉住。

案例名 = `<suite>::<test>`（suite 为地图命令键，test 为日志中的测试名逐字）。
每条案例 expect=1、actual=1（通过）/0（未通过/未执行）、ok=(actual==expect)。
日志缺失或套件回执退出码非 0 时，该套件全部案例 actual=0——绝不把未跑或
失败的测试记成通过。生产者自身只读日志，不执行测试，不改写任何套件回执。

用法：python3 r07_leaf_cases.py --evidence <EVIDENCE_ROOT> --out <dir>
退出码：产物写出即 0；读不到任何套件日志退出 1（空证据外壳不得成立）。
"""

from __future__ import annotations

import hashlib
import json
import os
import sys
from pathlib import Path

ROOT = Path(os.environ.get("LINGXI_ROOT", Path(__file__).resolve().parents[2]))
STAGE_MAP = ROOT / "rust/crates/xtask/src/stage_maps/R07.json"


def main(argv: list[str]) -> int:
    if "--evidence" not in argv or "--out" not in argv:
        raise SystemExit("--evidence <root> --out <dir> required")
    evidence = Path(argv[argv.index("--evidence") + 1])
    out = Path(argv[argv.index("--out") + 1])
    out.mkdir(parents=True, exist_ok=True)
    stage_map = json.loads(STAGE_MAP.read_text(encoding="utf-8"))
    suites = sorted(
        key for key, spec in stage_map["commands"].items()
        if any("--run-suite" in part for part in spec.get("argv", []))
    )
    # UI 套件（r07_ui_cases.py）不走 --run-suite：扫描证据目录下全部
    # SUITES/<suite> 实际落盘的回执，任何真实执行的套件都纳入案例文件。
    suites_dir = evidence / "SUITES"
    if suites_dir.is_dir():
        for entry in sorted(os.listdir(suites_dir)):
            if (suites_dir / entry / "output.log").is_file() and entry not in suites:
                suites.append(entry)
        suites.sort()
    cases = []
    suites_seen = 0
    suite_summary = {}
    for suite in suites:
        log_path = evidence / "SUITES" / suite / "output.log"
        receipt_path = evidence / "SUITES" / suite / "result.json"
        exit_code = None
        if receipt_path.is_file():
            try:
                exit_code = json.loads(receipt_path.read_text(encoding="utf-8")).get("exitCode")
            except json.JSONDecodeError:
                exit_code = None
        tests: dict[str, bool] = {}
        if log_path.is_file():
            suites_seen += 1
            for line in log_path.read_text(encoding="utf-8", errors="replace").splitlines():
                if not line.startswith("test ") or " ... " not in line:
                    continue
                name, _, verdict = line[5:].partition(" ... ")
                name = name.strip()
                verdict = verdict.strip()
                if not name or name.startswith("result:"):
                    continue
                tests[name] = verdict == "ok"
        suite_ok = exit_code == 0
        passed = sum(1 for ok in tests.values() if ok)
        suite_summary[suite] = {
            "exitCode": exit_code,
            "testsSeen": len(tests),
            "testsPassed": passed,
            "logPresent": log_path.is_file(),
        }
        for name in sorted(tests):
            actual = 1 if (tests[name] and suite_ok) else 0
            cases.append({
                "case": f"{suite}::{name}",
                "expect": 1,
                "actual": actual,
                "ok": actual == 1,
                "suite": suite,
                "suiteExitCode": exit_code,
            })
    if suites_seen == 0:
        print("leaf cases FAILED: no suite log readable under evidence root", file=sys.stderr)
        return 1
    payload = {
        "schema": "lingxi.leaf-case-results.v1",
        "generatedBy": "scripts/rust-tauri/r07_leaf_cases.py",
        "stage": "R07",
        "suites": suite_summary,
        "cases": cases,
    }
    text = json.dumps(payload, ensure_ascii=False, indent=1) + "\n"
    (out / "leaf-cases.json").write_text(text, encoding="utf-8")
    digest = hashlib.sha256(text.encode("utf-8")).hexdigest()
    (out / "leaf-cases.sha256").write_text(digest + "\n", encoding="utf-8")
    failing = sum(1 for c in cases if not c["ok"])
    print(f"leaf cases: {len(cases)} cases from {suites_seen} suites ({failing} not holding)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

#!/usr/bin/env python3
"""R09 门禁/生产者隔离负例探针：证明“零匹配/关键 skip/失败案例”不能变成 PASS。

只调用仓库里的真实工具（`scripts/rust-tauri/r09_producer.py` 的 Producer 与
cargo 汇总解析），不 mock 被测对象、不写生产代码。全部损坏样本都在
`/private/tmp` 隔离目录中构造；最后用“通过样本”证明工具未因负例损坏。

覆盖：
  1. 零匹配：没有产生任何 case 的“成功”套件必须 FAIL（不得以退出 0 冒充）；
  2. 失败案例：任一 case ok=false → 套件 FAIL 且回执列出 failedCases；
  3. 关键 skip：cargo 输出含 ignored/skipped 必须被拒绝（skip ≠ pass）；
  4. 零匹配：无可解析 cargo 汇总行必须被拒绝；
  5. 正常通过：全部 case ok=true → PASS，且 cases.json schema 正确；
  6. 还原：负例之后同一工具对正常样本仍 PASS（损坏样本未污染实现）。

输出（可被阶段门消费）：
  result.json / gate-negative-cases.json（lingxi.leaf-case-results.v1）/ output.log
"""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import time
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[1]
CASES: list[dict] = []
LOGS: list[str] = []


def load_producer():
    spec = importlib.util.spec_from_file_location(
        "r09_producer", SCRIPTS / "r09_producer.py")
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def case(name: str, ok: bool, detail=None) -> bool:
    CASES.append({"case": name, "ok": bool(ok), "actual": 1 if ok else 0, "detail": detail})
    return bool(ok)


def log(message: str) -> None:
    print(message, flush=True)
    LOGS.append(message)


def read(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def main() -> int:
    started = time.time()
    evidence = Path(sys.argv[sys.argv.index("--evidence") + 1]) \
        if "--evidence" in sys.argv else None
    if evidence is None:
        print("usage: r09_gate_negative_probe.py --evidence <dir>", file=sys.stderr)
        return 64
    evidence = evidence if evidence.is_absolute() else ROOT / evidence
    evidence.mkdir(parents=True, exist_ok=True)
    producer = load_producer()

    with tempfile.TemporaryDirectory(prefix="lingxi-r09-gate-negative-") as temporary:
        base = Path(temporary)

        # 1) 零匹配：没有 case 的 Producer 必须 FAIL，不能退出 0。
        zero_dir = base / "zero-cases"
        zero = producer.Producer("probe_zero_cases", zero_dir)
        zero_code = zero.finish()
        zero_receipt = read(zero_dir / "result.json")
        case("gate-negative::zero-cases-fails", zero_code != 0
             and zero_receipt.get("status") == "FAIL"
             and "zeroCaseGuard" in zero_receipt,
             {"exit": zero_code, "receiptStatus": zero_receipt.get("status")})
        log(f"zero-case producer exit={zero_code} status={zero_receipt.get('status')}")

        # 2) 失败案例：ok=false 必须 FAIL 并带 failedCases。
        fail_dir = base / "failed-case"
        failing = producer.Producer("probe_failed_case", fail_dir)
        failing.check("probe::will-fail", False, 0, detail="synthetic negative")
        failing.check("probe::will-pass", True, 1)
        fail_code = failing.finish()
        fail_receipt = read(fail_dir / "result.json")
        case("gate-negative::failed-case-fails", fail_code != 0
             and fail_receipt.get("status") == "FAIL"
             and fail_receipt.get("failedCases") == ["probe::will-fail"],
             {"exit": fail_code, "failedCases": fail_receipt.get("failedCases")})

        # 3) 关键 skip：cargo 汇总含 ignored 必须被 require_no_skips 拒绝。
        dirty_log = ("running 3 tests\n"
                     "test a ... ok\ntest b ... ok\n"
                     "test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out\n")
        try:
            producer.require_no_skips(dirty_log, "probe-dirty")
            skip_rejected = False
            detail = "not raised"
        except producer.EvidenceError as error:
            skip_rejected = True
            detail = str(error)
        case("gate-negative::ignored-tests-rejected", skip_rejected, {"error": detail})

        # 4) 零匹配：没有 cargo 汇总行必须被拒绝。
        try:
            producer.require_no_skips("build ok but no summary", "probe-zero-match")
            zero_match_rejected, zero_detail = False, "not raised"
        except producer.EvidenceError as error:
            zero_match_rejected, zero_detail = True, str(error)
        case("gate-negative::cargo-zero-match-rejected", zero_match_rejected,
             {"error": zero_detail})

        # 5) 正常通过：全部 case ok、无 skip → PASS 且 cases schema 正确。
        pass_dir = base / "passing"
        passing = producer.Producer("probe_passing", pass_dir)
        passing.check("probe::one", True, 1)
        passing.check("probe::two", True, 1, detail={"what": "second"})
        clean_log = ("test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
                     "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n")
        summary = producer.require_no_skips(clean_log, "probe-clean")
        pass_code = passing.finish()
        pass_receipt = read(pass_dir / "result.json")
        pass_cases = read(pass_dir / "cases.json")
        case("gate-negative::clean-run-passes", pass_code == 0
             and pass_receipt.get("status") == "PASS"
             and summary.get("passed") == 7,
             {"exit": pass_code, "summary": summary})
        case("gate-negative::cases-schema-correct",
             pass_cases.get("schema") == "lingxi.leaf-case-results.v1"
             and pass_cases.get("suite") == "probe_passing"
             and [row["case"] for row in pass_cases.get("cases", [])]
             == ["probe::one", "probe::two"]
             and all(row.get("ok") is True for row in pass_cases["cases"]),
             {"schema": pass_cases.get("schema"), "cases": pass_cases.get("cases")})

        # 6) 还原：负例之后正常样本仍 PASS（工具实现未被样本污染）。
        restore_dir = base / "restored"
        restored = producer.Producer("probe_restored", restore_dir)
        restored.check("probe::restored", True, 1)
        case("gate-negative::tool-restored-after-negatives", restored.finish() == 0
             and read(restore_dir / "result.json").get("status") == "PASS",
             {"exit": 0})

    failed = [row["case"] for row in CASES if not row["ok"]]
    status = "PASS" if not failed else "FAIL"
    (evidence / "gate-negative-cases.json").write_text(json.dumps(
        {"schema": "lingxi.leaf-case-results.v1", "suite": "r09_gate_negative",
         "cases": CASES}, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    output = "\n".join(LOGS) + "\n"
    (evidence / "output.log").write_text(output, encoding="utf-8")
    (evidence / "result.json").write_text(json.dumps({
        "schema": "lingxi.r09.gate-negative.v1",
        "suite": "r09_gate_negative",
        "status": status,
        "exitCode": 0 if status == "PASS" else 2,
        "caseCount": len(CASES),
        "failedCases": failed,
        "durationSecs": round(time.time() - started, 3),
        "note": "真实调用 r09_producer 工具；损坏样本在隔离临时目录构造并还原。",
    }, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"r09_gate_negative: status={status} cases={len(CASES)} failed={failed}")
    return 0 if status == "PASS" else 2


if __name__ == "__main__":
    sys.exit(main())

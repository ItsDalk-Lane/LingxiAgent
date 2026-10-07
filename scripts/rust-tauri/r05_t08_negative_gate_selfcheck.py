#!/usr/bin/env python3
"""F45 自检：注入缺失/重复锚点必须拒绝，单项结果不能冒充十六项通过。"""

import json
import pathlib
import re
import subprocess
import sys
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[2]
HELPER = ROOT / "scripts/rust-tauri/r05_t08_mutate_pin.py"
TARGET = "svc:r05_t01_model_plane"


def main() -> None:
    authoritative = (ROOT / "docs/rust-tauri/R05/r05_stage_pins.tsv").read_text()
    target_rows = [row for row in authoritative.splitlines(keepends=True)
                   if row.split()[:2] == ["pin", TARGET]]
    assert len(target_rows) == 1, "自检前置：权威目标必须唯一"
    row = target_rows[0]
    old = int(row.split()[2])
    count = list(re.finditer(r"\S+", row))[2]
    def counted(value: str) -> str:
        return row[:count.start()] + value + row[count.end():]
    alternate = counted(str(old + 5))
    cases = [("current-count", authoritative, 0),
             ("different-count", authoritative.replace(row, alternate), 0),
             ("missing-anchor", authoritative.replace(row, ""), 1),
             ("duplicate-anchor", authoritative + row, 1),
             ("conflicting-duplicate", authoritative + alternate, 1),
             ("malformed-count", authoritative.replace(row, counted("bad")), 1),
             ("minimum-count", authoritative.replace(row, counted("1")), 1)]
    results = []
    with tempfile.TemporaryDirectory(prefix="r05-n03-selfcheck-") as temporary:
        root = pathlib.Path(temporary)
        for name, content, expected in cases:
            table = root / (name + ".tsv")
            receipt = root / (name + ".json")
            table.write_text(content)
            run = subprocess.run([sys.executable, str(HELPER), str(table), str(receipt)],
                                 capture_output=True, text=True, check=False)
            assert run.returncode == expected, (name, run.stdout, run.stderr)
            if expected:
                assert table.read_text() == content, name
                assert not receipt.exists(), name
                assert TARGET in run.stderr, name
                if "anchor" in name or "duplicate" in name:
                    assert "expected exactly one anchor" in run.stderr, name
            else:
                record = json.loads(receipt.read_text())
                count = old + 5 if name == "different-count" else old
                assert record["old"] == count and record["new"] == count - 1, name
                assert record["matches"] == record["mutations"] == 1, name
                before = content.splitlines()
                after = table.read_text().splitlines()
                assert len(before) == len(after) and sum(a != b for a, b in zip(before, after)) == 1, name
            results.append({"check": name, "exitCode": run.returncode,
                            "expectedExitCode": expected, "stderr": run.stderr.strip()})

        # 使用主脚本中的原函数验证汇总拒绝空集、错项和重复项。
        source = (ROOT / "scripts/rust-tauri/r05_t08_negative_gate.sh").read_text()
        summary = source.split("write_results() {\n", 1)[1].split("\n}\n\n# ── controls:", 1)[0]
        ev = root / "summary"
        ev.mkdir()
        for control in ("control-xtask", "control-binwiring", "n03-restored"):
            (ev / control).mkdir()
            (ev / control / "exit-code.txt").write_text("0\n")
        good = "R05-GATE-N03\t101\tOK\tOK\tfixture\n"
        full = "".join(good.replace("N03", f"N{i:02d}") for i in range(1, 17))
        fixtures = [("selected-one", "N03", good, 0),
                    ("selected-empty", "N03", "", 1),
                    ("selected-wrong-case", "N03", good.replace("N03", "N01"), 1),
                    ("selected-duplicate-case", "N03", good * 2, 1),
                    ("selected-bad-verdict", "N03", good.replace("\tOK\tfixture", "\tBAD\tfixture"), 1),
                    ("full-missing-cases", "ALL", good, 1),
                    ("full-sixteen", "ALL", full, 0),
                    ("full-duplicate", "ALL", full + good, 1)]
        for name, scope, tsv, expected in fixtures:
            (ev / "case-results.tsv").write_text(tsv)
            run = subprocess.run(["bash", "-c", 'EV="$1"; CASE_SCOPE="$2"; COPY="$3"; fail() { exit 1; };\n' + summary,
                                  "summary-check", str(ev), scope, str(root)],
                                 capture_output=True, text=True, check=False)
            assert run.returncode == expected, (name, run.stdout, run.stderr)
            doc = json.loads((ev / "case-results.json").read_text())
            assert doc["allRefused"] == (expected == 0), name
            if name == "selected-one":
                assert len(doc["cases"]) == 1 and len(doc["unexecutedCases"]) == 15
            results.append({"check": name, "exitCode": run.returncode,
                            "expectedExitCode": expected})
    print(json.dumps({"findingId": "F45", "checks": results, "passed": len(results)},
                     indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()

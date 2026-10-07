write_results() {
# ── verdict ─────────────────────────────────────────────────────────────────
python3 - "$EV" "$CASE_SCOPE" "$COPY" <<'PY' || fail "at least one R05 negative case did NOT fail closed with the gap named"
import json, pathlib, sys
ev = pathlib.Path(sys.argv[1])
scope = sys.argv[2]
expected = ["R05-GATE-N03"] if scope == "N03" else [f"R05-GATE-N{i:02d}" for i in range(1, 17)]
rows = []
for line in (ev / "case-results.tsv").read_text().splitlines():
    if not line.strip():
        continue
    name, exit_code, named, verdict, note = line.split("\t")
    rows.append({"case": name, "exitCode": int(exit_code), "gapNamed": named,
                 "verdict": verdict, "note": note})
controls = [{"name": "xtask-r05-mirror", "exitCode": int((ev / "control-xtask" / "exit-code.txt").read_text()), "expected": 0},
            {"name": "n03-restored-mirror", "exitCode": int((ev / "n03-restored" / "exit-code.txt").read_text()), "expected": 0}]
if scope == "ALL":
    controls.append({"name": "binary-wiring-suite", "exitCode": int((ev / "control-binwiring" / "exit-code.txt").read_text()), "expected": 0})
doc = {
    "schema": "lingxi.r05-negative-gate.v1",
    "isolatedCopy": "local git clone (read-only on the source) + uncommitted working-tree overlay under $HOME; the main working tree carries ZERO injections (reset_copy restores every mutated file from the pristine snapshot)",
    "scope": scope,
    "isolatedCopyPath": sys.argv[3],
    "expectedCases": expected,
    "unexecutedCases": [f"R05-GATE-N{i:02d}" for i in range(1, 17) if f"R05-GATE-N{i:02d}" not in expected],
    "controls": controls,
    "cases": rows,
    "allRefused": [r["case"] for r in rows] == expected and all(r["verdict"] == "OK" for r in rows),
    "controlsGreen": all(c["exitCode"] == c["expected"] for c in controls),
}
(ev / "case-results.json").write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n")
print(json.dumps({"cases": len(rows), "allRefused": doc["allRefused"], "controlsGreen": doc["controlsGreen"]}))
if not (doc["allRefused"] and doc["controlsGreen"]):
    raise SystemExit(1)
PY
}

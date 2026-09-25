#!/usr/bin/env python3
"""R01-T08 阶段关卡检查器（R01-A15：高风险功能不被演示遮蔽）。

对五个高风险能力域（browser_host / pdf_renderer / shell_capabilities /
storage_cutover / protocol_chain）逐项核验：

  G1 必需能力必须 status=VERIFIED（实施平台实测），且证据文件存在、
     SHA-256 与钉住值一致——「截图通过但用户接管失败」这类输入会使
     browser_host 域判 NOT_COMPLETE，绝不标完整通过。
  G2 任何 FAILED/UNVERIFIED 的必需能力 → 该域 NOT_COMPLETE。
  G3 证据缺失或哈希不符 → 该域 BLOCKED（证据不可信，不猜测）。
  G4 每个递延项（UNVERIFIED/OPEN_FINDING/REGISTERED_DEFECT/DOCUMENTED）
     必须在 RISK_REGISTER.json 有对应条目且带 resolve_by_stage 与
     failure_handling；缺失 → BLOCKED（未跟踪的未验证项不得放行）。

总体判定：
  所有域 COMPLETE 且无递延项 → PASS
  所有域 COMPLETE 但有递延项（均已挂账） → PASS_WITH_CONDITIONS
  任一域 NOT_COMPLETE/BLOCKED → NO-GO，明确阻塞替壳路径（shell replacement）

用法：
  python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py [--out REPORT.json]
  python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --self-test
  python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --inputs OTHER.json --register OTHER.json

退出码：0 = PASS/PASS_WITH_CONDITIONS；1 = NO-GO；self-test 0 = 全部负向按预期拒绝。
"""
import argparse
import copy
import hashlib
import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
DEFAULT_INPUTS = REPO / "docs/rust-tauri/R01/r01_t08_gate_inputs.json"
DEFAULT_REGISTER = REPO / "docs/rust-tauri/R01/RISK_REGISTER.json"
DEFERRED_STATUSES = {"UNVERIFIED", "OPEN_FINDING", "REGISTERED_DEFECT", "DOCUMENTED"}


def _load(p):
    with open(p, "r", encoding="utf-8") as f:
        return json.load(f)


def _sha256(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def evaluate(inputs, register, repo=REPO):
    """返回 (verdict, report)。verdict ∈ PASS / PASS_WITH_CONDITIONS / NO-GO。"""
    risk_by_id = {r["id"]: r for r in register.get("risks", [])}
    areas_report = {}
    blocking = []
    deferred_total = 0

    for area_name, area in inputs.get("areas", {}).items():
        area_status = "COMPLETE"
        area_problems = []
        caps_report = []
        for cap in area.get("required_capabilities", []):
            cap_id = cap["id"]
            st = cap.get("status")
            entry = {"id": cap_id, "status": st}
            if st != "VERIFIED":
                area_status = "NOT_COMPLETE"
                area_problems.append(
                    f"必需能力 {cap_id} 状态={st}（非 VERIFIED）——不得以演示/截图证据遮蔽")
                entry["result"] = "FAIL"
            else:
                ev = cap.get("evidence")
                want = cap.get("sha256")
                p = repo / ev if ev else None
                if not ev or not p.is_file():
                    area_status = "BLOCKED"
                    area_problems.append(f"必需能力 {cap_id} 证据缺失: {ev}")
                    entry["result"] = "BLOCKED(evidence-missing)"
                elif want and _sha256(p) != want:
                    area_status = "BLOCKED"
                    area_problems.append(f"必需能力 {cap_id} 证据哈希不符: {ev}")
                    entry["result"] = "BLOCKED(evidence-hash-mismatch)"
                else:
                    entry["result"] = "PASS"
            caps_report.append(entry)

        for cap in area.get("deferred_capabilities", []):
            deferred_total += 1
            cap_id = cap["id"]
            rid = cap.get("risk_id")
            entry = {"id": cap_id, "status": cap.get("status"), "risk_id": rid}
            r = risk_by_id.get(rid)
            if not r:
                area_status = "BLOCKED"
                area_problems.append(f"递延项 {cap_id} 引用风险条目 {rid} 不存在")
                entry["result"] = "BLOCKED(risk-missing)"
            elif not str(r.get("resolve_by_stage", "")).strip() or not str(r.get("failure_handling", "")).strip():
                area_status = "BLOCKED"
                area_problems.append(f"递延项 {cap_id} 风险条目 {rid} 缺截止阶段或失败处理")
                entry["result"] = "BLOCKED(risk-incomplete)"
            else:
                entry["result"] = f"TRACKED({rid} -> {r['resolve_by_stage']})"
            caps_report.append(entry)

        areas_report[area_name] = {
            "verdict": area_status,
            "capabilities": caps_report,
            "problems": area_problems,
            "selected_candidate": area.get("selected_candidate"),
            "adr": area.get("adr"),
        }
        if area_status != "COMPLETE":
            blocking.append(f"{area_name}: {area_status} — {'; '.join(area_problems)}")

    if blocking:
        verdict = "NO-GO"
        shell_replacement = "BLOCKED: 存在未真实验证的高风险必需能力/不可信证据，替壳路径明确阻塞"
    elif deferred_total:
        verdict = "PASS_WITH_CONDITIONS"
        shell_replacement = ("ALLOWED_FOR_NEXT_STAGE_ONLY: 实施平台高风险必需能力均有真实原型证据；"
                             f"{deferred_total} 个递延项已全部挂账（截止阶段+失败处理），跨平台/授权态放行挂 R09/R10 强制关卡")
    else:
        verdict = "PASS"
        shell_replacement = "ALLOWED_FOR_NEXT_STAGE_ONLY"

    report = {
        "verdict": verdict,
        "shell_replacement_path": shell_replacement,
        "implementation_platform": inputs.get("implementation_platform"),
        "areas": areas_report,
        "deferred_count": deferred_total,
        "blocking_reasons": blocking,
    }
    return verdict, report


def self_test():
    inputs_real = _load(DEFAULT_INPUTS)
    register = _load(DEFAULT_REGISTER)
    results = []

    v, r = evaluate(copy.deepcopy(inputs_real), register)
    results.append(("positive-control",
                    v in ("PASS", "PASS_WITH_CONDITIONS") and r["areas"]["browser_host"]["verdict"] == "COMPLETE",
                    f"真实 R01 数据 verdict={v}（browser_host={r['areas']['browser_host']['verdict']}，deferred={r['deferred_count']}）"))

    # N1（A15 负向核心）：截图通过但用户接管失败
    m = copy.deepcopy(inputs_real)
    for cap in m["areas"]["browser_host"]["required_capabilities"]:
        if cap["id"] == "user_takeover":
            cap["status"] = "FAILED"
    v, r = evaluate(m, register)
    bh = r["areas"]["browser_host"]
    ok = (v == "NO-GO" and bh["verdict"] == "NOT_COMPLETE"
          and any("user_takeover" in p for p in bh["problems"])
          and "BLOCKED" in r["shell_replacement_path"])
    results.append(("N1-screenshot-pass-takeover-fail", ok,
                    f"verdict={v} browser_host={bh['verdict']} shell_path={r['shell_replacement_path'][:60]}…"))

    # N2：证据哈希被篡改 → BLOCKED
    m = copy.deepcopy(inputs_real)
    m["areas"]["pdf_renderer"]["required_capabilities"][0]["sha256"] = "0" * 64
    v, r = evaluate(m, register)
    ok = v == "NO-GO" and r["areas"]["pdf_renderer"]["verdict"] == "BLOCKED"
    results.append(("N2-evidence-hash-tampered", ok,
                    f"verdict={v} pdf_renderer={r['areas']['pdf_renderer']['verdict']}"))

    # N3：递延项未挂账（风险条目不存在）→ BLOCKED
    m = copy.deepcopy(inputs_real)
    m["areas"]["shell_capabilities"]["deferred_capabilities"][0]["risk_id"] = "RR-NOPE"
    v, r = evaluate(m, register)
    ok = v == "NO-GO" and r["areas"]["shell_capabilities"]["verdict"] == "BLOCKED"
    results.append(("N3-untracked-deferred", ok,
                    f"verdict={v} shell_capabilities={r['areas']['shell_capabilities']['verdict']}"))

    # N4：必需能力标 UNVERIFIED（演示遮蔽形态）→ NOT_COMPLETE + NO-GO
    m = copy.deepcopy(inputs_real)
    m["areas"]["storage_cutover"]["required_capabilities"][0]["status"] = "UNVERIFIED"
    v, r = evaluate(m, register)
    ok = v == "NO-GO" and r["areas"]["storage_cutover"]["verdict"] == "NOT_COMPLETE"
    results.append(("N4-required-unverified", ok,
                    f"verdict={v} storage_cutover={r['areas']['storage_cutover']['verdict']}"))

    all_ok = all(x[1] for x in results)
    for name, passed, detail in results:
        tag = "PASS" if name == "positive-control" else "PASS-NEG"
        print(f"{tag if passed else 'FAIL'} {name}: {detail}")
    print(f"SELF-TEST {'OK' if all_ok else 'FAILED'} ({sum(1 for x in results if x[1])}/{len(results)})")
    return 0 if all_ok else 1


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--inputs", default=str(DEFAULT_INPUTS))
    ap.add_argument("--register", default=str(DEFAULT_REGISTER))
    ap.add_argument("--out", default=None)
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()

    if args.self_test:
        sys.exit(self_test())

    verdict, report = evaluate(_load(args.inputs), _load(args.register))
    report["inputs_file"] = args.inputs
    report["register_file"] = args.register
    if args.out:
        Path(args.out).write_text(json.dumps(report, ensure_ascii=False, indent=1, sort_keys=True) + "\n", encoding="utf-8")
    for name, a in report["areas"].items():
        print(f"[{a['verdict']:>12}] {name}")
        for c in a["capabilities"]:
            print(f"    {c['id']}: {c['status']} -> {c['result']}")
        for p in a["problems"]:
            print(f"    PROBLEM: {p}")
    print(f"VERDICT: {verdict}")
    print(f"SHELL-REPLACEMENT: {report['shell_replacement_path']}")
    sys.exit(0 if verdict in ("PASS", "PASS_WITH_CONDITIONS") else 1)


if __name__ == "__main__":
    main()

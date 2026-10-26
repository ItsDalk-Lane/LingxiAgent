#!/usr/bin/env python3
"""独立复核脚本（只读）：R08 126 叶逐断言 case 覆盖与真值。

不导入 scripts/rust-tauri/r08_stage_inputs.py，不信任门收据派生逻辑；
直接从 R00 冻结账本、stage map、门-08 收据、suite 原始案例文件交叉核对。
输出 JSON 供报告引用。
"""
import json
import hashlib
import sys
from pathlib import Path

ROOT = Path("/Users/study_superior/Desktop/Code/LingxiAgent")
GATE = ROOT / "artifacts/rust-tauri/R08/verify-r08-closeout-08"
RESULT = GATE / "verify-stage-result.json"
STAGEMAP = ROOT / "rust/crates/xtask/src/stage_maps/R08.json"
CHECKLIST = ROOT / "docs/rust-tauri/R08/CLOSEOUT_CHECKLIST.json"
FSA = ROOT / "docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json"
AM = ROOT / "docs/rust-tauri/R00/ACCEPTANCE_MAP.json"

findings = []   # (severity, code, detail)
stats = {}

def F(code, detail):
    findings.append((code, detail))

def canon(obj):
    return json.dumps(obj, ensure_ascii=False, sort_keys=True, separators=(",", ":"))

gate = json.loads(RESULT.read_text(encoding="utf-8"))
smap = json.loads(STAGEMAP.read_text(encoding="utf-8"))
checklist = json.loads(CHECKLIST.read_text(encoding="utf-8"))
fsa_doc = json.loads(FSA.read_text(encoding="utf-8"))
am_doc = json.loads(AM.read_text(encoding="utf-8"))

fsa = {row["id"]: row for row in fsa_doc["supplemental_scenarios"]}
am = am_doc["scenarios"]
am_r08 = {k: v for k, v in am.items()
          if v.get("kind") == "supplemental" and "R08" in (v.get("execution_stage_ids") or [])}
fsa_r08 = {k: v for k, v in fsa.items() if "R08" in (v.get("execution_stage_ids") or [])}
gate_leaves = {row["id"]: row for row in gate["supplementalLeafScenarios"]}
map_leaves = {row["id"]: row for row in smap["supplementalLeafScenarios"]}
check_items = {row["id"]: row for row in checklist["items"]}

# ---- 0. 集合一致 ----
stats["leaf_counts"] = {
    "R00_ACCEPTANCE_MAP": len(am_r08),
    "R00_FEATURE_STAGE_ACCEPTANCE": len(fsa_r08),
    "stage_map": len(map_leaves),
    "gate_result": len(gate_leaves),
    "checklist": len(check_items),
}
if len(am_r08) != 126: F("LEAF_SET", f"R00 ACCEPTANCE_MAP R08 supplemental = {len(am_r08)}")
if len(fsa_r08) != 126: F("LEAF_SET", f"R00 FSA R08 supplemental = {len(fsa_r08)}")
for name, s in [("stage_map", set(map_leaves)), ("gate", set(gate_leaves)), ("checklist", set(check_items))]:
    if s != set(fsa_r08):
        F("LEAF_SET", f"{name} leaf set != R00 FSA: missing={sorted(set(fsa_r08)-s)[:5]} extra={sorted(s-set(fsa_r08))[:5]}")

# ---- 1. 逐叶：原断言文本/数量与 R00 两本账一致；断言组覆盖 ----
total_groups = 0
total_case_instances = 0
all_pinned = {}   # case -> leaf list
producer_counts = {}
for lid in sorted(fsa_r08):
    ref = fsa_r08[lid]
    amentry = am_r08[lid]
    assertions = ref["assertions"]
    total_groups += len(assertions)
    # gate r00Mirror
    g = gate_leaves.get(lid)
    if g is None:
        continue
    mirror = g["r00Mirror"]
    if mirror.get("assertions") != assertions:
        F("R00_ASSERTIONS", f"{lid}: gate r00Mirror.assertions != R00 FSA")
    if mirror.get("then") != ref.get("then"):
        F("R00_THEN", f"{lid}: gate r00Mirror.then != R00 FSA")
    if mirror.get("due") != ref.get("due"):
        F("R00_DUE", f"{lid}: gate r00Mirror.due != R00 FSA")
    if sorted(mirror.get("taskIds") or []) != sorted(ref.get("task_ids") or []):
        F("R00_TASKIDS", f"{lid}: gate r00Mirror.taskIds != R00 FSA")
    if amentry.get("then") != ref.get("then"):
        F("R00_THEN", f"{lid}: R00 ACCEPTANCE_MAP then != FSA then (ledger mismatch)")
    # stage map original mirror
    m = map_leaves.get(lid)
    if m is None:
        F("MAP_LEAF", f"{lid}: missing from stage map")
    else:
        if m.get("r00Assertions") != assertions:
            F("R00_ASSERTIONS", f"{lid}: stage map r00Assertions != R00 FSA")
        if m.get("r00Then") != ref.get("then"):
            F("R00_THEN", f"{lid}: stage map r00Then != R00 FSA")
        if m.get("r00Due") != ref.get("due"):
            F("R00_DUE", f"{lid}: stage map r00Due != R00 FSA")
        if m.get("basisKind") != "full_original_behavior":
            F("BASIS", f"{lid}: stage map basisKind={m.get('basisKind')}")
        # 断言合同 = flatten(originalAssertionCases)
        groups = m.get("originalAssertionCases")
        if not groups or len(groups) != len(assertions):
            F("COVERAGE", f"{lid}: stage map originalAssertionCases len={len(groups) if groups else 0} != assertions {len(assertions)}")
        else:
            flat = [c for grp in groups for c in grp]
            contract_names = [c["case"] for c in m["assertionContract"]["cases"]]
            if len(flat) != len(contract_names):
                F("COVERAGE", f"{lid}: flatten(originalAssertionCases)={len(flat)} != contract cases={len(contract_names)}")
            if sorted(flat) != sorted(contract_names):
                F("COVERAGE", f"{lid}: flatten set != contract set")
            if len(set(flat)) != len(flat):
                F("REUSE", f"{lid}: case reused within leaf: {[c for c in flat if flat.count(c)>1][:5]}")
            for i, grp in enumerate(groups):
                if not grp:
                    F("COVERAGE", f"{lid}: assertion #{i} has no case")
                for c in grp:
                    all_pinned.setdefault(c, set()).add(lid)
            if any(not grp for grp in groups):
                F("COVERAGE", f"{lid}: empty assertion group")
            total_case_instances += len(flat)
    # checklist
    it = check_items.get(lid)
    if it is None:
        continue
    if it["original"].get("assertions") != assertions:
        F("CHECKLIST_ASSERTIONS", f"{lid}: checklist original.assertions != R00 FSA")
    if it["original"].get("then") != ref.get("then"):
        F("CHECKLIST_THEN", f"{lid}: checklist original.then != R00 FSA")
    if sorted(it["original"].get("taskIds") or []) != sorted(ref.get("task_ids") or []):
        F("CHECKLIST_TASKIDS", f"{lid}: checklist taskIds != R00 FSA")
    ac = it.get("assertionCases")
    if not ac or len(ac) != len(assertions):
        F("CHECKLIST_COVERAGE", f"{lid}: checklist assertionCases len != assertions")
    else:
        if any(not grp for grp in ac):
            F("CHECKLIST_COVERAGE", f"{lid}: checklist empty assertion group")
        flat = [c for grp in ac for c in grp]
        if len(set(flat)) != len(flat):
            F("CHECKLIST_REUSE", f"{lid}: checklist case reused within leaf")
        if sorted(flat) != sorted(it.get("cases") or []):
            F("CHECKLIST_CASES", f"{lid}: assertionCases flatten != cases list")
        if it.get("status") != "PASS":
            F("CHECKLIST_STATUS", f"{lid}: status={it.get('status')}")
        if it.get("basisKind") != "full_original_behavior":
            F("CHECKLIST_BASIS", f"{lid}: basisKind={it.get('basisKind')}")
    # gate
    if g["status"] != "PASS":
        F("GATE_STATUS", f"{lid}: gate status={g['status']} reason={g.get('reason')}")
    if g["basisKind"] != "full_original_behavior":
        F("GATE_BASIS", f"{lid}: gate basisKind={g['basisKind']}")
    gac = g.get("originalAssertionCases")
    if not gac or len(gac) != len(assertions):
        F("GATE_COVERAGE", f"{lid}: gate originalAssertionCases len != assertions")
    else:
        # gate 序列化形状为 [{cases,r00Assertion,r00AssertionIndex}, ...]
        ok_shape = all(isinstance(e, dict) for e in gac)
        if ok_shape:
            for i, e in enumerate(gac):
                if e.get("r00Assertion") != assertions[i]:
                    F("GATE_COVERAGE", f"{lid}: gate assertion #{i} r00Assertion text != R00 FSA")
                if e.get("r00AssertionIndex") != i:
                    F("GATE_COVERAGE", f"{lid}: gate assertion #{i} index={e.get('r00AssertionIndex')}")
                if it is not None and e.get("cases") != (it.get("assertionCases") or [None]*len(assertions))[i]:
                    F("GATE_COVERAGE", f"{lid}: gate assertion #{i} cases != checklist")
        elif it is not None and gac != it.get("assertionCases"):
            F("GATE_COVERAGE", f"{lid}: gate originalAssertionCases != checklist assertionCases")
    gr = {r["case"]: r for r in g["assertionResults"]}
    if len(gr) != len(g["assertionResults"]):
        F("GATE_RESULTS", f"{lid}: duplicate case names in gate assertionResults")
    contract_names = [c["case"] for c in (g.get("assertionContract") or {}).get("cases", [])]
    if sorted(gr) != sorted(contract_names):
        F("GATE_CONTRACT", f"{lid}: gate assertionResults set != contract cases")
    for c, r in gr.items():
        if r.get("actual") != 1 or r.get("expect") != 1 or r.get("status") != "PASS" or r.get("recordedOk") is not True:
            F("GATE_CASE", f"{lid}: gate case {c} actual={r.get('actual')} expect={r.get('expect')} status={r.get('status')} recordedOk={r.get('recordedOk')}")
    # producer
    producers = g.get("evidenceCommandRefs") or []
    if len(producers) != 1:
        F("PRODUCER", f"{lid}: evidenceCommandRefs={producers}")
    else:
        producer_counts[producers[0]] = producer_counts.get(producers[0], 0) + 1
    if g.get("evidencePaths") and not any((GATE / p.replace("{EVIDENCE}/", "")).exists() for p in g["evidencePaths"]):
        F("EVIDENCE_PATH", f"{lid}: evidence path missing {g['evidencePaths']}")

stats["total_original_assertions"] = total_groups
stats["total_case_instances"] = total_case_instances
stats["distinct_pinned_cases"] = len(all_pinned)
stats["producer_leaf_counts"] = producer_counts
stats["cases_shared_across_leaves"] = sum(1 for c, ls in all_pinned.items() if len(ls) > 1)

# ---- 2. 原始证据文件核对（逐 case 真值；整份记录全真） ----
evidence_files = {
    "r08_t07_ab": GATE / "SUITES/r08_t07_ab/cases.json",
    "r08_ui_pages": GATE / "SUITES/r08_ui_pages/ui-cases.json",
    "r08_cli_data_vitest": GATE / "SUITES/r08_cli_data_vitest/cli-cases.json",
}
evidence_cases = {}
for suite, path in evidence_files.items():
    doc = json.loads(path.read_text(encoding="utf-8"))
    if doc.get("schema") != "lingxi.leaf-case-results.v1":
        F("EVIDENCE_SCHEMA", f"{suite}: schema={doc.get('schema')}")
    rows = doc["cases"]
    names = [r["case"] for r in rows]
    if len(set(names)) != len(names):
        F("EVIDENCE_DUP", f"{suite}: duplicate case names")
    bad = [r for r in rows if r.get("ok") is not True or r.get("actual") != 1 or r.get("expect") != 1]
    if bad:
        F("EVIDENCE_BAD", f"{suite}: {len(bad)} cases not true, e.g. {bad[:3]}")
    evidence_cases[suite] = {r["case"]: r for r in rows}
    stats.setdefault("evidence_file_cases", {})[suite] = len(rows)

# per leaf verify pinned case truth from raw producer file
for lid in sorted(gate_leaves):
    g = gate_leaves[lid]
    producer = (g.get("evidenceCommandRefs") or [None])[0]
    if producer not in evidence_cases:
        F("PRODUCER", f"{lid}: unknown producer {producer}")
        continue
    for r in g["assertionResults"]:
        c = r["case"]
        if c not in evidence_cases[producer]:
            F("CASE_MISSING", f"{lid}: case {c} absent from {producer} raw evidence file")
        elif evidence_cases[producer][c].get("ok") is not True:
            F("CASE_FALSE", f"{lid}: case {c} raw evidence not ok")
    # pinned set vs non-referenced cases in file (whole-file trust: gate requires whole file true, checked above)
# ensure all raw cases are referenced by some leaf (no stray)
for suite, cases in evidence_cases.items():
    referenced = {c for g in gate_leaves.values() if (g.get("evidenceCommandRefs") or [None])[0] == suite
                  for r in g["assertionResults"] for c in [r["case"]]}
    stray = set(cases) - referenced
    if stray:
        F("EVIDENCE_STRAY", f"{suite}: {len(stray)} cases not referenced by any leaf: {sorted(stray)[:5]}")
    used = referenced - set(cases)
    if used:
        F("EVIDENCE_USED_MISSING", f"{suite}: referenced cases missing: {sorted(used)[:5]}")

# ---- 3. cargo 旁证 case：find.bridge-redaction ----
side_log = GATE / "SUITES/r08_c3_find_bridge_redaction/output.log"
side_ok = False
if side_log.exists():
    side_ok = any(line.strip() == "test c3_find_redacts_bridge_time_tags_only_for_bridge_bound_sessions ... ok"
                  for line in side_log.read_text(encoding="utf-8", errors="replace").splitlines())
if not side_ok:
    F("CARGO_SIDE", "find.bridge-redaction cargo 旁证日志缺 test ... ok")
stats["cargo_side_case_ok"] = side_ok
# side suite receipt must have PASS
side_receipt = json.loads((GATE / "SUITES/r08_c3_find_bridge_redaction/result.json").read_text())
if side_receipt.get("exitCode") != 0:
    F("CARGO_SIDE", f"c3 suite exit={side_receipt.get('exitCode')}")

# ---- 4. 门-08 收据自身 ----
stats["gate"] = {
    "overall": gate["overall"],
    "commands": len(gate["commands"]),
    "commandsPass": sum(1 for c in gate["commands"] if c["status"] == "PASS"),
    "scenarios": len(gate["scenarios"]),
    "scenariosPass": sum(1 for s in gate["scenarios"] if s["status"] == "PASS"),
    "testedSha": gate["testedSha"],
    "worktreeDirty": gate["worktreeDirty"],
    "toolchainChannel": gate["toolchainChannel"],
}
if gate["overall"] != "PASS": F("GATE", f"overall={gate['overall']}")
for c in gate["commands"]:
    if c["status"] != "PASS" or c.get("exitCode") != 0:
        F("GATE_COMMAND", f"{c['key']} status={c['status']} exit={c.get('exitCode')}")
    if c.get("missingEvidence"): F("GATE_EVIDENCE", f"{c['key']} missingEvidence={c['missingEvidence']}")
    if c.get("timedOut"): F("GATE_TIMEOUT", c["key"])
for s in gate["scenarios"]:
    if s["status"] != "PASS": F("GATE_SCENARIO", f"{s['id']} status={s['status']}")
cb = gate["candidateSourceBinding"]
if not cb.get("stable"): F("CANDIDATE_STABLE", "candidateSourceBinding.stable false")
if cb["before"]["digestSha256"] != cb["after"]["digestSha256"]:
    F("CANDIDATE_STABLE", "before/after digest differ")
if cb["before"]["fileCount"] != cb["after"]["fileCount"]:
    F("CANDIDATE_STABLE", "before/after fileCount differ")
for cp in cb["checkpointAfterEveryCommand"]:
    if not cp.get("stable") or cp.get("digestSha256") != cb["before"]["digestSha256"] or cp.get("changedPathBytesHex"):
        F("CANDIDATE_STABLE", f"checkpoint {cp.get('commandKey')} unstable/changed")
if cb.get("testedShaAtEnd") != gate["testedSha"]:
    F("CANDIDATE_STABLE", "testedShaAtEnd != testedSha")
rs = gate["runnerSourceBinding"]
if rs.get("status") != "PASS": F("RUNNER_BINDING", f"runner status={rs.get('status')}")
if rs["before"].get("digestSha256") != rs["after"].get("digestSha256"):
    F("RUNNER_BINDING", "runner before/after digest differ")
stats["candidate_stable"] = cb["stable"]
stats["before_after_filecount"] = [cb["before"]["fileCount"], cb["after"]["fileCount"]]
stats["before_after_digest"] = [cb["before"]["digestSha256"], cb["after"]["digestSha256"]]
stats["runner_binding"] = rs["status"]

cov = gate["supplementalLeafCoverage"]
if cov.get("pass") != 126 or cov.get("blocked") != 0 or cov.get("fail") != 0 or cov.get("declaredInStageMap") != 126:
    F("GATE_COVERAGE_SUMMARY", json.dumps(cov, ensure_ascii=False)[:300])
stats["leaf_coverage"] = {k: cov.get(k) for k in ("pass", "blocked", "fail", "declaredInStageMap", "deferredToLaterStage", "deferredToR07")}

# ---- 5. 文档/清单数字与 gate-08 一致性 ----
summary = checklist["summary"]
gs = checklist["gateSummary"]
if summary.get("PASS") != 126 or len(summary) != 1: F("DOC", f"checklist summary={summary}")
if gs.get("overall") != "PASS" or gs.get("commandsPass") != 22 or gs.get("commandsTotal") != 22 \
        or gs.get("scenariosPass") != 16 or gs.get("scenariosTotal") != 16 or gs.get("candidateStable") is not True \
        or gs.get("testedSha") != gate["testedSha"]:
    F("DOC", f"checklist gateSummary mismatch gate-08: {gs}")

handoff = json.loads((ROOT / "docs/rust-tauri/R08/R08_HANDOFF.json").read_text(encoding="utf-8"))
so = handoff.get("stageOutcome", "")
for token in ["22/22", "16/16", "126 PASS", "0 BLOCKED", "0 FAIL", "stable=true", "overall=PASS", "71 tasks", "68 parity", "3 recorded", "0 differs"]:
    if token not in so:
        F("DOC", f"R08_HANDOFF.stageOutcome missing {token!r}")
if handoff.get("gitHead") != gate["testedSha"]:
    F("DOC", "R08_HANDOFF.gitHead != gate testedSha")
lp = handoff["closeoutRound"]["leavesPassByDomain"]
if sum(lp.values()) != 126: F("DOC", f"handoff leavesPassByDomain sum={sum(lp.values())}")
if handoff["closeoutRound"].get("leavesPendingByDomain") != {}: F("DOC", "handoff pending not empty")
if not any("21/22" in r and "closeout-07" in r for r in handoff["closeoutRound"]["gateReceipts"]):
    F("DOC", "handoff lacks closeout-07 21/22 red receipt")
if not any("126 PASS" in r and "closeout-08" in r and "overall PASS" in r for r in handoff["closeoutRound"]["gateReceipts"]):
    F("DOC", "handoff lacks closeout-08 PASS receipt")
if handoff.get("openItems", {}).get("pendingLeaves") != 0:
    F("DOC", "handoff openItems.pendingLeaves != 0")
stats["handoff_domain_counts"] = lp

# checklist items evidence.gateResult must point to gate-08
for it in checklist["items"]:
    gr = it["evidence"].get("gateResult", "")
    if "verify-r08-closeout-08" not in gr:
        F("DOC", f"checklist {it['id']} gateResult={gr}")
    if it["evidence"].get("gateStatus") != "PASS":
        F("DOC", f"checklist {it['id']} gateStatus!=PASS")

# ---- 6. stage map 顶层结构 ----
stats["stage_map"] = {
    "commands": len(smap["commands"]),
    "scenarios": len(smap["scenarios"]),
    "leaves": len(smap["supplementalLeafScenarios"]),
    "inputDigestSha256": smap.get("inputDigestSha256"),
}
if len(smap["commands"]) != 22: F("STAGEMAP", f"commands={len(smap['commands'])}")
if len(smap["scenarios"]) != 16: F("STAGEMAP", f"scenarios={len(smap['scenarios'])}")
# 每个场景 must be REQUIRED; leaf producer refs registered & referenced
reg = set(smap["commands"])
refs = set()
for s in smap["scenarios"]:
    refs |= set(s["commandRefs"])
    if s.get("requirement") != "REQUIRED": F("STAGEMAP", f"scenario {s['id']} requirement={s.get('requirement')}")
for l in smap["supplementalLeafScenarios"]:
    refs |= set(l.get("evidenceCommandRefs") or [])
if reg - refs: F("STAGEMAP", f"registered-but-unreferenced commands: {sorted(reg-refs)}")
if refs - reg: F("STAGEMAP", f"referenced-but-unregistered commands: {sorted(refs-reg)}")
# A13/A14 assertion contracts pinned counts
for s in smap["scenarios"]:
    if s["id"] in ("R08-A13", "R08-A14"):
        stats.setdefault("scenario_contracts", {})[s["id"]] = len((s.get("assertionContract") or {}).get("cases", []))
# supplementary leaf scenario per domain via feature prefix (rough)
stats["checklist_summary"] = summary

out = {"stats": stats, "findings": findings}
print(json.dumps(out, ensure_ascii=False, indent=1))
print(f"\nFINDINGS={len(findings)}", file=sys.stderr)
Path("/private/tmp/review-independent/coverage-result.json").write_text(
    json.dumps(out, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")

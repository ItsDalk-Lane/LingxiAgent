#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""RR3 E-04 文档自检：
1) 7 份 JSON 严格解析 + 重复键拒绝；
2) 5 份 owned Markdown 相对链接解析存在；
3) 七处 rr3_current 跨文档相等 + 六元组一致；
4) 历史保留（E-03 快照入史、旧 FAIL 文本仍在、两份未改文档字节相等）；
5) 矩阵-进度-报告-HANDOFF 结论一致；
6) 前后受保护输入相等（树快照对比，仅允许 12 份 owned 文档差异）。
输出 check-results.json；exit 0=全过，非 0=有失败断言。
"""
import hashlib
import json
import os
import re
import sys

REPO = "/Users/study_superior/Desktop/Code/LingxiAgent"
E04DIR = os.path.join(REPO, "artifacts/rust-tauri/R05/RR3/E-04")
JSONS = [
    "docs/rust-tauri/R05/R05_HANDOFF.json",
    "docs/rust-tauri/R05/PROGRESS_LEDGER.json",
    "docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json",
    "docs/rust-tauri/R05/R05_TEST_MAP.json",
    "docs/rust-tauri/R05/R05_PERFORMANCE_RESULTS.json",
    "docs/rust-tauri/R05/R05_LIVE_VERIFICATION.json",
    "docs/rust-tauri/ORCHESTRATOR_PROGRESS.json",
]
MDS = [
    "docs/rust-tauri/R05/R05_REPORT.md",
    "docs/rust-tauri/R05/R05_INDEPENDENT_REVIEW.md",
    "docs/rust-tauri/R05/R05_BLOCKERS.md",
    "docs/rust-tauri/R05/R05_NEGATIVE_GATE_REPORT.md",
    "docs/rust-tauri/R05/MODEL_USAGE_SEMANTICS.md",
]
OWNED14 = set(JSONS) | set(MDS) | {
    "docs/rust-tauri/R05/WORKER_MODEL_BOUNDARY.md",
    "docs/rust-tauri/R05/R05_INTERFACE_EVOLUTION.md",
}
results = []


def check(name, ok, detail=""):
    results.append({"name": name, "pass": bool(ok), "detail": detail})
    print(("PASS " if ok else "FAIL ") + name + (f" :: {detail}" if detail else ""))
    return ok


def strict_load(rel):
    def hook(pairs):
        seen = set()
        for k, _ in pairs:
            if k in seen:
                raise ValueError(f"duplicate key {k!r}")
            seen.add(k)
        return dict(pairs)
    with open(os.path.join(REPO, rel), encoding="utf-8") as fh:
        return json.load(fh, object_pairs_hook=hook)


def main():
    # 1) JSON 严格解析（含重复键拒绝，递归）
    docs = {}
    ok = True
    for rel in JSONS:
        try:
            docs[rel] = strict_load(rel)
        except Exception as e:  # noqa: BLE001
            ok = False
            check(f"json-strict:{rel}", False, str(e))
    if ok:
        check("json-strict:7份JSON严格解析无重复键", True)

    # 2) Markdown 相对链接
    link_re = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
    bad = []
    n_links = 0
    for rel in MDS:
        with open(os.path.join(REPO, rel), encoding="utf-8") as fh:
            text = fh.read()
        base = os.path.dirname(os.path.join(REPO, rel))
        for m in link_re.finditer(text):
            target = m.group(1).strip()
            if target.startswith(("http://", "https://", "mailto:", "#")):
                continue
            n_links += 1
            path = target.split("#", 1)[0]
            if not path:
                continue
            if not os.path.exists(os.path.normpath(os.path.join(base, path))):
                bad.append(f"{rel} -> {target}")
    check(f"md-links:{n_links}条相对链接全部解析存在", not bad, "; ".join(bad))

    # 3) 七处 rr3_current 相等
    cur = None
    mism = []
    for rel in JSONS:
        d = docs[rel]
        rc = d["rr3_current"] if "rr3_current" in d else d["stages"]["R05"]["rr3_current"]
        s = json.dumps(rc, ensure_ascii=False, sort_keys=True)
        if cur is None:
            cur = s
        elif s != cur:
            mism.append(rel)
    check("cross-doc:七处rr3_current逐字节相等", not mism, "; ".join(mism))

    rc = json.loads(cur)
    six = {
        "offline_gate": "PASS", "independent_review": "PASS",
        "live_verification": "BLOCKED_NOT_AUTHORIZED",
        "stage_readiness": "ACCEPTED_OFFLINE_SCOPE_WITH_REGISTERED_LIVE_DEFERRALS",
        "R06_READY": True, "release_state": "NOT_IN_SCOPE",
    }
    bad = [k for k, v in six.items() if rc.get(k) != v]
    check("six-tuple:rr3_current六元组=FINAL-04", not bad, str(bad))

    # 4) 历史保留
    hand = docs["docs/rust-tauri/R05/R05_HANDOFF.json"]
    hist = hand["rr3_current_history"]
    e03 = [h for h in hist if h.get("round") == "E-03"]
    check("history:E-03快照已入rr3_current_history",
          len(e03) == 1 and e03[0]["snapshot"]["stage_readiness"] == "NOT_ACCEPTED"
          and e03[0]["snapshot"]["R06_READY"] is False)
    orch = docs["docs/rust-tauri/ORCHESTRATOR_PROGRESS.json"]
    check("history:ORCH rr3_current_history含E-03快照",
          any(h.get("round") == "E-03" for h in orch["stages"]["R05"]["rr3_current_history"]))

    rep = open(os.path.join(REPO, "docs/rust-tauri/R05/R05_REPORT.md"), encoding="utf-8").read()
    rev = open(os.path.join(REPO, "docs/rust-tauri/R05/R05_INDEPENDENT_REVIEW.md"), encoding="utf-8").read()
    blk = open(os.path.join(REPO, "docs/rust-tauri/R05/R05_BLOCKERS.md"), encoding="utf-8").read()
    neg = open(os.path.join(REPO, "docs/rust-tauri/R05/R05_NEGATIVE_GATE_REPORT.md"), encoding="utf-8").read()
    hist_fail_keeps = [
        ("REPORT:RR2/FINAL层FAIL表保留", "0f5911462089380b8eb94d137b98f8d2382d062564c43d54cd84338b0852191e" in rep),
        ("REPORT:旧'只剩ALF'禁语不复现（否定引用除外）",
          all(("不能" in ln or "不写" in ln or "不得" in ln or "纠正" in ln)
              for ln in rep.split("\n") if "只剩" in ln)),
        ("REPORT:FINAL-01/02/03历史FAIL保留", all(k in rep for k in ("RR3/FINAL-01", "RR3/FINAL-02", "RR3/FINAL-03"))),
        ("REPORT:raw npm登记红保留", "seal trio" in rep and "registered-not-formal-green" in rep),
        ("REPORT:directed/E5许可保留", "E5 BY SCOPE SKIP" in rep and "directed" in rep),
        ("REPORT:LIVE延期保留", "BLOCKED_NOT_AUTHORIZED" in rep and "最迟 R10" in rep),
        ("REPORT:平台边界保留", "Linux x86_64 继承" in rep and "Windows" in rep),
        ("REPORT:Git未发生不预写", "零暂存/零提交/零推送" in rep),
        ("REVIEW:E-REVIEW-01历史FAIL保留", "E-REVIEW-01" in rev),
        ("BLOCKERS:G02空间阻断历史保留", "ENOSPC" in blk),
        ("NEG:G01/G02历史保留", "exit2/15 行" in neg or "G01" in neg),
        ("NEG:raw npm红保留", "registered-not-formal-green" in neg),
        ("REPORT:F42-F54闭合表述", "F42–F54 全部独立 CLOSED" in rep),
        ("REPORT:r00两对象与6次LAN", "cf9bce2f" in rep and "d57ea731" in rep and "364514be" in rep),
        ("REPORT:空间阻断时间线如实", "cargo clean" in rep and "BLOCKED_BY_STORAGE" not in rep.split("13.4")[1].split("。")[0] if "13.4" in rep else False),
    ]
    for name, okk in hist_fail_keeps:
        check(name, okk)

    # 5) 矩阵-进度-报告-HANDOFF 结论一致
    mtx = json.load(open(os.path.join(REPO, "docs/rust-tauri/R05/repair-current/RR3_ISSUE_MATRIX.json"), encoding="utf-8"))
    m_fg = mtx["finalGate"]["sixTuple"]
    agree = (m_fg["offline_gate"] == rc["offline_gate"]
             and m_fg["stage_readiness"] == rc["stage_readiness"]
             and m_fg["R06_READY"] == rc["R06_READY"]
             and m_fg["live_verification"] == rc["live_verification"])
    check("consistency:与总控RR3_ISSUE_MATRIX.finalGate六元组一致", agree)
    orch_st = orch["stages"]["R05"]
    check("consistency:ORCH stages.R05状态/verdict/READY",
          orch_st["status"] == rc["stage_readiness"]
          and orch_st["R06_READY"] is True
          and orch_st["stage_verdict"] == "PASS"
          and "rr3_repair_round" in orch_st)
    check("consistency:HANDOFF accepted_tasks=8任务+叶表背书",
          hand["accepted_tasks"] == [f"R05-T0{i}" for i in range(1, 9)]
          and hand["accepted_tasks_evidence"]["pass"] == 130)
    check("consistency:HANDOFF rr3_repair_round存在且指向FINAL-04",
          "FINAL-04" in hand["rr3_repair_round"]["result"]
          and hand["rr3_repair_round"]["candidate"].startswith("b3ac0e6a"))
    check("consistency:git_delivery未预写",
          hand["git_delivery_receipt_contract"]["status"] == "NOT_PERFORMED_AT_E04_CUTOFF"
          and rc["git_delivery"]["committed_sha"] is None)

    # 6) 前后树快照对比：差异仅允许 12 份 owned 文档 + E-04 证据根新增
    before = json.load(open(os.path.join(E04DIR, "tree-snapshot-before.json"), encoding="utf-8"))
    after = json.load(open(os.path.join(E04DIR, "tree-snapshot-after.json"), encoding="utf-8"))
    b, a = before["files"], after["files"]
    changed = sorted(p for p in b.keys() & a.keys() if b[p]["sha256"] != a[p]["sha256"])
    removed = sorted(b.keys() - a.keys())
    added = sorted(a.keys() - b.keys())
    expect_changed = sorted(OWNED14 - {
        "docs/rust-tauri/R05/WORKER_MODEL_BOUNDARY.md",
        "docs/rust-tauri/R05/R05_INTERFACE_EVOLUTION.md",
    })
    added_outside = [p for p in added if not p.startswith("artifacts/rust-tauri/R05/RR3/E-04/")]
    check(f"inputs:改动仅12份owned文档（实改{len(changed)}）", changed == expect_changed, str(changed))
    check("inputs:无删除文件", not removed, str(removed[:5]))
    check("inputs:新增仅E-04证据根", not added_outside, str(added_outside[:5]))
    # FINAL-04 冻结 33 项生产输入与当前树相等
    pc = json.load(open(os.path.join(REPO, "artifacts/rust-tauri/R05/RR3/FINAL-04/command-records/frozen-inputs-postcheck.json"), encoding="utf-8"))
    mism2 = [p for p, m in pc["inputs"].items()
             if a.get(p, {}).get("sha256") != m["sha256"]]
    check("inputs:FINAL-04冻结33项生产输入与当前树逐项相等", not mism2, str(mism2))
    # rust/scripts/lock/schema/pins-cids TSV 与 FINAL-04 tested 树相等（33 项中 32 项属该前缀集，
    # 第 33 项 docs/rust-tauri/R01/r01_t01_check_ownership.py 已含于上方 33 项全等检查）
    prefixes = ("rust/", "scripts/", "rust-toolchain.toml")
    named = ("docs/rust-tauri/R05/r05_stage_pins.tsv", "docs/rust-tauri/R05/r05_stage_cids.tsv",
             "docs/rust-tauri/R05/r05_required_cids.tsv", "docs/rust-tauri/R05/r05_leaf_case_map.tsv")
    keys = [p for p in pc["inputs"] if p.startswith(prefixes) or p in named]
    equal_named = all(a.get(p, {}).get("sha256") == pc["inputs"][p]["sha256"] for p in keys)
    check(f"inputs:rust/scripts/lock/toolchain/四TSV共{len(keys)}项全部相等", equal_named and len(keys) == 32)

    out = {"generated_at_utc": __import__("datetime").datetime.now(__import__("datetime").timezone.utc).isoformat(),
           "checks": results,
           "summary": {"total": len(results), "failed": sum(1 for r in results if not r["pass"])}}
    with open(os.path.join(E04DIR, "check-results.json"), "w", encoding="utf-8") as fh:
        json.dump(out, fh, ensure_ascii=False, indent=2)
        fh.write("\n")
    failed = out["summary"]["failed"]
    print(f"\n{len(results)} checks, {failed} failed")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()

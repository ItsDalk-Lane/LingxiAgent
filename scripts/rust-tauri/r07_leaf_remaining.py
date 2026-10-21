#!/usr/bin/env python3
"""R07 叶绑定剩余账：对 408 补充叶逐叶输出当前绑定状态、可用真实测试候选与
缺口类型，供后续绑定 authoring 与补测排期。只读：不改地图、不改绑定表。

输入：R07_CLOSEOUT_LEDGER.json（叶/断言/登记）、R07_LEAF_BINDINGS.json（已绑）、
      某轮门禁证据的 LEAF_CASES/leaf-cases.json 或套件日志（真实测试池）。
输出：docs/rust-tauri/R07/R07_LEAF_REMAINING.json + .md
"""

from __future__ import annotations

import json
import os
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / "docs/rust-tauri/R07/R07_CLOSEOUT_LEDGER.json"
BINDINGS = ROOT / "docs/rust-tauri/R07/R07_LEAF_BINDINGS.json"
OUT_JSON = ROOT / "docs/rust-tauri/R07/R07_LEAF_REMAINING.json"
OUT_MD = ROOT / "docs/rust-tauri/R07/R07_LEAF_REMAINING.md"

CLIENT_MARKERS = ("-UI-", "-CLI-", "-DESKTOP_BEHAVIOR-", "-UI_BEHAVIOR-", "MOBILE")


def test_pool(evidence: Path) -> dict[str, str]:
    pool: dict[str, str] = {}
    base = evidence / "SUITES"
    if not base.is_dir():
        return pool
    for suite in sorted(os.listdir(base)):
        log = base / suite / "output.log"
        if not log.is_file():
            continue
        for line in log.read_text(encoding="utf-8", errors="replace").splitlines():
            if line.startswith("test ") and " ... ok" in line:
                pool[f"{suite}::{line.split(' ... ')[0][5:].strip()}"] = suite
    return pool


def keywords(feature_id: str) -> list[str]:
    parts = [p.lower() for p in feature_id.split("-")[3:]]
    parts = [p for p in parts if not re.fullmatch(r"[0-9a-f]{6}", p)]
    stop = {"semantic", "effect", "ui", "cli", "tool", "behavior", "route", "provider", "post", "read", "get", "id"}
    return [p for p in parts if p not in stop and len(p) >= 4]


def main(argv: list[str]) -> int:
    if "--evidence" not in argv:
        raise SystemExit("--evidence <gate evidence root> required")
    evidence = Path(argv[argv.index("--evidence") + 1])
    pool = test_pool(evidence)
    if not pool:
        print("no suite logs under evidence root", file=sys.stderr)
        return 1
    ledger = json.loads(LEDGER.read_text(encoding="utf-8"))
    bound = json.loads(BINDINGS.read_text(encoding="utf-8"))["leaves"]
    rows = []
    by_domain: dict[str, Counter] = defaultdict(Counter)
    for leaf in ledger["leaves"]:
        leaf_id = leaf["id"]
        n = len(leaf["assertions"])
        fid = leaf["featureId"]
        domain = fid.split("-")[2] if fid.count("-") >= 2 else "?"
        client_facing = any(m in fid for m in CLIENT_MARKERS)
        if leaf_id in bound:
            status = "BOUND"
            gap = "none"
            candidates: list[str] = [c for g in bound[leaf_id]["assertionCases"] for c in g]
        else:
            keys = keywords(fid)
            scored = []
            for case in pool:
                name = case.lower()
                score = sum(1 for k in keys if k in name)
                if score >= 2 or (score >= 1 and len(keys) <= 2):
                    scored.append((score, case))
            scored.sort(reverse=True)
            candidates = [c for _, c in scored[:6]]
            registered = leaf.get("registeredTestIds") or []
            status = "UNBOUND"
            if client_facing:
                gap = "client_behavior_needs_ui_or_cli_suite_in_gate"
            elif not candidates and not registered:
                gap = "no_real_test_candidate_needs_new_probe"
            elif len(set(candidates)) < n:
                gap = "insufficient_distinct_tests_needs_negative_or_positive_probe"
            else:
                gap = "candidates_exist_needs_semantic_review"
        rows.append({
            "id": leaf_id,
            "featureId": fid,
            "domain": domain,
            "group": leaf["group"],
            "assertions": n,
            "status": status,
            "gap": gap,
            "candidates": candidates,
            "registeredTestIds": leaf.get("registeredTestIds") or [],
            "laterStages": leaf.get("laterStages") or [],
        })
        by_domain[domain][status] += 1
        by_domain[domain][f"gap:{gap}"] += 1
    summary = {
        "leaves": len(rows),
        "bound": sum(1 for r in rows if r["status"] == "BOUND"),
        "unbound": sum(1 for r in rows if r["status"] == "UNBOUND"),
        "assertionsBound": sum(r["assertions"] for r in rows if r["status"] == "BOUND"),
        "assertionsUnbound": sum(r["assertions"] for r in rows if r["status"] == "UNBOUND"),
        "gaps": dict(Counter(r["gap"] for r in rows if r["status"] == "UNBOUND")),
        "realTestPool": len(pool),
    }
    payload = {
        "schema": "lingxi.r07.leaf-remaining.v1",
        "generatedBy": "scripts/rust-tauri/r07_leaf_remaining.py",
        "evidenceRoot": str(evidence),
        "summary": summary,
        "byDomain": {d: dict(c) for d, c in sorted(by_domain.items())},
        "leaves": rows,
    }
    OUT_JSON.write_text(json.dumps(payload, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    lines = [
        "# R07 叶绑定剩余账（机器生成，只读）",
        "",
        f"- 真实测试池（门禁套件日志）：{summary['realTestPool']} 条通过测试",
        f"- 叶：{summary['leaves']}；已逐断言绑定 {summary['bound']}（{summary['assertionsBound']} 断言）；"
        f"未绑定 {summary['unbound']}（{summary['assertionsUnbound']} 断言）",
        "",
        "## 未绑定叶缺口类型",
        "",
    ]
    for gap, count in sorted(summary["gaps"].items(), key=lambda kv: -kv[1]):
        lines.append(f"- `{gap}`：{count} 叶")
    lines += ["", "## 按域", ""]
    for domain, counter in sorted(by_domain.items()):
        lines.append(f"- {domain}：BOUND {counter.get('BOUND', 0)} / UNBOUND {counter.get('UNBOUND', 0)}")
    lines += [
        "",
        "## 缺口类型含义与补齐路径",
        "",
        "- `candidates_exist_needs_semantic_review`：域内已有 ≥ 断言数的真实通过测试候选，需逐断言语义审核后写入 R07_LEAF_BINDINGS.json（测试名须与断言条款直接对应，叶内不复用）。",
        "- `insufficient_distinct_tests_needs_negative_or_positive_probe`：候选少于断言数，通常缺「拒绝/无效输入」负向探针或「成功效果」正向探针，需补具名测试。",
        "- `no_real_test_candidate_needs_new_probe`：无任何域内候选，需为该路由/操作新增正反向探针测试。",
        "- `client_behavior_needs_ui_or_cli_suite_in_gate`：断言属客户端（UI/CLI/桌面）行为，Rust 服务套件结构性不覆盖；需把 vitest/CLI 套件接入正式门禁作为生产者，或按已有依据划定阶段份额。",
        "",
        "逐叶明细见同名 .json（candidates 为关键词候选，仅供审核起点，不是绑定依据）。",
    ]
    OUT_MD.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(json.dumps(summary, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

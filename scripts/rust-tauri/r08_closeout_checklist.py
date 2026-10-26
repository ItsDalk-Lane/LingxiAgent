#!/usr/bin/env python3
"""生成 R08 收尾机器可读关闭清单 CLOSEOUT_CHECKLIST.json。

每项：原要求（R00 then + 逐断言）→ 实现路径（T01 绑定表候选）→ 真实消费者 →
断言/案例（FULL_BINDINGS 图钉）→ 本轮证据 → 状态/负责人。126 叶全部纳入，
保留原 ID 与责任。绑定表候选为关键词粗匹配（matchKind=keyword-candidate），
逐叶精确路径在补证落位时覆写。

用法：python3 scripts/rust-tauri/r08_closeout_checklist.py [--check]
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts/rust-tauri"))

from r08_stage_inputs import FULL_BINDINGS  # noqa: E402 - 复用唯一图钉真理源

MAP_PATH = ROOT / "rust/crates/xtask/src/stage_maps/R08.json"
TABLE_PATH = ROOT / "docs/rust-tauri/R08/T01_API_BINDING_TABLE.json"
OUT_PATH = ROOT / "docs/rust-tauri/R08/CLOSEOUT_CHECKLIST.json"

# 特性尾段 → API 路径关键词的手工锚点（粗匹配的补充，覆盖明显不规则命名）。
PATH_ANCHORS = {
    "QUICK-CHAT-WINDOW": "quick",
    "CHAT-TASK-TASKID-ABORT-POST": "chat",
    "WORKSPACE-ROLLBACK-PURGE": "workspace/rollback",
    "WORKSPACE-ROLLBACK-RETRY": "workspace/rollback",
    "SESSION-PERMISSION-MODE": "permission",
    "PLAN-MODE-READ": "plan",
    "SESSION-PLAN-MODE-TOGGLE": "plan",
    "SESSIONS-SWEEP-ORPHANED": "sweep",
    "SESSIONS-LATEST-USER-MESSAGE": "latest",
    "CHECKPOINTS-USER-EDIT": "checkpoint",
    "CONVERSATION-MAP": "conversation",
    "SESSION-PROJECTS": "project",
    "SESSION-FOLDERS": "folder",
}


def feature_tail(feature_id: str) -> str:
    # F-D02-ROUTE_BEHAVIOR-BEHAVIOR-SESSIONS-SESSIONS-FIND-1A2B → 保留路由段
    parts = feature_id.split("-")
    out = []
    for token in reversed(parts):
        out.append(token)
        if token in ("BEHAVIOR", "EFFECT"):
            break
    return "-".join(reversed(out))


def norm(text: str) -> str:
    return re.sub(r"[^a-z0-9]", "", text.lower())


def binding_candidates(tail: str, bindings: list[dict]) -> list[str]:
    keys = [k for k, v in PATH_ANCHORS.items() if tail.startswith(k)]
    needles = [norm(tail)]
    for key in keys:
        needles.append(norm(key))
    hits = []
    for row in bindings:
        path_norm = norm(row.get("path", ""))
        if any(n and n in path_norm for n in needles):
            hits.append(row["id"])
        if len(hits) >= 6:
            break
    return hits


def main() -> int:
    stage = json.loads(MAP_PATH.read_text(encoding="utf-8"))
    table = json.loads(TABLE_PATH.read_text(encoding="utf-8"))
    bindings = table["bindings"]
    # 可选：注入正式门结果（--gate <verify-stage-result.json>）→ 逐叶门判定与证据。
    gate = None
    gate_path = None
    if "--gate" in sys.argv:
        gate_path = Path(sys.argv[sys.argv.index("--gate") + 1])
        gate = json.loads(gate_path.read_text(encoding="utf-8"))
    gate_leaves = {l["id"]: l for l in (gate or {}).get("supplementalLeafScenarios", [])}
    items = []
    for leaf in stage["supplementalLeafScenarios"]:
        tail = feature_tail(leaf["featureId"])
        full = FULL_BINDINGS.get(leaf["id"])
        cases = [c for group in full["assertion_cases"] for c in group] if full else []
        gl = gate_leaves.get(leaf["id"])
        items.append({
            "id": leaf["id"],
            "featureId": leaf["featureId"],
            "requirement": leaf["requirement"],
            "original": {
                "then": leaf["r00Then"],
                "assertions": leaf["r00Assertions"],
                "taskIds": leaf["r00TaskIds"],
                "due": leaf["r00Due"],
            },
            "implPath": {
                "matchKind": "keyword-candidate",
                "bindingRows": binding_candidates(tail, bindings),
                "note": "T01 绑定表候选；补证落位时覆写为精确路由/模块。",
            },
            "consumers": [],  # 补证落位时从绑定行 consumers 覆写
            "cases": cases,
            "assertionCases": full["assertion_cases"] if full else None,
            "evidence": {
                "commandRefs": leaf.get("evidenceCommandRefs", []),
                "paths": leaf.get("evidencePaths", []),
                "gateResult": str(gate_path) if gate_path else None,
                "gateStatus": gl.get("status") if gl else None,
                "gateReason": (gl.get("reason") or "")[:240] if gl else None,
            },
            "status": gl.get("status") if gl else leaf["basisKind"],
            "basisKind": leaf["basisKind"],
            "owner": "r08-closeout-session",
        })
    summary = {}
    for item in items:
        summary[item["status"]] = summary.get(item["status"], 0) + 1
    doc = {
        "schema": "lingxi.r08.closeout-checklist.v1",
        "stage": "R08",
        "generatedFrom": {
            "stageMap": "rust/crates/xtask/src/stage_maps/R08.json",
            "bindingTable": "docs/rust-tauri/R08/T01_API_BINDING_TABLE.json",
            "fullBindings": "scripts/rust-tauri/r08_stage_inputs.py:FULL_BINDINGS",
            "gateResult": str(gate_path) if gate_path else None,
        },
        "gateSummary": None if not gate else {
            "overall": gate.get("overall"),
            "testedSha": gate.get("testedSha"),
            "candidateStable": (gate.get("candidateSourceBinding") or {}).get("stable"),
            "commandsPass": sum(1 for c in gate.get("commands", []) if c.get("status") == "PASS"),
            "commandsTotal": len(gate.get("commands", [])),
            "scenariosPass": sum(1 for s in gate.get("scenarios", []) if s.get("status") == "PASS"),
            "scenariosTotal": len(gate.get("scenarios", [])),
        },
        "summary": summary,
        "items": items,
    }
    OUT_PATH.write_text(json.dumps(doc, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"checklist written: {len(items)} items, {summary}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

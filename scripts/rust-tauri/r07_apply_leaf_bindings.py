#!/usr/bin/env python3
"""把 docs/rust-tauri/R07/R07_LEAF_BINDINGS.json 的逐断言绑定注入
rust/crates/xtask/src/stage_maps/R07.json：

- 注册案例生产者命令 `r07_leaf_cases`（读本轮套件日志产出 leaf-cases.json）；
- 对每个绑定叶：basisKind=full_original_behavior、evidenceCommandRefs=依赖套件
  + 生产者、evidencePaths/assertionContract/originalAssertionCases 按绑定表写入；
- 未绑定叶一律保持原样（required_behavior_pending），不批量改状态。

校验（失败即退出 1，不写地图）：叶必须在地图中；assertionCases 组数 = R00
原断言数；叶内 case 不重复；suiteRefs 必须是地图已注册的套件命令。
地图解析层（xtask stage_map.rs）随后还会做第二遍独立校验。
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MAP = ROOT / "rust/crates/xtask/src/stage_maps/R07.json"
BINDINGS = ROOT / "docs/rust-tauri/R07/R07_LEAF_BINDINGS.json"


def main() -> int:
    stage_map = json.loads(MAP.read_text(encoding="utf-8"))
    bindings = json.loads(BINDINGS.read_text(encoding="utf-8"))
    producer = bindings["producerCommand"]
    evidence_path = bindings["evidencePath"]
    commands = stage_map["commands"]
    commands[producer] = {
        "argv": [
            "python3", "-B", "scripts/rust-tauri/r07_leaf_cases.py",
            "--evidence", "{EVIDENCE}", "--out", "{EVIDENCE}/LEAF_CASES",
        ],
        "timeoutSecs": 600,
        "evidencePaths": [evidence_path],
    }
    for suite in ("r07_probe_skills", "r07_probe_access"):
        commands[suite] = {
            "argv": [
                "python3", "-B", "scripts/rust-tauri/r07_stage_inputs.py",
                "--run-suite", suite, "--evidence", "{EVIDENCE}/SUITES/" + suite,
            ],
            "timeoutSecs": 3600,
            "evidencePaths": [
                "{EVIDENCE}/SUITES/" + suite + "/result.json",
                "{EVIDENCE}/SUITES/" + suite + "/output.log",
            ],
        }
    commands["r07_ui_vitest"] = {
        "argv": [
            "python3", "-B", "scripts/rust-tauri/r07_ui_cases.py",
            "--evidence", "{EVIDENCE}",
        ],
        "timeoutSecs": 1200,
        "evidencePaths": [
            "{EVIDENCE}/SUITES/r07_ui_vitest/output.log",
            "{EVIDENCE}/SUITES/r07_ui_vitest/result.json",
        ],
    }
    leaves = {leaf["id"]: leaf for leaf in stage_map["supplementalLeafScenarios"]}
    errors: list[str] = []
    applied = 0
    for leaf_id, binding in bindings["leaves"].items():
        leaf = leaves.get(leaf_id)
        if leaf is None:
            errors.append(f"{leaf_id}: not in stage map")
            continue
        groups = binding["assertionCases"]
        if binding["basis"] == "full_original_behavior" and len(groups) != len(leaf["r00Assertions"]):
            errors.append(
                f"{leaf_id}: {len(groups)} case groups for {len(leaf['r00Assertions'])} assertions"
            )
            continue
        flat = [case for group in groups for case in group]
        if len(set(flat)) != len(flat):
            errors.append(f"{leaf_id}: duplicate case within leaf")
            continue
        if any(not group for group in groups):
            errors.append(f"{leaf_id}: an assertion has no case")
            continue
        for suite in binding["suiteRefs"]:
            if suite not in commands:
                errors.append(f"{leaf_id}: suiteRef {suite!r} is not a registered command")
        for case in flat:
            suite = case.split("::", 1)[0]
            if suite not in binding["suiteRefs"]:
                errors.append(f"{leaf_id}: case {case!r} comes from a suite outside suiteRefs")
        if errors:
            continue
        leaf["basisKind"] = binding["basis"]
        leaf["evidenceCommandRefs"] = list(binding["suiteRefs"]) + [producer]
        leaf["evidencePaths"] = [evidence_path]
        leaf["assertionContract"] = {
            "producerCommand": producer,
            "evidencePath": evidence_path,
            "cases": [{"case": case, "expect": 1} for case in flat],
        }
        if binding["basis"] == "full_original_behavior":
            leaf["originalAssertionCases"] = groups
            leaf.pop("stageShare", None)
            leaf.pop("laterShare", None)
        elif binding["basis"] == "stage_share_satisfied":
            # 阶段中立份额：仅含后续阶段的叶允许；份额边界与余款文本必填。
            if not binding.get("stageShare") or not binding.get("laterShare"):
                errors.append(f"{leaf_id}: stage_share_satisfied needs stageShare and laterShare")
                continue
            later = [s for s in leaf.get("r00ExecutionStageIds", []) if s > "R07"]
            if not later:
                errors.append(f"{leaf_id}: stage_share_satisfied but no later stage in R00 ledger")
                continue
            leaf["stageShare"] = binding["stageShare"]
            leaf["laterShare"] = binding["laterShare"]
            leaf.pop("originalAssertionCases", None)
        else:
            errors.append(f"{leaf_id}: unsupported basis {binding['basis']!r}")
            continue
        leaf["evidenceRequired"] = (
            "每条 R00 原断言绑定本轮真实套件测试案例（leaf-cases.json 由套件日志生成；"
            "套件退出非 0 或测试未通过即案例不成立）；绑定依据见 R07_LEAF_BINDINGS.json rationale。"
        )
        applied += 1
    if errors:
        for line in errors:
            print("binding error:", line, file=sys.stderr)
        return 1
    MAP.write_text(json.dumps(stage_map, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"applied {applied} leaf bindings; producer command {producer!r} registered")
    return 0


if __name__ == "__main__":
    sys.exit(main())

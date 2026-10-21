#!/usr/bin/env python3
"""R07 收口剩余账生成器（任务 0 建账）：按稳定叶 ID 合并两本 R00 原账、
功能盘点（入口/存储）、现行结果台账与各任务证据草案，逐叶给出分组、
原断言、当前状态、具名测试与剩余动作分类。只读原账，不改任何来源。

剩余动作分类（逐叶逐断言层面由门禁合同最终判定，本账是工作清单）：
- BIND_EVIDENCE：实现与测试据称存在，缺逐断言合同绑定（NOT_RUN 但套件绿）。
- IMPLEMENT_THEN_BIND：已知实现缺口（如无路由/契约冲突），先修实现再绑。
- SHARE_DOC_AND_BIND：含后续阶段份额，须拆清 R07 到期部分并绑定，余款留依据。
- CONVERT_REGISTRY：已有 PASS_LOCAL_* 登记与具名测试，转为正式合同绑定。
"""

from __future__ import annotations

import json
import os
from pathlib import Path

ROOT = Path(os.environ.get("LINGXI_ROOT", Path(__file__).resolve().parents[2]))
OUT = Path("docs/rust-tauri/R07/R07_CLOSEOUT_LEDGER.json")


def load(path: str):
    return json.loads((ROOT / path).read_text(encoding="utf-8"))


def later_stages(stage_ids: list[str]) -> list[str]:
    return [s for s in stage_ids if len(s) == 3 and s.startswith("R") and s > "R07"]


def main() -> int:
    acceptance = load("docs/rust-tauri/R00/ACCEPTANCE_MAP.json")["scenarios"]
    fsa = {r["id"]: r for r in load("docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json")["supplemental_scenarios"]}
    inventory = {f["feature_id"]: f for f in load("docs/rust-tauri/R00/FEATURE_INVENTORY.json")["features"]}
    parity = load("docs/rust-tauri/R07/FEATURE_PARITY_RESULTS.json")["scenarioResults"]

    drafts: dict[str, list[dict]] = {}
    for name in ("T07", "T08", "T09", "T10"):
        path = ROOT / f"docs/rust-tauri/R07/{name}_LEAF_EVIDENCE_DRAFT.json"
        if path.is_file():
            data = json.loads(path.read_text(encoding="utf-8"))
            for leaf_id, row in (data.get("leaves") or {}).items():
                drafts.setdefault(leaf_id, []).append({
                    "task": name,
                    "draftResult": row.get("draftResult"),
                    "proofs": row.get("directCurrentProof") or [],
                    "note": row.get("note"),
                })

    leaves = {k: v for k, v in acceptance.items()
              if v.get("kind") == "supplemental" and "R07" in (v.get("execution_stage_ids") or [])}
    if len(leaves) != 408:
        raise SystemExit(f"R07 补充叶集合应为 408，实际 {len(leaves)}")

    rows = []
    counts: dict[str, int] = {}
    for leaf_id in sorted(leaves):
        leaf = leaves[leaf_id]
        stage_ids = leaf["execution_stage_ids"]
        later = later_stages(stage_ids)
        group = "R07_ONLY" if stage_ids == ["R07"] else ("HAS_LATER_SHARE" if later else "EARLIER_PLUS_R07")
        feature = inventory.get(leaf["feature_id"]) or {}
        fsa_row = fsa[leaf_id]
        current = parity.get(leaf_id) or {}
        status = current.get("status", "NOT_RUN")
        test_ids = current.get("testIds") or []
        draft = drafts.get(leaf_id, [])
        if status.startswith("PASS_LOCAL"):
            action = "CONVERT_REGISTRY"
        elif later:
            action = "SHARE_DOC_AND_BIND"
        else:
            action = "BIND_EVIDENCE"
        rows.append({
            "id": leaf_id,
            "featureId": leaf["feature_id"],
            "taskIds": leaf["task_ids"],
            "executionStageIds": stage_ids,
            "laterStages": later,
            "group": group,
            "assertions": fsa_row["assertions"],
            "due": fsa_row["due"],
            "entrypoints": feature.get("current_entrypoints") or [],
            "stores": feature.get("current_stores") or [],
            "currentStatus": status,
            "registeredTestIds": test_ids,
            "evidenceDirectory": current.get("evidenceDirectory"),
            "draftEvidence": draft,
            "remainingAction": action,
        })
        counts[action] = counts.get(action, 0) + 1

    ledger = {
        "schema": "lingxi.r07.closeout-ledger.v1",
        "stage": "R07",
        "generatedBy": "scripts/rust-tauri/r07_closeout_ledger.py",
        "reviewBaseline": "bfe32d107fb60852712b897a25403d7f092671b3",
        "counts": {"leaves": len(rows), "byAction": counts,
                   "assertionsTotal": sum(len(r["assertions"]) for r in rows)},
        "leaves": rows,
    }
    out = ROOT / OUT
    out.write_text(json.dumps(ledger, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"ledger: {len(rows)} leaves, actions={counts}, "
          f"assertions={ledger['counts']['assertionsTotal']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

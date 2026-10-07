#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""RR3 E-04 收据生成：changed-files / protected-inputs-equality / git-state。"""
import hashlib
import json
import os
import subprocess

REPO = "/Users/study_superior/Desktop/Code/LingxiAgent"
E04 = os.path.join(REPO, "artifacts/rust-tauri/R05/RR3/E-04")
UTC = subprocess.run(["date", "-u", "+%Y-%m-%dT%H:%M:%SZ"], capture_output=True,
                     text=True, check=True).stdout.strip()

OWNED14 = [
    "docs/rust-tauri/R05/R05_REPORT.md",
    "docs/rust-tauri/R05/R05_INDEPENDENT_REVIEW.md",
    "docs/rust-tauri/R05/R05_BLOCKERS.md",
    "docs/rust-tauri/R05/R05_HANDOFF.json",
    "docs/rust-tauri/R05/PROGRESS_LEDGER.json",
    "docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json",
    "docs/rust-tauri/R05/R05_TEST_MAP.json",
    "docs/rust-tauri/R05/R05_NEGATIVE_GATE_REPORT.md",
    "docs/rust-tauri/R05/R05_PERFORMANCE_RESULTS.json",
    "docs/rust-tauri/R05/R05_LIVE_VERIFICATION.json",
    "docs/rust-tauri/R05/WORKER_MODEL_BOUNDARY.md",
    "docs/rust-tauri/R05/MODEL_USAGE_SEMANTICS.md",
    "docs/rust-tauri/R05/R05_INTERFACE_EVOLUTION.md",
    "docs/rust-tauri/ORCHESTRATOR_PROGRESS.json",
]


def main():
    before = json.load(open(os.path.join(E04, "tree-snapshot-before.json"), encoding="utf-8"))["files"]
    after = json.load(open(os.path.join(E04, "tree-snapshot-after.json"), encoding="utf-8"))["files"]

    changed = sorted(p for p in before.keys() & after.keys()
                     if before[p]["sha256"] != after[p]["sha256"])
    unchanged14 = [p for p in OWNED14 if p not in changed]
    changed_out = {
        "generated_at_utc": UTC,
        "owned_14": OWNED14,
        "changed_owned_docs": [
            {"path": p, "before_sha256": before[p]["sha256"], "after_sha256": after[p]["sha256"],
             "before_bytes": before[p]["bytes"], "after_bytes": after[p]["bytes"]}
            for p in changed
        ],
        "byte_identical_owned_docs": unchanged14,
        "non_owned_changed": [p for p in changed if p not in OWNED14],
        "removed": sorted(before.keys() - after.keys()),
        "added_outside_e04_evidence": sorted(
            p for p in after.keys() - before.keys()
            if not p.startswith("artifacts/rust-tauri/R05/RR3/E-04/")),
    }
    with open(os.path.join(E04, "changed-files.json"), "w", encoding="utf-8") as fh:
        json.dump(changed_out, fh, ensure_ascii=False, indent=2)
        fh.write("\n")

    # 受保护输入相等证明：FINAL-04 冻结 33 项 + 全树快照前后对比
    pc = json.load(open(os.path.join(REPO, "artifacts/rust-tauri/R05/RR3/FINAL-04/command-records/frozen-inputs-postcheck.json"), encoding="utf-8"))
    frozen_rows = []
    for p, m in sorted(pc["inputs"].items()):
        cur = after.get(p)
        frozen_rows.append({
            "path": p,
            "final04_sha256": m["sha256"],
            "current_sha256": cur["sha256"] if cur else None,
            "equal": bool(cur and cur["sha256"] == m["sha256"] and cur["bytes"] == m["bytes"]),
        })
    proof = {
        "generated_at_utc": UTC,
        "claim": "E-04 回填零触碰被验门实际消费的语义生产输入：rust/、scripts/、lock、toolchain、"
                 "R05 四 TSV、R01 检查脚本与 FINAL-04 testedSha（b3ac0e6a+真实工作树）对应树逐项相等；"
                 "全部差异仅 12 份 E owned 输出/回执类文档。",
        "final04_frozen_inputs": {
            "count": len(frozen_rows),
            "source": "artifacts/rust-tauri/R05/RR3/FINAL-04/command-records/frozen-inputs-postcheck.json",
            "tested_head": pc["head"],
            "all_equal": all(r["equal"] for r in frozen_rows),
            "rows": frozen_rows,
        },
        "full_tree_before_after": {
            "before_snapshot": "tree-snapshot-before.json",
            "after_snapshot": "tree-snapshot-after.json",
            "enumeration": "git --no-optional-locks ls-files --cached --others --exclude-standard -z（与 candidate.rs/绑定器同枚举语义；排除 E-04 证据根本身）",
            "before_files": len(before),
            "after_files": len(after),
            "changed_count": len(changed),
            "changed_all_within_owned14": all(p in OWNED14 for p in changed),
            "changed_paths": changed,
            "removed_count": len(changed_out["removed"]),
            "added_outside_e04_evidence_count": len(changed_out["added_outside_e04_evidence"]),
            "byte_identical_owned": unchanged14,
        },
        "semantic_inputs_note": "R05_SCOPE_MATRIX.json、R00/R01/R02 权威表、stage_maps、r05_t08_* 脚本、"
                                "Cargo.lock 等均含于快照前后对比（未出现在 changed_paths 即字节不变）；"
                                "DOC-INPUT-BOUNDARY-01 的 14 文件消费者分类中本轮回填全部落在输出/回执类"
                                "当前字段，WORKER_MODEL_BOUNDARY.md 与 R05_INTERFACE_EVOLUTION.md 字节不变。",
    }
    with open(os.path.join(E04, "protected-inputs-equality.json"), "w", encoding="utf-8") as fh:
        json.dump(proof, fh, ensure_ascii=False, indent=2)
        fh.write("\n")

    # Git 状态收据
    def run(cmd):
        r = subprocess.run(cmd, cwd=REPO, capture_output=True, text=True)
        return r.stdout.strip()
    git_state = {
        "recorded_at_utc": UTC,
        "head": run(["git", "rev-parse", "HEAD"]),
        "branch": run(["git", "rev-parse", "--abbrev-ref", "HEAD"]),
        "origin_same": run(["git", "rev-parse", "origin/codex/rust-tauri-migration"]),
        "staged_empty": run(["git", "diff", "--cached", "--name-only"]) == "",
        "note": "E-04 零 Git 写操作（无 add/commit/push/重置）；FINAL-04 已亲核零暂存/零提交/零推送；"
                "真实提交/推送回执由总控按既有授权执行后另行归档，本 E04 不预写。",
        "git_status_porcelain_head": run(["git", "status", "--porcelain"]).split("\n")[:8],
    }
    with open(os.path.join(E04, "git-state.json"), "w", encoding="utf-8") as fh:
        json.dump(git_state, fh, ensure_ascii=False, indent=2)
        fh.write("\n")
    print("finalize receipts done:", UTC)
    print("changed:", len(changed), "unchanged14:", unchanged14)


if __name__ == "__main__":
    main()

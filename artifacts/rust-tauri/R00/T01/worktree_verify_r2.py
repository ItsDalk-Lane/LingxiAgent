#!/usr/bin/env python3
"""R00-A02 复测：只读收集现况，再把结果另存为证据。"""

import hashlib
import json
import pathlib
from datetime import datetime, timezone

import worktree_current

ROOT = pathlib.Path(__file__).resolve().parents[4]
OUT = pathlib.Path(__file__).resolve().parent
OLD = worktree_current.OLD
NEW = worktree_current.NEW


def main():
    original_bytes = (OUT / "worktree-before.json").read_bytes()
    original = json.loads(original_bytes)
    current = worktree_current.collect(ROOT)
    checks = worktree_current.verify_first_snapshot(ROOT, original, current)
    result = {
        "captured_utc": datetime.now(timezone.utc).isoformat(),
        "tested_sha": current["head"],
        "checks": checks,
        "all_preserved": all(checks.values()),
        "old_deleted_count": len(current["old_deleted_paths"]),
        "new_untracked_count": len(current["new_files"]),
        "old_diff_sha256": current["old_diff_sha256"],
        "new_aggregate_sha256": hashlib.sha256(json.dumps(current["new_files"], sort_keys=True).encode()).hexdigest(),
        "first_snapshot_sha256": hashlib.sha256(original_bytes).hexdigest(),
        "current_worktree_fingerprint": worktree_current.fingerprint(current),
    }
    (OUT / "worktree-preservation-r2.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    return 0 if result["all_preserved"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

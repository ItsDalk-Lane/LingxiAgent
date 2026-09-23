#!/usr/bin/env python3
"""R00-T01：只读核对启动前已存在的两组任务书改动。"""

import hashlib
import json
import pathlib
import subprocess
import sys
from datetime import datetime, timezone


ROOT = pathlib.Path(__file__).resolve().parents[4]
OUT = pathlib.Path(__file__).resolve().parent
OLD = "Lingxi_Refactor_Taskbooks_2026-09-21"
NEW = "Lingxi_Rust_Tauri_Taskbooks_2026-09-23"


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT)


def main():
    if len(sys.argv) != 2 or sys.argv[1] not in {"before", "after"}:
        raise SystemExit("usage: worktree_snapshot.py <before|after>")
    old_paths = git("ls-files", "--", OLD).decode().splitlines()
    new_paths = sorted(path for path in (ROOT / NEW).rglob("*") if path.is_file())
    snapshot = {
        "captured_utc": datetime.now(timezone.utc).isoformat(),
        "candidate_sha": git("rev-parse", "HEAD").decode().strip(),
        "branch": git("branch", "--show-current").decode().strip(),
        "old_tracked_count": len(old_paths),
        "old_deleted_count": sum(not (ROOT / name).exists() for name in old_paths),
        "old_diff_sha256": hashlib.sha256(git("diff", "--binary", "--", OLD)).hexdigest(),
        "new_untracked_files": {
            str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in new_paths
        },
        "other_tracked_diff_name_status": git("diff", "--name-status", "--", ":!" + OLD).decode().splitlines(),
        "status_short": git("status", "--short", "--untracked-files=all").decode().splitlines(),
    }
    (OUT / f"worktree-{sys.argv[1]}.json").write_text(json.dumps(snapshot, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({
        "old_tracked_count": snapshot["old_tracked_count"],
        "old_deleted_count": snapshot["old_deleted_count"],
        "new_untracked_count": len(new_paths),
        "other_tracked_diff_name_status": snapshot["other_tracked_diff_name_status"],
    }, ensure_ascii=False))


if __name__ == "__main__":
    main()

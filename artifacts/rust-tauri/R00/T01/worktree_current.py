#!/usr/bin/env python3
"""只读收集现行 A02 工作树，不写验收结果。"""

import hashlib
import json
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[4]
OLD = "Lingxi_Refactor_Taskbooks_2026-09-21"
NEW = "Lingxi_Rust_Tauri_Taskbooks_2026-09-23"


def git(root, *args):
    return subprocess.check_output(["git", *args], cwd=root)


def collect(root=ROOT):
    old_paths = [name for name in git(root, "ls-files", "-z", "--", OLD).decode().split("\0") if name]
    new_paths = sorted(path for path in (root / NEW).rglob("*") if path.is_file())
    new_untracked = [name for name in git(root, "ls-files", "--others", "--exclude-standard", "-z", "--", NEW).decode().split("\0") if name]
    return {
        "head": git(root, "rev-parse", "HEAD").decode().strip(),
        "branch": git(root, "branch", "--show-current").decode().strip(),
        "old_tracked_paths": sorted(old_paths),
        "old_deleted_paths": sorted(path for path in old_paths if not (root / path).exists()),
        "old_diff_sha256": hashlib.sha256(git(root, "diff", "--binary", "--", OLD)).hexdigest(),
        "new_files": {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest() for path in new_paths},
        "new_untracked_paths": sorted(new_untracked),
        "staged_diff_sha256": hashlib.sha256(git(root, "diff", "--cached", "--binary")).hexdigest(),
        "staged_name_status": git(root, "diff", "--cached", "--name-status").decode().splitlines(),
        "other_tracked_diff_sha256": hashlib.sha256(git(root, "diff", "--binary", "--", ":!" + OLD)).hexdigest(),
        "other_tracked_name_status": git(root, "diff", "--name-status", "--", ":!" + OLD).decode().splitlines(),
    }


def verify_first_snapshot(root, before, current):
    empty_hash = hashlib.sha256(b"").hexdigest()
    checks = {
        "head_unchanged": current["head"] == before["candidate_sha"],
        "branch_unchanged": current["branch"] == before["branch"],
        "old_tracked_count_unchanged": len(current["old_tracked_paths"]) == before["old_tracked_count"],
        "old_deleted_count_unchanged": len(current["old_deleted_paths"]) == before["old_deleted_count"],
        "old_diff_unchanged": current["old_diff_sha256"] == before["old_diff_sha256"],
        "new_untracked_files_unchanged": current["new_files"] == before["new_untracked_files"],
        "new_taskbook_untracked_set_unchanged": current["new_untracked_paths"] == sorted(before["new_untracked_files"]),
        "staged_diff_absent": current["staged_diff_sha256"] == empty_hash and not current["staged_name_status"],
        "other_tracked_diff_absent": current["other_tracked_diff_sha256"] == empty_hash and not current["other_tracked_name_status"],
    }
    return checks


def fingerprint(current):
    return hashlib.sha256(json.dumps(current, sort_keys=True, ensure_ascii=False).encode()).hexdigest()

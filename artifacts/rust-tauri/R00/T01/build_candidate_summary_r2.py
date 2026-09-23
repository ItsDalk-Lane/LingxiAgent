#!/usr/bin/env python3
"""动态封存 R00-T01 候选；审查报告也属于候选。"""

import hashlib
import json
import pathlib
import stat
import subprocess

import worktree_current
from datetime import datetime, timezone

ROOT = pathlib.Path(__file__).resolve().parents[4]
OUT = pathlib.Path(__file__).resolve().parent
SCOPE = (ROOT / "docs/rust-tauri/R00", OUT)
# 摘要无法包含自身；独立校验输出不属于被校验输入。
EXCLUDED = {"artifacts/rust-tauri/R00/T01/candidate-summary-r2.json",
            "artifacts/rust-tauri/R00/T01/candidate-verify-r4.json"}


def candidate_paths():
    paths = set()
    for directory in SCOPE:
        for path in directory.rglob("*"):
            relative = path.relative_to(ROOT).as_posix()
            if path.is_dir() and not path.is_symlink():
                continue
            if relative in EXCLUDED:
                continue
            if path.is_symlink() or not path.is_file() or not stat.S_ISREG(path.lstat().st_mode):
                raise RuntimeError(f"unexpected candidate entry: {relative}")
            paths.add(relative)
    return paths


def hash_file(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def aggregate(files):
    digest = hashlib.sha256()
    for name, file_hash in sorted(files.items()):
        digest.update(f"{name}\0{file_hash}\n".encode())
    return digest.hexdigest()


def main():
    original = json.loads((OUT / "worktree-before.json").read_text())
    current_before = worktree_current.collect(ROOT)
    checks = worktree_current.verify_first_snapshot(ROOT, original, current_before)
    if not all(checks.values()):
        raise RuntimeError(f"preexisting worktree changed: {checks}")
    files = {name: hash_file(ROOT / name) for name in sorted(candidate_paths())}
    current_after = worktree_current.collect(ROOT)
    if current_after != current_before:
        raise RuntimeError("worktree changed while building candidate")
    worktree = json.loads((OUT / "worktree-preservation-r2.json").read_text())
    if not worktree["all_preserved"] or worktree.get("current_worktree_fingerprint") != worktree_current.fingerprint(current_before):
        raise RuntimeError("A02 saved result does not match live worktree")
    result = {
        "captured_utc": datetime.now(timezone.utc).isoformat(),
        "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "branch": subprocess.check_output(["git", "branch", "--show-current"], cwd=ROOT, text=True).strip(),
        "scope": [directory.relative_to(ROOT).as_posix() for directory in SCOPE],
        "excluded": sorted(EXCLUDED),
        "candidate_file_count": len(files),
        "candidate_files": files,
        "candidate_sha256": aggregate(files),
        "preexisting_worktree_preserved": worktree["all_preserved"],
        "current_worktree_fingerprint": worktree_current.fingerprint(current_before),
        "preexisting_old_deletions": worktree["old_deleted_count"],
        "preexisting_new_taskbook_files": worktree["new_untracked_count"],
        "r00_a01": json.loads((OUT / "R00-A01.result.json").read_text())["status"],
        "r00_a02": json.loads((OUT / "R00-A02.result.json").read_text())["status"],
    }
    (OUT / "candidate-summary-r2.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()

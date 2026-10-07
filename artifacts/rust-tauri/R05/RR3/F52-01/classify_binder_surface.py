#!/usr/bin/env python3
"""F52 binder-surface full reclassification (read-only evidence tool).

Reimplements the candidate binding traversal semantics of
rust/crates/xtask/src/candidate.rs (component-wise lstat walk):

  - enumeration: `git ls-files --cached --others --exclude-standard -z`
    run at the repository root (same argv the candidate binder uses);
  - for each enumerated entry, every path component from the repo root is
    lstat'ed (symlink_metadata, i.e. no follow):
      * any component is a symlink/reparse point -> "crosses a symlink"
        (recorded as ancestor_symlink, or final_symlink when it is the
        last component);
      * a non-final component that is not a directory -> ancestor_not_dir;
      * a missing/inaccessible component -> missing;
      * the final component: regular file -> regular_file; directory
        (trailing-slash git entry) -> directory_entry; anything else
        (fifo/socket/device/whiteout) -> irregular.

Usage:
  classify_binder_surface.py <repo_root> <output_json> [--assert-clean]

With --assert-clean the process exits 1 unless every enumerated entry
classifies as regular_file (zero directory/symlink/ancestor/irregular/
missing).  The repository is never modified; git is invoked with
--no-optional-locks.
"""

from __future__ import annotations

import hashlib
import json
import os
import stat
import subprocess
import sys
import time
from datetime import datetime, timezone


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def classify_entry(root: bytes, rel: bytes) -> tuple[str, str]:
    """Classify one enumerated entry; returns (class, detail_path)."""
    walked = root
    parts = rel.split(b"/") if b"/" in rel else [rel]
    # A git directory entry carries a trailing slash; drop empty tail.
    parts = [p for p in parts if p != b""]
    for idx, name in enumerate(parts):
        walked = os.path.join(walked, name)
        try:
            st = os.lstat(walked)
        except OSError as exc:
            return "missing", f"{os.fsdecode(walked)}: {exc.strerror}"
        mode = st.st_mode
        if stat.S_ISLNK(mode):
            if idx == len(parts) - 1:
                return "final_symlink", os.fsdecode(walked)
            return "ancestor_symlink", os.fsdecode(walked)
        final = idx == len(parts) - 1
        if not final:
            if not stat.S_ISDIR(mode):
                return "ancestor_not_dir", os.fsdecode(walked)
            continue
        if stat.S_ISREG(mode):
            return "regular_file", os.fsdecode(walked)
        if stat.S_ISDIR(mode):
            return "directory_entry", os.fsdecode(walked)
        return "irregular", os.fsdecode(walked)
    return "missing", os.fsdecode(rel) + ": empty path"


def main() -> int:
    if len(sys.argv) < 3:
        sys.stderr.write(__doc__)
        return 2
    repo = os.fsencode(sys.argv[1])
    out_path = sys.argv[2]
    assert_clean = "--assert-clean" in sys.argv[3:]

    started = utc_now()
    t0 = time.monotonic()

    git = "/usr/bin/git"
    argv = [
        git, "--no-optional-locks", "ls-files",
        "--cached", "--others", "--exclude-standard", "-z",
    ]
    proc = subprocess.run(argv, cwd=os.fsdecode(repo), capture_output=True)
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr.decode("utf-8", "replace"))
        return 3

    entries = proc.stdout.split(b"\0")
    if entries and entries[-1] == b"":
        entries.pop()

    counts: dict[str, int] = {}
    problems: list[dict[str, str]] = []
    for rel in entries:
        cls, detail = classify_entry(repo, rel)
        counts[cls] = counts.get(cls, 0) + 1
        if cls != "regular_file" and len(problems) < 200:
            problems.append({
                "class": cls,
                "path": os.fsdecode(rel),
                "detail": detail,
            })

    elapsed = time.monotonic() - t0
    total = len(entries)
    clean = (
        total > 0
        and counts.get("regular_file", 0) == total
        and all(counts.get(k, 0) == 0 for k in (
            "final_symlink", "ancestor_symlink", "directory_entry",
            "ancestor_not_dir", "irregular", "missing",
        ))
    )

    head_proc = subprocess.run(
        [git, "--no-optional-locks", "rev-parse", "HEAD"],
        cwd=os.fsdecode(repo), capture_output=True,
    )
    record = {
        "tool": "F52 classify_binder_surface.py",
        "semantics": "candidate.rs:326-369 component-wise lstat (symlink_metadata) walk; final component must be a regular file",
        "enumeration_argv": argv,
        "repo_root": os.fsdecode(repo),
        "head": head_proc.stdout.decode().strip(),
        "started_utc": started,
        "finished_utc": utc_now(),
        "elapsed_seconds": round(elapsed, 3),
        "total_entries": total,
        "counts": {k: counts.get(k, 0) for k in (
            "regular_file", "final_symlink", "ancestor_symlink",
            "directory_entry", "ancestor_not_dir", "irregular", "missing",
        )},
        "all_regular_files": counts.get("regular_file", 0) == total,
        "assert_clean": assert_clean,
        "clean": clean,
        "non_regular_samples": problems,
    }
    with open(out_path, "w", encoding="utf-8") as fh:
        json.dump(record, fh, ensure_ascii=False, indent=1, sort_keys=True)
        fh.write("\n")

    print(json.dumps({
        "total": total,
        "counts": record["counts"],
        "clean": clean,
        "elapsed_seconds": record["elapsed_seconds"],
    }, ensure_ascii=False))
    if assert_clean and not clean:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

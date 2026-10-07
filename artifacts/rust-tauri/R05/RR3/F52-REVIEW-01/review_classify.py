#!/usr/bin/env python3
"""F52-REVIEW-01 independent binder-surface classifier (reviewer's own method).

Deliberately a DIFFERENT implementation from the implementer's
classify_binder_surface.py: instead of re-walking every path component for
every entry, this tool lstats each entry's final component directly and
checks ancestors through a per-distinct-directory cache (each unique
ancestor directory is lstat'ed exactly once).  Semantics target is the same
contract read independently from rust/crates/xtask/src/candidate.rs
(component-wise symlink_metadata walk; symlink anywhere -> reject; final
component must be a regular file).

Classes: regular_file / final_symlink / ancestor_symlink / directory_entry /
ancestor_not_dir / irregular / missing.  Exit 1 unless 100% regular_file.

Usage: review_classify.py <repo_root> <out_json> [--assert-clean]
"""
from __future__ import annotations

import json
import os
import stat
import subprocess
import sys
import time
from datetime import datetime, timezone

CLASSES = ("regular_file", "final_symlink", "ancestor_symlink",
           "directory_entry", "ancestor_not_dir", "irregular", "missing")


def main() -> int:
    if len(sys.argv) < 3:
        sys.stderr.write(__doc__)
        return 2
    repo = sys.argv[1]
    out_path = sys.argv[2]
    assert_clean = "--assert-clean" in sys.argv[3:]

    t0 = time.monotonic()
    started = datetime.now(timezone.utc).isoformat()

    proc = subprocess.run(
        ["/usr/bin/git", "--no-optional-locks", "ls-files",
         "--cached", "--others", "--exclude-standard", "-z"],
        cwd=repo, capture_output=True,
    )
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr.decode("utf-8", "replace"))
        return 3
    entries = [e for e in proc.stdout.split(b"\0") if e]

    counts = {c: 0 for c in CLASSES}
    offenders: list[dict] = []
    dir_cache: dict[bytes, str | None] = {}  # ancestor dir -> None(ok) | class

    for rel in entries:
        parts = rel.split(b"/")
        cls = "regular_file"
        # ancestors (all but last component), cached per distinct directory
        prefix = b""
        for p in parts[:-1]:
            prefix = prefix + p
            hit = dir_cache.get(prefix)
            if hit is None:
                # not yet classified: lstat this ancestor directory
                st = os.lstat(os.path.join(repo, os.fsdecode(prefix)))
                if stat.S_ISLNK(st.st_mode):
                    hit = "ancestor_symlink"
                elif not stat.S_ISDIR(st.st_mode):
                    hit = "ancestor_not_dir"
                else:
                    hit = ""  # marker for "checked, fine" (empty string)
                dir_cache[prefix] = hit
            if hit:
                cls = hit
                break
            prefix += b"/"
        if cls == "regular_file":
            final = os.path.join(repo, os.fsdecode(rel))
            try:
                st = os.lstat(final)
            except OSError:
                cls = "missing"
            else:
                if stat.S_ISLNK(st.st_mode):
                    cls = "final_symlink"
                elif stat.S_ISDIR(st.st_mode):
                    cls = "directory_entry"
                elif not stat.S_ISREG(st.st_mode):
                    cls = "irregular"
        counts[cls] += 1
        if cls != "regular_file" and len(offenders) < 200:
            offenders.append({"class": cls, "path": os.fsdecode(rel)})

    total = len(entries)
    clean = total > 0 and counts["regular_file"] == total
    rec = {
        "tool": "F52-REVIEW-01 review_classify.py (independent method: "
                "final-component lstat + ancestor dir cache)",
        "repo_root": repo,
        "started_utc": started,
        "finished_utc": datetime.now(timezone.utc).isoformat(),
        "elapsed_seconds": round(time.monotonic() - t0, 3),
        "total_entries": total,
        "counts": counts,
        "distinct_ancestor_dirs_lstat_cached": len(dir_cache),
        "clean": clean,
        "assert_clean": assert_clean,
        "offenders": offenders,
    }
    with open(out_path, "w", encoding="utf-8") as fh:
        json.dump(rec, fh, ensure_ascii=False, indent=1, sort_keys=True)
        fh.write("\n")
    print(json.dumps({"total": total, "counts": counts, "clean": clean,
                      "elapsed_s": rec["elapsed_seconds"]}))
    return 1 if (assert_clean and not clean) else 0


if __name__ == "__main__":
    sys.exit(main())

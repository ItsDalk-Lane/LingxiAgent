#!/usr/bin/env python3
"""F52-REVIEW-01 binder simulation per candidate.rs semantics (reviewer's own
reading of rust/crates/xtask/src/candidate.rs ~326-369):

For each enumerated entry (git ls-files --cached --others --exclude-standard -z
at repo root), component-wise symlink_metadata walk:
  - any component linklike -> "candidate path {abs} crosses a symlink or
    reparse point"
  - non-final component not a dir -> "candidate parent {abs} is not a
    directory"
  - missing -> "candidate path {abs} is missing or inaccessible: {err}"
  - final not a regular file -> "candidate file {abs} was replaced by a
    non-file"
  - else: sha256 of contents (binder hashes the file).
The first offending entry aborts, exactly like the real binder (fail-closed,
one candidate at a time); messages are prefixed with the caller-level
"error: cannot snapshot candidate before stage gate: ".

Usage: binder_sim.py <repo_root>
Exit 0 = all entries bindable; 1 = rejection reproduced.
"""
from __future__ import annotations

import hashlib
import os
import stat
import subprocess
import sys


def bind(root: str, rel: str) -> tuple[str, str]:
    walked = root
    parts = [p for p in rel.split("/") if p]
    for idx, name in enumerate(parts):
        walked = os.path.join(walked, name)
        try:
            st = os.lstat(walked)
        except OSError as exc:
            return ("REJECT", f"candidate path {walked} is missing or "
                             f"inaccessible: {exc.strerror}")
        if stat.S_ISLNK(st.st_mode):
            return ("REJECT", f"candidate path {walked} crosses a symlink or "
                              f"reparse point")
        if idx < len(parts) - 1:
            if not stat.S_ISDIR(st.st_mode):
                return ("REJECT", f"candidate parent {walked} is not a "
                                  f"directory")
            continue
        if not stat.S_ISREG(st.st_mode):
            return ("REJECT", f"candidate file {walked} was replaced by a "
                              f"non-file")
        h = hashlib.sha256()
        with open(walked, "rb") as fh:
            for chunk in iter(lambda: fh.read(65536), b""):
                h.update(chunk)
        return ("OK", f"{rel} sha256={h.hexdigest()}")
    return ("REJECT", f"candidate path {rel} empty")


def main() -> int:
    repo = sys.argv[1]
    proc = subprocess.run(
        ["/usr/bin/git", "--no-optional-locks", "ls-files",
         "--cached", "--others", "--exclude-standard", "-z"],
        cwd=repo, capture_output=True)
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr.decode("utf-8", "replace"))
        return 3
    entries = [e for e in proc.stdout.split(b"\0") if e]
    rejected = 0
    for rel in entries:
        rel_s = rel.decode("utf-8", "surrogateescape")
        status, detail = bind(repo, rel_s)
        if status == "OK":
            continue
        rejected += 1
        print("error: cannot snapshot candidate before stage gate: " + detail)
        break  # binder aborts at the first offending candidate
    if rejected:
        print(f"BINDER-REJECTION-REPRODUCED (1 of {len(entries)} entries)")
        return 1
    print(f"ALL {len(entries)} ENTRIES BINDABLE (regular files, hashable)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

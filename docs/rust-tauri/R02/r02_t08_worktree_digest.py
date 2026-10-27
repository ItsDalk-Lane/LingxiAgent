#!/usr/bin/env python3
"""R02-T08 working-tree digest (R01_HANDOFF.json scheme, R02 scope).

SHA-256 over lines of "sha256  <path>" for:
  - every git-tracked file's WORKING TREE content (not the committed blob),
  - every untracked non-ignored file (new deliverables count),
  - plus *.log evidence files under artifacts/rust-tauri/R02/T08/ that are
    gitignored by the artifacts rule (raw logs are evidence, they count),
sorted by path, LF-terminated lines.
Excluded (self-reference avoidance): R02_HANDOFF.json, this script's own
output detail file, verify-stage command stdout/stderr logs are INCLUDED
(they are evidence), but nothing else is special-cased.

Usage: python3 r02_t08_worktree_digest.py [--detail OUT_FILE]
Prints "digest <hex>" and, with --detail, writes the per-line detail.
"""
import hashlib
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(sys.argv[sys.argv.index("--detail") + 1]) if "--detail" in sys.argv else None

EXCLUDE = {
    "docs/rust-tauri/R02/R02_HANDOFF.json",
    "docs/rust-tauri/R02/R02_REPORT.md",  # embeds the digest value itself
    "artifacts/rust-tauri/R02/T08/working-tree-digest.txt",
}


def _git_ls(args: list[str]) -> list[str]:
    # -z: never quote non-ASCII paths; NUL-separated
    out = subprocess.run(
        ["git", "-c", "core.quotepath=false", "ls-files", *args, "-z"],
        cwd=ROOT, capture_output=True, check=True,
    )
    return [p for p in out.stdout.decode("utf-8").split("\0") if p]


def tracked() -> list[str]:
    return _git_ls([])


def untracked() -> list[str]:
    return _git_ls(["--others", "--exclude-standard"])


def ignored_logs() -> list[str]:
    return [
        p for p in _git_ls(["--others", "-i", "--exclude-standard", "--",
                            "artifacts/rust-tauri/R02/T08/"])
        if p.endswith(".log") or p.endswith(".txt") or p.endswith(".json")
    ]


entries: list[tuple[str, Path]] = []
for p in tracked() + untracked() + ignored_logs():
    if p in EXCLUDE:
        continue
    entries.append((p, ROOT / p))
entries.sort()

lines = []
for rel, path in entries:
    data = path.read_bytes()
    lines.append(f"{hashlib.sha256(data).hexdigest()}  {rel}")
blob = "".join(line + "\n" for line in lines)
digest = hashlib.sha256(blob.encode()).hexdigest()
if OUT:
    OUT.write_text(blob)
print(f"entries: {len(lines)}")
print(f"digest {digest}")

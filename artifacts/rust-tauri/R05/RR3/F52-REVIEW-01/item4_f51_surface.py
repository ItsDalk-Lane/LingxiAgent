#!/usr/bin/env python3
"""F52-REVIEW-01 item 4: F51 relocation surface untouched check.

1. Destination root must contain exactly the 56 F51 entry names (read from
   F51-01/RELOCATION-RECEIPT.json) plus the single new F52 entry name.
2. Nothing under the destination root (excluding the new F52 entry) may have
   mtime newer than F52 start (2026-10-07 21:00:36 local == 13:00:36Z).
3. In the main tree: exactly 41 RELOCATED-F51.json markers (all older than
   F52 start) and exactly 1 RELOCATED-F52.json marker.

Exit 0 = PASS, 1 = FAIL.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
from datetime import datetime

REPO = "/Users/study_superior/Desktop/Code/LingxiAgent"
DEST = "/Users/study_superior/Desktop/Code/LingxiAgent-RR3-localonly-fixtures"
F51_RECEIPT = os.path.join(
    REPO, "artifacts/rust-tauri/R05/RR3/F51-01/RELOCATION-RECEIPT.json")
F52_ENTRY = "artifacts_rust_tauri_R05_RR3_A-REVIEW-02_independent-validator-bin"
F52_START_LOCAL = datetime(2026, 10, 7, 21, 0, 36)

FAILS: list[str] = []


def fail(m: str) -> None:
    FAILS.append(m)
    print(f"FAIL: {m}")


def main() -> int:
    receipt = json.load(open(F51_RECEIPT, encoding="utf-8"))
    f51_names = set()
    for ent in receipt.get("entries", []):
        f51_names.add(os.path.basename(ent["new_absolute_path"].rstrip("/")))
    print(f"F51 receipt entries: {len(f51_names)}")

    now = set(os.listdir(DEST))
    extra = sorted(now - f51_names - {F52_ENTRY})
    missing = sorted(f51_names - now)
    print(f"destination root entries now: {len(now)}")
    if extra:
        fail(f"unexpected destination entries beyond F51-56 + F52-1: {extra}")
    if missing:
        fail(f"missing F51 destination entries: {missing}")
    if now - f51_names != {F52_ENTRY}:
        fail(f"new-entry set != {{F52 entry}}: {now - f51_names}")
    else:
        print("destination = exactly F51-56 + F52-1: OK")

    # post-F52-start writes under destination, excluding the new entry
    proc = subprocess.run(
        ["/usr/bin/find", DEST, "-newermt", "2026-10-07 21:00:36"],
        capture_output=True, text=True)
    hits = [p for p in proc.stdout.splitlines() if p and
            not p.startswith(os.path.join(DEST, F52_ENTRY)) and p != DEST]
    print(f"post-F52-start writes in destination (excl new entry, excl root): "
          f"{len(hits)}")
    for h in hits[:10]:
        print(f"  HIT {h}")
    if hits:
        fail(f"{len(hits)} paths under F51 destinations modified after F52 start")

    # markers in main tree
    proc = subprocess.run(
        ["/usr/bin/find", os.path.join(REPO, "artifacts/rust-tauri/R05/RR3"),
         "-name", "RELOCATED-F51.json"],
        capture_output=True, text=True)
    markers = proc.stdout.splitlines()
    print(f"RELOCATED-F51.json markers: {len(markers)}")
    if len(markers) != 41:
        fail(f"expected 41 RELOCATED-F51 markers, got {len(markers)}")
    recent = []
    for m in markers:
        mt = datetime.fromtimestamp(os.path.getmtime(m))
        if mt >= F52_START_LOCAL:
            recent.append((m, mt))
    if recent:
        fail(f"F51 markers touched during/after F52: {recent}")
    else:
        print("all 41 F51 markers older than F52 start: OK")

    f52_markers = [p for p in subprocess.run(
        ["/usr/bin/find", os.path.join(REPO, "artifacts/rust-tauri/R05/RR3"),
         "-name", "RELOCATED-F52.json"], capture_output=True, text=True
    ).stdout.splitlines()]
    print(f"RELOCATED-F52.json markers: {len(f52_markers)} -> {f52_markers}")
    if len(f52_markers) != 1:
        fail(f"expected exactly 1 RELOCATED-F52 marker, got {len(f52_markers)}")

    if FAILS:
        print(f"RESULT: FAIL ({len(FAILS)})")
        return 1
    print("RESULT: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())

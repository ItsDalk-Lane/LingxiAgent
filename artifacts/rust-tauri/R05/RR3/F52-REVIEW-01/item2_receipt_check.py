#!/usr/bin/env python3
"""F52-REVIEW-01 relocation receipt comparator (reviewer's own).

Independently recomputes, at the receipt's new_absolute_path:
  - git          : sha256 + lstat fields
  - python3      : readlink target string + lstat fields
and compares them field-by-field with the receipt's pre/post records.
Equality criteria are inode-INDEPENDENT (sha256 / readlink target /
mode/uid/gid/size/mtime_ns; dev informational; ino reported as an
observation only, not a pass criterion).
Also validates the in-place marker RELOCATED-F52.json.

Usage:
  item2_receipt_check.py <repo_root> <receipt.json> <marker.json>
                         [--copy-mode]
Exit 0 = all equal; 1 = any mismatch; 2 = structural failure.
--copy-mode skips only the "receipt file is at its declared marker path"
identity check (used when checking a scratch COPY of the receipt in the
tamper self-control; the original item-2 run does NOT use it).
"""
from __future__ import annotations

import hashlib
import json
import os
import stat
import sys

FAIL: list[str] = []


def fail(msg: str) -> None:
    FAIL.append(msg)
    print(f"MISMATCH: {msg}")


def sha256_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(131072), b""):
            h.update(chunk)
    return h.hexdigest()


def lstat_fields(path: str) -> dict:
    st = os.lstat(path)
    return {
        "mode": f"0o{st.st_mode:o}",  # includes type bits (0o100755 / 0o120755)
        "perms": f"0o{stat.S_IMODE(st.st_mode):o}",
        "uid": st.st_uid,
        "gid": st.st_gid,
        "size": st.st_size,
        "mtime_ns": st.st_mtime_ns,
        "ino": st.st_ino,
        "dev": st.st_dev,
        "is_symlink": stat.S_ISLNK(st.st_mode),
        "is_regular": stat.S_ISREG(st.st_mode),
    }


def main() -> int:
    repo, receipt_path, marker_path = sys.argv[1], sys.argv[2], sys.argv[3]
    copy_mode = "--copy-mode" in sys.argv[4:]
    receipt = json.load(open(receipt_path, encoding="utf-8"))

    entries = receipt.get("entries", [])
    if len(entries) != 1:
        fail(f"expected exactly 1 entry, got {len(entries)}")
        return 2
    ent = entries[0]
    new_dir = ent["new_absolute_path"]
    if not os.path.isdir(new_dir):
        fail(f"new dir missing: {new_dir}")
        return 2

    names = sorted(os.listdir(new_dir))
    print(f"new-dir listing: {names}")
    if names != ["git", "python3"]:
        fail(f"new-dir contents unexpected: {names}")

    for phase in ("pre", "post"):
        rec = ent[phase]
        # --- git (regular file) ---
        p = os.path.join(new_dir, "git")
        got = lstat_fields(p)
        want = rec["git"]["lstat"]
        if not got["is_regular"]:
            fail(f"[{phase}] git not a regular file at new location")
        digest = sha256_file(p)
        if digest != rec["git"]["sha256"]:
            fail(f"[{phase}] git sha256 {digest} != receipt {rec['git']['sha256']}")
        else:
            print(f"[{phase}] git sha256 EQUAL {digest}")
        for f in ("mode", "uid", "gid", "size", "mtime_ns", "dev"):
            if str(want[f]) != str(got[f]):
                fail(f"[{phase}] git lstat.{f}: now {got[f]} != receipt {want[f]}")
        # --- python3 (symlink) ---
        p = os.path.join(new_dir, "python3")
        got = lstat_fields(p)
        want = rec["python3"]["lstat"]
        if not got["is_symlink"]:
            fail(f"[{phase}] python3 not a symlink at new location")
        target = os.readlink(p)
        if target != rec["python3"]["readlink_target"]:
            fail(f"[{phase}] python3 readlink {target!r} != receipt "
                 f"{rec['python3']['readlink_target']!r}")
        else:
            print(f"[{phase}] python3 readlink EQUAL {target!r}")
        for f in ("uid", "gid", "size", "mtime_ns", "dev"):
            if str(want[f]) != str(got[f]):
                fail(f"[{phase}] python3 lstat.{f}: now {got[f]} != receipt {want[f]}")
        if str(want["mode"]) != got["mode"]:
            fail(f"[{phase}] python3 lstat.mode: now {got['mode']} != "
                 f"receipt {want['mode']} (perms {got['perms']})")
        # inode observation (NOT a pass criterion; identity info only)
        g_ino = lstat_fields(os.path.join(new_dir, "git"))["ino"]
        s_ino = lstat_fields(os.path.join(new_dir, "python3"))["ino"]
        print(f"[{phase}] inode observation: git {g_ino} / python3 {s_ino} "
              f"(receipt ino: git {ent['pre']['git']['lstat']['ino']}, "
              f"python3 {ent['pre']['python3']['lstat']['ino']})")

    # --- marker ---
    marker = json.load(open(marker_path, encoding="utf-8"))
    child = marker["relocated_children"][0]
    checks = {
        "marker field": marker.get("marker") == "RELOCATED-F52",
        "marker receipt path": os.path.exists(os.path.join(repo, marker["receipt"])),
        "child name": child["name"],
        "new abs path matches receipt": child["new_absolute_path"] == new_dir,
        "old rel path matches receipt":
            child["old_relative_path"] == ent["old_relative_path"],
        "old rel path no longer exists":
            not os.path.exists(os.path.join(repo, ent["old_relative_path"])),
        "receipt path is the F52 receipt": True if copy_mode else
            os.path.abspath(receipt_path) == os.path.join(repo, marker["receipt"]),
    }
    for k, v in checks.items():
        if v is False:
            fail(f"marker check failed: {k}")
        else:
            print(f"marker OK: {k}")

    if FAIL:
        print(f"RESULT: FAIL ({len(FAIL)} mismatches)")
        return 1
    print("RESULT: PASS (all sha256/readlink/lstat fields equal; inode-independent)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

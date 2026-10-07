#!/usr/bin/env python3
"""F52-REVIEW-01 item 6: tamper-detection self-controls.

A. Receipt tamper: copy the F52 receipt, flip one hex digit of the git
   entry's sha256, re-run the reviewer's comparator -> must FAIL (exit 1).
   Untampered copy -> must PASS (exit 0).
B. Classification tamper: copy binder-surface-post-relocation.json, lower
   counts.regular_file by 1 (total kept) -> the reviewer's consistency
   checker must detect (sum != total). Untampered copy -> OK.

Exit 0 = all four sub-controls behave as required.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys

REPO = "/Users/study_superior/Desktop/Code/LingxiAgent"
REV = f"{REPO}/artifacts/rust-tauri/R05/RR3/F52-REVIEW-01"
SCRATCH = "/private/tmp/f52-review-01-scratch/tamper"
RECEIPT = f"{REPO}/artifacts/rust-tauri/R05/RR3/F52-01/RELOCATION-RECEIPT-F52.json"
MARKER = f"{REPO}/artifacts/rust-tauri/R05/RR3/A-REVIEW-02/RELOCATED-F52.json"
CLASS_JSON = f"{REV}/../F52-01/binder-surface-post-relocation.json"


def consistency_check(path: str) -> tuple[bool, str]:
    d = json.load(open(path, encoding="utf-8"))
    counts = d["counts"]
    total = d["total_entries"]
    s = sum(counts.values())
    if s != total:
        return False, f"sum(counts)={s} != total_entries={total}"
    non_reg = sum(v for k, v in counts.items() if k != "regular_file")
    if d["clean"] != (non_reg == 0 and counts["regular_file"] == total):
        return False, f"clean={d['clean']} inconsistent with counts {counts}"
    return True, f"consistent (total={total}, regular={counts['regular_file']})"


def main() -> int:
    os.makedirs(SCRATCH, exist_ok=True)
    ok = True
    # --- A: receipt tamper ---
    tampered = f"{SCRATCH}/receipt-tampered.json"
    pristine = f"{SCRATCH}/receipt-copy.json"
    shutil.copy(RECEIPT, tampered)
    shutil.copy(RECEIPT, pristine)
    d = json.load(open(tampered, encoding="utf-8"))
    old = d["entries"][0]["post"]["git"]["sha256"]
    new = ("f" if old[0] != "f" else "e") + old[1:]
    d["entries"][0]["post"]["git"]["sha256"] = new
    json.dump(d, open(tampered, "w", encoding="utf-8"), indent=1)
    print(f"tampered receipt sha256 {old[:8]}… -> {new[:8]}…")

    for label, path, want in (("tampered", tampered, 1),
                              ("pristine", pristine, 0)):
        proc = subprocess.run(
            [sys.executable, f"{REV}/item2_receipt_check.py",
             REPO, path, MARKER, "--copy-mode"],
            capture_output=True, text=True)
        got = proc.returncode
        verdict = "BEHAVED-AS-REQUIRED" if got == want else "control broken"
        print(f"[A/{label}] comparator exit {got} (want {want}) -> {verdict}"
              + ("  TAMPER-DETECTED" if label == "tampered" and got == want
                 else ""))
        tail = [l for l in proc.stdout.splitlines()
                if l.startswith(("MISMATCH", "RESULT"))]
        for l in tail[:4]:
            print(f"    {l}")
        if got != want:
            ok = False

    # --- B: classification tamper ---
    tpath = f"{SCRATCH}/binder-surface-post-relocation-tampered.json"
    ppath = f"{SCRATCH}/binder-surface-post-relocation-copy.json"
    shutil.copy(CLASS_JSON, tpath)
    shutil.copy(CLASS_JSON, ppath)
    d = json.load(open(tpath, encoding="utf-8"))
    d["counts"]["regular_file"] -= 1
    json.dump(d, open(tpath, "w", encoding="utf-8"), indent=1)
    print(f"tampered classification regular_file 69130 -> "
          f"{d['counts']['regular_file']} (total kept {d['total_entries']})")

    for label, path, want_ok in (("tampered", tpath, False),
                                 ("pristine", ppath, True)):
        good, msg = consistency_check(path)
        verdict = ("TAMPER-DETECTED" if (label == "tampered" and not good)
                   else "OK-AS-EXPECTED" if good == want_ok
                   else "control broken")
        print(f"[B/{label}] consistency: {msg} -> {verdict}")
        if good != want_ok:
            ok = False

    print("RESULT: " + ("PASS (tampering detectable both axes)" if ok
                        else "FAIL"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())

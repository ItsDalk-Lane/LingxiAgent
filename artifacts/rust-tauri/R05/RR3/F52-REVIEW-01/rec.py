#!/usr/bin/env python3
"""F52-REVIEW-01 command recorder.

Wraps a shell command, records label/cmd/cwd/UTC start->stop/exit plus raw
stdout/stderr files (with SHA256) into F52-REVIEW-01/, and appends one JSONL
line per run to commands.jsonl.  Usage:

  rec.py <label> --cwd <dir> -- <command...>

stdout/stderr land in out-<label>.txt / err-<label>.txt inside this
directory.  Exit code of the wrapped command is propagated (unless
--propagate-off, then always 0).
"""
from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
import time
from datetime import datetime, timezone


def utc(ts: float) -> str:
    return datetime.fromtimestamp(ts, timezone.utc).isoformat()


def sha256_file(path: str) -> str | None:
    if not os.path.exists(path):
        return None
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(131072), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    argv = sys.argv[1:]
    label = argv[0]
    cwd = os.getcwd()
    propagate = True
    rest: list[str] = []
    i = 1
    while i < len(argv):
        a = argv[i]
        if a == "--cwd":
            cwd = argv[i + 1]
            i += 2
        elif a == "--propagate-off":
            propagate = False
            i += 1
        elif a == "--":
            rest = argv[i + 1:]
            break
        else:
            rest = argv[i:]
            break
    here = os.path.dirname(os.path.abspath(__file__))
    out_path = os.path.join(here, f"out-{label}.txt")
    err_path = os.path.join(here, f"err-{label}.txt")
    t0 = time.time()
    with open(out_path, "wb") as out, open(err_path, "wb") as err:
        proc = subprocess.run(rest, cwd=cwd, stdout=out, stderr=err)
    t1 = time.time()
    rec = {
        "label": label,
        "argv": rest,
        "cwd": cwd,
        "utc_start": utc(t0),
        "utc_stop": utc(t1),
        "exit": proc.returncode,
        "stdout_file": os.path.basename(out_path),
        "stdout_sha256": sha256_file(out_path),
        "stderr_file": os.path.basename(err_path),
        "stderr_sha256": sha256_file(err_path),
    }
    with open(os.path.join(here, "commands.jsonl"), "a", encoding="utf-8") as fh:
        fh.write(json.dumps(rec, ensure_ascii=False, sort_keys=True) + "\n")
    return proc.returncode if propagate else 0


if __name__ == "__main__":
    sys.exit(main())

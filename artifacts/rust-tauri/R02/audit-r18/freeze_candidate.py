#!/usr/bin/env python3
"""Freeze the current nonignored candidate and compare it with the R16 opening snapshot."""

import datetime
import hashlib
import json
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[4]
OUT = pathlib.Path(__file__).resolve().parent
BASE = ROOT / "artifacts/rust-tauri/R02/audit-r16/prechange-full-sha256-corrected.txt"
OUTPUT_NAMES = (
    "candidate-final-full-sha256.txt",
    "candidate-final-status.txt",
    "pre-to-final-diff.json",
)
OUTPUT_PATHS = {str((OUT / name).relative_to(ROOT)) for name in OUTPUT_NAMES}


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT)


def original():
    result = {}
    for line in BASE.read_text().splitlines():
        digest, path = line.split("  ", 1)
        result[path] = digest
    return result


def current():
    names = git("ls-files", "-c", "-o", "--exclude-standard", "-z").split(b"\0")
    result = {}
    skipped = []
    for raw in names:
        if not raw:
            continue
        name = raw.decode("utf-8", "surrogateescape")
        if name in OUTPUT_PATHS:
            continue
        path = ROOT / name
        if not path.is_file():
            skipped.append(name)
            continue
        result[name] = hashlib.sha256(path.read_bytes()).hexdigest()
    return result, skipped


def main():
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    before = original()
    after, skipped = current()
    manifest = "".join(f"{digest}  {name}\n" for name, digest in sorted(after.items()))
    (OUT / OUTPUT_NAMES[0]).write_text(manifest)
    status = git("status", "--porcelain=v1", "--untracked-files=all").decode("utf-8", "surrogateescape")
    (OUT / OUTPUT_NAMES[1]).write_text(status)
    added = sorted(after.keys() - before.keys())
    removed = sorted(before.keys() - after.keys())
    changed = sorted(name for name in before.keys() & after.keys() if before[name] != after[name])
    unchanged = len(before.keys() & after.keys()) - len(changed)
    record = {
        "startedUtc": started,
        "endedUtc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "head": git("rev-parse", "HEAD").decode().strip(),
        "branch": git("branch", "--show-current").decode().strip(),
        "baselineManifest": str(BASE.relative_to(ROOT)),
        "baselineCount": len(before),
        "candidateCount": len(after),
        "candidateManifestSha256": hashlib.sha256(manifest.encode()).hexdigest(),
        "added": added,
        "removed": removed,
        "contentChanged": changed,
        "unchangedCount": unchanged,
        "skippedNonRegular": skipped,
        "excludedSelfReferentialOutputs": sorted(OUTPUT_PATHS),
        "statusPath": str((OUT / OUTPUT_NAMES[1]).relative_to(ROOT)),
        "scope": "git tracked plus nonignored untracked files; ignored build/cache directories excluded",
    }
    (OUT / OUTPUT_NAMES[2]).write_text(json.dumps(record, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"baselineCount": len(before), "candidateCount": len(after), "added": len(added), "removed": len(removed), "contentChanged": len(changed), "unchanged": unchanged, "manifestSha256": record["candidateManifestSha256"]}, ensure_ascii=False))


if __name__ == "__main__":
    main()

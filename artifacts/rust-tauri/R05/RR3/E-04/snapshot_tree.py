#!/usr/bin/env python3
"""E-04 树快照工具：枚举候选绑定面全部文件并记录 SHA256/size。

用法：snapshot_tree.py <out.json>
枚举命令与 xtask candidate.rs / FINAL-04 postcheck 相同：
  git --no-optional-locks ls-files --cached --others --exclude-standard -z
仅排除本次 E-04 证据根自身（与 candidate.rs 排除本轮 evidence root 同一语义）。
"""
import hashlib
import json
import os
import subprocess
import sys


def enumerate_files(repo):
    cmd = ["git", "--no-optional-locks", "ls-files", "--cached", "--others",
           "--exclude-standard", "-z"]
    out = subprocess.run(cmd, cwd=repo, capture_output=True, check=True)
    entries = []
    for rec in out.stdout.decode("utf-8").split("\0"):
        if not rec:
            continue
        entries.append(rec)
    return entries


def main():
    repo = "/Users/study_superior/Desktop/Code/LingxiAgent"
    out_path = sys.argv[1]
    skip_prefix = "artifacts/rust-tauri/R05/RR3/E-04/"
    entries = enumerate_files(repo)
    files = {}
    errors = {}
    for rel in entries:
        if rel.endswith("/"):
            errors[rel] = "directory-entry"
            continue
        if rel.startswith(skip_prefix):
            continue
        p = os.path.join(repo, rel)
        try:
            if os.path.islink(p):
                errors[rel] = "symlink"
                continue
            h = hashlib.sha256()
            n = 0
            with open(p, "rb") as fh:
                while True:
                    b = fh.read(1 << 20)
                    if not b:
                        break
                    h.update(b)
                    n += len(b)
            files[rel] = {"sha256": h.hexdigest(), "bytes": n}
        except OSError as e:
            errors[rel] = f"io:{e}"
    doc = {
        "repo": repo,
        "taken_at_utc": subprocess.run(["date", "-u", "+%Y-%m-%dT%H:%M:%SZ"],
                                       capture_output=True, text=True,
                                       check=True).stdout.strip(),
        "enumerated_entries": len(entries),
        "file_count": len(files),
        "error_count": len(errors),
        "errors": errors,
        "files": files,
    }
    with open(out_path, "w", encoding="utf-8") as fh:
        json.dump(doc, fh, ensure_ascii=False, indent=2, sort_keys=True)
        fh.write("\n")
    print(f"snapshot: {len(files)} files, {len(errors)} errors -> {out_path}")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""E-04 MANIFEST 生成（manifest 自身与 tree-snapshot-* 大文件不计入，避免自引用；
快照文件在 changed-files/protected-inputs-equality 中被引用并可复算）。"""
import hashlib
import json
import os
import subprocess

E04 = "/Users/study_superior/Desktop/Code/LingxiAgent/artifacts/rust-tauri/R05/RR3/E-04"
UTC = subprocess.run(["date", "-u", "+%Y-%m-%dT%H:%M:%SZ"], capture_output=True,
                     text=True, check=True).stdout.strip()
EXCLUDE = {"MANIFEST.json", "tree-snapshot-before.json", "tree-snapshot-after.json"}


def sha(p):
    h = hashlib.sha256()
    with open(p, "rb") as fh:
        for b in iter(lambda: fh.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def main():
    rows = []
    for name in sorted(os.listdir(E04)):
        p = os.path.join(E04, name)
        if os.path.isdir(p):
            for root, _dirs, files in os.walk(p):
                for fn in sorted(files):
                    fp = os.path.join(root, fn)
                    rel = os.path.relpath(fp, E04)
                    rows.append({"path": rel, "sha256": sha(fp), "bytes": os.path.getsize(fp)})
        elif name not in EXCLUDE:
            rows.append({"path": name, "sha256": sha(p), "bytes": os.path.getsize(name and p)})
    doc = {
        "round": "RR3/E-04",
        "generated_at_utc": UTC,
        "recorded_by": "rr3_e_impl_04（全新空历史文档实施者；无子代理）",
        "files": rows,
        "notes": [
            "MANIFEST.json 自排除；tree-snapshot-before/after.json（各约 73,078 文件枚举）过大不计入，"
            "由 protected-inputs-equality.json 引用，可用 snapshot_tree.py 复算核验。",
            "本目录全部脚本仅在本轮文档回填/自检中运行；未运行任何 Cargo/产品测试。",
        ],
    }
    with open(os.path.join(E04, "MANIFEST.json"), "w", encoding="utf-8") as fh:
        json.dump(doc, fh, ensure_ascii=False, indent=2)
        fh.write("\n")
    print(f"MANIFEST: {len(rows)} files at {UTC}")


if __name__ == "__main__":
    main()

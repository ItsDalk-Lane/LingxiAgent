#!/usr/bin/env python3
"""F51-REVIEW-01 receipt comparator.

Usage:
  compare_sample.py receipt.json <old_relative_path> <digest_json_file>
     -> compares a fresh digest_tree.py scan (digest_json_file) against the
        receipt entry fields (file_count, symlink_count, total_bytes,
        tree_digest) and prints EQUAL True/False per field.
  compare_sample.py --tamper-control receipt.json <old_relative_path> <digest_json_file>
     -> same comparison but with one receipt tree_digest deliberately altered
        in memory (negative control: must report EQUAL False).
Exit 0 if the requested comparison outcome holds, 1 otherwise.
"""
import json
import sys

mode = "normal"
args = sys.argv[1:]
if args and args[0] == "--tamper-control":
    mode = "tamper"
    args = args[1:]
receipt_path, old_rel, digest_path = args
receipt = json.load(open(receipt_path))
fresh = json.load(open(digest_path))
entry = None
for e in receipt["entries"]:
    if e["old_relative_path"].rstrip("/") == old_rel.rstrip("/"):
        entry = e
        break
if entry is None:
    print("ENTRY-NOT-FOUND", old_rel)
    sys.exit(1)
post = entry["post"]
if mode == "tamper":
    post = dict(post)
    post["tree_digest"] = ("0" if post["tree_digest"][0] != "0" else "1") + post["tree_digest"][1:]
fields = ["file_count", "symlink_count", "total_bytes", "tree_digest"]
results = {f: fresh.get(f) == post.get(f) for f in fields}
ok = all(results.values())
print("entry:", old_rel)
for f in fields:
    print("  %-14s fresh=%s receipt=%s EQUAL=%s" % (f, fresh.get(f), post.get(f), results[f]))
print("  ALL_EQUAL:", ok, "mode:", mode)
sys.exit(0 if ok else 1)

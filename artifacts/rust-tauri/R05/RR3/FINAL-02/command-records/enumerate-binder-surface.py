#!/usr/bin/env python3
"""Read-only classification of every ls-files entry against binder walk semantics
(candidate.rs:326-369): intermediate components must be dirs; any symlink component
fails; final component must be a regular file."""
import os, stat, subprocess, json, collections

REPO = "/Users/study_superior/Desktop/Code/LingxiAgent"
raw = subprocess.run(["git", "--no-optional-locks", "ls-files", "--cached", "--others",
                      "--exclude-standard", "-z"], cwd=REPO, capture_output=True)
entries = [e.decode("utf-8", "surrogateescape") for e in raw.stdout.split(b"\x00") if e]

kinds = collections.Counter()
failures = collections.defaultdict(list)
for rel in entries:
    walked = REPO
    comps = rel.split("/")
    bad = None
    for i, name in enumerate(comps):
        final = (i == len(comps) - 1)
        walked = os.path.join(walked, name)
        try:
            st = os.lstat(walked)
        except OSError:
            bad = "missing"; break
        m = st.st_mode
        if stat.S_ISLNK(m):
            bad = "symlink_final" if final else "symlink_ancestor"; break
        if final:
            if stat.S_ISDIR(m): bad = "directory_final"; break
            if not stat.S_ISREG(m): bad = "irregular_final"; break
        else:
            if not stat.S_ISDIR(m): bad = "ancestor_not_dir"; break
    if bad:
        kinds[bad] += 1
        if len(failures[bad]) < 500:
            failures[bad].append(rel)

out = {"repo_root": REPO,
       "enumeration_command": "git --no-optional-locks ls-files --cached --others --exclude-standard -z",
       "total_entries": len(entries),
       "ok_plain_files": len(entries) - sum(kinds.values()),
       "failing_kinds": dict(kinds),
       "failures": {k: v for k, v in failures.items()}}
with open("binder-surface-enumeration.json", "w") as f:
    json.dump(out, f, indent=1)
print(json.dumps({k: out[k] for k in ("total_entries", "ok_plain_files", "failing_kinds")}, indent=1))
for k, v in failures.items():
    print(f"--- {k} ({kinds[k]}) first 60:")
    for p in v[:60]: print("   ", p)

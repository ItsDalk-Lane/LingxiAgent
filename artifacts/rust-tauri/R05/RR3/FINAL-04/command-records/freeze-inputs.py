#!/usr/bin/env python3
"""FINAL-04: freeze production inputs + candidate binder observation (full
classification) + git state. Read-only w.r.t. repo; writes only into
/private/tmp/rr3-final04-dir. Input scope = FINAL-03's 30 items + 3 items
added by L/F53 (r04_t08_tool_matrix.rs) and M/F54 (stage_map.rs mirror
tests, r04_t08_generate_stage_map.py generator) = 33 items."""
import hashlib, json, os, stat, subprocess, collections, datetime

REPO = "/Users/study_superior/Desktop/Code/LingxiAgent"
OUT = "/private/tmp/rr3-final04-dir"

INPUTS = [
 "docs/rust-tauri/R01/r01_t01_check_ownership.py",
 "docs/rust-tauri/R05/r05_leaf_case_map.tsv",
 "docs/rust-tauri/R05/r05_required_cids.tsv",
 "docs/rust-tauri/R05/r05_stage_cids.tsv",
 "docs/rust-tauri/R05/r05_stage_pins.tsv",
 "rust-toolchain.toml",
 "rust/Cargo.lock",
 "rust/Cargo.toml",
 "rust/crates/lingxi-service/src/lib.rs",
 "rust/crates/lingxi-service/src/logging.rs",
 "rust/crates/lingxi-service/src/redaction.rs",
 "rust/crates/lingxi-service/tests/r00_management_leaves.rs",
 "rust/crates/lingxi-service/tests/r05_t08_resources.rs",
 # FINAL-04 additions (L/F53 + M/F54 production inputs):
 "rust/crates/lingxi-service/tests/r04_t08_tool_matrix.rs",
 "rust/crates/xtask/src/stage_map.rs",
 "scripts/rust-tauri/r04_t08_generate_stage_map.py",
 "rust/crates/xtask/src/candidate.rs",
 "rust/crates/xtask/src/main.rs",
 "rust/crates/xtask/src/stage_maps/R02.json",
 "rust/crates/xtask/src/stage_maps/R03.json",
 "rust/crates/xtask/src/stage_maps/R04.json",
 "rust/crates/xtask/src/stage_maps/R05.json",
 "rust/crates/xtask/src/verify.rs",
 "scripts/rust-tauri/prepare_git_copy.py",
 "scripts/rust-tauri/r01-t02-check-generated.sh",
 "scripts/rust-tauri/r02_run_output_regression.py",
 "scripts/rust-tauri/r02_t08_legacy_entry_regression.sh",
 "scripts/rust-tauri/r05_t08_mutate_pin.py",
 "scripts/rust-tauri/r05_t08_negative_gate.sh",
 "scripts/rust-tauri/r05_t08_negative_gate_selfcheck.py",
 "scripts/rust-tauri/r05_t08_prepare_node.py",
 "scripts/rust-tauri/r05_t08_restore_selfcheck.py",
 "scripts/rust-tauri/r05_t08_stage_suites.sh",
]

def sha256(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()

recorded, missing = {}, []
for rel in INPUTS:
    p = os.path.join(REPO, rel)
    if not os.path.isfile(p):
        missing.append(rel); continue
    recorded[rel] = {"bytes": os.path.getsize(p), "sha256": sha256(p)}

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
    else:
        kinds["regular_file"] += 1

binder_observation = {
    "observed_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "enumeration_command": "git --no-optional-locks ls-files --cached --others --exclude-standard -z",
    "enum_exit": raw.returncode,
    "total_entries": len(entries),
    "classification": dict(kinds),
    "failures": {k: v for k, v in failures.items()},
    "note": "F51/F52 relocations expected to hold (100% regular files, six failure "
            "classes zero). Any non-zero class would fail-close cmd-06 the way "
            "FINAL-01/FINAL-02 observed. FINAL-04 dir itself is a new untracked "
            "regular-file tree, enumerated but classified regular.",
}

res = {
    "frozen_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "head": subprocess.run(["git", "-C", REPO, "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip(),
    "inputs": recorded,
    "missing": missing,
    "binder_observation": binder_observation,
}
with open(os.path.join(OUT, "frozen-inputs.json"), "w") as f:
    json.dump(res, f, indent=1, sort_keys=True)
print("frozen inputs:", len(recorded), "missing:", len(missing))
print("total entries:", len(entries), "classification:", dict(kinds))

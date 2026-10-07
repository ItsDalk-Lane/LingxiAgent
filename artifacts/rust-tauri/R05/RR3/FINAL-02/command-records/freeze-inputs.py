#!/usr/bin/env python3
"""FINAL-02: freeze production inputs + candidate binder observation + git state.
Read-only w.r.t. repo; writes only into /private/tmp/rr3-final02-dir."""
import hashlib, json, os, subprocess, sys, datetime

REPO = "/Users/study_superior/Desktop/Code/LingxiAgent"
OUT = "/private/tmp/rr3-final02-dir"

# Same 30-item production input scope as FINAL-01 frozen-inputs.json
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

# Binder observation: the enumeration the candidate binder consumes
enum_cmd = ["git", "--no-optional-locks", "ls-files", "--cached", "--others", "--exclude-standard"]
raw = subprocess.run(enum_cmd, cwd=REPO, capture_output=True, text=True)
all_entries = [l for l in raw.stdout.split("\n") if l != ""]
dir_entries = [e for e in all_entries if e.endswith("/")]
binder_observation = {
    "observed_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "enumeration_command": " ".join(enum_cmd),
    "enum_exit": raw.returncode,
    "total_entries": len(all_entries),
    "directory_entries": len(dir_entries),
    "all_directory_entries": dir_entries,
    "note": "F51 relocated the 56 nested-.git fixture dirs out of the main tree (F51-REVIEW-01 PASS); "
            "directory entries are expected to be 0. Binder hashes each entry as a file; a directory "
            "entry here would fail-closed the way FINAL-01 cmd-06 observed.",
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
print("binder dir entries:", len(dir_entries), "total entries:", len(all_entries))

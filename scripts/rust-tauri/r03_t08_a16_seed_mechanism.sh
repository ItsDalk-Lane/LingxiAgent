#!/usr/bin/env bash
# R03-T08 / acceptance R03-A16 — 故障种子可重放（机制证明）。
#
# Honest scope: the two long-term state-machine property tests
# (run_finalize_property / late_result_fencing_property) have NEVER
# observed a natural anomaly under their default seeds. Per the stage
# book's allowed path, this script proves the CAPTURE-and-REPLAY
# MECHANISM with an isolated controlled-defect injection — it is a
# MECHANISM PROOF, not a claim that a natural anomaly was ever found.
#
# What it proves, end to end:
#   P0  baseline: the REAL property test passes under its default seed;
#   P1  mutation: a TEMPORARY copy of the real property test (built in a
#       throwaway /tmp cargo crate — the repository tree is NEVER
#       touched, so stage-gate candidate snapshots stay stable) gets a
#       controlled defect injected: on a rare seeded subset of
#       deliveries it tampers the durable run row (terminal -> active)
#       through an independent SQLite connection AFTER the loop's
#       pre-delivery snapshot — exactly the illegal durable sequence the
#       property's own "terminal states never flip" detector must catch;
#   P2  discovery+capture: seeds are scanned until the mutated copy
#       fails; the failing seed is captured from the
#       R03_T04_PROPERTY_FAILURE seed=0x… line AND the seed-capture file
#       (both must agree);
#   P3  stable reproduction: the SAME fixed seed fails the mutated copy
#       twice, identically;
#   P4  removal: the mutation is gone (scratch crate deleted); the REAL
#       test passes under the SAME captured seed and the full property
#       group passes — the replay machinery is part of the long-term
#       regression, not a special branch.
#
# Environment guards: rustup-locked toolchain, task-dedicated target
# dirs, offline builds, synthetic /tmp scratch only, no network, no
# production-code change.
#
# Usage: scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh [EVIDENCE_DIR]
set -euo pipefail
cd "$(dirname "$0")/../.."

# The argument IS this command's own evidence directory (the stage gate
# passes {EVIDENCE}/A16; running standalone defaults to the T08 root's
# A16 subdir). It must be fresh — stale evidence must never mask a run.
EVIDENCE_DIR="${1:-artifacts/rust-tauri/R03/T08/A16}"
if [ -L "$EVIDENCE_DIR" ] || { [ -e "$EVIDENCE_DIR" ] && [ ! -d "$EVIDENCE_DIR" ]; }; then
  echo "ERROR: evidence path is not a regular directory: $EVIDENCE_DIR" >&2
  exit 1
fi
if [ -d "$EVIDENCE_DIR" ]; then
  FIRST_ENTRY="$(find "$EVIDENCE_DIR" -mindepth 1 -print -quit)" || exit 1
  [ -z "$FIRST_ENTRY" ] || { echo "ERROR: evidence directory is not empty: $EVIDENCE_DIR" >&2; exit 1; }
fi
mkdir -p "$EVIDENCE_DIR"

TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/rust-target-r03-t08-a16}"
PROBE_TARGET_DIR="${TMPDIR:-/tmp}/rust-target-r03-t08-a16-probe"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/lingxi-r03t08-a16-XXXXXX")"

TOOLCHAIN="$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml | head -n 1)"
if [ -z "$TOOLCHAIN" ]; then
  echo "ERROR: cannot parse toolchain channel from rust-toolchain.toml" >&2
  exit 1
fi
if ! command -v rustup >/dev/null 2>&1; then
  if [ -x "$HOME/.cargo/bin/rustup" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
  else
    echo "ERROR: rustup not found; this gate requires the locked toolchain ($TOOLCHAIN)" >&2
    exit 1
  fi
fi
CARGO="env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TARGET_DIR rustup run $TOOLCHAIN cargo"

cleanup() { rm -rf "$SCRATCH"; }
trap cleanup EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }
note() { printf '%s\n' "$*" | tee -a "$EVIDENCE_DIR/summary.txt"; }

# ---- P0: baseline — the REAL property test under its default seed -------------
note "== P0: baseline (real property test, default seed) =="
$CARGO test --manifest-path rust/Cargo.toml --locked --offline \
  -p lingxi-adapters --test late_result_fencing_property -- --nocapture \
  > "$EVIDENCE_DIR/p0-baseline-real-default-seed.log" 2>&1 \
  || { cat "$EVIDENCE_DIR/p0-baseline-real-default-seed.log"; fail "P0 baseline run failed"; }
grep -q "R03_T04_PROPERTY totals:" "$EVIDENCE_DIR/p0-baseline-real-default-seed.log" \
  || fail "P0 baseline must print the totals witness line"
note "PASS P0 real property green under default seed (totals line present)"

# ---- P1: build the isolated mutation probe crate in scratch --------------------
note "== P1: isolated mutation probe (scratch crate, repo tree untouched) =="
REPO_ROOT="$(pwd)"
mkdir -p "$SCRATCH/probe"
python3 - "$REPO_ROOT" "$SCRATCH/probe" << 'PYEOF' || fail "P1 probe generation failed"
import json, sys, tomllib
repo, probe_dir = sys.argv[1], sys.argv[2]
with open(f"{repo}/rust/Cargo.lock", "rb") as fh:
    lock = tomllib.load(fh)
pkgs = {p["name"]: p["version"] for p in lock["package"]}
def spec(v):
    major = int(v.split(".")[0])
    return v if major >= 2 else f">={v}, <2"
cargo_toml = f"""[package]
name = "r03-t08-a16-mutation-probe"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
lingxi-adapters = {{ path = "{repo}/rust/crates/lingxi-adapters" }}
lingxi-kernel = {{ path = "{repo}/rust/crates/lingxi-kernel" }}
lingxi-protocol = {{ path = "{repo}/rust/crates/lingxi-protocol" }}
rusqlite = {{ version = "{spec(pkgs['rusqlite'])}", features = ["bundled"] }}
serde_json = "{spec(pkgs['serde_json'])}"
tokio = {{ version = "{spec(pkgs['tokio'])}", features = ["macros", "rt"] }}

[workspace]

[[test]]
name = "mutation_probe"
path = "mutation_probe.rs"
"""
src = open(f"{repo}/rust/crates/lingxi-adapters/tests/late_result_fencing_property.rs").read()

tls_anchor = """thread_local! {
    static PROPERTY_SEED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
"""
assert tls_anchor in src, "tls anchor not found"
extra_tls = """
// [A16 CONTROLLED DEFECT — mechanism probe only, never production code]
thread_local! {
    static A16_TAMPER: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static A16_DB_PATH: std::cell::RefCell<String> =
        const { std::cell::RefCell::new(String::new()) };
}
"""
src = src.replace(tls_anchor, tls_anchor + extra_tls, 1)

seed_line = """    let seed = effective_seed();
    let mut rng = seed;
"""
assert seed_line in src, "seed entrypoint not found"
src = src.replace(seed_line, seed_line + """    A16_TAMPER.with(|cell| cell.set(seed | 1));
""", 1)

loop_anchor = """        let db = seeded_store(&path).await;
        let counters = round(&db, round_index, &mut rng).await;"""
assert loop_anchor in src, "loop anchor not found"
src = src.replace(loop_anchor, """        let db = seeded_store(&path).await;
        A16_DB_PATH.with(|cell| {
            *cell.borrow_mut() = path.to_string_lossy().to_string();
        });
        let counters = round(&db, round_index, &mut rng).await;""", 1)

tamper_anchor = """        let audit_before = scalar(
            db,
            "SELECT COUNT(*) FROM stale_result_audit WHERE run_id = ?1",
            run_id.clone(),
        )
        .await;
"""
assert tamper_anchor in src, "tamper anchor not found"
tamper_code = tamper_anchor + """
        // [A16 CONTROLLED DEFECT — mechanism probe only, never production]
        // A rare seeded tamper: through an INDEPENDENT SQLite connection,
        // flip the durable status of a TERMINAL run right after this
        // pre-delivery snapshot. The loop's own "terminal states never
        // flip" detector must catch it. The tamper draw comes from the
        // effective seed, so whether it ever coincides with a terminal
        // snapshot (and which delivery follows) depends on the SEED —
        // some seeds pass, some fail, exactly what a scan can discover.
        let a16_draw = A16_TAMPER.with(|cell| {
            let mut state = cell.get();
            let out = xorshift64(&mut state);
            cell.set(state);
            out
        });
        if a16_draw % 64 == 0 && terminal {
            let db_file = A16_DB_PATH.with(|cell| cell.borrow().clone());
            let conn = rusqlite::Connection::open(&db_file)
                .expect("independent tamper connection opens");
            conn.busy_timeout(std::time::Duration::from_millis(5000))
                .expect("tamper busy timeout");
            let flipped = conn
                .execute(
                    "UPDATE runs SET status = 'queued' WHERE run_id = ?1",
                    rusqlite::params![run_id],
                )
                .expect("controlled tamper executes");
            assert!(flipped == 1, "controlled tamper must touch exactly one row");
        }
"""
src = src.replace(tamper_anchor, tamper_code, 1)
open(f"{probe_dir}/Cargo.toml", "w").write(cargo_toml)
open(f"{probe_dir}/mutation_probe.rs", "w").write(src)
print("probe crate generated")
PYEOF

env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$PROBE_TARGET_DIR \
  rustup run "$TOOLCHAIN" cargo test --manifest-path "$SCRATCH/probe/Cargo.toml" --offline --no-run \
  > "$EVIDENCE_DIR/p1-probe-build.log" 2>&1 \
  || { tail -40 "$EVIDENCE_DIR/p1-probe-build.log"; fail "P1 probe build failed"; }
note "PASS P1 probe crate built in scratch (repo tree untouched)"

run_probe() { # $1=seed $2=logfile $3=seedfile-out -> rc reflects the test outcome
  local seed="$1" logfile="$2" seedfile="$3"
  rm -f "$seedfile"
  set +e
  env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
    CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$PROBE_TARGET_DIR \
    R03_T04_PROPERTY_SEED="$seed" R03_T04_PROPERTY_SEED_FILE="$seedfile" \
    rustup run "$TOOLCHAIN" cargo test --manifest-path "$SCRATCH/probe/Cargo.toml" --offline \
    --test mutation_probe -- --nocapture \
    > "$logfile" 2>&1
  local rc=$?
  set -e
  return $rc
}

# ---- P2: discovery + capture ----------------------------------------------------
note "== P2: seed scan until the mutated copy reports an illegal sequence =="
CAPTURED_SEED=""
for i in $(seq 0 59); do
  CANDIDATE_SEED=$(printf '0x%x' "$(( 0xa16d0000000000 + i ))")
  if run_probe "$CANDIDATE_SEED" \
      "$EVIDENCE_DIR/p2-scan-attempt-$i.log" \
      "$EVIDENCE_DIR/p2-captured-seed-attempt-$i.txt"; then
    printf '%s\n' "seed $CANDIDATE_SEED did not trigger (legitimate: the rare tamper draw never coincided with a terminal snapshot)" \
      >> "$EVIDENCE_DIR/p2-scan-attempt-$i.log"
    continue
  fi
  # The tamper IS an illegal durable sequence; whichever oracle trips
  # first (terminal flip / refused-class shape / finalize-contract
  # rejection) is a legitimate detection — the mechanism proof needs the
  # failure LINE to carry the seed and the seed FILE to agree, and the
  # failure to be a real test failure (not a seed-parse error).
  if grep -q "R03_T04_PROPERTY_FAILURE seed=$CANDIDATE_SEED" "$EVIDENCE_DIR/p2-scan-attempt-$i.log" \
     && ! grep -q "invalid R03_T04_PROPERTY_SEED" "$EVIDENCE_DIR/p2-scan-attempt-$i.log" \
     && grep -q "test result: FAILED" "$EVIDENCE_DIR/p2-scan-attempt-$i.log" \
     && [ -s "$EVIDENCE_DIR/p2-captured-seed-attempt-$i.txt" ]; then
    CAPTURED_SEED=$(tr -d '[:space:]' < "$EVIDENCE_DIR/p2-captured-seed-attempt-$i.txt")
    [ "$CAPTURED_SEED" = "$CANDIDATE_SEED" ] \
      || fail "P2 captured seed file disagrees with the env-provided seed ($CAPTURED_SEED vs $CANDIDATE_SEED)"
    cp "$EVIDENCE_DIR/p2-captured-seed-attempt-$i.txt" "$EVIDENCE_DIR/captured-seed.txt"
    note "P2 anomaly triggered and captured at scan attempt $i (seed=$CAPTURED_SEED; failure line + seed file agree)"
    break
  fi
  fail "P2 attempt $i failed WITHOUT the expected illegal-sequence shape (see p2-scan-attempt-$i.log)"
done
[ -n "$CAPTURED_SEED" ] || fail "P2 scan exhausted without triggering the controlled defect"
note "PASS P2 capture: seed=$CAPTURED_SEED"

# ---- P3: stable reproduction under the fixed seed --------------------------------
note "== P3: fixed-seed reproduction (twice, identical outcome) =="
for rep in 1 2; do
  if run_probe "$CAPTURED_SEED" "$EVIDENCE_DIR/p3-replay-$rep.log" "$EVIDENCE_DIR/p3-replay-$rep-seed.txt"; then
    fail "P3 replay $rep passed — the captured seed does not reproduce the anomaly"
  fi
  grep -q "R03_T04_PROPERTY_FAILURE seed=$CAPTURED_SEED" "$EVIDENCE_DIR/p3-replay-$rep.log" \
    || fail "P3 replay $rep missing the failure line for the captured seed"
  grep -q "test result: FAILED" "$EVIDENCE_DIR/p3-replay-$rep.log" \
    || fail "P3 replay $rep did not fail"
  GOT=$(tr -d '[:space:]' < "$EVIDENCE_DIR/p3-replay-$rep-seed.txt")
  [ "$GOT" = "$CAPTURED_SEED" ] \
    || fail "P3 replay $rep seed file records '$GOT' instead of the captured seed"
  note "PASS P3 replay $rep: same seed, same illegal sequence (stable reproduction)"
done

# ---- P4: mutation removed — same seed + full group pass on the REAL test --------
note "== P4: mutation removed — real test passes under the captured seed + full group =="
rm -rf "$SCRATCH/probe"
[ ! -e "$SCRATCH/probe" ] || fail "P4 scratch probe still present"
if env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
    CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TARGET_DIR \
    R03_T04_PROPERTY_SEED="$CAPTURED_SEED" \
    rustup run "$TOOLCHAIN" cargo test --manifest-path rust/Cargo.toml --locked --offline \
    -p lingxi-adapters --test late_result_fencing_property -- --nocapture \
    > "$EVIDENCE_DIR/p4-real-same-seed.log" 2>&1; then
  grep -q "R03_T04_PROPERTY totals:" "$EVIDENCE_DIR/p4-real-same-seed.log" \
    || fail "P4 same-seed run must still print the totals witness line"
  note "PASS P4 real property green under the captured seed (no mutation present)"
else
  cat "$EVIDENCE_DIR/p4-real-same-seed.log"
  fail "P4 real property failed under the captured seed"
fi
$CARGO test --manifest-path rust/Cargo.toml --locked --offline \
  -p lingxi-adapters --test run_finalize_property --test late_result_fencing_property -- --nocapture \
  > "$EVIDENCE_DIR/p4-full-property-group.log" 2>&1 \
  || { cat "$EVIDENCE_DIR/p4-full-property-group.log"; fail "P4 full property group failed"; }
note "PASS P4 full property group green (replay machinery is part of the long-term regression)"

python3 - "$EVIDENCE_DIR" "$CAPTURED_SEED" << 'PYEOF' || fail "summary JSON failed"
import json, sys
ev, seed = sys.argv[1], sys.argv[2]
doc = {
  "schema": "lingxi.r03-t08-a16-seed-mechanism.v1",
  "kind": "MECHANISM_PROOF_ISOLATED_MUTATION",
  "honesty_note": "No natural property-test anomaly was ever observed; this is the "
                  "stage-book-allowed isolated controlled-defect proof of the "
                  "capture->replay mechanism, not a natural discovery history.",
  "captured_seed": seed,
  "phases": {
    "p0_baseline_real_default_seed": "PASS",
    "p1_isolated_mutation_probe_built": "PASS",
    "p2_anomaly_triggered_and_seed_captured": "PASS",
    "p3_fixed_seed_stable_reproduction": "PASS (2/2 identical illegal sequences)",
    "p4_mutation_removed_same_seed_and_full_group": "PASS",
  },
  "permanent_mechanism": {
    "seed_override_env": "R03_T04_PROPERTY_SEED / R03_A02_PROPERTY_SEED",
    "failure_line": "R03_T04_PROPERTY_FAILURE seed=0x... location=...",
    "seed_file_env": "R03_T04_PROPERTY_SEED_FILE / R03_A02_PROPERTY_SEED_FILE",
    "home": "rust/crates/lingxi-adapters/tests/late_result_fencing_property.rs, run_finalize_property.rs",
  },
  "mutation_isolation": "throwaway cargo crate under mktemp TMPDIR; repository tree untouched; probe removed before P4",
}
open(f"{ev}/seed-mechanism.json", "w").write(json.dumps(doc, indent=1, ensure_ascii=False) + "\n")
PYEOF
note "RESULT: R03-A16 seed mechanism proof ALL GREEN (mechanism proof, honestly labeled)"

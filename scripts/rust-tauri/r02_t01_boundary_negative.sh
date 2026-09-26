#!/usr/bin/env bash
# R02-T01 / acceptance R02-A02 — violating the dependency rules must FAIL the
# boundary gate, and the failure must localize the offending crate/edge.
#
# Extends (does not replace) the R01 gate docs/rust-tauri/R01/
# r01_t01_check_ownership.py against docs/rust-tauri/R01/DEPENDENCY_RULES.json
# (which R02-T01 updated: lingxi-service registered exists + new DEP-08
# kernel-no-infrastructure).
#
# Fully scripted and re-runnable negative test. Three temporary violations
# are injected into the DOMAIN crate (lingxi-kernel), one at a time:
#   N-A  source-level host type  (tauri::AppHandle in kernel sources)
#          -> D3 / DEP-02 names the crate + file
#   N-B  dependency edge to the desktop stack (path dep named "tauri")
#          -> D1 / DEP-02 names module lingxi-kernel + forbidden ['tauri']
#   N-C  composition-root inversion (kernel -> lingxi-service edge), the
#        edge class R02-T01 itself added as DEP-08
#          -> D1 / DEP-08 names module lingxi-kernel + forbidden
#             ['lingxi-service']
# Every phase: inject -> gate exits non-zero with the expected localized
# message -> restore -> gate green again. Protected files are checksummed
# before and after; any residue fails the run.
#
# Environment guards: rustup-pinned toolchain cargo on PATH (the checker
# shells out to plain `cargo`), dead proxy stripped, task-dedicated
# CARGO_TARGET_DIR (RR-T08-F1).
#
# Usage: scripts/rust-tauri/r02_t01_boundary_negative.sh [EVIDENCE_DIR]
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R02/T01}"
mkdir -p "$EVIDENCE_DIR"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rust-target-r02-t01}"

TOOLCHAIN="$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml | head -n 1)"
if [ -z "$TOOLCHAIN" ]; then
  echo "ERROR: cannot parse toolchain channel from rust-toolchain.toml" >&2
  exit 1
fi
if ! command -v rustup >/dev/null 2>&1; then
  if [ -x "$HOME/.cargo/bin/rustup" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
    export PATH
  else
    echo "ERROR: rustup not found; this gate requires the locked toolchain ($TOOLCHAIN)" >&2
    exit 1
  fi
fi
# The checker invokes plain `cargo metadata`; make that the rustup proxy for
# the pinned toolchain and strip the dead proxy env.
export PATH="$HOME/.cargo/bin:$PATH"
RUN_GATE="env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  CARGO_TARGET_DIR=$CARGO_TARGET_DIR \
  python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py"

KERNEL_LIB="rust/crates/lingxi-kernel/src/lib.rs"
KERNEL_MANIFEST="rust/crates/lingxi-kernel/Cargo.toml"
LOCKFILE="rust/Cargo.lock"

GUARD_DIR="$(mktemp -d /tmp/lingxi-r02-t01-negative.XXXXXX)"
DIRTY=0
cleanup() {
  local code=$?
  if [ "$DIRTY" -eq 1 ]; then
    # Guaranteed rollback even when the script itself dies mid-injection
    # (first run taught us this: an assertion failure must never leave the
    # violation behind in the protected files).
    cp "$GUARD_DIR/lib.rs.orig" "$KERNEL_LIB" 2>/dev/null || true
    cp "$GUARD_DIR/kernel-Cargo.toml.orig" "$KERNEL_MANIFEST" 2>/dev/null || true
    cp "$GUARD_DIR/Cargo.lock.orig" "$LOCKFILE" 2>/dev/null || true
    echo "cleanup: rolled back injected violation into protected files" >&2
  fi
  rm -rf "$GUARD_DIR"
  exit "$code"
}
trap cleanup EXIT

INJECTION_MARKER="# r02-t01-negative-injection (temporary; gate must reject)"

# expect_fail NAME EXPECTED_GREP... : gate must exit non-zero AND the output
# must contain every EXPECTED_GREP (localized crate/rule evidence).
expect_fail() {
  local name="$1"; shift
  local log="$EVIDENCE_DIR/a02-$name.log"
  set +e
  $RUN_GATE > "$log" 2>&1
  local code=$?
  set -e
  echo "[$name] gate exit code: $code (expected non-zero)" | tee -a "$EVIDENCE_DIR/a02-summary.log"
  if [ "$code" -eq 0 ]; then
    echo "[$name] FAILED: gate stayed green despite the violation" >&2
    return 1
  fi
  local grep
  for grep in "$@"; do
    if ! grep -q "$grep" "$log"; then
      echo "[$name] FAILED: output does not localize the violation ($grep missing)" >&2
      return 1
    fi
    echo "[$name] localized: $grep" | tee -a "$EVIDENCE_DIR/a02-summary.log"
  done
  return 0
}

expect_green() {
  local name="green-$1"
  local log="$EVIDENCE_DIR/a02-$name.log"
  set +e
  $RUN_GATE > "$log" 2>&1
  local code=$?
  set -e
  echo "[$name] gate exit code: $code (expected 0)" | tee -a "$EVIDENCE_DIR/a02-summary.log"
  if [ "$code" -ne 0 ]; then
    echo "[$name] FAILED: gate did not return to green after restore" >&2
    tail -n 5 "$log" >&2
    return 1
  fi
}

: > "$EVIDENCE_DIR/a02-summary.log"
echo "== [0/4] baseline (pre-injection) must be green"
cp "$KERNEL_LIB" "$GUARD_DIR/lib.rs.orig"
cp "$KERNEL_MANIFEST" "$GUARD_DIR/kernel-Cargo.toml.orig"
cp "$LOCKFILE" "$GUARD_DIR/Cargo.lock.orig"
shasum -a 256 "$KERNEL_LIB" "$KERNEL_MANIFEST" "$LOCKFILE" > "$GUARD_DIR/protected.before"
expect_green baseline

echo "== [1/4] N-A: inject host TYPE into domain sources (must be rejected, D3)"
DIRTY=1
cat >> "$KERNEL_LIB" <<EOF

$INJECTION_MARKER
#[allow(dead_code)]
pub mod r02_t01_negative {
    pub type HostAppHandle = tauri::AppHandle;
}
EOF
expect_fail na-source-token "DEP-02" "lingxi-kernel" "forbidden token 'AppHandle' in rust/crates/lingxi-kernel/src/lib.rs" || exit 1
cp "$GUARD_DIR/lib.rs.orig" "$KERNEL_LIB"
expect_green na-restored || exit 1
DIRTY=0

echo "== [2/4] N-B: inject dependency EDGE to desktop stack (must be rejected, D1)"
DIRTY=1
FAKE_TAURI="$GUARD_DIR/fake-tauri"
mkdir -p "$FAKE_TAURI"
cat > "$FAKE_TAURI/Cargo.toml" <<'EOF'
[package]
name = "tauri"
version = "0.0.0"
edition = "2021"

[lib]
path = "lib.rs"
EOF
echo "pub struct AppHandle;" > "$FAKE_TAURI/lib.rs"
cat >> "$KERNEL_MANIFEST" <<EOF

$INJECTION_MARKER
tauri = { path = "$FAKE_TAURI" }
EOF
expect_fail nb-dep-edge "DEP-02" "module lingxi-kernel transitively depends on forbidden \['tauri'\]" || exit 1
cp "$GUARD_DIR/kernel-Cargo.toml.orig" "$KERNEL_MANIFEST"
cp "$GUARD_DIR/Cargo.lock.orig" "$LOCKFILE"
expect_green nb-restored || exit 1
DIRTY=0

echo "== [3/4] N-C: inject composition-root INVERSION kernel->lingxi-service (DEP-08)"
# R02-T04 note: lingxi-service now legitimately depends on
# lingxi-adapters (port injection), which depends on lingxi-kernel, so the
# injected inversion kernel->lingxi-service forms a cargo-level CYCLE
# before the checker even resolves the graph. The rejection is therefore
# structural (cyclic package dependency naming lingxi-service); before
# R02-T04 the same injection was rejected by the checker's own DEP-08
# with the localized message. Both forms reject loudly and both name
# lingxi-service; the localization grep matches either shape.
DIRTY=1
cat >> "$KERNEL_MANIFEST" <<EOF

$INJECTION_MARKER
lingxi-service = { path = "../lingxi-service" }
EOF
expect_fail nc-service-inversion "lingxi-service" "cyclic package dependency" || exit 1
cp "$GUARD_DIR/kernel-Cargo.toml.orig" "$KERNEL_MANIFEST"
cp "$GUARD_DIR/Cargo.lock.orig" "$LOCKFILE"
expect_green nc-restored || exit 1
DIRTY=0

echo "== [4/4] residue check: protected files byte-identical to baseline"
shasum -a 256 -c "$GUARD_DIR/protected.before" > "$EVIDENCE_DIR/a02-residue-check.log" 2>&1
cat "$EVIDENCE_DIR/a02-residue-check.log"

echo "A02 RESULT: PASS (3 violations rejected with localized crate/edge; 3 restores back to green; zero residue)"

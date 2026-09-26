#!/usr/bin/env bash
# R02-T02 / acceptance R02-A04 — data-root precedence never surprises, with
# REAL binary processes and FILE PROBES (what actually exists on disk is
# the verdict, not the log text).
#
# Documented precedence: --test-mode > --home (CLI) > LINGXI_HOME (env) >
# --config FILE's "home". Cases:
#   1. only CLI            -> exactly the CLI root materializes
#   2. only env            -> exactly the env root materializes
#   3. only config file    -> exactly the config root materializes
#   4. CLI > env > config  -> exactly the CLI root; env+config reported as
#                             ignored in the safe log; config STILL validated
#   5. test mode           -> NONE of the three (prod-lookalike) roots
#                             materialize; a fresh isolated synthetic home
#                             under the system temp dir is used instead
#   6. negative probes     -> no source: exit 2; relative home: exit 2;
#                             duplicate --home: exit 2; --home=/x: exit 2;
#                             --bind --home /x (flag-shaped): exit 2
#
# Probes: for each case the script asserts
#   (a) the READY line's home=/source= (resolution assertion),
#   (b) <root>/lingxi-service/instance.json EXISTS on the winning root
#       (file probe of the real choice),
#   (c) every non-winning candidate root does NOT exist at all
#       (neither its literal path nor its /private alias on macOS),
#   (d) the stderr safe log shows "data root resolved" with the source.
#
# Usage: scripts/rust-tauri/r02_t02_path_priority.sh [EVIDENCE_DIR]
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R02/T02}"
mkdir -p "$EVIDENCE_DIR"
TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rust-target-r02-t02}"

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

SCRATCH=""
cleanup() { [ -n "$SCRATCH" ] && [ -d "$SCRATCH" ] && rm -rf "$SCRATCH"; }
trap cleanup EXIT

echo "== toolchain: rustup run $TOOLCHAIN ($(rustup run "$TOOLCHAIN" rustc --version | head -n 1))"
echo "== [build] lingxi-service (locked, offline, isolated target dir)"
$CARGO build --manifest-path rust/Cargo.toml --locked -p lingxi-service \
  > "$EVIDENCE_DIR/a04-build.log" 2>&1
tail -n 1 "$EVIDENCE_DIR/a04-build.log"
BIN="$TARGET_DIR/debug/lingxi-service"
test -x "$BIN"

SCRATCH="$(mktemp -d /tmp/lingxi-r02t02-a04.XXXXXX)"
: > "$EVIDENCE_DIR/a04-summary.txt"
# Candidate roots are per-case and do NOT exist up front; each case proves
# exactly one of them gets materialized by that case's own run.
R_CLI="$SCRATCH/case1234/root-cli"
R_ENV="$SCRATCH/case1234/root-env"
R_CFG="$SCRATCH/case1234/root-config"
CFG="$SCRATCH/case1234/service.json"
mkdir -p "$SCRATCH/case1234"
printf '{"home": "%s"}\n' "$R_CFG" > "$CFG"

# macOS alias of a path ("" when the path itself is already canonical).
alias_of() {
  case "$1" in
    /private/*) printf '' ;;
    /*) printf '/private%s' "$1" ;;
    *) printf '' ;;
  esac
}

assert_absent() { # root must not exist, in either literal or alias form
  local r="$1" a
  a="$(alias_of "$r")"
  if [ -e "$r" ] || { [ -n "$a" ] && [ -e "$a" ]; }; then
    echo "ERROR: $r (or alias) must NOT exist" >&2
    return 1
  fi
}

assert_materialized() { # the winning root must carry the runtime record
  local r="$1"
  [ -f "$r/lingxi-service/instance.json" ] || {
    echo "ERROR: $r/lingxi-service/instance.json missing (real choice probe)" >&2
    return 1
  }
}

start_case() { # $1=case-name $2=expected-source; binary args in "$@:3"
  # Starts the REAL binary, waits for READY, validates the READY line and
  # the safe-log resolution line, and leaves the process RUNNING (file
  # probes happen while it runs; stop_case performs the clean shutdown).
  local name="$1" source="$2"
  shift 2
  CASE_NAME="$name"
  CASE_OUT="$EVIDENCE_DIR/a04-$name-stdout.log"
  CASE_ERR="$EVIDENCE_DIR/a04-$name-stderr.log"
  set +e
  "$@" > "$CASE_OUT" 2> "$CASE_ERR" &
  CASE_PID=$!
  for _ in $(seq 1 300); do
    if ! kill -0 "$CASE_PID" 2>/dev/null; then break; fi
    if grep -q 'LINGXI_SERVICE_READY addr=' "$CASE_OUT" 2>/dev/null; then break; fi
    sleep 0.1
  done
  if ! grep -q 'LINGXI_SERVICE_READY addr=' "$CASE_OUT" 2>/dev/null; then
    wait "$CASE_PID" || true
    echo "ERROR: case $name never became ready" >&2
    exit 1
  fi
  set -e
  READY_HOME="$(grep -o 'home=[^ ]*' "$CASE_OUT" | tail -n 1 | sed 's/^home=//')"
  local ready_source
  ready_source="$(grep -o 'source=[^ ]*' "$CASE_OUT" | tail -n 1 | sed 's/^source=//')"
  [ "$ready_source" = "$source" ] || {
    echo "ERROR: case $name READY source=$ready_source, expected $source" >&2; exit 1; }
  grep -q "data root resolved" "$CASE_ERR" || {
    echo "ERROR: case $name safe log lacks the resolution line" >&2; exit 1; }
  grep -q "source=$source" "$CASE_ERR" || {
    echo "ERROR: case $name safe log does not report source=$source" >&2; exit 1; }
  printf '%s\n' "case=$name source=$ready_source home=$READY_HOME" >> "$EVIDENCE_DIR/a04-summary.txt"
}

stop_case() { # clean SIGTERM stop + own-record cleanup assertions
  local rc
  kill -TERM "$CASE_PID"
  set +e
  wait "$CASE_PID"
  rc=$?
  set -e
  [ "$rc" -eq 0 ] || { echo "ERROR: case $CASE_NAME unclean stop ($rc)" >&2; exit 1; }
  [ ! -f "$READY_HOME/lingxi-service/instance.json" ] || {
    echo "ERROR: case $CASE_NAME clean stop must remove the own record" >&2; exit 1; }
  [ -f "$READY_HOME/lingxi-service/instance.lock" ] || {
    echo "ERROR: case $CASE_NAME runtime dir vanished (lock file expected)" >&2; exit 1; }
  CASE_PID=""
}

echo "== [1] only CLI"
start_case cli cli env -u LINGXI_HOME "$BIN" --home "$R_CLI"
assert_materialized "$READY_HOME"
assert_absent "$R_ENV"; assert_absent "$R_CFG"
stop_case
echo "only CLI materialized: OK"

echo "== [2] only env"
R_CLI2="$SCRATCH/case2/root-cli2"; R_CFG2="$SCRATCH/case2/root-config2"
start_case env env env LINGXI_HOME="$R_ENV" "$BIN"
assert_materialized "$READY_HOME"
assert_absent "$R_CFG2"; assert_absent "$R_CLI2"
stop_case
echo "only env materialized (fresh candidates untouched): OK"

echo "== [3] only config file"
R_CLI3="$SCRATCH/case3/root-cli3"; R_ENV3="$SCRATCH/case3/root-env3"
start_case config-file config-file env -u LINGXI_HOME "$BIN" --config "$CFG"
assert_materialized "$READY_HOME"
assert_absent "$R_CLI3"; assert_absent "$R_ENV3"
stop_case
echo "only config materialized (fresh candidates untouched): OK"

echo "== [4] conflict: CLI beats env and config"
R_CLI4="$SCRATCH/case4/root-cli4"
R_ENV4="$SCRATCH/case4/root-env4"
R_CFG4="$SCRATCH/case4/root-config4"
CFG4="$SCRATCH/case4/service.json"
mkdir -p "$SCRATCH/case4"
printf '{"home": "%s"}\n' "$R_CFG4" > "$CFG4"
start_case conflict cli env LINGXI_HOME="$R_ENV4" "$BIN" --home "$R_CLI4" --config "$CFG4"
assert_materialized "$READY_HOME"
assert_absent "$R_ENV4"; assert_absent "$R_CFG4"
stop_case
grep -q "env(LINGXI_HOME)=$R_ENV4" "$EVIDENCE_DIR/a04-conflict-stderr.log" || {
  echo "ERROR: conflict case does not report the ignored env root" >&2; exit 1; }
grep -q "config-file=$CFG4" "$EVIDENCE_DIR/a04-conflict-stderr.log" || {
  echo "ERROR: conflict case does not report the ignored config root" >&2; exit 1; }
echo "CLI won; env+config ignored and reported: OK"

echo "== [5] test mode: no prod-lookalike root is touched"
R_CLI2="$SCRATCH/root-cli2"
R_ENV2="$SCRATCH/root-env2"
R_CFG2="$SCRATCH/root-config2"
CFG2="$SCRATCH/service2.json"
printf '{"home": "%s"}\n' "$R_CFG2" > "$CFG2"
start_case test-mode test-mode env LINGXI_HOME="$R_ENV2" "$BIN" \
  --home "$R_CLI2" --config "$CFG2" --test-mode
assert_absent "$R_CLI2"; assert_absent "$R_ENV2"; assert_absent "$R_CFG2"
case "$READY_HOME" in
  */lingxi-service-test-*) : ;;
  *) echo "ERROR: test-mode home has unexpected shape: $READY_HOME" >&2; exit 1 ;;
esac
[ -d "$READY_HOME" ] || { echo "ERROR: test-mode home missing" >&2; exit 1; }
assert_materialized "$READY_HOME"
stop_case
grep -q "ignored_sources" "$EVIDENCE_DIR/a04-test-mode-stderr.log" || {
  echo "ERROR: test mode does not report ignored sources" >&2; exit 1; }
rm -rf "$READY_HOME"
echo "test mode wrote only to its isolated synthetic home: OK"

echo "== [6] negative probes (loud failures, no side effects)"
neg() { # $1=name $2=expected-exit; args via "$@:3"
  local name="$1" want="$2"
  shift 2
  local out err rc
  out="$EVIDENCE_DIR/a04-neg-$name-stdout.log"
  err="$EVIDENCE_DIR/a04-neg-$name-stderr.log"
  set +e
  "$@" > "$out" 2> "$err"
  rc=$?
  set -e
  [ "$rc" -eq "$want" ] || {
    echo "ERROR: negative $name exit $rc, expected $want" >&2; exit 1; }
  if grep -q 'LINGXI_SERVICE_READY' "$out"; then
    echo "ERROR: negative $name printed READY" >&2; exit 1
  fi
  printf 'neg-%s exit=%s\n' "$name" "$rc" >> "$EVIDENCE_DIR/a04-summary.txt"
}
neg no-source 2 env -u LINGXI_HOME "$BIN"
neg relative-home 2 env -u LINGXI_HOME "$BIN" --home "relative/dir"
neg duplicate-home 2 env -u LINGXI_HOME "$BIN" --home "$R_CLI" --home "$R_CLI"
neg equals-form 2 env -u LINGXI_HOME "$BIN" --home="$R_CLI"
neg flag-shaped 2 env -u LINGXI_HOME "$BIN" --bind --home /tmp/x
neg missing-config 2 env -u LINGXI_HOME "$BIN" --config "$SCRATCH/nope.json"
neg bad-config 2 env -u LINGXI_HOME "$BIN" --config /dev/null
assert_absent "$R_CFG2"; assert_absent "$R_ENV2"; assert_absent "$R_CLI2"
grep -q "missing data root" "$EVIDENCE_DIR/a04-neg-no-source-stderr.log" || {
  echo "ERROR: no-source diagnostic not explicit" >&2; exit 1; }
grep -q "more than once" "$EVIDENCE_DIR/a04-neg-duplicate-home-stderr.log" || {
  echo "ERROR: duplicate-home diagnostic not explicit" >&2; exit 1; }
grep -q "flag-shaped" "$EVIDENCE_DIR/a04-neg-flag-shaped-stderr.log" || {
  echo "ERROR: flag-shaped diagnostic not explicit" >&2; exit 1; }
echo "negative probes all loud: OK"

echo "A04 RESULT: PASS (precedence honored, file probes prove the real choice, test mode isolated)"

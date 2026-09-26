#!/usr/bin/env bash
# R02-T08 / acceptance R02-A15 — the REAL binary completes the full chain:
#
#   start -> authenticate -> write data -> subscribe (WS live events)
#         -> graceful close -> RESTART -> read the pre-restart data back
#   ...and NO leftover child processes / listening ports after shutdown.
#
# This is the T01–T07 capability chain (service composition root, config/
# instance lock, HTTP/WS auth, storage transactions, event subscription,
# shutdown coordination, redacted logging) driven end-to-end against ONE
# real lingxi-service process pair on a fresh synthetic home — not test
# functions, not in-process mocks.
#
# Environment guards (T01–T07 convention):
#   - rustup-locked toolchain, --locked offline build, task-dedicated
#     CARGO_TARGET_DIR (RR-T08-F1), proxy vars stripped;
#   - fresh synthetic home under /tmp every run (no real user data);
#   - cleanup kills the service and removes the home; leftover-process and
#     port checks run AFTER the stop as acceptance evidence.
#
# Usage: scripts/rust-tauri/r02_t08_full_chain_smoke.sh [EVIDENCE_DIR]
# Exit 0 only if every step holds.
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R02/T08/A15-direct}"
EVIDENCE_DIR="$EVIDENCE_DIR/full-chain"
mkdir -p "$EVIDENCE_DIR"
TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rust-target-r02-t08}"

TOOLCHAIN="$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml | head -n 1)"
[ -n "$TOOLCHAIN" ] || { echo "ERROR: no toolchain in rust-toolchain.toml" >&2; exit 1; }
if ! command -v rustup >/dev/null 2>&1; then
  if [ -x "$HOME/.cargo/bin/rustup" ]; then PATH="$HOME/.cargo/bin:$PATH";
  else echo "ERROR: rustup not found" >&2; exit 1; fi
fi
CARGO="env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TARGET_DIR rustup run $TOOLCHAIN cargo"

HOME_DIR=""
SERVICE_PID=""
cleanup() {
  if [ -n "$SERVICE_PID" ] && kill -0 "$SERVICE_PID" 2>/dev/null; then
    kill -TERM "$SERVICE_PID" 2>/dev/null || true
    wait "$SERVICE_PID" 2>/dev/null || true
  fi
  [ -n "$HOME_DIR" ] && [ -d "$HOME_DIR" ] && rm -rf "$HOME_DIR"
  return 0
}
trap cleanup EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }
note() { printf '%s\n' "$*" | tee -a "$EVIDENCE_DIR/summary.txt"; }
RUN_STAMP="$(date '+%Y-%m-%dT%H:%M:%S%z')"
note "== run $RUN_STAMP (pid $$) =="
note "== R02-T08 / R02-A15 full-chain smoke (toolchain $TOOLCHAIN, target $TARGET_DIR) =="

note "== building lingxi-service (--locked, offline) =="
$CARGO build --manifest-path rust/Cargo.toml --locked -p lingxi-service \
  > "$EVIDENCE_DIR/build.log" 2>&1 || { tail -30 "$EVIDENCE_DIR/build.log"; fail "build failed"; }
BIN="$TARGET_DIR/debug/lingxi-service"
[ -x "$BIN" ] || fail "binary missing at $BIN"
note "PASS build"

wait_ready() { # $1=stdout log $2=pid
  for _ in $(seq 1 300); do
    kill -0 "$2" 2>/dev/null || { cat "$EVIDENCE_DIR/service1.err" "$EVIDENCE_DIR/service2.err" 2>/dev/null >&2; fail "service died during startup"; }
    READY="$(grep -o 'LINGXI_SERVICE_READY addr=[^ ]*' "$1" 2>/dev/null | tail -n 1 || true)"
    [ -n "$READY" ] && { printf '%s' "${READY#LINGXI_SERVICE_READY addr=}"; return 0; }
    sleep 0.1
  done
  fail "no READY line within 30s"
}

leftover_check() { # $1=label
  local label="$1" procs port
  # any lingxi-service process still bound to THIS synthetic home?
  procs="$(pgrep -f "lingxi-service --home $HOME_DIR" || true)"
  [ -z "$procs" ] || fail "$label: leftover service processes: $procs"
  note "PASS $label-no-leftover-processes (pgrep -f 'lingxi-service --home <synthetic>' -> empty)"
  # the ephemeral port must not accept connections any more
  if curl -sS --noproxy '*' --max-time 2 -o /dev/null "http://$ADDR/lingxi/v1/health" 2>/dev/null; then
    fail "$label: port $ADDR still accepting after shutdown"
  fi
  note "PASS $label-port-closed (health probe on $ADDR refused)"
}

# ── boot 1: start -> authenticate -> write -> subscribe ────────────────────
note "== boot 1: start, authenticate, write, subscribe =="
HOME_DIR="$(mktemp -d /tmp/lingxi-r02t08-a15-home.XXXXXX)"
export A15_HEAD_FILE="$EVIDENCE_DIR/pre-restart-head.json"
"$BIN" --home "$HOME_DIR" > "$EVIDENCE_DIR/service1.out" 2> "$EVIDENCE_DIR/service1.err" &
SERVICE_PID=$!
ADDR="$(wait_ready "$EVIDENCE_DIR/service1.out" "$SERVICE_PID")"
note "PASS boot1-ready addr=$ADDR home=$HOME_DIR"

python3 scripts/rust-tauri/r02_t08_full_chain_probe.py boot-and-write "${ADDR##*:}" "$HOME_DIR" \
  2> "$EVIDENCE_DIR/probe1.err" > "$EVIDENCE_DIR/probe1.pass-lines"
{ cat "$EVIDENCE_DIR/probe1.pass-lines"; cat "$EVIDENCE_DIR/probe1.err"; } | tee -a "$EVIDENCE_DIR/summary.txt"
if [ -s "$EVIDENCE_DIR/probe1.err" ]; then cat "$EVIDENCE_DIR/probe1.err" >&2; fail "probe phase 1 failed"; fi
note "PASS phase1 (health/auth-negative/auth/write/subscribe/live-event/read-your-writes/future-cursor)"

# ── graceful close 1 ────────────────────────────────────────────────────────
note "== close 1: SIGTERM graceful shutdown =="
kill -TERM "$SERVICE_PID"
set +e; wait "$SERVICE_PID"; EXIT1=$?; set -e
SERVICE_PID=""
[ "$EXIT1" -eq 0 ] || fail "expected exit 0 on graceful stop, got $EXIT1"
note "PASS close1-exit-0 (service exit code $EXIT1)"
leftover_check "close1"

# ── boot 2: restart on the SAME home -> read the data back ─────────────────
note "== boot 2: restart on the same home, read back =="
"$BIN" --home "$HOME_DIR" > "$EVIDENCE_DIR/service2.out" 2> "$EVIDENCE_DIR/service2.err" &
SERVICE_PID=$!
ADDR="$(wait_ready "$EVIDENCE_DIR/service2.out" "$SERVICE_PID")"
note "PASS boot2-ready addr=$ADDR"

python3 scripts/rust-tauri/r02_t08_full_chain_probe.py readback "${ADDR##*:}" "$HOME_DIR" \
  2> "$EVIDENCE_DIR/probe2.err" > "$EVIDENCE_DIR/probe2.pass-lines"
{ cat "$EVIDENCE_DIR/probe2.pass-lines"; cat "$EVIDENCE_DIR/probe2.err"; } | tee -a "$EVIDENCE_DIR/summary.txt"
if [ -s "$EVIDENCE_DIR/probe2.err" ]; then cat "$EVIDENCE_DIR/probe2.err" >&2; fail "probe phase 2 failed"; fi
note "PASS phase2 (old-token-rejected/new-token auth / session readback / events preserved / health)"

# ── graceful close 2 + final leftover checks ────────────────────────────────
note "== close 2: SIGTERM graceful shutdown =="
kill -TERM "$SERVICE_PID"
set +e; wait "$SERVICE_PID"; EXIT2=$?; set -e
SERVICE_PID=""
[ "$EXIT2" -eq 0 ] || fail "expected exit 0 on second graceful stop, got $EXIT2"
note "PASS close2-exit-0 (service exit code $EXIT2)"
leftover_check "close2"

# instance record: a clean stop removes OUR record (T06 contract)
if [ -f "$HOME_DIR/lingxi-service/instance.json" ]; then
  fail "instance record still present after clean stop"
fi
note "PASS instance-record-removed (single-writer record cleaned on stop)"

note "== R02-T08 / R02-A15 full chain: ALL GREEN =="

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
[ ! -e "$EVIDENCE_DIR" ] || { echo "FAIL: A15 evidence directory already exists: $EVIDENCE_DIR" >&2; exit 1; }
mkdir -p "$EVIDENCE_DIR"
EVIDENCE_DIR="$(cd "$EVIDENCE_DIR" && pwd -P)"
TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/rust-target-r02-t08}"

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
# R02 stage-repair R7 / R7-F02: SERVICE_PID is the CURRENT handle — set
# on spawn, RETIRED (cleared) after every wait/reap (the close1/close2
# paths already did). The trap signals it ONLY while it still proves
# CURRENT ownership (exists AND ppid is THIS shell): a retired or
# recycled number is never signalled (R6-F02 A12 pattern).
# R12-F01: the boolean probe's false branch conflated exited/foreign/
# unobservable and the trap's owned branch was `kill -TERM; wait` with
# NO deadline — a child ignoring or delaying TERM hung the trap itself,
# and this trap runs on the FAILURE path of the very leftover-process
# acceptance it protects. Four-state probe + bounded ladder below
# (≈10 s worst case per handle); residue reported loudly at expiry,
# never an unbounded wait.
child_state() {
  # child_state <pid> → exited | owned | foreign | unobservable
  local ppid
  if ! kill -0 "$1" 2>/dev/null; then
    printf 'exited\n'
    return 0
  fi
  ppid="$(ps -o ppid= -p "$1" 2>/dev/null | tr -d '[:space:]')"
  if [ -z "$ppid" ]; then
    printf 'unobservable\n'
  elif [ "$ppid" = "$$" ]; then
    printf 'owned\n'
  else
    printf 'foreign\n'
  fi
}
# R12-F01: bounded stop for ONE provably-owned handle under THIS script's
# contract (TERM first — the close1/close2 graceful stops and their exit
# codes are the acceptance assertions and stay untouched): TERM → ≤5 s
# poll → direct-pid KILL only while still provably ours → ≤5 s re-check.
# Prints the final state.
bounded_stop_owned() {
  local pid="$1" state="" i
  kill -TERM "$pid" 2>/dev/null || true
  for i in $(seq 1 100); do
    state="$(child_state "$pid")"
    case "$state" in
      exited|foreign) break ;;
      owned|unobservable) : ;;
    esac
    sleep 0.05
  done
  if [ "${state:-owned}" = "owned" ]; then
    kill -KILL "$pid" 2>/dev/null || true
    for i in $(seq 1 100); do
      state="$(child_state "$pid")"
      case "$state" in
        exited|foreign) break ;;
        owned|unobservable) : ;;
      esac
      sleep 0.05
    done
  fi
  printf '%s\n' "${state:-unobservable}"
}
# 两次正常关停共用同一归属/限时规则；调用点仍核对退出码 0。
stop_service_gracefully() {
  local pid="$1" state
  [ "$(child_state "$pid")" = "owned" ] || fail "service $pid not owned before graceful stop"
  state="$(bounded_stop_owned "$pid")"
  case "$state" in
    exited) ;;
    *) fail "service $pid survived the TERM/KILL budget (state=$state)" ;;
  esac
  if wait "$pid"; then SERVICE_STOP_RC=0; else SERVICE_STOP_RC=$?; fi
}
cleanup() {
  local residue=0 state
  if [ -n "$SERVICE_PID" ]; then
    state="$(child_state "$SERVICE_PID")"
    case "$state" in
      owned)
        case "$(bounded_stop_owned "$SERVICE_PID")" in
          exited)
            wait "$SERVICE_PID" 2>/dev/null || true
            ;;
          *)
            echo "cleanup: pid $SERVICE_PID may still be running after stop budget — preserving synthetic home $HOME_DIR" >&2
            residue=1
            ;;
        esac
        ;;
      exited)
        wait "$SERVICE_PID" 2>/dev/null || true
        ;;
      foreign)
        echo "cleanup: pid $SERVICE_PID is NOT currently owned by this shell — preserving synthetic home $HOME_DIR" >&2
        residue=1
        ;;
      unobservable)
        echo "cleanup: pid $SERVICE_PID ownership UNOBSERVABLE (ps unreadable) — preserving synthetic home $HOME_DIR" >&2
        residue=1
        ;;
    esac
    SERVICE_PID=""
  fi
  if [ "$residue" -eq 0 ]; then
    [ -n "$HOME_DIR" ] && [ -d "$HOME_DIR" ] && rm -rf "$HOME_DIR"
  else
    # 无法证明服务已退出时保留数据根与现场，且整项验收失败。
    trap - EXIT
    exit 1
  fi
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
HOME_DIR=$(mktemp -d "${TMPDIR:-/tmp}/lingxi-r02t08-a15-home.XXXXXX")
export A15_HEAD_FILE="$EVIDENCE_DIR/pre-restart-head.json"
export A15_EVIDENCE_DIR="$EVIDENCE_DIR"
"$BIN" --home "$HOME_DIR" > "$EVIDENCE_DIR/service1.out" 2> "$EVIDENCE_DIR/service1.err" &
SERVICE_PID=$!
ADDR="$(wait_ready "$EVIDENCE_DIR/service1.out" "$SERVICE_PID")"
note "PASS boot1-ready addr=$ADDR home=$HOME_DIR"
# 旧令牌仅保留在本脚本内存，供重启后的拒绝断言使用，不写入交付证据。
OLD_TOKEN="$(python3 -c 'import json,sys; from pathlib import Path; print(json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))["token"])' "$HOME_DIR/lingxi-service/local-token.json")"
[ -n "$OLD_TOKEN" ] || fail "pre-restart token missing"

python3 scripts/rust-tauri/r02_t08_full_chain_probe.py boot-and-write "${ADDR##*:}" "$HOME_DIR" \
  2> "$EVIDENCE_DIR/probe1.err" > "$EVIDENCE_DIR/probe1.pass-lines"
{ cat "$EVIDENCE_DIR/probe1.pass-lines"; cat "$EVIDENCE_DIR/probe1.err"; } | tee -a "$EVIDENCE_DIR/summary.txt"
if [ -s "$EVIDENCE_DIR/probe1.err" ]; then cat "$EVIDENCE_DIR/probe1.err" >&2; fail "probe phase 1 failed"; fi
note "PASS phase1 (health/auth-negative/auth/write/subscribe/live-event/read-your-writes/future-cursor)"

# ── graceful close 1 ────────────────────────────────────────────────────────
note "== close 1: SIGTERM graceful shutdown =="
stop_service_gracefully "$SERVICE_PID"
EXIT1="$SERVICE_STOP_RC"
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

printf '%s\n' "$OLD_TOKEN" | python3 scripts/rust-tauri/r02_t08_full_chain_probe.py readback "${ADDR##*:}" "$HOME_DIR" \
  2> "$EVIDENCE_DIR/probe2.err" > "$EVIDENCE_DIR/probe2.pass-lines"
{ cat "$EVIDENCE_DIR/probe2.pass-lines"; cat "$EVIDENCE_DIR/probe2.err"; } | tee -a "$EVIDENCE_DIR/summary.txt"
if [ -s "$EVIDENCE_DIR/probe2.err" ]; then cat "$EVIDENCE_DIR/probe2.err" >&2; fail "probe phase 2 failed"; fi
note "PASS phase2 (old-token-rejected/new-token auth / session readback / events preserved / health)"

# ── graceful close 2 + final leftover checks ────────────────────────────────
note "== close 2: SIGTERM graceful shutdown =="
stop_service_gracefully "$SERVICE_PID"
EXIT2="$SERVICE_STOP_RC"
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

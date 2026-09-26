#!/usr/bin/env bash
# R02-T02 / acceptance R02-A03 — dual-instance write rejection, with REAL
# binary processes (not in-process mocks).
#
# Proves:
#   A. instance 1 (real binary) holds a synthetic test data home and serves;
#   B. instance 2 (real binary, SAME home) is explicitly rejected:
#      exit code 3 + LINGXI_SERVICE_SINGLE_WRITER_BLOCKED stderr marker with
#      recorded pid/instanceId/startNonce/addr and probe=live;
#   C. instance 1 keeps serving (health 200) across the rejection and its
#      data is byte-identical before/after (file-tree hash comparison —
#      the "库校验" for the data surface that exists at T02);
#   D. an ALIASED second start (via the /private prefix of the macOS /tmp
#      symlink) is also rejected (canonicalization collapses aliases);
#   E. crash semantics: SIGKILL instance 1 -> record left behind -> restart
#      (instance 3) DETECTS the stale record (STALE_RECORD_TAKEN_OVER
#      marker + archived instance.stale.json matching the dead owner) and
#      takes over; clean SIGTERM afterwards removes its OWN record (exit 0);
#   F. PID-reuse adversarial: a forged record claiming a LIVE unrelated pid
#      (pid 1) with no lock held does NOT block takeover (the decision
#      never consults pid existence).
#
# Environment guards: same as r02_t01_service_smoke.sh (rustup-locked
# toolchain, task-dedicated target dir, offline locked build, proxy vars
# stripped, synthetic /tmp home only).
#
# Usage: scripts/rust-tauri/r02_t02_dual_instance.sh [EVIDENCE_DIR]
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

HOME_A=""
INST1_PID=""
INST3_PID=""
INST4_PID=""
cleanup() {
  for pid in "$INST1_PID" "$INST3_PID" "$INST4_PID"; do
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
      echo "cleanup: service $pid still running, sending SIGTERM" >&2
      kill -TERM "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
  done
  for h in "$HOME_A"; do
    if [ -n "$h" ] && [ -d "$h" ]; then rm -rf "$h"; fi
  done
}
trap cleanup EXIT

wait_ready() { # $1=pid $2=stdout-log $3=what
  for _ in $(seq 1 300); do
    if ! kill -0 "$1" 2>/dev/null; then
      echo "ERROR: $3 exited before becoming ready" >&2
      return 1
    fi
    if grep -q 'LINGXI_SERVICE_READY addr=' "$2" 2>/dev/null; then return 0; fi
    sleep 0.1
  done
  echo "ERROR: no READY line from $3 within 30s" >&2
  return 1
}

ready_addr() { # $1=stdout-log
  grep -o 'LINGXI_SERVICE_READY addr=[^ ]*' "$1" | tail -n 1 | sed 's/^LINGXI_SERVICE_READY addr=//'
}

tree_hash() { # deterministic digest of every file under a root
  find "$1" -type f | sort | while read -r f; do
    printf '%s  ' "${f#"$1"}"
    shasum -a 256 "$f" | awk '{print $1}'
  done | shasum -a 256 | awk '{print $1}'
}

echo "== toolchain: rustup run $TOOLCHAIN ($(rustup run "$TOOLCHAIN" rustc --version | head -n 1))"
echo "== [build] lingxi-service (locked, offline, isolated target dir)"
$CARGO build --manifest-path rust/Cargo.toml --locked -p lingxi-service \
  > "$EVIDENCE_DIR/a03-build.log" 2>&1
tail -n 1 "$EVIDENCE_DIR/a03-build.log"
BIN="$TARGET_DIR/debug/lingxi-service"
test -x "$BIN"

HOME_A="$(mktemp -d /tmp/lingxi-r02t02-a03-home.XXXXXX)"

echo "== [A] start instance 1 on the synthetic home"
"$BIN" --home "$HOME_A" \
  > "$EVIDENCE_DIR/a03-inst1-stdout.log" 2> "$EVIDENCE_DIR/a03-inst1-stderr.log" &
INST1_PID=$!
wait_ready "$INST1_PID" "$EVIDENCE_DIR/a03-inst1-stdout.log" "instance 1"
ADDR1="$(ready_addr "$EVIDENCE_DIR/a03-inst1-stdout.log")"
echo "instance 1: pid=$INST1_PID addr=$ADDR1 home=$HOME_A"
HTTP_CODE="$(curl -sS -o "$EVIDENCE_DIR/a03-inst1-health.json" -w '%{http_code}' "http://$ADDR1/lingxi/v1/health")"
[ "$HTTP_CODE" = "200" ] || { echo "ERROR: instance 1 health=$HTTP_CODE" >&2; exit 1; }
RECORD1="$(cat "$HOME_A/lingxi-service/instance.json")"
INST1_ID="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["instanceId"])' <<<"$RECORD1")"
echo "instance 1 record instanceId=$INST1_ID"

HASH_BEFORE="$(tree_hash "$HOME_A")"
echo "home tree hash before rejection: $HASH_BEFORE"

echo "== [B] second instance, SAME home -> must be rejected (exit 3 + marker)"
set +e
"$BIN" --home "$HOME_A" \
  > "$EVIDENCE_DIR/a03-inst2-stdout.log" 2> "$EVIDENCE_DIR/a03-inst2-stderr.log"
INST2_RC=$?
set -e
echo "instance 2 exit code: $INST2_RC"
[ "$INST2_RC" -eq 3 ] || { echo "ERROR: expected exit 3" >&2; exit 1; }
grep -q "^LINGXI_SERVICE_SINGLE_WRITER_BLOCKED " "$EVIDENCE_DIR/a03-inst2-stderr.log" || {
  echo "ERROR: no single-writer marker in instance 2 stderr" >&2; exit 1; }
grep -q "recordedPid=$INST1_PID " "$EVIDENCE_DIR/a03-inst2-stderr.log" || {
  echo "ERROR: marker does not name instance 1 pid" >&2; exit 1; }
grep -q "recordedInstanceId=$INST1_ID " "$EVIDENCE_DIR/a03-inst2-stderr.log" || {
  echo "ERROR: marker does not name instance 1 instanceId" >&2; exit 1; }
grep -q "probe=live" "$EVIDENCE_DIR/a03-inst2-stderr.log" || {
  echo "ERROR: diagnostic probe did not confirm the live peer" >&2; exit 1; }
if grep -q 'LINGXI_SERVICE_READY' "$EVIDENCE_DIR/a03-inst2-stdout.log"; then
  echo "ERROR: rejected instance printed a READY line" >&2; exit 1
fi
echo "instance 2 rejected as required (marker + diagnostics + no READY)"
grep -m1 "^LINGXI_SERVICE_SINGLE_WRITER_BLOCKED" "$EVIDENCE_DIR/a03-inst2-stderr.log" \
  | tee "$EVIDENCE_DIR/a03-inst2-marker.txt"

echo "== [C] instance 1 unaffected; data byte-identical"
kill -0 "$INST1_PID" || { echo "ERROR: instance 1 died" >&2; exit 1; }
HTTP_CODE="$(curl -sS -o /dev/null -w '%{http_code}' "http://$ADDR1/lingxi/v1/health")"
[ "$HTTP_CODE" = "200" ] || { echo "ERROR: instance 1 health after rejection=$HTTP_CODE" >&2; exit 1; }
HASH_AFTER="$(tree_hash "$HOME_A")"
echo "home tree hash after rejection:  $HASH_AFTER"
[ "$HASH_BEFORE" = "$HASH_AFTER" ] || {
  echo "ERROR: home data changed across the rejection" >&2; exit 1; }
echo "instance 1 still healthy and data unchanged: OK"

echo "== [D] aliased second start (macOS /private alias of the same dir) also rejected"
ALIAS="/private${HOME_A}"
set +e
"$BIN" --home "$ALIAS" > "$EVIDENCE_DIR/a03-inst2b-stdout.log" 2> "$EVIDENCE_DIR/a03-inst2b-stderr.log"
INST2B_RC=$?
set -e
echo "aliased instance exit code: $INST2B_RC"
[ "$INST2B_RC" -eq 3 ] || { echo "ERROR: aliased start must also be rejected" >&2; exit 1; }
grep -q "^LINGXI_SERVICE_SINGLE_WRITER_BLOCKED " "$EVIDENCE_DIR/a03-inst2b-stderr.log" || {
  echo "ERROR: no marker for aliased rejection" >&2; exit 1; }
echo "alias collapsed to the same canonical root: OK"

echo "== [E] SIGKILL crash -> stale record -> takeover restart -> clean stop"
kill -KILL "$INST1_PID"
wait "$INST1_PID" 2>/dev/null || true
INST1_PID=""
[ -f "$HOME_A/lingxi-service/instance.json" ] || {
  echo "ERROR: crash should leave the record behind" >&2; exit 1; }
"$BIN" --home "$HOME_A" \
  > "$EVIDENCE_DIR/a03-inst3-stdout.log" 2> "$EVIDENCE_DIR/a03-inst3-stderr.log" &
INST3_PID=$!
wait_ready "$INST3_PID" "$EVIDENCE_DIR/a03-inst3-stdout.log" "instance 3 (takeover)"
grep -q "^LINGXI_SERVICE_STALE_RECORD_TAKEN_OVER " "$EVIDENCE_DIR/a03-inst3-stderr.log" || {
  echo "ERROR: takeover restart did not log the stale-record marker" >&2; exit 1; }
grep -q "previousInstanceId=$INST1_ID " "$EVIDENCE_DIR/a03-inst3-stderr.log" || {
  echo "ERROR: takeover marker does not name the dead owner" >&2; exit 1; }
STALE_ARCHIVED="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["instanceId"])' \
  < "$HOME_A/lingxi-service/instance.stale.json")"
[ "$STALE_ARCHIVED" = "$INST1_ID" ] || {
  echo "ERROR: stale archive does not match the dead owner" >&2; exit 1; }
ADDR3="$(ready_addr "$EVIDENCE_DIR/a03-inst3-stdout.log")"
HTTP_CODE="$(curl -sS -o /dev/null -w '%{http_code}' "http://$ADDR3/lingxi/v1/health")"
[ "$HTTP_CODE" = "200" ] || { echo "ERROR: takeover instance health=$HTTP_CODE" >&2; exit 1; }
echo "instance 3 took over the stale home and serves: OK"
kill -TERM "$INST3_PID"
set +e
wait "$INST3_PID"
INST3_RC=$?
set -e
INST3_PID=""
echo "instance 3 exit after SIGTERM: $INST3_RC"
[ "$INST3_RC" -eq 0 ] || { echo "ERROR: expected clean exit 0" >&2; exit 1; }
[ ! -f "$HOME_A/lingxi-service/instance.json" ] || {
  echo "ERROR: graceful stop must remove the OWN record" >&2; exit 1; }
[ -f "$HOME_A/lingxi-service/instance.lock" ] || {
  echo "ERROR: lock file is kept (never deleted) — expected present" >&2; exit 1; }
echo "graceful stop removed own record, kept lock file: OK"

echo "== [F] forged record with a LIVE unrelated pid does not block takeover"
python3 - "$HOME_A" "$INST1_ID" > "$EVIDENCE_DIR/a03-forged-record.json" <<'PY'
import json, sys
home, old_id = sys.argv[1], sys.argv[2]
record = {
    "schemaVersion": 1,
    "serverKind": "lingxi-service",
    "serverVersion": "0.0.0",
    "wireProtocolMin": 1,
    "wireProtocolMax": 1,
    "dataEpoch": 1,
    "instanceId": old_id,
    "startNonce": "0000000000000000",
    "pid": 1,  # launchd on macOS: alive, and definitively not our service
    "bindAddr": None,
    "startedAtUnixMs": 0,
    "homePath": home,
}
print(json.dumps(record, indent=2))
PY
cp "$EVIDENCE_DIR/a03-forged-record.json" "$HOME_A/lingxi-service/instance.json"
"$BIN" --home "$HOME_A" \
  > "$EVIDENCE_DIR/a03-inst4-stdout.log" 2> "$EVIDENCE_DIR/a03-inst4-stderr.log" &
INST4_PID=$!
wait_ready "$INST4_PID" "$EVIDENCE_DIR/a03-inst4-stdout.log" "instance 4 (pid-reuse adversarial)"
grep -q "^LINGXI_SERVICE_STALE_RECORD_TAKEN_OVER " "$EVIDENCE_DIR/a03-inst4-stderr.log" || {
  echo "ERROR: forged live-pid record blocked takeover (decision must ignore pid existence)" >&2
  exit 1; }
kill -TERM "$INST4_PID"
set +e
wait "$INST4_PID"
INST4_RC=$?
set -e
INST4_PID=""
[ "$INST4_RC" -eq 0 ] || { echo "ERROR: instance 4 clean stop expected" >&2; exit 1; }
echo "takeover proceeded despite a live recorded pid (lock is the authority): OK"

rm -rf "$HOME_A"; HOME_A=""
echo "A03 RESULT: PASS (dual-instance rejection, first instance intact, stale takeover, pid-reuse adversarial)"

# Keep parseable evidence summary for the report.
{
  echo "inst2_exit=$INST2_RC marker=verified probe=live"
  echo "hash_before=$HASH_BEFORE"
  echo "hash_after=$HASH_AFTER"
  echo "stale_archive_instance_id=$STALE_ARCHIVED"
  echo "inst3_exit_after_sigterm=$INST3_RC"
  echo "inst4_takeover_with_live_recorded_pid=ok"
} > "$EVIDENCE_DIR/a03-summary.txt"

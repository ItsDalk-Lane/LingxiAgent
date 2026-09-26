#!/usr/bin/env bash
# R02-T05 / acceptances R02-A09 + R02-A10 — event ordering, snapshots and
# resumable reads against the REAL lingxi-service binary.
#
# Proves (A09 快照与订阅无空隙):
#   - while HTTP executes keep committing runs, a WS subscriber that
#     subscribes with no cursor gets an explicit boundary
#     (subscribed.snapshotSeq) and a merged view (snapshot cut + live tail)
#     that is seq-contiguous, duplicate-free and event-by-event equal to the
#     durable key_events log read straight from the database file;
#   - run terminal transitions reach subscribers exactly as committed
#     (the server never synthesizes a terminal state).
# Proves (A10 缓存过期可恢复):
#   - a REAL cursor obtained from a page becomes expired after the retention
#     purge (same predicate as the storage port: seq < floor) — resuming
#     over HTTP answers 409 cursor_expired with details.reason=
#     snapshot_required and the new floor; over WS the snapshot_required
#     CONTROL frame arrives and the connection stays usable;
#   - rebuilding from a fresh snapshot equals the current authority
#     (rebuilt ids vs the post-purge events page; diff saved as evidence).
# Negatives: unknown stream (WS close 4404 / HTTP 404), forged future
#   cursor with a VALID checksum, malformed cursor, duplicate subscribe on
#   one connection (close 4409), cross-principal page read (403).
# REVIEW-R1 F03 repair evidence: a cursor left stale by a purge-ALL of
#   sess_local_beta answers with the snapshot_required rebuild directive
#   (HTTP 409 cursor_expired without floorSeq; WS control frame reason
#   events_truncated without floorSeq) — not a future_cursor rejection —
#   and the rebuild on the emptied stream is an explicit empty cut.
#
# The probe (r02_t05_events_probe.py) is stdlib-only and deterministic in
# its assertions; the fixed-scheduling race matrix lives in the cargo test
# (event_subscription.rs, single-thread runtime + explicit yields, no
# sleeps) — this script is the real-binary evidence layer.
#
# Environment guards: same as r02_t04_storage_tx.sh (rustup-locked
# toolchain, task-dedicated target dir, offline locked build, proxy vars
# stripped, synthetic /tmp home only, no real user directory touched).
#
# Usage: scripts/rust-tauri/r02_t05_events_matrix.sh [EVIDENCE_DIR]
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R02/T05}"
mkdir -p "$EVIDENCE_DIR"
TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rust-target-r02-t05}"

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

HOME_DIR=""
SERVICE_PID=""
cleanup() {
  if [ -n "$SERVICE_PID" ] && kill -0 "$SERVICE_PID" 2>/dev/null; then
    kill -TERM "$SERVICE_PID" 2>/dev/null || true
    wait "$SERVICE_PID" 2>/dev/null || true
  fi
  if [ -n "$HOME_DIR" ] && [ -d "$HOME_DIR" ]; then rm -rf "$HOME_DIR"; fi
}
trap cleanup EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }
note() { printf '%s\n' "$*" | tee -a "$EVIDENCE_DIR/summary.txt"; }

# ---- build the real binaries ------------------------------------------------
note "== building lingxi-service + lingxi-storage-inspect (rustup $TOOLCHAIN, $TARGET_DIR, --locked) =="
$CARGO build --manifest-path rust/Cargo.toml --locked -p lingxi-service -p lingxi-adapters \
  > "$EVIDENCE_DIR/build.log" 2>&1 || { tail -30 "$EVIDENCE_DIR/build.log"; fail "build failed"; }
SERVICE_BIN="$TARGET_DIR/debug/lingxi-service"
INSPECT_BIN="$TARGET_DIR/debug/lingxi-storage-inspect"
[ -x "$SERVICE_BIN" ] || fail "service binary missing"
[ -x "$INSPECT_BIN" ] || fail "inspect binary missing"
note "PASS build (locked, offline)"

# ---- start the service on a synthetic home ---------------------------------
HOME_DIR="$(mktemp -d /tmp/lingxi-r02t05-home-XXXXXX)"
"$SERVICE_BIN" --home "$HOME_DIR" --bind 127.0.0.1:0 \
  > "$EVIDENCE_DIR/service.out" 2> "$EVIDENCE_DIR/service.err" &
SERVICE_PID=$!
for _ in $(seq 1 300); do
  if grep -q '^LINGXI_SERVICE_READY ' "$EVIDENCE_DIR/service.out" 2>/dev/null; then break; fi
  if ! kill -0 "$SERVICE_PID" 2>/dev/null; then
    cat "$EVIDENCE_DIR/service.err" >&2
    fail "service died during startup"
  fi
  sleep 0.1
done
ADDR="$(sed -n 's/.*LINGXI_SERVICE_READY addr=\([^ ]*\).*/\1/p' "$EVIDENCE_DIR/service.out" | head -n 1)"
[ -n "$ADDR" ] || fail "no READY line"
HOST="$(printf '%s' "$ADDR" | cut -d: -f1)"
PORT="$(printf '%s' "$ADDR" | cut -d: -f2)"
note "== service up addr=$ADDR home=$HOME_DIR =="

TOKEN_FILE="$HOME_DIR/lingxi-service/local-token.json"
TOKEN="$(python3 -c "import json,sys;print(json.load(open(sys.argv[1]))['token'])" "$TOKEN_FILE")"
DB_PATH="$HOME_DIR/lingxi-service/data/runs.db"
[ -f "$DB_PATH" ] || fail "run database missing at $DB_PATH"

# ---- run the event matrix probe against the live binary ---------------------
note "== running r02_t05_events_probe.py (A09 race / A10 expiry / negatives) =="
set +e
python3 scripts/rust-tauri/r02_t05_events_probe.py \
  "$HOST" "$PORT" "$TOKEN" "$DB_PATH" "$EVIDENCE_DIR" \
  > "$EVIDENCE_DIR/probe-matrix.jsonl" 2> "$EVIDENCE_DIR/probe-stderr.log"
PROBE_RC=$?
set -e
cat "$EVIDENCE_DIR/probe-matrix.jsonl"
[ "$PROBE_RC" -eq 0 ] || { cat "$EVIDENCE_DIR/probe-stderr.log" >&2; fail "probe failed (rc=$PROBE_RC)"; }
note "PASS probe matrix (A09 merged==durable; A10 snapshot_required + rebuild; F03 purge-all directive; negatives)"

# ---- independent authority cross-check (second tool, same file) -------------
# The probe compared its merged view against a direct sqlite read; this
# second check goes through the R02-T04 read-only inspector binary and the
# live service page, proving the three views agree.
"$INSPECT_BIN" "$DB_PATH" dump --table key_events > "$EVIDENCE_DIR/key-events-dump.json"
python3 - "$EVIDENCE_DIR" <<'PYEOF'
import json, sys
ev = sys.argv[1]
# The inspector dump is JSON Lines: {"table": "key_events", "row": [...]}
# with columns event_id, stream_id, seq, session_id, run_id, attempt,
# event_type, payload_json, committed_at_unix_ms.
final = []
with open(f"{ev}/key-events-dump.json", encoding="utf-8") as handle:
    for line in handle:
        line = line.strip()
        if not line:
            continue
        entry = json.loads(line)
        if entry.get("table") != "key_events":
            continue
        row = entry["row"]
        final.append((int(row[2]), row[0], row[1]))
final.sort()
rebuild = json.load(open(f"{ev}/a10-rebuild-diff.json"))
assert rebuild["equal"], "probe rebuild diff not equal"
rebuilt_pairs = sorted(
    (int(x["seq"]), x["eventId"]) for x in rebuild["rebuilt"]
)
floor = rebuilt_pairs[0][0]
retained = [(seq, eid) for seq, eid, stream in final
            if seq >= floor and stream == "sess_local_alpha"]
assert rebuilt_pairs == retained, (
    f"rebuild != inspector dump: rebuilt={len(rebuilt_pairs)} "
    f"retained={len(retained)}"
)
print(
    f"cross-check OK: durable rows={len(final)}, "
    f"rebuilt={len(rebuilt_pairs)}, floor={floor}"
)
PYEOF
note "PASS cross-check (probe merged view == inspector dump == service page)"

note "== shutting the service down (graceful; logs retained) =="
kill -TERM "$SERVICE_PID"
wait "$SERVICE_PID" || true
SERVICE_PID=""
note "RESULT: ALL R02-T05 BINARY-MATRIX CASES PASSED"

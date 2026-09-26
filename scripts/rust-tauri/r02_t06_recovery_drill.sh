#!/usr/bin/env bash
# R02-T06 / acceptance R02-A12 + PROD-DEFECT-1 4-variant drill — the REAL
# lingxi-service binary against deliberately corrupted data homes.
#
# The four drill variants (mirroring the incumbent Node probe shapes of
# ADR-004 §2 / RR-T07-PROD-DEFECT-1; R02 new-stack equivalents):
#   v1  torn stamp + readable barrier_raised transition journal 1→2 (Node R7:
#       fail-open, wrote 55 entries, lied "no higher-epoch evidence was found")
#   v2  valid stamp + torn transition journal (Node R9: fail-open, wrote 55)
#   v3  MISSING stamp + readable barrier_raised journal 1→2 (Node R10: THE
#       fail-open shape — wrote 55 entries with an untruthful warning)
#   v4  healthy control (fresh home: stamps epoch 1 and starts normally)
# Expected in the new stack: v1–v3 REFUSE startup (exit 2) with truthful
# evidence (the readable journal's epochs are NAMED), every pre-existing
# file byte-identical, no stamp invented, no auth state written (the gate
# precedes the auth bootstrap); v4 starts and stops cleanly.
#
# Additional corruption forms for A12 (多形态损坏库):
#   v5  garbage main db file      -> storage open refuses NOTADB (exit 2),
#                                    corrupt file byte-identical, no empty
#                                    db created in its place
#   v6  torn WAL tail (INFO row)  -> SQLite recovers BY DESIGN (incomplete
#                                    trailing frame = transaction never
#                                    committed); the drill records the actual
#                                    outcome and asserts the recovered view
#                                    is transaction-consistent (no half run)
#   v7  tampered migration receipt-> SchemaTampered refusal (exit 2), file
#                                    byte-identical
#   v8  garbage WAL header (INFO) -> observed recovery-by-design row: the
#                                    SQLite WAL recovery discards invalid
#                                    frames; recorded with actual outcome
#
# Environment guards: rustup-locked toolchain 1.98.1, task-dedicated target
# dir, offline locked build, proxy vars stripped, synthetic /tmp homes only.
#
# Usage: scripts/rust-tauri/r02_t06_recovery_drill.sh [EVIDENCE_DIR]
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R02/T06}"
EVIDENCE_DIR="$EVIDENCE_DIR/recovery-drill"
mkdir -p "$EVIDENCE_DIR"
TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rust-target-r02-t06}"

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

SERVICE_PID=""
cleanup() {
  [ -n "$SERVICE_PID" ] && kill -9 "$SERVICE_PID" 2>/dev/null || true
  wait 2>/dev/null || true
}
trap cleanup EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }
note() { printf '%s\n' "$*" | tee -a "$EVIDENCE_DIR/summary.txt"; }

note "== building lingxi-service + lingxi-storage-inspect (rustup $TOOLCHAIN, $TARGET_DIR, --locked) =="
$CARGO build --manifest-path rust/Cargo.toml --locked -p lingxi-service -p lingxi-adapters \
  > "$EVIDENCE_DIR/build.log" 2>&1 || { cat "$EVIDENCE_DIR/build.log"; fail "build failed"; }
SERVICE_BIN="$TARGET_DIR/debug/lingxi-service"
INSPECT_BIN="$TARGET_DIR/debug/lingxi-storage-inspect"
[ -x "$SERVICE_BIN" ] || fail "service binary missing"
[ -x "$INSPECT_BIN" ] || fail "inspect binary missing"
note "PASS build (locked, offline)"

# run_probe <HOME> <TAG> — starts the binary, waits for READY or exit,
# records exit code / ready-line presence. Refusals exit 2 before binding.
run_probe() { # $1=home $2=tag -> sets PROBE_EXIT / PROBE_READY
  "$SERVICE_BIN" --home "$1" --bind 127.0.0.1:0 \
    > "$EVIDENCE_DIR/$2.out" 2> "$EVIDENCE_DIR/$2.err" &
  SERVICE_PID=$!
  PROBE_READY=0
  for _ in $(seq 1 200); do
    if grep -q '^LINGXI_SERVICE_READY ' "$EVIDENCE_DIR/$2.out" 2>/dev/null; then
      PROBE_READY=1
      break
    fi
    kill -0 "$SERVICE_PID" 2>/dev/null || break
    sleep 0.05
  done
  if [ "$PROBE_READY" = "1" ]; then
    # started: stop it cleanly and collect the real exit code
    kill -TERM "$SERVICE_PID"
    wait "$SERVICE_PID" 2>/dev/null
    PROBE_EXIT=$?
  else
    set +e
    wait "$SERVICE_PID" 2>/dev/null
    PROBE_EXIT=$?
    set -e
  fi
  SERVICE_PID=""
}

hash_metadata() { # $1=home -> hashes of every pre-existing data file
  (cd "$1" 2>/dev/null && {
    for f in data-epoch.json data-epoch-transition.json \
             lingxi-service/data/runs.db lingxi-service/data/runs.db-wal \
             lingxi-service/data/runs.db-shm; do
      [ -f "$f" ] && shasum -a 256 "$f"
    done
  }) || true
}

craft_barrier_journal() { # $1=path $2=phase
  python3 - "$1" "$2" <<'PYEOF'
import json, sys
path, phase = sys.argv[1], sys.argv[2]
journal = {
    "schemaVersion": 1,
    "transitionId": "transition-1-2",
    "fromEpoch": 1,
    "toEpoch": 2,
    "migrationIds": ["preferences-1-to-2"],
    "affectedStoreIds": ["user-preferences"],
    "recoveryModes": {"preferences-1-to-2": "restore-only"},
    "phase": phase,
    "createdAt": "2026-09-26T08:00:00.000Z",
    "updatedAt": "2026-09-26T08:00:00.000Z",
    "lastVersion": "2.0.0",
}
if phase == "prepared":
    journal["checkpointId"] = None
    journal["checkpointReceipt"] = None
else:
    journal["checkpointId"] = "checkpoint-1-2"
    journal["checkpointReceipt"] = {"id": "checkpoint-1-2"}
with open(path, "w") as fh:
    json.dump(journal, fh, indent=2)
    fh.write("\n")
PYEOF
}

# ---- v1: torn stamp + readable barrier_raised journal (Node R7 mirror) ------
note "== v1 (R02-A12/drill): torn stamp + readable higher-epoch journal =="
V1=$(mktemp -d /tmp/lingxi-r02t06-drill-v1-XXXXXX)
printf '{ "epoch": 1, "dat' > "$V1/data-epoch.json"          # torn stamp
craft_barrier_journal "$V1/data-epoch-transition.json" barrier_raised
hash_metadata "$V1" > "$EVIDENCE_DIR/v1-hashes-before.txt"
run_probe "$V1" v1
[ "$PROBE_EXIT" = "2" ] || fail "v1 must refuse with exit 2, got $PROBE_EXIT"
[ "$PROBE_READY" = "0" ] || fail "v1 must not publish readiness"
grep -q "LINGXI_DATA_EPOCH_TRANSITION_INCOMPLETE reason=corrupt-stamp" "$EVIDENCE_DIR/v1.err" \
  || fail "v1 stderr must carry the corrupt-stamp marker"
grep -q "evidence=transitionJournal(fromEpoch=1,toEpoch=2,phase=barrier_raised" "$EVIDENCE_DIR/v1.err" \
  || fail "v1 refusal must NAME the readable journal evidence"
grep -q "epochs 1→2" "$EVIDENCE_DIR/v1.err" \
  || fail "v1 refusal must state the higher-epoch target truthfully"
grep -q "no higher-epoch evidence was found" "$EVIDENCE_DIR/v1.err" \
  && fail "v1 must NOT print the incumbent lie"
[ ! -f "$V1/lingxi-service/local-token.json" ] || fail "v1: auth bootstrap must not run after a gate refusal"
hash_metadata "$V1" > "$EVIDENCE_DIR/v1-hashes-after.txt"
diff "$EVIDENCE_DIR/v1-hashes-before.txt" "$EVIDENCE_DIR/v1-hashes-after.txt" > /dev/null \
  || fail "v1: pre-existing files must be byte-identical after the refusal"
note "PASS v1 exit=2 reason=corrupt-stamp evidence 1→2 named; files byte-identical; no auth state written"

# ---- v2: valid stamp + torn journal (Node R9 mirror) -------------------------
note "== v2: valid stamp + torn transition journal =="
V2=$(mktemp -d /tmp/lingxi-r02t06-drill-v2-XXXXXX)
printf '{\n  "schemaVersion": 2,\n  "epoch": 1,\n  "minimumReaderEpoch": 1,\n  "committedDataEpoch": 1,\n  "lastVersion": "0.4.0",\n  "updatedAt": "2026-09-26T08:00:00.000Z"\n' > "$V2/data-epoch.json"   # torn: unterminated
printf '{ "schemaVersion": 1, "transitionId": "transition-1-2", "fro' > "$V2/data-epoch-transition.json"
hash_metadata "$V2" > "$EVIDENCE_DIR/v2-hashes-before.txt"
run_probe "$V2" v2
[ "$PROBE_EXIT" = "2" ] || fail "v2 must refuse with exit 2, got $PROBE_EXIT"
grep -q "LINGXI_DATA_EPOCH_TRANSITION_INCOMPLETE reason=corrupt-journal" "$EVIDENCE_DIR/v2.err" \
  || fail "v2 stderr must carry the corrupt-journal marker"
[ ! -f "$V2/lingxi-service/local-token.json" ] || fail "v2: auth bootstrap must not run"
hash_metadata "$V2" > "$EVIDENCE_DIR/v2-hashes-after.txt"
diff "$EVIDENCE_DIR/v2-hashes-before.txt" "$EVIDENCE_DIR/v2-hashes-after.txt" > /dev/null \
  || fail "v2: files must be byte-identical after the refusal"
note "PASS v2 exit=2 reason=corrupt-journal; files byte-identical"

# ---- v3: MISSING stamp + readable barrier_raised journal (Node R10 mirror) ---
note "== v3 (headline fail-open shape): missing stamp + readable higher-epoch journal =="
V3=$(mktemp -d /tmp/lingxi-r02t06-drill-v3-XXXXXX)
craft_barrier_journal "$V3/data-epoch-transition.json" barrier_raised
hash_metadata "$V3" > "$EVIDENCE_DIR/v3-hashes-before.txt"
run_probe "$V3" v3
[ "$PROBE_EXIT" = "2" ] || fail "v3 must refuse with exit 2, got $PROBE_EXIT (the incumbent Node gate REALLY WROTE here)"
grep -q "LINGXI_DATA_EPOCH_TRANSITION_INCOMPLETE reason=corrupt-transition" "$EVIDENCE_DIR/v3.err" \
  || fail "v3 stderr must carry the corrupt-transition marker"
grep -q "evidence=transitionJournal(fromEpoch=1,toEpoch=2,phase=barrier_raised" "$EVIDENCE_DIR/v3.err" \
  || fail "v3 refusal must NAME the readable journal evidence"
grep -q "no higher-epoch evidence was found" "$EVIDENCE_DIR/v3.err" \
  && fail "v3 must NOT print the incumbent lie"
[ ! -f "$V3/data-epoch.json" ] || fail "v3: no stamp may be invented for a refused home"
[ ! -f "$V3/lingxi-service/local-token.json" ] || fail "v3: auth bootstrap must not run"
hash_metadata "$V3" > "$EVIDENCE_DIR/v3-hashes-after.txt"
diff "$EVIDENCE_DIR/v3-hashes-before.txt" "$EVIDENCE_DIR/v3-hashes-after.txt" > /dev/null \
  || fail "v3: files must be byte-identical after the refusal"
note "PASS v3 exit=2 reason=corrupt-transition evidence 1→2 named; no stamp invented; files byte-identical"

# ---- v4: healthy control ------------------------------------------------------
note "== v4: healthy control (fresh home starts) =="
V4=$(mktemp -d /tmp/lingxi-r02t06-drill-v4-XXXXXX)
run_probe "$V4" v4
[ "$PROBE_READY" = "1" ] || fail "v4 healthy home must start (err: $(tail -2 "$EVIDENCE_DIR/v4.err"))"
[ "$PROBE_EXIT" = "0" ] || fail "v4 healthy service must stop cleanly (exit 0), got $PROBE_EXIT"
python3 - "$V4/data-epoch.json" <<'PYEOF' || fail "v4 stamp must be v2 epoch 1"
import json, sys
stamp = json.load(open(sys.argv[1]))
assert stamp["schemaVersion"] == 2, stamp
assert stamp["minimumReaderEpoch"] == 1 and stamp["committedDataEpoch"] == 1, stamp
PYEOF
note "PASS v4 fresh home stamped (v2, epoch 1) and served; graceful stop exit 0"

# ---- v5: garbage main db file -------------------------------------------------
note "== v5 (A12): garbage main db file =="
V5=$(mktemp -d /tmp/lingxi-r02t06-drill-v5-XXXXXX)
mkdir -p "$V5/lingxi-service/data"
python3 - <<PYEOF
import os
blob = bytes((i % 251) for i in range(8192))
blob = b"GARBAGE-NOT-A-DB" + blob[16:]
open("$V5/lingxi-service/data/runs.db", "wb").write(blob)
PYEOF
shasum -a 256 "$V5/lingxi-service/data/runs.db" > "$EVIDENCE_DIR/v5-hashes-before.txt"
run_probe "$V5" v5
[ "$PROBE_EXIT" = "2" ] || fail "v5 must refuse with exit 2, got $PROBE_EXIT"
grep -Eq "Corrupted|not a database|bootstrap failed" "$EVIDENCE_DIR/v5.err" \
  || fail "v5 stderr must name the corruption"
shasum -a 256 "$V5/lingxi-service/data/runs.db" > "$EVIDENCE_DIR/v5-hashes-after.txt"
diff "$EVIDENCE_DIR/v5-hashes-before.txt" "$EVIDENCE_DIR/v5-hashes-after.txt" > /dev/null \
  || fail "v5: the corrupt file must be preserved byte-identical (no empty-db substitution)"
note "PASS v5 exit=2 explicit corruption refusal; corrupt file byte-identical; no empty db substituted"

# ---- v6 (INFO row): torn WAL tail — recovery by design ------------------------
# Observed semantics (probe recorded in the report): after a REAL crash, an
# incomplete trailing WAL frame is the exact "transaction never committed"
# case SQLite is designed to recover from — recovery drops it and the
# database stays transaction-consistent (frames are only applied up to a
# commit frame; no half transaction can ever appear). This row therefore
# records the actual outcome and asserts the CONSISTENCY invariant instead
# of a refusal; the A12 refusal forms are v1/v2/v5/v7 (+v3 from the drill).
note "== v6 (INFO row): torn WAL tail (SQLite recovery by design) =="
V6=$(mktemp -d /tmp/lingxi-r02t06-drill-v6-XXXXXX)
mkdir -p "$V6/lingxi-service/data"
# Seed via the REAL service and crash it (kill -9) so a REAL WAL with
# committed-but-uncheckpointed data survives on disk.
"$SERVICE_BIN" --home "$V6" --bind 127.0.0.1:0 > "$EVIDENCE_DIR/v6-seed.out" 2> "$EVIDENCE_DIR/v6-seed.err" &
SEED_PID=$!
for _ in $(seq 1 200); do
  grep -q '^LINGXI_SERVICE_READY ' "$EVIDENCE_DIR/v6-seed.out" 2>/dev/null && break
  kill -0 "$SEED_PID" 2>/dev/null || break
  sleep 0.05
done
if grep -q '^LINGXI_SERVICE_READY ' "$EVIDENCE_DIR/v6-seed.out" 2>/dev/null; then
  TOKEN=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["token"])' "$V6/lingxi-service/local-token.json")
  ADDR=$(sed -n 's/^LINGXI_SERVICE_READY addr=\([^ ]*\).*/\1/p' "$EVIDENCE_DIR/v6-seed.out" | head -1)
  curl -sS -o /dev/null -w '%{http_code}' -X POST -H "Authorization: Bearer $TOKEN" \
    -H 'Content-Type: application/json' -d '{"input":"v6 wal seed"}' \
    "http://$ADDR/lingxi/v1/sessions/sess_local_alpha/execute" \
    > "$EVIDENCE_DIR/v6-seed-execute.txt" || true
fi
kill -9 "$SEED_PID" 2>/dev/null || true
wait "$SEED_PID" 2>/dev/null || true
SEED_PID=""
[ -s "$V6/lingxi-service/data/runs.db-wal" ] || fail "v6 setup: WAL not present after the crash seeding"
python3 - "$V6/lingxi-service/data/runs.db-wal" <<'PYEOF'
import sys
p = sys.argv[1]
data = open(p, "rb").read()
open(p, "wb").write(data[:-11])  # tear mid-frame at the tail
PYEOF
run_probe "$V6" v6
if [ "$PROBE_READY" = "1" ]; then
  # Recovery by design: the recovered view must be transaction-consistent —
  # a run is either fully terminal (row + its terminal event) or not
  # terminal at all; no half commit may exist.
  "$INSPECT_BIN" "$V6/lingxi-service/data/runs.db" dump > "$EVIDENCE_DIR/v6-recovered-dump.jsonl" 2>/dev/null
  python3 - "$EVIDENCE_DIR/v6-recovered-dump.jsonl" <<'PYEOF' || fail "v6: recovered view is NOT transaction-consistent"
import json, sys
rows = [json.loads(l) for l in open(sys.argv[1]) if l.strip().startswith("{")]
runs = {r["row"][0]: r["row"][6] for r in rows if r["table"] == "runs"}
events = [r for r in rows if r["table"] == "key_events"]
for run_id, status in runs.items():
    done = any(r["row"][0] == f"{run_id}-done" for r in events)
    if status == "completed" and not done:
        raise SystemExit(f"half terminal state for {run_id}: completed row without its event")
    if status != "completed" and done:
        raise SystemExit(f"half terminal state for {run_id}: event without terminal row")
print(f"runs={len(runs)} events={len(events)} transaction-consistent")
PYEOF
  note "v6 observed: service started after WAL recovery (by design); recovered view transaction-consistent; graceful stop exit=$PROBE_EXIT"
else
  # A refusal is equally acceptable (SQLite build differences): explicit failure.
  [ "$PROBE_EXIT" = "2" ] || fail "v6: unexpected outcome (exit=$PROBE_EXIT)"
  note "v6 observed: explicit refusal (exit 2) on the torn WAL"
fi

# ---- v7: tampered migration receipt -------------------------------------------
note "== v7 (A12): tampered migration receipt =="
V7=$(mktemp -d /tmp/lingxi-r02t06-drill-v7-XXXXXX)
mkdir -p "$V7/lingxi-service/data"
# Seed a healthy db by running the service once (graceful stop).
run_probe "$V7" v7-seed
[ "$PROBE_READY" = "1" ] || fail "v7 seed start failed"
sqlite3 "$V7/lingxi-service/data/runs.db" \
  "UPDATE schema_migrations SET fingerprint='deadbeef' WHERE version=1" \
  || fail "v7: cannot tamper the receipt (sqlite3 CLI missing?)"
shasum -a 256 "$V7/lingxi-service/data/runs.db" > "$EVIDENCE_DIR/v7-hashes-before.txt"
run_probe "$V7" v7
[ "$PROBE_EXIT" = "2" ] || fail "v7 must refuse with exit 2, got $PROBE_EXIT"
grep -Eq "SchemaTampered|fingerprint|tampered" "$EVIDENCE_DIR/v7.err" \
  || fail "v7 stderr must name the schema tampering"
shasum -a 256 "$V7/lingxi-service/data/runs.db" > "$EVIDENCE_DIR/v7-hashes-after.txt"
diff "$EVIDENCE_DIR/v7-hashes-before.txt" "$EVIDENCE_DIR/v7-hashes-after.txt" > /dev/null \
  || fail "v7: the tampered db must be preserved byte-identical"
note "PASS v7 exit=2 SchemaTampered refusal; file byte-identical"

# ---- v8 (INFO): garbage WAL header — recovery by design -----------------------
note "== v8 (INFO row): garbage WAL header (SQLite recovery semantics) =="
V8=$(mktemp -d /tmp/lingxi-r02t06-drill-v8-XXXXXX)
mkdir -p "$V8/lingxi-service/data"
"$SERVICE_BIN" --home "$V8" --bind 127.0.0.1:0 > "$EVIDENCE_DIR/v8-seed.out" 2> "$EVIDENCE_DIR/v8-seed.err" &
SEED_PID=$!
for _ in $(seq 1 200); do
  grep -q '^LINGXI_SERVICE_READY ' "$EVIDENCE_DIR/v8-seed.out" 2>/dev/null && break
  kill -0 "$SEED_PID" 2>/dev/null || break
  sleep 0.05
done
TOKEN=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["token"])' "$V8/lingxi-service/local-token.json" 2>/dev/null || echo "")
if [ -n "$TOKEN" ]; then
  ADDR=$(sed -n 's/^LINGXI_SERVICE_READY addr=\([^ ]*\).*/\1/p' "$EVIDENCE_DIR/v8-seed.out" | head -1)
  curl -sS -o /dev/null -X POST -H "Authorization: Bearer $TOKEN" \
    -H 'Content-Type: application/json' -d '{"input":"v8 wal seed"}' \
    "http://$ADDR/lingxi/v1/sessions/sess_local_alpha/execute" || true
fi
kill -9 "$SEED_PID" 2>/dev/null || true
wait "$SEED_PID" 2>/dev/null || true
SEED_PID=""
if [ -s "$V8/lingxi-service/data/runs.db-wal" ]; then
  python3 - "$V8/lingxi-service/data/runs.db-wal" <<'PYEOF'
import sys
p = sys.argv[1]
data = bytearray(open(p, "rb").read())
data[0:4] = b"\xde\xad\xbe\xef"   # garbage magic, size preserved
open(p, "wb").write(bytes(data))
PYEOF
  run_probe "$V8" v8
  note "v8 observed: exit=$PROBE_EXIT ready=$PROBE_READY (SQLite may recover or refuse; files: $(ls "$V8/lingxi-service/data" | tr '\n' ' '))"
  cat "$EVIDENCE_DIR/v8.err" >> "$EVIDENCE_DIR/v8-observed.err" || true
else
  note "v8 skipped: no WAL survived seeding"
fi

# ---- residue check -------------------------------------------------------------
LEFT=$( { pgrep -f "lingxi-service --home /tmp/lingxi-r02t06-drill" || true; } | wc -l | tr -d ' ')
[ "$LEFT" = "0" ] || fail "leftover service processes: $LEFT"
rm -rf /tmp/lingxi-r02t06-drill-v*-*
note "RESULT: R02-T06 recovery drill + A12 corruption matrix ALL GREEN (v8 informational)"

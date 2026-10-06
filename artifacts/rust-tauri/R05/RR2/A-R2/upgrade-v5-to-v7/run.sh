#!/usr/bin/env bash
# RR2 WP-A (F41) self-check: a hand-built v5 database carrying NON-EMPTY
# model_call_usage rows is upgraded to v7 by the CURRENT service binary;
# every pre-existing value must survive verbatim, the v6 columns get their
# honest defaults, and the v7 unknown-value semantics (NULL transport
# attempts, never 0) must hold. Pre-upgrade NOT NULL is proven too.
#
# The v5 base is built by applying the VERBATIM V1..V5 SQL parsed out of
# rust/crates/lingxi-adapters/src/storage/migrations.rs (same parser as
# scripts/rust-tauri/r02_registry_consistency.py) and writing receipts with
# the sha256(SQL) fingerprints — i.e. a faithful pre-v6 database, not a
# mock of the schema.
set -uo pipefail
cd "$(dirname "$0")/../../../../../.."   # repo root

EV_DIR="artifacts/rust-tauri/R05/RR2/A-R2/upgrade-v5-to-v7"
mkdir -p "$EV_DIR"
TARGET_DIR="${TMPDIR:-/tmp}/rust-target-r02-t04"
SERVICE_BIN="$TARGET_DIR/debug/lingxi-service"
INSPECT_BIN="$TARGET_DIR/debug/lingxi-storage-inspect"
LOG="$EV_DIR/run.log"
: > "$LOG"

log() { printf '%s\n' "$*" | tee -a "$LOG"; }

HOME5=$(mktemp -d "${TMPDIR:-/tmp}/lingxi-rr2a-v5-XXXXXX")
mkdir -p "$HOME5/lingxi-service/data"
DB5="$HOME5/lingxi-service/data/runs.db"
log "V5_HOME=$HOME5"

# 1) build the v5 database from the verbatim V1..V5 SQL + true fingerprints
python3 - "$DB5" >> "$LOG" 2>&1 << 'PYEOF'
import sqlite3, sys, hashlib
from pathlib import Path
sys.dont_write_bytecode = True  # keep scripts/rust-tauri free of __pycache__
sys.path.insert(0, "scripts/rust-tauri")
import r02_registry_consistency as rrc

db = sys.argv[1]
migs = rrc.parse_migrations_rs(Path("rust/crates/lingxi-adapters/src/storage/migrations.rs"))
conn = sqlite3.connect(db)
conn.execute("PRAGMA foreign_keys=ON")
# The receipts table is created outside the receipted transactions (same DDL
# as ensure_receipts_table in migrations.rs) — the migration SQL never
# contains it.
conn.execute(
    "CREATE TABLE schema_migrations (\n"
    "    version            INTEGER PRIMARY KEY,\n"
    "    name               TEXT NOT NULL,\n"
    "    fingerprint        TEXT NOT NULL,\n"
    "    applied_at_unix_ms INTEGER NOT NULL,\n"
    "    applied_by         TEXT NOT NULL\n"
    ")"
)
for m in migs:
    if m["version"] > 5:
        break
    conn.executescript(m["sql"])
    fp = hashlib.sha256(m["sql"].encode()).hexdigest()
    assert fp == m["fingerprint"]
    conn.execute(
        "INSERT INTO schema_migrations (version, name, fingerprint, "
        "applied_at_unix_ms, applied_by) VALUES (?,?,?,?,?)",
        (m["version"], m["name"], fp, 1_700_000_000_000, "v5-builder"),
    )
conn.execute("PRAGMA user_version=5")
conn.execute(
    "INSERT INTO model_call_usage (model_call_id, session_id, run_id, attempt,"
    " purpose, origin, parent_run_id, cause_ref, provider, model, protocol,"
    " usage_state, input_tokens, output_tokens, cache_read_tokens,"
    " cache_write_tokens, reasoning_tokens, missing_fields, estimate_basis,"
    " invalid_detail, transport_attempts, cost_basis, recorded_at_unix_ms)"
    " VALUES ('call-alpha','sess-1','run-1','a1','main','user',NULL,NULL,"
    "'acme','acme-1','anthropic','ok',100,50,NULL,NULL,NULL,NULL,NULL,NULL,"
    "1,NULL,1111)"
)
conn.execute(
    "INSERT INTO model_call_usage (model_call_id, session_id, run_id, attempt,"
    " purpose, origin, parent_run_id, cause_ref, provider, model, protocol,"
    " usage_state, input_tokens, output_tokens, cache_read_tokens,"
    " cache_write_tokens, reasoning_tokens, missing_fields, estimate_basis,"
    " invalid_detail, transport_attempts, cost_basis, recorded_at_unix_ms)"
    " VALUES ('call-beta','sess-1','run-1','a1','worker_callback','subagent',"
    "'run-0',NULL,'acme','acme-2','openai','partial',NULL,NULL,NULL,NULL,NULL,"
    "'output_tokens',NULL,NULL,3,NULL,2222)"
)
conn.commit()
print("v5 base built: user_version=%d receipts=%d usage_rows=%d" % (
    conn.execute("PRAGMA user_version").fetchone()[0],
    conn.execute("SELECT COUNT(*) FROM schema_migrations").fetchone()[0],
    conn.execute("SELECT COUNT(*) FROM model_call_usage").fetchone()[0]))
PYEOF
[ $? -eq 0 ] || { log "FAIL: v5 base build"; exit 1; }
log "PASS v5 base built (V1..V5 SQL verbatim from migrations.rs, receipts + user_version=5, 2 usage rows)"

sqlite3 "$DB5" ".mode json" "SELECT * FROM model_call_usage ORDER BY model_call_id" > "$EV_DIR/usage-before-upgrade.json"
sqlite3 "$DB5" "SELECT sql FROM sqlite_master WHERE name='model_call_usage'" > "$EV_DIR/schema-v5.sql"

# 2) pre-upgrade: NULL transport_attempts must be REFUSED (v5 NOT NULL)
NULL_INSERT_SQL="INSERT INTO model_call_usage (model_call_id, purpose, origin, provider, model, protocol, usage_state, transport_attempts, recorded_at_unix_ms) VALUES ('should-not-exist','main','user','acme','acme-1','anthropic','ok',NULL,1)"
if sqlite3 "$DB5" "$NULL_INSERT_SQL" >> "$LOG" 2>&1; then
  log "FAIL: v5 accepted NULL transport_attempts (NOT NULL missing)"
  exit 1
else
  log "PASS v5 refuses NULL transport_attempts (NOT NULL constraint enforced pre-v7)"
fi

# 3) upgrade with the CURRENT binary (service open-time migration pass)
"$SERVICE_BIN" --home "$HOME5" --bind 127.0.0.1:0 > "$EV_DIR/upgrade-service.out" 2> "$EV_DIR/upgrade-service.err" &
SPID=$!
READY=0
for _ in $(seq 1 200); do
  grep -q '^LINGXI_SERVICE_READY ' "$EV_DIR/upgrade-service.out" 2>/dev/null && { READY=1; break; }
  kill -0 "$SPID" 2>/dev/null || break
  sleep 0.05
done
if [ "$READY" != "1" ]; then
  wait "$SPID" 2>/dev/null
  log "FAIL: service did not become ready on the v5 db (exit=$?)"
  cat "$EV_DIR/upgrade-service.err" >> "$LOG"
  exit 1
fi
kill -TERM "$SPID" 2>/dev/null
wait "$SPID" 2>/dev/null
log "PASS service opened the v5 db and became ready (v6+v7 applied at open)"

# 4) post-upgrade state
"$INSPECT_BIN" "$DB5" migrations > "$EV_DIR/migrations-after-upgrade.json"
python3 - "$EV_DIR/migrations-after-upgrade.json" >> "$LOG" 2>&1 << 'PYEOF'
import json, sys
doc = json.load(open(sys.argv[1]))
assert doc["userVersion"] == doc["supportedVersion"] == 7, doc["userVersion"]
keys = ("version", "name", "fingerprint")
assert [[r[k] for k in keys] for r in doc["receipts"]] == \
       [[c[k] for k in keys] for c in doc["compiledIn"]]
print("post-upgrade: userVersion=7, 7 receipts == compiledIn")
PYEOF
[ $? -eq 0 ] || { log "FAIL: post-upgrade migrations state"; exit 1; }
log "PASS post-upgrade userVersion=7, receipts==compiledIn (7)"

sqlite3 "$DB5" ".mode json" "SELECT * FROM model_call_usage ORDER BY model_call_id" > "$EV_DIR/usage-after-upgrade.json"
python3 - "$EV_DIR" >> "$LOG" 2>&1 << 'PYEOF'
import json, sys
ev = sys.argv[1]
before = {r["model_call_id"]: r for r in json.load(open(f"{ev}/usage-before-upgrade.json"))}
after = {r["model_call_id"]: r for r in json.load(open(f"{ev}/usage-after-upgrade.json"))}
assert set(before) == set(after) == {"call-alpha", "call-beta"}, (set(before), set(after))
for cid, b in before.items():
    a = after[cid]
    for col, v in b.items():
        assert a[col] == v, (cid, col, v, a[col])
    # v6 columns: honest defaults, nothing invented
    assert a["outcome"] == "unknown", (cid, a["outcome"])
    for col in ("started_at_unix_ms", "settled_at_unix_ms",
                "parent_tool_call_id", "emitted_tool_calls"):
        assert a[col] is None, (cid, col, a[col])
# the two pre-existing rows keep their observed counts verbatim
assert after["call-alpha"]["transport_attempts"] == 1
assert after["call-beta"]["transport_attempts"] == 3
print("data intact: both rows preserved verbatim; v6 defaults outcome='unknown', ts/lineage NULL")
PYEOF
[ $? -eq 0 ] || { log "FAIL: usage data integrity after upgrade"; exit 1; }
log "PASS usage data intact across v5->v6->v7 (values verbatim, v6 defaults honest)"

# 5) unknown-value semantics on v7: NULL is accepted and stays NULL (never 0)
DROPPED_INSERT_SQL="INSERT INTO model_call_usage (model_call_id, purpose, origin, provider, model, protocol, usage_state, transport_attempts, recorded_at_unix_ms) VALUES ('call-dropped','main','user','acme','acme-1','anthropic','ok',NULL,3333)"
if sqlite3 "$DB5" "$DROPPED_INSERT_SQL"; then
  log "PASS v7 accepts NULL transport_attempts (dropped-call unknown)"
else
  log "FAIL: v7 still refuses NULL transport_attempts"
  exit 1
fi
sqlite3 "$DB5" ".mode json" "SELECT model_call_id, transport_attempts FROM model_call_usage WHERE model_call_id IN ('call-alpha','call-dropped')" > "$EV_DIR/unknown-semantics.json"
NULLVAL=$(python3 -c "
import json
rows = {r['model_call_id']: r['transport_attempts'] for r in json.load(open('$EV_DIR/unknown-semantics.json'))}
assert rows['call-alpha'] == 1, rows
assert rows['call-dropped'] is None, rows
print('observed=1 kept; unknown=NULL kept (not 0, not 1)')")
[ $? -eq 0 ] || { log "FAIL: unknown semantics readback"; exit 1; }
log "PASS $NULLVAL"

rm -rf "$HOME5"
log "RESULT: v5(non-empty usage) -> v7 upgrade ALL GREEN"

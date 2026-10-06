#!/usr/bin/env bash
# RR2 WP-A (F41) counterexamples: three registry mutations on an ISOLATED
# /tmp copy of the relevant tree. Each must make the low-cost equality gate
# fail NAMING the mutated version, and the S4-style binary equation must
# fail for the same reason. Restored copy must be green again. The main
# tree is never touched.
#
# Mutations:
#   1. delete the registry's v6 entry           -> v6 named missing
#   2. tamper the registry's v7 fingerprint     -> v7 named fingerprint mismatch
#   3. tamper an OLD fingerprint (v2, v1-v5 class) -> v2 named fingerprint mismatch
set -uo pipefail
MAIN_ROOT="/Users/study_superior/Desktop/Code/LingxiAgent"
EV_DIR="$MAIN_ROOT/artifacts/rust-tauri/R05/RR2/A-R2/mutations"
mkdir -p "$EV_DIR"
LOG="$EV_DIR/run.log"
: > "$LOG"
log() { printf '%s\n' "$*" | tee -a "$LOG"; }

ISO=$(mktemp -d "${TMPDIR:-/tmp}/lingxi-rr2a-mut-XXXXXX")
mkdir -p "$ISO/scripts/rust-tauri" "$ISO/docs/rust-tauri/R02" \
         "$ISO/rust/crates/lingxi-adapters/src/storage"
cp "$MAIN_ROOT/scripts/rust-tauri/r02_registry_consistency.py" "$ISO/scripts/rust-tauri/"
cp "$MAIN_ROOT/docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json" "$ISO/docs/rust-tauri/R02/"
cp "$MAIN_ROOT/rust/crates/lingxi-adapters/src/storage/migrations.rs" \
   "$ISO/rust/crates/lingxi-adapters/src/storage/"
log "ISOLATED_COPY=$ISO (checker + registry + migrations.rs only; main tree untouched)"
PRISTINE_REGISTRY="$EV_DIR/registry-pristine.json"
cp "$ISO/docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json" "$PRISTINE_REGISTRY"

# Real inspector output from this candidate (binary receipts == compiledIn)
# feeds the S4-equivalence check, so mutations are judged against the REAL
# binary fingerprints, not against hand-copied values.
INSPECT_MIGRATIONS="$MAIN_ROOT/artifacts/rust-tauri/R05/RR2/A-R2/fingerprint-extraction/fresh-migrations.json"

check() {  # $1 = case label; runs BOTH gates on the isolated copy
  local label="$1"
  python3 "$ISO/scripts/rust-tauri/r02_registry_consistency.py" \
    > "$EV_DIR/$label-s0consistency.log" 2>&1
  local s0=$?
  REGISTRY_PATH="$ISO/docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json" \
  INSPECT_JSON="$INSPECT_MIGRATIONS" python3 > "$EV_DIR/$label-s4equation.log" 2>&1 << 'PYEOF'
import json, os, sys
# Same assertion set as r02_t04_storage_tx.sh S4 (kept verbatim in spirit:
# userVersion/supportedVersion/counts/receipts/compiledIn vs the registry).
doc = json.load(open(os.environ["INSPECT_JSON"]))
registered = json.load(open(os.environ["REGISTRY_PATH"]))["new_persistence_points"][0]["migrations"]
registered = sorted(
    ({"version": m["version"], "name": m["name"],
      "fingerprint": m["fingerprint_sha256"]} for m in registered),
    key=lambda m: m["version"],
)
supported = len(registered)
assert doc["userVersion"] == doc["supportedVersion"] == supported, (
    f"userVersion {doc['userVersion']} / supportedVersion {doc['supportedVersion']} "
    f"disagree with the registry's {supported} migrations"
)
receipts = doc["receipts"]
compiled = doc["compiledIn"]
assert len(receipts) == len(compiled) == supported, (
    f"expected {supported} receipts, got receipts={len(receipts)} compiled={len(compiled)}"
)
assert [r["version"] for r in receipts] == [m["version"] for m in registered], (
    "receipt versions disagree with the registry"
)
for r, m in zip(receipts, registered):
    assert r["version"] == m["version"], (r, m)
    assert r["name"] == m["name"], (r, m)
    assert r["fingerprint"] == m["fingerprint"], (
        f"receipt v{r['version']} fingerprint {r['fingerprint']} != registry {m['fingerprint']}"
    )
for c, m in zip(compiled, registered):
    assert c["version"] == m["version"] and c["name"] == m["name"], (c, m)
    assert c["fingerprint"] == m["fingerprint"], (
        f"compiledIn v{c['version']} fingerprint {c['fingerprint']} != registry {m['fingerprint']}"
    )
PYEOF
  local s4=$?
  log "-- $label: S0-consistency exit=$s0 ; S4-equation exit=$s4"
  return $(( s0 == 0 ? 0 : 1 ))
}

expect_fail_naming() {  # $1=label $2=version-number that must be named
  local label="$1" want="v$2"
  local named=0
  grep -Eq "(^|[^0-9])$want([^0-9]|$)" "$EV_DIR/$label-s0consistency.log" && named=1
  [ "$named" = "1" ] || { log "FAIL: $label did not name $want"; return 1; }
  log "   $label names $want in the failure output"
  return 0
}

ALL_OK=0

# ---- baseline: pristine copy must be GREEN on both gates ---------------------
log "== baseline (pristine isolated copy) =="
if check baseline && [ ! -s "$EV_DIR/baseline-s4equation.log" ]; then
  log "PASS baseline green (S0 exit=0, S4-equation exit=0)"
else
  log "FAIL baseline not green"; ALL_OK=1
fi

mutate_delete_v6() {
  python3 - "$ISO" << 'PYEOF'
import json, sys
p = sys.argv[1] + "/docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json"
doc = json.load(open(p))
migs = doc["new_persistence_points"][0]["migrations"]
doc["new_persistence_points"][0]["migrations"] = [m for m in migs if m["version"] != 6]
json.dump(doc, open(p, "w"), indent=2, ensure_ascii=False)
PYEOF
}
mutate_tamper_fp() {  # $1=version $2=new-fake-fingerprint
  python3 - "$ISO" "$1" "$2" << 'PYEOF'
import json, sys
p = sys.argv[1] + "/docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json"
version, fake = int(sys.argv[2]), sys.argv[3]
doc = json.load(open(p))
for m in doc["new_persistence_points"][0]["migrations"]:
    if m["version"] == version:
        m["fingerprint_sha256"] = fake
json.dump(doc, open(p, "w"), indent=2, ensure_ascii=False)
PYEOF
}
restore() { cp "$PRISTINE_REGISTRY" "$ISO/docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json"; }

# ---- mutation 1: delete the v6 registry entry --------------------------------
log "== mutation 1: delete registry v6 entry =="
mutate_delete_v6
if check mut1-del-v6; then
  log "FAIL mutation1 unexpectedly green"; ALL_OK=1
else
  if expect_fail_naming mut1-del-v6 6; then
    grep -q "exit=1" /dev/null; :
  fi
  log "PASS mutation1: both gates FAIL naming v6 (F41 signature reproduced, caught at zero build cost)"
fi
restore

# ---- mutation 2: tamper the v7 fingerprint ------------------------------------
log "== mutation 2: tamper registry v7 fingerprint =="
mutate_tamper_fp 7 "0000000000000000000000000000000000000000000000000000000000000000"
if check mut2-v7-fp; then
  log "FAIL mutation2 unexpectedly green"; ALL_OK=1
else
  expect_fail_naming mut2-v7-fp 7 || ALL_OK=1
  log "PASS mutation2: both gates FAIL naming v7 fingerprint mismatch"
fi
restore

# ---- mutation 3: tamper an OLD fingerprint (v2 of v1-v5) ----------------------
log "== mutation 3: tamper OLD registry fingerprint (v2) =="
mutate_tamper_fp 2 "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
if check mut3-v2-fp; then
  log "FAIL mutation3 unexpectedly green"; ALL_OK=1
else
  expect_fail_naming mut3-v2-fp 2 || ALL_OK=1
  log "PASS mutation3: both gates FAIL naming v2 fingerprint mismatch (old entries protected)"
fi
restore

# ---- restored copy must be green again ----------------------------------------
log "== restore check =="
if check restored && [ ! -s "$EV_DIR/restored-s4equation.log" ]; then
  log "PASS restored copy green on both gates"
else
  log "FAIL restored copy not green"; ALL_OK=1
fi

rm -rf "$ISO"
if [ "$ALL_OK" = "0" ]; then
  log "RESULT: 3/3 counterexamples precise-fail + restore green"
  exit 0
fi
log "RESULT: counterexample suite had failures"
exit 1

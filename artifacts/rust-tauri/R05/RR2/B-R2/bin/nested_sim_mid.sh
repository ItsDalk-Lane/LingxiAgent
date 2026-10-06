#!/usr/bin/env bash
# T1 middle sim runner: INVOKED with stdout/stderr already redirected to
# <run-root>/R04_REGRESSION/r03_regression_gate/*.log — the direct-parent
# ancestor sink of the gate (the verify-stage R03 level). Backgrounds a
# heartbeat writing to its own inherited stdout (the r03 sink grows DURING
# the gate window — parent+child logs growing simultaneously), runs the
# gate with stdout/stderr captured to the leaf command dir, propagates rc.
set -u
RR="$1"
LEAF="$RR/R04_REGRESSION/R03_REGRESSION/r02_legacy_regression"
mkdir -p "$LEAF"
( while :; do echo "mid-sim alive $(date '+%H:%M:%S')"; sleep 5; done ) &
HEART=$?
R02_LEGACY_REGRESSION_MODE=directed-no-seal-family \
  bash scripts/rust-tauri/r02_t08_legacy_entry_regression.sh \
    "$RR/R04_REGRESSION/R03_REGRESSION/R02/A16" \
    > "$LEAF/stdout.log" 2> "$LEAF/stderr.log"
RC=$?
kill "$HEART" 2>/dev/null
wait "$HEART" 2>/dev/null
exit "$RC"

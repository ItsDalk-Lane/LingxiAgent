#!/usr/bin/env bash
# T1 outermost sim runner: INVOKED with stdout/stderr already redirected to
# <run-root>/r04_regression_gate/*.log (see the launcher in
# commands-and-exits.txt) — that fd-1 file is the gate's ANCESTOR sink, the
# F42 shape. Backgrounds a heartbeat that keeps writing to its own inherited
# stdout (the r04 sink grows DURING the gate window), runs the middle runner
# with its stdout/stderr captured to r03_regression_gate/*.log, propagates rc.
set -u
RR="$1"; MID="$2"
mkdir -p "$RR/r04_regression_gate" "$RR/R04_REGRESSION/r03_regression_gate"
( while :; do echo "outer-sim alive $(date '+%H:%M:%S')"; sleep 5; done ) &
HEART=$!
bash "$MID" "$RR" \
  > "$RR/R04_REGRESSION/r03_regression_gate/stdout.log" \
  2> "$RR/R04_REGRESSION/r03_regression_gate/stderr.log"
RC=$?
kill "$HEART" 2>/dev/null
wait "$HEART" 2>/dev/null
exit "$RC"

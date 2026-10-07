#!/bin/bash
# Read-only r00 sampler: polls for r00_management_leaves processes, records
# ps identity, listening ports (lsof), and exe path. Output: NDJSON.
OUT="$1"
REPO=/Users/study_superior/Desktop/Code/LingxiAgent
while [ ! -f "$OUT.stop" ]; do
  for PID in $(pgrep -f 'r00_management_leaves' 2>/dev/null); do
    TS=$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)
    CMDLINE=$(ps -o command= -p "$PID" 2>/dev/null | head -c 400)
    PPID_=$(ps -o ppid= -p "$PID" 2>/dev/null | tr -d ' ')
    EXE=$(ps -o comm= -p "$PID" 2>/dev/null)
    LISTEN=$(lsof -nP -iTCP -sTCP:LISTEN -a -p "$PID" -F Pn 2>/dev/null | tr '\n' ' ' | head -c 300)
    printf '{"ts":"%s","pid":%s,"ppid":"%s","comm":"%s","listen":"%s","cmd":"%s"}\n' \
      "$TS" "$PID" "$PPID_" "$EXE" "$LISTEN" "$CMDLINE" >> "$OUT" 2>/dev/null
  done
  sleep 1
done

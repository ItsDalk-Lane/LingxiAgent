#!/bin/bash
# FINAL-04 r00 listener monitor: read-only sampling (pgrep/lsof/ps) of any live
# r00_management_leaves test process; records listener state per sample to
# /private/tmp/rr3-final04-dir/r00-monitor.jsonl. No system modification.
REPO=/Users/study_superior/Desktop/Code/LingxiAgent
OUT=/private/tmp/rr3-final04-dir/r00-monitor.jsonl
STOP=/private/tmp/rr3-final04-dir/r00-monitor.stop
while [ ! -f "$STOP" ]; do
  ts=$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)
  pids=$(pgrep -f 'r00_management_leaves' 2>/dev/null || true)
  if [ -n "$pids" ]; then
    for pid in $pids; do
      comm=$(ps -p "$pid" -o comm= 2>/dev/null | tr -d '[:space:]')
      case "$comm" in
        *r00_management_leaves*)
          bin=$(ps -p "$pid" -o command= 2>/dev/null | sed 's/ .*//')
          listeners=$(lsof -nP -p "$pid" 2>/dev/null | awk '$NF=="(LISTEN)" || $8=="TCP" {print}' | grep -i listen || true)
          printf '{"ts":"%s","pid":%s,"comm":"%s","bin":"%s","listeners":%s}\n' \
            "$ts" "$pid" "$comm" "$bin" "$(printf '%s' "$listeners" | python3 -c 'import sys,json; print(json.dumps(sys.stdin.read()))')"
          ;;
      esac
    done
  fi
  sleep 1
done
echo "monitor stopped at $(date -u +%Y-%m-%dT%H:%M:%S.%NZ)" >> "$OUT"

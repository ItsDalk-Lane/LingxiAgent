#!/bin/bash
# Read-only sampler: observe r00_management_leaves processes & listening sockets during cmd-03.
OUT=/private/tmp/rr3-final01-dir/r00-monitor.jsonl
STOP=/private/tmp/rr3-final01-dir/r00-monitor.stop
: > "$OUT"
END=$(( $(date +%s) + 14400 ))
while [ "$(date +%s)" -lt "$END" ]; do
  [ -f "$STOP" ] && { echo "{\"event\":\"stopped\",\"utc\":\"$(date -u +%FT%T.%NZ)\"}" >> "$OUT"; break; }
  PIDS=$(pgrep -f 'r00_management_leaves' 2>/dev/null)
  if [ -n "$PIDS" ]; then
    for P in $PIDS; do
      {
        echo "{\"event\":\"saw\",\"utc\":\"$(date -u +%FT%T.%NZ)\",\"pid\":$P,"
        ps -p "$P" -o pid=,ppid=,etime=,comm= | awk '{printf "\"ps\":\"%s\",\n", $0}'
        echo "\"listen\":$(lsof -a -p "$P" -iTCP -sTCP:LISTEN -F Pi 2>/dev/null | tr '\n' ' ' | sed 's/"/\\"/g;s/^/"/;s/$/"/'),"
        echo "\"exe\":$(lsof -p "$P" 2>/dev/null | awk '$4=="txt" && $NF ~ /r00_management_leaves/ {print $NF}' | head -1 | sed 's/"/\\"/g;s/^/"/;s/$/"/')}"
      } >> "$OUT" 2>/dev/null
    done
    sleep 1
  else
    sleep 2
  fi
done
echo "{\"event\":\"monitor-exit\",\"utc\":\"$(date -u +%FT%T.%NZ)\"}" >> "$OUT"

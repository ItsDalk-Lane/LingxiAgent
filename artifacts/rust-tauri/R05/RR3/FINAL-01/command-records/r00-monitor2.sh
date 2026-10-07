#!/bin/bash
OUT=/private/tmp/rr3-final01-dir/r00-monitor-2.jsonl
STOP=/private/tmp/rr3-final01-dir/r00-monitor2.stop
: > "$OUT"
END=$(( $(date +%s) + 21600 ))
while [ "$(date +%s)" -lt "$END" ]; do
  [ -f "$STOP" ] && { echo "{\"event\":\"stopped\"}" >> "$OUT"; break; }
  PIDS=$(pgrep -f 'r00_management_leaves' 2>/dev/null)
  if [ -n "$PIDS" ]; then
    for P in $PIDS; do
      echo "{\"event\":\"saw\",\"utc\":\"$(date -u +%FT%T.%NZ)\",\"pid\":$P,\"ps\":\"$(ps -p $P -o pid=,ppid=,etime=,comm= | tr -d '\n')\",\"listen\":\"$(lsof -nP -a -p $P -iTCP -sTCP:LISTEN 2>/dev/null | tail -n +2 | tr '\n' ';')\",\"exe\":\"$(lsof -p $P 2>/dev/null | awk '$4==\"txt\" && $NF ~ /r00_management_leaves/ {print $NF}' | head -1)\"}" >> "$OUT"
    done
    sleep 1
  else
    sleep 2
  fi
done
echo "{\"event\":\"monitor-exit\"}" >> "$OUT"

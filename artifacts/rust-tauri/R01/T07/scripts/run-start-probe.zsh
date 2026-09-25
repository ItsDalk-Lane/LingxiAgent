#!/bin/zsh
# R01-T07 start-probe: expects the server to come UP (control scenarios).
# Polls http://127.0.0.1:<port>/api/health (loopback only), then SIGTERMs.
# Usage: run-start-probe.zsh <label> <home-dir> <port> [extra-env KEY=VAL ...]
set -u
LABEL="$1"; HOME_DIR="$2"; PORT="$3"; shift 3
REPO="/Users/study_superior/Desktop/Code/LingxiAgent"
OUT_DIR="$REPO/artifacts/rust-tauri/R01/T07/a13"
LOG="$OUT_DIR/${LABEL}.log"

cd "$REPO"
: > "$LOG"  # truncate once, then every writer uses O_APPEND (no offset clobbering)
env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  LINGXI_HOME="$HOME_DIR" LINGXI_PORT="$PORT" LINGXI_TOKEN="r01t07-probe-token" "$@" \
  node server/main-full.ts >> "$LOG" 2>&1 &
PID=$!

STARTED=0
for i in $(seq 1 60); do
  if curl -s --noproxy '*' --max-time 2 "http://127.0.0.1:${PORT}/api/health" > "$OUT_DIR/${LABEL}.health.json" 2>/dev/null; then
    STARTED=1; break
  fi
  kill -0 "$PID" 2>/dev/null || break
  sleep 0.5
done
echo "STARTED=$STARTED" | tee -a "$LOG"
if kill -0 "$PID" 2>/dev/null; then
  kill -TERM "$PID" 2>/dev/null
fi
wait "$PID"
CODE=$?
echo "EXIT_CODE=$CODE" | tee -a "$LOG"
echo "EFFECTIVE_HOME_CHECK:" | tee -a "$LOG"
cat "$HOME_DIR/server-info.json" 2>/dev/null | tee -a "$LOG" || echo "(no server-info.json)" | tee -a "$LOG"
exit 0

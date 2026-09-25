#!/bin/zsh
# R01-T07 repair-r1 variant probe (ZCode:R01-T07-repair-r1).
# Runs the current-HEAD production server entry (node server/main-full.ts,
# launch.js deliberately bypassed because scripts/dev-env.js:11 force-overrides
# LINGXI_HOME) against one synthetic variant home, then records:
#   - whether the HTTP server came up (loopback /api/health poll)
#   - the real exit code (1 = gate refusal; otherwise SIGTERM after start)
#   - server-info.json read back from the SYNTHETIC home (proves the effective
#     LINGXI_HOME value, i.e. writes landed in the sandbox and nowhere else)
# Proxies are stripped (env -u); the server only ever binds loopback here.
# Log discipline: truncate once, then every writer appends (O_APPEND) — the
# fd-offset clobbering defect from the original T07 probe is not repeated.
# Usage: probe-variant.zsh <label> <home-dir> <port>
set -u
LABEL="$1"; HOME_DIR="$2"; PORT="$3"
REPO="/Users/study_superior/Desktop/Code/LingxiAgent"
OUT_DIR="$REPO/artifacts/rust-tauri/R01/T07/repair-r1/logs"
LOG="$OUT_DIR/${LABEL}.log"

cd "$REPO"
: > "$LOG"
env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  LINGXI_HOME="$HOME_DIR" LINGXI_PORT="$PORT" LINGXI_TOKEN="r01t07-repair-r1-token" \
  node server/main-full.ts >> "$LOG" 2>&1 &
PID=$!

STARTED=0
for i in $(seq 1 70); do
  if curl -s --noproxy '*' --max-time 2 "http://127.0.0.1:${PORT}/api/health" > "$OUT_DIR/${LABEL}.health.json" 2>/dev/null; then
    STARTED=1; break
  fi
  kill -0 "$PID" 2>/dev/null || break
  sleep 0.5
done
echo "STARTED=$STARTED" >> "$LOG"
if kill -0 "$PID" 2>/dev/null; then
  kill -TERM "$PID" 2>/dev/null
fi
wait "$PID"
CODE=$?
echo "EXIT_CODE=$CODE" >> "$LOG"
echo "EFFECTIVE_HOME_CHECK (server-info.json read back from synthetic home):" >> "$LOG"
if [ -f "$HOME_DIR/server-info.json" ]; then
  cat "$HOME_DIR/server-info.json" >> "$LOG"
else
  echo "(no server-info.json in synthetic home)" >> "$LOG"
fi
echo "$LABEL STARTED=$STARTED EXIT_CODE=$CODE"
exit 0

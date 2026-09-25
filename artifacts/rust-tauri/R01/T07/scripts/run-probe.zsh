#!/bin/zsh
# R01-T07 / R01-A13 probe runner.
# Usage: run-probe.zsh <label> <home-dir> <port> [extra-env KEY=VAL ...]
# Runs the current-HEAD production server entry (server/main-full.ts, the exact
# entry launched by `npm run server` via scripts/launch.js, minus launch.js's
# LINGXI_HOME override) against an isolated synthetic LINGXI_HOME.
# A watchdog kills the process after 40s if it keeps running (listening case).
# Proxies are stripped; the server only ever binds loopback in these probes.
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

# watchdog
( sleep 40; kill -TERM "$PID" 2>/dev/null ) &
WATCH=$!

wait "$PID"
CODE=$?
kill "$WATCH" 2>/dev/null
wait "$WATCH" 2>/dev/null
echo "EXIT_CODE=$CODE" | tee -a "$LOG"
exit 0

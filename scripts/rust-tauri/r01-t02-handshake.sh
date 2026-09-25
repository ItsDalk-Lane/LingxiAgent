#!/usr/bin/env bash
# R01-A04 — version-negotiation handshake prototype over real HTTP/WS.
# Starts the deterministic local stand-in (lingxi-proto-server, loopback
# only) and drives 5 scenarios with the Node client; asserts exit codes and
# writes the transcript + server log to --out DIR.
set -euo pipefail
cd "$(dirname "$0")/../.."

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/lingxi-r01t02-target}"
OUT_DIR="${1:-/tmp/lingxi-r01t02-handshake}"
mkdir -p "$OUT_DIR"
SERVER_BIN="$CARGO_TARGET_DIR/debug/lingxi-proto-server"
CARGO_OFFLINE="env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY CARGO_NET_OFFLINE=true"

echo "== [1/3] build lingxi-proto-server"
$CARGO_OFFLINE cargo build -q -p lingxi-protocol --manifest-path rust/Cargo.toml --bins

echo "== [2/3] start server (5 connections) + run handshake client"
"$SERVER_BIN" --port 0 --connections 5 > "$OUT_DIR/server.log" 2>&1 &
SRV=$!
for _ in $(seq 1 50); do
  grep -q '^LISTENING' "$OUT_DIR/server.log" 2>/dev/null && break
  sleep 0.1
done
PORT=$(awk '/^LISTENING/{print $2}' "$OUT_DIR/server.log")
echo "server on 127.0.0.1:$PORT (pid $SRV)"

set +e
node tests/migration/r01-t02/handshake-client.mjs \
  --port "$PORT" --transcript "$OUT_DIR/transcript.jsonl"
CLIENT_EXIT=$?
wait $SRV
SERVER_EXIT=$?
set -e

echo "client exit=$CLIENT_EXIT server exit=$SERVER_EXIT"
cat "$OUT_DIR/server.log"

echo "== [3/3] result"
if [ "$CLIENT_EXIT" -ne 0 ] || [ "$SERVER_EXIT" -ne 0 ]; then
  echo "FAIL: handshake gate (client=$CLIENT_EXIT server=$SERVER_EXIT)" >&2
  exit 1
fi
echo "OK: R01-A04 handshake gate passed; transcript: $OUT_DIR/transcript.jsonl"

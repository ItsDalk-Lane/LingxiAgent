#!/usr/bin/env bash
# R02-T01 / acceptance R02-A01 — desktop-free service start + health check.
#
# Proves, with a REAL binary process (not an in-process test):
#   build -> start -> GET /lingxi/v1/health -> expected body -> clean
#   SIGTERM shutdown (exit 0, port freed, no leftover child processes),
# and proves "dependency tree has no desktop packages" from cargo metadata
# (the machine-checked resolve graph, not a verbal claim).
#
# Environment guards (R01 lessons):
#   - cargo is invoked EXPLICITLY through the rustup toolchain pinned by
#     rust-toolchain.toml (never a bare PATH cargo; Homebrew cargo ignores
#     the pin). Mirrors scripts/rust-tauri/r01-t02-check-generated.sh.
#   - CARGO_TARGET_DIR is task-dedicated (/tmp/rust-target-r02-t01 by
#     default) per RR-T08-F1: shared target dirs once bound a stale binary
#     to a leftover tree.
#   - the dead local proxy (127.0.0.1:7890) is stripped from the env of
#     every network-capable command; builds run offline against the lock.
#   - the service data root is a fresh synthetic dir under /tmp — the real
#     user directory is never touched (taskbook R02 §1 boundary).
#
# Usage: scripts/rust-tauri/r02_t01_service_smoke.sh [EVIDENCE_DIR]
# Exit 0 only if every step holds; evidence lands in EVIDENCE_DIR
# (default artifacts/rust-tauri/R02/T01).
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R02/T01}"
mkdir -p "$EVIDENCE_DIR"
TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rust-target-r02-t01}"

TOOLCHAIN="$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml | head -n 1)"
if [ -z "$TOOLCHAIN" ]; then
  echo "ERROR: cannot parse toolchain channel from rust-toolchain.toml" >&2
  exit 1
fi
if ! command -v rustup >/dev/null 2>&1; then
  if [ -x "$HOME/.cargo/bin/rustup" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
  else
    echo "ERROR: rustup not found; this gate requires the locked toolchain ($TOOLCHAIN)" >&2
    exit 1
  fi
fi
CARGO="env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TARGET_DIR rustup run $TOOLCHAIN cargo"

SERVICE_PID=""
SMOKE_HOME=""
cleanup() {
  if [ -n "$SERVICE_PID" ] && kill -0 "$SERVICE_PID" 2>/dev/null; then
    echo "cleanup: service $SERVICE_PID still running, sending SIGTERM" >&2
    kill -TERM "$SERVICE_PID" 2>/dev/null || true
    wait "$SERVICE_PID" 2>/dev/null || true
  fi
  if [ -n "$SMOKE_HOME" ] && [ -d "$SMOKE_HOME" ]; then
    rm -rf "$SMOKE_HOME"
  fi
}
trap cleanup EXIT

echo "== toolchain: rustup run $TOOLCHAIN ($(rustup run "$TOOLCHAIN" rustc --version | head -n 1))"

echo "== [1/5] build the service binary (locked, offline, isolated target dir)"
$CARGO build --manifest-path rust/Cargo.toml --locked -p lingxi-service \
  > "$EVIDENCE_DIR/a01-build.log" 2>&1
tail -n 2 "$EVIDENCE_DIR/a01-build.log"
BIN="$TARGET_DIR/debug/lingxi-service"
test -x "$BIN"

echo "== [2/5] dependency-tree desktop proof (cargo metadata resolve graph)"
$CARGO metadata --manifest-path rust/Cargo.toml --format-version 1 --offline \
  > "$EVIDENCE_DIR/a01-cargo-metadata.json" 2> "$EVIDENCE_DIR/a01-cargo-metadata.err"
python3 - "$EVIDENCE_DIR/a01-cargo-metadata.json" > "$EVIDENCE_DIR/a01-deptree-evidence.log" <<'PY'
import json, re, sys

meta = json.load(open(sys.argv[1]))
# Same matching semantics as r01_t01_check_ownership.py D1: package-id keyed
# resolve graph, hyphen/underscore-normalized substring patterns.
DESKTOP_PATTERNS = ["tauri", "electron", "tao", "wry", "webkit2gtk", "winit", "webview"]

def norm(name: str) -> str:
    return name.lower().replace("_", "-")

names = {p["id"]: norm(p["name"]) for p in meta["packages"]}
hits = {}
for node in meta["resolve"]["nodes"]:
    for pattern in DESKTOP_PATTERNS:
        if pattern in names.get(node["id"], ""):
            hits.setdefault(names[node["id"]], []).append(pattern)
members = sorted(names[m] for m in meta["workspace_members"])
print(f"workspace members ({len(members)}): {members}")
print(f"resolve-graph packages: {len(meta['resolve']['nodes'])}")
if hits:
    print(f"DESKTOP DEPENDENCIES FOUND: {hits}")
    sys.exit(1)
print("desktop dependency scan: 0 hits for patterns", DESKTOP_PATTERNS)
PY
tail -n 3 "$EVIDENCE_DIR/a01-deptree-evidence.log"

echo "== [3/5] start real service process (synthetic /tmp home, loopback, ephemeral port)"
SMOKE_HOME="$(mktemp -d /tmp/lingxi-r02-t01-smoke-home.XXXXXX)"
"$BIN" --home "$SMOKE_HOME" \
  > "$EVIDENCE_DIR/a01-service-stdout.log" 2> "$EVIDENCE_DIR/a01-service-stderr.log" &
SERVICE_PID=$!

ADDR=""
for _ in $(seq 1 300); do
  if ! kill -0 "$SERVICE_PID" 2>/dev/null; then
    echo "ERROR: service exited before becoming ready" >&2
    cat "$EVIDENCE_DIR/a01-service-stdout.log" >&2
    cat "$EVIDENCE_DIR/a01-service-stderr.log" >&2
    exit 1
  fi
  READY="$(grep -o 'LINGXI_SERVICE_READY addr=[^ ]*' "$EVIDENCE_DIR/a01-service-stdout.log" | tail -n 1 || true)"
  if [ -n "$READY" ]; then
    ADDR="${READY#LINGXI_SERVICE_READY addr=}"
    break
  fi
  sleep 0.1
done
if [ -z "$ADDR" ]; then
  echo "ERROR: no LINGXI_SERVICE_READY line within 30s" >&2
  exit 1
fi
echo "service ready: pid=$SERVICE_PID addr=$ADDR home=$SMOKE_HOME"
test -d "$SMOKE_HOME"

echo "== [4/5] health check"
HTTP_CODE="$(curl -sS -o "$EVIDENCE_DIR/a01-health-body.json" -w '%{http_code}' \
  "http://$ADDR/lingxi/v1/health")"
{
  echo "GET /lingxi/v1/health -> HTTP $HTTP_CODE"
  cat "$EVIDENCE_DIR/a01-health-body.json"
  echo
} > "$EVIDENCE_DIR/a01-health-check.log"
[ "$HTTP_CODE" = "200" ]
python3 - "$EVIDENCE_DIR/a01-health-body.json" >> "$EVIDENCE_DIR/a01-health-check.log" <<'PY'
import json, sys

body = json.load(open(sys.argv[1]))
expected = {
    "status": "ok",
    "serverKind": "lingxi-service",
    "wireProtocolMin": 1,
    "wireProtocolMax": 1,
    "dataEpoch": 1,
}
for key, value in expected.items():
    assert body.get(key) == value, f"health field {key}: expected {value}, got {body.get(key)!r}"
assert set(body) == set(expected) | {"serverVersion"}, f"unexpected fields: {sorted(body)}"
print("health validation: OK (minimal surface, protocol-sourced versions)")
PY
tail -n 1 "$EVIDENCE_DIR/a01-health-check.log"

echo "== [5/5] clean shutdown (SIGTERM, exit 0, port freed, no children)"
kill -TERM "$SERVICE_PID"
set +e
wait "$SERVICE_PID"
EXIT_CODE=$?
set -e
echo "service exit code after SIGTERM: $EXIT_CODE" | tee -a "$EVIDENCE_DIR/a01-shutdown.log"
[ "$EXIT_CODE" -eq 0 ] || { echo "ERROR: expected exit 0" >&2; exit 1; }

if pgrep -P "$SERVICE_PID" >/dev/null 2>&1; then
  echo "ERROR: child processes of $SERVICE_PID still alive" >&2
  pgrep -lP "$SERVICE_PID" >&2 || true
  exit 1
fi
echo "no leftover child processes: OK" | tee -a "$EVIDENCE_DIR/a01-shutdown.log"
if curl -sS --max-time 2 -o /dev/null "http://$ADDR/lingxi/v1/health" 2>/dev/null; then
  echo "ERROR: port $ADDR still accepting after shutdown" >&2
  exit 1
fi
echo "port closed after shutdown: OK" | tee -a "$EVIDENCE_DIR/a01-shutdown.log"
rm -rf "$SMOKE_HOME"; SMOKE_HOME=""

echo "A01 RESULT: PASS (build, desktop-free dep tree, real-process start, health 200, clean SIGTERM shutdown)"

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
# 每轮证据目录必须全新，防止独立直跑时旧日志覆盖或冒充本轮结果。
if [ -L "$EVIDENCE_DIR" ] || { [ -e "$EVIDENCE_DIR" ] && [ ! -d "$EVIDENCE_DIR" ]; }; then
  echo "ERROR: evidence path is not a regular directory: $EVIDENCE_DIR" >&2
  exit 1
fi
if [ -d "$EVIDENCE_DIR" ]; then
  FIRST_ENTRY="$(find "$EVIDENCE_DIR" -mindepth 1 -print -quit)" || exit 1
  [ -z "$FIRST_ENTRY" ] || { echo "ERROR: evidence directory is not empty: $EVIDENCE_DIR" >&2; exit 1; }
fi
mkdir -p "$EVIDENCE_DIR"
TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/rust-target-r02-t01}"

# 选根与来源是 INFO 诊断，也是下方原保护断言的必需前置。
# 固定有效配置，避免调用者的 warn 过滤把真实拒启误报为切根失败。
export RUST_LOG=info

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
# R02 stage-repair R7 / R7-F02: SERVICE_PID is the CURRENT handle of this
# run's service child — set on spawn, RETIRED (cleared) immediately after
# every wait/reap. A reaped number can be recycled into an unrelated
# process, so the trap signals a pid ONLY while it still proves CURRENT
# ownership: it exists AND its ppid is THIS shell (the R6-F02 A12
# pattern). A retired or recycled number is never probed or signalled.
# R12-F01: the boolean probe's false branch conflated exited/foreign/
# unobservable — safe for "don't signal", but it cannot tell a REAPABLE
# object (bash already collected it) from an UNKNOWN one, and the trap's
# owned branch did `kill -TERM; wait` with NO deadline: a child ignoring
# or delaying TERM hung the trap itself. The four-state probe plus the
# bounded ladder below keep the retirement contract and give the trap an
# explicit total budget; an unattributable object is never signalled and
# an overdue survivor is reported as residue instead of waited on.
child_state() {
  # child_state <pid> → exited | owned | foreign | unobservable
  local ppid
  if ! kill -0 "$1" 2>/dev/null; then
    printf 'exited\n'
    return 0
  fi
  ppid="$(ps -o ppid= -p "$1" 2>/dev/null | tr -d '[:space:]')"
  if [ -z "$ppid" ]; then
    printf 'unobservable\n'
  elif [ "$ppid" = "$$" ]; then
    printf 'owned\n'
  else
    printf 'foreign\n'
  fi
}
# R12-F01: bounded stop for ONE provably-owned handle, matching THIS
# script's lifecycle contract (TERM is the graceful-stop signal the
# normal shutdown-hygiene path asserts on): TERM → ≤5 s poll → KILL on
# the direct pid ONLY if it is still provably ours at that instant (no
# group signal, no bare number) → ≤5 s re-check. Prints the final state.
# Total worst case per handle ≈ 10 s; never waits unboundedly.
bounded_stop_owned() {
  local pid="$1" state="" i
  # 发信号前在函数内再次核实，调用方的先前判断不能替代当前归属。
  state="$(child_state "$pid")"
  if [ "$state" != "owned" ]; then printf '%s\n' "$state"; return 0; fi
  kill -TERM "$pid" 2>/dev/null || true
  for i in $(seq 1 100); do
    state="$(child_state "$pid")"
    case "$state" in
      exited|foreign) break ;;
      owned|unobservable) : ;;
    esac
    sleep 0.05
  done
  if [ "${state:-owned}" = "owned" ]; then
    state="$(child_state "$pid")"
  fi
  if [ "$state" = "owned" ]; then
    kill -KILL "$pid" 2>/dev/null || true
    for i in $(seq 1 100); do
      state="$(child_state "$pid")"
      case "$state" in
        exited|foreign) break ;;
        owned|unobservable) : ;;
      esac
      sleep 0.05
    done
  fi
  printf '%s\n' "${state:-unobservable}"
}
cleanup() {
  local cleanup_residue=0
  if [ -n "$SERVICE_PID" ]; then
    case "$(child_state "$SERVICE_PID")" in
      owned)
        case "$(bounded_stop_owned "$SERVICE_PID")" in
          exited)
            wait "$SERVICE_PID" 2>/dev/null || true
            ;;
          foreign)
            echo "cleanup: pid $SERVICE_PID 已不属于本脚本，不等待或发信号" >&2
            cleanup_residue=1
            ;;
          owned)
            echo "cleanup: pid $SERVICE_PID still OWNED after the TERM and KILL budgets — RESIDUE left behind, no unbounded wait" >&2
            cleanup_residue=1
            ;;
          unobservable)
            echo "cleanup: pid $SERVICE_PID state UNOBSERVABLE after the stop budgets — not signalled further, no unbounded wait; possible residue" >&2
            cleanup_residue=1
            ;;
        esac
        ;;
      exited)
        wait "$SERVICE_PID" 2>/dev/null || true
        ;;
      foreign)
        echo "cleanup: pid $SERVICE_PID is NOT currently owned by this shell — NOT signalled" >&2
        cleanup_residue=1
        ;;
      unobservable)
        echo "cleanup: pid $SERVICE_PID ownership UNOBSERVABLE (ps unreadable) — NOT signalled" >&2
        cleanup_residue=1
        ;;
    esac
    SERVICE_PID=""
  fi
  if [ "$cleanup_residue" -eq 0 ] && [ -n "$SMOKE_HOME" ] && [ -d "$SMOKE_HOME" ]; then
    rm -rf "$SMOKE_HOME"
  fi
  if [ "$cleanup_residue" -ne 0 ]; then
    echo "cleanup: 进程仍存活或归属不明，保留本轮 home=$SMOKE_HOME 供核查" >&2
    exit 1
  fi
  return 0
}
trap cleanup EXIT

# R14-F01 (R02 stage-repair R14): per-case structured records for the R00
# supplemental-leaf gate. The stage map's leaf R00-T02-LA-1B09760C2B1C (CLI
# serve — "前台服务进程本体与配置/实例边界") consumes a01-leaf-cases.json:
# verify-stage checks each declared case's ACTUAL value, so a green command
# exit alone can never pass that leaf.
CASES_NDJSON="$EVIDENCE_DIR/leaf-cases.ndjson"
: > "$CASES_NDJSON"
record_case() { # $1=case $2=expect $3=actual $4=ok(1/0)
  python3 -c 'import json,sys
print(json.dumps({"case": sys.argv[1], "expect": int(sys.argv[2]),
                  "actual": int(sys.argv[3]), "ok": sys.argv[4] == "1"}))' \
    "$1" "$2" "$3" "$4" >> "$CASES_NDJSON"
}
record_ok() { # $1=case — boolean observation that held
  record_case "$1" 1 1 1
}
record_bad() { # $1=case — boolean observation that FAILED; recorded, then exit 1
  record_case "$1" 1 0 0
  assemble_leaf_cases || true
  echo "ERROR: case $1 failed" >&2
  exit 1
}
assemble_leaf_cases() {
  python3 - "$CASES_NDJSON" "$EVIDENCE_DIR/a01-leaf-cases.json" <<'PY'
import json, sys
cases = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
doc = {"schema": "lingxi.leaf-case-results.v1", "cases": cases}
with open(sys.argv[2], "w") as fh:
    json.dump(doc, fh, ensure_ascii=False, indent=1)
bad = [c for c in cases if not c["ok"]]
print(f"a01-leaf-cases.json: {len(cases)} cases, {len(bad)} failing")
sys.exit(1 if bad else 0)
PY
}
# 负面服务在独立合成 home 里启动；即便意外一直运行，也只做有界回收。
run_refusal() {
  local tag="$1" fail_case="$2" state="" i
  shift 2
  "$@" > "$EVIDENCE_DIR/$tag.stdout.log" 2> "$EVIDENCE_DIR/$tag.stderr.log" &
  SERVICE_PID=$!
  for i in $(seq 1 200); do
    state="$(child_state "$SERVICE_PID")"
    case "$state" in exited|foreign) break ;; owned|unobservable) : ;; esac
    sleep 0.05
  done
  case "$state" in
    exited) ;;
    *) record_bad "$fail_case" ;;
  esac
  if wait "$SERVICE_PID" 2>/dev/null; then REFUSAL_RC=0; else REFUSAL_RC=$?; fi
  SERVICE_PID=""
}
# 同时登记空目录、文件内容和链接目标；拒绝前后逐字比较，避免只数文件。
snapshot_homes() {
  python3 - "$@" <<'PY'
import hashlib, os, pathlib, sys
for root_name in sys.argv[1:]:
    root = pathlib.Path(root_name)
    print("ROOT", root)
    for base, dirs, files in os.walk(root, followlinks=False):
        for name in sorted(dirs + files):
            item = pathlib.Path(base, name)
            rel = item.relative_to(root)
            if item.is_symlink():
                print("LINK", rel, os.readlink(item))
            elif item.is_dir():
                print("DIR", rel)
            elif item.is_file():
                print("FILE", rel, hashlib.sha256(item.read_bytes()).hexdigest())
            else:
                print("OTHER", rel)
PY
}

echo "== toolchain: rustup run $TOOLCHAIN ($(rustup run "$TOOLCHAIN" rustc --version | head -n 1))"

echo "== [1/5] build the service binary (locked, offline, isolated target dir)"
$CARGO build --manifest-path rust/Cargo.toml --locked -p lingxi-service \
  > "$EVIDENCE_DIR/a01-build.log" 2>&1
tail -n 2 "$EVIDENCE_DIR/a01-build.log"
BIN="$TARGET_DIR/debug/lingxi-service"
if [ -x "$BIN" ]; then record_ok "a01-build-locked-offline"; else record_bad "a01-build-locked-offline"; fi

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
record_ok "a01-deptree-desktop-free"

echo "== [3/5] start real service process (synthetic /tmp home, loopback, ephemeral port)"
SMOKE_HOME=$(mktemp -d "${TMPDIR:-/tmp}/lingxi-r02-t01-smoke-home.XXXXXX")
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
record_ok "a01-service-start-ready"

echo "== [4/5] health check"
HTTP_CODE="$(curl -sS -o "$EVIDENCE_DIR/a01-health-body.json" -w '%{http_code}' \
  "http://$ADDR/lingxi/v1/health")"
{
  echo "GET /lingxi/v1/health -> HTTP $HTTP_CODE"
  cat "$EVIDENCE_DIR/a01-health-body.json"
  echo
} > "$EVIDENCE_DIR/a01-health-check.log"
if [ "$HTTP_CODE" = "200" ]; then
  record_case "a01-health-200" 200 "$HTTP_CODE" 1
else
  record_case "a01-health-200" 200 "$HTTP_CODE" 0
  echo "ERROR: health check expected 200, got $HTTP_CODE" >&2
  exit 1
fi
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
# R7-F02: snapshot the service's children (pid + ps birth time) BEFORE
# the TERM — while the parent is still our live child, parentage is a
# sound attribution. After the reap a bare `pgrep -P <reaped-number>`
# would query whoever recycled the number; instead each snapshotted
# child is judged gone only when its number disappears OR its birth time
# changed (a recycled replacement is NOT our leftover and is never
# signalled).
CHILD_SNAPSHOT="$EVIDENCE_DIR/a01-child-snapshot.tsv"
: > "$CHILD_SNAPSHOT"
for cpid in $(pgrep -P "$SERVICE_PID" 2>/dev/null || true); do
  cbirth="$(ps -o lstart= -p "$cpid" 2>/dev/null | sed 's/  */ /g' || true)"
  printf '%s\t%s\n' "$cpid" "$cbirth" >> "$CHILD_SNAPSHOT"
done
# 正常关停也使用同一归属与期限检查；不能因 wait 卡住或清理失败仍报通过。
case "$(child_state "$SERVICE_PID")" in
  owned) STOP_STATE="$(bounded_stop_owned "$SERVICE_PID")" ;;
  *) record_bad "a01-sigterm-clean-exit-0" ;;
esac
case "$STOP_STATE" in
  exited) ;;
  *) record_bad "a01-sigterm-clean-exit-0" ;;
esac
if wait "$SERVICE_PID"; then EXIT_CODE=0; else EXIT_CODE=$?; fi
# reaped → the handle RETIRES immediately; the trap can never signal
# this number again (R7-F02).
SERVICE_PID=""
echo "service exit code after SIGTERM: $EXIT_CODE" | tee -a "$EVIDENCE_DIR/a01-shutdown.log"
if [ "$EXIT_CODE" -eq 0 ]; then
  record_case "a01-sigterm-clean-exit-0" 0 "$EXIT_CODE" 1
else
  record_case "a01-sigterm-clean-exit-0" 0 "$EXIT_CODE" 0
  echo "ERROR: expected exit 0" >&2
  exit 1
fi

while IFS=$'\t' read -r cpid cbirth; do
  [ -n "$cpid" ] || continue
  cnow="$(ps -o lstart= -p "$cpid" 2>/dev/null | sed 's/  */ /g' || true)"
  if [ -n "$cnow" ] && [ -n "$cbirth" ] && [ "$cnow" = "$cbirth" ]; then
    echo "ERROR: child $cpid (same birth) still alive after service shutdown" >&2
    exit 1
  fi
done < "$CHILD_SNAPSHOT"
echo "no leftover child processes (pre-TERM snapshot, birth-time verified): OK" | tee -a "$EVIDENCE_DIR/a01-shutdown.log"
record_ok "a01-no-leftover-children"
if curl -sS --max-time 2 -o /dev/null "http://$ADDR/lingxi/v1/health" 2>/dev/null; then
  echo "ERROR: port $ADDR still accepting after shutdown" >&2
  record_bad "a01-port-closed-after-shutdown"
  exit 1
fi
echo "port closed after shutdown: OK" | tee -a "$EVIDENCE_DIR/a01-shutdown.log"
record_ok "a01-port-closed-after-shutdown"

echo "== [6/6] startup refusal and newer data version: explicit error, no fallback root =="
STARTUP_HOME="$SMOKE_HOME/negative-invalid-bind"
STARTUP_FALLBACK="$SMOKE_HOME/negative-invalid-bind-fallback"
mkdir -p "$STARTUP_HOME" "$STARTUP_FALLBACK"
snapshot_homes "$STARTUP_HOME" "$STARTUP_FALLBACK" > "$EVIDENCE_DIR/a01-invalid-bind-homes-before.txt"
LINGXI_HOME="$STARTUP_FALLBACK" HOME="$STARTUP_FALLBACK" \
  XDG_DATA_HOME="$STARTUP_FALLBACK/xdg-data" XDG_CONFIG_HOME="$STARTUP_FALLBACK/xdg-config" \
  run_refusal "a01-invalid-bind" "a01-startup-failure-no-ready" \
  "$BIN" --home "$STARTUP_HOME" --bind not-an-address
snapshot_homes "$STARTUP_HOME" "$STARTUP_FALLBACK" > "$EVIDENCE_DIR/a01-invalid-bind-homes-after.txt"
STARTUP_OK=0
if [ "$REFUSAL_RC" -ne 0 ] &&
   ! grep -q '^LINGXI_SERVICE_READY ' "$EVIDENCE_DIR/a01-invalid-bind.stdout.log" &&
   grep -q -- 'invalid --bind' "$EVIDENCE_DIR/a01-invalid-bind.stderr.log" &&
   cmp -s "$EVIDENCE_DIR/a01-invalid-bind-homes-before.txt" "$EVIDENCE_DIR/a01-invalid-bind-homes-after.txt"; then
  STARTUP_OK=1
fi
record_case "a01-startup-failure-no-ready" 1 "$STARTUP_OK" "$STARTUP_OK"
[ "$STARTUP_OK" -eq 1 ] || { assemble_leaf_cases || true; echo "ERROR: invalid startup did not fail explicitly before READY" >&2; exit 1; }

FALLBACK_HOME="$SMOKE_HOME/negative-fallback"
mkdir -p "$FALLBACK_HOME"
# 三种真实选根入口各用独立已标记数据目录；较新版本拒绝不能改写原数据或另建根。
seed_newer_home() {
  local selected="$1"
  mkdir -p "$selected/app-data"
  printf 'preserve-original-data\n' > "$selected/app-data/existing.bin"
  cat > "$selected/data-epoch.json" <<'JSON'
{
  "schemaVersion": 2,
  "epoch": 2,
  "minimumReaderEpoch": 2,
  "committedDataEpoch": 2,
  "lastVersion": "9.9.9",
  "updatedAt": "2026-09-26T08:00:00.000Z"
}
JSON
}
check_newer_refusal() {
  local tag="$1" selected="$2" source="$3" refused=0 explicit_error=0 no_switch=0
  shift 3
  snapshot_homes "$selected" "$FALLBACK_HOME" > "$EVIDENCE_DIR/$tag-homes-before.txt"
  snapshot_homes "$FALLBACK_HOME" > "$EVIDENCE_DIR/$tag-fallback-before.txt"
  shasum -a 256 "$selected/data-epoch.json" "$selected/app-data/existing.bin" \
    > "$EVIDENCE_DIR/$tag-original-before.sha256"
  run_refusal "$tag" "$tag-epoch-refused" "$@"
  snapshot_homes "$selected" "$FALLBACK_HOME" > "$EVIDENCE_DIR/$tag-homes-after.txt"
  snapshot_homes "$FALLBACK_HOME" > "$EVIDENCE_DIR/$tag-fallback-after.txt"
  shasum -a 256 "$selected/data-epoch.json" "$selected/app-data/existing.bin" \
    > "$EVIDENCE_DIR/$tag-original-after.sha256"
  if [ "$REFUSAL_RC" -eq 2 ] &&
     ! grep -q '^LINGXI_SERVICE_READY ' "$EVIDENCE_DIR/$tag.stdout.log"; then
    refused=1
  fi
  if grep -q 'LINGXI_DATA_EPOCH_BLOCKED reason=epoch-downgrade-blocked' \
       "$EVIDENCE_DIR/$tag.stderr.log" &&
     grep -q 'epoch 2 or newer' "$EVIDENCE_DIR/$tag.stderr.log"; then
    explicit_error=1
  fi
  # 诊断/锁文件允许出现在原 home；原数据和备用 home 必须完全不变。
  if cmp -s "$EVIDENCE_DIR/$tag-original-before.sha256" "$EVIDENCE_DIR/$tag-original-after.sha256" &&
     cmp -s "$EVIDENCE_DIR/$tag-fallback-before.txt" "$EVIDENCE_DIR/$tag-fallback-after.txt" &&
     grep -Fq "effective_home=$selected" "$EVIDENCE_DIR/$tag.stderr.log" &&
     grep -Fq "source=$source" "$EVIDENCE_DIR/$tag.stderr.log" &&
     [ ! -e "$selected/lingxi-service/data" ] &&
     [ ! -e "$FALLBACK_HOME/lingxi-service/data" ] &&
     [ ! -e "$selected/lingxi-service/local-token.json" ] &&
     [ ! -e "$FALLBACK_HOME/lingxi-service/local-token.json" ] &&
     ! grep -q '^LINGXI_SERVICE_READY ' "$EVIDENCE_DIR/$tag.stdout.log"; then
    no_switch=1
  fi
  record_case "$tag-epoch-refused" 1 "$refused" "$refused"
  record_case "$tag-epoch-explicit-error" 1 "$explicit_error" "$explicit_error"
  record_case "$tag-epoch-no-root-switch" 1 "$no_switch" "$no_switch"
  if [ "$refused" -ne 1 ] || [ "$explicit_error" -ne 1 ] || [ "$no_switch" -ne 1 ]; then
    assemble_leaf_cases || true
    echo "ERROR: $tag newer data version refusal or root preservation failed" >&2
    exit 1
  fi
}

NEWER_HOME="$SMOKE_HOME/negative-newer-data"
seed_newer_home "$NEWER_HOME"
LINGXI_HOME="$FALLBACK_HOME" HOME="$FALLBACK_HOME" \
  XDG_DATA_HOME="$FALLBACK_HOME/xdg-data" XDG_CONFIG_HOME="$FALLBACK_HOME/xdg-config" \
  check_newer_refusal "a01-newer-data" "$NEWER_HOME" cli \
  "$BIN" --home "$NEWER_HOME" --bind 127.0.0.1:0

NEWER_ENV_HOME="$SMOKE_HOME/negative-newer-env-data"
seed_newer_home "$NEWER_ENV_HOME"
LINGXI_HOME="$NEWER_ENV_HOME" HOME="$FALLBACK_HOME" \
  XDG_DATA_HOME="$FALLBACK_HOME/xdg-data" XDG_CONFIG_HOME="$FALLBACK_HOME/xdg-config" \
  check_newer_refusal "a01-newer-env-data" "$NEWER_ENV_HOME" env \
  "$BIN" --bind 127.0.0.1:0

NEWER_CONFIG_HOME="$SMOKE_HOME/negative-newer-config-data"
NEWER_CONFIG_FILE="$SMOKE_HOME/negative-newer-config.json"
seed_newer_home "$NEWER_CONFIG_HOME"
python3 - "$NEWER_CONFIG_FILE" "$NEWER_CONFIG_HOME" <<'PY'
import json, sys
with open(sys.argv[1], "w") as fh:
    json.dump({"home": sys.argv[2]}, fh)
PY
HOME="$FALLBACK_HOME" XDG_DATA_HOME="$FALLBACK_HOME/xdg-data" \
  XDG_CONFIG_HOME="$FALLBACK_HOME/xdg-config" \
  check_newer_refusal "a01-newer-config-data" "$NEWER_CONFIG_HOME" config-file \
  env -u LINGXI_HOME "$BIN" --config "$NEWER_CONFIG_FILE" --bind 127.0.0.1:0
rm -rf "$SMOKE_HOME"; SMOKE_HOME=""

# 汇总案例，任何失败都使脚本非零。
assemble_leaf_cases

echo "A01 RESULT: PASS (build, desktop-free dep tree, real-process start, health 200, clean SIGTERM shutdown)"

#!/usr/bin/env bash
# r01-t04-replay.sh — R01-T04 浏览器宿主原型一键重放（本机 macOS arm64）。
#
# 流程：起本地测试站(18281)+代理(18282，仅 loopback 转发) → cargo build spike →
# 编译 swift 辅助 → 跑 Chromium 四阶段（main/isolation/proxy/takeover）→
# 编译+跑 WKWebView spike → 跑 Electron harness 三模式 → 汇总 SHA-256。
#
# 用法：bash scripts/rust-tauri/r01-t04-replay.sh [--skip-takeover]
# 依赖：本机 /Applications/Google Chrome.app（受控 Chromium 宿主）、swiftc、node、
#       repo node_modules/electron。产物输出到 artifacts/rust-tauri/R01/T04/replay-<ts>/。
# 浏览器数据目录与 CARGO_TARGET_DIR 全部隔离在 /tmp/r01t04-replay。

set -u
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TS="$(date +%Y%m%d-%H%M%S)"
EVIDENCE="$REPO_ROOT/artifacts/rust-tauri/R01/T04/replay-$TS"
WORK=/tmp/r01t04-replay
export CARGO_TARGET_DIR="$WORK/cargo-target"
SITE_PORT=18281
PROXY_PORT=18282
SKIP_TAKEOVER=0
[ "${1:-}" = "--skip-takeover" ] && SKIP_TAKEOVER=1

unset all_proxy ALL_PROXY http_proxy HTTP_PROXY https_proxy HTTPS_PROXY
mkdir -p "$EVIDENCE" "$WORK"
FAIL=0
note() { echo "[replay] $*"; }

# ── 1. 本地测试站 + 代理 ──
kill_port() { local p=$1; local pid; pid=$(lsof -nP -iTCP:"$p" -sTCP:LISTEN -t 2>/dev/null || true); [ -n "$pid" ] && kill "$pid" 2>/dev/null || true; }
kill_port $SITE_PORT; kill_port $PROXY_PORT
node "$REPO_ROOT/tests/migration/r01-t04/testsite.mjs" > "$EVIDENCE/testsite.log" 2>&1 &
SITE_PID=$!
node "$REPO_ROOT/tests/migration/r01-t04/proxy.mjs" > "$EVIDENCE/proxy.log" 2>&1 &
PROXY_PID=$!
sleep 1
curl -s --noproxy '*' -o /dev/null -w 'site HTTP=%{http_code}\n' "http://127.0.0.1:$SITE_PORT/form" || FAIL=1

# ── 2. 构建 spike + swift 辅助 ──
(cd "$REPO_ROOT/rust" && cargo build -p lingxi-browser-spike --offline) > "$EVIDENCE/cargo-build.log" 2>&1 || FAIL=1
SPIKE_BIN="$CARGO_TARGET_DIR/debug/spike_browser"
swiftc -O -o "$WORK/list_windows" "$REPO_ROOT/tests/migration/r01-t04/list_windows.swift" 2> "$EVIDENCE/swiftc-helper.log" || FAIL=1
swiftc -O -o "$WORK/wkwebview_spike" "$REPO_ROOT/tests/migration/r01-t04/wkwebview_spike.swift" 2> "$EVIDENCE/swiftc-wk.log" || FAIL=1

run_phase() { # phase extra-args...
  local phase=$1; shift
  mkdir -p "$EVIDENCE/chromium-$phase"
  cp "$WORK/list_windows" "$EVIDENCE/chromium-$phase/list_windows" 2>/dev/null || true
  "$SPIKE_BIN" run --site "http://127.0.0.1:$SITE_PORT" --repo "$REPO_ROOT" \
    --evidence "$EVIDENCE/chromium-$phase" --profile-root "$WORK/profiles" \
    --phases "$phase" --proxy-port $PROXY_PORT "$@" > "$EVIDENCE/chromium-$phase/run.log" 2>&1
  local rc=$?
  echo "[replay] chromium-$phase exit=$rc"
  [ $rc -ne 0 ] && FAIL=1
}

# ── 3. Chromium 四阶段 ──
run_phase main
run_phase isolation
run_phase proxy
[ $SKIP_TAKEOVER -eq 0 ] && run_phase takeover

# ── 4. WKWebView spike ──
mkdir -p "$EVIDENCE/wkwebview"
"$WORK/wkwebview_spike" --site "http://127.0.0.1:$SITE_PORT" --repo "$REPO_ROOT" \
  --evidence "$EVIDENCE/wkwebview" > "$EVIDENCE/wkwebview/run.log" 2>&1
WK_RC=$?
echo "[replay] wkwebview exit=$WK_RC（0=无 FAILED；W6 上传/W13 代理/W16 接管为如实 UNVERIFIED 不计失败）"
[ $WK_RC -ne 0 ] && FAIL=1

# ── 5. Electron harness 三模式 ──
ELEC="$REPO_ROOT/node_modules/.bin/electron"
mkdir -p "$EVIDENCE/electron"
rm -rf "$WORK/electron-profile" "$WORK/electron-profile-persist"
"$ELEC" "$REPO_ROOT/tests/migration/r01-t04/electron_harness.cjs" -- \
  --site "http://127.0.0.1:$SITE_PORT" --repo "$REPO_ROOT" --evidence "$EVIDENCE/electron" \
  --user-data "$WORK/electron-profile" > "$EVIDENCE/electron/run-main.log" 2>&1 || FAIL=1
"$ELEC" "$REPO_ROOT/tests/migration/r01-t04/electron_harness.cjs" -- \
  --site "http://127.0.0.1:$SITE_PORT" --repo "$REPO_ROOT" --evidence "$EVIDENCE/electron" \
  --user-data "$WORK/electron-profile-persist" --mode login-write > "$EVIDENCE/electron/run-login-write.log" 2>&1 || FAIL=1
"$ELEC" "$REPO_ROOT/tests/migration/r01-t04/electron_harness.cjs" -- \
  --site "http://127.0.0.1:$SITE_PORT" --repo "$REPO_ROOT" --evidence "$EVIDENCE/electron" \
  --user-data "$WORK/electron-profile-persist" --mode login-verify > "$EVIDENCE/electron/run-login-verify.log" 2>&1 || FAIL=1

# ── 6. 收尾 + SHA-256 汇总 ──
kill $SITE_PID $PROXY_PID 2>/dev/null || true
pkill -f 'remote-debugging-pipe' 2>/dev/null || true
(cd "$EVIDENCE" && find . -type f ! -name 'SHA256SUMS.txt' -exec shasum -a 256 {} + | sort -k2 > SHA256SUMS.txt)
note "evidence=$EVIDENCE"
note "overall FAIL=$FAIL（0=全部可重放步骤通过）"
exit $FAIL

#!/usr/bin/env bash
# R01-T05 repair-r1 负向用例（对应 R1 验收 F1/F2）：
#   case1 ws-negative      — WebSocket 边界：ws://loopback canary 与 wss://远端必须被
#                            Network.setBlockedURLs 阻断（canary 零命中 + 页面标记 BLOCKED），
#                            页面其余部分正常产出（exit 0）。
#   case2 print-timeout    — load 后挂死主线程使 printToPDF 阶段超时，契约 exit=2
#                            （修复前实测 exit 1）、零伪产物。
# 用法：bash tests/migration/r01-t05/run_repair_r1_negative.sh [artifacts_dir]
# 红线：全程 loopback-only 代理 + canary 仅绑 127.0.0.1；数据目录 /tmp。
# 注意：ws-loopback.html 内 canary URL 端口硬编码 18291，CANARY_PORT 不可改。
set -u
REPO="$(cd "$(dirname "$0")/../../.." && pwd)"
ART="${1:-/tmp/r01t05-repair-negative}"
TMP=/tmp/r01t05-repair-negative
T05=$REPO/tests/migration/r01-t05
PROXY_PORT="${R01_T05_PROXY_PORT:-18482}"
CANARY_PORT=18291
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/r01t05-repair/cargo-target}"
export PATH="$HOME/.cargo/bin:$PATH"

unset all_proxy ALL_PROXY http_proxy HTTP_PROXY https_proxy HTTPS_PROXY

mkdir -p "$ART" "$TMP"
VERDICTS="$ART/negative-verdicts.txt"
: > "$VERDICTS"
FAIL=0
say() { echo "[repair-r1-neg] $*" >&2; }
verdict() { # name PASS/FAIL detail
  echo "$1 $2 $3" >> "$VERDICTS"
  say "$1 => $2 ($3)"
  [ "$2" = PASS ] || FAIL=1
}

# ---------- 构建 ----------
say "build spike_pdf + pdf_probe"
(cd "$REPO/rust" && cargo build --locked --offline -p lingxi-browser-spike) || { echo "cargo build FAILED"; exit 1; }
SPIKE="$CARGO_TARGET_DIR/debug/spike_pdf"
swiftc -O -o "$TMP/pdf_probe" "$T05/pdf_probe.swift" || { echo "swiftc FAILED"; exit 1; }

# ---------- loopback 代理 + canary ----------
node "$REPO/tests/migration/r01-t04/proxy.mjs" "$PROXY_PORT" > "$ART/proxy.log" 2>&1 &
PROXY_PID=$!
CANARY_LOG="$ART/ws-canary.log"
: > "$CANARY_LOG"
node "$T05/canary_server.mjs" "$CANARY_PORT" "$CANARY_LOG" > "$ART/canary-server.log" 2>&1 &
CANARY_PID=$!
sleep 1
kill -0 $PROXY_PID && kill -0 $CANARY_PID || { echo "proxy/canary start FAILED"; exit 1; }
cleanup() { kill $PROXY_PID $CANARY_PID 2>/dev/null; }
trap cleanup EXIT

mkjob() { # out_json html output extra-json
  python3 - "$1" "$2" "$3" "$4" <<'PY'
import json, sys
job = {"htmlPath": sys.argv[2], "outputPath": sys.argv[3],
       "viewport": {"width": 1280, "height": 900},
       "printBackground": True, "preferCSSPageSize": True, "pageSize": "A4",
       "allowJavaScript": False, "embedLingxiFonts": True, "settleMs": 250, "timeoutMs": 60000}
if sys.argv[4] != "-":
    job.update(json.loads(sys.argv[4]))
json.dump(job, open(sys.argv[1], "w"), ensure_ascii=False, indent=1)
PY
}

# ---------- case1: WebSocket 边界 ----------
say "case1 ws-negative"
D1="$ART/ws-negative"
mkdir -p "$D1"
mkjob "$TMP/job-ws.json" "$T05/negative/ws-loopback.html" "$D1/ws.pdf" \
  '{"allowJavaScript": true, "settleMs": 2000, "timeoutMs": 30000}'
"$SPIKE" run --job "$TMP/job-ws.json" --repo "$REPO" --evidence "$D1" \
  --profile-root "$TMP/profiles-ws" --proxy "127.0.0.1:$PROXY_PORT" \
  > "$D1/run-result.json" 2> "$D1/run.stderr"
EC=$?
if [ "$EC" -eq 0 ] && [ -s "$D1/ws.pdf" ]; then
  verdict ws-negative-exit PASS "exit=0 且页面正常产出（封堵不误伤渲染）"
else
  verdict ws-negative-exit FAIL "exit=$EC（期望 0）"
fi
if grep -q "t05-ws-repair-probe" "$CANARY_LOG"; then
  verdict ws-negative-canary FAIL "canary 命中（WS 逃逸）: $(grep -c t05-ws-repair-probe "$CANARY_LOG") 次"
else
  verdict ws-negative-canary PASS "canary 对 ws://127.0.0.1:$CANARY_PORT 握手零命中"
fi
mkdir -p "$D1/probe"
"$TMP/pdf_probe" "$D1/ws.pdf" "$D1/probe" 2 > "$D1/probe/stdout.json" 2>&1
TXT="$D1/probe/text.txt"
if grep -q "WS-LOOPBACK-BLOCKED" "$TXT" && grep -q "WS-REMOTE-BLOCKED" "$TXT" \
   && ! grep -q "WS-LOOPBACK-OPEN" "$TXT" && ! grep -q "WS-REMOTE-OPEN" "$TXT" \
   && ! grep -q "PENDING" "$TXT"; then
  verdict ws-negative-pagemarker PASS "页面 JS 观测：loopback/remote 均 BLOCKED"
else
  verdict ws-negative-pagemarker FAIL "页面标记异常: $(grep -o 'WS-[A-Z-]*' "$TXT" | tr '\n' ' ')"
fi

# ---------- case2: printToPDF 阶段超时 exit=2 ----------
say "case2 print-timeout"
D2="$ART/print-timeout"
mkdir -p "$D2"
# embedLingxiFonts=false：挂死点须落在 printToPDF 阶段（复刻验收 ATK5 形态）；
# 若开字体注入，主线程挂死会让 CSS.enable 先超时（仍属超时 exit=2，但偏离 ATK5 复现口径）。
mkjob "$TMP/job-hang.json" "$T05/negative/hang-after-load.html" "$D2/hang.pdf" \
  '{"allowJavaScript": true, "timeoutMs": 6000, "embedLingxiFonts": false}'
"$SPIKE" run --job "$TMP/job-hang.json" --repo "$REPO" --evidence "$D2" \
  --profile-root "$TMP/profiles-hang" --proxy "127.0.0.1:$PROXY_PORT" \
  > "$D2/run-result.json" 2> "$D2/run.stderr"
EC=$?
ERR=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["outcome"].get("error",""))' "$D2/run-result.json" 2>/dev/null)
if [ "$EC" -eq 2 ]; then
  verdict print-timeout-exit PASS "exit=2（printToPDF 阶段超时按契约归类）"
else
  verdict print-timeout-exit FAIL "exit=$EC（期望 2）"
fi
if [ ! -e "$D2/hang.pdf" ]; then
  verdict print-timeout-artifact PASS "零伪产物"
else
  verdict print-timeout-artifact FAIL "hang.pdf 不应存在"
fi
case "$ERR" in
  *printToPDF*timeout*) verdict print-timeout-error PASS "error=$ERR" ;;
  *) verdict print-timeout-error FAIL "error=$ERR" ;;
esac
PROF_REMOVED=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["cleanup"]["profile_removed"])' "$D2/run-result.json" 2>/dev/null)
if [ "$PROF_REMOVED" = "True" ]; then
  verdict print-timeout-cleanup PASS "profile_removed=true"
else
  verdict print-timeout-cleanup FAIL "profile_removed=$PROF_REMOVED"
fi

say "done; verdicts at $VERDICTS"
cat "$VERDICTS"
exit $FAIL

#!/bin/zsh
# R01-T06 桌面 E2E 可重放编排。用法：
#   scripts/run_e2e.sh <证据目录绝对路径>
# 步骤（每步落日志/退出码进证据目录）：
#   0  环境留证
#   1  构建 sidecar（release）+ 集成测试（真实二进制握手/拒绝/退出矩阵）
#   2  构建 app 双产物：test(--features e2e-test) 与 release（无 feature）
#   3  构造 updater 环回夹具（合成 artifact + 测试私钥签名 manifest；篡改 manifest 负向）
#   4  Run A：test 产物全量 E2E（能力探针 + A11 负向 + sidecar 崩溃恢复 + WebDriver）
#   5  Run B：release 产物探测（A12：无 WebDriver 监听；页面电池仍全拒；SIGKILL 后 sidecar 不变孤儿）
#   6  relaunch 探针（app:restart 对应物）
#   7  tauri build 打包 .app（真实签名链）+ 打包产物运行冒烟（autostart/SMAppService、sidecar 随包）
# 全程流量限 loopback；窗口 hidden；通知 1 次最小真实调用。
set -u
setopt PIPEFAIL 2>/dev/null || true

SPIKE_DIR="$(cd "$(dirname "$0")/.." && pwd)"
EVID="$1"
[ -z "${EVID:-}" ] && { echo "usage: run_e2e.sh <evidence-dir>"; exit 2; }
# repair-r1 (F4/R2)：macOS 上 /tmp 是 /private/tmp 的符号链接，Tauri 拒绝符号链接路径下的
# current_exe()（"StartingBinary found current_exe() that contains a symlink"，报告 R2），
# 证据目录经符号链接时 sidecar restart / relaunch 会语义失败。runner 入口统一 realpath
# 证据目录并显式记录，保证符号链接工作目录下结果真实而非假绿。
mkdir -p "$EVID"
EVID_REAL="$(cd "$EVID" && pwd -P)"
if [ "$EVID_REAL" != "$EVID" ]; then
  echo "run_e2e: evidence dir '$EVID' resolves through symlink -> '$EVID_REAL' (using realpath; Tauri rejects symlink current_exe, see TAURI_SPIKE_REPORT R2)"
  EVID="$EVID_REAL"
fi
mkdir -p "$EVID"/{build,run-test,run-release,run-relaunch,run-bundle,sidecar-it,updates,bundle}
SCRATCH=/tmp/lingxi-t06-scratch
rm -rf "$SCRATCH"; mkdir -p "$SCRATCH"
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR_SIDE=/tmp/lingxi-t06-target-sidecar
export CARGO_TARGET_DIR_APP=/tmp/lingxi-t06-target-app
export UNPROXY="env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY"
TRIPLE=aarch64-apple-darwin
WD_PORT=19275
EV_PORT=19274
SUMMARY="$EVID/runner-summary.txt"
: > "$SUMMARY"
log() { echo "$*" | tee -a "$SUMMARY"; }
[ "$EVID" != "$1" ] && log "note: evidence dir realpathed '$1' -> '$EVID' (symlink path; see TAURI_SPIKE_REPORT R2)"
record() { # record <step> <expected> <actual> <exit>
  echo "[$1] expected=[$2] actual=[$3] exit=$4" >> "$SUMMARY"
}

# ---------- 0 环境留证 ----------
{
  echo "date: $(date -u +%FT%TZ)"
  echo "uname: $(uname -a)"
  sw_vers
  echo "HEAD: $(git -C "$SPIKE_DIR/../.." rev-parse HEAD)"
  echo "git status --porcelain:"; git -C "$SPIKE_DIR/../.." status --porcelain
  echo "rustc: $(rustc --version)"; echo "cargo: $(cargo --version)"
  echo "node: $(node --version)"
  echo "proxy env present: ${http_proxy:+http_proxy}${https_proxy:+https_proxy}${all_proxy:+all_proxy}"
  echo "tauri CLI: $(cd "$SPIKE_DIR/app" && npx tauri --version 2>&1)"
} > "$EVID/preflight.log" 2>&1
log "step0 preflight done"

# ---------- 1 sidecar ----------
( cd "$SPIKE_DIR/sidecar" && CARGO_TARGET_DIR=$CARGO_TARGET_DIR_SIDE ${=UNPROXY} cargo build --release ) \
  > "$EVID/build/sidecar-build.log" 2>&1
SC_EC=$?
SIDECAR_BIN=$CARGO_TARGET_DIR_SIDE/release/lingxi-t06-sidecar
cp "$SIDECAR_BIN" "$SPIKE_DIR/app/src-tauri/binaries/lingxi-t06-sidecar-$TRIPLE"
record "sidecar-build" "exit 0" "$(tail -1 "$EVID/build/sidecar-build.log")" "$SC_EC"

( cd "$SPIKE_DIR/app/src-tauri" && CARGO_TARGET_DIR=$CARGO_TARGET_DIR_APP SIDECAR_BIN="$SIDECAR_BIN" \
    ${=UNPROXY} cargo test --release --test sidecar_handshake ) > "$EVID/sidecar-it/cargo-test.log" 2>&1
IT_EC=$?
record "sidecar-integration-tests" "exit 0, 7 tests pass" "$(grep -E 'test result' "$EVID/sidecar-it/cargo-test.log" | tail -1)" "$IT_EC"

# ---------- 2 app 双产物 ----------
( cd "$SPIKE_DIR/app/src-tauri" && CARGO_TARGET_DIR=$CARGO_TARGET_DIR_APP \
    ${=UNPROXY} cargo build --release --features e2e-test ) > "$EVID/build/app-test-build.log" 2>&1
BT_EC=$?
cp $CARGO_TARGET_DIR_APP/release/lingxi-tauri-shell-spike "$EVID/build/app-test"
chmod +x "$EVID/build/app-test"
cp "$SIDECAR_BIN" "$EVID/build/lingxi-t06-sidecar"
record "app-test-build" "exit 0" "$(tail -1 "$EVID/build/app-test-build.log")" "$BT_EC"

( cd "$SPIKE_DIR/app/src-tauri" && CARGO_TARGET_DIR=$CARGO_TARGET_DIR_APP \
    ${=UNPROXY} cargo build --release ) > "$EVID/build/app-release-build.log" 2>&1
BR_EC=$?
cp $CARGO_TARGET_DIR_APP/release/lingxi-tauri-shell-spike "$EVID/build/app-release"
chmod +x "$EVID/build/app-release"
record "app-release-build" "exit 0" "$(tail -1 "$EVID/build/app-release-build.log")" "$BR_EC"

shasum -a 256 "$EVID/build/app-test" "$EVID/build/app-release" "$SIDECAR_BIN" > "$EVID/build/SHASUMS-binaries.txt"

# ---------- 3 updater 夹具 ----------
mkdir -p "$SCRATCH/updates-src"
echo "R01-T06 synthetic update payload v0.2.0 (not a real app)" > "$SCRATCH/updates-src/PAYLOAD.txt"
tar -czf "$EVID/updates/fake-update-0.2.0.tar.gz" -C "$SCRATCH/updates-src" PAYLOAD.txt
# 篡改件：字节不同（签名不变 -> 下载后验签必须失败），manifest 签名本身仍是合法 minisign 编码
echo "R01-T06 TAMPERED payload (bytes differ from signed artifact)" > "$SCRATCH/updates-src/PAYLOAD.txt"
tar -czf "$EVID/updates/fake-update-tampered.tar.gz" -C "$SCRATCH/updates-src" PAYLOAD.txt
# 测试密钥对与口令只存 /tmp（不入库不入报告）；首次自动生成。
KEY_DIR=/tmp/lingxi-t06-keys
if [ ! -f $KEY_DIR/t06-updater-test.key ]; then
  mkdir -p $KEY_DIR && chmod 700 $KEY_DIR
  uuidgen > $KEY_DIR/.pass && chmod 600 $KEY_DIR/.pass
  ( cd "$SPIKE_DIR/app" && ${=UNPROXY} npx tauri signer generate -w $KEY_DIR/t06-updater-test.key \
      -p "$(cat $KEY_DIR/.pass)" --force ) > "$EVID/updates/keygen.log" 2>&1
  echo "!! 重新生成了测试密钥；必须同步更新 tauri.conf.json 的 pubkey 并重跑步骤 2" | tee -a "$SUMMARY"
fi
[ ! -f $KEY_DIR/.pass ] && { echo "missing $KEY_DIR/.pass (throwaway key password file)" | tee -a "$SUMMARY"; exit 1; }
KEY_PASS="$(cat $KEY_DIR/.pass)"
( cd "$SPIKE_DIR/app" && ${=UNPROXY} npx tauri signer sign -f $KEY_DIR/t06-updater-test.key \
    -p "$KEY_PASS" "$EVID/updates/fake-update-0.2.0.tar.gz" ) > "$EVID/updates/sign.log" 2>&1
SIG=$(cat "$EVID/updates/fake-update-0.2.0.tar.gz.sig")
PUB=$(cat /tmp/lingxi-t06-keys/t06-updater-test.key.pub)
python3 - "$EVID" "$SIG" <<'EOF'
import json, sys
evid, sig = sys.argv[1], sys.argv[2]
m = {
  "version": "0.2.0",
  "pub_date": "2026-09-25T00:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": sig,
      "url": "http://127.0.0.1:19274/updates/fake-update-0.2.0.tar.gz"
    }
  }
}
open(f"{evid}/updates/latest.json", "w").write(json.dumps(m, indent=2))
# 负向：manifest 签名是合法 minisign 编码（对原始 artifact 有效），但 url 指向字节被篡改的包，
# updater 下载后验签必须失败（InvalidSignature 类错误）。
bad = dict(m)
bad["platforms"] = {"darwin-aarch64": dict(m["platforms"]["darwin-aarch64"])}
bad["platforms"]["darwin-aarch64"]["url"] = "http://127.0.0.1:19274/updates/fake-update-tampered.tar.gz"
open(f"{evid}/updates/latest-tampered.json", "w").write(json.dumps(bad, indent=2))
EOF
echo "pubkey-in-config-must-match: $PUB" >> "$EVID/updates/sign.log"
cp "$EVID/updates/latest.json" "$EVID/updates/latest-good.json" # 留底：B 轮后恢复用

# ---------- 证据服务器 ----------
pkill -f evidence_server.mjs 2>/dev/null
T06_EVIDENCE_DIR="$SCRATCH" T06_WWW_DIR="$SPIKE_DIR/scripts/www" \
  T06_PROBE_JS="$SPIKE_DIR/app/frontend/probe.js" T06_UPDATES_DIR="$EVID/updates" \
  nohup node "$SPIKE_DIR/scripts/evidence_server.mjs" > "$EVID/evidence-server.log" 2>&1 &
EV_PID=$!
sleep 1
curl -s --noproxy '*' "http://127.0.0.1:$EV_PORT/__status" > /dev/null || { log "evidence server failed"; exit 1; }

# swift 探针
mkdir -p /tmp/lingxi-t06-bin
swiftc -O -o /tmp/lingxi-t06-bin/tcc_probe "$SPIKE_DIR/scripts/tcc_probe.swift" 2> "$EVID/build/tcc-probe-build.log"
swiftc -O -o /tmp/lingxi-t06-bin/post_shortcut "$SPIKE_DIR/scripts/post_shortcut.swift" 2> "$EVID/build/post-shortcut-build.log"
swiftc -O -o /tmp/lingxi-t06-bin/post_escape "$SPIKE_DIR/scripts/post_escape.swift" 2> "$EVID/build/post-escape-build.log"
swiftc -O -o /tmp/lingxi-t06-bin/post_cmd_period "$SPIKE_DIR/scripts/post_cmd_period.swift" 2> "$EVID/build/post-cmd-period-build.log"

wait_reports() { # wait_reports <expected-count> <timeout-s>
  local n=$1 t=$2 i=0
  while [ $i -lt $t ]; do
    local c=$(curl -s --noproxy '*' "http://127.0.0.1:$EV_PORT/__status" | python3 -c "import json,sys;print(len(json.load(sys.stdin)['reports']))" 2>/dev/null || echo 0)
    [ "$c" -ge "$n" ] && return 0
    sleep 1; i=$((i+1))
  done
  return 1
}

# ---------- 4 Run A：test 产物 ----------
rm -f "$SCRATCH/page-reports.jsonl"
T06_MODE=e2e T06_SCRATCH="$SCRATCH" T06_HOST_REPORT="$EVID/run-test/host-report.ndjson" \
  T06_TCC_PROBE=/tmp/lingxi-t06-bin/tcc_probe T06_REMOTE_URL="http://127.0.0.1:$EV_PORT/remote.html" \
  TAURI_WEBDRIVER_PORT=$WD_PORT \
  nohup "$EVID/build/app-test" > "$EVID/run-test/app-stdout.log" 2>&1 &
APP_PID=$!
( sleep 240; kill -9 $APP_PID 2>/dev/null ) & WATCHDOG=$!
# 对话框自动取消：激活应用（best-effort）+ Cmd+period postToPid（确定性 cancel:，见报告 §7-R1）
zsh "$SPIKE_DIR/scripts/dialog_dismiss.sh" $APP_PID 5 /tmp/lingxi-t06-bin/post_cmd_period \
  > "$EVID/run-test/dialog-dismiss.log" 2>&1 &

wait_reports 3 90; WA=$?
record "runA-page-reports" "3 reports (main,untrusted,remote)" "$(curl -s --noproxy '*' http://127.0.0.1:$EV_PORT/__status)" "$WA"
cp "$SCRATCH/page-reports.jsonl" "$EVID/run-test/page-reports.jsonl" 2>/dev/null

ps -Ao pid,ppid,rss,comm | grep -E "app-test|sidecar|WebKit" > "$EVID/run-test/ps-tree.txt" 2>&1
/tmp/lingxi-t06-bin/post_shortcut > "$EVID/run-test/post-shortcut.log" 2>&1; echo "exit=$?" >> "$EVID/run-test/post-shortcut.log"

node "$SPIKE_DIR/scripts/probe_webdriver.mjs" session-test $WD_PORT "$EVID/run-test/webdriver-evidence.json" \
  > "$EVID/run-test/webdriver-session.log" 2>&1
WD_EC=$?
record "runA-webdriver-session" "exit 0, 3 windows" "$(cat "$EVID/run-test/webdriver-session.log")" "$WD_EC"

# sidecar 崩溃恢复：kill -9 sidecar，验证宿主检测死亡并可重启一次
SC_PID=$(python3 -c "
import json
for l in open('$EVID/run-test/host-report.ndjson'):
    d=json.loads(l)
    if d['event']=='sidecar.spawned': print(d['data']['pid']); break
")
echo "sidecar pid=$SC_PID" > "$EVID/run-test/sidecar-crash.log"
kill -9 $SC_PID && echo "kill -9 sent" >> "$EVID/run-test/sidecar-crash.log"
sleep 2
node "$SPIKE_DIR/scripts/probe_webdriver.mjs" sidecar-recovery $WD_PORT "$EVID/run-test/sidecar-recovery.json" \
  >> "$EVID/run-test/sidecar-crash.log" 2>&1
RC_EC=$?
# repair-r1 (F4)：恢复语义断言——不再只记 probe 退出码。probe 只判"捕获到 drill 输出"，
# restart 真实失败（如 symlink 拒绝）时旧 runner 仍记 exit=0 假绿（验收 §3.1 实测）。
# 这里直接校验 drill 语义：kill 后 alive=false、restart.restarted=true、pong 真实往返且
# seq 递增（>=2）、final restarts=1；任一不满足则该步 exit 非零。
python3 - "$EVID/run-test/sidecar-recovery.json" >> "$EVID/run-test/sidecar-crash.log" 2>&1 <<'PYEOF'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except Exception as e:
    print(f"RECOVERY-ASSERT FAILED: sidecar-recovery.json unreadable: {e}")
    sys.exit(1)
errs = []
sak = d.get('status_after_kill')
if not isinstance(sak, dict) or sak.get('alive') is not False:
    errs.append(f"status_after_kill.alive != false: {sak!r}")
rst = d.get('restart')
if not isinstance(rst, dict) or rst.get('restarted') is not True:
    errs.append(f"restart.restarted != true: {rst!r}")
ping = d.get('ping_after_restart')
if not isinstance(ping, dict) or ping.get('ok') is not True or ping.get('pong') is not True:
    errs.append(f"ping_after_restart not a real pong: {ping!r}")
elif not isinstance(ping.get('seq'), int) or ping['seq'] < 2:
    errs.append(f"pong seq not incremented (expect >=2): {ping.get('seq')!r}")
fs = d.get('final_status')
if not isinstance(fs, dict) or fs.get('alive') is not True or fs.get('restarts') != 1:
    errs.append(f"final_status alive!=true or restarts!=1: {fs!r}")
if errs:
    print('RECOVERY-ASSERT FAILED: ' + '; '.join(errs))
    sys.exit(1)
print('RECOVERY-ASSERT OK: alive=false -> restarted=true -> pong seq>=2 -> restarts=1')
PYEOF
RC_ASSERT_EC=$?
RC_STEP_EC=$RC_EC
[ "$RC_EC" -eq 0 ] && [ "$RC_ASSERT_EC" -ne 0 ] && RC_STEP_EC=$RC_ASSERT_EC
record "runA-sidecar-crash-recovery" "dead detected + restart ok + pong seq>=2 + restarts=1" "$(cat "$EVID/run-test/sidecar-recovery.json" 2>/dev/null | python3 -c 'import json,sys; d=json.load(sys.stdin); print(json.dumps({k:(str(v)[:60]) for k,v in d.items()}))' 2>/dev/null)" "$RC_STEP_EC"

node "$SPIKE_DIR/scripts/probe_webdriver.mjs" finish $WD_PORT >> "$EVID/run-test/webdriver-session.log" 2>&1
wait $APP_PID; APP_EC=$?
kill $WATCHDOG 2>/dev/null
record "runA-app-exit" "exit 0 (graceful finish_e2e)" "exit=$APP_EC" "$APP_EC"
sleep 1
pgrep -f lingxi-t06-sidecar > /dev/null && echo "sidecar STILL RUNNING (bad)" > "$EVID/run-test/sidecar-after-exit.txt" \
  || echo "no sidecar process after app exit (good)" > "$EVID/run-test/sidecar-after-exit.txt"
record "runA-sidecar-cleanup" "no orphan sidecar" "$(cat "$EVID/run-test/sidecar-after-exit.txt")" "0"

# ---------- 5 Run B：release 产物（A12 + 篡改 manifest 负向 + SIGKILL 孤儿检查）----------
cp "$EVID/updates/latest-tampered.json" "$EVID/updates/latest.json"
rm -f "$SCRATCH/page-reports.jsonl"
T06_MODE=e2e T06_SCRATCH="$SCRATCH" T06_HOST_REPORT="$EVID/run-release/host-report.ndjson" \
  T06_TCC_PROBE=/tmp/lingxi-t06-bin/tcc_probe T06_REMOTE_URL="http://127.0.0.1:$EV_PORT/remote.html" \
  TAURI_WEBDRIVER_PORT=$WD_PORT \
  nohup "$EVID/build/app-release" > "$EVID/run-release/app-stdout.log" 2>&1 &
APPB_PID=$!
sleep 3
node "$SPIKE_DIR/scripts/probe_webdriver.mjs" closed-check $WD_PORT > "$EVID/run-release/webdriver-closed.json" 2>&1
CC_EC=$?
record "runB-webdriver-closed" "port closed, exit 0" "$(tail -1 "$EVID/run-release/webdriver-closed.json")" "$CC_EC"
wait_reports 3 90; WB=$?
record "runB-page-reports" "3 reports" "$(curl -s --noproxy '*' http://127.0.0.1:$EV_PORT/__status)" "$WB"
cp "$SCRATCH/page-reports.jsonl" "$EVID/run-release/page-reports.jsonl" 2>/dev/null
# 二进制内容探测：release 不得含 wdio/webdriver 插件痕迹
{ echo "== app-test strings wdio/webdriver count:"; strings "$EVID/build/app-test" | grep -ci "wdio" || true
  echo "== app-release strings wdio/webdriver count:"; strings "$EVID/build/app-release" | grep -ci "wdio" || true
  echo "== app-test webdriver count:"; strings "$EVID/build/app-test" | grep -ci "webdriver" || true
  echo "== app-release webdriver count:"; strings "$EVID/build/app-release" | grep -ci "webdriver" || true
} > "$EVID/run-release/binary-strings-probe.txt" 2>&1
# SIGKILL 宿主 -> sidecar 必须靠 stdin EOF 自行退出
kill -9 $APPB_PID
sleep 3
pgrep -f lingxi-t06-sidecar > /dev/null && echo "sidecar STILL RUNNING after SIGKILL (bad)" > "$EVID/run-release/sidecar-after-sigkill.txt" \
  || echo "no sidecar process after host SIGKILL (good)" > "$EVID/run-release/sidecar-after-sigkill.txt"
record "runB-sigkill-orphan" "no orphan sidecar" "$(cat "$EVID/run-release/sidecar-after-sigkill.txt")" "0"
wait $APPB_PID 2>/dev/null

# ---------- 6 relaunch 探针 ----------
T06_MODE=relaunch-probe T06_SCRATCH="$SCRATCH" T06_HOST_REPORT="$EVID/run-relaunch/host-report.ndjson" \
  nohup "$EVID/build/app-release" > "$EVID/run-relaunch/app-stdout.log" 2>&1 &
RL_PID=$!
for i in $(seq 1 25); do
  grep -q "second_instance_observed" "$EVID/run-relaunch/host-report.ndjson" 2>/dev/null && break
  sleep 1
done
kill $RL_PID 2>/dev/null; pkill -f "app-release" 2>/dev/null
grep -q "second_instance_observed" "$EVID/run-relaunch/host-report.ndjson" \
  && record "relaunch-probe" "second instance observed" "observed" "0" \
  || record "relaunch-probe" "second instance observed" "NOT observed" "1"

# ---------- 7 打包 .app（真实签名链）+ 打包冒烟 ----------
( cd "$SPIKE_DIR/app" && \
  TAURI_SIGNING_PRIVATE_KEY="$(cat $KEY_DIR/t06-updater-test.key)" \
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$KEY_PASS" \
  CARGO_TARGET_DIR=$CARGO_TARGET_DIR_APP ${=UNPROXY} npx tauri build --bundles app ) \
  > "$EVID/bundle/tauri-build.log" 2>&1
BD_EC=$?
record "tauri-build-bundle" "exit 0, .app + signed updater artifacts" "$(tail -3 "$EVID/bundle/tauri-build.log" | tr '\n' ' ')" "$BD_EC"
APP_BUNDLE=$(ls -d $CARGO_TARGET_DIR_APP/release/bundle/macos/*.app 2>/dev/null | head -1)
if [ -n "$APP_BUNDLE" ]; then
  APP_BUNDLE_BIN=$(find "$APP_BUNDLE/Contents/MacOS" -maxdepth 1 -type f ! -name "lingxi-t06-sidecar" | head -1)
  echo "bundle bin: $APP_BUNDLE_BIN" >> "$SUMMARY"
  find $CARGO_TARGET_DIR_APP/release/bundle -name "*.tar.gz*" -o -name "*.sig" 2>/dev/null > "$EVID/bundle/bundle-artifacts.txt"
  shasum -a 256 "$APP_BUNDLE_BIN" >> "$EVID/build/SHASUMS-binaries.txt"
  cp "$EVID/updates/latest.json" "$EVID/updates/latest-tampered-used.json" # 记录 B 轮用的篡改件
  cp "$EVID/updates/latest-good.json" "$EVID/updates/latest.json" # 打包冒烟恢复正常 manifest
  # 打包产物冒烟：sidecar 随包 + autostart（SMAppService 真实注册/注销）。
  # 注意：macOS 上 /tmp 是 /private/tmp 的符号链接，Tauri 拒绝符号链接路径下的 current_exe()
  # （"StartingBinary found current_exe() that contains a symlink"），因此复制到 realpath 目录运行。
  REAL_TMP="$(cd /tmp && pwd -P)"
  BUNDLE_RUN_DIR="$REAL_TMP/lingxi-t06-bundle-run"
  rm -rf "$BUNDLE_RUN_DIR"; mkdir -p "$BUNDLE_RUN_DIR"
  cp -R "$APP_BUNDLE" "$BUNDLE_RUN_DIR/"
  BUNDLE_APP_COPY="$BUNDLE_RUN_DIR/$(basename "$APP_BUNDLE")"
  BUNDLE_RUN_BIN=$(find "$BUNDLE_APP_COPY/Contents/MacOS" -maxdepth 1 -type f ! -name "lingxi-t06-sidecar" | head -1)
  echo "bundle run from realpath: $BUNDLE_RUN_BIN" >> "$SUMMARY"
  rm -f "$SCRATCH/page-reports.jsonl"
  T06_MODE=e2e T06_SCRATCH="$SCRATCH" T06_HOST_REPORT="$EVID/run-bundle/host-report.ndjson" \
    T06_TCC_PROBE=/tmp/lingxi-t06-bin/tcc_probe T06_REMOTE_URL="http://127.0.0.1:$EV_PORT/remote.html" \
    nohup "$BUNDLE_RUN_BIN" > "$EVID/run-bundle/app-stdout.log" 2>&1 &
  BUN_PID=$!
  ( sleep 120; kill -9 $BUN_PID 2>/dev/null ) & BW=$!
  wait_reports 3 90; BB=$?
  record "runBundle-page-reports" "3 reports" "$(curl -s --noproxy '*' http://127.0.0.1:$EV_PORT/__status)" "$BB"
  cp "$SCRATCH/page-reports.jsonl" "$EVID/run-bundle/page-reports.jsonl" 2>/dev/null
  # 打包产物：main 页 45s 兜底 finish；等退出
  for i in $(seq 1 60); do kill -0 $BUN_PID 2>/dev/null || break; sleep 1; done
  kill $BUN_PID 2>/dev/null; kill $BW 2>/dev/null
  sleep 2
  pgrep -f lingxi-t06-sidecar > /dev/null && echo "sidecar STILL RUNNING (bad)" > "$EVID/run-bundle/sidecar-after-exit.txt" \
    || echo "no sidecar process after bundle app exit (good)" > "$EVID/run-bundle/sidecar-after-exit.txt"
fi

kill $EV_PID 2>/dev/null
log "run_e2e complete. summary at $SUMMARY"

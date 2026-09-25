#!/usr/bin/env bash
# R01-T05 一键重放：HTML/PDF 与办公处理原型全部场景。
# 用法：bash scripts/rust-tauri/r01-t05-replay.sh [artifacts_dir]
# 产物：artifacts/rust-tauri/R01/T05/（默认）；临时目录 /tmp/r01t05-replay。
# 红线：全部流量限 loopback（loopback-only 正向代理 + 浏览器 proxy-bypass-list=<-loopback>）；
#       样本零远端引用；旧链 Electron 全部写入路径重定向 /tmp。
set -u
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
ART="${1:-$REPO/artifacts/rust-tauri/R01/T05}"
TMP=/tmp/r01t05-replay
T05=$REPO/tests/migration/r01-t05
PROXY_PORT="${R01_T05_PROXY_PORT:-18482}"
CANARY_PORT="${R01_T05_CANARY_PORT:-18291}"   # 必须与样本内 loopback canary URL 端口一致
SAMPLES=$T05/samples
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/r01t05-replay/cargo-target}"
export PATH="$HOME/.cargo/bin:$PATH"

unset all_proxy ALL_PROXY http_proxy HTTP_PROXY https_proxy HTTPS_PROXY

mkdir -p "$ART" "$TMP"
SUMMARY="$ART/replay-summary.jsonl"
: > "$SUMMARY"

step() { echo "[r01-t05] $*" >&2; }
record() { # name exit expected detail
  python3 - "$1" "$2" "$3" "$4" >> "$SUMMARY" <<'PY'
import json, sys
print(json.dumps({"scenario": sys.argv[1], "exit": int(sys.argv[2]), "expected_exit": sys.argv[3], "detail": sys.argv[4]}, ensure_ascii=False))
PY
}

# ---------- 0. 构建与样本 ----------
step "0 build spike + probe + samples"
(cd "$REPO/rust" && cargo build --locked --offline -p lingxi-browser-spike) || { echo "cargo build FAILED"; exit 1; }
SPIKE="$CARGO_TARGET_DIR/debug/spike_pdf"
swiftc -O -o "$TMP/pdf_probe" "$T05/pdf_probe.swift" || { echo "swiftc FAILED"; exit 1; }
python3 "$T05/generate_samples.py" --check || { echo "sample drift"; exit 1; }
(cd "$REPO" && shasum -a 256 tests/migration/r01-t05/samples/s*.html tests/migration/r01-t05/samples/assets/* tests/migration/r01-t05/decoy/* > "$ART/samples-sha256.txt")

# ---------- 1. loopback 代理 + canary ----------
node "$REPO/tests/migration/r01-t04/proxy.mjs" "$PROXY_PORT" > "$ART/proxy.log" 2>&1 &
PROXY_PID=$!
: > "$TMP/canary.log"
node "$T05/canary_server.mjs" "$CANARY_PORT" "$TMP/canary.log" > "$ART/canary-server.log" 2>&1 &
CANARY_PID=$!
sleep 1
kill -0 $PROXY_PID && kill -0 $CANARY_PID || { echo "proxy/canary start FAILED"; exit 1; }
cleanup() { kill $PROXY_PID $CANARY_PID 2>/dev/null; }
trap cleanup EXIT
# 代理自验：loopback 放行、非 loopback 拒绝
curl -s -o /dev/null -w "%{http_code}" -x "http://127.0.0.1:$PROXY_PORT" "http://127.0.0.1:$CANARY_PORT/selftest" > "$TMP/proxy-selftest.txt"
echo -n " " >> "$TMP/proxy-selftest.txt"
curl -s -o /dev/null -w "%{http_code}" -x "http://127.0.0.1:$PROXY_PORT" "http://example.com/" >> "$TMP/proxy-selftest.txt" || true
cp "$TMP/proxy-selftest.txt" "$ART/proxy-selftest.txt"

# ---------- 场景助手 ----------
mkjob() { # out_json html output [extra-json]
  python3 - "$1" "$2" "$3" "$4" <<'PY'
import json, sys
job = {"htmlPath": sys.argv[2], "outputPath": sys.argv[3],
       "viewport": {"width": 1280, "height": 900},
       "printBackground": True, "preferCSSPageSize": True, "pageSize": "A4",
       "allowJavaScript": False, "embedLingxiFonts": True, "settleMs": 250, "timeoutMs": 60000}
if len(sys.argv) > 4 and sys.argv[4] != "-":
    job.update(json.loads(sys.argv[4]))
json.dump(job, open(sys.argv[1], "w"), ensure_ascii=False, indent=1)
PY
}

run_old() { # name job
  local name="$1"
  local job="$2"
  local dir="$ART/$name/old"
  mkdir -p "$dir"
  R01_T05_PROXY="127.0.0.1:$PROXY_PORT" R01_T05_OLD_TMP="$TMP/old-tmp-$name" \
    node "$T05/run_old_chain.mjs" --job "$job" --log "$dir/run.log" > "$dir/run-result.json" 2>"$dir/run.stderr"
  echo $?
}

run_new() { # name job
  local name="$1"
  local job="$2"
  local dir="$ART/$name/new"
  mkdir -p "$dir"
  "$SPIKE" run --job "$job" --repo "$REPO" --evidence "$dir" \
    --profile-root "$TMP/profiles-$name" --proxy "127.0.0.1:$PROXY_PORT" \
    > "$dir/run-result.json" 2>"$dir/run.stderr"
  echo $?
}

probe() { # name side pdf
  mkdir -p "$ART/$1/$2/probe"
  "$TMP/pdf_probe" "$3" "$ART/$1/$2/probe" 2 > "$ART/$1/$2/probe/stdout.json" 2>&1
}

# ---------- A09: 中文长文档（S1）双链 ----------
step "A09 S1 dual-chain"
mkdir -p "$ART/a09"
mkjob "$TMP/job-a09-old.json" "$SAMPLES/s1-long-zh.html" "$ART/a09/old/s1-long-zh.pdf" -
mkjob "$TMP/job-a09-new.json" "$SAMPLES/s1-long-zh.html" "$ART/a09/new/s1-long-zh.pdf" -
ec=$(run_old a09 "$TMP/job-a09-old.json"); record a09-old "$ec" 0 "S1 旧链"
ec=$(run_new a09 "$TMP/job-a09-new.json"); record a09-new "$ec" 0 "S1 候选链"
probe a09 old "$ART/a09/old/s1-long-zh.pdf"
probe a09 new "$ART/a09/new/s1-long-zh.pdf"
cat > "$ART/a09/expect.json" <<'EOF'
{"required_tokens": ["龘","靐","麤","爨","驫","鬱","齉","爩","龗","灪","𠮷",
  "ROW-001-END","ROW-070-END","ROW-140-END","DOC-END-OMEGA","求根公式","欧拉恒等式",
  "render","print_to_pdf","跨页长表格"],
 "required_tokens_new_only": ["𪚥"],
 "expected_table_rows": 140,
 "expect_equal_page_count": true}
EOF
python3 "$T05/compare_outputs.py" --old-probe "$ART/a09/old/probe" --new-probe "$ART/a09/new/probe" \
  --old-pdf "$ART/a09/old/s1-long-zh.pdf" --new-pdf "$ART/a09/new/s1-long-zh.pdf" \
  --expect "$ART/a09/expect.json" --out "$ART/a09/compare.json"
record a09-compare $? 0 "A09 语义+视觉断言"

# ---------- S2: @page 语义（preferCSSPageSize true/false × 双链） ----------
step "S2 @page semantics"
for pref in true false; do
  for chain in old new; do
    name="s2-pref$pref"
    mkdir -p "$ART/$name"
    out="$ART/$name/$chain/s2.pdf"
    mkjob "$TMP/job-$name-$chain.json" "$SAMPLES/s2-page-semantics.html" "$out" "{\"preferCSSPageSize\": $pref}"
    if [ "$chain" = old ]; then ec=$(run_old "$name" "$TMP/job-$name-$chain.json"); else ec=$(run_new "$name" "$TMP/job-$name-$chain.json"); fi
    record "$name-$chain" "$ec" 0 "S2 preferCSSPageSize=$pref $chain"
    probe "$name" "$chain" "$out"
  done
done

# ---------- S3: 纸张/页边距/方向 ----------
step "S3 paper/margins"
for chain in old new; do
  name="s3"
  mkdir -p "$ART/$name"
  out="$ART/$name/$chain/s3.pdf"
  # margins 数值 = inch（Electron 42.8.1 实测语义，见 PDF_SPIKE_REPORT §3 margins 考据）
  mkjob "$TMP/job-s3-$chain.json" "$SAMPLES/s3-margins-paper.html" "$out" \
    '{"pageSize":"Letter","landscape":true,"margins":{"marginType":"custom","top":1,"bottom":1,"left":1.5,"right":1.5}}'
  if [ "$chain" = old ]; then ec=$(run_old "$name" "$TMP/job-s3-$chain.json"); else ec=$(run_new "$name" "$TMP/job-s3-$chain.json"); fi
  record "s3-$chain" "$ec" 0 "S3 Letter landscape margins $chain"
  probe "$name" "$chain" "$out"
done

# ---------- S4: JS 开关 ----------
step "S4 js toggle"
for js in false true; do
  for chain in old new; do
    name="s4-js$js"
    mkdir -p "$ART/$name"
    out="$ART/$name/$chain/s4.pdf"
    mkjob "$TMP/job-$name-$chain.json" "$SAMPLES/s4-js-gated.html" "$out" "{\"allowJavaScript\": $js}"
    if [ "$chain" = old ]; then ec=$(run_old "$name" "$TMP/job-$name-$chain.json"); else ec=$(run_new "$name" "$TMP/job-$name-$chain.json"); fi
    record "$name-$chain" "$ec" 0 "S4 allowJavaScript=$js $chain"
    probe "$name" "$chain" "$out"
  done
done

# ---------- 字体负对照：embedLingxiFonts=false ----------
step "font negative control"
mkdir -p "$ART/font-negative"
mkjob "$TMP/job-fontneg.json" "$SAMPLES/s1-long-zh.html" "$ART/font-negative/new/s1-nofont.pdf" '{"embedLingxiFonts": false}'
ec=$(run_new font-negative "$TMP/job-fontneg.json"); record font-negative-new "$ec" 0 "候选链不注入字体"
probe font-negative new "$ART/font-negative/new/s1-nofont.pdf"

# ---------- A10: 危险资源（S5） ----------
step "A10 dangerous resources"
mkdir -p "$ART/a10-dangerous"
mkjob "$TMP/job-a10-new.json" "$SAMPLES/s5-dangerous.html" "$ART/a10-dangerous/new/s5.pdf" '{"allowJavaScript": true, "settleMs": 500}'
ec=$(run_new a10-dangerous "$TMP/job-a10-new.json"); record a10-dangerous-new "$ec" 0 "候选链危险样本（拦截后仍应产出）"
probe a10-dangerous new "$ART/a10-dangerous/new/s5.pdf" 2>/dev/null
# 旧链对照使用 decoy 变体：泄漏证据不含真实系统文件内容（真实 /etc/passwd 泄漏已人工确认一次，
# 见 PDF_SPIKE_REPORT §A10；提交的证据链一律用合成诱饵）。
mkjob "$TMP/job-a10-old.json" "$SAMPLES/s5-dangerous-decoy.html" "$ART/a10-dangerous/old/s5-decoy.pdf" '{"allowJavaScript": true, "settleMs": 500}'
ec=$(run_old a10-dangerous "$TMP/job-a10-old.json"); record a10-dangerous-old "$ec" 0 "旧链危险样本decoy变体（参考行为）"
probe a10-dangerous old "$ART/a10-dangerous/old/s5-decoy.pdf" 2>/dev/null
cp "$TMP/canary.log" "$ART/a10-dangerous/canary.log"

# ---------- A10: 无限脚本（S6）超时 ----------
step "A10 infinite script timeout"
mkdir -p "$ART/a10-infinite"
mkjob "$TMP/job-a10i-new.json" "$SAMPLES/s6-infinite-script.html" "$ART/a10-infinite/new/s6.pdf" '{"allowJavaScript": true, "timeoutMs": 5000}'
ec=$(run_new a10-infinite "$TMP/job-a10i-new.json"); record a10-infinite-new "$ec" 2 "候选链无限脚本必须超时退出且无产物"
mkjob "$TMP/job-a10i-old.json" "$SAMPLES/s6-infinite-script.html" "$ART/a10-infinite/old/s6.pdf" '{"allowJavaScript": true, "timeoutMs": 5000}'
ec=$(run_old a10-infinite "$TMP/job-a10i-old.json"); record a10-infinite-old "$ec" "非0" "旧链无限脚本（参考行为）"
# 产物存在性检查
for c in new old; do
  if [ -e "$ART/a10-infinite/$c/s6.pdf" ]; then echo "FALSE-SUCCESS-ARTIFACT $c" >> "$ART/a10-infinite/artifact-check.txt"; else echo "no-artifact $c" >> "$ART/a10-infinite/artifact-check.txt"; fi
done
# JS 关闭时同文件应正常渲染（死脚本惰性化）
mkdir -p "$ART/a10-infinite-jsoff"
mkjob "$TMP/job-a10i-jsoff.json" "$SAMPLES/s6-infinite-script.html" "$ART/a10-infinite-jsoff/new/s6.pdf" '{"allowJavaScript": false, "timeoutMs": 15000}'
ec=$(run_new a10-infinite-jsoff "$TMP/job-a10i-jsoff.json"); record a10-infinite-jsoff-new "$ec" 0 "JS 关闭时无限脚本样本应正常产出"

# ---------- 性能：候选链 16× + 旧链 8×（S1 直测口径） ----------
step "perf runs"
mkdir -p "$ART/perf"
for i in $(seq 1 16); do
  mkjob "$TMP/job-perf-new-$i.json" "$SAMPLES/s1-long-zh.html" "$TMP/perf-new-$i.pdf" -
  t0=$(python3 -c 'import time; print(time.time())')
  "$SPIKE" run --job "$TMP/job-perf-new-$i.json" --repo "$REPO" --evidence "$TMP/perf-ev-new-$i" \
    --profile-root "$TMP/perf-prof" --proxy "127.0.0.1:$PROXY_PORT" > "$TMP/perf-new-$i.json" 2>/dev/null
  ec=$?
  t1=$(python3 -c 'import time; print(time.time())')
  python3 - "$TMP/perf-new-$i.json" "$t0" "$t1" "$ec" >> "$ART/perf/candidate-runs.jsonl" <<'PY'
import json, sys
r = json.load(open(sys.argv[1]))
print(json.dumps({"exit": int(sys.argv[4]), "wall_ms": round((float(sys.argv[3])-float(sys.argv[2]))*1000, 1),
                  "print_ms": (r.get("timings") or {}).get("print_ms"), "pdf_bytes": (r.get("outcome") or {}).get("pdf_bytes")}))
PY
done
for i in $(seq 1 8); do
  mkjob "$TMP/job-perf-old-$i.json" "$SAMPLES/s1-long-zh.html" "$TMP/perf-old-$i.pdf" -
  node "$T05/run_old_chain.mjs" --job "$TMP/job-perf-old-$i.json" --log "$TMP/perf-old-$i.log" > "$TMP/perf-old-$i.json" 2>/dev/null
  ec=$?
  python3 - "$TMP/perf-old-$i.json" "$ec" >> "$ART/perf/old-runs.jsonl" <<'PY'
import json, sys
r = json.load(open(sys.argv[1]))
print(json.dumps({"exit": int(sys.argv[2]), "wall_ms": r["wall_ms"], "pdf_bytes": r["output_size"]}))
PY
done
python3 - "$ART/perf" "$SAMPLES/s1-long-zh.html" <<'PY'
import json, os, statistics, sys
d = sys.argv[1]
def stats(path, key):
    rows = [json.loads(l) for l in open(path)]
    xs = sorted(r[key] for r in rows if r["exit"] == 0)
    n = len(xs)
    return {"n": n, "median_ms": xs[n//2] if n % 2 else (xs[n//2-1]+xs[n//2])/2,
            "p95_ms": xs[min(n-1, int(0.95*(n-1)+0.5))], "min_ms": xs[0], "max_ms": xs[-1],
            "all_exit0": n == len(rows)}
out = {"candidate_spawn_to_exit": stats(f"{d}/candidate-runs.jsonl", "wall_ms"),
       "candidate_printToPDF_only": stats(f"{d}/candidate-runs.jsonl", "print_ms"),
       "old_spawn_to_exit": stats(f"{d}/old-runs.jsonl", "wall_ms"),
       "sample_bytes": os.path.getsize(sys.argv[2]),
       "r00_t06_baseline": {"helper_direct_median_ms": 812.1, "helper_direct_p95_ms": 835.6,
                            "product_chain_median_ms": 851.8, "product_chain_p95_ms": 879.4,
                            "sample_bytes": 239073}}
json.dump(out, open(f"{d}/perf-summary.json", "w"), ensure_ascii=False, indent=1)
print(json.dumps(out, ensure_ascii=False))
PY

step "done; summary at $SUMMARY"
cat "$SUMMARY"

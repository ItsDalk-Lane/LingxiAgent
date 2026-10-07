#!/usr/bin/env bash
# R05-T08 — the Appendix-C NEGATIVE gate (16 fault injections, R05-GATE-N01…N16).
#
# Proves on an ISOLATED copy of the repo under $HOME (never by tampering the
# production gate in place; the main working tree keeps ZERO injections)
# that the R05 registration is not an empty shell: every fake-green shape
# fails CLOSED with the gap NAMED. Per case: a same-environment CONTROL run
# (the untouched check is green), then ONE targeted mutation, then the judged
# check must fail for the REASON the case exists (never an unrelated compile
# error — each mutation compiles and only changes the behavior under test).
#
# The copy is created WITHOUT any git write operation on the main repo:
# `git archive HEAD` (read-only export) + an rsync overlay of the uncommitted
# working-tree state, plus a `git init`-free metadata shim — verify-stage
# needs git HEAD, so the copy clones the local repo read-only via
# `git clone --no-hardlinks` (a pure read of the source) before the overlay.
#
# Usage: scripts/rust-tauri/r05_t08_negative_gate.sh [EVIDENCE_DIR] [--case N03]
#        every case's real exit code and gap-naming text land under
#        EVIDENCE_DIR; the script exits 0 only if EVERY case exited non-zero
#        AND the expected gap text was found AND its control was green.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
EV_ARG="${1:-$ROOT/artifacts/rust-tauri/R05/T08-E01/negative}"
# 单项复验只登记实际运行的 N03，不把其他十五项算作本轮通过。
CASE_SCOPE=ALL
if [ "$#" -gt 1 ]; then
  if [ "$#" -ne 3 ] || [ "$2" != "--case" ] || [ "$3" != "N03" ]; then
    echo "FAIL: usage: $0 [EVIDENCE_DIR] [--case N03]" >&2
    exit 1
  fi
  CASE_SCOPE=N03
fi
case "$EV_ARG" in
  /*) EV="$EV_ARG" ;;
  *)  EV="$ROOT/$EV_ARG" ;;
esac
mkdir -p "$EV"
: > "$EV/summary.txt"
: > "$EV/case-results.tsv"

note() { printf '%s\n' "$*" | tee -a "$EV/summary.txt"; }
fail() { echo "FAIL: $*" >&2; exit 1; }
CARGO="$HOME/.cargo/bin/cargo"
[ -x "$CARGO" ] || fail "~/.cargo/bin/cargo not found"
export CARGO_NET_OFFLINE=true
NEG_TARGET="$HOME/.cache/lingxi-r05-neg-target"
export CARGO_TARGET_DIR="$NEG_TARGET"
TOOLCHAIN="$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$ROOT/rust-toolchain.toml" | head -n 1)"
[ -n "$TOOLCHAIN" ] || fail "cannot parse toolchain channel"

# ── the isolated copy: local clone (reads the source repo only) + overlay ───
mkdir -p "$HOME/r05t08-work"
# 每次建立新副本，避免覆盖历史证据或另一个正在运行的注入副本。
COPY="$(mktemp -d "$HOME/r05t08-work/negcopy.XXXXXX")" || fail "cannot create isolated copy"
SOURCE_HEAD="$(git -C "$ROOT" rev-parse --short HEAD)" || fail "source HEAD query failed"
note "== preparing the isolated copy at $COPY (local clone of HEAD $SOURCE_HEAD + uncommitted working-tree overlay) =="
rm -rf "$COPY"
if ! git -C "$ROOT" clone --no-hardlinks --quiet --branch codex/rust-tauri-migration "$ROOT" "$COPY" > "$EV/clone.log" 2>&1; then
  cat "$EV/clone.log"; fail "local clone failed"
fi
# Overlay the uncommitted candidate: rust/, scripts/, docs/rust-tauri/
# (the clone already carries the committed R00–R04 docs; rsync --delete makes
# the overlaid subtrees byte-equal to the main working tree). rust/target is
# excluded — it is a gitignored build cache, not candidate source, and the
# copy builds into its own NEG_TARGET cache anyway.
for sub in rust scripts docs/rust-tauri; do
  rsync -a --delete --exclude 'target/' "$ROOT/$sub/" "$COPY/$sub/" || fail "rsync overlay failed for $sub"
done
# The copy must NOT carry main-tree artifacts (evidence trees stay behind);
# its candidate binding covers its own tree only.
COPY_BRANCH="$(git -C "$COPY" rev-parse --abbrev-ref HEAD)" || fail "copy branch query failed"
COPY_HEAD="$(git -C "$COPY" rev-parse --short HEAD)" || fail "copy HEAD query failed"
COPY_STATUS="$(git -C "$COPY" status --porcelain)" || fail "copy status query failed"
COPY_DIRTY_COUNT="$(printf '%s\n' "$COPY_STATUS" | sed '/^$/d' | wc -l | tr -d ' ')" || fail "copy dirty count failed"
note "== copy ready (branch $COPY_BRANCH, HEAD $COPY_HEAD, worktree dirty=$COPY_DIRTY_COUNT files) =="

# Pristine copies of every file this harness mutates (reset between cases).
PRISTINE="$EV/pristine"
mkdir -p "$PRISTINE"
snapshot_pristine() {
  # 快照只拍一次，不能把变异或漂移重新当作合法基线。
  [ ! -e "$PRISTINE/$1" ] || fail "pristine snapshot already exists: $1"
  mkdir -p "$PRISTINE/$(dirname "$1")" || fail "pristine directory failed: $1"
  cp "$COPY/$1" "$PRISTINE/$1" || fail "pristine copy failed: $1"
  cmp "$COPY/$1" "$PRISTINE/$1" || fail "pristine bytes differ: $1"
}
# 快照和恢复消费同一份清单，避免新增变异目标只登记到一侧。
MUTATED_FILES=(
  rust/crates/xtask/src/stage_maps/R05.json
  rust/crates/xtask/src/stage_maps/R04.json
  rust/crates/lingxi-service/src/lib.rs
  rust/crates/lingxi-kernel/src/lib.rs
  rust/crates/lingxi-adapters/src/models/tool_render.rs
  rust/crates/lingxi-adapters/src/models/openai_completions.rs
  rust/crates/lingxi-adapters/src/models/credentials.rs
  rust/crates/lingxi-service/src/runs.rs
  rust/crates/lingxi-service/src/credentials/mod.rs
  rust/crates/lingxi-service/tests/r05_t01_binary_wiring.rs
  docs/rust-tauri/R05/r05_stage_pins.tsv
  scripts/rust-tauri/r05_t08_stage_suites.sh
)
for f in "${MUTATED_FILES[@]}"; do
  snapshot_pristine "$f"
done
reset_copy() {
  local f
  for f in "${MUTATED_FILES[@]}"; do
    cp "$PRISTINE/$f" "$COPY/$f" || fail "restore copy failed: $f"
    cmp "$PRISTINE/$f" "$COPY/$f" || fail "restore bytes differ: $f"
  done
}

n06_start_signal() {
  # 使用现行场景首命令的权威 argv，匹配 verify.rs 已有的启动输出。
  python3 - "$COPY/rust/crates/xtask/src/stage_maps/R02.json" "$1" <<'PY'
import json, pathlib, sys
stage = json.loads(pathlib.Path(sys.argv[1]).read_text())
key = stage["scenarios"][0]["commandRefs"][0]
argv = stage["commands"][key]["argv"]
print(f'xtask: verify-stage {stage["stage"]} [{" ".join(argv)}] > {pathlib.Path(sys.argv[2]) / key}')
PY
}

wait_for_n06_start() {
  local pid="$1" log="$2" signal="$3" limit="${4:-240}" matched i
  for ((i=0; i<limit; i++)); do
    n06_gate_running "$pid" || return 1
    grep -Fx -- "$signal" "$log" >/dev/null
    matched=$?
    if [ "$matched" -eq 0 ]; then
      n06_gate_running "$pid" || return 1
      return 0
    fi
    [ "$matched" -eq 1 ] || { echo "N06 prerequisite failed: start log query failed" >&2; return 1; }
    sleep 1
  done
  echo "N06 prerequisite failed: authoritative command start not observed" >&2
  return 1
}

n06_gate_running() {
  # 未回收的已退出进程仍可能响应 kill -0，因此同时拒绝僵尸状态。
  local state status
  state="$(ps -p "$1" -o stat=)"
  status=$?
  if [ "$status" -gt 1 ]; then
    echo "N06 prerequisite failed: process query failed" >&2
    return 1
  fi
  case "$state" in
    ""|*Z*) echo "N06 prerequisite failed: gate exited" >&2; return 1 ;;
  esac
  [ "$status" -eq 0 ] && kill -0 "$1" 2>/dev/null || { echo "N06 prerequisite failed: gate exited" >&2; return 1; }
}

record_case() { # name exit_code named verdict note
  note "case $1: exit=$2 gap-named=$3 verdict=$4 ($5)"
  printf '%s\t%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" "$5" >> "$EV/case-results.tsv"
}

cargo_in_copy() { # args...
  (cd "$COPY" && "$CARGO" "$@" )
}
xtask_test() { # name filter [pattern...]
  local name="$1"; shift
  local filter="$1"; shift
  local dir="$EV/$name"
  mkdir -p "$dir"
  cargo_in_copy test --manifest-path rust/Cargo.toml --locked -p xtask --bin xtask "$filter" \
    > "$dir/test.stdout.log" 2>&1
  local exit_code=$?
  echo "$exit_code" > "$dir/exit-code.txt"
  local named=OK
  local pattern
  for pattern in "$@"; do
    if ! grep -qF -- "$pattern" "$dir/test.stdout.log"; then named="MISSING:$pattern"; break; fi
  done
  local verdict=BAD
  if [ "$exit_code" -ne 0 ] && [ "$named" = "OK" ]; then verdict=OK; fi
  RECORD_NAME="$name"; RECORD_EXIT="$exit_code"; RECORD_NAMED="$named"; RECORD_VERDICT="$verdict"
}

run_n03() {
  reset_copy
  local dir="$EV/n03-tamper-pin-count"
  mkdir -p "$dir"
  python3 "$COPY/scripts/rust-tauri/r05_t08_mutate_pin.py" \
    "$COPY/docs/rust-tauri/R05/r05_stage_pins.tsv" "$dir/mutation.json" \
    > "$dir/mutation.log" 2>&1
  local mutation_exit=$?
  echo "$mutation_exit" > "$dir/mutation-exit-code.txt"
  [ "$mutation_exit" -eq 0 ] || { cat "$dir/mutation.log"; fail "N03 injection refused: unique authoritative anchor required"; }
  note "== N03: exactly one authoritative pin count lowered; old/new saved in mutation.json =="
  xtask_test n03-tamper-pin-count r05_stage_pin_table_matches \
    "svc:r05_t01_model_plane" "drifted" \
    "test stage_map::map_tests::r05_stage_pin_table_matches_the_registered_suites ... FAILED" \
    "0 passed; 1 failed; 0 ignored"
  record_case "R05-GATE-N03" "$RECORD_EXIT" "$RECORD_NAMED" "$RECORD_VERDICT" "权威表唯一目标降钉恰一次 → 镜像单测红点名漂移"
  reset_copy
  mkdir -p "$EV/n03-restored"
  cmp "$COPY/docs/rust-tauri/R05/r05_stage_pins.tsv" \
    "$PRISTINE/docs/rust-tauri/R05/r05_stage_pins.tsv" || fail "N03 restore differs"
  cargo_in_copy test --manifest-path rust/Cargo.toml --locked -p xtask --bin xtask \
    stage_map::map_tests::r05_stage_pin_table_matches_the_registered_suites -- --exact \
    > "$EV/n03-restored/test.stdout.log" 2>&1
  local restored_exit=$?
  echo "$restored_exit" > "$EV/n03-restored/exit-code.txt"
  [ "$restored_exit" -eq 0 ] && \
    grep -qF "1 passed; 0 failed; 0 ignored" "$EV/n03-restored/test.stdout.log" \
    || fail "N03 restored mirror was not run or not green"
  note "PASS N03 restore: authoritative table byte-equal, exact mirror 1/1 green"
}

write_results() {
# ── verdict ─────────────────────────────────────────────────────────────────
python3 - "$EV" "$CASE_SCOPE" "$COPY" <<'PY' || fail "at least one R05 negative case did NOT fail closed with the gap named"
import json, pathlib, sys
ev = pathlib.Path(sys.argv[1])
scope = sys.argv[2]
expected = ["R05-GATE-N03"] if scope == "N03" else [f"R05-GATE-N{i:02d}" for i in range(1, 17)]
rows = []
for line in (ev / "case-results.tsv").read_text().splitlines():
    if not line.strip():
        continue
    name, exit_code, named, verdict, note = line.split("\t")
    rows.append({"case": name, "exitCode": int(exit_code), "gapNamed": named,
                 "verdict": verdict, "note": note})
controls = [{"name": "xtask-r05-mirror", "exitCode": int((ev / "control-xtask" / "exit-code.txt").read_text()), "expected": 0},
            {"name": "n03-restored-mirror", "exitCode": int((ev / "n03-restored" / "exit-code.txt").read_text()), "expected": 0}]
if scope == "ALL":
    controls.append({"name": "binary-wiring-suite", "exitCode": int((ev / "control-binwiring" / "exit-code.txt").read_text()), "expected": 0})
doc = {
    "schema": "lingxi.r05-negative-gate.v1",
    "isolatedCopy": "local git clone (read-only on the source) + uncommitted working-tree overlay under $HOME; the main working tree carries ZERO injections (reset_copy restores every mutated file from the pristine snapshot)",
    "scope": scope,
    "isolatedCopyPath": sys.argv[3],
    "expectedCases": expected,
    "unexecutedCases": [f"R05-GATE-N{i:02d}" for i in range(1, 17) if f"R05-GATE-N{i:02d}" not in expected],
    "controls": controls,
    "cases": rows,
    "allRefused": [r["case"] for r in rows] == expected and all(r["verdict"] == "OK" for r in rows),
    "controlsGreen": all(c["exitCode"] == c["expected"] for c in controls),
}
(ev / "case-results.json").write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n")
print(json.dumps({"cases": len(rows), "allRefused": doc["allRefused"], "controlsGreen": doc["controlsGreen"]}))
if not (doc["allRefused"] and doc["controlsGreen"]):
    raise SystemExit(1)
PY
}

# ── controls: the untouched checks are green in the SAME copy ───────────────
note "== CONTROL: xtask r05 mirror tests green in the copy =="
mkdir -p "$EV/control-xtask"
cargo_in_copy test --manifest-path rust/Cargo.toml --locked -p xtask --bin xtask r05_ \
  > "$EV/control-xtask/test.stdout.log" 2>&1
CONTROL_XTASK_EXIT=$?
echo "$CONTROL_XTASK_EXIT" > "$EV/control-xtask/exit-code.txt"
[ "$CONTROL_XTASK_EXIT" -eq 0 ] || { cat "$EV/control-xtask/test.stdout.log"; fail "CONTROL failed: the untouched r05 mirror tests are not green in the copy"; }
note "PASS control: xtask r05_ tests green (exit 0)"

if [ "$CASE_SCOPE" = "N03" ]; then
  run_n03
  write_results
  note "RESULT: selected N03 failed closed with the target named (1/1), restored mirror green; other 15 cases NOT RUN"
  exit 0
fi

note "== CONTROL: the binary-wiring suite is green in the copy (for N08/N10) =="
mkdir -p "$EV/control-binwiring"
cargo_in_copy test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t01_binary_wiring \
  > "$EV/control-binwiring/test.stdout.log" 2>&1
CONTROL_BW_EXIT=$?
echo "$CONTROL_BW_EXIT" > "$EV/control-binwiring/exit-code.txt"
[ "$CONTROL_BW_EXIT" -eq 0 ] || { cat "$EV/control-binwiring/test.stdout.log"; fail "CONTROL failed: r05_t01_binary_wiring is not green in the copy"; }
note "PASS control: r05_t01_binary_wiring green (exit 0)"

# ── N01: delete the R05-A16 scenario → the map-pinning tests go red ────────
reset_copy
python3 - "$COPY" <<'PY'
import json, sys
p = sys.argv[1] + "/rust/crates/xtask/src/stage_maps/R05.json"
m = json.load(open(p))
m["scenarios"] = [s for s in m["scenarios"] if s["id"] != "R05-A16"]
json.dump(m, open(p, "w"), indent=2, ensure_ascii=False)
PY
note "== N01: deleted the R05-A16 scenario from the copy's map; xtask map-pinning tests: =="
xtask_test n01-delete-a16 r05_production_map_keeps_the_sixteen "R05-A16"
record_case "R05-GATE-N01" "$RECORD_EXIT" "$RECORD_NAMED" "$RECORD_VERDICT" "删 A16 → 镜像单测红并点名 R05-A16"

# ── N02: a filter that matches zero tests must fail even though cargo 0 ────
reset_copy
sed -i '' 's|^pin lib-adapters/models::streaming::tests::the_buffer_bound_is_loud 1 |pin lib-adapters/models::streaming::tests::the_buffer_bound_is_loud_XYZ 1 |' \
  "$COPY/docs/rust-tauri/R05/r05_stage_pins.tsv"
note "== N02: pin path mutated to match zero tests; the producer must fail naming the 0-match run (full producer run) =="
N02_DIR="$EV/n02-zero-match"
mkdir -p "$N02_DIR"
(cd "$COPY" && bash scripts/rust-tauri/r05_t08_stage_suites.sh "$N02_DIR/suites" > "$N02_DIR/run.log" 2>&1)
N02_EXIT=$?
echo "$N02_EXIT" > "$N02_DIR/exit-code.txt"
N02_NAMED=OK
grep -qF "filter matched 0 tests" "$N02_DIR/run.log" || grep -qF "filter matched 0 tests" "$N02_DIR/suites/gaps.txt" || N02_NAMED="MISSING:filter matched 0 tests"
N02_VERDICT=BAD; if [ "$N02_EXIT" -ne 0 ] && [ "$N02_NAMED" = "OK" ]; then N02_VERDICT=OK; fi
record_case "R05-GATE-N02" "$N02_EXIT" "$N02_NAMED" "$N02_VERDICT" "零匹配筛选词 → 生产者非零并点名 running=0"

run_n03

# ── N04: a swallowed exit code must not green a failing run ────────────────
reset_copy
# (a) make ONE pinned lib test actually fail (a real assertion flip);
sed -i '' 's|assert!(text.contains("main"));|assert!(false, "N04 injected failure");|' \
  "$COPY/rust/crates/lingxi-service/src/credentials/mod.rs"
# (b) swallow the cargo exit code in the producer (RUN_EXIT hardcoded 0).
sed -i '' 's|^  RUN_EXIT=$?$|  RUN_EXIT=0 # N04 mutation: exit swallowed|' \
  "$COPY/scripts/rust-tauri/r05_t08_stage_suites.sh"
note "== N04: one lib test fails AND the producer's exit capture is neutralized; the producer must still fail via the parsed-count check =="
N04_DIR="$EV/n04-swallowed-exit"
mkdir -p "$N04_DIR"
(cd "$COPY" && bash scripts/rust-tauri/r05_t08_stage_suites.sh "$N04_DIR/suites" > "$N04_DIR/run.log" 2>&1)
N04_EXIT=$?
echo "$N04_EXIT" > "$N04_DIR/exit-code.txt"
N04_NAMED=OK
grep -qF "handle_refusal_texts_carry_no_material" "$N04_DIR/run.log" || grep -qF "handle_refusal_texts_carry_no_material" "$N04_DIR/suites/cid-gaps.txt" || N04_NAMED="MISSING:failing-test-named"
grep -qE "passed [0-9]+ but the stage pin is 1" "$N04_DIR/run.log" || grep -qE "passed [0-9]+ but the stage pin is 1" "$N04_DIR/suites/gaps.txt" || N04_NAMED="MISSING:count-check"
N04_VERDICT=BAD; if [ "$N04_EXIT" -ne 0 ] && [ "$N04_NAMED" = "OK" ]; then N04_VERDICT=OK; fi
record_case "R05-GATE-N04" "$N04_EXIT" "$N04_NAMED" "$N04_VERDICT" "吞退出码 → 计数核对仍非零并点名失败测试"

# ── N05: a non-empty evidence root must be refused (stale-evidence reuse) ──
reset_copy
N05_DIR="$EV/n05-stale-evidence"
mkdir -p "$N05_DIR/evidence"
printf 'foreign stale content from another run\n' > "$N05_DIR/evidence/stale.json"
note "== N05: pre-populated evidence root; verify-stage R05 must refuse =="
(cd "$COPY" && "$CARGO" run --manifest-path rust/Cargo.toml --locked -p xtask -- \
   verify-stage R05 --evidence "$N05_DIR/evidence" > "$N05_DIR/run.log" 2>&1)
N05_EXIT=$?
echo "$N05_EXIT" > "$N05_DIR/exit-code.txt"
N05_NAMED=OK
grep -qF "is not empty" "$N05_DIR/run.log" || N05_NAMED="MISSING:not-empty"
N05_VERDICT=BAD; if [ "$N05_EXIT" -ne 0 ] && [ "$N05_NAMED" = "OK" ]; then N05_VERDICT=OK; fi
record_case "R05-GATE-N05" "$N05_EXIT" "$N05_NAMED" "$N05_VERDICT" "旧证据目录 → 拒收并点名 not empty"

# ── N06: source changed MID-gate → the candidate binding goes STALE ────────
# Demonstrated on the registered R02 gate (the binding/checkpoint machinery
# in candidate.rs/main.rs is stage-shared; the R05 run embeds the same
# mechanism — see N16's digest pair). Scope noted in the report.
reset_copy
N06_DIR="$EV/n06-midgate-mutation"
mkdir -p "$N06_DIR"
note "== N06: verify-stage R02 launched; a source file is edited mid-run; the result must be STALE/FAIL =="
N06_SIGNAL="$(n06_start_signal "$N06_DIR/evidence")" || fail "N06 authority query failed"
printf '%s\n' "$N06_SIGNAL" > "$N06_DIR/expected-start.txt" || fail "N06 signal receipt failed"
: > "$N06_DIR/run.log" || fail "N06 log creation failed"
(cd "$COPY" && "$CARGO" run --manifest-path rust/Cargo.toml --locked -p xtask -- \
   verify-stage R02 --evidence "$N06_DIR/evidence" > "$N06_DIR/run.log" 2>&1) &
N06_PID=$!
# 启动行在初始快照及 runner 校验之后输出；仅目录出现不能证明此前提。
wait_for_n06_start "$N06_PID" "$N06_DIR/run.log" "$N06_SIGNAL"
N06_SYNC_EXIT=$?
echo "$N06_SYNC_EXIT" > "$N06_DIR/sync-exit-code.txt"
if [ "$N06_SYNC_EXIT" -ne 0 ]; then
  wait "$N06_PID"
  echo "$?" > "$N06_DIR/exit-code.txt"
  fail "N06 injection refused: initial binding and live gate not proven"
fi
n06_gate_running "$N06_PID" || fail "N06 injection refused: gate already exited or inaccessible"
printf '\n// N06 mid-gate mutation: a candidate byte change during the gate run\n' \
  >> "$COPY/rust/crates/lingxi-kernel/src/lib.rs" || fail "N06 source append failed"
wait $N06_PID
N06_EXIT=$?
echo "$N06_EXIT" > "$N06_DIR/exit-code.txt"
N06_NAMED=OK
python3 - "$N06_DIR" <<'PY' > "$N06_DIR/named-check.txt"
import json, pathlib, sys
d = pathlib.Path(sys.argv[1])
try:
    r = json.load(open(d / "evidence" / "verify-stage-result.json"))
except Exception as exc:
    print("result-unreadable:", exc)
    raise SystemExit(0)
binding = r.get("candidateSourceBinding", {})
print("overall=" + str(r.get("overall")))
print("stable=" + str(binding.get("stable")))
print("reason=" + str(binding.get("reason", "")))
PY
grep -qF "overall=FAIL" "$N06_DIR/named-check.txt" || N06_NAMED="MISSING:overall=FAIL"
grep -qF "stable=False" "$N06_DIR/named-check.txt" || N06_NAMED="MISSING:stable=False"
N06_VERDICT=BAD; if [ "$N06_EXIT" -ne 0 ] && [ "$N06_NAMED" = "OK" ]; then N06_VERDICT=OK; fi
record_case "R05-GATE-N06" "$N06_EXIT" "$N06_NAMED" "$N06_VERDICT" "门禁中改源码 → stable=false 整体 FAIL（R02 注册门禁演示同一机制）"

# ── N07: the R04 regression reference must not be removable ────────────────
reset_copy
# (a) drop the command while a scenario still references it → hard parse refusal
python3 - "$COPY" <<'PY'
import json, sys
p = sys.argv[1] + "/rust/crates/xtask/src/stage_maps/R05.json"
m = json.load(open(p))
del m["commands"]["r04_regression_gate"]
json.dump(m, open(p, "w"), indent=2, ensure_ascii=False)
PY
N07_DIR="$EV/n07-drop-r04reg"
mkdir -p "$N07_DIR"
note "== N07: r04_regression_gate command deleted while SUP-R04REG still references it; verify-stage R05 must refuse fast =="
(cd "$COPY" && "$CARGO" run --manifest-path rust/Cargo.toml --locked -p xtask -- \
   verify-stage R05 --evidence "$N07_DIR/evidence" > "$N07_DIR/run.log" 2>&1)
N07_EXIT=$?
echo "$N07_EXIT" > "$N07_DIR/exit-code.txt"
N07_NAMED=OK
grep -qF "references unknown command" "$N07_DIR/run.log" || N07_NAMED="MISSING:unknown-command"
N07_VERDICT=BAD; if [ "$N07_EXIT" -ne 0 ] && [ "$N07_NAMED" = "OK" ]; then N07_VERDICT=OK; fi
record_case "R05-GATE-N07" "$N07_EXIT" "$N07_NAMED" "$N07_VERDICT" "删 R04 回归命令 → 场景引用即硬拒；镜像单测亦钉住该命令"

# ── N08: production wiring removed (test-constructor-only shape) ───────────
reset_copy
sed -i '' 's|let model_gateway_for_wiring = if deps.turn_provider.is_none() {|let model_gateway_for_wiring = if deps.turn_provider.is_some() {|' \
  "$COPY/rust/crates/lingxi-service/src/lib.rs"
note "== N08: the real-chain wiring condition inverted (never wires without an injected provider); the BINARY suite must go red while units stay green =="
N08_DIR="$EV/n08-wiring-removed"
mkdir -p "$N08_DIR"
cargo_in_copy test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t01_binary_wiring \
  > "$N08_DIR/binary.log" 2>&1
N08_EXIT=$?
echo "$N08_EXIT" > "$N08_DIR/exit-code.txt"
N08_NAMED=OK
grep -qF "c02_real_binary_full_chain_through_authenticated_endpoint" "$N08_DIR/binary.log" || N08_NAMED="MISSING:c02-named"
grep -qE "never reached|FAILED|panicked" "$N08_DIR/binary.log" || N08_NAMED="MISSING:failure"
cargo_in_copy test --manifest-path rust/Cargo.toml --locked -p lingxi-service --lib \
  > "$N08_DIR/lib.log" 2>&1
N08_LIB_EXIT=$?
echo "$N08_LIB_EXIT" > "$N08_DIR/lib-exit-code.txt"
[ "$N08_LIB_EXIT" -eq 0 ] || N08_NAMED="CONTROL-BROKEN:lib-units-failed"
N08_VERDICT=BAD; if [ "$N08_EXIT" -ne 0 ] && [ "$N08_NAMED" = "OK" ]; then N08_VERDICT=OK; fi
record_case "R05-GATE-N08" "$N08_EXIT" "$N08_NAMED" "$N08_VERDICT" "移除正常启动接线 → 二进制链红、单测仍绿"

# ── N09: a fixed "done" replaces the real tool result ──────────────────────
reset_copy
python3 - "$COPY" <<'PY'
import sys
p = sys.argv[1] + "/rust/crates/lingxi-adapters/src/models/tool_render.rs"
s = open(p).read()
old = """pub fn render_tool_outcome_text(outcome: &ToolOutcome) -> String {
    match outcome {
        ToolOutcome::Success { result } => {"""
new = """pub fn render_tool_outcome_text(outcome: &ToolOutcome) -> String {
    match outcome {
        ToolOutcome::Success { result } => {
            let _ = result;
            return "done".to_string();"""
assert old in s, "N09 anchor not found"
open(p, "w").write(s.replace(old, new, 1))
PY
note "== N09: Success rendered as the fixed string 'done'; the runtime-nonce checks must go red =="
N09_DIR="$EV/n09-fixed-done"
mkdir -p "$N09_DIR"
cargo_in_copy test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t03_protocol_adapters c05_runtime_nonce \
  > "$N09_DIR/test.stdout.log" 2>&1
N09_EXIT=$?
echo "$N09_EXIT" > "$N09_DIR/exit-code.txt"
N09_NAMED=OK
grep -qF "c05_runtime_nonce_rides_the_next_request_and_follows_changes" "$N09_DIR/test.stdout.log" || N09_NAMED="MISSING:c05-named"
N09_VERDICT=BAD; if [ "$N09_EXIT" -ne 0 ] && [ "$N09_NAMED" = "OK" ]; then N09_VERDICT=OK; fi
record_case "R05-GATE-N09" "$N09_EXIT" "$N09_NAMED" "$N09_VERDICT" "固定 done 替代真实回传 → 运行时 nonce 检查红"

# ── N10: the provider call id mispaired to a constant ──────────────────────
reset_copy
python3 - "$COPY" <<'PY'
import sys
p = sys.argv[1] + "/rust/crates/lingxi-adapters/src/models/openai_completions.rs"
s = open(p).read()
old = """                messages.push(RequestMessage::Tool {
                    tool_call_id: provider_call_id,
                    content: render_tool_outcome_text(outcome),
                });"""
new = """                messages.push(RequestMessage::Tool {
                    tool_call_id: "call-fixed-mispaired".to_string(),
                    content: render_tool_outcome_text(outcome),
                });"""
assert old in s, "N10 anchor not found"
open(p, "w").write(s.replace(old, new, 1))
PY
note "== N10: tool results paired to a constant id; the pairing assertions must go red =="
N10_DIR="$EV/n10-callid-mispair"
mkdir -p "$N10_DIR"
cargo_in_copy test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t01_binary_wiring \
  > "$N10_DIR/test.stdout.log" 2>&1
N10_EXIT=$?
echo "$N10_EXIT" > "$N10_DIR/exit-code.txt"
N10_NAMED=OK
grep -qF "call_bin_1" "$N10_DIR/test.stdout.log" || N10_NAMED="MISSING:pairing-named"
N10_VERDICT=BAD; if [ "$N10_EXIT" -ne 0 ] && [ "$N10_NAMED" = "OK" ]; then N10_VERDICT=OK; fi
record_case "R05-GATE-N10" "$N10_EXIT" "$N10_NAMED" "$N10_VERDICT" "callId 错配 → 配对断言红（call_bin_1）"

# ── N11: Unknown/StopUnconfirmed flattened into success vocabulary ─────────
reset_copy
python3 - "$COPY" <<'PY'
import sys
p = sys.argv[1] + "/rust/crates/lingxi-adapters/src/models/tool_render.rs"
s = open(p).read()
# N11 mutation: the StopUnconfirmed status arm (INSIDE ToolOutcome::Success)
# is flattened into a clean exit-0 success claim — exactly the R04-honesty
# violation the pinned test exists to catch.
old_arm = """                    ToolRunStatus::StopUnconfirmed { handle, detail } => {
                        text.push_str(&format!(
                            "[process stop unconfirmed (handle: {handle}): {detail}]"
                        ));
                    }"""
new_arm = """                    ToolRunStatus::StopUnconfirmed { handle, detail } => {
                        let _ = (handle, detail);
                        text.push_str("[process exited with code 0]");
                    }"""
assert old_arm in s, "N11 anchor not found"
open(p, "w").write(s.replace(old_arm, new_arm, 1))
PY
note "== N11: Unknown flattened to an exit-0 success claim; the honesty unit pin must go red =="
N11_DIR="$EV/n11-flatten-unknown"
mkdir -p "$N11_DIR"
cargo_in_copy test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --lib \
  models::tool_render::tests::running_and_stop_unconfirmed_statuses_are_honest -- --exact \
  > "$N11_DIR/test.stdout.log" 2>&1
N11_EXIT=$?
echo "$N11_EXIT" > "$N11_DIR/exit-code.txt"
N11_NAMED=OK
grep -qF "running_and_stop_unconfirmed_statuses_are_honest" "$N11_DIR/test.stdout.log" || N11_NAMED="MISSING:honesty-named"
N11_VERDICT=BAD; if [ "$N11_EXIT" -ne 0 ] && [ "$N11_NAMED" = "OK" ]; then N11_VERDICT=OK; fi
record_case "R05-GATE-N11" "$N11_EXIT" "$N11_NAMED" "$N11_VERDICT" "压平 Unknown → 状态诚实单测红"

# ── N12: the model permit is NOT released before tool execution ────────────
reset_copy
python3 - "$COPY" <<'PY'
import sys
p = sys.argv[1] + "/rust/crates/lingxi-service/src/runs.rs"
s = open(p).read()
old = """            drop(model_permit);
            // R03-T04 result fence:"""
new = """            // N12 mutation: the permit is NOT released before tools.
            // R03-T04 result fence:"""
assert old in s, "N12 anchor not found"
open(p, "w").write(s.replace(old, new, 1))
PY
note "== N12: model permit held across tool execution; the concurrency=1 nested worker chain must go red =="
N12_DIR="$EV/n12-permit-held"
mkdir -p "$N12_DIR"
cargo_in_copy test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t06_worker_model c06_ \
  > "$N12_DIR/test.stdout.log" 2>&1
N12_EXIT=$?
echo "$N12_EXIT" > "$N12_DIR/exit-code.txt"
N12_NAMED=OK
grep -qE "c06_(main_run_worker_and_callback_complete_under_global_model_concurrency_one|a_held_global_model_permit)" "$N12_DIR/test.stdout.log" || N12_NAMED="MISSING:c06-named"
N12_VERDICT=BAD; if [ "$N12_EXIT" -ne 0 ] && [ "$N12_NAMED" = "OK" ]; then N12_VERDICT=OK; fi
record_case "R05-GATE-N12" "$N12_EXIT" "$N12_NAMED" "$N12_VERDICT" "不释放模型 permit → 配额=1 嵌套链红"

# ── N13: an R04 deferred leaf is re-graded / dropped ────────────────────────
reset_copy
python3 - "$COPY" <<'PY'
import json, sys
p = sys.argv[1] + "/rust/crates/xtask/src/stage_maps/R04.json"
m = json.load(open(p))
dropped = 0
for leaf in m.get("supplementalLeafScenarios", []):
    if leaf.get("basisKind") == "deferred_to_later_stage":
        del leaf["basisKind"]  # re-grade shape: parse-level hard error
        dropped += 1
        break
json.dump(m, open(p, "w"), indent=2, ensure_ascii=False)
print("mutated deferred leaves:", dropped)
PY
note "== N13: an R04 deferred leaf's kind field removed (re-grade shape); verify-stage R04 must refuse at the map/coverage check =="
N13_DIR="$EV/n13-r04-deferred-regrade"
mkdir -p "$N13_DIR"
(cd "$COPY" && "$CARGO" run --manifest-path rust/Cargo.toml --locked -p xtask -- \
   verify-stage R04 --evidence "$N13_DIR/evidence" > "$N13_DIR/run.log" 2>&1)
N13_EXIT=$?
echo "$N13_EXIT" > "$N13_DIR/exit-code.txt"
N13_NAMED=OK
grep -qEi "deferred_to_later_stage|invalid stage map" "$N13_DIR/run.log" || N13_NAMED="MISSING:leaf-kind-refusal"
N13_VERDICT=BAD; if [ "$N13_EXIT" -ne 0 ] && [ "$N13_NAMED" = "OK" ]; then N13_VERDICT=OK; fi
record_case "R05-GATE-N13" "$N13_EXIT" "$N13_NAMED" "$N13_VERDICT" "R04 递延叶改判/删除 → R04 门禁硬拒"

# ── N14: an unauthorized LIVE lane must not register ────────────────────────
reset_copy
python3 - "$COPY" <<'PY'
import json, sys
p = sys.argv[1] + "/rust/crates/xtask/src/stage_maps/R05.json"
m = json.load(open(p))
m["commands"]["live_probe"] = {
    "argv": ["bash", "-c", "echo live with LIVE_API_KEY=$LIVE_API_KEY"],
    "timeoutSecs": 60,
    "evidencePaths": ["{EVIDENCE}/live_probe/stdout.log"],
}
m["scenarios"].append({
    "id": "R05-LIVE-SNEAK",
    "requirement": "REQUIRED",
    "commandRefs": ["live_probe"],
})
json.dump(m, open(p, "w"), indent=2, ensure_ascii=False)
PY
note "== N14: a LIVE lane command (env-key pickup) sneaked into the map; the offline-only mirror must go red =="
xtask_test n14-live-lane r05_map_declares_no_live_lane "live_probe"
record_case "R05-GATE-N14" "$RECORD_EXIT" "$RECORD_NAMED" "$RECORD_VERDICT" "私开 LIVE lane → 离线命令集镜像红"

# ── N15: a synthetic secret appears in an error text ───────────────────────
reset_copy
python3 - "$COPY" <<'PY'
import sys
p = sys.argv[1] + "/rust/crates/lingxi-adapters/src/models/credentials.rs"
s = open(p).read()
old = '''            CredentialError::HandleRefused { provider, detail } => write!(
                f,
                "provider {provider:?} credential handle refused: {detail}"
            ),'''
new = '''            CredentialError::HandleRefused { provider, detail } => write!(
                f,
                "provider {provider:?} credential handle refused: {detail} material=sk-N15-SYNTHETIC-SECRET"
            ),'''
assert old in s, "N15 anchor not found"
open(p, "w").write(s.replace(old, new, 1))
PY
note "== N15: the credential refusal Display leaks a synthetic secret (sk-N15-…); the no-material pin must go red =="
N15_DIR="$EV/n15-secret-in-error"
mkdir -p "$N15_DIR"
cargo_in_copy test --manifest-path rust/Cargo.toml --locked -p lingxi-service --lib \
  credentials::tests::handle_refusal_texts_carry_no_material -- --exact \
  > "$N15_DIR/test.stdout.log" 2>&1
N15_EXIT=$?
echo "$N15_EXIT" > "$N15_DIR/exit-code.txt"
N15_NAMED=OK
grep -qF "handle_refusal_texts_carry_no_material" "$N15_DIR/test.stdout.log" || N15_NAMED="MISSING:no-material-named"
N15_VERDICT=BAD; if [ "$N15_EXIT" -ne 0 ] && [ "$N15_NAMED" = "OK" ]; then N15_VERDICT=OK; fi
record_case "R05-GATE-N15" "$N15_EXIT" "$N15_NAMED" "$N15_VERDICT" "秘密进错误文本 → 无料单测红"

# ── N16: code changed after acceptance; the old binding cannot be reused ───
reset_copy
N16_DIR="$EV/n16-binding-drift"
mkdir -p "$N16_DIR"
note "== N16: two verify-stage R02 runs bracket a source edit; the candidate digests must differ, and the used evidence root must be refused =="
(cd "$COPY" && "$CARGO" run --manifest-path rust/Cargo.toml --locked -p xtask -- \
   verify-stage R02 --evidence "$N16_DIR/run-a" > "$N16_DIR/run-a.log" 2>&1)
N16_A_EXIT=$?
printf '\n// N16 post-acceptance mutation: the candidate moved after run A\n' \
  >> "$COPY/rust/crates/lingxi-kernel/src/lib.rs"
(cd "$COPY" && "$CARGO" run --manifest-path rust/Cargo.toml --locked -p xtask -- \
   verify-stage R02 --evidence "$N16_DIR/run-b" > "$N16_DIR/run-b.log" 2>&1)
N16_B_EXIT=$?
# Evidence-root reuse refusal on the ALREADY-USED run-a root:
(cd "$COPY" && "$CARGO" run --manifest-path rust/Cargo.toml --locked -p xtask -- \
   verify-stage R02 --evidence "$N16_DIR/run-a" > "$N16_DIR/reuse.log" 2>&1)
N16_REUSE_EXIT=$?
echo "$N16_A_EXIT $N16_B_EXIT $N16_REUSE_EXIT" > "$N16_DIR/exit-codes.txt"
N16_NAMED=OK
python3 - "$N16_DIR" <<'PY' > "$N16_DIR/named-check.txt"
import json, sys
d = sys.argv[1]
try:
    a = json.load(open(d + "/run-a/verify-stage-result.json"))
    b = json.load(open(d + "/run-b/verify-stage-result.json"))
    da = a["candidateSourceBinding"]["before"]["digestSha256"]
    db = b["candidateSourceBinding"]["before"]["digestSha256"]
    print("digestA=" + da)
    print("digestB=" + db)
    print("digestsDiffer=" + str(da != db))
    print("overallA=" + str(a.get("overall")))
    print("overallB=" + str(b.get("overall")))
except Exception as exc:
    print("result-unreadable:", exc)
PY
grep -qF "digestsDiffer=True" "$N16_DIR/named-check.txt" || N16_NAMED="MISSING:digest-drift"
grep -qF "is not empty" "$N16_DIR/reuse.log" || N16_NAMED="MISSING:reuse-refused"
# N16's PROPERTY is the binding drift + reuse refusal (both runs must WRITE
# results with bindings; digests must differ; the used root must be refused).
# The R02 runs' own overall verdict is ORTHOGONAL — in this environment the
# registered intermittent LAN-stall item (R05-ENV-ALF-UNSIGNED-TEST-BINARY)
# can fail the R02 matrix commands in the copy's fresh binaries without any
# bearing on the binding mechanism under test.
N16_VERDICT=BAD
if [ "$N16_REUSE_EXIT" -ne 0 ] && [ "$N16_NAMED" = "OK" ]; then N16_VERDICT=OK; fi
record_case "R05-GATE-N16" "$N16_REUSE_EXIT" "$N16_NAMED" "$N16_VERDICT" "验收后改码 → 绑定摘要漂移且旧证据根拒收（属性判定不依赖 R02 整体结果——其矩阵命令受已登记的间歇 LAN 停驻环境项影响，与绑定机制无关）"

# 最后一项也必须还原，汇总不能在副本仍带变异时宣称完整恢复。
reset_copy
write_results
note "RESULT: every R05 negative case failed closed with the gap named (16/16), controls green"

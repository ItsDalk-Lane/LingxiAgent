#!/usr/bin/env bash
# R04 RR1 repair round G05 / CLOSE-C01 — adversarial-repair suite producer.
#
# Runs the six integration suites and the twenty-six lib unit tests added by
# the RR1 repair workorders G01–G04 (findings F01–F05) through the REAL
# service chain and pins each run's executed test count EXACTLY. This is
# the registered producer behind the R04 stage map's `r04_rr1_repair_suites`
# command / the R04-RR1-F01..F05 scenarios: the gate observes this script's
# real exit code, and this script refuses every fake-green shape on its
# own:
#   - a run whose filter matches 0 tests ("running 0 tests") is a GAP,
#     never a pass (cargo itself exits 0 there — the classic hole);
#   - a run that executed fewer/more tests than pinned (filtered subset,
#     renamed/deleted tests, duplicated runs) is a GAP;
#   - any failing/ignored test is a failure;
#   - every executed test must additionally be OWNED by exactly one RR1
#     C-ID of the acceptance checklist (the `cid` table below): a test
#     that ran but belongs to no case, or a pinned case whose test never
#     ran green (renamed/deleted/moved module), is a named GAP — 漏 ID
#     fails closed.
#
# Outputs (declared evidence of the stage map — must be FRESH per run,
# the gate's F04 freshness check enforces that):
#   <DIR>/rr1-cases.json   — lingxi.r04-rr1-repair-suite-results.v1, one
#     machine record per pinned run (expect == pinned count, actual ==
#     observed passed count) and one per C-ID (expect == pinned test-name
#     count, actual == names observed green in the run logs);
#   <DIR>/summary.txt      — per-run PASS lines + totals;
#   <DIR>/<run>.log        — the raw cargo test stdout per run;
#   <DIR>/pin-table.txt / cid-table.txt — the exact tables this run
#     verified (machine-mirrored by the xtask stage_map pin tests).
#
# The pin/cid tables below are mirrored by the xtask unit tests
# (stage_map.rs `r04_rr1_*` map-pinning tests) — deleting a mapping here,
# in the stage map, lowering a pinned count, or dropping a C-ID line
# turns the workspace test suite (a gate command itself) red.
#
# Usage: scripts/rust-tauri/r04_rr1_g05_repair_suites.sh [EVIDENCE_DIR]
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R04/RR1-G05-E01/rr1-repair-suites}"
if [ -L "$EVIDENCE_DIR" ] || { [ -e "$EVIDENCE_DIR" ] && [ ! -d "$EVIDENCE_DIR" ]; }; then
  echo "ERROR: evidence path is not a regular directory: $EVIDENCE_DIR" >&2
  exit 1
fi
if [ -d "$EVIDENCE_DIR" ]; then
  FIRST_ENTRY="$(find "$EVIDENCE_DIR" -mindepth 1 -print -quit)" || exit 1
  [ -z "$FIRST_ENTRY" ] || { echo "ERROR: evidence directory is not empty: $EVIDENCE_DIR" >&2; exit 1; }
fi
mkdir -p "$EVIDENCE_DIR"

TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/rust-target-r04-rr1-g05}"
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

fail() { echo "FAIL: $*" >&2; exit 1; }
note() { printf '%s\n' "$*" | tee -a "$EVIDENCE_DIR/summary.txt"; }

# ── pin table: run <pinned-count> <F-ID> ─────────────────────────────────────
# pin <run> <count> <F-ID>   (machine-checked by xtask stage_map tests)
# A `lib/`-prefixed run is an EXACT lib unit test (`cargo test --lib
# <full-path> -- --exact`); a bare run is an integration suite
# (`cargo test --test <suite>`). Registered 2026-10-01 by G05-E01 from the
# RR1 repair candidates G01(1692d2314)/G02(da15c4bd9)/G03(614fab1af)/
# G04(1285c3bf6): 6 integration suites (37 tests) + 26 lib unit tests =
# 63 tests (the F04 pure-logic family and the G04 exectools output-
# integrity family are unit tests by design — the G04 report's C-ID
# evidence mapping is mirrored verbatim).
PIN_LINES="
pin r04_t05_registry_capacity 9 RR1-F01
pin r04_rr1_f02_reaper_cleanup 11 RR1-F02
pin r04_rr1_f03_stop_honesty 7 RR1-F03
pin r04_rr1_f04_pty_consumption 1 RR1-F04
pin r04_rr1_f05_output_integrity 7 RR1-F05
pin r04_rr1_f05_spill_failure 2 RR1-F05
pin lib/procsupervisor::tests::live_slot_reservation_enforces_the_cap_atomically 1 RR1-F01
pin lib/procsupervisor::tests::live_slot_commit_transfers_release_to_settle_exactly_once 1 RR1-F01
pin lib/procsupervisor::tests::live_slot_drop_after_panic_style_abandon_still_releases 1 RR1-F01
pin lib/procsupervisor::tests::group_signal_is_skipped_once_the_child_reap_is_published 1 RR1-F02
pin lib/procsupervisor::tests::group_signal_is_skipped_when_the_kernel_identity_disagrees 1 RR1-F02
pin lib/procsupervisor::tests::exit_fact_status_codes_follow_the_shell_convention 1 RR1-F03
pin lib/procsupervisor::tests::terminal_facts_never_fabricate_an_exit_for_unconfirmed_or_live_phases 1 RR1-F03
pin lib/procsupervisor::tests::transcript_delivers_split_multibyte_characters_intact 1 RR1-F04
pin lib/procsupervisor::tests::transcript_mixed_chunk_prefix_is_consumed_byte_exactly_r04_rr1_f04_c01 1 RR1-F04
pin lib/procsupervisor::tests::transcript_idle_polls_after_a_partial_delivery_hold_back_without_loss 1 RR1-F04
pin lib/procsupervisor::tests::transcript_property_valid_input_reassembles_exactly 1 RR1-F04
pin lib/procsupervisor::tests::transcript_property_invalid_bytes_replaced_exactly_once 1 RR1-F04
pin lib/procsupervisor::tests::transcript_property_eviction_accounting_counts_only_real_loss 1 RR1-F04
pin lib/procsupervisor::tests::transcript_ring_overflow_while_holding_back_counts_only_real_evictions 1 RR1-F04
pin lib/procsupervisor::tests::transcript_boundary_across_many_chunks_with_interleaved_polls 1 RR1-F04
pin lib/procsupervisor::tests::transcript_force_delivery_flushes_a_dangling_partial 1 RR1-F04
pin lib/procsupervisor::tests::transcript_ring_drop_of_undelivered_is_counted_honestly 1 RR1-F04
pin lib/exectools::tests::transcript_spill_claim_vocabulary_is_state_exclusive 1 RR1-F05
pin lib/exectools::tests::full_output_claim_vocabulary_is_state_exclusive 1 RR1-F05
pin lib/exectools::tests::spill_resource_ref_never_claims_full_when_capped_or_failed 1 RR1-F05
pin lib/exectools::tests::assemble_retained_output_small_stream_is_whole_and_exact 1 RR1-F05
pin lib/exectools::tests::assemble_retained_output_evicted_middle_is_counted_and_marked 1 RR1-F05
pin lib/exectools::tests::truncate_head_tail_single_huge_ascii_line_keeps_head_and_tail 1 RR1-F05
pin lib/exectools::tests::truncate_head_tail_multibyte_line_cuts_on_char_boundaries 1 RR1-F05
pin lib/exectools::tests::truncate_head_tail_newline_only_at_end_keeps_both_markers 1 RR1-F05
pin lib/exectools::tests::truncate_head_tail_small_output_stays_whole 1 RR1-F05
"

# ── cid table: which executed test evidences which acceptance C-ID ──────────
# cid <C-ID> <run> <test-name-1>+<test-name-2>...
# Every executed test is owned by EXACTLY ONE C-ID of the RR1 acceptance
# checklist (五 F 共 22 个 C-ID；CLOSE-C01..C04 是收口检查，不是阶段图
# 场景，由 G05/总控收口执行)。The assembly step verifies each name has a
# literal `test <name> ... ok` line in the run's log and that the union of
# names equals the union of executed tests (no orphan, no double count).
CID_LINES="
cid R04-RR1-F01-C01 r04_t05_registry_capacity repro_rr1_f01_registry_full_refusal_must_not_execute_the_command+rr1_f01_c01_registry_full_refuses_with_zero_dispatch
cid R04-RR1-F01-C01 lib/procsupervisor::tests::live_slot_reservation_enforces_the_cap_atomically procsupervisor::tests::live_slot_reservation_enforces_the_cap_atomically
cid R04-RR1-F01-C02 r04_t05_registry_capacity rr1_f01_c02_two_one_shots_race_the_last_slot_through_a_barrier+rr1_f01_c02_two_ptys_race_the_last_slot_through_a_barrier+rr1_f01_c02_one_shot_and_pty_race_the_last_slot_through_a_barrier
cid R04-RR1-F01-C03 r04_t05_registry_capacity rr1_f01_c03_one_shot_group_verify_failure_compensates_exactly_once+rr1_f01_c03_pty_group_verify_failure_closes_the_pty_and_returns_the_slot+rr1_f01_c03_pty_open_failure_is_zero_dispatch_and_returns_the_slot
cid R04-RR1-F01-C03 lib/procsupervisor::tests::live_slot_commit_transfers_release_to_settle_exactly_once procsupervisor::tests::live_slot_commit_transfers_release_to_settle_exactly_once
cid R04-RR1-F01-C03 lib/procsupervisor::tests::live_slot_drop_after_panic_style_abandon_still_releases procsupervisor::tests::live_slot_drop_after_panic_style_abandon_still_releases
cid R04-RR1-F01-C04 r04_t05_registry_capacity rr1_f01_c04_same_process_usable_after_repeated_refusals_and_releases
cid R04-RR1-F02-C01 r04_rr1_f02_reaper_cleanup rr1_f02_repro_grandchild_pump_outlives_the_grace_and_mutates_the_settled_collector+rr1_f02_c01_grandchild_holding_both_ends_freezes_the_result_and_closes_the_read_ends+rr1_f02_c01_pty_family_grandchild_holding_the_slave_closes_the_master+rr1_f02_c01_adversarial_slow_writer_cannot_mutate_the_settled_collector
cid R04-RR1-F02-C02 r04_rr1_f02_reaper_cleanup rr1_f02_fast_exiting_child_is_not_refused_by_the_reap_race+rr1_f02_c02_terminate_in_the_reaped_undrained_window_signals_nothing+rr1_f02_c02_adversarial_window_offsets_never_signal_a_stale_group
cid R04-RR1-F02-C02 lib/procsupervisor::tests::group_signal_is_skipped_once_the_child_reap_is_published procsupervisor::tests::group_signal_is_skipped_once_the_child_reap_is_published
cid R04-RR1-F02-C02 lib/procsupervisor::tests::group_signal_is_skipped_when_the_kernel_identity_disagrees procsupervisor::tests::group_signal_is_skipped_when_the_kernel_identity_disagrees
cid R04-RR1-F02-C03 r04_rr1_f02_reaper_cleanup rr1_f02_c03_double_stuck_pumps_complete_within_the_single_budget+rr1_f02_c03_adversarial_panicking_pump_is_observed_and_honest+rr1_f02_c03_adversarial_exit_and_cancel_racing_never_signals_unprovably
cid R04-RR1-F02-C04 r04_rr1_f02_reaper_cleanup rr1_f02_c04_stress_cycles_return_to_the_declared_steady_state
cid R04-RR1-F03-C01 r04_rr1_f03_stop_honesty rr1_f03_c01_timeout_cleanup_unconfirmed_never_reports_an_exit
cid R04-RR1-F03-C01 lib/procsupervisor::tests::terminal_facts_never_fabricate_an_exit_for_unconfirmed_or_live_phases procsupervisor::tests::terminal_facts_never_fabricate_an_exit_for_unconfirmed_or_live_phases
cid R04-RR1-F03-C02 r04_rr1_f03_stop_honesty rr1_f03_c02_pty_cleanup_unconfirmed_poll_agrees_with_the_text
cid R04-RR1-F03-C03 r04_rr1_f03_stop_honesty rr1_f03_c03_repeated_terminate_does_not_upgrade_unconfirmed_to_confirmed
cid R04-RR1-F03-C04 r04_rr1_f03_stop_honesty rr1_f03_c04_control_group_real_states_stay_accurate+rr1_f03_c04_adversarial_cancel_racing_natural_exit_stays_real
cid R04-RR1-F03-C04 lib/procsupervisor::tests::exit_fact_status_codes_follow_the_shell_convention procsupervisor::tests::exit_fact_status_codes_follow_the_shell_convention
cid R04-RR1-F03-C05 r04_rr1_f03_stop_honesty rr1_f03_c05_run_cancel_keeps_control_flow_and_external_unconfirmed_separate+rr1_f03_c05_adversarial_dropped_tool_future_keeps_the_honest_chain
cid R04-RR1-F04-C01 lib/procsupervisor::tests::transcript_mixed_chunk_prefix_is_consumed_byte_exactly_r04_rr1_f04_c01 procsupervisor::tests::transcript_mixed_chunk_prefix_is_consumed_byte_exactly_r04_rr1_f04_c01
cid R04-RR1-F04-C01 lib/procsupervisor::tests::transcript_delivers_split_multibyte_characters_intact procsupervisor::tests::transcript_delivers_split_multibyte_characters_intact
cid R04-RR1-F04-C02 lib/procsupervisor::tests::transcript_idle_polls_after_a_partial_delivery_hold_back_without_loss procsupervisor::tests::transcript_idle_polls_after_a_partial_delivery_hold_back_without_loss
cid R04-RR1-F04-C03 lib/procsupervisor::tests::transcript_property_valid_input_reassembles_exactly procsupervisor::tests::transcript_property_valid_input_reassembles_exactly
cid R04-RR1-F04-C03 lib/procsupervisor::tests::transcript_property_invalid_bytes_replaced_exactly_once procsupervisor::tests::transcript_property_invalid_bytes_replaced_exactly_once
cid R04-RR1-F04-C03 lib/procsupervisor::tests::transcript_property_eviction_accounting_counts_only_real_loss procsupervisor::tests::transcript_property_eviction_accounting_counts_only_real_loss
cid R04-RR1-F04-C03 lib/procsupervisor::tests::transcript_ring_overflow_while_holding_back_counts_only_real_evictions procsupervisor::tests::transcript_ring_overflow_while_holding_back_counts_only_real_evictions
cid R04-RR1-F04-C03 lib/procsupervisor::tests::transcript_boundary_across_many_chunks_with_interleaved_polls procsupervisor::tests::transcript_boundary_across_many_chunks_with_interleaved_polls
cid R04-RR1-F04-C03 lib/procsupervisor::tests::transcript_force_delivery_flushes_a_dangling_partial procsupervisor::tests::transcript_force_delivery_flushes_a_dangling_partial
cid R04-RR1-F04-C03 lib/procsupervisor::tests::transcript_ring_drop_of_undelivered_is_counted_honestly procsupervisor::tests::transcript_ring_drop_of_undelivered_is_counted_honestly
cid R04-RR1-F04-C04 r04_rr1_f04_pty_consumption f04_c04_real_pty_handshake_delivers_mixed_bytes_exactly_once
cid R04-RR1-F05-C01 r04_rr1_f05_output_integrity f05_c01_hidden_window_prefix_loss_is_flagged_not_hidden+f05_c01_control_small_stream_big_budget_distinguishes_facts+f05_c01_big_stream_marks_eviction_and_keeps_true_head_and_tail
cid R04-RR1-F05-C01 lib/exectools::tests::assemble_retained_output_small_stream_is_whole_and_exact exectools::tests::assemble_retained_output_small_stream_is_whole_and_exact
cid R04-RR1-F05-C01 lib/exectools::tests::assemble_retained_output_evicted_middle_is_counted_and_marked exectools::tests::assemble_retained_output_evicted_middle_is_counted_and_marked
cid R04-RR1-F05-C02 r04_rr1_f05_output_integrity f05_c02_single_multibyte_line_keeps_head_and_tail_content
cid R04-RR1-F05-C02 lib/exectools::tests::truncate_head_tail_single_huge_ascii_line_keeps_head_and_tail exectools::tests::truncate_head_tail_single_huge_ascii_line_keeps_head_and_tail
cid R04-RR1-F05-C02 lib/exectools::tests::truncate_head_tail_multibyte_line_cuts_on_char_boundaries exectools::tests::truncate_head_tail_multibyte_line_cuts_on_char_boundaries
cid R04-RR1-F05-C02 lib/exectools::tests::truncate_head_tail_newline_only_at_end_keeps_both_markers exectools::tests::truncate_head_tail_newline_only_at_end_keeps_both_markers
cid R04-RR1-F05-C02 lib/exectools::tests::truncate_head_tail_small_output_stays_whole exectools::tests::truncate_head_tail_small_output_stays_whole
cid R04-RR1-F05-C03 r04_rr1_f05_output_integrity f05_c03_capped_spill_is_partial_and_honest
cid R04-RR1-F05-C03 lib/exectools::tests::transcript_spill_claim_vocabulary_is_state_exclusive exectools::tests::transcript_spill_claim_vocabulary_is_state_exclusive
cid R04-RR1-F05-C03 lib/exectools::tests::full_output_claim_vocabulary_is_state_exclusive exectools::tests::full_output_claim_vocabulary_is_state_exclusive
cid R04-RR1-F05-C03 lib/exectools::tests::spill_resource_ref_never_claims_full_when_capped_or_failed exectools::tests::spill_resource_ref_never_claims_full_when_capped_or_failed
cid R04-RR1-F05-C04 r04_rr1_f05_spill_failure f05_c04_spill_write_failure_diagnoses_honestly_no_ghost_full_file+f05_c04_unwritable_spill_dir_reports_no_spill_kept
cid R04-RR1-F05-C05 r04_rr1_f05_output_integrity f05_c05_high_output_and_many_pty_polls_stay_bounded+f05_c05_cancelling_mid_output_closes_the_spill_and_bounds_everything
"

printf '%s\n' "$PIN_LINES" | sed '/^$/d' > "$EVIDENCE_DIR/pin-table.txt"
printf '%s\n' "$CID_LINES" | sed '/^$/d' > "$EVIDENCE_DIR/cid-table.txt"

log_name() { printf '%s' "$1" | tr ':/' '__'; }

note "== building the RR1 repair suites (rustup $TOOLCHAIN, $TARGET_DIR, --locked, offline) =="
env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TARGET_DIR \
  rustup run "$TOOLCHAIN" cargo test --manifest-path rust/Cargo.toml --locked --offline \
  -p lingxi-service --lib \
  --test r04_t05_registry_capacity \
  --test r04_rr1_f02_reaper_cleanup \
  --test r04_rr1_f03_stop_honesty \
  --test r04_rr1_f04_pty_consumption \
  --test r04_rr1_f05_output_integrity \
  --test r04_rr1_f05_spill_failure \
  --no-run \
  > "$EVIDENCE_DIR/build.log" 2>&1 || { cat "$EVIDENCE_DIR/build.log"; fail "build failed"; }
note "PASS build (locked, offline)"

GAPS_FILE="$EVIDENCE_DIR/gaps.txt"
: > "$GAPS_FILE"
CASES_FILE="$EVIDENCE_DIR/cases.jsonl"
: > "$CASES_FILE"

while read -r _ run pinned fid; do
  [ -n "$run" ] || continue
  LOG="$EVIDENCE_DIR/$(log_name "$run").log"
  if [[ "$run" == lib/* ]]; then
    TEST_PATH="${run#lib/}"
    RUN_ARGS=(--lib "$TEST_PATH" -- --exact)
    note "== running lib unit test $TEST_PATH (F-ID $fid, pinned $pinned) =="
  else
    RUN_ARGS=(--test "$run" -- --test-threads=2)
    note "== running repair suite $run (F-ID $fid, pinned $pinned) =="
  fi
  set +e
  env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
    CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TARGET_DIR \
    rustup run "$TOOLCHAIN" cargo test --manifest-path rust/Cargo.toml --locked --offline \
    -p lingxi-service "${RUN_ARGS[@]}" \
    > "$LOG" 2>&1
  RUN_EXIT=$?
  set -e
  RAN="$(sed -n 's/^running \([0-9][0-9]*\) test[s]*$/\1/p' "$LOG" | head -n 1)"
  PASSED="$(sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed.*/\1/p' "$LOG" | head -n 1)"
  if [ -z "$PASSED" ]; then
    PASSED="$(sed -n 's/^test result: FAILED\. [0-9][0-9]* failed; \([0-9][0-9]*\) passed.*/\1/p' "$LOG" | head -n 1)"
  fi
  FAILED="$(sed -n 's/^test result: FAILED\. \([0-9][0-9]*\) failed.*/\1/p' "$LOG" | head -n 1)"
  if [ "$RUN_EXIT" -ne 0 ]; then
    echo "run $run: cargo test exit $RUN_EXIT" >> "$GAPS_FILE"
  fi
  if [ -z "$RAN" ] || [ -z "$PASSED" ]; then
    echo "run $run: no parseable libtest summary (running='${RAN:-}' passed='${PASSED:-}') — the filter matched nothing or the harness output drifted" >> "$GAPS_FILE"
  else
    if [ "$RAN" -eq 0 ]; then
      echo "run $run: filter matched 0 tests (running=0, pinned=$pinned) — empty test collections must not pass" >> "$GAPS_FILE"
    fi
    if [ "$RAN" -ne "$pinned" ]; then
      echo "run $run: executed $RAN tests but the stage pin is $pinned (filtered subset / renamed or deleted tests)" >> "$GAPS_FILE"
    fi
    if [ "$PASSED" -ne "$pinned" ]; then
      echo "run $run: passed $PASSED but the stage pin is $pinned" >> "$GAPS_FILE"
    fi
  fi
  if [ -n "$FAILED" ] && [ "$FAILED" -ne 0 ]; then
    echo "run $run: $FAILED failing tests" >> "$GAPS_FILE"
  fi
  ACTUAL="${PASSED:-0}"
  OK="false"
  if [ -z "$(grep "^run $run:" "$GAPS_FILE")" ]; then OK="true"; fi
  python3 - "$CASES_FILE" "$run" "$fid" "$pinned" "$ACTUAL" "$OK" <<'PY'
import json, sys
path, run, fid, expect, actual, ok = sys.argv[1:7]
record = {"run": run, "issue": fid, "expect": int(expect),
          "actual": int(actual), "ok": ok == "true"}
with open(path, "a", encoding="utf-8") as fh:
    fh.write(json.dumps(record, ensure_ascii=False) + "\n")
PY
  if [ "$OK" = "true" ]; then
    note "PASS $run ($fid): $PASSED/$pinned tests green"
  else
    note "FAIL $run ($fid): see gaps.txt"
  fi
done <<EOF
$PIN_LINES
EOF

# Assemble the machine-consumable case file from the per-run records and
# verify the per-C-ID test-name ownership against the real run logs.
python3 - "$EVIDENCE_DIR" << 'PYEOF' || fail "case assembly failed"
import json, pathlib, re, sys
ev = pathlib.Path(sys.argv[1])
records = [json.loads(line) for line in (ev / "cases.jsonl").read_text().splitlines() if line.strip()]
if not records:
    raise SystemExit("no per-run records were produced")
pins = {}
for line in (ev / "pin-table.txt").read_text().splitlines():
    _, run, count, fid = line.split()
    pins[run] = (int(count), fid)

def log_of(run):
    return ev / (run.replace(":", "_").replace("/", "_") + ".log")

cid_lines = [line.split() for line in (ev / "cid-table.txt").read_text().splitlines() if line.strip()]
gaps = []
cases = {}
ownership = {}
for fields in cid_lines:
    if len(fields) != 4:
        gaps.append(f"malformed cid line: {' '.join(fields)!r}")
        continue
    _, cid, run, names_field = fields
    if run not in pins:
        gaps.append(f"cid {cid}: run {run!r} is not in the pin table")
        continue
    fid = pins[run][1]
    if not cid.startswith("R04-" + fid + "-"):
        gaps.append(f"cid {cid}: run {run!r} belongs to {fid} but the case id does not")
    log = log_of(run).read_text() if log_of(run).exists() else ""
    for name in names_field.split("+"):
        key = (run, name)
        if key in ownership:
            gaps.append(f"test {name!r} in run {run!r} is claimed by both "
                        f"{ownership[key]} and {cid}")
            continue
        ownership[key] = cid
        ok_line = re.search(r"^test " + re.escape(name) + r" \.\.\. ok$", log, re.M)
        entry = cases.setdefault(cid, {"expect": 0, "actual": 0})
        entry["expect"] += 1
        if ok_line:
            entry["actual"] += 1
        else:
            gaps.append(f"cid {cid}: test {name!r} has no `... ok` line in {log_of(run).name} "
                        f"(renamed, deleted, moved module, or not green)")

total_pinned = sum(count for count, _ in pins.values())
if len(ownership) != total_pinned:
    executed = {r["run"]: r for r in records}
    for run, (count, fid) in pins.items():
        owned = sum(1 for (r, _n) in ownership if r == run)
        if owned != count:
            gaps.append(f"run {run!r} pins {count} executed tests but the cid table owns {owned} "
                        f"— every executed test must belong to exactly one C-ID")
if not cases:
    gaps.append("no C-ID case records were produced from the cid table")

case_records = []
for cid in sorted(cases):
    entry = cases[cid]
    case_records.append({"case": cid, "expect": entry["expect"],
                         "actual": entry["actual"], "ok": entry["expect"] == entry["actual"]})
doc = {
    "schema": "lingxi.r04-rr1-repair-suite-results.v1",
    "producedBy": "scripts/rust-tauri/r04_rr1_g05_repair_suites.sh "
                  "(cargo test -p lingxi-service, real chain)",
    "suites": records,
    "cases": case_records,
    "allSuitesOk": all(r["ok"] for r in records),
    "allCasesOk": all(c["ok"] for c in case_records) if case_records else False,
}
(ev / "rr1-cases.json").write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n")
print(f"assembled {len(records)} run records / {len(case_records)} C-ID case records")
if gaps:
    (ev / "cid-gaps.txt").write_text("\n".join(gaps) + "\n")
    raise SystemExit("cid ownership gaps:\n" + "\n".join(gaps))
PYEOF
note "PASS case files assembled (rr1-cases.json; every executed test owned by exactly one C-ID)"

if grep -q . "$GAPS_FILE"; then
  note "GAPS (each must be named, none may pass silently):"
  sed 's/^/  /' "$GAPS_FILE" | tee -a "$EVIDENCE_DIR/summary.txt"
  fail "RR1 repair-suite coverage has gaps ($(wc -l < "$GAPS_FILE" | tr -d ' ') lines)"
fi
note "RESULT: all RR1 repair runs green with exact pinned counts (6 integration suites + 26 lib unit tests, 63 tests across 22 C-IDs)"

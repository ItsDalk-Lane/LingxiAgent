# RR2-F05-E01 evidence root — R03-RR2-F05-01 fixed repair (canonical requestId chain)

Executor: EXECUTOR-R03-RR2-F05-E01. Baseline: c96f7cc635cf18fa83a81b0e28f33d4fed8baf9c
(branch codex/rust-tauri-migration, clean at start). This directory holds the
evidence of the one confirmed defect this executor was scoped to; no commit or
push was performed (Git belongs to the coordinator).

## Directory map (every file ↔ command ↔ C-ID)

### failure-originals/ — Step A: the defect reproduced on the UNMODIFIED baseline
- `baseline-run.txt` — raw `cargo test --manifest-path rust/Cargo.toml --locked
  -p lingxi-service --test request_id_canonicalization -- --test-threads=4`
  (offline, dedicated CARGO_TARGET_DIR), exit 101: 1 passed (C01 control),
  4 FAILED. Proof for C02 (padded id), C02-matrix (fg/confirmed/raw cell got a
  FRESH acceptance, run_count 2 — the blind re-execution), C03 (Tab variant
  persisted the raw anchor), C05 (legacy row missed → fresh acceptance).
- `README.txt` — the exact command, environment and per-failure reading.

### selfcheck/ — Step E: local gate evidence (all real runs)
- `e1-suites.txt` — C02-affected suites:
  `cargo test ... -p lingxi-service --test request_id_canonicalization
  --test admission_dedup_adversarial --test admission_dedup_consistency --
  --test-threads=4` → 7 + 5 + 5 passed, exit 0. Covers C01..C05 suites plus the
  two frozen G04/F05 suites (no regression).
- `e2-repair-suites.txt` — `bash scripts/rust-tauri/r03_g07_repair_suites.sh
  <this-dir>/repair-suites` (fresh empty dir) → exit 0, all TEN suites green at
  exact pinned counts (C06-adjacent producer gate; includes RR2-F05 7/7).
- `e3-e4-fmt-clippy.txt` — `cargo fmt --all -- --check` (exit 0) and
  `cargo clippy --workspace --all-targets --locked -- -D warnings` (exit 0).
- `e5-workspace-tests.pointer.txt` + `../logs/e5-workspace-tests.txt` —
  `cargo test --manifest-path rust/Cargo.toml --locked --workspace` → exit 0,
  73 "ok" result lines, 718 passed total (RR1 baseline 709 + 7 new integration
  tests + 2 new kernel unit tests). Includes the xtask pin tests and all
  pre-existing suites.
- `e6-e7-contracts-boundaries.txt` — `cargo run ... -p xtask -- check-contracts`
  (exit 0) and `-- check-boundaries` (exit 0), all PASS lines.

### logs/ — raw large outputs
- `e5-workspace-tests.txt` — the full workspace test log behind the pointer.

### repair-suites/ — the registered producer's own evidence (E2)
- `repair-cases.json` / `cases.jsonl` / `summary.txt` / `gaps.txt` (empty) /
  `build.log` / per-suite `*.log` — machine records of the ten pinned suites,
  produced by the registered `repair_suites` gate command against this
  candidate's working tree.

## C-ID → suite → test names (all in
`rust/crates/lingxi-service/tests/request_id_canonicalization.rs`)

| C-ID | test |
|---|---|
| C01 | `rr2_f05_c01_plain_id_control_group_keeps_the_restart_contract` |
| C02 | `rr2_f05_c02_padded_id_raw_and_canonical_retries_bind_one_logical_request`, `rr2_f05_c02_restart_matrix_foreground_background_confirmed_unknown` (8 cells) |
| C03 | `rr2_f05_c03_whitespace_variants_share_one_canonical_chain` |
| C04 | `rr2_f05_c04_colliding_raw_ids_one_namespace_isolation_and_compensation` |
| C05 | `rr2_f05_c05_legacy_cause_ids_stay_linkable_across_restart_shapes`, `rr2_f05_c05_colliding_legacy_rows_refuse_as_explicit_ambiguity` |
| C06 | not written by this executor (belongs to the coordinator + independent reviewer); the producer-gate leg is `e2-repair-suites.txt` |

The execution report is at `docs/rust-tauri/R03/repair-current/R03_RR2_F05_E01_REPORT.md`.

# RR2-F05-E01 failure originals (Step A — defect reproduction on the UNMODIFIED baseline)

- Baseline commit: c96f7cc635cf18fa83a81b0e28f33d4fed8baf9c (branch codex/rust-tauri-migration,
  workspace clean except the new test suite file itself — no product code was modified
  before this run).
- Suite (new, part of this fix): rust/crates/lingxi-service/tests/request_id_canonicalization.rs
  (version at this run: C01/C02/C02-matrix/C03/C05-single+abnormal; the C05-ambiguity and
  C04 cases were added AFTER the fix, because they reference the new
  RequestIdBoundAmbiguous error variant that does not exist on the baseline).
- Command (from the repository root):
    CARGO_NET_OFFLINE=true \
    CARGO_TARGET_DIR=${TMPDIR:-/tmp}/rust-target-r03-rr2-f05 \
    cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service \
      --test request_id_canonicalization -- --test-threads=4
  Raw output: baseline-run.txt ; exit code: 101 (test failure).
- Result: 1 passed (rr2_f05_c01_plain_id_control_group — the no-whitespace control group
  keeps the frozen contract), 4 FAILED:
    * rr2_f05_c02_padded_id_raw_and_canonical_retries_bind_one_logical_request —
      the persisted cause_id is "request: req-42 " (built from the RAW id), expected
      "request:req-42".
    * rr2_f05_c02_restart_matrix_foreground_background_confirmed_unknown —
      cell fg=true/confirmed=true/raw=true: the post-restart retry returned a FRESH
      acceptance (run_count: 2) — the silent blind re-execution R03-RR2-F05-01 names.
    * rr2_f05_c03_whitespace_variants_share_one_canonical_chain —
      variant "\treq-tab\t" persisted cause_id "request:\treq-tab\t".
    * rr2_f05_c05_legacy_cause_ids_stay_linkable_across_restart_shapes —
      a legacy row cause_id="request: req-42 " is MISSED by the logical-key retry
      (fresh acceptance, run_count: 2) instead of being linkable.
- Toolchain: rustup-managed locked toolchain (rust/crates rust-toolchain.toml at repo
  root, rustc/cargo 1.98.1), offline mode, dedicated CARGO_TARGET_DIR.
- These failures were produced BEFORE any product-code change; they prove the defect
  is real on the live admission chain, not a paper derivation.

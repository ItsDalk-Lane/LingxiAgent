#!/bin/bash
# FINAL-03 supplementary evidence commands (per FINAL brief: workspace failure
# stopped subsequent test binaries -> complete the unexecuted scope with
# --no-fail-fast and enumerate ALL failures; plus a targeted exact rerun of the
# single failing test to characterize flakiness). These do NOT replace §5.3
# commands and cannot flip the gate verdict.
set -u
HERE=/private/tmp/rr3-final03-dir
R=$HERE/run-cmd.sh
CARGO=/Users/study_superior/.cargo/bin/cargo
echo "supp start $(date -u +%Y-%m-%dT%H:%M:%S.%NZ)" >> $HERE/supplementary.meta

$R supp-01-terminal-family-exact $CARGO test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r04_t08_tool_matrix terminal_family_share_cases -- --exact > $HERE/supp-01.console 2>&1; S1=$?

$R supp-02-workspace-nofailfast $CARGO test --manifest-path rust/Cargo.toml --workspace --locked --no-fail-fast > $HERE/supp-02.console 2>&1; S2=$?

echo "supp end $(date -u +%Y-%m-%dT%H:%M:%S.%NZ) exact=$S1 nofailfast=$S2" >> $HERE/supplementary.meta
echo "SUPP DONE exact=$S1 nofailfast=$S2"

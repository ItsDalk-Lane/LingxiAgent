#!/bin/bash
# RR3 FINAL-03: run all six §5.3 commands sequentially, exactly as dispatched.
set -u
CARGO=/Users/study_superior/.cargo/bin/cargo
HERE=/private/tmp/rr3-final03-dir
R=$HERE/run-cmd.sh
{
echo "utcStart: $(date -u +%Y-%m-%dT%H:%M:%S.%NZ)"
echo "dfStart: $(df -g /System/Volumes/Data | tail -1)"
} > $HERE/run-all.meta

$R cmd-01-fmt        $CARGO fmt --manifest-path rust/Cargo.toml --all -- --check                     > $HERE/cmd-01.console 2>&1; E1=$?
$R cmd-02-clippy     $CARGO clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings > $HERE/cmd-02.console 2>&1; E2=$?

# r00 monitor runs across workspace test AND verify-stage (r00 runs in both)
rm -f $HERE/r00-monitor.jsonl $HERE/r00-monitor.stop
nohup $HERE/r00-monitor.sh $HERE/r00-monitor.jsonl > $HERE/r00-monitor-launch.log 2>&1 &
MON=$!
$R cmd-03-workspace-test $CARGO test --manifest-path rust/Cargo.toml --workspace --locked           > $HERE/cmd-03.console 2>&1; E3=$?

$R cmd-04-check-contracts $CARGO run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts > $HERE/cmd-04.console 2>&1; E4=$?
$R cmd-05-check-boundaries $CARGO run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries > $HERE/cmd-05.console 2>&1; E5=$?
$R cmd-06-verify-stage-R05 $CARGO run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R05 --evidence artifacts/rust-tauri/R05/RR3/FINAL-03/verify-R05 > $HERE/cmd-06.console 2>&1; E6=$?

touch $HERE/r00-monitor.stop; sleep 2; kill $MON 2>/dev/null; wait $MON 2>/dev/null

{
echo "utcEnd: $(date -u +%Y-%m-%dT%H:%M:%S.%NZ)"
echo "dfEnd: $(df -g /System/Volumes/Data | tail -1)"
echo "exits: fmt=$E1 clippy=$E2 workspace=$E3 contracts=$E4 boundaries=$E5 verifyStage=$E6"
} >> $HERE/run-all.meta
echo "ALL DONE exits: $E1 $E2 $E3 $E4 $E5 $E6"

#!/bin/bash
# RR3 FINAL-03 attempt-2: rerun ONLY cmd-06 (verify-stage R05) into the mandated
# fresh evidence root. Commands 1-5 completed green earlier this round (own-run
# records in cmd-01..cmd-05), not rerun per no-new-information rule.
set -u
HERE=/private/tmp/rr3-final03-dir
R=$HERE/run-cmd.sh
CARGO=/Users/study_superior/.cargo/bin/cargo

# relaunch r00 monitor (attempt-1 monitor died with the host kill)
rm -f $HERE/r00-monitor-attempt2.jsonl $HERE/r00-monitor-attempt2.stop
nohup $HERE/r00-monitor.sh $HERE/r00-monitor-attempt2.jsonl > $HERE/r00-monitor-attempt2-launch.log 2>&1 &

echo "attempt2 start $(date -u +%Y-%m-%dT%H:%M:%S.%NZ)" >> $HERE/attempt2.meta
echo "dfStart: $(df -g /System/Volumes/Data | tail -1)" >> $HERE/attempt2.meta

$R cmd-06-verify-stage-R05-attempt2 $CARGO run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R05 --evidence artifacts/rust-tauri/R05/RR3/FINAL-03/verify-R05 > $HERE/cmd-06-attempt2.console 2>&1
E=$?

touch $HERE/r00-monitor-attempt2.stop; sleep 2
echo "attempt2 end $(date -u +%Y-%m-%dT%H:%M:%S.%NZ) exit=$E" >> $HERE/attempt2.meta
echo "dfEnd: $(df -g /System/Volumes/Data | tail -1)" >> $HERE/attempt2.meta
echo "ATTEMPT2 DONE exit=$E"

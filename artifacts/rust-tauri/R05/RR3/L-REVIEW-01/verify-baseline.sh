#!/bin/sh
# Recompute the three baselines and compare against baseline-hashes.txt.
# Any tampering (with the test file OR with this baseline record) must surface as MISMATCH.
A="$(cd "$(dirname "$0")" && pwd)"
R=/Users/study_superior/Desktop/Code/LingxiAgent
cd "$R" || exit 9
d=$(git diff HEAD -- rust/crates/lingxi-service/tests/r04_t08_tool_matrix.rs | shasum -a 256 | awk '{print $1}')
h=$(git show HEAD:rust/crates/lingxi-service/tests/r04_t08_tool_matrix.rs | shasum -a 256 | awk '{print $1}')
w=$(shasum -a 256 rust/crates/lingxi-service/tests/r04_t08_tool_matrix.rs | awk '{print $1}')
exp_d=$(grep 'diff HEAD' "$A/baseline-hashes.txt" | awk -F'= ' '{print $2}')
exp_h=$(grep 'git show HEAD' "$A/baseline-hashes.txt" | awk -F'= ' '{print $2}')
exp_w=$(grep 'working' "$A/baseline-hashes.txt" | awk -F'= ' '{print $2}')
rc=0
[ "$d" = "$exp_d" ] || { echo "MISMATCH diff-of-test-file: now=$d baseline=$exp_d"; rc=1; }
[ "$h" = "$exp_h" ] || { echo "MISMATCH head-version: now=$h baseline=$exp_h"; rc=1; }
[ "$w" = "$exp_w" ] || { echo "MISMATCH working-file: now=$w baseline=$exp_w"; rc=1; }
[ "$rc" = 0 ] && echo "BASELINE-OK all three hashes match"
exit $rc

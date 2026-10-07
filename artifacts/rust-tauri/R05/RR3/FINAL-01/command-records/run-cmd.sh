#!/bin/bash
# RR3 FINAL-01 command runner: records argv/cwd/UTC/df/exit + raw stdout/stderr.
# Usage: run-cmd.sh <label> <argv...>
set -u
LABEL="$1"; shift
BASE="/private/tmp/rr3-final01-dir/$LABEL"
mkdir -p "$BASE"
REPO=/Users/study_superior/Desktop/Code/LingxiAgent
{
  echo "label: $LABEL"
  printf 'argv:'
  for a in "$@"; do printf ' %q' "$a"; done; echo
  echo "cwd: $REPO"
  echo "HOME: $HOME"
  echo "utcStart: $(date -u +%Y-%m-%dT%H:%M:%S.%NZ)"
  echo "unixStartMs: $(date -u +%s%3N 2>/dev/null || python3 -c 'import time;print(int(time.time()*1000))')"
  echo "dfBefore: $(df -g /System/Volumes/Data | tail -1)"
} > "$BASE/meta.txt"
cd "$REPO" || { echo "cd failed" >> "$BASE/meta.txt"; exit 125; }
"$@" > "$BASE/stdout.log" 2> "$BASE/stderr.log"
EC=$?
{
  echo "utcEnd: $(date -u +%Y-%m-%dT%H:%M:%S.%NZ)"
  echo "unixEndMs: $(date -u +%s%3N 2>/dev/null || python3 -c 'import time;print(int(time.time()*1000))')"
  echo "exit: $EC"
  echo "dfAfter: $(df -g /System/Volumes/Data | tail -1)"
} >> "$BASE/meta.txt"
echo "[$LABEL] exit=$EC end=$(date -u +%H:%M:%SZ)"
exit $EC

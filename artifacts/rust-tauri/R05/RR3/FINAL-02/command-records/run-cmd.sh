#!/bin/bash
# RR3 FINAL-02 command runner: records argv/cwd/UTC/df/exit + raw stdout/stderr.
# Usage: run-cmd.sh <label> <argv...>
set -u
LABEL="$1"; shift
BASE="/private/tmp/rr3-final02-dir/$LABEL"
mkdir -p "$BASE"
REPO=/Users/study_superior/Desktop/Code/LingxiAgent
{
  echo "label: $LABEL"
  printf 'argv:'
  for a in "$@"; do printf ' %q' "$a"; done; echo
  echo "cwd: $REPO"
  echo "HOME: $HOME"
  echo "user: $(whoami)"
  echo "utcStart: $(date -u +%Y-%m-%dT%H:%M:%S.%NZ)"
  echo "dfBefore: $(df -g /System/Volumes/Data | tail -1)"
} > "$BASE/meta.txt"
cd "$REPO" || { echo "cd failed" >> "$BASE/meta.txt"; exit 125; }
"$@" > "$BASE/stdout.log" 2> "$BASE/stderr.log"
EC=$?
{
  echo "utcEnd: $(date -u +%Y-%m-%dT%H:%M:%S.%NZ)"
  echo "exit: $EC"
  echo "stdoutBytes: $(wc -c < "$BASE/stdout.log" | tr -d ' ')"
  echo "stderrBytes: $(wc -c < "$BASE/stderr.log" | tr -d ' ')"
  echo "dfAfter: $(df -g /System/Volumes/Data | tail -1)"
} >> "$BASE/meta.txt"
echo "[$LABEL] exit=$EC end=$(date -u +%H:%M:%SZ)"
exit $EC

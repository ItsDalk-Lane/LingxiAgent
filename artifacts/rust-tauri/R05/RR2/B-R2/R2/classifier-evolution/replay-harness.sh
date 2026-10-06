#!/usr/bin/env bash
# RR2 B-R2 R2 classifier-evolution replay harness.
# Replays the classify_file path of r02_t08_legacy_entry_regression.sh
# (extracted verbatim from the edited script) against the real evidence
# logs, per failing block and per file, WITHOUT running the full gate.
set -uo pipefail
VDIR=/tmp/ptl-verify
REPO=/Users/study_superior/Desktop/Code/LingxiAgent
OUT="$REPO/artifacts/rust-tauri/R05/RR2/B-R2/R2/classifier-evolution"
mkdir -p "$OUT"
# shellcheck disable=SC1091
mkdir -p "$VDIR/ev"; EVIDENCE_DIR="$VDIR/ev"; source "$VDIR/cls-lib.sh"

run_name="$1"; log="$2"
res="$OUT/$run_name"
mkdir -p "$res"
copy_path="$(dirname "$log")"
extract_blocks "$log" "$copy_path" > "$res/blocks.txt"
{
  echo "== classifier-evolution replay: $run_name =="
  echo "log: $log"
  echo "script: scripts/rust-tauri/r02_t08_legacy_entry_regression.sh (working tree)"
  echo
  echo "== per-file classes (union of per-block verdicts) =="
} > "$res/verdicts.txt"
files="$(awk -F'\t' '{print $1}' "$res/blocks.txt" | awk 'NF' | sort -u)"
overall_unrecognized=0
for f in $files; do
  classes="$(classify_file "$res/blocks.txt" "$f" | sort | tr '\n' ',')"
  classes="${classes%,}"
  printf '%s\t%s\n' "$f" "$classes" >> "$res/verdicts.txt"
  case "$classes" in *UNRECOGNIZED*) overall_unrecognized=1 ;; esac
done
{
  echo
  block_unrec=""; echo "== per-block classes =="
  blocks="$(awk -F'\t' '{print $1"\t"$2}' "$res/blocks.txt" | awk -F'\t' '$2!="" && $1!=""' | sort -u -t$'\t' -k2,2n -k1,1)"
  while IFS=$'\t' read -r f b; do
    [ -n "$f" ] || continue
    awk -F'\t' -v f="$f" -v b="$b" '$1==f && $2==b' "$res/blocks.txt" > "$res/block-$b.tsv"
    bclasses="$(classify_file "$res/block-$b.tsv" "$f" | sort | tr '\n' ',')"
    bclasses="${bclasses%,}"
    head3="$(awk -F'\t' '$3!="" && $3!="⟦FAIL-BLOCK⟧"{print $3; c++} c>=1{exit}' "$res/block-$b.tsv" | cut -c1-90)"
    printf 'block %-2s %-45s classes=[%s]  first-payload: %s\n' "$b" "$f" "$bclasses" "$head3" >> "$res/verdicts.txt"
    case "$bclasses" in *UNRECOGNIZED*) block_unrec="$block_unrec $b" ;; esac
  done <<< "$blocks"
  echo
  if [ "$overall_unrecognized" = "0" ]; then
    echo "OVERALL: only registered-class failures (seal-coordinate-lag / uncommitted-source-rejection) — classified governance state, NOT fail-closed"
  else
    echo "OVERALL: UNRECOGNIZED present — gate would FAIL CLOSED (honest if a crash/foreign shape exists in this log)"
  fi
} >> "$res/verdicts.txt"
cat "$res/verdicts.txt"

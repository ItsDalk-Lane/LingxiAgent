#!/bin/zsh
# R01-T07 repair-r1 driver: rebuild all variant samples, snapshot each home
# before/after, run the production server probe, and diff the snapshots.
# Usage: run-all.zsh <run-tag>   (evidence lands under logs/ and snapshots/
# with the run-tag prefix so the pre- and post-doc-fix runs stay separate)
set -u
TAG="$1"
REPO="/Users/study_superior/Desktop/Code/LingxiAgent"
EV="$REPO/artifacts/rust-tauri/R01/T07/repair-r1"
HOMES="/tmp/lingxi-r01t07-repair-r1/homes"

node "$EV/scripts/build-samples.cjs"

# label:port pairs (ports unique to repair-r1, loopback only)
set -x
for SPEC in \
  "c1-blocked-control:19871" \
  "r7-corrupt-stamp-valid-barrier-journal:19872" \
  "r9-corrupt-journal-no-stamp:19873" \
  "r10-valid-barrier-journal-no-stamp:19874" \
  "r11-valid-prepared-journal-no-stamp:19875"
do
  set +x
  LABEL="${SPEC%%:*}"; PORT="${SPEC##*:}"
  HOME_DIR="$HOMES/$LABEL"
  # re-plant this variant's pristine sample before the run (homes are
  # disposable; samples/ keeps the authoritative planted copies)
  "$EV/scripts/snap.zsh" "$HOME_DIR" "$EV/snapshots/${TAG}-${LABEL}-before"
  "$EV/scripts/probe-variant.zsh" "${TAG}-${LABEL}" "$HOME_DIR" "$PORT"
  "$EV/scripts/snap.zsh" "$HOME_DIR" "$EV/snapshots/${TAG}-${LABEL}-after"
  if diff -q "$EV/snapshots/${TAG}-${LABEL}-before.listing" "$EV/snapshots/${TAG}-${LABEL}-after.listing" > /dev/null \
     && diff -q "$EV/snapshots/${TAG}-${LABEL}-before.sha256" "$EV/snapshots/${TAG}-${LABEL}-after.sha256" > /dev/null; then
    echo "FS-DIFF ${TAG}-${LABEL}: IDENTICAL" | tee "$EV/snapshots/${TAG}-${LABEL}-fsdiff.txt"
  else
    { echo "FS-DIFF ${TAG}-${LABEL}: CHANGED"; echo "--- listing diff ---"
      diff "$EV/snapshots/${TAG}-${LABEL}-before.listing" "$EV/snapshots/${TAG}-${LABEL}-after.listing" || true
      echo "--- sha256 diff ---"
      diff "$EV/snapshots/${TAG}-${LABEL}-before.sha256" "$EV/snapshots/${TAG}-${LABEL}-after.sha256" || true
    } | tee "$EV/snapshots/${TAG}-${LABEL}-fsdiff.txt"
  fi
  # count newly written entries as a fail-open severity measure
  NEW=$(comm -13 <(sort "$EV/snapshots/${TAG}-${LABEL}-before.listing") <(sort "$EV/snapshots/${TAG}-${LABEL}-after.listing") | grep -c '^F ' || true)
  echo "NEW-FILES ${TAG}-${LABEL}: $NEW" | tee -a "$EV/snapshots/${TAG}-${LABEL}-fsdiff.txt"
  set -x
done
set +x
echo "repair-r1 run-all ($TAG) complete"

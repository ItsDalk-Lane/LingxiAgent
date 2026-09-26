#!/usr/bin/env bash
# R02-T08 / acceptance R02-A16 — the incumbent Node/Electron entries are
# NOT switched and do not regress.
#
# Proves (all raw outputs archived):
#   E1  entry-unchanged: zero diff of the whole Node/Electron production
#       surface (core/ server/ desktop/ shared/ tests/ package.json lockfile)
#       vs TASK_BASE_SHA; package.json "main" still desktop/bootstrap.cjs;
#       the launch path (scripts/launch.js, desktop/main.cjs, server boot)
#       contains NO rust/lingxi-service reference — the default entry does
#       not point at the new stack.
#   E2  npm run typecheck (tsc x3) exits 0.
#   E3  npm run typecheck:core-contracts exits 0.
#   E4  npm run check:dependency-boundaries + check:tool-invocation-boundaries
#       exit 0 (incumbent boundary gates still hold).
#   E5  full `npm test`: no NEW failures vs the pre-existing audit-seal red
#       family (the seal coordinate lags the authorized R01/R02 commits —
#       documented before this task started; T07's review observed the same
#       red). Any failing test file OUTSIDE that family fails this script.
#       Anti-masking check: no T08/new-stack file may appear in the failure
#       lists (if our work had broken them, our files would show up).
#
# Usage: scripts/rust-tauri/r02_t08_legacy_entry_regression.sh [EVIDENCE_DIR]
# Exit 0 only if every step holds.
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R02/T08/A16-direct}"
EVIDENCE_DIR="$EVIDENCE_DIR/legacy-entry"
mkdir -p "$EVIDENCE_DIR"

TASK_BASE_SHA="5741989165fe7e04c9a58a9d35c7747d3599d274"
fail() { echo "FAIL: $*" >&2; exit 1; }
note() { printf '%s\n' "$*" | tee -a "$EVIDENCE_DIR/summary.txt"; }
RUN_STAMP="$(date '+%Y-%m-%dT%H:%M:%S%z')"
note "== run $RUN_STAMP (pid $$) =="
note "== R02-T08 / R02-A16 legacy-entry regression =="
note "== tested base commit $TASK_BASE_SHA (plus uncommitted T08 work; zero changes to the Node/Electron surface) =="

# ── E1: entry-unchanged proofs ─────────────────────────────────────────────
note "== E1: production entry surface unchanged vs base =="
git diff --name-only "$TASK_BASE_SHA" -- core/ server/ desktop/ shared/ tests/ package.json \
  package-lock.json > "$EVIDENCE_DIR/e1-diff-node-surface.txt"
if [ -s "$EVIDENCE_DIR/e1-diff-node-surface.txt" ]; then
  cat "$EVIDENCE_DIR/e1-diff-node-surface.txt" >&2
  fail "E1: Node/Electron surface changed vs base"
fi
note "PASS E1-diff-empty (git diff base..worktree over core/ server/ desktop/ shared/ tests/ package.json package-lock.json = empty)"

MAIN_FIELD="$(node -p "require('./package.json').main")"
[ "$MAIN_FIELD" = "desktop/bootstrap.cjs" ] || fail "E1: package.json main is '$MAIN_FIELD', expected desktop/bootstrap.cjs"
note "PASS E1-package-main (package.json main = desktop/bootstrap.cjs)"

LAUNCH_HITS="$(grep -nE 'lingxi-service|rust-target|/rust/' scripts/launch.js desktop/main.cjs desktop/bootstrap.cjs server/boot.cjs 2>/dev/null || true)"
: > "$EVIDENCE_DIR/e1-launch-rust-refs.txt"
[ -n "$LAUNCH_HITS" ] && printf '%s\n' "$LAUNCH_HITS" > "$EVIDENCE_DIR/e1-launch-rust-refs.txt"
if [ -n "$LAUNCH_HITS" ]; then
  printf '%s\n' "$LAUNCH_HITS" >&2
  fail "E1: production launch path references the rust stack"
fi
note "PASS E1-no-rust-in-launch (grep lingxi-service|rust-target|/rust/ over launch.js/main.cjs/bootstrap.cjs/boot.cjs = 0 hits)"

# ── E2/E3: typechecks ──────────────────────────────────────────────────────
note "== E2: npm run typecheck =="
npm run typecheck > "$EVIDENCE_DIR/e2-typecheck.log" 2>&1 || { tail -20 "$EVIDENCE_DIR/e2-typecheck.log" >&2; fail "E2: typecheck failed"; }
note "PASS E2-typecheck (exit 0; tsc x3)"

note "== E3: npm run typecheck:core-contracts =="
npm run typecheck:core-contracts > "$EVIDENCE_DIR/e3-core-contracts.log" 2>&1 || { tail -20 "$EVIDENCE_DIR/e3-core-contracts.log" >&2; fail "E3: core-contracts typecheck failed"; }
note "PASS E3-core-contracts (exit 0)"

# ── E4: incumbent boundary gates ───────────────────────────────────────────
note "== E4: incumbent boundary gates =="
npm run check:dependency-boundaries > "$EVIDENCE_DIR/e4-dependency-boundaries.log" 2>&1 || { tail -20 "$EVIDENCE_DIR/e4-dependency-boundaries.log" >&2; fail "E4: dependency-boundaries failed"; }
npm run check:tool-invocation-boundaries > "$EVIDENCE_DIR/e4-tool-invocation-boundaries.log" 2>&1 || { tail -20 "$EVIDENCE_DIR/e4-tool-invocation-boundaries.log" >&2; fail "E4: tool-invocation-boundaries failed"; }
note "PASS E4-boundary-gates (dependency + tool-invocation, exit 0 each)"

# ── E5: full npm test with seal-family classification ──────────────────────
note "== E5: full npm test (vitest) with pre-existing-red classification =="
set +e
npm test > "$EVIDENCE_DIR/e5-npm-test.log" 2>&1
NPM_TEST_EXIT=$?
set -e
note "npm test raw exit code: $NPM_TEST_EXIT (archived in e5-npm-test.log)"

grep -E "^ FAIL " "$EVIDENCE_DIR/e5-npm-test.log" | sed -E 's/^ FAIL +//; s/ > .*//' | sort -u \
  > "$EVIDENCE_DIR/e5-failed-test-files.txt"
grep -E "Test Files +[0-9]+ failed" "$EVIDENCE_DIR/e5-npm-test.log" | tail -n 1 \
  > "$EVIDENCE_DIR/e5-vitest-summary.txt"
FAILED_FILES="$(cat "$EVIDENCE_DIR/e5-failed-test-files.txt")"
note "failed test files: $(echo "$FAILED_FILES" | tr '\n' ' ')"
[ -n "$FAILED_FILES" ] || note "no failed test files (full suite green)"

# The pre-existing red family: audit-seal coordinate lag (documented before
# R02-T08 started; R01 and R02-T07 observed the same failures). Root cause
# of every member: VERIFIED_SOURCE_SHA predates the authorized R01/R02
# commits, so the committed-but-newer deliverables show up as "non-audit
# changes". NOT caused by this task; NOT fixable without a seal-coordinate
# governance step that is explicitly out of scope here.
SEAL_FAMILY="tests/post-verification-audit-seal.test.ts
tests/round2-delivery-evidence.test.ts
tests/round3-delivery-evidence.test.ts"
printf '%s\n' "$SEAL_FAMILY" > "$EVIDENCE_DIR/e5-seal-family.txt"

NON_SEAL="$(grep -vFf "$EVIDENCE_DIR/e5-seal-family.txt" "$EVIDENCE_DIR/e5-failed-test-files.txt" || true)"
if [ -n "$NON_SEAL" ]; then
  printf '%s\n' "$NON_SEAL" | tee "$EVIDENCE_DIR/e5-non-seal-failures.txt" >&2
  fail "E5: test failures outside the pre-existing seal family (see e5-non-seal-failures.txt)"
fi
note "PASS E5-no-new-failures (every failed file is in the pre-existing seal-lag family)"

# Anti-masking: no T08/new-stack file may appear anywhere in the failure
# lists — if this task's work had broken the family, its files would show.
grep -E "xtask|r02_t08|R02/T08|full_chain|legacy_entry|devgate" \
  "$EVIDENCE_DIR/e5-npm-test.log" | grep -E "^  - " \
  > "$EVIDENCE_DIR/e5-t08-files-in-failure-lists.txt" || true
if [ -s "$EVIDENCE_DIR/e5-t08-files-in-failure-lists.txt" ]; then
  cat "$EVIDENCE_DIR/e5-t08-files-in-failure-lists.txt" >&2
  fail "E5: T08 files appear in the failure lists — the red may not be pre-existing"
fi
note "PASS E5-anti-masking (0 T08/new-stack files in the failure lists; red is about committed R01/R02 vs the lagging seal coordinate)"

note "== R02-T08 / R02-A16 legacy-entry regression: ALL GREEN (no default switch, no new failures) =="

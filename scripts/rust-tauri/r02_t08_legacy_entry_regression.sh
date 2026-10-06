#!/usr/bin/env bash
# R02-T08 / acceptance R02-A16 — the incumbent Node/Electron entries are
# NOT switched and do not regress.
#
# R02 stage-repair R1 rewrite (review finding F06). The previous version
# pinned TASK_BASE to the executor's pre-commit moment ("plus uncommitted
# T08 work") and inferred behaviour attribution from file NAMES appearing
# in the seal tests' diff listing — which broke the moment T08 was
# committed, because the seal-coordinate-lag failures legitimately
# enumerate R02/T08 paths. This rewrite binds the comparison to explicit
# SHAs and replays the failing family at BOTH ends of the range.
#
# R02 FINAL CLOSEOUT (repair-group2, 2026-09-28): E1 semantics corrected to
# the governing A16 definition — "the incumbent Node/Electron PRODUCTION
# DEFAULT entry is not switched and no new client-affecting regressions
# appeared" — replacing the pre-closeout predicate "zero diff over the
# production surface / zero rust references in the launch path", which
# permanently failed the gate once the AUTHORIZED opt-in client wiring
# landed (53-file surface diff; guarded rust references in main.cjs that
# are pure serverNodeKind guards/publication points). E1a/E1b are now
# RECORD-ONLY evidence of the authorized wiring range; E1c plus the new
# E1d default-entry assertion set carry the gate. E5's candidate-red
# whitelist is generalized to "baseline-replay reds ∪ REGISTERED
# pre-existing failure families" (an explicit registration ledger; today
# it equals the seal coordinate-lag trio — the frozen VERIFIED_SOURCE_SHA
# predates the authorized R01/R02 commits, a seal-workflow coordinate lag
# per PROGRESS.md's seal process, not an R02 behavior regression). E4.5
# adds the renderer build to the regression surface (the authorized wiring
# touches renderer sources). The R1-R11 repair history is preserved; only
# the current-behaviour bullets (E1/E5) are updated to match. Evidence
# basis: /tmp/r02-final/a16-audit-r1.md (A16-AUDITOR-R1).
#
# R02 FINAL CLOSEOUT repair-group5 (E0 self-reference binding defect,
# 2026-09-28): the final-candidate verify stage runs this gate with an
# IN-REPO evidence root (artifacts/rust-tauri/R02/final-candidate-<id>/
# A16/legacy-entry), and the gate itself writes there BETWEEN the two
# worktree bindings — summary.txt appends, the binding tsv going from
# empty (the shell redirect creates it before python populates it) to
# full, the e0-candidate-dirty.* captures — so the exhaustive binding
# could never mirror byte-identically: the main-repo scan and the
# candidate-copy scan differed BY CONSTRUCTION (first real evidence:
# final-candidate-a0af83666f89cc4b, "candidate copy does not mirror the
# invoking worktree"). The chain had only ever been green with
# out-of-repo /tmp evidence roots. Fix: both bindings exclude EXACTLY
# this gate's own evidence subtree ($EVIDENCE_DIR — repo-relative prefix
# computed ONCE and passed UNCHANGED to both scans), record the active
# exclusion as `#` header rows inside both tsv files, and keep EVERYTHING
# ELSE fully bound: the other verify-stage evidence directories are
# static during this gate's run and stay in the binding; the runner's
# stdout/stderr captures are gitignored (*.log) and never were part of
# it; candidate source never lives under this gate's own output
# directory; and the orchestrator's xtask candidate binding (full-tree
# SHA over the candidate) remains the outer backstop. The R4-F02 lesson
# is unchanged — the candidate SOURCE binding stays complete; the only
# invisible paths are this run's own OUTPUT. The E0s fixtures prove the
# exclusion surgical: excluded-subtree changes are invisible, a content
# change ANYWHERE else still flips the binding, and without the prefix
# the binding stays exhaustive.
#
# R02 FINAL CLOSEOUT repair-group10 (gate round 2, 2026-09-28/29): E5(3)
# registered the guard-tail WINDOW form. Gate round 2's candidate run
# failed E5 with `UNRECOGNIZED,seal-coordinate-lag,
# uncommitted-source-rejection` on round2/round3 — those blocks embed the
# generators' `post-verification diff guard failed: {guard_output[-500:]}`
# line, and with this repo's ~2100-violator listing the window carries NO
# ✗ sentence at all (front-cut path fragment + `  - ` listing lines,
# ending mid-token where the guard's own console.error output was lost
# past the 64 KiB pipe buffer at process.exit — the captured guard part
# is byte-exactly 65536). Both reds' true causes are still the two
# REGISTERED ones (seal coordinate lag + dirty candidate tree); the
# shapes were simply absent from the ledger. Registered PRECISELY, no
# generalization: generator-wrapper producer binding in the same block,
# printable-ASCII no-space/no-colon fragment (every guard sentence suffix
# carries a space or the full-width `）` — sentence material can never
# pose as a path fragment), at least one complete `  - ` listing line,
# and the reconstructed window EXACTLY 500 code points (the literal
# [-500:] slice; all-ASCII by grammar so the length is locale-proof).
# Evidence: artifacts/rust-tauri/R02/final-candidate-a29e5adbb404ad70/
# A16/legacy-entry/e5-candidate-blocks.txt (round2 block 4, round3 block
# 6) + /tmp/r02-final/repair-group10-r1.md. Eight E0s fixtures pin it:
# the two real gate-r2 windows and a derived complete-end variant
# classify; five single-point mutations (off-by-one char, trailing junk,
# sentence fragment, wrong producer, deleted line) all stay UNRECOGNIZED.
# The bare `.py` tail and mid-sentence cuts of R6-F01 REMAIN unregistered
# — fail-closed unchanged.
#
# R05 RR2 F42 (WP B, 2026-10-06): run-output attribution. d80737b6's
# evidence-distribution .gitignore rules (!artifacts/rust-tauri/**/*.log
# etc.) UN-IGNORE every runner log inside the repo, so under the nested
# verify-stage chain (verify-stage R05 → R04 → R03 → this gate) the
# stdout/stderr captures that carry THIS gate's output flow — the gate's
# own sinks <evidence>/r02_legacy_regression/{stdout,stderr}.log plus
# every ancestor runner's sink (.../r03_regression_gate/stdout.log,
# .../r04_regression_gate/stdout.log) — became unignored binding content
# that keeps growing after the main binding, breaking the E0 mirror cmp
# BY CONSTRUCTION (RR1 INDEPENDENT-9: four live .log rows in the failing
# binding; the REVIEW-T08-era chain passed only because c549ff654's
# .gitignore still ignored *.log). The repair-group5 exclusion covered
# the gate's evidence subtree alone — too narrow once the ignore rule
# changed. The gate now attributes "this run's ENTIRE run-time output"
# EXPLICITLY and symmetrically: it discovers the fd-1/fd-2 FILE sinks of
# its own process tree (self + ancestors — exactly the files that
# receive this run's output flow; /proc fast path, lsof fallback on
# darwin) and attributes each in-repo sink under FENCES, never by trust:
#   • DIR unit  — the sink's command directory, ONLY when it is a
#     DEDICATED fresh run-output directory: strictly inside artifacts/
#     at run-root depth (>= artifacts/<area>/<stage>/… — `artifacts`,
#     `artifacts/rust-tauri` or a bare stage dir can never be a unit:
#     excluding the whole evidence tree is forbidden), carries no
#     index-tracked content, and EVERY file currently under it is
#     attributable to THIS run (itself a discovered sink, inside the
#     gate's own evidence subtree, or inside another sink directory) —
#     a directory that already holds foreign/previous-run files is NOT
#     dedicated and never becomes a unit;
#   • FILE unit — otherwise exactly the sink file alone (minimal
#     attribution: nothing wider is excluded, everything around the
#     sink stays fully bound).
# R02_A16_RUN_OUTPUT_ROOTS may declare additional repo-relative run
# roots — an explicit operator attestation, fenced the same way
# (artifacts/-inside, run-root depth, existing directory, no tracked
# content). Sinks redirected ONTO index-tracked content are refused
# outright (that would corrupt bound candidate/evidence state); if
# neither /proc nor lsof can resolve ANY pid the gate fails closed
# rather than silently degrading to the F42 failure. The exclusion can
# never mask candidate source or committed/old evidence: mid-binding
# source changes, new files, deletions and old STATIC evidence changes
# OUTSIDE the attributed units still fail the mirror cmp, and an
# illegal root (rust/, the whole artifacts tree, anything tracked, any
# too-shallow artifacts dir) is REJECTED. The unit set is computed ONCE
# before the first binding, recorded auditable in
# e0-run-output-sinks.txt / e0-binding-exclusions.txt, and passed
# UNCHANGED to both bindings. An evidence root outside the repository
# keeps the exhaustive no-exclusion binding (the historical /tmp form).
# Concurrent sibling writers OUTSIDE the attributed units remain what
# they always were: copy-race drift that fails the mirror cmp.
#
# R05 RR2 B-R2 R2 (WP B round 2, 2026-10-06): E5 FORM-EVOLUTION
# registration — the generators' patch-too-large replay crash. The two
# generator-wrapped block positions registered by repair-group10
# (round2 R10-09 replay / round3 live replay) have been RED at every
# candidate since the R02 closeout; the ONLY full-E5 runs that ever
# passed E5-cause-classification accepted them via the registered
# guard-tail WINDOW + firstDiff forms (R02 final-candidate-
# fc9971898df7a9e8 / e876171c5cd0a6f8). As the tree kept growing, the
# regenerated delivery patch crossed git's OWN apply ceiling (git
# apply.c MAX_APPLY_SIZE = 1024*1024*1024-1 ≈ 1023 MiB; F44 measured
# `git diff --binary --full-index 67dee5d2 HEAD` = 3,671,013,696 B
# uncompressed, committed face only), so `git apply --cached` refuses
# with `error: patch too large` before the generators reach their
# guard/uncommitted checkpoints: create-delivery-patch.py:359 raises
# RuntimeError(f"patch replay failed: {apply.stdout…[:2000]}") — an
# UNCAUGHT exception, python prints its 6-row traceback (frames 457 in
# <module> / 359 in replay_and_verify) ending in the complete sentence —
# while create-round3-patch.py:342 raises SystemExit with the same
# message, which python prints BARE (no traceback). The round3 form has
# been visible since the R03 verify-stage chains (round3 block 6/7
# there) and was honestly UNRECOGNIZED — every chain that reached
# classification failed E5 fail-closed, and the R04/R05 REVIEW-T08
# FINAL-R3 chain ran this gate DIRECTED (E5 skipped by scope). F44
# (RR2) removed the ENOBUFS crash that masked the sibling blocks' true
# seal-lag shapes, leaving exactly these two blocks as the remaining E5
# reds (s5-full-4 evidence). Registered as the FIFTH seal-lag shape —
# the SAME registered family's evolved form (the same governance-lag
# tree growth that produced the guard's ~2100-violator listing now
# makes the full patch unreplayable by git itself), still a REGISTERED
# RED: the raw npm exit stays 1, recorded loudly, never formal green.
# Anchored end-to-end to the producers' exact geometry, no substring
# accepted:
#   • producer binding: ONLY inside a block whose FIRST payload row is
#     the GENERATOR wrapper, and each form binds to ITS OWN generator —
#     round2: the complete CONSECUTIVE 6-row traceback whose two File
#     frames cite the WRAPPED script (line 457 in <module> / line 359
#     in replay_and_verify) with their real source-context rows, ending
#     in the complete sentence `RuntimeError: patch replay failed:
#     error: patch too large`; round3: the bare line `patch replay
#     failed: error: patch too large` (SystemExit message). A bare
#     sentence under the round2 wrapper, the traceback under the round3
#     wrapper, or either shape under the node-guard wrapper is NOT this
#     class;
#   • the sentence must end with git's complete refusal text
#     `error: patch too large` — any OTHER git error (corrupt patch,
#     unrecognized input, …), a truncated sentence, a mutated frame, or
#     a bare wrapper is UNRECOGNIZED and fails the gate (fail closed,
#     R6-F01 discipline).
# Evidence: artifacts/rust-tauri/R05/RR2/B-R2/R2/s5-full-4/ev/
# legacy-entry/e5-candidate-blocks.txt (round2 block 4, round3 block 6)
# + artifacts/rust-tauri/R05/RR2/F44/f44-failure-blocks.txt + the
# classifier-evolution/ replay. Nine E0s fixtures pin it (two REAL
# positives; seven negatives — truncated sentence, other git error,
# foreign SystemExit text, cross-producer both ways, mutated frame,
# node-guard producer).
#
# R02 stage-repair R4 rewrite (review finding R4-F02). The R1 rewrite still
# built the "baseline" by copying the dirty candidate worktree and running
# `git checkout -f BASE`: that restores TRACKED files only, so untracked
# candidate sources leaked into the baseline, the baseline's seal-family
# replay then failed with the generators' dirty-tree refusal
# ("current source manifest does not match HEAD"), and the classifier —
# which read only the FIRST outer `Error: Command failed: ...` wrapper
# line — filed that pollution as "known seal-coordinate lag". Both halves
# are fixed here, generically (no hardcoded file list):
#
#   E0  coordinate binding: BASE_SHA must be an ancestor of the candidate
#       (HEAD). The candidate worktree state is bound COMPLETELY — every
#       tracked modification AND every untracked (non-ignored) file with
#       its content SHA-256, never `--untracked-files=no` — and the
#       candidate copy must reproduce that binding byte-identically.
#       (repair-group5: the ONE exclusion is this gate's own evidence
#       subtree, applied symmetrically to both scans — see the
#       bind_worktree header; everything else, including every other
#       verify-stage evidence directory, remains fully bound.)
#   E0b baseline purity (R5 workflow + R5-F01 rework): the ORIGINAL user
#       attachment prohibits `git reset --hard` and `git clean -fd` with
#       NO throwaway-copy exception, and the R4 construction used exactly
#       those inside the copy. The baseline is now BUILT FRESH from exact
#       git content instead of inherit-then-clean: the copy gets its own
#       CoW-cloned .git store with the stale index re-initialized (the one
#       file removed is the copy's own stale metadata, never a worktree
#       path), a NON-forced `checkout --detach BASE_SHA` over an EMPTY
#       worktree (nothing to overwrite, nothing to delete — the copy never
#       contains the candidate files at all), and an isolated CoW copy of
#       node_modules (ignored deps reused, never written back to the
#       invoking repo). Before anything runs, the baseline must prove it
#       is a pristine checkout: HEAD == BASE_SHA, index clean,
#       `git diff BASE_SHA` empty, and
#       `git status --porcelain --untracked-files=all` EMPTY. Any residue
#       fails the gate loudly instead of poisoning the replay. No
#       `checkout -f`, no `clean -fd`, no commit anywhere in this gate.
#   E0s gate self-checks: the classifier and the worktree binding are
#       exercised against synthetic fixtures FIRST — a clean-baseline
#       shape, a real seal-coordinate-lag shape, an uncommitted-source
#       rejection, a generator crash, the bare `Error: Command failed`
#       wrapper with no diagnostics (the exact R4-F02 false-green shape),
#       a cross-block mix (R5-F01: a recognized block must NOT absorb an
#       unrecognized block in the same file), a non-coordinate guard
#       refusal and a TRUNCATED guard line (R5-F01: the guard prefix
#       alone proves nothing — only the complete non-audit-change
#       diagnostic classifies as seal lag), and a green no-failure log
#       (the legal all-green state) must each produce their distinct
#       verdicts; the binding must detect added/changed untracked files
#       in a scratch repo WITHOUT any commit (R5 workflow: no scratch
#       commits either). A broken gate fails fast here, before any npm
#       run. The baseline-construction proof is the E0 purity assert on
#       the real freshly-built copy itself.
#       R6-F01: the R5 "registered-but-accepted" class
#       seal-guard-refusal-unreadable is GONE — a guard refusal whose
#       line carries anything but the COMPLETE non-audit-change
#       diagnostic is UNRECOGNIZED and fails the gate at BOTH ends. The
#       fixtures now carry every REAL guard failure sentence (missing
#       coordinate file, non-hex coordinate, non-commit object,
#       `git diff ..HEAD 失败`, the generator's truncated tail window)
#       plus the zero-payload block shapes (a FAIL header followed
#       immediately by a stack frame, with and without a suite path) —
#       each must be REJECTED. Block-count consistency (every failed
#       file has blocks, every failed file has non-empty classes) is
#       asserted before any verdict.
#       R7-F01: family membership is now EXACT WHOLE-LINE identity — the
#       old `grep -vFf` was a substring filter, so the legitimate vitest
#       name `tests/post-verification-audit-seal.test.ts.regression.test.ts`
#       (NOT a family member) contained a member as a substring and was
#       accepted. A failing file passes only when its COMPLETE path
#       equals one of the three documented members. Per-line
#       classification is now COMPLETE-STRUCTURE: a line explains a
#       block ONLY when it is EXACTLY one of the real producers'
#       complete diagnostic shapes (anchored end-to-end); a line that
#       carries a complete known sentence PLUS anything else (e.g. an
#       appended `Error: EACCES: permission denied`) sets the known
#       cause AND is foreign — the known sentence no longer `continue`s
#       past the rest of the line, and a same-block `fatal: …` line
#       (outside every legitimate shape) is foreign instead of body
#       noise. parseable_run additionally requires an exit-0 log to
#       actually carry the vitest completion summary (an empty exit-0
#       log is UNPARSEABLE, not GREEN — evidence completeness). New
#       negative controls: the exact-suffix non-member, the same-block
#       fatal, the same-line mixed error, the exit-0-empty-log, and the
#       REAL direct-guard positive shape; the R6 guard-direct-full
#       fixture's synthetic `AssertionError: expected guard to be green`
#       lead-in is corrected to its honest verdict (seal + UNRECOGNIZED —
#       no real producer emits that lead-in; see the fixture comment).
#       R8-F01: no structural category is a pass-through any more, and
#       every body line is BOUND TO ITS PRODUCER inside the same block:
#       a no-space single token containing `:` (`Error:EACCES`) is not a
#       violator path; a `  - path` listing, a git warning, a vitest
#       matcher summary, or a wrapper line with ANY trailing content is
#       FOREIGN; the wrapper is structural only as the FIRST payload row
#       and only for the three commands the seal family actually spawns;
#       bare-path / diff-body / matcher lines are body only inside a
#       block whose COMPLETE AssertionError lead is present, and `  - `
#       listings only under a complete guard line. parseable_run now
#       proves the COMPLETE summary (both Test Files AND Tests lines,
#       full `N <word> | … (T)` grammar, components summing to the total)
#       and its agreement with the exit code and the FAIL-block count:
#       exit 0 + `Test Files 1 failed (1)` is CONTRADICTORY and a bare
#       truncated `Test Files 1` is UNPARSEABLE — neither is GREEN.
#   E1  default-entry-not-switched (final-closeout semantics, see the
#       closeout block above): the surface diff vs BASE and the candidate
#       binding (which sees untracked files that git diff cannot)
#       intersected with the same surface are archived as the
#       authorized-wiring audit trail (RECORD ONLY — never gated). The
#       GATED assertions are: package.json "main" still
#       desktop/bootstrap.cjs; the runtime selector defaults to node
#       (rustDesktopEnabled: unset/node → false, explicit rust → true);
#       nothing in the launch/packaging chain injects
#       LINGXI_DESKTOP_SERVER_RUNTIME; main.cjs keeps the
#       rustDesktopEnabled() startServer guard AND the incumbent Node
#       server body (server-info.json handling); bootstrap still loads
#       main(.bundle).cjs by isPackaged; cli/args.ts defaults runtime
#       "node"; the data-root no-double-write guards
#       (RUST_DESKTOP_NODE_SERVER_INFO_PRESENT mutex + rust
#       RUNTIME_DIR_NAME layout) are present.
#   E2/E3/E4  npm run typecheck (tsc x3), typecheck:core-contracts,
#       check:dependency-boundaries + check:tool-invocation-boundaries —
#       executed INSIDE the candidate copy.
#   E4.5  npm run build:renderer (final closeout): the authorized client
#       wiring touches renderer sources, so the renderer build is part of
#       the no-new-regressions surface — executed INSIDE the candidate
#       copy.
#   E5  failure attribution by REPLAY, with CONTENT-BASED cause
#       classification:
#       - the full npm test runs in the candidate copy;
#       - the three documented seal-family test files run again in the
#         pristine BASE_SHA copy;
#       - candidate failing files must be a subset of the BASELINE
#         REPLAY's failing files ∪ REGISTERED pre-existing failure
#         families (an explicit registration ledger, currently the seal
#         trio — a red OUTSIDE that union always fails; registration is
#         a documented audit decision, never a way to absorb a new red);
#       - parseability (R5-F01): a non-zero npm exit whose log shows
#         neither a vitest "Test Files" summary nor any parsable FAIL
#         block is an UNPARSEABLE failure — the gate fails closed instead
#         of classifying nothing; an exit 0 that still shows FAIL blocks
#         is a contradictory log and also fails;
#       - every failing test BLOCK is classified INDEPENDENTLY from the
#         actual generator/guard diagnostics embedded in that block (the
#         subprocess stderr lines that follow the `Error: Command failed:`
#         wrapper), never from the wrapper alone and never from another
#         block of the same file (R5-F01: per-file aggregation let a
#         second, unrecognized failure hide behind a recognized one).
#         Recognized classes:
#           seal-coordinate-lag          the audit guard found COMMITTED
#                                        non-audit changes after the frozen
#                                        VERIFIED_SOURCE_SHA — legitimate
#                                        mid-stage governance lag. A
#                                        "post-verification diff guard
#                                        failed:" line counts ONLY when it
#                                        carries the COMPLETE non-audit-
#                                        change diagnostic, and so does a
#                                        direct guard `✗` line carrying it;
#                                        repair-group10: the guard-tail
#                                        WINDOW also counts — the
#                                        generators' guard_output[-500:]
#                                        slice over a listing longer than
#                                        the window (front-cut printable-
#                                        ASCII path fragment + ≥1 complete
#                                        `  - ` listing line + exactly 500
#                                        code points reconstructed, ONLY
#                                        inside a block whose first payload
#                                        row is a GENERATOR wrapper; gate-r2
#                                        round2 block 4 / round3 block 6);
#                                        RR2 B-R2 R2 form evolution: the
#                                        generators' patch-too-large replay
#                                        crash is the FIFTH shape — the
#                                        tree outgrew git's own apply
#                                        ceiling (MAX_APPLY_SIZE ≈ 1023 MiB
#                                        vs a ≥3.67 GB regenerated patch),
#                                        so round2's complete 6-row
#                                        RuntimeError traceback (File frames
#                                        bound to the wrapped generator:
#                                        line 457 <module> / 359
#                                        replay_and_verify) ending in
#                                        `RuntimeError: patch replay failed:
#                                        error: patch too large`, and
#                                        round3's bare SystemExit line
#                                        `patch replay failed: error: patch
#                                        too large`, each ONLY under ITS
#                                        OWN generator wrapper, classify
#                                        here (same registered family, same
#                                        governance-lag cause; still a
#                                        registered RED, never green);
#                                        ANY other guard reason or payload
#                                        shape (R6-F01: missing coordinate
#                                        file, non-hex / non-commit
#                                        coordinate, `git diff ..HEAD 失败`,
#                                        permission failure, a TRUNCATED
#                                        sentence, a bare or geometry-
#                                        violating tail window) is
#                                        NOT this class — it is
#                                        UNRECOGNIZED and fails the gate
#                                        at both ends (fail closed; the
#                                        refusal proves nothing about its
#                                        cause);
#           uncommitted-source-rejection a patch generator refused because
#                                        the tree under test is not the
#                                        committed HEAD. Expected at the
#                                        candidate ONLY while the worktree
#                                        is dirty (registered separately,
#                                        never called green); at the base
#                                        it proves baseline pollution and
#                                        FAILS the gate.
#       - anything else — a bare wrapper with no diagnostics, a python
#         Traceback (RR2: other than the ONE registered round2
#         patch-too-large traceback shape above), a foreign Error line,
#         any other generator/guard
#         refusal, an empty or zero-payload block — is UNRECOGNIZED and
#         fails the gate CLOSED. The outer `Error: Command failed: ...`
#         line by itself NEVER passes; a recognized sibling block NEVER
#         absorbs it; a guard line with an unreadable/truncated reason
#         NEVER passes as "registered" (R6-F01: no honest-label class may
#         take a gate green — the cause must be COMPLETE and PRECISE or
#         the gate fails);
#       - per-block base/candidate class sets are archived for audit;
#       - reds at base that are green at candidate are recorded loudly;
#       - the raw npm exit codes stay visible in the summary: a classified
#         red is registered, not repainted as formal green.
#
# ISOLATION (orchestrator constraint): every command that can regenerate
# delivery artifacts — the seal tests execute the round2/round3 patch
# scripts, which rewrite delivery evidence — runs in throwaway copies of
# this repo under $TMPDIR. The invoking worktree is used for git reads
# ONLY; nothing here builds or tests inside it. R5: the copies are BUILT
# FRESH from exact git content (no `checkout -f`, no `clean -fd`, no
# commit, no reset — the original user attachment prohibits those with no
# throwaway-copy exception), so no destructive git operation exists in
# this gate at all and nothing of the invoking worktree is ever deleted.
#
# This script is a REGRESSION gate, not a seal: PASS means "the production
# DEFAULT entry is not switched (Node/Electron incumbent default, asserted
# structurally by E1c+E1d) and no new test failures appeared at the
# candidate vs the baseline replay ∪ registered pre-existing failure
# families". It never asserts an audit-seal PASS; the formal seal remains
# the orchestrator's governance step, and any rehearsal SHA recorded here
# is a regression-attribution coordinate, not a seal coordinate.
#
# Usage: scripts/rust-tauri/r02_t08_legacy_entry_regression.sh [EVIDENCE_DIR]
# Env:   R02_A16_BASE_SHA  overrides the baseline commit.
#        R02_A16_RUN_OUTPUT_ROOTS  optional ':'-separated repo-relative
#             run-output roots (F42): each must be an EXISTING dedicated
#             directory strictly inside artifacts/ at run-root depth
#             (>= artifacts/<area>/<stage>/…) with no index-tracked
#             content — an illegal root (rust/, artifacts itself, a bare
#             stage dir, anything tracked) is rejected, never silently
#             widened.
#        R02_LEGACY_REGRESSION_MODE  full | directed-no-seal-family.
# Exit 0 only if every step holds.
set -euo pipefail
cd "$(dirname "$0")/../.."
MAIN_REPO="$(pwd -P)"

fail() { echo "FAIL: $*" >&2; exit 1; }
EVIDENCE_ROOT="${1:-artifacts/rust-tauri/R02/T08/A16-direct}"
# 每次运行只写全新的证据根；复跑必须换路径，不能覆盖首次失败记录。
if [ -e "$EVIDENCE_ROOT" ] || [ -L "$EVIDENCE_ROOT" ]; then
  fail "evidence root already exists (or is a symlink): $EVIDENCE_ROOT"
fi
mkdir -p "$(dirname "$EVIDENCE_ROOT")"
mkdir "$EVIDENCE_ROOT" || fail "cannot create a new evidence root: $EVIDENCE_ROOT"
EVIDENCE_DIR="$EVIDENCE_ROOT/legacy-entry"
mkdir "$EVIDENCE_DIR" || fail "cannot create a new legacy-entry evidence directory: $EVIDENCE_DIR"
EVIDENCE_DIR="$(cd "$EVIDENCE_DIR" && pwd -P)"

# R02 stage base: parent of the first R02 commit (9d26b5aa9 "R02-T01").
BASE_SHA="${R02_A16_BASE_SHA:-201584f2917a7fd96d6ea603bdeddbd420082cfe}"
CANDIDATE_SHA="$(git rev-parse HEAD)"

note() { printf '%s\n' "$*" | tee -a "$EVIDENCE_DIR/summary.txt"; }
RUN_STAMP="$(date '+%Y-%m-%dT%H:%M:%S%z')"
note "== run $RUN_STAMP (pid $$) =="
note "== R02-T08 / R02-A16 legacy-entry regression (SHA-bound baseline/candidate replay, pristine baseline, content-based cause classes) =="
note "base SHA:      $BASE_SHA"
note "candidate SHA: $CANDIDATE_SHA"
note "scope: regression gate only — PASS here is NOT the formal audit seal (orchestrator step)"

git merge-base --is-ancestor "$BASE_SHA" "$CANDIDATE_SHA" \
  || fail "E0: base $BASE_SHA is not an ancestor of candidate $CANDIDATE_SHA"
note "PASS E0-ancestry (base is an ancestor of candidate)"

# ── Complete worktree binding (tracked modifications AND untracked content) ─
# bind_worktree <repo> <out.tsv> [exclude-units]: one line per non-clean
# path — `<sha256|->  <XY>  <path>` sorted by path. Untracked (non-ignored)
# files are included with their content hash; renames record the new path.
# This is the full candidate state — never git's tracked-only view
# (R4-F02: `--untracked-files=no` let four new candidate files slip past
# the binding and into the baseline).
#
# R02 final-closeout repair-group5 + R05 RR2 F42: the optional
# exclude-units argument is the NEWLINE-SEPARATED set of THIS RUN's
# run-time output units (repo-relative run-output DIRECTORIES or single
# output FILES) — the gate's own evidence subtree plus every
# attributed/declared run-output unit (see the F42 header block and the
# discovery below). Paths under (or equal to) a unit are this
# run's OUTPUT (runner evidence), never candidate source; both bindings
# exclude the SAME unit set symmetrically, and every unit is recorded as
# a `#` header row in BOTH tsv files so the compared artifacts stay
# self-describing and auditable. Units are validated before use (no
# index-tracked content under them — candidate source and committed
# evidence stay fully bound); every OTHER path, including static
# verify-stage evidence of PAST runs, stays exhaustively bound. With no
# third argument (the E0s scratch fixtures) or an empty string (evidence
# root outside the repository, the historical /tmp layout) the binding
# stays exhaustive and emits no header row.
bind_worktree() {
  python3 - "$1" "${3:-}" > "$2" <<'PYBIND'
import hashlib
import os
import subprocess
import sys

repo = sys.argv[1]
exclude_units = [u for u in (sys.argv[2] if len(sys.argv) > 2 else "").split("\n") if u]
raw = subprocess.run(
    ["git", "-C", repo, "status", "--porcelain=v1", "-z", "--untracked-files=all"],
    check=True, capture_output=True,
).stdout
records = raw.split(b"\0")
rows = []
i = 0
while i < len(records):
    rec = records[i]
    i += 1
    if not rec:
        continue
    xy = rec[:2].decode("ascii", "replace")
    path = rec[3:].decode("utf-8", "surrogateescape")
    if "R" in xy or "C" in xy:
        # rename/copy records carry the original path in the next record
        i += 1
    if any(path == unit or path.startswith(unit + "/") for unit in exclude_units):
        # this run's own run-time output root (runner output, never
        # candidate source) — excluded SYMMETRICALLY from every binding
        # that passes the same unit set (repair-group5 / F42; see the
        # bind_worktree header)
        continue
    digest = "-"
    full = os.path.join(repo, path)
    if os.path.isfile(full) and not os.path.islink(full):
        with open(full, "rb") as fh:
            digest = hashlib.sha256(fh.read()).hexdigest()
    rows.append((path, xy, digest))
rows.sort()
for unit in exclude_units:
    print(f"# binding-exclusion: {unit}/** — this run's run-time output root (runner output, never candidate source); applied symmetrically to BOTH the main-repo binding and the candidate-copy binding (E0 mirror compare)")
for path, xy, digest in rows:
    print(f"{digest}  {xy}  {path}")
PYBIND
}

# The gate's own evidence subtree relative to the repo root (repair-group5):
# computed ONCE from the two canonical (pwd -P) paths. Empty when the
# evidence root lives OUTSIDE the repository — there the binding stays
# exhaustive for this part.
EVIDENCE_REL_PREFIX="$(python3 - "$MAIN_REPO" "$EVIDENCE_DIR" <<'PYREL'
import os
import sys

rel = os.path.relpath(sys.argv[2], sys.argv[1])
if rel == "." or rel == ".." or rel.startswith(".." + os.sep):
    print("")
else:
    print(rel)
PYREL
)"

# ── F42 (R05 RR2 WP B): attribute THIS run's run-time output roots ──────────
# validate_run_output_unit <repo> <unit>: 0 = legal fresh output root (no
# index-tracked content under it — candidate source and committed evidence
# must stay bound), 1 = illegal (reason on stderr). Index-based
# (git ls-files): works on the no-commit E0s scratch repo too.
validate_run_output_unit() {
  local repo="$1" unit="$2"
  case "$unit" in
    ""|.|..|../*|/*)
      printf 'not a repo-relative path (repo root / parent / absolute paths can never be exclusion roots)\n' >&2
      return 1
      ;;
  esac
  if [ -n "$(git -C "$repo" ls-files -- "$unit")" ]; then
    printf 'covers INDEX-TRACKED content (candidate source / committed evidence must stay bound)\n' >&2
    return 1
  fi
  return 0
}

# discover_run_output_sinks: one `<KIND> <repo-relative-path>` row per
# fd-1/fd-2 FILE sink of this process tree (self + ancestors — exactly the
# files that receive this run's output flow) that lives inside this repo:
#   DIR <path>         the sink's command directory — ONLY when it is a
#                      DEDICATED fresh run-output directory: strictly
#                      inside artifacts/ at run-root depth (>= 4 path
#                      components: artifacts/<area>/<stage>/…), no
#                      index-tracked content under it, and every file
#                      currently under it attributable to THIS run (itself
#                      a discovered sink, inside the gate's own evidence
#                      subtree, or inside another sink directory). The
#                      verify-stage per-command layout qualifies; a shared
#                      dir that already holds foreign/previous-run files
#                      NEVER qualifies (F42: old evidence must stay bound);
#   FILE <path>        the sink file alone, attributed to itself only
#                      (sink in a source tree, the repo root, a too-
#                      shallow artifacts dir, or a non-dedicated directory
#                      — nothing wider may be excluded);
#   TRACKED-SINK <p>   run output redirected ONTO tracked content — the
#                      caller refuses outright (it would corrupt bound
#                      state);
#   DISCOVERY-UNAVAILABLE  neither /proc nor lsof could resolve ANY pid —
#                      the caller fails closed instead of silently
#                      degrading to the F42 failure.
# /proc/<pid>/fd is the Linux fast path; lsof (-Fpfn: explicit field
# request — never rely on a platform implicitly emitting fd rows) is the
# darwin fallback. argv: <repo-abs> <gate-evidence-dir-abs>.
discover_run_output_sinks() {
  python3 - "$MAIN_REPO" "$EVIDENCE_DIR" <<'PYDISC'
import os
import subprocess
import sys

repo = os.path.realpath(sys.argv[1])
evidence_dir = os.path.realpath(sys.argv[2])
ev_rel = os.path.relpath(evidence_dir, repo)
if ev_rel == "." or ev_rel == ".." or ev_rel.startswith(".." + os.sep):
    ev_rel = ""                                   # evidence root outside the repo

# this process tree: self up to (excluding) pid 1, cycle- and depth-capped
pids, cur, seen = [], os.getpid(), set()
while cur > 1 and cur not in seen and len(pids) < 64:
    seen.add(cur)
    pids.append(cur)
    try:
        out = subprocess.run(
            ["ps", "-o", "ppid=", "-p", str(cur)],
            capture_output=True, text=True,
        ).stdout.strip()
        cur = int(out.split()[0]) if out else 1
    except Exception:
        cur = 1

# fd 1/2 file targets per pid — /proc fast path, lsof fallback (darwin)
resolved_any = False
fd_targets = set()
have_proc = os.path.isdir("/proc")
for pid in pids:
    fds = {}
    if have_proc:
        for fd in ("1", "2"):
            try:
                fds[fd] = os.path.realpath(os.readlink("/proc/%d/fd/%s" % (pid, fd)))
            except OSError:
                pass
    if not fds:
        try:
            out = subprocess.run(
                ["lsof", "-a", "-p", str(pid), "-d", "1,2", "-Fpfn"],
                capture_output=True, text=True,
            ).stdout
        except Exception:
            out = ""
        cur_fd = None
        for line in out.splitlines():
            if line.startswith("p"):
                cur_fd = None
            elif line.startswith("f") and line[1:] in ("1", "2"):
                cur_fd = line[1:]
            elif line.startswith("n") and cur_fd is not None:
                fds[cur_fd] = line[1:]
                cur_fd = None
    if fds:
        resolved_any = True
        for target in fds.values():
            fd_targets.add(target)

if not resolved_any:
    print("DISCOVERY-UNAVAILABLE")
    sys.exit(0)

def tracked(path):
    # index-tracked entries under path (ls-files: also works on an
    # unborn-HEAD scratch repo); an unreadable answer is fail-closed
    try:
        out = subprocess.run(
            ["git", "-C", repo, "ls-files", "--", path],
            check=True, capture_output=True, text=True,
        ).stdout
    except Exception:
        return True
    return out.strip() != ""

sinks = []                                        # repo-relative sink files
for target in sorted(fd_targets):
    if not target.startswith("/"):
        continue                                  # pipe / device / unnamed target
    real = os.path.realpath(target)
    if real == repo or not real.startswith(repo + os.sep):
        continue                                  # outside this repository
    if not os.path.isfile(real):
        continue                                  # directories/devices carry no output
    sinks.append(os.path.relpath(real, repo))

for rel in sinks:
    if tracked(rel):
        print("TRACKED-SINK %s" % rel)

# DIR-unit candidacy, under the F42 fences. Candidate attribution dirs:
# every sink's directory plus the gate's own evidence dir — a file under
# a candidate DIR unit is attributable to THIS run only when it is itself
# a sink, or some directory STRICTLY BELOW the unit (never the unit
# itself — loose foreign files directly under a non-dedicated dir must
# not inherit its candidacy) is an attribution dir.
attribution_dirs = set(os.path.dirname(r) for r in sinks)
if ev_rel:
    attribution_dirs.add(ev_rel)
sink_set = set(sinks)

def dedicated(unit):
    base = os.path.join(repo, unit)
    if not os.path.isdir(base):
        return False
    for root, dirs, files in os.walk(base):
        for d in dirs:
            if os.path.islink(os.path.join(root, d)):
                return False                      # a symlinked dir could shadow content
        for f in files:
            fr = os.path.relpath(os.path.join(root, f), repo)
            if fr in sink_set:
                continue                          # itself a discovered sink
            d_ = os.path.dirname(fr)
            attributed = False
            while d_ and d_ != unit:
                if d_ in attribution_dirs:
                    attributed = True
                    break
                d_ = os.path.dirname(d_)
            if not attributed:
                return False                      # foreign/pre-existing file — not dedicated
    return True

dir_units = set()
for cand in sorted(attribution_dirs):
    if not cand or not cand.startswith("artifacts/"):
        continue                                  # DIR units only inside the evidence tree
    if len(cand.split("/")) < 4:
        continue                                  # artifacts / artifacts/<area> / a bare stage dir can never be a unit
    if tracked(cand):
        continue                                  # tracked content must stay bound
    if cand == ev_rel or dedicated(cand):
        dir_units.add(cand)

emitted_dir = set()
for rel in sinks:
    if tracked(rel):
        continue                                  # already reported as TRACKED-SINK
    du = os.path.dirname(rel)
    if du in dir_units:
        if du not in emitted_dir:
            emitted_dir.add(du)
            print("DIR %s" % du)
    else:
        print("FILE %s" % rel)
PYDISC
}

# validate_declared_run_root <repo> <unit>: an R02_A16_RUN_OUTPUT_ROOTS
# entry — an EXPLICIT operator attestation — still passes the same
# fences: strictly inside artifacts/ at run-root depth (>= 4 components),
# an existing directory, no index-tracked content under it, and a sane
# repo-relative path. 0 = legal, 1 = illegal (reason on stderr).
validate_declared_run_root() {
  local repo="$1" unit="$2"
  case "$unit" in
    ""|.|..|../*|/*|*/)
      printf 'not a plain repo-relative directory path\n' >&2
      return 1
      ;;
  esac
  case "$unit" in
    artifacts/*/*/*) : ;;                        # >= artifacts/<area>/<stage>/<run>: `*` in a case
                                                 # pattern matches across `/`, so this is a depth
                                                 # floor of 4 components — artifacts itself, a bare
                                                 # area, or a bare stage dir can never pass
    *)
      printf 'not a dedicated run-output root strictly inside artifacts/ at run-root depth (>= artifacts/<area>/<stage>/…): excluding the whole evidence tree or foreign directories is forbidden\n' >&2
      return 1
      ;;
  esac
  [ -d "$repo/$unit" ] || { printf 'not an existing directory\n' >&2; return 1; }
  if [ -n "$(git -C "$repo" ls-files -- "$unit")" ]; then
    printf 'covers INDEX-TRACKED content (candidate source / committed evidence must stay bound)\n' >&2
    return 1
  fi
  return 0
}

# Assemble the exclusion-unit set ONCE, before the first binding:
# discovered sink units + explicitly declared roots + the gate's own
# evidence subtree — every unit validated, duplicates dropped, the final
# set recorded auditable in e0-binding-exclusions.txt.
BINDING_EXCLUSIONS="$EVIDENCE_DIR/e0-binding-exclusions.txt"
SINKS_FILE="$EVIDENCE_DIR/e0-run-output-sinks.txt"
CANDIDATES_FILE="$EVIDENCE_DIR/e0-binding-exclusion-candidates.tsv"
: > "$BINDING_EXCLUSIONS"
# The discovery call runs in a SUBSHELL whose redirection to $SINKS_FILE
# applies to the subshell only: redirecting the FUNCTION CALL directly
# would swap the gate bash's OWN fd-1 to $SINKS_FILE for the duration of
# the scan, hiding this run's real stdout sink from the walk exactly when
# it must be attributed (debugged live 2026-10-06: a call-site redirect
# made the parent's stdout.log invisible, stderr stayed visible via the
# child — the subshell keeps every ancestor's genuine sinks in view).
( discover_run_output_sinks ) > "$SINKS_FILE"
if grep -qx 'DISCOVERY-UNAVAILABLE' "$SINKS_FILE"; then
  fail "E0: cannot discover this run's output sinks (neither /proc nor lsof resolved any pid) — the run-output attribution would silently degrade to the F42 failure; failing closed"
fi
if grep -q '^TRACKED-SINK ' "$SINKS_FILE"; then
  grep '^TRACKED-SINK ' "$SINKS_FILE" >&2
  fail "E0: this run's output was redirected ONTO tracked content (see e0-run-output-sinks.txt above) — that would corrupt bound candidate/evidence state; move the run's output to a fresh untracked run root or outside the repository (F42)"
fi
{
  sed -n 's/^DIR /sink-dir\t/p; s/^FILE /sink-file\t/p' "$SINKS_FILE"
  if [ -n "${R02_A16_RUN_OUTPUT_ROOTS:-}" ]; then
    printf '%s\n' "$R02_A16_RUN_OUTPUT_ROOTS" | tr ':' '\n' | sed 's/^/declared\t/'
  fi
  [ -z "$EVIDENCE_REL_PREFIX" ] || printf 'evidence\t%s\n' "$EVIDENCE_REL_PREFIX"
} | awk -F '\t' '$2 != "" && !seen[$2]++' > "$CANDIDATES_FILE"
while IFS=$'\t' read -r kind unit; do
  [ -n "$unit" ] || continue
  case "$kind" in
    declared)
      # an explicitly declared root is an OPERATOR ATTESTATION, but it
      # still passes the same fences (artifacts/-inside at run-root
      # depth, existing directory) — never a source dir, never the
      # whole evidence tree, never the repo root
      if ! validate_declared_run_root "$MAIN_REPO" "$unit"; then
        fail "E0: declared run-output root '$unit' is illegal (F42) — run-output roots must be dedicated directories strictly inside artifacts/ covering no tracked content; fix the invocation instead of widening the exclusion"
      fi
      ;;
  esac
  if ! validate_run_output_unit "$MAIN_REPO" "$unit"; then
    fail "E0: illegal binding-exclusion unit '$unit' (kind $kind) — exclusion units must be fresh untracked paths covering no tracked content; fix the invocation instead of widening the exclusion (F42)"
  fi
  printf '%s\n' "$unit" >> "$BINDING_EXCLUSIONS"
done < "$CANDIDATES_FILE"

# Candidate worktree state — recorded COMPLETELY, never hidden. The
# exclusions are this run's run-time output roots ONLY (repair-group5
# evidence subtree + F42-attributed runner sinks/roots): with an in-repo
# evidence root or a nested verify-stage chain, the gate's own writes and
# the chain's runner captures between the two scans would otherwise make
# the mirror compare fail BY CONSTRUCTION; see bind_worktree's header.
bind_worktree "$MAIN_REPO" "$EVIDENCE_DIR/e0-candidate-binding.tsv" "$(cat "$BINDING_EXCLUSIONS")"
CANDIDATE_BINDING_ROWS="$(grep -vc '^#' "$EVIDENCE_DIR/e0-candidate-binding.tsv" || true)"
if [ "$CANDIDATE_BINDING_ROWS" -gt 0 ]; then
  git status --porcelain --untracked-files=no > "$EVIDENCE_DIR/e0-candidate-dirty.txt"
  git diff > "$EVIDENCE_DIR/e0-candidate-dirty.diff"
  DIRTY_DIGEST="$(shasum -a 256 "$EVIDENCE_DIR/e0-candidate-dirty.diff" | awk '{print $1}')"
  CANDIDATE_DIRTY=1
  note "NOTE candidate-worktree-dirty: this run binds the WORKTREE (uncommitted changes), tracked diff sha256=$DIRTY_DIGEST, full tracked+untracked binding in e0-candidate-binding.tsv ($CANDIDATE_BINDING_ROWS paths)"
else
  CANDIDATE_DIRTY=0
  note "candidate worktree clean: this run binds commit $CANDIDATE_SHA exactly"
fi
if [ -s "$BINDING_EXCLUSIONS" ]; then
  note "NOTE e0-binding-exclusion: this run's run-time output roots (runner output, never candidate source; applied symmetrically to BOTH bindings; one # header row per root in e0-candidate-binding.tsv; audit: e0-binding-exclusions.txt + e0-run-output-sinks.txt):"
  while IFS= read -r unit; do
    note "NOTE e0-binding-exclusion-root: $unit/**"
  done < "$BINDING_EXCLUSIONS"
fi

# ── Isolated copies (APFS copy-on-write; main worktree stays read-only) ────
# R5 workflow constraint: the original user attachment prohibits
# `git reset --hard` / `git clean -fd` / commits with NO throwaway-copy
# exception — the R4 construction used exactly those inside the copies.
# Both copies are therefore BUILT FRESH, never inherit-then-cleaned, and
# no git command in this gate is forced, destructive, or creates commits.
WORK=$(mktemp -d "${TMPDIR:-/tmp}/r02-a16.XXXXXX")
BASE_COPY="$WORK/repo-base"
CAND_COPY="$WORK/repo-candidate"
cleanup() { [ -n "${WORK:-}" ] && [ -d "$WORK" ] && rm -rf -- "$WORK"; }
trap cleanup EXIT
note "== isolated copies under $WORK =="

# Candidate copy: a full CoW mirror of the invoking worktree — tracked
# modifications, untracked candidates and ignored dependencies included.
# Nothing is ever deleted or reset inside it; the invoking worktree is
# used for git reads only.
cp -Rc "$MAIN_REPO" "$CAND_COPY"

# Candidate copy must mirror the invoking worktree EXACTLY — tracked
# modifications and untracked file contents alike. The exclusion units
# are the SAME set used for the main-repo binding above (repair-group5 +
# F42: symmetric exclusion of this run's run-time output roots; everything
# else must still match byte for byte).
[ "$(git -C "$CAND_COPY" rev-parse HEAD)" = "$CANDIDATE_SHA" ] || fail "candidate copy HEAD mismatch"
bind_worktree "$CAND_COPY" "$WORK/candidate-copy-binding.tsv" "$(cat "$BINDING_EXCLUSIONS")"
cmp -s "$EVIDENCE_DIR/e0-candidate-binding.tsv" "$WORK/candidate-copy-binding.tsv" \
  || fail "candidate copy does not mirror the invoking worktree (tracked+untracked content binding differs)"

# Baseline copy: exact historical git content of BASE_SHA and NOTHING else,
# constructed without ever inheriting the candidate files:
#   1. own .git object store (CoW clone of the invoking repo's store — new
#      directory entries in this copy; object blobs are immutable and are
#      never written back);
#   2. the copy's stale index file is re-initialized (the ONE file removed
#      here is the copy's own metadata copied in step 1 — no worktree path,
#      no candidate file, nothing the invoking repo owns);
#   3. `checkout --detach BASE_SHA` (NOT -f) over the still-EMPTY worktree:
#      git populates exactly the tracked files of BASE_SHA. There is
#      nothing to overwrite and nothing to delete — no `clean -fd` exists
#      in this gate because nothing untracked ever enters the copy;
#   4. dependencies: an isolated CoW copy of node_modules (ignored path,
#      reused as-is, never written back to the invoking repo).
mkdir -p "$BASE_COPY"
cp -Rc "$MAIN_REPO/.git" "$BASE_COPY/.git"
rm -f "$BASE_COPY/.git/index"
git -C "$BASE_COPY" checkout --detach "$BASE_SHA" > "$EVIDENCE_DIR/e0-base-checkout.log" 2>&1 \
  || { cat "$EVIDENCE_DIR/e0-base-checkout.log" >&2; fail "cannot check out base $BASE_SHA in the isolated copy"; }
cp -Rc "$MAIN_REPO/node_modules" "$BASE_COPY/node_modules"
{
  echo "HEAD=$(git -C "$BASE_COPY" rev-parse HEAD)"
  echo "diff-vs-base: $(git -C "$BASE_COPY" diff --quiet "$BASE_SHA" -- && echo empty || echo NONEMPTY)"
  echo "index: $(git -C "$BASE_COPY" diff --cached --quiet && echo clean || echo DIRTY)"
  echo "status-all: $(git -C "$BASE_COPY" status --porcelain --untracked-files=all | wc -l | tr -d ' ') entries"
  echo "construction: fresh .git CoW store + index re-init + non-forced detach checkout + isolated node_modules (no checkout -f, no clean -fd, no commit)"
} > "$EVIDENCE_DIR/e0-base-purity.txt"
[ "$(git -C "$BASE_COPY" rev-parse HEAD)" = "$BASE_SHA" ] || fail "base copy HEAD mismatch"
git -C "$BASE_COPY" diff --quiet "$BASE_SHA" -- \
  || fail "baseline copy tracked content differs from committed $BASE_SHA"
git -C "$BASE_COPY" diff --cached --quiet \
  || fail "baseline copy index is not clean"
[ -z "$(git -C "$BASE_COPY" status --porcelain --untracked-files=all)" ] \
  || { git -C "$BASE_COPY" status --porcelain --untracked-files=all > "$EVIDENCE_DIR/e0-base-residue.txt"; fail "baseline copy is not a pristine checkout of $BASE_SHA (see e0-base-residue.txt)"; }
note "PASS E0-baseline-purity (baseline = fresh construction of committed $BASE_SHA content; no prohibited git operations; purity asserts in e0-base-purity.txt)"
note "isolated copies verified: base pristine at $BASE_SHA; candidate mirrors the invoking state (tracked+untracked)"

# ── E5 classification machinery (defined early: the gate self-checks in E0s
#    exercise it before any npm run) ────────────────────────────────────────
#
# The documented pre-existing red family. Every member's legitimate red
# root cause is the seal coordinate lag: VERIFIED_SOURCE_SHA predates the
# authorized R01/R02 commits, so the committed-but-newer deliverables show
# up as "non-audit changes". NOT caused by this task; NOT fixable without
# the seal-coordinate governance step that is explicitly out of scope here.
# The family list is a hard requirement, not a whitelist to grow.
SEAL_FAMILY="tests/post-verification-audit-seal.test.ts
tests/round2-delivery-evidence.test.ts
tests/round3-delivery-evidence.test.ts"
printf '%s\n' "$SEAL_FAMILY" > "$EVIDENCE_DIR/e5-seal-family.txt"

# Recognized diagnostic signatures — the COMPLETE sentences of the REAL
# producers (path-normalized with <REPO>), matched as EXACT whole-line
# shapes below. R7-F01: a fixed substring "hit" proves nothing by itself —
# the complete line must be one of the real producers' complete shapes.
# R8-F01: EVERY structural category is additionally BOUND TO ITS PRODUCER
# within the same block — a body line is only legitimate inside the block
# whose complete lead line (its actual producer) is present:
#   - the seal TEST's own assertion (tests/post-verification-audit-seal.test.ts):
#       AssertionError: VERIFIED_SOURCE_SHA 之后出现了非审计文件改动（…）:
#     followed by the bare violator paths (one whitespace-and-colon-free
#     token per line), the vitest toEqual diff body, and the matcher
#     summary `<path>: expected [ …(N) ] to deeply equal []`;
#   - the GUARD's own refusal line (.sync-audit/verify-post-verification-diff.mjs
#     fail(): `✗ ` + sentence + `:`), followed by `  - path` listing lines
#     (two-space dash + one token, nothing else on the line);
#   - the GENERATOR's embedded tail (create-delivery-patch.py /
#     create-round3-patch.py `post-verification diff guard failed: ` + the
#     guard's `✗ …` output) — INCLUDING the guard-tail WINDOW variant
#     (repair-group10, gate-r2 evidence): when the violator listing is
#     longer than the 500-code-point window the generators slice off the
#     guard's output (`guard_output[-500:]`), the ✗ sentence is gone and
#     the embedded line is `post-verification diff guard failed: ` + a
#     front-cut printable-ASCII path fragment, followed by the guard's
#     complete `  - path` listing lines and (when the guard's own
#     console.error was lost past the 64 KiB pipe buffer at exit) a final
#     mid-token listing line; accepted ONLY as that exact structure —
#     generator wrapper as the block's first payload row, fragment with
#     no space/colon/non-ASCII (sentence material can never pass), ≥1
#     complete listing line, and the reconstructed fragment+lines window
#     EXACTLY 500 code points;
#   - the GENERATOR's uncommitted/untracked refusal with its firstDiff body;
#   - the Node wrapper `Error: Command failed: <argv0> <script>` — ONLY as
#     the FIRST payload row of a block (it is the head of the thrown
#     error's message) and ONLY for the three commands the seal-family
#     tests actually spawn (node …/verify-post-verification-diff.mjs,
#     python3 …/create-delivery-patch.py, python3 …/create-round3-patch.py
#     — tests/round2-delivery-evidence.test.ts:95/255,
#     tests/round3-delivery-evidence.test.ts:127/220);
#   - the git CRLF warning — git's complete sentence
#     `warning: in the working copy of '<path>', (LF|CRLF) will be replaced
#     by (LF|CRLF) the next time Git touches it`, and nothing else on the
#     line.
# Anything else in a payload line — a `fatal:` line, an appended
# `Error: EACCES`, a foreign ✗ reason, a truncated sentence, a bare token
# containing `:` (R8-F01: `Error:EACCES`), a listing line with a trailing
# error, a warning with a trailing error, a matcher line with a trailing
# error, a wrapper for a command the family never runs, or a body line in
# a block without its producing lead — is FOREIGN.
SEAL_LAG_SENT_GUARD='VERIFIED_SOURCE_SHA 之后出现非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）'
SEAL_LAG_SENT_ASSERT='VERIFIED_SOURCE_SHA 之后出现了非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）'
SEAL_LAG_GUARD_PREFIX='post-verification diff guard failed:'
UNCOMMITTED_DIAG='current source manifest does not match HEAD (uncommitted or untracked source changes)'
# repair-group10: the heredoc delimiter is QUOTED — the previous unquoted
# form left the backticked terms below live to shell command substitution
# (`✗`, `Error: EACCES…`, `git diff ..HEAD 失败` each ran as a command:
# "✗: command not found" on the gate's stderr, and the backticked segments
# were silently REPLACED by empty output inside this archived ledger — a
# silent corruption of the audit record). The one intended expansion
# ($UNCOMMITTED_DIAG) is now written literally.
cat > "$EVIDENCE_DIR/e5-cause-classes.txt" <<'CLASSES'
seal-coordinate-lag	committed non-audit changes post-date the frozen VERIFIED_SOURCE_SHA — ONLY a payload line that is EXACTLY one of the real producers' complete shapes (the seal test's AssertionError sentence, the guard's direct `✗` sentence line, or the generator's guard-embedded line carrying the COMPLETE sentence) classifies here; each shape may end with the trailing `:` and NOTHING else. repair-group10 (gate-r2 round2 block 4 / round3 block 6): the generator's guard-tail WINDOW is the fourth registered shape — `post-verification diff guard failed: ` + a front-cut printable-ASCII path fragment + the guard's complete `  - path` listing lines (final line possibly mid-token where the guard's output was lost past the 64 KiB pipe buffer), accepted ONLY under a GENERATOR wrapper as the block's first payload row, ONLY with no space/colon/non-ASCII in the fragment (sentence material can never pose as a path fragment), ONLY with at least one complete listing line, and ONLY when the reconstructed window is EXACTLY 500 code points (the literal guard_output[-500:] window). RR2 B-R2 R2 form evolution (the tree outgrew git's own apply ceiling — MAX_APPLY_SIZE ≈ 1023 MiB vs the ≥3.67 GB regenerated patch, so `git apply --cached` refuses with `error: patch too large` before the generator reaches its guard/uncommitted checkpoints): the generators' patch-too-large replay crash is the FIFTH registered shape, the same family's evolved form and the same registered RED — round2: the complete CONSECUTIVE 6-row python traceback (Traceback header; File frames citing the WRAPPED generator script at line 457 in <module> and line 359 in replay_and_verify with their real source-context rows) ending in the complete sentence `RuntimeError: patch replay failed: error: patch too large`; round3: the bare SystemExit line `patch replay failed: error: patch too large`; each accepted ONLY under ITS OWN generator wrapper as the block's first payload row — a cross-producer combination, either shape under the node-guard wrapper, any OTHER git error text, a truncated sentence, or a mutated frame is NOT this class. A line that carries the complete sentence PLUS additional content (R7-F01: an appended `Error: EACCES: permission denied`) still records the known cause AND is UNRECOGNIZED for its unexplained remainder; any other guard payload (missing/wrong coordinate file, non-hex/non-commit coordinate, `git diff ..HEAD 失败`, permission failure, truncated sentence, bare or geometry-violating tail window) is NOT this class.
uncommitted-source-rejection	a patch generator refused: current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=[…] — the COMPLETE generator line whose firstDiff body is EXACTLY the producers' json.dumps(diff_paths[:3], ensure_ascii=False) output (a JSON array of 1..3 repo-relative path string literals, nothing else on the line; R9-F01: validated element-by-element, non-JSON/mixed/truncated/empty fail closed) and nothing else; expected at the candidate only while the worktree is dirty; at the base it proves baseline pollution (gate FAILS)
UNRECOGNIZED	every payload line that is not a COMPLETE recognized shape and not COMPLETELY part of one's body. R8-F01: every structural category is bound to its producer INSIDE the same block — the `  - path` listing only under a complete guard `✗`/embedded line or a VALIDATED guard-tail window (repair-group10: generator-wrapped, ≥1 complete listing line, exactly 500 code points — a bare `.py` tail, a wrong-length run, a mid-sentence fragment, or a window under any other first row stays right here), the bare-path/diff-body/matcher lines only under the complete AssertionError lead, the wrapper only as the first payload row and only for the three real family commands, the warning only as git's complete CRLF sentence. RR2 B-R2 R2: a patch-too-large traceback/sentence row only counts under its OWN generator wrapper with the COMPLETE end-anchored geometry — a truncated `patch replay failed:` sentence, any other git error text, a foreign SystemExit message, a cross-producer combination, a mutated traceback frame, or the shape under the node-guard wrapper stays right here. A body line without its producing lead, a `  - path`/warning/matcher/wrapper line with ANY trailing content, a bare token containing `:`, a `fatal:` line, any foreign Error/✗/Traceback, a truncated or unregistered tail-window guard refusal, a zero-payload block, or a block no recognized diagnostic FULLY explains is UNRECOGNIZED (gate FAILS CLOSED, per BLOCK — no "registered unreadable" class exists; an unproven cause never takes the gate green, at either end)
CLASSES

# extract_blocks <vitest-log> <copy-path>: prints one
# `file<TAB>block<TAB>payload-line` row per line of each failing test's
# error MESSAGE — the first `AssertionError:`/`Error:` line plus every
# following line of embedded subprocess output (git warnings, generator
# stderr), stopping at the stack-trace marker. BLOCK IDENTITY IS KEPT
# (R5-F01): each ` FAIL ` header starts a new block (numbered across the
# whole log), so a file's blocks stay distinguishable — classification is
# per block, never per file. The copy's absolute path is normalized to
# <REPO>. R6-F01: the header itself emits a BLOCK-MARKER row, so a block
# whose message body is EMPTY (a FAIL header followed immediately by a
# stack frame — the zero-payload shape) still has a row and is therefore
# JUDGED (as an unexplained block), instead of vanishing from the blocks
# file while failed_files still lists it. A header without ` > ` keeps
# the whole header line as the file name (the previous `current = ""`
# made such blocks invisible — the exact zero-payload hole).
extract_blocks() {
  local log="$1" copy_real copy_arg
  copy_real="$(cd "$2" && pwd -P)"
  copy_arg="$2"
  awk -v copy_real="$copy_real" -v copy_arg="$copy_arg" '
    /^ FAIL  / {
      line = $0
      sub(/^ FAIL  +/, "", line)
      if (line ~ / > /) { sub(/ > .*/, "", line); current = line } else { current = line }
      block++
      inblock = 1
      print current "\t" block "\t⟦FAIL-BLOCK⟧"
      next
    }
    inblock && (/^ ❯ / || /^⎯/ || /^ *Test Files /) { inblock = 0; current = "" }
    inblock && current != "" { print current "\t" block "\t" $0 }
  ' "$log" | sed -e "s|$copy_real/|<REPO>/|g" -e "s|$copy_arg/|<REPO>/|g"
}

# failed_files <vitest-log>: unique failing test files (summary lines and
# detailed block headers both normalize to the bare path). A green run has
# NO ` FAIL ` lines — grep then exits 1, which under `set -o pipefail`
# would kill the gate; emptiness is a valid outcome (the pristine baseline
# is supposed to be green), so the no-match case is neutralized here.
failed_files() {
  { grep -E "^ FAIL " "$1" || true; } | sed -E 's/^ FAIL +//; s/ > .*//; s/ \([0-9]+ tests.*$//' | sort -u
}

# non_family_files <failed-files-list>: failing files that are NOT members
# of the documented seal family. R7-F01: this used to be `grep -vFf` — a
# SUBSTRING filter — so the legitimate vitest name
# `tests/post-verification-audit-seal.test.ts.regression.test.ts`, which
# CONTAINS a family member as a substring but is NOT that file, was
# filtered out as if it were the member and the gate accepted it. Family
# membership is now EXACT WHOLE-LINE identity (-x): a failing file passes
# only when its COMPLETE path equals one of the three documented members
# byte for byte. No suffix/prefix rule — full-path identity only.
non_family_files() {
  { grep -vxF -f "$EVIDENCE_DIR/e5-seal-family.txt" "$1" || true; }
}

# classify_file <blocks-file> <file>: prints the file's cause CLASSES, one
# per line: `seal-coordinate-lag`, `uncommitted-source-rejection`, and/or
# `UNRECOGNIZED` (fail-closed). R5-F01: classification is PER FAILING
# BLOCK — every block of the file is judged independently on the actual
# diagnostics inside THAT block, and the file's classes are the UNION of
# the block verdicts. R6-F01: no "registered unreadable" outcome exists.
# R7-F01: each payload line is verified against the COMPLETE structure of
# the real producers' shapes, END-ANCHORED:
#   • a line that is EXACTLY a complete recognized shape sets that cause;
#   • a line that CONTAINS a complete known sentence but is NOT exactly
#     the complete shape (extra text on the same line — e.g. the appended
#     `Error: EACCES: permission denied`) sets the cause AND is FOREIGN:
#     the sentence is real evidence of the known cause, but the remainder
#     is unexplained, and nothing `continue`s past it any more;
#   • a line that is part of a recognized diagnostic's body explains
#     nothing but is not foreign — R8-F01: ONLY while the COMPLETE lead
#     line of that body's actual producer is present in the SAME block,
#     and ONLY when the body line itself is complete (a `  - path`
#     listing or a warning or a matcher summary with ANY trailing error
#     text is foreign; a bare token containing `:` — `Error:EACCES` — is
#     not a path);
#   • the `Error: Command failed:` wrapper is structural ONLY as the
#     FIRST payload row of the block (head of the thrown error message)
#     and ONLY for the three commands the seal-family tests spawn;
#   • EVERYTHING ELSE — a `fatal:` line, any other Error/✗/Traceback, a
#     truncated or tail-window guard line, a body line in a block whose
#     producing lead is absent — is FOREIGN. There is no finite
#     foreign-prefix list to escape: unknown means foreign.
classify_file() {
  awk -v file="$2" \
      -v sent_pre="${SEAL_LAG_SENT_GUARD%非审计*}" \
      -v sent_post="${SEAL_LAG_SENT_GUARD#*之后出现}" \
      -v sent_assert="$SEAL_LAG_SENT_ASSERT" \
      -v guard_prefix="$SEAL_LAG_GUARD_PREFIX" \
      -v uncommitted_sent="$UNCOMMITTED_DIAG" '
    # R9-F01: raw_ctrl(c) — is c a raw control character (U+0001..U+001F)?
    # Built via sprintf("%c") so it stays byte/locale-agnostic; UTF-8
    # continuation/lead bytes (>= 0x80) can never collide with it.
    function raw_ctrl(c,    j, t) {
      t = ""
      for (j = 1; j <= 31; j++) t = t sprintf("%c", j)
      return index(t, c) > 0
    }
    # R9-F01: firstdiff_ok(tail) — strict validation of the ONLY structure
    # the REAL producers emit after `firstDiff=[`: both round2
    # create-delivery-patch.py and round3 create-round3-patch.py return
    #   json.dumps(diff_paths[:3], ensure_ascii=False)
    # i.e. a JSON array of ONE..THREE string literals whose elements are
    # separated by exactly `", "` (json.dumps default separators), closed
    # by the tail-final `]`, with NOTHING after it. String-literal grammar
    # as the producer emits it: literal content is any character except
    # `"` and `\` and raw control characters (json always escapes those);
    # escapes are exactly `\"` `\\` `\n` `\t` `\r` `\b` `\f` and
    # `\u00[01][0-9a-f]` (Python with ensure_ascii=False only ever escapes
    # U+0000..U+001F, lowercase hex); `\u0000` (NUL) is impossible in a
    # git path. Element-level path semantics: non-empty, not absolute
    # (git manifest paths are repo-relative). Everything else — a bare
    # unquoted token (`firstDiff=[Error:EACCES]`), mixed content
    # (`["docs/x.json" Error:EACCES]`), a truncated array, `[]`, more
    # than three elements, a `]`/`,` outside a string, or trailing text —
    # is NOT the producer shape and fails closed. This replaces the old
    # prefix + tail-`]`-with-no-second-`]` check, which accepted those
    # non-JSON payloads as the pure known cause AND wrongly rejected
    # legal `]`-containing paths. Sequential scan, never split on `", "`:
    # a legal path may itself contain `", "` (e.g. `a, b.txt`).
    function firstdiff_ok(tail,    n, body, nb, i, elems, c, e, hexd, h, nch) {
      n = length(tail)
      if (n < 3) return 0                          # shortest legal tail is `"x"]`
      if (substr(tail, n, 1) != "]") return 0      # complete line endpoint
      body = substr(tail, 1, n - 1)
      if (body == "") return 0                     # `[]` — an empty diff is never a real mismatch reason
      nb = length(body)
      i = 1
      elems = 0
      while (1) {
        if (i > nb || substr(body, i, 1) != "\"") return 0   # element must open with a quote
        if (substr(body, i + 1, 1) == "/") return 0          # absolute path — manifest paths are repo-relative
        i++
        nch = 0
        while (1) {
          if (i > nb) return 0                     # unterminated literal — truncated
          c = substr(body, i, 1)
          if (c == "\"") { i++; break }
          if (c == "\\") {
            if (i + 1 > nb) return 0
            e = substr(body, i + 1, 1)
            if (e == "\"" || e == "\\" || e == "n" || e == "t" || e == "r" || e == "b" || e == "f") {
              i += 2; nch++; continue
            }
            if (e == "u") {
              if (i + 5 > nb) return 0
              hexd = substr(body, i + 2, 4)
              if (substr(hexd, 1, 2) != "00") return 0        # producer only escapes U+0000..U+001F
              if (substr(hexd, 3, 1) != "0" && substr(hexd, 3, 1) != "1") return 0
              h = substr(hexd, 4, 1)
              if (h !~ /^[0-9a-f]$/) return 0                 # Python emits lowercase hex
              if (hexd == "0000") return 0                    # NUL cannot appear in a git path
              i += 6; nch++; continue
            }
            return 0                               # any other escape (`\/`, `\x…`): not producer output
          }
          if (raw_ctrl(c)) return 0                # raw control char: json.dumps always escapes it
          i++; nch++
        }
        if (nch == 0) return 0                     # empty string is not a real manifest path
        elems++
        if (elems > 3) return 0                    # producer slices diff_paths[:3]
        if (i > nb) break
        if (substr(body, i, 2) != ", ") return 0   # json.dumps default item separator
        i += 2
      }
      return 1
    }
    {
      # Split on the FIRST two tabs only — the payload may itself contain
      # tabs, and the file field may contain the block id as a substring.
      pos1 = index($0, "\t")
      f = substr($0, 1, pos1 - 1)
      if (f != file) next
      saw_any = 1
      rest = substr($0, pos1 + 1)
      pos2 = index(rest, "\t")
      b = substr(rest, 1, pos2 - 1) + 0
      n[b]++
      payload[b, n[b]] = substr(rest, pos2 + 1)
      if (b > maxb) maxb = b
    }
    END {
      # A file listed as failing but with NO block rows at all is an
      # unexplained failure — fail closed, never silently skipped.
      if (!saw_any) { print "UNRECOGNIZED"; exit }
      # The COMPLETE legitimate shapes, end-anchored, with the 了 of the
      # seal-test assertion (出现了) optional in every alternative — the
      # test asserts 出现了, the guard fail() prints 出现, and the
      # generator can embed either.
      opt = sent_pre "(了)?" sent_post
      seal_exact = "^(AssertionError: " opt "|✗ " opt "|" guard_prefix " (✗ )?" opt "):?$"
      # R8-F01 producer binding: the two lead shapes whose BODY lines are
      # legitimate only inside the same block as the complete lead.
      assert_lead_exact = "^AssertionError: " opt ":$"
      guard_line_exact = "^(✗ " opt "|" guard_prefix " (✗ )?" opt "):$"
      # The wrapper is structural only as the FIRST payload row (row 1 is
      # the block marker) and only for the three real family commands
      # (single-token argv: a `;`-chained or otherwise extended tail
      # cannot match the end anchor).
      wrapper_exact = "^Error: Command failed: (node [^ ]+/\\.sync-audit/verify-post-verification-diff\\.mjs|python3 [^ ]+/create-delivery-patch\\.py|python3 [^ ]+/create-round3-patch\\.py)$"
      # The complete git CRLF warning sentence — any trailing content
      # fails the end anchor (the apostrophes arrive via sprintf, not
      # shell).
      q = sprintf("%c", 39)
      warn_exact = "^warning: in the working copy of " q "[^" q "]+" q ", (LF|CRLF) will be replaced by (LF|CRLF) the next time Git touches it$"
      # The vitest array-matcher summary the seal test emits — complete
      # line, either the elided (`…(N)`) or the inline-quoted form, and
      # only inside an AssertionError-led block.
      matcher_exact = "^[^[:space:]:]+: expected \\[ (…\\([0-9]+\\)|(\"[^\"]*\", )*\"[^\"]*\") \\] to deeply equal \\[\\]$"
      # The uncommitted shape is verified structurally: prefix strip plus
      # firstdiff_ok (the real json.dumps string-array grammar — see the
      # function header), NOT via one ERE: the real sentence contains
      # literal ASCII parentheses, which an ERE would read as grouping
      # operators.
      uncommitted_prefix = uncommitted_sent ": firstDiff=["
      # R02 final-closeout repair-group10 (gate-r2 evidence: artifacts/
      # rust-tauri/R02/final-candidate-a29e5adbb404ad70/A16/legacy-entry/
      # e5-candidate-blocks.txt — tests/round2-delivery-evidence.test.ts
      # block 4, tests/round3-delivery-evidence.test.ts block 6): the
      # guard-tail WINDOW form. Both patch generators print the guard
      # refusal as `guard_prefix + " " + guard_output[-500:]`
      # (artifacts/f1-f12-repair/round2/create-delivery-patch.py:449,
      # artifacts/f1-f12-repair/round3-c01-c03/create-round3-patch.py:449)
      # — the LAST 500 code points of the guard output. When the
      # changed-file listing is longer than the window (this repo at
      # gate-r2: ~65 KB, ~2100 violators), the ✗ sentence is windowed
      # away and what remains is a FRONT-CUT PATH FRAGMENT ([-500:] cuts
      # mid-path; gate-r2: `udit-r17-cli-rust-targeted/typecheck-05/
      # exit-codes.txt`) followed by the guard own complete `  - path`
      # listing lines, ending in a mid-token line when the guard
      # console.error output was itself lost beyond the 64 KiB pipe
      # buffer at process.exit(1) (gate-r2 window ending: `  - artifacts/
      # rust-t`; the guard part byte-exactly 65536). Registered as a
      # STRUCTURED shape anchored to the producer exact geometry — no
      # substring is ever accepted:
      #   • producer binding: ONLY inside a block whose FIRST payload
      #     row is one of the two GENERATOR wrappers — the prefix line
      #     has no other producer (a window under the node-guard wrapper
      #     or any other first row is UNRECOGNIZED);
      #   • fragment grammar `[!-9;-~]*` (printable ASCII, no space, no
      #     colon): every suffix fragment of EVERY guard fail() sentence
      #     carries a space or the full-width `）`/CJK tail, so sentence
      #     material can never pose as a path fragment (a mid-sentence
      #     cut stays UNRECOGNIZED — the truncated-guard fixture);
      #   • ≥1 following listing line and ≥1 COMPLETE listing line
      #     (`  - ` + the same ASCII grammar; the bare `.py` tail of the
      #     R6-F01 fixtures has zero following lines and stays
      #     UNRECOGNIZED);
      #   • the reconstructed window — fragment plus newline-joined
      #     listing lines, no trailing newline — is EXACTLY 500 code
      #     points (all-ASCII by grammar, so length is locale-proof):
      #     the literal `[-500:]` window size. Any mutation (dropped
      #     char/line, appended junk, foreign line inside the run)
      #     breaks the geometry and fails closed.
      # A validated window proves the guard violator listing existed,
      # and only the non-audit-change refusal ever prints one — the
      # block records seal-coordinate-lag and the window rows are its
      # body.
      guard_tail_prefix = guard_prefix " "
      genwrapper_exact = "^Error: Command failed: (python3 [^ ]+/create-delivery-patch\\.py|python3 [^ ]+/create-round3-patch\\.py)$"
      tail_frag_exact = "^[!-9;-~]*$"
      tail_line_exact = "^  - [!-9;-~]*$"
      # R05 RR2 B-R2 R2 (form evolution of the repair-group10 family):
      # the generator patch-too-large replay crash. The tree outgrew
      # the apply ceiling INSIDE git itself (git apply.c MAX_APPLY_SIZE
      # = 1024*1024*1024-1 ~ 1023 MiB; F44 measured the regenerated
      # uncompressed delivery patch at 3,671,013,696 B), so `git apply
      # --cached` now refuses with `error: patch too large` BEFORE the
      # generators reach their guard/uncommitted checkpoints. The two
      # frozen producers surface this differently, and each shape binds
      # to ITS OWN generator wrapper (the block first payload row):
      #   • round2 create-delivery-patch.py:359 raises
      #     RuntimeError(f"patch replay failed: {apply.stdout[:2000]}")
      #     — an UNCAUGHT exception, so python3 prints its 6-row
      #     traceback: the Traceback header, the File frame citing the
      #     WRAPPED script at line 457 in <module> with its source-
      #     context row, the File frame at line 359 in
      #     replay_and_verify with the raise line itself, and the
      #     complete final sentence `RuntimeError: patch replay failed:
      #     error: patch too large` (the tail IS the complete git
      #     refusal text — any other git error, a truncated sentence,
      #     or a mutated frame breaks the anchor);
      #   • round3 create-round3-patch.py:342 raises SystemExit with the
      #     same message — python3 prints a string SystemExit BARE (no
      #     traceback): exactly the line `patch replay failed: error:
      #     patch too large`. A foreign SystemExit message (`round3
      #     patch is empty`, cat-file failures, ...) is a DIFFERENT
      #     failure and never matches.
      # Cross-producer combinations (a bare sentence under the round2
      # wrapper, the traceback under the round3 wrapper) and the shapes
      # under the node-guard wrapper are not real producer outputs and
      # stay UNRECOGNIZED. A validated form proves the wrapped
      # generator died at its replay step on the git size ceiling — the
      # registered family evolved form of the same governance-lag
      # cause (still a REGISTERED RED, never formal green).
      ptl_sentence = "patch replay failed: error: patch too large"
      ptl_r2_final = "RuntimeError: " ptl_sentence
      qq = sprintf("%c", 39)   # apostrophe, shell-safe (the awk program is single-quoted)
      ptl_r2_raise = "    raise RuntimeError(f\"patch replay failed: {apply.stdout.decode(errors=" qq "replace" qq ")[:2000]}\")"
      for (b = 1; b <= maxb; b++) {
        win_start[b] = 0; win_end[b] = 0
        ptl_start[b] = 0; ptl_end[b] = 0
        if (n[b] < 3) continue
        if (payload[b, 2] !~ genwrapper_exact) continue
        gscript = payload[b, 2]
        sub(/^Error: Command failed: python3 /, "", gscript)
        for (i = 3; i <= n[b]; i++) {
          if (substr(payload[b, i], 1, length(guard_tail_prefix)) != guard_tail_prefix) continue
          frag = substr(payload[b, i], length(guard_tail_prefix) + 1)
          if (frag !~ tail_frag_exact) break   # sentence material / foreign tail: no window; the line is judged below
          win = frag; consumed = 0; complete_seen = 0
          for (j = i + 1; j <= n[b]; j++) {
            if (payload[b, j] !~ tail_line_exact) break
            win = win "\n" payload[b, j]
            consumed++
            if (length(payload[b, j]) > 4) complete_seen = 1
          }
          if (consumed >= 1 && complete_seen && length(win) == 500) {
            win_start[b] = i; win_end[b] = i + consumed
          }
          break   # exactly one guard-prefix line per real generator failure; an unvalidated one stays fail-closed
        }
        if (gscript ~ /\/create-delivery-patch\.py$/) {
          # round2: the complete consecutive 6-row traceback, File
          # frames bound to the WRAPPED script, ending in the complete
          # sentence (see the shape block above).
          for (i = 3; i + 5 <= n[b]; i++) {
            if (payload[b, i] != "Traceback (most recent call last):") continue
            if (payload[b, i+1] != "  File \"" gscript "\", line 457, in <module>") continue
            if (payload[b, i+2] != "    outcome = replay_and_verify(patch, stored)") continue
            if (payload[b, i+3] != "  File \"" gscript "\", line 359, in replay_and_verify") continue
            if (payload[b, i+4] != ptl_r2_raise) continue
            if (payload[b, i+5] != ptl_r2_final) continue
            ptl_start[b] = i; ptl_end[b] = i + 5
            break
          }
        } else if (gscript ~ /\/create-round3-patch\.py$/) {
          # round3: the bare SystemExit message line, complete and
          # end-anchored (nothing else on the line).
          for (i = 3; i <= n[b]; i++) {
            if (payload[b, i] == ptl_sentence) {
              ptl_start[b] = i; ptl_end[b] = i
              break
            }
          }
        }
      }
      seal = 0; uncommitted = 0; unrecognized = 0
      for (b = 1; b <= maxb; b++) {
        if (n[b] == 0) continue   # block ids are LOG-global: ids with no rows for THIS file belong to other files
        b_seal = 0; b_uncommitted = 0; b_foreign = 0
        b_assert_lead = 0; b_guard_line = 0
        for (i = 1; i <= n[b]; i++) {
          line = payload[b, i]
          if (line == "⟦FAIL-BLOCK⟧") continue              # block marker: proves existence, explains nothing
          if (line == "") continue                          # blank separator inside a real message body
          # repair-group10: a row inside a VALIDATED guard-tail window is
          # the body of the classified seal-lag diagnostic (see the
          # pre-pass above) — explained, never foreign.
          if (win_start[b] && i >= win_start[b] && i <= win_end[b]) continue
          # RR2 B-R2 R2: a row inside a VALIDATED patch-too-large form
          # (round2 traceback / round3 bare SystemExit line) is the body
          # of the registered family evolved seal-lag diagnostic —
          # explained, never foreign.
          if (ptl_start[b] && i >= ptl_start[b] && i <= ptl_end[b]) continue
          if (index(line, sent_assert) || index(line, sent_pre sent_post)) {
            # The COMPLETE known sentence is present (with or without 了).
            # It proves the known cause — but ONLY the exact complete
            # shape is fully explained: anything else on the line
            # (R7-F01: an appended `Error: EACCES: permission denied`)
            # stays unexplained and the block is ALSO unrecognized. No
            # early continue past the remainder any more. The two lead
            # shapes open their body bindings (R8-F01).
            b_seal = 1
            if (line ~ seal_exact) {
              if (line ~ assert_lead_exact) b_assert_lead = 1
              if (line ~ guard_line_exact) b_guard_line = 1
            } else {
              b_foreign = 1
            }
            continue
          }
          if (index(line, uncommitted_sent)) {
            b_uncommitted = 1
            # R9-F01 structural exact check: the line must BE
            # `<sentence>: firstDiff=[<json array>]` where the body is
            # EXACTLY the real producer json.dumps(diff_paths[:3],
            # ensure_ascii=False) output — validated element-by-element
            # by firstdiff_ok (quotes/escapes, separator, element cap,
            # path semantics, complete line endpoint). The old check
            # (prefix + tail-`]` + no second `]`) accepted non-JSON
            # payloads like `firstDiff=[Error:EACCES]` as the pure known
            # cause and wrongly rejected legal `]`-containing paths.
            # String ops only (no ERE): the sentence contains literal
            # ASCII parentheses.
            utail = substr(line, length(uncommitted_prefix) + 1)
            uexact = (substr(line, 1, length(uncommitted_prefix)) == uncommitted_prefix) \
                     && (firstdiff_ok(utail))
            if (!uexact) b_foreign = 1
            continue
          }
          # The wrapper: structural ONLY as the first payload row (row 2
          # counting the marker) and ONLY for a real family command — a
          # `; Error: …` tail or a command the family never spawns is
          # FOREIGN (R8-F01: the wrapper is no longer a pass-through).
          if (i == 2 && line ~ wrapper_exact) continue
          # The complete git CRLF warning sentence only.
          if (line ~ warn_exact) continue
          # Body lines of a recognized diagnostic, each BOUND to its
          # producer within THIS block (R8-F01) and complete on its own:
          # the guard listing (`  - <one token, no whitespace, no colon>`),
          # valid only under a complete guard line.
          if (b_guard_line && line ~ /^  - [^[:space:]:]+$/) continue
          # the seal-test bare violator paths (a single token, no
          # whitespace AND no colon — `Error:EACCES` is not a path), valid
          # only under the complete AssertionError lead.
          if (b_assert_lead && line ~ /^[^[:space:]:]+$/) continue
          # The vitest toEqual diff body of the REAL seal-test failure
          # (observed live in artifacts/rust-tauri/R01/STAGE_REPAIR_R5/
          # f02-npm-test-iso-r5.txt): after the message and the bare
          # violator paths, vitest prints `- Expected`, `+ Received`,
          # `- []`, `+ [`, `+   "path",` … `+ ]`; the LAST violator path
          # line carries the matcher summary appended
          # (`…: expected [ …(N) ] to deeply equal []` — vitest array
          # matcher summaries always reopen a bracket). These are the
          # assertion body, not foreign errors — but ONLY inside an
          # AssertionError-led block (a synthetic lead-in like
          # `AssertionError: expected guard to be green` opens no
          # binding, and its summary would match no complete shape).
          if (b_assert_lead) {
            if (line ~ /^[-+] (Expected|Received)([[:space:]][-+][[:space:]]+[0-9]+)?$/) continue
            if (line ~ /^[-+] \[\]$/) continue
            if (line ~ /^[-+] \[$/) continue
            if (line ~ /^[-+] \]$/) continue
            if (line ~ /^[-+][[:space:]]+".*",?$/) continue
            if (line ~ matcher_exact) continue
          }
          # Everything else is FOREIGN: `fatal:` lines, foreign Errors,
          # ✗ refusals without the complete sentence, Tracebacks, other
          # guard/generator payloads, body lines without their producing
          # lead — unknown means foreign (R7-F01: no finite foreign-prefix
          # list any more; R8-F01: no prefix of a structural category is
          # a pass-through).
          b_foreign = 1
        }
        # repair-group10: a validated guard-tail window proves the guard
        # refused with the non-audit-change violator listing (the only
        # fail() sentence that ever prints one) — seal-coordinate-lag.
        if (win_start[b]) b_seal = 1
        # RR2 B-R2 R2: a validated patch-too-large form proves the
        # wrapped generator died at its replay step on the apply
        # ceiling inside git itself — the registered family evolved
        # form of the same governance-lag cause (registered RED, never
        # formal green).
        if (ptl_start[b]) b_seal = 1
        if (b_seal) seal = 1
        if (b_uncommitted) uncommitted = 1
        # An EMPTY block, a zero-payload block (marker only), or an
        # unexplained one is unrecognized FOR ITS FILE: never absorbed by
        # a sibling block (R5-F01), never registered-then-accepted (R6-F01).
        if (b_foreign || (!b_seal && !b_uncommitted)) unrecognized = 1
      }
      if (seal) print "seal-coordinate-lag"
      if (uncommitted) print "uncommitted-source-rejection"
      if (unrecognized) print "UNRECOGNIZED"
    }
  ' "$1"
}

# ── E0s: gate self-checks (fail fast, before any npm run) ─────────────────
# The classifier must give each documented shape its distinct verdict, and
# the binding must detect untracked-file changes. These fixtures are the
# functional proof that a clean historical baseline, candidate new-file
# changes, a real coordinate lag, and a non-coordinate generator failure
# are four DIFFERENT gate outcomes (R4-F02).
note "== E0s: gate self-checks (classifier + binding fixtures) =="
SELFCHECK="$WORK/selfcheck"
mkdir -p "$SELFCHECK"
: > "$EVIDENCE_DIR/e0s-self-checks.log"

sc_expect() { # sc_expect <name> <fixture-log> <expected-classes...>
  local name="$1" log="$2"; shift 2
  local blocks classes
  blocks="$SELFCHECK/$name.blocks"
  extract_blocks "$log" "$SELFCHECK" > "$blocks"
  classes="$(classify_file "$blocks" fixture.ts | sort | tr '\n' ',')"
  local expected
  expected="$(printf '%s\n' "$@" | sort | tr '\n' ',')"
  {
    echo "fixture $name: expected=[$expected] actual=[$classes]"
  } | tee -a "$EVIDENCE_DIR/e0s-self-checks.log"
  [ "$classes" = "$expected" ] || fail "E0s: classifier fixture $name misclassified (expected [$expected], got [$classes])"
}

cat > "$SELFCHECK/seal-lag.log" <<'FIX'
 FAIL  fixture.ts > seal > guard
AssertionError: VERIFIED_SOURCE_SHA 之后出现了非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:
foo.rs
 ❯ fixture.ts:102:10
FIX
sc_expect seal-lag "$SELFCHECK/seal-lag.log" seal-coordinate-lag

cat > "$SELFCHECK/seal-lag-via-generator.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
post-verification diff guard failed: ✗ VERIFIED_SOURCE_SHA 之后出现非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:
  - scripts/rust-tauri/r02_t08_legacy_entry_regression.sh
 ❯ fixture.ts:255:22
FIX
sc_expect seal-lag-via-generator "$SELFCHECK/seal-lag-via-generator.log" seal-coordinate-lag

cat > "$SELFCHECK/uncommitted.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
warning: in the working copy of 'a.headers', CRLF will be replaced by LF the next time Git touches it
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["rust/x.rs"]
 ❯ fixture.ts:255:22
FIX
sc_expect uncommitted "$SELFCHECK/uncommitted.log" uncommitted-source-rejection

cat > "$SELFCHECK/generator-crash.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
Traceback (most recent call last):
RuntimeError: boom
 ❯ fixture.ts:255:22
FIX
sc_expect generator-crash "$SELFCHECK/generator-crash.log" UNRECOGNIZED

cat > "$SELFCHECK/bare-wrapper.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
 ❯ fixture.ts:255:22
FIX
sc_expect bare-wrapper "$SELFCHECK/bare-wrapper.log" UNRECOGNIZED

cat > "$SELFCHECK/mixed-foreign.log" <<'FIX'
 FAIL  fixture.ts > seal > guard
AssertionError: VERIFIED_SOURCE_SHA 之后出现了非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:
foo.rs
Error: EACCES: permission denied, open '/x'
 ❯ fixture.ts:102:10
FIX
sc_expect mixed-foreign "$SELFCHECK/mixed-foreign.log" seal-coordinate-lag UNRECOGNIZED

# R5-F01: TWO independent failure blocks in ONE file — a recognized
# coordinate-lag block plus a bare-wrapper block. Per-file aggregation
# filed this as pure seal-coordinate-lag (the unknown failure was
# absorbed); per-block classification must surface BOTH verdicts.
cat > "$SELFCHECK/cross-block-mix.log" <<'FIX'
 FAIL  fixture.ts > seal > guard
AssertionError: VERIFIED_SOURCE_SHA 之后出现了非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:
foo.rs
 ❯ fixture.ts:102:10
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
 ❯ fixture.ts:255:22
FIX
sc_expect cross-block-mix "$SELFCHECK/cross-block-mix.log" seal-coordinate-lag UNRECOGNIZED

# R5-F01: the guard prefix with a NON-coordinate reason (missing
# coordinate file) is NOT seal-coordinate-lag.
cat > "$SELFCHECK/non-coordinate-guard.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
post-verification diff guard failed: ✗ 缺少坐标文件: VERIFIED_SOURCE_SHA
 ❯ fixture.ts:255:22
FIX
sc_expect non-coordinate-guard "$SELFCHECK/non-coordinate-guard.log" UNRECOGNIZED

# R6-F01: the REAL guard's actual missing-coordinate sentence
# (.sync-audit/verify-post-verification-diff.mjs:73 — the classifier's
# older pattern list knew only the fixture sentence `缺少坐标文件`, so
# the real sentence fell through to the "registered unreadable" hole).
# Both shapes — embedded in the producer's guard line, and as a direct
# guard `✗` line (the vitest-run shape) — must be REJECTED.
cat > "$SELFCHECK/real-missing-coordinate-embedded.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
post-verification diff guard failed: ✗ 缺少 .sync-audit/verified-source-sha.txt
 ❯ fixture.ts:255:22
FIX
sc_expect real-missing-coordinate-embedded "$SELFCHECK/real-missing-coordinate-embedded.log" UNRECOGNIZED

cat > "$SELFCHECK/real-missing-coordinate-direct.log" <<'FIX'
 FAIL  fixture.ts > seal > guard-runs-guard-directly
AssertionError: guard unexpectedly refused
✗ 缺少 .sync-audit/verified-source-sha.txt
 ❯ fixture.ts:30:5
FIX
sc_expect real-missing-coordinate-direct "$SELFCHECK/real-missing-coordinate-direct.log" UNRECOGNIZED

# R6-F01: the REAL guard's operational-failure sentence (:98) — git diff
# itself failed (not "only coordinate lag"). Never acceptable.
cat > "$SELFCHECK/real-diff-failed.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
post-verification diff guard failed: ✗ git diff 0ed84e0b5e9821d217457e7013b4b579002ae095..HEAD 失败: Error: Command failed: git diff --name-only
 ❯ fixture.ts:255:22
FIX
sc_expect real-diff-failed "$SELFCHECK/real-diff-failed.log" UNRECOGNIZED

# R6-F01: the REAL guard's malformed / non-commit coordinate sentences
# (:78 / :87 / :90) — again never the lag class.
cat > "$SELFCHECK/real-nonhex-coordinate.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
post-verification diff guard failed: ✗ VERIFIED_SOURCE_SHA 非 40 位十六进制: not-a-sha
 ❯ fixture.ts:255:22
FIX
sc_expect real-nonhex-coordinate "$SELFCHECK/real-nonhex-coordinate.log" UNRECOGNIZED

cat > "$SELFCHECK/real-noncommit-coordinate.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
post-verification diff guard failed: ✗ VERIFIED_SOURCE_SHA 必须指向真实 commit 对象（git cat-file -t 实际返回 tree）: 1111111111111111111111111111111111111111
 ❯ fixture.ts:255:22
FIX
sc_expect real-noncommit-coordinate "$SELFCHECK/real-noncommit-coordinate.log" UNRECOGNIZED

# R6-F01: a permission failure inside the guard line — an operational
# error, never "registered-then-accepted".
cat > "$SELFCHECK/real-guard-eacces.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
post-verification diff guard failed: ✗ EACCES: permission denied, open '/x/.sync-audit/verified-source-sha.txt'
 ❯ fixture.ts:255:22
FIX
sc_expect real-guard-eacces "$SELFCHECK/real-guard-eacces.log" UNRECOGNIZED

# R5-F01: a TRUNCATED guard line — the real generator can cut the output
# before the diagnostic completes. An incomplete SIGNATURE (cut
# mid-diagnostic, still carrying a VERIFIED_SOURCE_SHA-stated marker)
# classifies as nothing known: fail closed.
cat > "$SELFCHECK/truncated-guard.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
post-verification diff guard failed: ✗ VERIFIED_SOURCE_SHA 之后出现了非审计文件改动（禁止修改生产代码/测试逻辑/runti
 ❯ fixture.ts:255:22
FIX
sc_expect truncated-guard "$SELFCHECK/truncated-guard.log" UNRECOGNIZED

# R5-F01→R6-F01: the REAL generator truncation shape — the
# guard_output[-500:] window keeps only the tail of the changed-file
# listing, so the guard line carries NO diagnostic at all (observed
# live: "…failed: .py"). R5 registered this as an "unreadable" class
# and accepted it at the candidate; R6-F01 removes that middle ground:
# the refusal proves NOTHING about its cause, so it is UNRECOGNIZED and
# fails the gate at both ends.
cat > "$SELFCHECK/guard-tail-cut.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["docs/x.json"]
post-verification diff guard failed: .py
 ❯ fixture.ts:255:22
FIX
sc_expect guard-tail-cut "$SELFCHECK/guard-tail-cut.log" UNRECOGNIZED uncommitted-source-rejection

# R6-F01: the real guard's COMPLETE refusal, in its direct (vitest-run)
# multi-line shape — the ✗ line carries the full diagnostic and the
# violator listing is its body. This IS the legitimate lag class (the
# same sentence the guard itself asserts).
# R7-F01 correction: the synthetic lead-in `AssertionError: expected
# guard to be green` is NOT a real producer shape (no test in this repo
# prints it), and this fixture's seal-only expectation was never
# actually runnable green — under the R6 classifier the lead-in line
# already made the block seal+UNRECOGNIZED (E0s never ran the full
# chain to expose it). The input is kept as a NEGATIVE control with its
# honest verdict (an unexplained assertion lead-in next to the complete
# ✗ sentence stays partly unexplained → seal + UNRECOGNIZED); the REAL
# positive shape — round2's manifestSourceRef: the
# `Error: Command failed: node …/verify-post-verification-diff.mjs`
# wrapper followed by the guard's own ✗ sentence and listing
# (tests/round2-delivery-evidence.test.ts:95-99) — is added below as
# guard-direct-full-real.
cat > "$SELFCHECK/guard-direct-full.log" <<'FIX'
 FAIL  fixture.ts > seal > guard
AssertionError: expected guard to be green
✗ VERIFIED_SOURCE_SHA 之后出现非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:
  - scripts/rust-tauri/r02_t08_legacy_entry_regression.sh
 ❯ fixture.ts:30:5
FIX
sc_expect guard-direct-full "$SELFCHECK/guard-direct-full.log" seal-coordinate-lag UNRECOGNIZED

cat > "$SELFCHECK/guard-direct-full-real.log" <<'FIX'
 FAIL  fixture.ts > round2 > manifestSourceRef
Error: Command failed: node /x/.sync-audit/verify-post-verification-diff.mjs
✗ VERIFIED_SOURCE_SHA 之后出现非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:
  - docs/a.md
  - docs/b.md
 ❯ fixture.ts:95:15
FIX
sc_expect guard-direct-full-real "$SELFCHECK/guard-direct-full-real.log" seal-coordinate-lag

# R7-F01: same-line mixing — the guard line carries the COMPLETE known
# sentence AND an appended `Error: EACCES: permission denied`. The
# sentence proves the known cause; the appended error is an unexplained
# second cause on the same line. The R6 early-`continue` accepted this
# as pure seal-coordinate-lag; it must be seal + UNRECOGNIZED.
cat > "$SELFCHECK/same-line-known-error.log" <<'FIX'
 FAIL  fixture.ts > suite > case
post-verification diff guard failed: ✗ VERIFIED_SOURCE_SHA 之后出现了非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）: Error: EACCES: permission denied
 ❯ fixture.ts:1:1
FIX
sc_expect same-line-known-error "$SELFCHECK/same-line-known-error.log" seal-coordinate-lag UNRECOGNIZED

# R7-F01: same-block mixing via an error line OUTSIDE every legitimate
# shape — `fatal: unable to access …` matched no member of the R6 finite
# foreign-prefix set and was swallowed as listing noise. Under the
# complete-structure grammar an unknown line IS foreign: seal +
# UNRECOGNIZED, never pure seal.
cat > "$SELFCHECK/mixed-known-fatal.log" <<'FIX'
 FAIL  fixture.ts > suite > case
AssertionError: VERIFIED_SOURCE_SHA 之后出现了非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:
foo.rs
fatal: unable to access '/repo': Permission denied
 ❯ fixture.ts:1:1
FIX
sc_expect mixed-known-fatal "$SELFCHECK/mixed-known-fatal.log" seal-coordinate-lag UNRECOGNIZED

# R6-F01: a FAIL block with ZERO payload — the header is followed
# immediately by the stack-frame marker (a real vitest shape when the
# assertion message is empty). extract_blocks still emits the block
# marker row, so the block is JUDGED: no diagnostic → UNRECOGNIZED.
# Previously the block vanished from the blocks file entirely while
# failed_files still listed it, and the file got an EMPTY cause row that
# passed the coverage grep — the zero-payload false green.
cat > "$SELFCHECK/zero-payload.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
 ❯ fixture.ts:255:22
FIX
sc_expect zero-payload "$SELFCHECK/zero-payload.log" UNRECOGNIZED

# R6-F01: zero payload WITHOUT a suite path — the FAIL header itself is
# the file name (the old parser set current="" here, hiding the block).
cat > "$SELFCHECK/zero-payload-no-suite.log" <<'FIX'
 FAIL  fixture.ts
 ❯ fixture.ts:255:22
FIX
sc_expect zero-payload-no-suite "$SELFCHECK/zero-payload-no-suite.log" UNRECOGNIZED

# R6-F01: a file listed as failing but with NO extractable block rows
# at all — classify_file must still answer UNRECOGNIZED (fail closed),
# never an empty class set that slips through the coverage check.
printf 'fixture-without-rows.ts\t99\t⟦FAIL-BLOCK⟧\n' > "$SELFCHECK/missing-blocks.blocks"
CLASSES_NO_ROWS="$(classify_file "$SELFCHECK/missing-blocks.blocks" fixture-absent.ts | sort | tr '\n' ',')"
[ "$CLASSES_NO_ROWS" = "UNRECOGNIZED," ] \
  || fail "E0s: a failing file with zero block rows must classify UNRECOGNIZED (got [$CLASSES_NO_ROWS])"
echo "fixture missing-block-rows: UNRECOGNIZED OK" | tee -a "$EVIDENCE_DIR/e0s-self-checks.log"

# R6-F01: SAME block, known diagnostic + unreadable guard tail. The
# recognized line explains THAT line only; the guard tail in the SAME
# block stays unproven → the block is UNRECOGNIZED (R5 accepted it via
# the "registered unreadable" class; removed).
cat > "$SELFCHECK/same-block-known-plus-guard-tail.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
post-verification diff guard failed: ✗ VERIFIED_SOURCE_SHA 之后出现非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:
  - docs/a.md
  - docs/b.md
post-verification diff guard failed: .py
 ❯ fixture.ts:255:22
FIX
sc_expect same-block-known-plus-guard-tail "$SELFCHECK/same-block-known-plus-guard-tail.log" UNRECOGNIZED seal-coordinate-lag

# ── R8-F01 classifier self-checks: no structural category is a
# pass-through; every body line is bound to its producing lead; every
# accepted line must be COMPLETE. Each fixture reproduces a shape the R8
# review demonstrated reaching a pure known verdict under the R7
# classifier. ────────────────────────────────────────────────────────────
# R8-F01 compact error: a no-space single token `Error:EACCES` next to the
# complete known sentence is NOT a violator path (a path token contains no
# colon) — seal + UNRECOGNIZED, never pure seal.
cat > "$SELFCHECK/compact-error-mixed.log" <<'FIX'
 FAIL  fixture.ts > suite > case
AssertionError: VERIFIED_SOURCE_SHA 之后出现了非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:
Error:EACCES
 ❯ fixture.ts:1:1
FIX
sc_expect compact-error-mixed "$SELFCHECK/compact-error-mixed.log" seal-coordinate-lag UNRECOGNIZED

# R8-F01 list mixing: a `  - path` listing line with a trailing error is
# not a complete listing body line — seal + UNRECOGNIZED.
cat > "$SELFCHECK/list-line-mixed.log" <<'FIX'
 FAIL  fixture.ts > suite > case
post-verification diff guard failed: ✗ VERIFIED_SOURCE_SHA 之后出现非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:
  - docs/a.md Error: EACCES: permission denied
 ❯ fixture.ts:1:1
FIX
sc_expect list-line-mixed "$SELFCHECK/list-line-mixed.log" seal-coordinate-lag UNRECOGNIZED

# R8-F01 warning mixing: a git CRLF warning with a trailing error is not
# git's complete sentence — seal + UNRECOGNIZED.
cat > "$SELFCHECK/warning-mixed.log" <<'FIX'
 FAIL  fixture.ts > suite > case
AssertionError: VERIFIED_SOURCE_SHA 之后出现了非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:
warning: in the working copy of 'a', CRLF will be replaced by LF; Error: EACCES
 ❯ fixture.ts:1:1
FIX
sc_expect warning-mixed "$SELFCHECK/warning-mixed.log" seal-coordinate-lag UNRECOGNIZED

# R8-F01 matcher-tail mixing: the vitest matcher summary with an appended
# error does not match the complete summary shape — seal + UNRECOGNIZED.
cat > "$SELFCHECK/matcher-tail-mixed.log" <<'FIX'
 FAIL  fixture.ts > suite > case
AssertionError: VERIFIED_SOURCE_SHA 之后出现了非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:
foo.rs: expected [ foo ] to deeply equal [] Error: EACCES
 ❯ fixture.ts:1:1
FIX
sc_expect matcher-tail-mixed "$SELFCHECK/matcher-tail-mixed.log" seal-coordinate-lag UNRECOGNIZED

# R8-F01 wrapper mixing: `Error: Command failed:` is structural ONLY for
# the three real family commands, ONLY as the first payload row — a
# `; Error:` tail or a foreign command (`node tool.js`) is FOREIGN.
cat > "$SELFCHECK/wrapper-mixed.log" <<'FIX'
 FAIL  fixture.ts > suite > case
AssertionError: VERIFIED_SOURCE_SHA 之后出现了非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:
Error: Command failed: node tool.js; Error: EACCES
 ❯ fixture.ts:1:1
FIX
sc_expect wrapper-mixed "$SELFCHECK/wrapper-mixed.log" seal-coordinate-lag UNRECOGNIZED

cat > "$SELFCHECK/wrapper-foreign-command.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: node tool.js
 ❯ fixture.ts:1:1
FIX
sc_expect wrapper-foreign-command "$SELFCHECK/wrapper-foreign-command.log" UNRECOGNIZED

cat > "$SELFCHECK/wrapper-not-first-row.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
unexplained first row
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
 ❯ fixture.ts:1:1
FIX
sc_expect wrapper-not-first-row "$SELFCHECK/wrapper-not-first-row.log" UNRECOGNIZED

# R8-F01 producer binding: body lines are legitimate ONLY inside the block
# whose complete lead is present. A bare token, a `  - ` listing, or a
# diff body WITHOUT its producing lead line is FOREIGN.
cat > "$SELFCHECK/bare-token-without-lead.log" <<'FIX'
 FAIL  fixture.ts > suite > case
foo.rs
 ❯ fixture.ts:1:1
FIX
sc_expect bare-token-without-lead "$SELFCHECK/bare-token-without-lead.log" UNRECOGNIZED

cat > "$SELFCHECK/listing-without-guard-line.log" <<'FIX'
 FAIL  fixture.ts > suite > case
  - docs/a.md
 ❯ fixture.ts:1:1
FIX
sc_expect listing-without-guard-line "$SELFCHECK/listing-without-guard-line.log" UNRECOGNIZED

cat > "$SELFCHECK/diff-body-without-lead.log" <<'FIX'
 FAIL  fixture.ts > suite > case
- Expected
+ Received
- []
+ [
 ❯ fixture.ts:1:1
FIX
sc_expect diff-body-without-lead "$SELFCHECK/diff-body-without-lead.log" UNRECOGNIZED

# R8-F01 positive controls from the REAL polluted log (artifacts/
# rust-tauri/R01/STAGE_REPAIR_R5/f02-npm-test-iso-r5.txt:9450-9462): the
# elided matcher summary `…(N)` with the full toEqual diff body, and the
# inline-quoted form — both complete producer shapes inside an
# AssertionError-led block: pure seal, no UNRECOGNIZED.
cat > "$SELFCHECK/real-matcher-elided.log" <<'FIX'
 FAIL  fixture.ts > seal > guard
AssertionError: VERIFIED_SOURCE_SHA 之后出现了非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:
docs/a.rs
docs/b.md: expected [ …(2103) ] to deeply equal []

- Expected
+ Received

- []
+ [
+   "docs/a.rs",
+   "docs/b.md",
+ ]

 ❯ fixture.ts:104:7
FIX
sc_expect real-matcher-elided "$SELFCHECK/real-matcher-elided.log" seal-coordinate-lag

cat > "$SELFCHECK/real-matcher-inline.log" <<'FIX'
 FAIL  fixture.ts > seal > guard
AssertionError: VERIFIED_SOURCE_SHA 之后出现了非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:
docs/a.rs
docs/b.rs: expected [ "docs/a.rs", "docs/b.rs" ] to deeply equal []
 ❯ fixture.ts:104:7
FIX
sc_expect real-matcher-inline "$SELFCHECK/real-matcher-inline.log" seal-coordinate-lag

# ── R9-F01 firstDiff structure self-checks: the firstDiff body must be
# the REAL producers' complete JSON array (round2/round3
# json.dumps(diff_paths[:3], ensure_ascii=False)) — legal paths that
# merely CONTAIN `]`, `,`, space, quotes or backslashes are NOT
# rejected, while non-JSON payloads, mixed errors, truncation, `[]`,
# >3 elements and trailing text all stay mixed (known cause AND
# UNRECOGNIZED for the unexplained remainder — never the pure class).
cat > "$SELFCHECK/uncommitted-bracket-path.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["docs/a]b.json", "docs/c,d e.txt"]
 ❯ fixture.ts:255:22
FIX
sc_expect uncommitted-bracket-path "$SELFCHECK/uncommitted-bracket-path.log" uncommitted-source-rejection

cat > "$SELFCHECK/uncommitted-escaped-path.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["a\"b\\c", "docs/x.json", "e\tf"]
 ❯ fixture.ts:255:22
FIX
sc_expect uncommitted-escaped-path "$SELFCHECK/uncommitted-escaped-path.log" uncommitted-source-rejection

cat > "$SELFCHECK/uncommitted-nonascii-path.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["文档/中文.json", "docs/x.json"]
 ❯ fixture.ts:255:22
FIX
sc_expect uncommitted-nonascii-path "$SELFCHECK/uncommitted-nonascii-path.log" uncommitted-source-rejection

cat > "$SELFCHECK/uncommitted-bare-error.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=[Error:EACCES]
 ❯ fixture.ts:255:22
FIX
sc_expect uncommitted-bare-error "$SELFCHECK/uncommitted-bare-error.log" uncommitted-source-rejection UNRECOGNIZED

cat > "$SELFCHECK/uncommitted-mixed-error.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["docs/x.json" Error:EACCES]
 ❯ fixture.ts:255:22
FIX
sc_expect uncommitted-mixed-error "$SELFCHECK/uncommitted-mixed-error.log" uncommitted-source-rejection UNRECOGNIZED

cat > "$SELFCHECK/uncommitted-truncated.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["docs/x.json"
 ❯ fixture.ts:255:22
FIX
sc_expect uncommitted-truncated "$SELFCHECK/uncommitted-truncated.log" uncommitted-source-rejection UNRECOGNIZED

cat > "$SELFCHECK/uncommitted-empty-array.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=[]
 ❯ fixture.ts:255:22
FIX
sc_expect uncommitted-empty-array "$SELFCHECK/uncommitted-empty-array.log" uncommitted-source-rejection UNRECOGNIZED

cat > "$SELFCHECK/uncommitted-four-elements.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["a.rs", "b.rs", "c.rs", "d.rs"]
 ❯ fixture.ts:255:22
FIX
sc_expect uncommitted-four-elements "$SELFCHECK/uncommitted-four-elements.log" uncommitted-source-rejection UNRECOGNIZED

cat > "$SELFCHECK/uncommitted-trailing-junk.log" <<'FIX'
 FAIL  fixture.ts > round2 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["a.rs"] Error: EACCES: permission denied
 ❯ fixture.ts:255:22
FIX
sc_expect uncommitted-trailing-junk "$SELFCHECK/uncommitted-trailing-junk.log" uncommitted-source-rejection UNRECOGNIZED

# ── repair-group10: guard-tail WINDOW fixtures (gate-r2 evidence — the
# exact blocks that made gate round 2 fail E5 with `UNRECOGNIZED,
# seal-coordinate-lag, uncommitted-source-rejection` for round2/round3).
# Producer mechanics (create-delivery-patch.py:449 /
# create-round3-patch.py:449): the generators embed the guard refusal as
# `post-verification diff guard failed: {guard_output[-500:]}`; with a
# listing longer than the window the ✗ sentence is gone and the line
# carries a front-cut path fragment + the guard's `  - ` listing lines,
# ending mid-token where the guard's own output was lost past the 64 KiB
# pipe buffer at process.exit (verified: the captured guard part is byte-
# exactly 65536 in the gate-r2 log). Both real windows are EXACTLY 500
# code points reconstructed. ─────────────────────────────────────────────
# POSITIVE 1 — the real gate-r2 round2 block 4 text (wrapper + 2 of the
# 72 real CRLF warning lines kept — classification is per line, the count
# changes nothing — + the real firstDiff line + the real 500-point
# window). Source: artifacts/rust-tauri/R02/final-candidate-
# a29e5adbb404ad70/A16/legacy-entry/e5-candidate-blocks.txt, block 4.
cat > "$SELFCHECK/guard-window-r2-round2.log" <<'FIX'
 FAIL  fixture.ts > round2 > R10-09 replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
warning: in the working copy of 'artifacts/rust-tauri/R02/T07/redaction-scan/p1-execute-ok.headers', CRLF will be replaced by LF the next time Git touches it
warning: in the working copy of 'artifacts/rust-tauri/R02/final-candidate-a29e5adbb404ad70/A13/redaction-scan/p2-ws-bad-query.headers', CRLF will be replaced by LF the next time Git touches it
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["artifacts/rust-tauri/R02/final-candidate-a0af83666f89cc4b/A01/a01-cargo-metadata.err", "artifacts/rust-tauri/R02/final-candidate-a0af83666f89cc4b/A01/a01-cargo-metadata.json", "artifacts/rust-tauri/R02/final-candidate-a0af83666f89cc4b/A01/a01-child-snapshot.tsv"]
post-verification diff guard failed: udit-r17-cli-rust-targeted/typecheck-05/exit-codes.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-05/start-utc.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/candidate-sha256.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/commands.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/end-utc.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/exit-codes.txt
  - artifacts/rust-t
 ❯ fixture.ts:255:22
FIX
sc_expect guard-window-r2-round2 "$SELFCHECK/guard-window-r2-round2.log" seal-coordinate-lag uncommitted-source-rejection

# POSITIVE 2 — the real gate-r2 round3 block 6 text (create-round3-patch
# wrapper; same deterministic window — both generators embed the SAME
# guard output). Source: e5-candidate-blocks.txt, block 6.
cat > "$SELFCHECK/guard-window-r2-round3.log" <<'FIX'
 FAIL  fixture.ts > round3 > replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round3-c01-c03/create-round3-patch.py
warning: in the working copy of 'artifacts/rust-tauri/R02/T08/verify-stage/A13/redaction-scan/p1-execute-ok.headers', CRLF will be replaced by LF the next time Git touches it
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["artifacts/rust-tauri/R02/final-candidate-a0af83666f89cc4b/A01/a01-cargo-metadata.err", "artifacts/rust-tauri/R02/final-candidate-a0af83666f89cc4b/A01/a01-cargo-metadata.json", "artifacts/rust-tauri/R02/final-candidate-a0af83666f89cc4b/A01/a01-child-snapshot.tsv"]
post-verification diff guard failed: udit-r17-cli-rust-targeted/typecheck-05/exit-codes.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-05/start-utc.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/candidate-sha256.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/commands.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/end-utc.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/exit-codes.txt
  - artifacts/rust-t
 ❯ fixture.ts:220:18
FIX
sc_expect guard-window-r2-round3 "$SELFCHECK/guard-window-r2-round3.log" seal-coordinate-lag uncommitted-source-rejection

# POSITIVE 3 — derived (not gate-r2 bytes; producer-mechanics variant):
# the SAME window geometry with the guard output NOT pipe-truncated, so
# the final window line is a COMPLETE listing line. The fragment is
# padded so the reconstruction is exactly 500 code points.
cat > "$SELFCHECK/guard-window-complete-end.log" <<'FIX'
 FAIL  fixture.ts > round2 > R10-09 replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["docs/x.json"]
post-verification diff guard failed: rust-tauri/R02/audit-r17-cli
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-05/start-utc.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/candidate-sha256.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/commands.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/end-utc.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/exit-codes.txt
  - docs/rust-tauri/R01/API_COMPAT_MATRIX.json
 ❯ fixture.ts:255:22
FIX
sc_expect guard-window-complete-end "$SELFCHECK/guard-window-complete-end.log" seal-coordinate-lag uncommitted-source-rejection

# NEGATIVE 1 — single-point mutation: one character dropped from the
# window's final line (500 → 499). The geometry anchor fails closed.
cat > "$SELFCHECK/guard-window-off-by-one.log" <<'FIX'
 FAIL  fixture.ts > round2 > R10-09 replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["docs/x.json"]
post-verification diff guard failed: udit-r17-cli-rust-targeted/typecheck-05/exit-codes.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-05/start-utc.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/candidate-sha256.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/commands.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/end-utc.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/exit-codes.txt
  - artifacts/rust-
 ❯ fixture.ts:255:22
FIX
sc_expect guard-window-off-by-one "$SELFCHECK/guard-window-off-by-one.log" uncommitted-source-rejection UNRECOGNIZED

# NEGATIVE 2 — trailing junk appended to the final window line: neither
# the listing grammar nor the 500-point geometry survives.
cat > "$SELFCHECK/guard-window-trailing-junk.log" <<'FIX'
 FAIL  fixture.ts > round2 > R10-09 replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["docs/x.json"]
post-verification diff guard failed: udit-r17-cli-rust-targeted/typecheck-05/exit-codes.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-05/start-utc.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/candidate-sha256.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/commands.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/end-utc.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/exit-codes.txt
  - artifacts/rust-t Error: EACCES: permission denied
 ❯ fixture.ts:255:22
FIX
sc_expect guard-window-trailing-junk "$SELFCHECK/guard-window-trailing-junk.log" uncommitted-source-rejection UNRECOGNIZED

# NEGATIVE 3 — the fragment replaced by SENTENCE material (`artifacts）:`
# is a legal mid-sentence cut suffix of the guard's ✗ line): sentence
# material can never pose as a path fragment — UNRECOGNIZED, and the
# whole run loses its producing lead.
cat > "$SELFCHECK/guard-window-sentence-fragment.log" <<'FIX'
 FAIL  fixture.ts > round2 > R10-09 replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["docs/x.json"]
post-verification diff guard failed: artifacts）:
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-05/start-utc.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/candidate-sha256.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/commands.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/end-utc.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/exit-codes.txt
  - artifacts/rust-t
 ❯ fixture.ts:255:22
FIX
sc_expect guard-window-sentence-fragment "$SELFCHECK/guard-window-sentence-fragment.log" uncommitted-source-rejection UNRECOGNIZED

# NEGATIVE 4 — producer binding: the identical valid window under the
# NODE-GUARD wrapper (a producer that never prints the embedded prefix)
# must NOT classify — the window form exists only under a GENERATOR
# wrapper.
cat > "$SELFCHECK/guard-window-wrong-producer.log" <<'FIX'
 FAIL  fixture.ts > round2 > manifestSourceRef
Error: Command failed: node /x/.sync-audit/verify-post-verification-diff.mjs
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["docs/x.json"]
post-verification diff guard failed: udit-r17-cli-rust-targeted/typecheck-05/exit-codes.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-05/start-utc.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/candidate-sha256.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/commands.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/end-utc.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/exit-codes.txt
  - artifacts/rust-t
 ❯ fixture.ts:95:15
FIX
sc_expect guard-window-wrong-producer "$SELFCHECK/guard-window-wrong-producer.log" uncommitted-source-rejection UNRECOGNIZED

# NEGATIVE 5 — one complete listing line deleted from the run (500 → 419
# points): the geometry anchor fails closed.
cat > "$SELFCHECK/guard-window-missing-line.log" <<'FIX'
 FAIL  fixture.ts > round2 > R10-09 replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
current source manifest does not match HEAD (uncommitted or untracked source changes): firstDiff=["docs/x.json"]
post-verification diff guard failed: udit-r17-cli-rust-targeted/typecheck-05/exit-codes.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-05/start-utc.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/candidate-sha256.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/end-utc.txt
  - artifacts/rust-tauri/R02/audit-r17-cli-rust-targeted/typecheck-06/exit-codes.txt
  - artifacts/rust-t
 ❯ fixture.ts:255:22
FIX
sc_expect guard-window-missing-line "$SELFCHECK/guard-window-missing-line.log" uncommitted-source-rejection UNRECOGNIZED

# ── RR2 B-R2 R2: patch-too-large FORM-EVOLUTION fixtures (the family
# registered by repair-group10, evolved: the tree outgrew git's own
# apply ceiling — MAX_APPLY_SIZE ≈ 1023 MiB vs the ≥3.67 GB regenerated
# patch — so `git apply --cached` refuses with `error: patch too large`
# before the generators reach their guard/uncommitted checkpoints).
# Producer mechanics: create-delivery-patch.py:359 raises RuntimeError
# (uncaught → python3 prints the 6-row traceback, File frames citing the
# wrapped script at lines 457/<module> and 359/replay_and_verify, ending
# in the complete sentence); create-round3-patch.py:342 raises SystemExit
# (a string message prints BARE — no traceback). Evidence: artifacts/
# rust-tauri/R05/RR2/B-R2/R2/s5-full-4/ev/legacy-entry/
# e5-candidate-blocks.txt round2 block 4 / round3 block 6 + F44. ──────
# POSITIVE 1 — the real s5-full-4 round2 block-4 shape (wrapper + one
# real CRLF warning kept + the complete 6-row traceback).
cat > "$SELFCHECK/ptl-r2-real.log" <<'FIX'
 FAIL  fixture.ts > round2 > R10-09 replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
warning: in the working copy of 'artifacts/rust-tauri/R02/T07/redaction-scan/p1-execute-ok.headers', CRLF will be replaced by LF the next time Git touches it
Traceback (most recent call last):
  File "/x/artifacts/f1-f12-repair/round2/create-delivery-patch.py", line 457, in <module>
    outcome = replay_and_verify(patch, stored)
  File "/x/artifacts/f1-f12-repair/round2/create-delivery-patch.py", line 359, in replay_and_verify
    raise RuntimeError(f"patch replay failed: {apply.stdout.decode(errors='replace')[:2000]}")
RuntimeError: patch replay failed: error: patch too large

 ❯ fixture.ts:255:22
FIX
sc_expect ptl-r2-real "$SELFCHECK/ptl-r2-real.log" seal-coordinate-lag

# POSITIVE 2 — the real s5-full-4 round3 block-6 shape (round3 wrapper +
# one real warning + the bare SystemExit line).
cat > "$SELFCHECK/ptl-r3-real.log" <<'FIX'
 FAIL  fixture.ts > round3 > live replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round3-c01-c03/create-round3-patch.py
warning: in the working copy of 'artifacts/rust-tauri/R02/T07/redaction-scan/p1-execute-ok.headers', CRLF will be replaced by LF the next time Git touches it
patch replay failed: error: patch too large

 ❯ fixture.ts:220:18
FIX
sc_expect ptl-r3-real "$SELFCHECK/ptl-r3-real.log" seal-coordinate-lag

# NEGATIVE 1 — truncated final sentence (`too` cut): the end anchor is
# gone; the whole traceback stays unexplained.
cat > "$SELFCHECK/ptl-r2-truncated.log" <<'FIX'
 FAIL  fixture.ts > round2 > R10-09 replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
Traceback (most recent call last):
  File "/x/artifacts/f1-f12-repair/round2/create-delivery-patch.py", line 457, in <module>
    outcome = replay_and_verify(patch, stored)
  File "/x/artifacts/f1-f12-repair/round2/create-delivery-patch.py", line 359, in replay_and_verify
    raise RuntimeError(f"patch replay failed: {apply.stdout.decode(errors='replace')[:2000]}")
RuntimeError: patch replay failed: error: patch too
 ❯ fixture.ts:255:22
FIX
sc_expect ptl-r2-truncated "$SELFCHECK/ptl-r2-truncated.log" UNRECOGNIZED

# NEGATIVE 2 — a DIFFERENT git error text (`error: corrupt patch at
# line 5`): not the size-ceiling family — another failure, fail closed.
cat > "$SELFCHECK/ptl-r2-other-git-error.log" <<'FIX'
 FAIL  fixture.ts > round2 > R10-09 replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
Traceback (most recent call last):
  File "/x/artifacts/f1-f12-repair/round2/create-delivery-patch.py", line 457, in <module>
    outcome = replay_and_verify(patch, stored)
  File "/x/artifacts/f1-f12-repair/round2/create-delivery-patch.py", line 359, in replay_and_verify
    raise RuntimeError(f"patch replay failed: {apply.stdout.decode(errors='replace')[:2000]}")
RuntimeError: patch replay failed: error: corrupt patch at line 5
 ❯ fixture.ts:255:22
FIX
sc_expect ptl-r2-other-git-error "$SELFCHECK/ptl-r2-other-git-error.log" UNRECOGNIZED

# NEGATIVE 3 — a FOREIGN SystemExit message under the round3 wrapper
# (the generator's line-239 `round3 patch is empty`): a different
# producer failure, never the size-ceiling sentence.
cat > "$SELFCHECK/ptl-r3-other-systemexit.log" <<'FIX'
 FAIL  fixture.ts > round3 > live replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round3-c01-c03/create-round3-patch.py
round3 patch is empty
 ❯ fixture.ts:220:18
FIX
sc_expect ptl-r3-other-systemexit "$SELFCHECK/ptl-r3-other-systemexit.log" UNRECOGNIZED

# NEGATIVE 4 — cross-producer: the round3 BARE sentence under the
# round2 wrapper (the round2 producer always prints the traceback).
cat > "$SELFCHECK/ptl-cross-bare-under-r2.log" <<'FIX'
 FAIL  fixture.ts > round2 > R10-09 replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
patch replay failed: error: patch too large
 ❯ fixture.ts:255:22
FIX
sc_expect ptl-cross-bare-under-r2 "$SELFCHECK/ptl-cross-bare-under-r2.log" UNRECOGNIZED

# NEGATIVE 5 — cross-producer: the complete round2 traceback under the
# round3 wrapper (the round3 producer raises SystemExit — bare line
# only).
cat > "$SELFCHECK/ptl-cross-traceback-under-r3.log" <<'FIX'
 FAIL  fixture.ts > round3 > live replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round3-c01-c03/create-round3-patch.py
Traceback (most recent call last):
  File "/x/artifacts/f1-f12-repair/round2/create-delivery-patch.py", line 457, in <module>
    outcome = replay_and_verify(patch, stored)
  File "/x/artifacts/f1-f12-repair/round2/create-delivery-patch.py", line 359, in replay_and_verify
    raise RuntimeError(f"patch replay failed: {apply.stdout.decode(errors='replace')[:2000]}")
RuntimeError: patch replay failed: error: patch too large
 ❯ fixture.ts:220:18
FIX
sc_expect ptl-cross-traceback-under-r3 "$SELFCHECK/ptl-cross-traceback-under-r3.log" UNRECOGNIZED

# NEGATIVE 6 — single-point mutation: the first File frame's line number
# 458 instead of 457 (a different generator build would be a different
# producer). The frame anchor fails closed.
cat > "$SELFCHECK/ptl-r2-mutated-frame.log" <<'FIX'
 FAIL  fixture.ts > round2 > R10-09 replay
Error: Command failed: python3 /x/artifacts/f1-f12-repair/round2/create-delivery-patch.py
Traceback (most recent call last):
  File "/x/artifacts/f1-f12-repair/round2/create-delivery-patch.py", line 458, in <module>
    outcome = replay_and_verify(patch, stored)
  File "/x/artifacts/f1-f12-repair/round2/create-delivery-patch.py", line 359, in replay_and_verify
    raise RuntimeError(f"patch replay failed: {apply.stdout.decode(errors='replace')[:2000]}")
RuntimeError: patch replay failed: error: patch too large
 ❯ fixture.ts:255:22
FIX
sc_expect ptl-r2-mutated-frame "$SELFCHECK/ptl-r2-mutated-frame.log" UNRECOGNIZED

# NEGATIVE 7 — producer binding: the complete round2 shape under the
# NODE-guard wrapper (verify-post-verification-diff.mjs never spawns the
# generators' replay step).
cat > "$SELFCHECK/ptl-node-guard-wrapper.log" <<'FIX'
 FAIL  fixture.ts > seal > guard
Error: Command failed: node /x/.sync-audit/verify-post-verification-diff.mjs
Traceback (most recent call last):
  File "/x/artifacts/f1-f12-repair/round2/create-delivery-patch.py", line 457, in <module>
    outcome = replay_and_verify(patch, stored)
  File "/x/artifacts/f1-f12-repair/round2/create-delivery-patch.py", line 359, in replay_and_verify
    raise RuntimeError(f"patch replay failed: {apply.stdout.decode(errors='replace')[:2000]}")
RuntimeError: patch replay failed: error: patch too large
 ❯ fixture.ts:95:20
FIX
sc_expect ptl-node-guard-wrapper "$SELFCHECK/ptl-node-guard-wrapper.log" UNRECOGNIZED

# Positive control: a GREEN log (no FAIL block at all) parses to an empty
# failing-file set and zero classes — the legal all-green state the
# pristine baseline is expected to produce (never an error).
cat > "$SELFCHECK/green.log" <<'FIX'
 ✓ tests/green.test.ts (3 tests) 4ms
 Test Files  1 passed (1)
      Tests  3 passed (3)
FIX
GREEN_FAILED="$(failed_files "$SELFCHECK/green.log")"
GREEN_BLOCKS="$(extract_blocks "$SELFCHECK/green.log" "$SELFCHECK" | wc -l | tr -d ' ')"
[ -z "$GREEN_FAILED" ] && [ "$GREEN_BLOCKS" = "0" ] \
  || fail "E0s: a green log must parse to zero failures (got files=[$GREEN_FAILED] blocks=$GREEN_BLOCKS)"
echo "fixture green-log: zero failed files, zero blocks — legal no-failure state OK" | tee -a "$EVIDENCE_DIR/e0s-self-checks.log"

# Parseability predicate (R5-F01 + R7-F01 evidence completeness + R8-F01
# summary completeness): a run whose exit code disagrees with its parsable
# content is UNPARSEABLE/contradictory, never classifiable. The vitest
# completion summary is COMPLETE only when BOTH lines exist exactly once
# (`Test Files …` AND `Tests …`) and each is the full producer shape:
#   ` *<label> +(<N> (failed|passed|skipped|todo) \| )*<N> \(<total>\)`
# with every component counted once and the components summing to the
# parenthesized total (real producer output:
# ` Test Files  3 failed | 1463 passed | 3 skipped (1469)` /
# `      Tests  6 failed | 14943 passed | 15 skipped (14964)` —
# artifacts/rust-tauri/R01/STAGE_REPAIR_R5/f02-npm-test-iso-r5.txt:11662).
#   parseable_run <log> <exit-code>:
#   exit 0   → GREEN      only when both summary lines are complete and
#              self-consistent AND count ZERO failures AND no FAIL block;
#              CONTRADICTORY when the log shows a FAIL block or a failed
#              count (an exit-0 run claiming failures);
#              UNPARSEABLE for anything less (empty log, missing/truncated
#              `Test Files 1` with no verdict, only one of the two lines).
#   exit ≠0  → PARSABLE   only when the summary is complete, counts ≥1
#              failure, and no FAIL-block EVIDENCE LOSS: the real
#              reporter emits one ` FAIL ` header per failed test (every
#              observed red log: 6==6, 4==4, 1==1) AND one per failed
#              SUITE block (the no-` > ` collection-error shape that
#              fails the file without any failed test), and a single
#              case may surface more than one error header — so the
#              provable invariant is ONE-DIRECTIONAL: fewer headers than
#              the Tests failed count means blocks were lost / the log
#              truncated (CONTRADICTORY); MORE headers are legal
#              producer structure (R9-F01: the old equality requirement
#              mis-filed suite-error and multi-error reds as
#              contradictions — a false rejection, never a false
#              acceptance). R10-F01 adds TWO further floors, both
#              one-directional for the same reason (only LOSS is
#              provable; supersets are legal producer structure):
#              (a) FILE-level — every failed file (case failures AND
#              suite-error blocks) emits at least one header, so fewer
#              raw headers than the Test Files failed count is header
#              LOSS: `Test Files 1 failed` + `Tests 0 failed` + zero
#              headers is CONTRADICTORY, not PARSABLE (the R10-F01 hole:
#              the old case-floor is vacuous exactly when tests_failed
#              is 0, so a detail-stripped suite-error red parsed clean,
#              produced an EMPTY failed-files list, made every
#              downstream coverage loop vacuous, and still printed the
#              GREEN verdict);
        #              (b) summary COHERENCE — Tests failed > 0 with Test Files
        #              failed == 0 is not a real producer summary (a failed test
        #              always fails its file): headers surviving while the Test
        #              Files line claims a pass is misattributed evidence;
        #              CONTRADICTORY. R11-F02 adds a third fail-closed floor:
        #              a FAIL header whose normalized identity is EMPTY
        #              (damaged/truncated — no real producer emits a nameless
        #              header) is CONTRADICTORY even when every count floor is
        #              numerically satisfied, because the blank identity would
        #              pass through failed_files as a skipped blank line and
        #              leave the failure unattributed while the gate turned
        #              GREEN; an optional third argument <counts-out>
#              additionally records the parsed evidence state (raw
#              headers, distinct header files, summary counts) for the
#              E5 cross-check of the extracted failed-file list;
#              CONTRADICTORY when the summary is complete but disagrees
#              with the exit code or the FAIL-block count;
#              UNPARSEABLE when the summary is missing or incomplete.
parseable_run() {
  # R10-F01: optional third argument <counts-out> — when given, the parsed
  # evidence state (raw FAIL-header count, distinct FAIL-header files,
  # summary failure counts, summary-line multiplicities) is written to that
  # file so the E5 flow can cross-check the EXTRACTED failed-file list
  # against the summary's own failed-file count. Without it, output and
  # verdicts are byte-identical to the two-argument form (every E0s
  # fixture call stays two-argument).
  awk -v code="$2" -v counts_out="${3:-}" '
    function reset() { files_failed = "?"; files_total = "?"; tests_failed = "?"; tests_total = "?" }
    function bad(label, why) { bad_label = label; bad_why = why; return 0 }
    # parse_summary <raw> <label>: fills cnt_<word>; 1 on a complete,
    # self-consistent line, 0 otherwise.
    function parse_summary(raw, label,    s, tail, total, m, n, i, comp, val, w, sum) {
      s = raw
      sub(/^ +/, "", s)
      if (label == "files") {
        if (substr(s, 1, 11) != "Test Files ") return bad(label, "not a Test Files line")
        s = substr(s, 12)
      } else {
        if (substr(s, 1, 6) != "Tests ") return bad(label, "not a Tests line")
        s = substr(s, 7)
      }
      sub(/^ +/, "", s)
      if (s !~ /\([0-9]+\)$/) return bad(label, "no (total) verdict — truncated")
      tail = s
      sub(/^.*\(/, "", tail); sub(/\)$/, "", tail)
      total = tail + 0
      s = substr(s, 1, length(s) - length(tail) - 2)
      sub(/ +$/, "", s)
      if (s == "") return bad(label, "no count components")
      n = split(s, comp, / \| /)
      sum = 0
      delete seen_word
      for (i = 1; i <= n; i++) {
        if (comp[i] !~ /^[0-9]+ (failed|passed|skipped|todo)$/) return bad(label, "malformed component: " comp[i])
        val = comp[i]; sub(/ .*/, "", val)
        w = comp[i]; sub(/^[0-9]+ /, "", w)
        if (w in seen_word) return bad(label, "duplicate " w " count")
        seen_word[w] = 1
        cnt[label "_" w] = val + 0
        sum += val + 0
      }
      if (sum != total) return bad(label, "components sum " sum " != total " total)
      return 1
    }
    /^ FAIL / {
      fails++
      # R10-F01: normalize the header to its FILE path with the SAME rules
      # failed_files() uses (strip the ` FAIL ` lead, cut at the first
      # ` > `, drop a trailing ` (N tests…` suffix) and collect the
      # distinct set — the file-level floors below and the downstream
      # cross-check both need the per-file view, not just the raw count.
      hline = $0
      sub(/^ FAIL +/, "", hline)
      if (hline ~ / > /) sub(/ > .*/, "", hline)
      sub(/ \([0-9]+ tests.*/, "", hline)
      seen_file[hline] = 1
    }
    /^ *Test Files +[0-9]/ { tf_lines++; tf_raw = $0 }
    /^ *Tests +[0-9]/ { t_lines++; t_raw = $0 }
    END {
      fails += 0
      distinct_files = 0
      for (k in seen_file) distinct_files++
      complete = (tf_lines == 1 && t_lines == 1)
      if (complete) {
        reset()
        complete = parse_summary(tf_raw, "files") && parse_summary(t_raw, "tests")
      }
      if (code == "0") {
        if (fails > 0) verdict = "CONTRADICTORY"
        else if (!complete) verdict = "UNPARSEABLE"
        else if (cnt["files_failed"] > 0 || cnt["tests_failed"] > 0) verdict = "CONTRADICTORY"
        else verdict = "GREEN"
      } else {
        if (!complete) verdict = "UNPARSEABLE"
        else if (cnt["files_failed"] == 0 && cnt["tests_failed"] == 0) verdict = "CONTRADICTORY"
        # R10-F01 summary coherence: a failed TEST with ZERO failed FILES
        # is not a real producer summary — every failed test fails its
        # file. This catches the shape where headers DID survive but the
        # Test Files line claims the file passed (misattributed evidence).
        else if (cnt["tests_failed"] > 0 && cnt["files_failed"] == 0) verdict = "CONTRADICTORY"
        # R9-F01: one-directional case-level evidence-loss invariant — see
        # the predicate header. cnt["tests_failed"] is unset (no failed
        # component printed) for suite-only failures and evaluates to 0.
        else if (fails < cnt["tests_failed"]) verdict = "CONTRADICTORY"
        # R10-F01: FILE-level evidence-loss invariant — every failed file
        # (case failures AND the no-` > ` suite-error shape) emits at least
        # one ` FAIL ` header in complete real reporter output, so fewer
        # raw headers than the summary failed-file count means header
        # LOSS (truncation/detail loss), never a legal producer shape:
        # `Test Files 1 failed` + `Tests 0 failed` + zero headers must NOT
        # pass as PARSABLE — the old predicate only floored headers
        # against Tests failed, which is 0 exactly for the suite-error
        # shape, so a detail-stripped log sailed through with an EMPTY
        # failed-files list and every downstream coverage loop vacuous.
        else if (fails < cnt["files_failed"]) verdict = "CONTRADICTORY"
        # R11-F02: a FAIL header whose normalized identity is EMPTY
        # (sub() stripped the ` FAIL ` lead and ALL following spaces, but
        # no file name was there — a damaged/truncated header; no real
        # producer emits a nameless header) used to still count toward
        # fails/distinct_files, satisfy BOTH floors above (1 >= 1), and
        # pass a BLANK line into failed_files that every downstream
        # `[ -z "$f" ] && continue` loop skipped — the summary-counted
        # failure was never attributed, yet the gate ended GREEN. An
        # empty identity is incoherent evidence, same family as (b):
        # CONTRADICTORY, at ANY mix with real identities.
        else if ("" in seen_file) verdict = "CONTRADICTORY"
        else verdict = "PARSABLE"
      }
      if (counts_out != "") {
        print "fails=" (fails + 0) > counts_out
        print "distinct_files=" (distinct_files + 0) > counts_out
        print "files_failed=" (cnt["files_failed"] + 0) > counts_out
        print "tests_failed=" (cnt["tests_failed"] + 0) > counts_out
        print "tf_lines=" (tf_lines + 0) > counts_out
        print "t_lines=" (t_lines + 0) > counts_out
        print "verdict=" verdict > counts_out
        close(counts_out)
      }
      print verdict
      exit
    }
  ' "$1"
}
[ "$(parseable_run "$SELFCHECK/green.log" 0)" = "GREEN" ] \
  || fail "E0s: parseability predicate misjudged a green run"
printf 'Killed by signal\n' > "$SELFCHECK/crash-no-summary.log"
[ "$(parseable_run "$SELFCHECK/crash-no-summary.log" 134)" = "UNPARSEABLE" ] \
  || fail "E0s: parseability predicate accepted a non-zero exit with no parsable failures"
printf ' FAIL  fixture.ts > a > b\nAssertionError: boom\n ❯ fixture.ts:1:1\n Test Files  1 failed (1)\n      Tests  1 failed (1)\n' > "$SELFCHECK/red-with-summary.log"
[ "$(parseable_run "$SELFCHECK/red-with-summary.log" 1)" = "PARSABLE" ] \
  || fail "E0s: parseability predicate rejected a normal red run"
# R8-F01: a complete red summary has BOTH lines (Test Files AND Tests) and
# the FAIL-block count equals the Tests failed count; a nonzero exit whose
# summary claims zero failures is contradictory; a Tests line missing is
# incomplete → UNPARSEABLE (never PARSABLE).
printf ' FAIL  fixture.ts > a > b\nAssertionError: boom\n ❯ fixture.ts:1:1\n Test Files  1 failed (1)\n' > "$SELFCHECK/red-summary-truncated.log"
[ "$(parseable_run "$SELFCHECK/red-summary-truncated.log" 1)" = "UNPARSEABLE" ] \
  || fail "E0s: a red run with only the Test Files line is an INCOMPLETE summary (must be UNPARSEABLE)"
printf ' FAIL  fixture.ts > a > b\nAssertionError: boom\n ❯ fixture.ts:1:1\n Test Files  1 failed (1)\n      Tests  2 failed (2)\n' > "$SELFCHECK/red-summary-count-mismatch.log"
[ "$(parseable_run "$SELFCHECK/red-summary-count-mismatch.log" 1)" = "CONTRADICTORY" ] \
  || fail "E0s: FAIL blocks (1) fewer than the Tests failed count (2) — evidence loss — must be CONTRADICTORY"
# R10-F01: FILE-LEVEL evidence loss, the exact hole shape — a complete
# nonzero-exit summary counting a failed FILE with ZERO surviving FAIL
# headers (the suite-error detail blocks lost). The old predicate's only
# floor compared headers against Tests failed, which is 0 exactly for
# the suite-error shape, so this parsed PARSABLE; failed_files() then
# produced an EMPTY list, every coverage loop was vacuous, and the gate
# printed GREEN. Now the file-level floor rejects it.
printf ' Test Files  1 failed (1)\n      Tests  2 passed (2)\n' > "$SELFCHECK/red-file-level-loss.log"
[ "$(parseable_run "$SELFCHECK/red-file-level-loss.log" 1)" = "CONTRADICTORY" ] \
  || fail "E0s: a nonzero exit whose summary counts a failed FILE with zero FAIL headers — file-level evidence loss — must be CONTRADICTORY"
# R10-F01: file-level loss the case-level floor cannot see — headers (1)
# equal Tests failed (1), but the summary counts TWO failed files: one
# failed file's blocks were lost entirely.
printf ' FAIL  fixture.ts > a > b\nAssertionError: boom\n ❯ fixture.ts:1:1\n Test Files  2 failed (2)\n      Tests  1 failed (1)\n' > "$SELFCHECK/red-file-loss-case-ok.log"
[ "$(parseable_run "$SELFCHECK/red-file-loss-case-ok.log" 1)" = "CONTRADICTORY" ] \
  || fail "E0s: headers (1) >= Tests failed (1) but fewer than failed files (2) — file-level loss invisible to the case floor — must be CONTRADICTORY"
# R10-F01: summary coherence — a failed TEST with ZERO failed FILES is
# not a real producer summary (every failed test fails its file). The
# header survived, so the case floor passes (1 >= 1); only the
# coherence floor catches the Test Files line claiming a pass.
printf ' FAIL  fixture.ts > a > b\nAssertionError: boom\n ❯ fixture.ts:1:1\n Test Files  1 passed (1)\n      Tests  1 failed (1)\n' > "$SELFCHECK/red-tests-fail-files-pass.log"
[ "$(parseable_run "$SELFCHECK/red-tests-fail-files-pass.log" 1)" = "CONTRADICTORY" ] \
  || fail "E0s: Tests failed > 0 with Test Files failed == 0 — a summary shape no real producer emits — must be CONTRADICTORY"
# R11-F02: a FAIL header whose normalized identity is EMPTY (the ` FAIL `
# lead plus spaces with NO file name — damaged/truncated output; no real
# producer emits a nameless header) used to satisfy every numeric floor
# (1 header >= 1 failed file) and flow one BLANK line into failed_files
# that each downstream `[ -z "$f" ] && continue` loop skipped — the
# failure was never attributed yet the gate ended GREEN. Both the
# pure-empty shape and a MIX of a real header plus an empty identity
# must be CONTRADICTORY.
printf ' FAIL  \n Test Files  1 failed (1)\n      Tests  1 passed (1)\n' > "$SELFCHECK/red-empty-file-identity.log"
[ "$(parseable_run "$SELFCHECK/red-empty-file-identity.log" 1)" = "CONTRADICTORY" ] \
  || fail "E0s: a FAIL header whose normalized identity is EMPTY must be CONTRADICTORY (damaged/truncated evidence), never PARSABLE"
printf ' FAIL  tests/fixture.ts > a > b\nAssertionError: boom\n ❯ tests/fixture.ts:1:1\n FAIL  \n Test Files  2 failed (2)\n      Tests  1 failed (1)\n' > "$SELFCHECK/red-mixed-empty-identity.log"
[ "$(parseable_run "$SELFCHECK/red-mixed-empty-identity.log" 1)" = "CONTRADICTORY" ] \
  || fail "E0s: a log mixing a real FAIL header with an EMPTY-identity header must be CONTRADICTORY — the empty identity leaves a summary-counted failure unattributed"
# R9-F01: a failed SUITE block (header with no ` > ` — a collection
# error failing the FILE with zero failed tests counted) is a legal
# producer shape: file-level failure count 1, one no-arrow FAIL header,
# complete summary. The old header==Tests-failed equality wrongly
# rejected this as CONTRADICTORY.
printf ' FAIL  fixture.ts\nError: cannot collect fixture.ts\n ❯ fixture.ts:1:1\n Test Files  1 failed (1)\n      Tests  2 passed (2)\n' > "$SELFCHECK/red-suite-error.log"
[ "$(parseable_run "$SELFCHECK/red-suite-error.log" 1)" = "PARSABLE" ] \
  || fail "E0s: a suite-error red (no-arrow FAIL header, zero failed tests in the summary) must be PARSABLE"
# R10-F01 counts side-channel: the three-argument form records the parsed
# evidence state for the same suite-error red; the two-argument verdict
# above is unchanged (byte-identical output path).
: > "$SELFCHECK/red-counts-out.txt"
[ "$(parseable_run "$SELFCHECK/red-suite-error.log" 1 "$SELFCHECK/red-counts-out.txt")" = "PARSABLE" ] \
  || fail "E0s: the three-argument parseability call must return the same verdict for the suite-error red"
grep -q '^files_failed=1$' "$SELFCHECK/red-counts-out.txt" \
  || fail "E0s: counts side-channel must record files_failed=1 for the suite-error red"
grep -q '^distinct_files=1$' "$SELFCHECK/red-counts-out.txt" \
  || fail "E0s: counts side-channel must record distinct_files=1 for the suite-error red"
grep -q '^verdict=PARSABLE$' "$SELFCHECK/red-counts-out.txt" \
  || fail "E0s: counts side-channel must record the verdict itself"
# R9-F01: two FAIL headers for ONE failed test (a case surfacing more
# than one error block) — headers >= failed tests is legal structure.
printf ' FAIL  fixture.ts > a > b\nAssertionError: boom\n ❯ fixture.ts:1:1\n FAIL  fixture.ts > a > b\nError: Command failed: node tool.js\n ❯ fixture.ts:1:1\n Test Files  1 failed (1)\n      Tests  1 failed (1)\n' > "$SELFCHECK/red-multi-error-headers.log"
[ "$(parseable_run "$SELFCHECK/red-multi-error-headers.log" 1)" = "PARSABLE" ] \
  || fail "E0s: multiple FAIL headers for one failed test must be PARSABLE (only header LOSS is contradictory)"
printf ' FAIL  fixture.ts > a > b\nAssertionError: boom\n ❯ fixture.ts:1:1\n Test Files  1 passed (1)\n      Tests  1 passed (1)\n' > "$SELFCHECK/red-summary-all-passed.log"
[ "$(parseable_run "$SELFCHECK/red-summary-all-passed.log" 1)" = "CONTRADICTORY" ] \
  || fail "E0s: a nonzero exit whose complete summary counts zero failures must be CONTRADICTORY"
printf ' FAIL  fixture.ts > a > b\nAssertionError: boom\n ❯ fixture.ts:1:1\n' > "$SELFCHECK/contradictory-green.log"
[ "$(parseable_run "$SELFCHECK/contradictory-green.log" 0)" = "CONTRADICTORY" ] \
  || fail "E0s: parseability predicate missed FAIL blocks in an exit-0 log"
# R7-F01: an exit-0 log with NO test-completion summary (including the
# empty log) is UNPARSEABLE — never GREEN. Exit 0 without the summary is
# not evidence that the suite ran at all.
: > "$SELFCHECK/exit0-empty.log"
[ "$(parseable_run "$SELFCHECK/exit0-empty.log" 0)" = "UNPARSEABLE" ] \
  || fail "E0s: parseability predicate called an empty exit-0 log GREEN (evidence completeness)"
printf 'node_modules/…\n' > "$SELFCHECK/exit0-no-summary.log"
[ "$(parseable_run "$SELFCHECK/exit0-no-summary.log" 0)" = "UNPARSEABLE" ] \
  || fail "E0s: parseability predicate accepted exit 0 without a completion summary"
# R8-F01: exit 0 whose COMPLETE summary itself counts failures is
# CONTRADICTORY (the old predicate returned GREEN — it only counted FAIL
# headers and any `Test Files <digit>` prefix).
printf ' Test Files  1 failed (1)\n      Tests  1 failed (1)\n' > "$SELFCHECK/exit0-summary-says-failed.log"
[ "$(parseable_run "$SELFCHECK/exit0-summary-says-failed.log" 0)" = "CONTRADICTORY" ] \
  || fail "E0s: exit 0 with a summary counting 1 failure must be CONTRADICTORY, never GREEN"
# R8-F01: a bare `Test Files 1` with no verdict components is a TRUNCATED
# summary — exit 0 with it is UNPARSEABLE, never GREEN.
printf ' Test Files  1\n' > "$SELFCHECK/exit0-summary-bare.log"
[ "$(parseable_run "$SELFCHECK/exit0-summary-bare.log" 0)" = "UNPARSEABLE" ] \
  || fail "E0s: exit 0 with a truncated bare `Test Files 1` summary must be UNPARSEABLE, never GREEN"
# R8-F01: components that do not sum to the parenthesized total are not a
# complete summary either.
printf ' Test Files  2 passed (3)\n      Tests  5 passed (5)\n' > "$SELFCHECK/exit0-summary-sum-mismatch.log"
[ "$(parseable_run "$SELFCHECK/exit0-summary-sum-mismatch.log" 0)" = "UNPARSEABLE" ] \
  || fail "E0s: a summary whose components do not sum to its total must be UNPARSEABLE"
# R8-F01 positive: a green summary with skipped counts is still complete
# and passes as GREEN.
printf ' Test Files  1 passed | 1 skipped (2)\n      Tests  3 passed | 1 skipped (4)\n' > "$SELFCHECK/green-with-skipped.log"
[ "$(parseable_run "$SELFCHECK/green-with-skipped.log" 0)" = "GREEN" ] \
  || fail "E0s: a complete all-passed summary with skipped counts must be GREEN"
echo "fixtures parseability: GREEN / UNPARSEABLE(signal exit, no summary) / PARSABLE(red with complete summary; R9-F01: suite-error no-arrow header with zero failed tests, and multi-error headers >= failed count) / CONTRADICTORY(exit 0 with reds / exit 0 summary-counts-failures / nonzero all-passed summary / FAIL-block LOSS: headers < Tests-failed count / R10-F01 FILE-level LOSS: headers < Test-Files-failed count (zero-header suite-error summary is the hole shape; case floor vacuous at tests_failed=0) / R10-F01 summary incoherence: Tests failed > 0 with Test Files failed == 0 / R11-F02 EMPTY file identity: a FAIL header normalizing to an empty name — pure or mixed with real headers — is damaged evidence, CONTRADICTORY, never PARSABLE) / UNPARSEABLE(exit0 empty, exit0 no summary, exit0 bare truncated summary, exit0 sum mismatch, red missing Tests line) + R10-F01 counts side-channel records files_failed/distinct_files/verdict for the E5 cross-check OK" | tee -a "$EVIDENCE_DIR/e0s-self-checks.log"

# R7-F01 family-membership self-checks: EXACT whole-line identity, not
# substring containment. The legitimate vitest name
# `…seal.test.ts.regression.test.ts` contains a member as a substring —
# it must be NON-family. An exact member must pass; an unrelated file
# must be non-family.
MEMBERSHIP="$SELFCHECK/membership"
mkdir -p "$MEMBERSHIP"
printf '%s\n' tests/post-verification-audit-seal.test.ts > "$MEMBERSHIP/exact-only.txt"
NONFAM_EXACT="$(non_family_files "$MEMBERSHIP/exact-only.txt")"
[ -z "$NONFAM_EXACT" ] \
  || fail "E0s: exact family member rejected by exact-identity membership (got [$NONFAM_EXACT])"
printf '%s\n' tests/post-verification-audit-seal.test.ts.regression.test.ts > "$MEMBERSHIP/suffix-legit-name.txt"
NONFAM_SUFFIX="$(non_family_files "$MEMBERSHIP/suffix-legit-name.txt")"
[ "$NONFAM_SUFFIX" = "tests/post-verification-audit-seal.test.ts.regression.test.ts" ] \
  || fail "E0s: the .regression.test.ts suffix name must be NON-family under exact identity (got [$NONFAM_SUFFIX])"
printf '%s\n' tests/unrelated.test.ts > "$MEMBERSHIP/unrelated.txt"
NONFAM_OTHER="$(non_family_files "$MEMBERSHIP/unrelated.txt")"
[ "$NONFAM_OTHER" = "tests/unrelated.test.ts" ] \
  || fail "E0s: an unrelated failing file must be NON-family (got [$NONFAM_OTHER])"
printf '%s\n' tests/round2-delivery-evidence.test.ts tests/round3-delivery-evidence.test.ts tests/post-verification-audit-seal.test.ts > "$MEMBERSHIP/all-three.txt"
NONFAM_ALL="$(non_family_files "$MEMBERSHIP/all-three.txt")"
[ -z "$NONFAM_ALL" ] \
  || fail "E0s: all three exact members must pass exact identity (got [$NONFAM_ALL])"
echo "fixtures membership: exact members pass; .regression.test.ts suffix / unrelated rejected (grep -vxF whole-line identity) OK" | tee -a "$EVIDENCE_DIR/e0s-self-checks.log"

# Binding self-check: identical state must bind identically; any untracked
# content change or addition must change the binding (the R4-F02 hole was
# invisible to `--untracked-files=no`). R5 workflow: NO commit — the
# scratch repo stays at its initial unborn HEAD, every file is untracked,
# and `git status` (what the binding reads) works fine without commits.
SCRATCH="$SELFCHECK/scratch"
mkdir -p "$SCRATCH"
git -C "$SCRATCH" init -q
printf 'committed\n' > "$SCRATCH/tracked.txt"
printf 'ignored-dir/\n' > "$SCRATCH/.gitignore"
mkdir -p "$SCRATCH/ignored-dir"
printf 'dep\n' > "$SCRATCH/ignored-dir/dep.bin"
printf 'new\n' > "$SCRATCH/untracked.rs"
bind_worktree "$SCRATCH" "$SELFCHECK/bind-a.tsv"
bind_worktree "$SCRATCH" "$SELFCHECK/bind-a2.tsv"
cmp -s "$SELFCHECK/bind-a.tsv" "$SELFCHECK/bind-a2.tsv" \
  || fail "E0s: identical worktree state produced different bindings"
printf 'changed\n' > "$SCRATCH/untracked.rs"
bind_worktree "$SCRATCH" "$SELFCHECK/bind-b.tsv"
cmp -s "$SELFCHECK/bind-a.tsv" "$SELFCHECK/bind-b.tsv" \
  && fail "E0s: binding missed an untracked-file content change"
printf 'newer\n' > "$SCRATCH/another-new.rs"
bind_worktree "$SCRATCH" "$SELFCHECK/bind-c.tsv"
cmp -s "$SELFCHECK/bind-b.tsv" "$SELFCHECK/bind-c.tsv" \
  && fail "E0s: binding missed a new untracked file"
grep -q 'untracked.rs' "$SELFCHECK/bind-a.tsv" \
  || fail "E0s: binding does not list untracked files at all"
# R02 final-closeout repair-group5: the E0 self-reference exclusion must
# be SURGICAL. Rehearsed on the no-commit scratch repo with the REAL
# verify-stage prefix shape: changes under the EXCLUDED subtree are
# invisible to the binding (that is the fix — those files are this
# gate's own run-time output), a content change ANYWHERE ELSE still
# flips the binding (the exclusion must never mask candidate source),
# the active exclusion is recorded as a binding header row, and WITHOUT
# the prefix the evidence subtree stays fully bound (the default
# binding remains exhaustive).
SCRATCH_EV="artifacts/rust-tauri/R02/final-candidate-fixture/A16/legacy-entry"
mkdir -p "$SCRATCH/$SCRATCH_EV"
printf 'summary line\n' > "$SCRATCH/$SCRATCH_EV/summary.txt"
bind_worktree "$SCRATCH" "$SELFCHECK/bind-ev1.tsv" "$SCRATCH_EV"
printf 'summary line grew between the two scans\n' > "$SCRATCH/$SCRATCH_EV/summary.txt"
printf 'binding rows\n' > "$SCRATCH/$SCRATCH_EV/e0-candidate-binding.tsv"
bind_worktree "$SCRATCH" "$SELFCHECK/bind-ev2.tsv" "$SCRATCH_EV"
cmp -s "$SELFCHECK/bind-ev1.tsv" "$SELFCHECK/bind-ev2.tsv" \
  || fail "E0s: excluded-subtree changes must not change the binding (E0 self-reference exclusion broken)"
grep -q "^# binding-exclusion: $SCRATCH_EV/" "$SELFCHECK/bind-ev1.tsv" \
  || fail "E0s: the active exclusion must be recorded in the binding header row"
if awk '{print $3}' "$SELFCHECK/bind-ev1.tsv" | grep -qxF "$SCRATCH_EV/summary.txt"; then
  fail "E0s: excluded paths must not appear as binding rows"
fi
printf 'changed-again\n' > "$SCRATCH/untracked.rs"
bind_worktree "$SCRATCH" "$SELFCHECK/bind-ev3.tsv" "$SCRATCH_EV"
cmp -s "$SELFCHECK/bind-ev2.tsv" "$SELFCHECK/bind-ev3.tsv" \
  && fail "E0s: the exclusion masked a non-excluded untracked content change"
bind_worktree "$SCRATCH" "$SELFCHECK/bind-ev4.tsv"
grep -qF "$SCRATCH_EV/summary.txt" "$SELFCHECK/bind-ev4.tsv" \
  || fail "E0s: without the exclusion prefix the evidence subtree must stay fully bound (default exhaustive)"
# ── F42 (R05 RR2 WP B): run-output-root VALIDATION fixtures — the
# exclusion can never cover index-tracked content (candidate source /
# committed evidence) and never the repo root; a fresh untracked run
# directory validates. The scratch repo stays at its unborn HEAD (no
# commit): the tracked fixture paths are STAGED (index entries), which is
# exactly what validate_run_output_unit reads.
mkdir -p "$SCRATCH/run-out/fresh-dir" "$SCRATCH/rust"
printf 'out\n' > "$SCRATCH/run-out/fresh-dir/sink.log"
printf 'pub fn f() {}\n' > "$SCRATCH/rust/lib.rs"
printf 'src\n' > "$SCRATCH/rust-main.rs"
git -C "$SCRATCH" add rust/lib.rs rust-main.rs
validate_run_output_unit "$SCRATCH" "run-out/fresh-dir" \
  || fail "E0s: a fresh untracked run-output directory must validate as an exclusion root"
validate_run_output_unit "$SCRATCH" "rust" 2>/dev/null \
  && fail "E0s: a SOURCE directory (rust/, carrying tracked content) must be REJECTED as an exclusion root"
validate_run_output_unit "$SCRATCH" "rust-main.rs" 2>/dev/null \
  && fail "E0s: an index-tracked path must be REJECTED as an exclusion root"
validate_run_output_unit "$SCRATCH" "." 2>/dev/null \
  && fail "E0s: the repo root must be REJECTED as an exclusion root"
validate_run_output_unit "$SCRATCH" "/abs" 2>/dev/null \
  && fail "E0s: an absolute path must be REJECTED as an exclusion root"
validate_run_output_unit "$SCRATCH" ".." 2>/dev/null \
  && fail "E0s: a parent-relative path must be REJECTED as an exclusion root"
# ── F42: DECLARED run-output roots (R02_A16_RUN_OUTPUT_ROOTS) are an
# operator attestation but pass the SAME fences: a dedicated directory
# strictly inside artifacts/ at run-root depth. The whole evidence tree
# (artifacts / artifacts/rust-tauri), a bare stage dir, a source dir, a
# directory outside artifacts/, and a non-existent path are ALL illegal —
# `不得排除整个 artifacts` is enforced structurally, never by trust.
mkdir -p "$SCRATCH/artifacts/rust-tauri/R05/rr2-fixture-run-root"
printf 'run\n' > "$SCRATCH/artifacts/rust-tauri/R05/rr2-fixture-run-root/stdout.log"
validate_declared_run_root "$SCRATCH" "artifacts/rust-tauri/R05/rr2-fixture-run-root" \
  || fail "E0s: a dedicated fresh run root strictly inside artifacts/ must validate as a declared run-output root"
validate_declared_run_root "$SCRATCH" "rust" 2>/dev/null \
  && fail "E0s: a SOURCE directory (rust/) must be REJECTED as a declared run-output root"
validate_declared_run_root "$SCRATCH" "artifacts" 2>/dev/null \
  && fail "E0s: the whole artifacts tree must be REJECTED as a declared run-output root"
validate_declared_run_root "$SCRATCH" "artifacts/rust-tauri" 2>/dev/null \
  && fail "E0s: the whole evidence-distribution tree (artifacts/rust-tauri) must be REJECTED as a declared run-output root"
validate_declared_run_root "$SCRATCH" "artifacts/rust-tauri/R05" 2>/dev/null \
  && fail "E0s: a bare stage dir (artifacts/rust-tauri/R05) is not a dedicated run root — must be REJECTED"
validate_declared_run_root "$SCRATCH" "scripts" 2>/dev/null \
  && fail "E0s: a directory outside artifacts/ must be REJECTED as a declared run-output root"
validate_declared_run_root "$SCRATCH" "artifacts/rust-tauri/R05/does-not-exist" 2>/dev/null \
  && fail "E0s: a non-existent declared run-output root must be REJECTED"
# ── F42: MULTIPLE exclusion units — growth under ANY excluded run root
# is invisible to the binding (each root is this run's output), a
# content change OUTSIDE every unit still flips it, and each unit
# records its own # header row.
mkdir -p "$SCRATCH/run-root-a/cmd" "$SCRATCH/run-root-b/cmd"
printf 'a\n' > "$SCRATCH/run-root-a/cmd/stdout.log"
printf 'b\n' > "$SCRATCH/run-root-b/cmd/stdout.log"
bind_worktree "$SCRATCH" "$SELFCHECK/bind-mu1.tsv" "$(printf '%s\n%s' run-root-a run-root-b)"
printf 'a-grew\n' >> "$SCRATCH/run-root-a/cmd/stdout.log"
printf 'b-grew\n' >> "$SCRATCH/run-root-b/cmd/stdout.log"
bind_worktree "$SCRATCH" "$SELFCHECK/bind-mu2.tsv" "$(printf '%s\n%s' run-root-a run-root-b)"
cmp -s "$SELFCHECK/bind-mu1.tsv" "$SELFCHECK/bind-mu2.tsv" \
  || fail "E0s: multi-unit exclusion broken (growth under an excluded run root changed the binding)"
[ "$(grep -c '^# binding-exclusion: ' "$SELFCHECK/bind-mu1.tsv")" = "2" ] \
  || fail "E0s: each exclusion unit must record its own # header row (F42)"
printf 'changed-outside-units\n' > "$SCRATCH/untracked.rs"
bind_worktree "$SCRATCH" "$SELFCHECK/bind-mu3.tsv" "$(printf '%s\n%s' run-root-a run-root-b)"
cmp -s "$SELFCHECK/bind-mu2.tsv" "$SELFCHECK/bind-mu3.tsv" \
  && fail "E0s: the multi-unit exclusion masked a non-excluded untracked content change"
# ── F42: FILE units (the demoted minimal attribution) — exactly the
# sink file is excluded: growth of the sink itself is invisible, growth
# of its SIBLING in the same directory still flips the binding. The
# dedication fence demotes a sink in a non-dedicated dir to exactly this
# shape, so the sibling old-evidence files around it stay fully bound.
printf 'c\n' > "$SCRATCH/run-root-b/cmd/stderr.log"
bind_worktree "$SCRATCH" "$SELFCHECK/bind-fu1.tsv" "run-root-b/cmd/stdout.log"
printf 'c-grew\n' >> "$SCRATCH/run-root-b/cmd/stdout.log"
bind_worktree "$SCRATCH" "$SELFCHECK/bind-fu2.tsv" "run-root-b/cmd/stdout.log"
cmp -s "$SELFCHECK/bind-fu1.tsv" "$SELFCHECK/bind-fu2.tsv" \
  || fail "E0s: a FILE unit must hide its own sink's growth"
printf 'c-grew-again\n' >> "$SCRATCH/run-root-b/cmd/stderr.log"
bind_worktree "$SCRATCH" "$SELFCHECK/bind-fu3.tsv" "run-root-b/cmd/stdout.log"
cmp -s "$SELFCHECK/bind-fu2.tsv" "$SELFCHECK/bind-fu3.tsv" \
  && fail "E0s: a FILE unit masked a SIBLING file's content change (minimal attribution broken)"
# The baseline-CONSTRUCTION proof is the E0 purity assert on the real
# freshly-built base copy (HEAD == BASE_SHA, empty diff, empty status):
# the R4 scratch-repo rehearsal used `checkout -f` + `clean -fd` and is
# gone with them — the real construction contains no such commands.
{
  echo "binding: identical-state match OK; untracked content change detected; new untracked file detected (no-commit scratch repo)"
  echo "binding exclusion (repair-group5): excluded-subtree changes invisible; non-excluded content change still detected; header row recorded; default binding stays exhaustive without the prefix"
  echo "binding exclusion (F42): run-output-root validation OK — fresh untracked dir accepted; tracked source dir (rust/), tracked path, repo root, absolute and parent-relative paths all REJECTED; declared roots fenced — dedicated artifacts/-inside run root accepted, the whole evidence tree (artifacts / artifacts/rust-tauri), a bare stage dir, rust/, outside-artifacts dirs and non-existent paths all REJECTED; multi-unit exclusion — growth under any excluded run root invisible, non-excluded content change still detected, one # header row per unit; FILE units — own sink growth invisible, sibling content change still detected (minimal attribution)"
  echo "baseline construction: proven by the E0 purity asserts on the real fresh copy (no checkout -f / clean -fd anywhere)"
} | tee -a "$EVIDENCE_DIR/e0s-self-checks.log"
note "PASS E0s-gate-self-checks (62 classifier fixtures incl. every REAL guard failure sentence, the zero-payload shapes, the R7 same-line/same-block mixes, the real direct-guard shape, the R8 structural-mixing negatives (compact Error:EACCES token, list/warning/matcher/wrapper tails, foreign/misplaced wrapper) and the producer-binding negatives (body lines without their lead) plus the REAL matcher-summary/diff-body positives, the R9-F01 firstDiff structure fixtures (legal bracket/comma/escape/non-ASCII paths pure, non-JSON/mixed/truncated/empty/4-element/trailing-junk payloads fail closed), the repair-group10 guard-tail-window fixtures (the two gate-r2 round2/round3 REAL 500-point windows + the derived complete-end variant classify seal+uncommitted; single-point mutations — off-by-one char, trailing junk, sentence-material fragment, wrong producer wrapper, deleted listing line — all fail closed to UNRECOGNIZED), the RR2 patch-too-large form-evolution fixtures (the two REAL s5-full-4 shapes — round2's complete 6-row RuntimeError traceback and round3's bare SystemExit line — classify seal; truncated sentence, other git error text, foreign SystemExit message, cross-producer both ways, mutated frame and node-guard producer all fail closed to UNRECOGNIZED) + no-block-rows rejection + green positive control + parseability predicate incl. exit-0 summary completeness and summary/exit consistency with the R9-F01 one-directional header-loss rule (suite-error no-arrow header and multi-error headers are PARSABLE; headers < Tests-failed is CONTRADICTORY) and the R10-F01 floors (headers < Test-Files-failed count — the zero-header suite-error summary hole — and Tests-failed>0 with Test-Files-failed==0 incoherence — both CONTRADICTORY; counts side-channel records files_failed/distinct_files/verdict) + exact-identity family membership checks + no-commit binding checks + F42 run-output-root validation (fresh artifacts/ dir accepted; tracked source dir / tracked path / repo root / absolute / parent-relative rejected), declared-root fencing (dedicated artifacts/-inside run root accepted; the whole evidence tree, a bare stage dir, rust/, outside-artifacts and non-existent rejected) and multi-unit/FILE-unit exclusion fixtures (growth under any excluded run root invisible, non-excluded and sibling changes still detected, one # header row per unit); details in e0s-self-checks.log)"

# ── E1: default-entry-not-switched proofs (inside the candidate copy, so
#        uncommitted candidate state is included) ────────────────────────────
# R02 final closeout (repair-group2): A16's correct semantics are "the
# incumbent Node/Electron PRODUCTION DEFAULT is not switched and no new
# client-affecting regressions appeared" — NOT "zero diff over the
# desktop/tests/package.json surface" and NOT "zero rust references in
# the launch path". Authorized opt-in wiring (explicit runtime selection,
# packaged preview resources, guarded branches) legitimately touches that
# surface; the assertions below pin the DEFAULT (Node/Electron) while the
# authorized wiring range is ARCHIVED as evidence. Evidence basis:
# /tmp/r02-final/a16-audit-r1.md (A16-AUDITOR-R1).
note "== E1: default entry not switched (Node/Electron incumbent default) =="

# E1a (RECORD ONLY — authorized-wiring audit trail): the surface diff vs
# BASE, computed and archived, never gated. The pre-closeout zero-diff
# FAIL semantics was the wrong predicate — it went permanently red the
# moment the authorized client wiring landed, regardless of the default
# runtime. This list documents WHAT was wired for the audit; the gate
# itself is E1c + E1d below.
git -C "$CAND_COPY" diff --name-only "$BASE_SHA" -- \
  core/ server/ desktop/ shared/ tests/ package.json package-lock.json \
  > "$EVIDENCE_DIR/e1-surface-diff-files.txt"
SURFACE_DIFF_COUNT="$(grep -c . "$EVIDENCE_DIR/e1-surface-diff-files.txt" || true)"
if [ "$SURFACE_DIFF_COUNT" = "0" ]; then
  echo "(empty — no audited-surface file differs from base $BASE_SHA)" >> "$EVIDENCE_DIR/e1-surface-diff-files.txt"
fi
note "RECORD E1a-surface-diff (authorized wiring range vs base $BASE_SHA: $SURFACE_DIFF_COUNT file(s) — archived in e1-surface-diff-files.txt, not gated)"

# E1b (RECORD ONLY): the uncommitted/untracked part of the wiring range —
# git diff cannot see untracked files, so intersect the full candidate
# binding (R4-F02: every tracked+untracked path) with the same surface
# and archive it. The prefix branch also carries the package.json /
# package-lock.json name forms so the record is complete. The binding's
# `#` header row (repair-group5 exclusion record) is data-free and is
# skipped before the field extraction.
grep -v '^#' "$EVIDENCE_DIR/e0-candidate-binding.tsv" | awk '{print $3}' \
  | grep -E '^(core/|server/|desktop/|shared/|tests/|package\.json|package-lock\.json)' \
  > "$EVIDENCE_DIR/e1-binding-surface-intersect.txt" || true
BINDING_HIT_COUNT="$(grep -c . "$EVIDENCE_DIR/e1-binding-surface-intersect.txt" || true)"
if [ "$BINDING_HIT_COUNT" = "0" ]; then
  echo "(empty — the candidate binding has no tracked/untracked path inside the audited surface)" >> "$EVIDENCE_DIR/e1-binding-surface-intersect.txt"
fi
note "RECORD E1b-binding-surface-intersect (uncommitted/untracked part of the wiring range: $BINDING_HIT_COUNT path(s) — archived in e1-binding-surface-intersect.txt, not gated)"

# E1c: the Electron entry point is still the incumbent bootstrap.
MAIN_FIELD="$(cd "$CAND_COPY" && node -p "require('./package.json').main")"
[ "$MAIN_FIELD" = "desktop/bootstrap.cjs" ] || fail "E1c: package.json main is '$MAIN_FIELD', expected desktop/bootstrap.cjs"
note "PASS E1c-package-main (package.json main = desktop/bootstrap.cjs)"

# E1d: the default-entry-not-switched assertion set. Every assertion runs
# INSIDE the candidate copy; ANY violation fails E1. Evidence per
# assertion lands in e1-default-entry-assertions.txt. Grep-based
# assertions capture stderr too and require grep's exact exit status, so
# a missing file (exit 2) fails CLOSED instead of passing vacuously.
E1D_LOG="$EVIDENCE_DIR/e1-default-entry-assertions.txt"
: > "$E1D_LOG"

# d-1 runtime selector default: unset → node, 'node' → node, 'rust' → rust.
set +e
(cd "$CAND_COPY" && node -e "const m=require('./desktop/src/shared/rust-local-service.cjs'); if(m.rustDesktopEnabled({})!==false||m.rustDesktopEnabled({LINGXI_DESKTOP_SERVER_RUNTIME:'node'})!==false||m.rustDesktopEnabled({LINGXI_DESKTOP_SERVER_RUNTIME:'rust'})!==true)process.exit(1); console.log('d-1 rustDesktopEnabled: {} -> false (node), node -> false, rust -> true')") >> "$E1D_LOG" 2>&1
D1_STATUS=$?
set -e
if [ "$D1_STATUS" -ne 0 ]; then
  tail -5 "$E1D_LOG" >&2
  fail "E1d-1: desktop runtime selector no longer defaults to node (rustDesktopEnabled behavior changed or the module is unreadable — see e1-default-entry-assertions.txt)"
fi
note "PASS E1d-1-runtime-default (rustDesktopEnabled: unset/node → false, explicit rust → true)"

# d-2 no default injection: nothing in the launch/packaging chain may set
# LINGXI_DESKTOP_SERVER_RUNTIME for the user (package.json scripts AND
# build/electron-builder sections, launch.js, bootstrap.cjs, notarize.cjs).
set +e
INJECTION_HITS="$(cd "$CAND_COPY" && grep -n 'LINGXI_DESKTOP_SERVER_RUNTIME' package.json scripts/launch.js desktop/bootstrap.cjs scripts/notarize.cjs 2>&1)"
INJECTION_STATUS=$?
set -e
{ echo "--- d-2 injection grep (exit $INJECTION_STATUS) ---"; printf '%s\n' "$INJECTION_HITS"; } >> "$E1D_LOG"
if [ "$INJECTION_STATUS" -ne 1 ] || [ -n "$INJECTION_HITS" ]; then
  printf '%s\n' "$INJECTION_HITS" >&2
  fail "E1d-2: the launch/packaging chain injects LINGXI_DESKTOP_SERVER_RUNTIME (package.json / scripts/launch.js / desktop/bootstrap.cjs / scripts/notarize.cjs) or the grep could not run — see e1-default-entry-assertions.txt"
fi
note "PASS E1d-2-no-default-injection (LINGXI_DESKTOP_SERVER_RUNTIME absent from package.json, scripts/launch.js, desktop/bootstrap.cjs, scripts/notarize.cjs)"

# d-3 branch guard + incumbent Node body: main.cjs guards the Rust branch
# with rustDesktopEnabled() and keeps the incumbent Node server body
# (server-info.json reuse/cleanup/spawn) as the default path.
set +e
GUARD_HITS="$(cd "$CAND_COPY" && grep -nF 'if (rustDesktopEnabled())' desktop/main.cjs 2>&1)"
GUARD_STATUS=$?
NODE_BODY_HITS="$(cd "$CAND_COPY" && grep -nF 'server-info.json' desktop/main.cjs 2>&1)"
NODE_BODY_STATUS=$?
set -e
{ echo "--- d-3 guard grep (exit $GUARD_STATUS) ---"; printf '%s\n' "$GUARD_HITS"; echo "--- d-3 node-body grep (exit $NODE_BODY_STATUS) ---"; printf '%s\n' "$NODE_BODY_HITS"; } >> "$E1D_LOG"
if [ "$GUARD_STATUS" -ne 0 ] || [ -z "$GUARD_HITS" ] || [ "$NODE_BODY_STATUS" -ne 0 ] || [ -z "$NODE_BODY_HITS" ]; then
  { printf '%s\n' "$GUARD_HITS"; printf '%s\n' "$NODE_BODY_HITS"; } >&2
  fail "E1d-3: desktop main.cjs lost the rustDesktopEnabled() startServer guard and/or the incumbent Node server body (server-info.json handling) — see e1-default-entry-assertions.txt"
fi
note "PASS E1d-3-branch-guard (main.cjs: 'if (rustDesktopEnabled())' guards the Rust branch; incumbent Node server body with server-info.json handling intact)"

# d-4 bootstrap load chain: bootstrap still selects main(.bundle).cjs by
# isPackaged and requires it — the incumbent entry chain
# package.json main → bootstrap.cjs → main(.bundle).cjs.
set +e
BOOTSTRAP_SELECT="$(cd "$CAND_COPY" && grep -nE 'app\.isPackaged.*main\.bundle\.cjs.*main\.cjs' desktop/bootstrap.cjs 2>&1)"
BOOTSTRAP_SELECT_STATUS=$?
BOOTSTRAP_REQUIRE="$(cd "$CAND_COPY" && grep -nF 'require(app.isPackaged' desktop/bootstrap.cjs 2>&1)"
BOOTSTRAP_REQUIRE_STATUS=$?
set -e
{ echo "--- d-4 bootstrap select grep (exit $BOOTSTRAP_SELECT_STATUS) ---"; printf '%s\n' "$BOOTSTRAP_SELECT"; echo "--- d-4 bootstrap require grep (exit $BOOTSTRAP_REQUIRE_STATUS) ---"; printf '%s\n' "$BOOTSTRAP_REQUIRE"; } >> "$E1D_LOG"
if [ "$BOOTSTRAP_SELECT_STATUS" -ne 0 ] || [ -z "$BOOTSTRAP_SELECT" ] || [ "$BOOTSTRAP_REQUIRE_STATUS" -ne 0 ] || [ -z "$BOOTSTRAP_REQUIRE" ]; then
  { printf '%s\n' "$BOOTSTRAP_SELECT"; printf '%s\n' "$BOOTSTRAP_REQUIRE"; } >&2
  fail "E1d-4: desktop/bootstrap.cjs no longer selects/requires main(.bundle).cjs by app.isPackaged — see e1-default-entry-assertions.txt"
fi
note "PASS E1d-4-bootstrap-load-chain (bootstrap.cjs: app.isPackaged selects and requires main.bundle.cjs / main.cjs)"

# d-5 CLI default runtime stays node.
set +e
CLI_DEFAULT="$(cd "$CAND_COPY" && grep -nF 'runtime: "node"' cli/args.ts 2>&1)"
CLI_DEFAULT_STATUS=$?
set -e
{ echo "--- d-5 cli default grep (exit $CLI_DEFAULT_STATUS) ---"; printf '%s\n' "$CLI_DEFAULT"; } >> "$E1D_LOG"
if [ "$CLI_DEFAULT_STATUS" -ne 0 ] || [ -z "$CLI_DEFAULT" ]; then
  printf '%s\n' "$CLI_DEFAULT" >&2
  fail "E1d-5: cli/args.ts no longer defaults runtime to \"node\" — see e1-default-entry-assertions.txt"
fi
note "PASS E1d-5-cli-default-node (cli/args.ts default runtime = \"node\")"

# d-6 data-root no-double-write guards on both sides: the desktop Rust
# start refuses a data home owned by a Node server, and the Rust service
# keeps its runtime records inside the isolated {home}/lingxi-service/
# layout (never {home}/server-info.json).
set +e
DESKTOP_MUTEX="$(cd "$CAND_COPY" && grep -nF 'RUST_DESKTOP_NODE_SERVER_INFO_PRESENT' desktop/main.cjs 2>&1)"
DESKTOP_MUTEX_STATUS=$?
RUST_LAYOUT="$(cd "$CAND_COPY" && grep -nF 'const RUNTIME_DIR_NAME: &str = "lingxi-service"' rust/crates/lingxi-service/src/paths.rs 2>&1)"
RUST_LAYOUT_STATUS=$?
set -e
{ echo "--- d-6 desktop mutex grep (exit $DESKTOP_MUTEX_STATUS) ---"; printf '%s\n' "$DESKTOP_MUTEX"; echo "--- d-6 rust layout grep (exit $RUST_LAYOUT_STATUS) ---"; printf '%s\n' "$RUST_LAYOUT"; } >> "$E1D_LOG"
if [ "$DESKTOP_MUTEX_STATUS" -ne 0 ] || [ -z "$DESKTOP_MUTEX" ] || [ "$RUST_LAYOUT_STATUS" -ne 0 ] || [ -z "$RUST_LAYOUT" ]; then
  { printf '%s\n' "$DESKTOP_MUTEX"; printf '%s\n' "$RUST_LAYOUT"; } >&2
  fail "E1d-6: data-root no-double-write guards missing (RUST_DESKTOP_NODE_SERVER_INFO_PRESENT in desktop/main.cjs and/or RUNTIME_DIR_NAME in rust/crates/lingxi-service/src/paths.rs) — see e1-default-entry-assertions.txt"
fi
note "PASS E1d-6-no-double-write-guards (main.cjs RUST_DESKTOP_NODE_SERVER_INFO_PRESENT mutex + rust paths.rs RUNTIME_DIR_NAME runtime-dir layout both present)"

# ── E2/E3: typechecks (candidate copy) ─────────────────────────────────────
note "== E2: npm run typecheck (candidate copy) =="
(cd "$CAND_COPY" && npm run typecheck) > "$EVIDENCE_DIR/e2-typecheck.log" 2>&1 || { tail -20 "$EVIDENCE_DIR/e2-typecheck.log" >&2; fail "E2: typecheck failed"; }
note "PASS E2-typecheck (exit 0; tsc x3)"

note "== E3: npm run typecheck:core-contracts (candidate copy) =="
(cd "$CAND_COPY" && npm run typecheck:core-contracts) > "$EVIDENCE_DIR/e3-core-contracts.log" 2>&1 || { tail -20 "$EVIDENCE_DIR/e3-core-contracts.log" >&2; fail "E3: core-contracts typecheck failed"; }
note "PASS E3-core-contracts (exit 0)"

# ── E4: incumbent boundary gates (candidate copy) ──────────────────────────
note "== E4: incumbent boundary gates (candidate copy) =="
(cd "$CAND_COPY" && npm run check:dependency-boundaries) > "$EVIDENCE_DIR/e4-dependency-boundaries.log" 2>&1 || { tail -20 "$EVIDENCE_DIR/e4-dependency-boundaries.log" >&2; fail "E4: dependency-boundaries failed"; }
(cd "$CAND_COPY" && npm run check:tool-invocation-boundaries) > "$EVIDENCE_DIR/e4-tool-invocation-boundaries.log" 2>&1 || { tail -20 "$EVIDENCE_DIR/e4-tool-invocation-boundaries.log" >&2; fail "E4: tool-invocation-boundaries failed"; }
note "PASS E4-boundary-gates (dependency + tool-invocation, exit 0 each)"

# ── E4.5: renderer build (candidate copy) ──────────────────────────────────
# R02 final closeout: the authorized client wiring touches renderer sources
# (server-connection / websocket / status surfaces), so the renderer build
# is part of the no-new-regressions surface; the pre-closeout gate never
# built it. Runs INSIDE the candidate copy (its outputs land there, never
# in the invoking worktree).
note "== E4.5: npm run build:renderer (candidate copy) =="
(cd "$CAND_COPY" && npm run build:renderer) > "$EVIDENCE_DIR/e4-5-build-renderer.log" 2>&1 \
  || { tail -20 "$EVIDENCE_DIR/e4-5-build-renderer.log" >&2; fail "E4.5: build:renderer failed"; }
note "PASS E4.5-build-renderer (exit 0; vite build --config vite.config.ts)"

# ── E5: failure attribution by replay ──────────────────────────────────────
# R03-T08 scope split (dispatch: "npm 侧按 R02 惯例跑受影响定向（完整 npm
# 与审计封印状态由总控另记，不在本 Task 伪造）"): the DEFAULT mode stays
# the FULL chain (E0–E5, exactly what the R02 stage map registers). The
# directed mode runs E0–E4.5 (entry/binding/typecheck/contracts/boundary
# gates/renderer build) and STOPS BEFORE the full-npm + seal-family
# classification, recording the skip loudly — it never reclassifies,
# never weakens, and never turns an E5 red green.
LEGACY_REGRESSION_MODE="${R02_LEGACY_REGRESSION_MODE:-full}"
case "$LEGACY_REGRESSION_MODE" in
  full) : ;;
  directed-no-seal-family)
    note "SKIP E5 (full npm + seal-family classification) BY SCOPE: directed mode requested; E0–E4.5 all green above; the full chain remains the R02 stage map's a16 command and the controller's ledger item"
    note "RESULT: R02 legacy entry regression DIRECTED (E0–E4.5) ALL GREEN"
    exit 0
    ;;
  *)
    echo "FAIL: unknown R02_LEGACY_REGRESSION_MODE "$LEGACY_REGRESSION_MODE" (expected full|directed-no-seal-family)" >&2
    exit 1
    ;;
esac
note "== E5a: full npm test (candidate copy) =="
set +e
(cd "$CAND_COPY" && npm test) > "$EVIDENCE_DIR/e5-candidate-npm-test.log" 2>&1
CAND_EXIT=$?
set -e
note "candidate npm test raw exit code: $CAND_EXIT (archived in e5-candidate-npm-test.log; a non-zero exit with only registered-class reds is a classified governance state, never formal green)"
grep -E "Test Files +[0-9]+" "$EVIDENCE_DIR/e5-candidate-npm-test.log" | tail -n 1 \
  > "$EVIDENCE_DIR/e5-candidate-vitest-summary.txt" || true

# R5-F01 parseability gate: the exit code and the log content must agree
# before ANY classification runs. A non-zero exit whose log shows neither
# a COMPLETE vitest summary nor any parsable FAIL block (crash, signal,
# OOM, output loss) is UNPARSEABLE — fail closed; an exit 0 that still
# shows FAIL blocks, or whose summary counts failures, is CONTRADICTORY —
# also fail closed; a red whose FAIL-block count disagrees with the Tests
# failed count is likewise contradictory (R8-F01: the summary must be
# complete and consistent before anything downstream classifies).
# Nothing downstream may turn an unparseable run into classes or into an
# implicit green.
CAND_PARSE="$(parseable_run "$EVIDENCE_DIR/e5-candidate-npm-test.log" "$CAND_EXIT" "$EVIDENCE_DIR/e5-candidate-summary-counts.txt")"
case "$CAND_PARSE" in
  GREEN) note "candidate npm test parseable state: GREEN (exit 0, zero FAIL blocks — the legal no-failure state)";;
  PARSABLE) :;;
  UNPARSEABLE) fail "E5: candidate npm test exited $CAND_EXIT but the log has no vitest summary and no parsable FAIL block — unparseable failure, failing closed (see e5-candidate-npm-test.log)";;
  CONTRADICTORY) fail "E5: candidate npm test exited 0 yet the log contains FAIL blocks — contradictory output, failing closed";;
esac

failed_files "$EVIDENCE_DIR/e5-candidate-npm-test.log" > "$EVIDENCE_DIR/e5-candidate-failed-files.txt"
# R11-F02: no BLANK identity may enter the downstream loops. A blank
# line here is a FAIL header whose normalized identity is EMPTY
# (damaged/truncated — parseable_run should already have rejected the
# log as CONTRADICTORY); if one still reaches this file, the two
# extractions disagree and every downstream loop would silently skip
# the unattributed failure — fail closed instead.
if grep -q '^$' "$EVIDENCE_DIR/e5-candidate-failed-files.txt"; then
  fail "E5: candidate failed-files list contains an EMPTY file identity — damaged/truncated FAIL header survived parseable_run (parser drift), failing closed"
fi
extract_blocks "$EVIDENCE_DIR/e5-candidate-npm-test.log" "$CAND_COPY" > "$EVIDENCE_DIR/e5-candidate-blocks.txt"
note "candidate failed files: $(tr '\n' ' ' < "$EVIDENCE_DIR/e5-candidate-failed-files.txt" || true)"

# R10-F01: file-level coverage vs the summary, INDEPENDENT of the floors
# inside parseable_run (which see only RAW header counts). The per-file
# classification/coverage loops below iterate ONLY the files the FAIL
# headers name — a file the summary counts failed but whose headers were
# lost would be silently unexplained (empty failed-files list, vacuous
# loops, verdict printed anyway). Two guards: (a) the shell-side
# distinct-file count must equal the awk-side header normalization (any
# drift between the two normalizations is parser drift — fail closed);
# (b) the distinct FAIL-header files must cover the summary's own
# files_failed count (distinct >= files_failed is one-directional: a
# real complete log names EVERY failed file in a header, while extra
# distinct files can only mean headers for files the summary does not
# count failed — evidence that fails classification on its own, never a
# bypass). Fewer distinct header files than the summary counts failed is
# file-level evidence loss.
CAND_HEADER_FILES="$(wc -l < "$EVIDENCE_DIR/e5-candidate-failed-files.txt" | tr -d ' ')"
CAND_AWK_DISTINCT="$(sed -n 's/^distinct_files=//p' "$EVIDENCE_DIR/e5-candidate-summary-counts.txt")"
CAND_SUM_FILES_FAILED="$(sed -n 's/^files_failed=//p' "$EVIDENCE_DIR/e5-candidate-summary-counts.txt")"
[ "$CAND_HEADER_FILES" = "$CAND_AWK_DISTINCT" ] \
  || fail "E5: candidate failed-file extraction ($CAND_HEADER_FILES) disagrees with parseable_run's header normalization ($CAND_AWK_DISTINCT) — parser drift, failing closed"
if [ "$CAND_HEADER_FILES" -lt "$CAND_SUM_FILES_FAILED" ]; then
  fail "E5: candidate vitest summary counts $CAND_SUM_FILES_FAILED failed test file(s) but only $CAND_HEADER_FILES distinct FAIL-header file(s) survived in the log — file-level evidence loss, failing closed (see e5-candidate-summary-counts.txt)"
fi
note "PASS E5-file-level-coverage (candidate: $CAND_HEADER_FILES distinct FAIL-header file(s) >= summary files_failed=$CAND_SUM_FILES_FAILED — no file-level evidence loss)"

note "== E5b: seal-family replay at pristine base $BASE_SHA (base copy) =="
set +e
(cd "$BASE_COPY" && npm test -- \
  tests/post-verification-audit-seal.test.ts \
  tests/round2-delivery-evidence.test.ts \
  tests/round3-delivery-evidence.test.ts) > "$EVIDENCE_DIR/e5-base-seal-family.log" 2>&1
BASE_EXIT=$?
set -e
note "base seal-family npm test raw exit code: $BASE_EXIT (archived in e5-base-seal-family.log)"

# Same parseability gate on the base replay (a green base family run is
# the expected pristine-baseline outcome and stays legal).
BASE_PARSE="$(parseable_run "$EVIDENCE_DIR/e5-base-seal-family.log" "$BASE_EXIT" "$EVIDENCE_DIR/e5-base-summary-counts.txt")"
case "$BASE_PARSE" in
  UNPARSEABLE) fail "E5: base seal-family replay exited $BASE_EXIT but the log has no vitest summary and no parsable FAIL block — unparseable, failing closed";;
  CONTRADICTORY) fail "E5: base seal-family replay exited 0 yet the log contains FAIL blocks — contradictory output, failing closed";;
esac

failed_files "$EVIDENCE_DIR/e5-base-seal-family.log" > "$EVIDENCE_DIR/e5-base-failed-files.txt"
# R11-F02: the same empty-identity guard on the base replay (the base's
# downstream loops are equally vacuous against a blank identity).
if grep -q '^$' "$EVIDENCE_DIR/e5-base-failed-files.txt"; then
  fail "E5: base failed-files list contains an EMPTY file identity — damaged/truncated FAIL header survived parseable_run (parser drift), failing closed"
fi
extract_blocks "$EVIDENCE_DIR/e5-base-seal-family.log" "$BASE_COPY" > "$EVIDENCE_DIR/e5-base-blocks.txt"
note "base failed files (family replay): $(tr '\n' ' ' < "$EVIDENCE_DIR/e5-base-failed-files.txt" || true)"

# R10-F01: the same two file-level guards on the BASE replay — the base's
# classification/coverage loops are equally vacuous against a
# summary-failed file whose headers were lost.
BASE_HEADER_FILES="$(wc -l < "$EVIDENCE_DIR/e5-base-failed-files.txt" | tr -d ' ')"
BASE_AWK_DISTINCT="$(sed -n 's/^distinct_files=//p' "$EVIDENCE_DIR/e5-base-summary-counts.txt")"
BASE_SUM_FILES_FAILED="$(sed -n 's/^files_failed=//p' "$EVIDENCE_DIR/e5-base-summary-counts.txt")"
[ "$BASE_HEADER_FILES" = "$BASE_AWK_DISTINCT" ] \
  || fail "E5: base failed-file extraction ($BASE_HEADER_FILES) disagrees with parseable_run's header normalization ($BASE_AWK_DISTINCT) — parser drift, failing closed"
if [ "$BASE_HEADER_FILES" -lt "$BASE_SUM_FILES_FAILED" ]; then
  fail "E5: base vitest summary counts $BASE_SUM_FILES_FAILED failed test file(s) but only $BASE_HEADER_FILES distinct FAIL-header file(s) survived in the log — file-level evidence loss, failing closed (see e5-base-summary-counts.txt)"
fi
note "PASS E5-file-level-coverage-base (base: $BASE_HEADER_FILES distinct FAIL-header file(s) >= summary files_failed=$BASE_SUM_FILES_FAILED — no file-level evidence loss)"

# (1) Failing files that are NEW at the candidate (green at base, red
# now) are NOT automatic failures: inside the seal family that is exactly
# the documented lag mechanism — the stage's authorized commits post-date
# the seal coordinate, so the guard goes red until the orchestrator
# advances the seal. They are recorded loudly here, and their causes must
# still classify into documented classes (check 3) and stay inside the
# family (check 2) — those two checks are the anti-regression teeth.
NEW_REDS="$(comm -23 "$EVIDENCE_DIR/e5-candidate-failed-files.txt" "$EVIDENCE_DIR/e5-base-failed-files.txt")"
if [ -n "$NEW_REDS" ]; then
  printf '%s\n' "$NEW_REDS" > "$EVIDENCE_DIR/e5-new-since-base.txt"
  note "NOTE new-since-base (green at base, red at candidate): $(printf '%s' "$NEW_REDS" | tr '\n' ' ' ) — must be inside baseline-reds ∪ registered pre-existing families with registered-class causes (checked below)"
fi

# (2) No NEW failing files vs the baseline (R02 final closeout semantics):
# a candidate red passes only when its COMPLETE path is EXACTLY (R7-F01
# whole-line identity — a path that merely CONTAINS a member, the
# `.regression.test.ts` suffix shape, is outside) EITHER a red of the
# pristine BASE replay OR a member of a REGISTERED pre-existing failure
# family. The registration list below is an explicit ledger of DOCUMENTED
# pre-existing reds — registering a family is an audit decision that
# carries its own evidence, NEVER a way to absorb a new red: any candidate
# red outside base-reds ∪ registered families still fails the gate right
# here. Current registration = the seal coordinate-lag trio: the frozen
# VERIFIED_SOURCE_SHA predates the authorized R01/R02 commits, so the
# committed-but-newer deliverables legitimately read as "non-audit
# changes" — a seal-workflow coordinate lag governed by PROGRESS.md's
# seal process (the orchestrator's step, out of scope for this gate),
# not an R02 behavior regression.
REGISTERED_PREEXISTING_FAMILY="$SEAL_FAMILY"
printf '%s\n' "$REGISTERED_PREEXISTING_FAMILY" > "$EVIDENCE_DIR/e5-registered-preexisting-family.txt"
sort -u "$EVIDENCE_DIR/e5-base-failed-files.txt" "$EVIDENCE_DIR/e5-registered-preexisting-family.txt" \
  > "$EVIDENCE_DIR/e5-allowed-reds.txt"
NEW_OUTSIDE="$(comm -23 "$EVIDENCE_DIR/e5-candidate-failed-files.txt" "$EVIDENCE_DIR/e5-allowed-reds.txt")"
if [ -n "$NEW_OUTSIDE" ]; then
  printf '%s\n' "$NEW_OUTSIDE" | tee "$EVIDENCE_DIR/e5-non-family-failures.txt" >&2
  fail "E5: candidate failures OUTSIDE baseline-replay reds ∪ registered pre-existing families (exact whole-line identity; see e5-non-family-failures.txt vs e5-allowed-reds.txt)"
fi
BASE_NON_FAMILY="$(non_family_files "$EVIDENCE_DIR/e5-base-failed-files.txt")"
if [ -n "$BASE_NON_FAMILY" ]; then
  printf '%s\n' "$BASE_NON_FAMILY" | tee "$EVIDENCE_DIR/e5-base-non-family-failures.txt" >&2
  fail "E5: BASE replay shows failures outside the seal family — the family's base state is broken, attribution impossible"
fi
note "PASS E5-no-new-reds (every candidate red ∈ baseline-replay reds ∪ registered pre-existing families — exact whole-line identity, substring look-alikes rejected; registration ledger archived in e5-registered-preexisting-family.txt (currently the seal trio: coordinate lag per the seal workflow, not an R02 regression); allowed set in e5-allowed-reds.txt; base replay reds all inside the registered family)"

# (3) Content-based cause classification at BOTH ends. Every failing file
# gets its classes from the actual generator/guard diagnostics. Class
# rules:
#   candidate: seal-coordinate-lag allowed; uncommitted-source-rejection
#              allowed ONLY while the candidate worktree is dirty (E0
#              recorded it); UNRECOGNIZED never allowed.
#   base:      seal-coordinate-lag allowed (a governance state, currently
#              green); uncommitted-source-rejection proves the baseline was
#              polluted — impossible after the E0 purity assert, so its
#              appearance here means the baseline construction is broken:
#              FAIL CLOSED; UNRECOGNIZED never allowed.
: > "$EVIDENCE_DIR/e5-candidate-causes.txt"
: > "$EVIDENCE_DIR/e5-candidate-cause-classes.txt"
while IFS= read -r f; do
  # R11-F02: an empty identity is NEVER skipped here any more — skipping
  # left the failure unattributed while the gate carried on.
  [ -n "$f" ] || fail "E5: empty failing-file identity in the candidate failed-files list — damaged/truncated FAIL header, failing closed"
  classes="$(classify_file "$EVIDENCE_DIR/e5-candidate-blocks.txt" "$f" | sort | tr '\n' ',')"
  classes="${classes%,}"
  printf '%s\t%s\n' "$f" "$classes" >> "$EVIDENCE_DIR/e5-candidate-causes.txt"
  printf '%s\n' "$classes" | tr ',' '\n' >> "$EVIDENCE_DIR/e5-candidate-cause-classes.txt"
done < "$EVIDENCE_DIR/e5-candidate-failed-files.txt"
sort -u -o "$EVIDENCE_DIR/e5-candidate-cause-classes.txt" "$EVIDENCE_DIR/e5-candidate-cause-classes.txt" 2>/dev/null || true

: > "$EVIDENCE_DIR/e5-base-causes.txt"
: > "$EVIDENCE_DIR/e5-base-cause-classes.txt"
while IFS= read -r f; do
  [ -n "$f" ] || fail "E5: empty failing-file identity in the base failed-files list — damaged/truncated FAIL header, failing closed"
  classes="$(classify_file "$EVIDENCE_DIR/e5-base-blocks.txt" "$f" | sort | tr '\n' ',')"
  classes="${classes%,}"
  printf '%s\t%s\n' "$f" "$classes" >> "$EVIDENCE_DIR/e5-base-causes.txt"
  printf '%s\n' "$classes" | tr ',' '\n' >> "$EVIDENCE_DIR/e5-base-cause-classes.txt"
done < "$EVIDENCE_DIR/e5-base-failed-files.txt"
sort -u -o "$EVIDENCE_DIR/e5-base-cause-classes.txt" "$EVIDENCE_DIR/e5-base-cause-classes.txt" 2>/dev/null || true

if grep -q 'UNRECOGNIZED' "$EVIDENCE_DIR/e5-candidate-cause-classes.txt" 2>/dev/null; then
  cat "$EVIDENCE_DIR/e5-candidate-causes.txt" >&2
  fail "E5: unrecognized CANDIDATE failure cause (a bare command-failed wrapper, a crash, or a foreign diagnostic — not a documented seal class; see e5-candidate-blocks.txt)"
fi
if grep -q 'UNRECOGNIZED' "$EVIDENCE_DIR/e5-base-cause-classes.txt" 2>/dev/null; then
  cat "$EVIDENCE_DIR/e5-base-causes.txt" >&2
  fail "E5: unrecognized BASE failure cause (not the documented seal lag; see e5-base-blocks.txt)"
fi
if grep -q 'uncommitted-source-rejection' "$EVIDENCE_DIR/e5-base-cause-classes.txt" 2>/dev/null; then
  cat "$EVIDENCE_DIR/e5-base-causes.txt" >&2
  fail "E5: BASE replay hit the generators' uncommitted/untracked-source refusal — the baseline is polluted (impossible after the E0 purity assert; baseline construction is broken)"
fi
if grep -q 'uncommitted-source-rejection' "$EVIDENCE_DIR/e5-candidate-cause-classes.txt" 2>/dev/null \
  && [ "$CANDIDATE_DIRTY" != "1" ]; then
  cat "$EVIDENCE_DIR/e5-candidate-causes.txt" >&2
  fail "E5: candidate hit the generators' uncommitted-source refusal although the candidate worktree is CLEAN — contradictory state, failing closed"
fi
# R6-F01: there is no "registered unreadable" class any more — every
# guard refusal without the COMPLETE non-audit-change diagnostic is
# UNRECOGNIZED and has already failed the gate above, at BOTH ends (a
# pristine base must not trip the guard at all; a candidate guard refusal
# whose cause cannot be completely read is an unproven failure).
note "PASS E5-cause-classification (every red BLOCK at both ends independently classified from actual generator/guard diagnostics against COMPLETE end-anchored producer shapes with R8-F01 producer binding: body lines legitimate only under their complete lead, wrapper only first-row-and-only-real-family-commands, structural prefixes never pass-through; repair-group10: the guard-tail WINDOW — generator wrapper + ASCII path fragment + >=1 complete listing line + exactly 500 code points reconstructed — is the registered fourth seal-lag shape, every other window geometry included; RR2 B-R2 R2 form evolution: the generators' patch-too-large replay crash — round2's complete 6-row RuntimeError traceback (File frames bound to the wrapped generator, lines 457/<module> and 359/replay_and_verify) and round3's bare SystemExit line, each ONLY under its OWN generator wrapper and each ending in git's complete \`error: patch too large\` refusal — is the registered fifth seal-lag shape (the family outgrew git's MAX_APPLY_SIZE apply ceiling; still a registered RED, never formal green), every other payload (other git errors, truncated sentences, cross-producer shapes, mutated frames) included; no bare wrapper accepted; no recognized block absorbs an unrecognized sibling; a line carrying a complete known sentence PLUS anything else, and any unknown payload line (fatal:, foreign error, truncated/unregistered-tail-window/missing-coordinate guard refusal, zero payload), is UNRECOGNIZED and has failed the gate; no registered-unreadable acceptance exists)"

# (3b) Class registration — the two documented classes are recorded
# separately and loudly; neither is repainted as formal green.
{
  echo "candidate npm raw exit: $CAND_EXIT"
  echo "base seal-family raw exit: $BASE_EXIT"
  echo "candidate classes by file:"
  cat "$EVIDENCE_DIR/e5-candidate-causes.txt"
  echo "base classes by file:"
  cat "$EVIDENCE_DIR/e5-base-causes.txt"
} > "$EVIDENCE_DIR/e5-class-registration.txt"
note "class registration archived in e5-class-registration.txt (seal-coordinate-lag = committed governance lag — including, since RR2, the generators' patch-too-large replay crash as the registered family's evolved form: the tree outgrew git's MAX_APPLY_SIZE apply ceiling; uncommitted-source-rejection = dirty-tree generator refusal; every other shape — wrapper-only, crash, foreign error, truncated/tail-window/patch-too-large-with-wrong-geometry guard refusal, zero payload — is UNRECOGNIZED and fails the gate; registered reds stay red, never formal green)"

# (4) Per-file cause sets at base vs candidate are REPORTED, not
# set-compared: a family file legitimately GAINS classes as the stage's
# post-seal commits and the dirty worktree state accumulate (base: guard
# cause only; candidate: + verifier cause, + uncommitted-source refusal
# while dirty). The gate is the cause-CLASS check (3) above; this record
# makes the drift explicit.
: > "$EVIDENCE_DIR/e5-cause-comparison.txt"
for f in $(cat "$EVIDENCE_DIR/e5-candidate-failed-files.txt" "$EVIDENCE_DIR/e5-base-failed-files.txt" | sort -u); do
  {
    echo "file: $f"
    echo "  candidate classes: $(grep -F "$f" "$EVIDENCE_DIR/e5-candidate-causes.txt" | cut -f2- | paste -sd' ' - || true)"
    echo "  base classes: $(grep -F "$f" "$EVIDENCE_DIR/e5-base-causes.txt" | cut -f2- | paste -sd' ' - || true)"
  } >> "$EVIDENCE_DIR/e5-cause-comparison.txt"
done
note "per-file cause comparison (union of per-block verdicts) archived in e5-cause-comparison.txt"

# R6-F01: block-count / class-coverage consistency BEFORE any verdict.
# Every failing file must have at least one block row, and its class set
# must be NON-EMPTY — the previous coverage check grepped only for the
# file NAME in the causes file, so a file with zero extracted blocks (or
# zero classes) passed while its failure was never explained. Failing
# files with zero blocks, or blocks with zero explanations, are loud
# failures, never silent gaps.
if [ -s "$EVIDENCE_DIR/e5-candidate-failed-files.txt" ] && [ ! -s "$EVIDENCE_DIR/e5-candidate-blocks.txt" ]; then
  fail "E5: candidate lists failing files but zero blocks were extracted — parser/failure-shape mismatch, failing closed"
fi
if [ -s "$EVIDENCE_DIR/e5-base-failed-files.txt" ] && [ ! -s "$EVIDENCE_DIR/e5-base-blocks.txt" ]; then
  fail "E5: base replay lists failing files but zero blocks were extracted — parser/failure-shape mismatch, failing closed"
fi
while IFS= read -r f; do
  [ -n "$f" ] || fail "E5: empty failing-file identity in the candidate failed-files list — damaged/truncated FAIL header, failing closed"
  # R11-F03: EXACT first-field identity against the blocks file. The
  # producer (extract_blocks) writes `file<TAB>block<TAB>payload` with a
  # real TAB; the old `grep -qF "$f<TAB>"` was an UNANCHORED substring
  # match — file `a` would be satisfied by the row of `x/a`, and any
  # payload line embedding `name<TAB>` could satisfy coverage for a file
  # whose own rows were never extracted. awk compares the COMPLETE first
  # field byte for byte (ENVIRON, not -v, so a legal backslash inside a
  # file name is never re-interpreted); identities with spaces or
  # Unicode match exactly.
  WANT="$f" awk -F '\t' '$1 == ENVIRON["WANT"] { found = 1; exit } END { exit found ? 0 : 1 }' \
    "$EVIDENCE_DIR/e5-candidate-blocks.txt" 2>/dev/null \
    || fail "E5: failing file $f has no block rows in the candidate blocks file (extractor missed the failure shape)"
done < "$EVIDENCE_DIR/e5-candidate-failed-files.txt"
while IFS= read -r f; do
  [ -n "$f" ] || fail "E5: empty failing-file identity in the base failed-files list — damaged/truncated FAIL header, failing closed"
  WANT="$f" awk -F '\t' '$1 == ENVIRON["WANT"] { found = 1; exit } END { exit found ? 0 : 1 }' \
    "$EVIDENCE_DIR/e5-base-blocks.txt" 2>/dev/null \
    || fail "E5: failing file $f has no block rows in the base blocks file (extractor missed the failure shape)"
done < "$EVIDENCE_DIR/e5-base-failed-files.txt"
note "PASS E5-block-coverage (every failing file at both ends has extracted block rows)"

# (5) A red file with zero classifiable blocks means the parser missed the
# failure shape — loud, never silently skipped. R6-F01: the file's class
# line must be present AND non-empty (an empty class row is an unexplained
# failure, exactly like a missing one).
while IFS= read -r f; do
  [ -n "$f" ] || fail "E5: empty failing-file identity in the candidate failed-files list — damaged/truncated FAIL header, failing closed"
  # R11-F03: exact first-field identity (same as the block-coverage
  # match above) — the causes file is written as `file<TAB>classes` with
  # a real TAB; grep -F substring semantics could return a DIFFERENT
  # file's row (`a` matching `x/a`) and let its classes vouch for this
  # file. The full first field must be the file itself.
  line="$(WANT="$f" awk -F '\t' '$1 == ENVIRON["WANT"] { print; exit }' \
    "$EVIDENCE_DIR/e5-candidate-causes.txt" 2>/dev/null || true)"
  [ -n "$line" ] && [ "${line#*$'\t'}" != "" ] \
    || fail "E5: failing file $f has no non-empty cause class in the candidate log"
done < "$EVIDENCE_DIR/e5-candidate-failed-files.txt"
while IFS= read -r f; do
  [ -n "$f" ] || fail "E5: empty failing-file identity in the base failed-files list — damaged/truncated FAIL header, failing closed"
  line="$(WANT="$f" awk -F '\t' '$1 == ENVIRON["WANT"] { print; exit }' \
    "$EVIDENCE_DIR/e5-base-causes.txt" 2>/dev/null || true)"
  [ -n "$line" ] && [ "${line#*$'\t'}" != "" ] \
    || fail "E5: failing file $f has no non-empty cause class in the base log"
done < "$EVIDENCE_DIR/e5-base-failed-files.txt"
note "PASS E5-cause-coverage (every red file at both ends has NON-EMPTY classified causes)"

# Reds at base that are green at the candidate: recorded, not a failure
# (e.g. the orchestrator advancing the seal after the stage closes).
RESOLVED="$(comm -13 "$EVIDENCE_DIR/e5-candidate-failed-files.txt" "$EVIDENCE_DIR/e5-base-failed-files.txt")"
if [ -n "$RESOLVED" ]; then
  printf '%s\n' "$RESOLVED" > "$EVIDENCE_DIR/e5-resolved-since-base.txt"
  note "NOTE resolved-since-base (red at base, green at candidate): $(printf '%s' "$RESOLVED" | tr '\n' ' ')"
fi

note "== R02-A16 legacy-entry regression: GREEN — default entry NOT switched vs $BASE_SHA (Node/Electron incumbent default: E1c entry chain + E1d runtime-default / no-injection / branch-guard / bootstrap-load / cli-default / no-double-write assertions all hold; authorized wiring range archived in e1-surface-diff-files.txt); no new regressions vs baseline (candidate reds ⊆ baseline-replay reds ∪ registered pre-existing families, currently the seal trio); base replay ran on a pristine historical checkout; all reds classified from actual diagnostics with file-level coverage pinned to the summary at both ends (R10-F01: distinct FAIL-header files >= summary files_failed, parser-drift assert) (candidate raw exit $CAND_EXIT, base raw exit $BASE_EXIT — registered, not formal green) =="

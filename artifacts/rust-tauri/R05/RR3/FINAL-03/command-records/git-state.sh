#!/bin/bash
# Read-only git state snapshot for FINAL-03. All git reads use --no-optional-locks.
REPO=/Users/study_superior/Desktop/Code/LingxiAgent
OUT=$1
{
echo "utc: $(date -u +%Y-%m-%dT%H:%M:%S.%NZ)"
echo "HEAD: $(git -C $REPO rev-parse HEAD)"
echo "branch: $(git -C $REPO rev-parse --abbrev-ref HEAD)"
echo "remote-HEAD(origin/codex/rust-tauri-migration): $(git -C $REPO rev-parse origin/codex/rust-tauri-migration 2>/dev/null)"
echo "git version: $(git --version)"
echo "-- git diff HEAD sha256 --"
git -C $REPO --no-optional-locks diff HEAD | shasum -a 256
echo "-- git ls-files -s sha256 --"
git -C $REPO --no-optional-locks ls-files -s | shasum -a 256
echo "-- status counts --"
echo "modified_tracked: $(git -C $REPO --no-optional-locks status --porcelain | grep -c '^ M')"
echo "untracked: $(git -C $REPO --no-optional-locks status --porcelain | grep -c '^??')"
echo "staged: $(git -C $REPO --no-optional-locks status --porcelain | grep -c '^[MARC]' )"
echo "-- reflog head -3 --"
git -C $REPO --no-optional-locks reflog HEAD | head -3
echo "-- .git/index --"
ls -la $REPO/.git/index
echo "-- diff stat --"
git -C $REPO --no-optional-locks diff HEAD --stat | tail -3
} > $OUT 2>&1

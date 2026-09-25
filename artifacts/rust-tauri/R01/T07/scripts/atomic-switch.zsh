#!/bin/zsh
# R01-T07 separated-root prototype: atomic active-directory pointer switch.
# Usage: atomic-switch.zsh <cutover-root> <epoch-target>   (e.g. epoch-1 | epoch-2)
# Performs symlink swap via rename(2) (atomic on POSIX/macOS): build a temp
# symlink, then mv -h it over `active`.
set -eu
ROOT="$1"; TARGET="$2"
case "$TARGET" in
  epoch-1|epoch-2|epoch-1-restored) ;;
  *) echo "refusing unknown target: $TARGET" >&2; exit 2;;
esac
TMP="$ROOT/.active.tmp.$$"
ln -sfn "$ROOT/$TARGET" "$TMP"
mv -h "$TMP" "$ROOT/active"
echo "active -> $(readlink "$ROOT/active")"

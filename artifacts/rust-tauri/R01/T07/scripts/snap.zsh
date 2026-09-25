#!/bin/zsh
# Snapshot a directory: full entry listing (with symlink targets and empty
# dirs) + SHA-256 of every regular file. Usage: snap.zsh <dir> <out-prefix>
set -u
DIR="$1"; OUT="$2"
( cd "$DIR" && find . | sort | while read -r p; do
    if [ -L "$p" ]; then echo "L $p -> $(readlink "$p")";
    elif [ -d "$p" ]; then echo "D $p";
    elif [ -f "$p" ]; then echo "F $p";
    else echo "? $p"; fi
  done ) > "${OUT}.listing"
( cd "$DIR" && find . -type f | sort | xargs shasum -a 256 ) > "${OUT}.sha256" 2>/dev/null || true

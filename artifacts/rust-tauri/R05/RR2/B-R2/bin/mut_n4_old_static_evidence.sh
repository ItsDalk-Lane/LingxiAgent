#!/usr/bin/env bash
# N4: OLD STATIC (tracked) evidence log changes mid-binding
F="$1/artifacts/rust-tauri/R02/T01/a01-build.log"
printf 'rr2-b-r2 N4 mid-binding old-static-evidence change\n' >> "$F"
echo "appended to $F (tracked: $(git -C "$1" ls-files -- "artifacts/rust-tauri/R02/T01/a01-build.log"))"

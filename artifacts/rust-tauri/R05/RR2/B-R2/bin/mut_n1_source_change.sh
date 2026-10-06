#!/usr/bin/env bash
# N1: a REAL tracked source file changes mid-binding
F="$1/rust/crates/lingxi-service/src/lib.rs"
printf '\n// rr2-b-r2 N1 mid-binding source change\n' >> "$F"
echo "appended to $F"

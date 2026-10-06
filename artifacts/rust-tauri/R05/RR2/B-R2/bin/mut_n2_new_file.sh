#!/usr/bin/env bash
# N2: a NEW untracked candidate source file appears mid-binding
F="$1/rust/crates/lingxi-service/src/rr2_b_r2_n2_probe.rs"
printf 'pub fn rr2_b_r2_n2_probe() {}\n' > "$F"
echo "created $F"

set -uo pipefail
COPY="$1"; PRISTINE="$2"
snapshot_pristine() {
  mkdir -p "$PRISTINE/$(dirname "$1")"
  cp "$COPY/$1" "$PRISTINE/$1"
}
for f in \
  rust/crates/xtask/src/stage_maps/R05.json \
  rust/crates/xtask/src/stage_maps/R04.json \
  rust/crates/lingxi-service/src/lib.rs \
  rust/crates/lingxi-kernel/src/lib.rs \
  rust/crates/lingxi-adapters/src/models/tool_render.rs \
  rust/crates/lingxi-adapters/src/models/openai_completions.rs \
  rust/crates/lingxi-adapters/src/models/credentials.rs \
  rust/crates/lingxi-service/src/runs.rs \
  rust/crates/lingxi-service/src/credentials/mod.rs \
  rust/crates/lingxi-service/tests/r05_t01_binary_wiring.rs \
  docs/rust-tauri/R05/r05_stage_pins.tsv \
  scripts/rust-tauri/r05_t08_stage_suites.sh; do
  snapshot_pristine "$f"
done
reset_copy() {
  for f in \
    rust/crates/xtask/src/stage_maps/R05.json \
    rust/crates/xtask/src/stage_maps/R04.json \
    rust/crates/lingxi-service/src/lib.rs \
    rust/crates/lingxi-adapters/src/models/tool_render.rs \
    rust/crates/lingxi-adapters/src/models/openai_completions.rs \
    rust/crates/lingxi-adapters/src/models/credentials.rs \
    rust/crates/lingxi-service/src/runs.rs \
    rust/crates/lingxi-service/src/credentials/mod.rs \
    rust/crates/lingxi-service/tests/r05_t01_binary_wiring.rs \
    docs/rust-tauri/R05/r05_stage_pins.tsv \
    scripts/rust-tauri/r05_t08_stage_suites.sh; do
    cp "$PRISTINE/$f" "$COPY/$f"
  done
}

printf "\n// N06 mid-gate mutation: a candidate byte change during the gate run\n" >> "$COPY/rust/crates/lingxi-kernel/src/lib.rs"
reset_copy
cmp "$COPY/rust/crates/lingxi-kernel/src/lib.rs" "$PRISTINE/rust/crates/lingxi-kernel/src/lib.rs"

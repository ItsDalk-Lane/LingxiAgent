//! Repository-root binding for the in-crate gate binaries
//! (lingxi-protocol-gen / lingxi-protocol-verify).
//!
//! RR-T08-F1 hardening (docs/rust-tauri/R01/RISK_REGISTER.json, owned by
//! R02): these binaries used to resolve the repo root from
//! `env!("CARGO_MANIFEST_DIR")` — a COMPILE-TIME path. Because cargo
//! reuses build artifacts across checkouts whose package content matches
//! (shared/leaked `CARGO_TARGET_DIR`s), a gate could execute a binary
//! compiled in a DIFFERENT checkout and validate that checkout's
//! `contracts/generated` tree instead of the caller's — green light for
//! the wrong tree (observed 2026-09-25: a leftover review-clone binary in
//! `/tmp/lingxi-r01t02-target` made the R01-T08 rerun print
//! "under /private/tmp/r01t02-review-clone/contracts/generated").
//!
//! The fix direction registered in RR-T08-F1: resolve the repo root from
//! the CURRENT WORKING DIRECTORY at runtime and refuse to run unless it
//! matches the compile-time root. The gate scripts additionally derive
//! their default target dir from the checkout path, so two checkouts
//! never share build artifacts in the first place.

use std::path::{Path, PathBuf};

/// Depth from `rust/crates/lingxi-protocol` back to the repo root
/// (manifest dir -> crates -> rust -> repo root).
const MANIFEST_DIR_TO_ROOT_ANCESTORS: usize = 3;

/// Checkout marker used to locate the repo root from the runtime working
/// directory.
const ROOT_MARKER: &str = "rust/Cargo.toml";

/// Resolves the repo root from the CURRENT WORKING DIRECTORY (walking up
/// until a checkout marker is found) and refuses — with a diagnostic, not
/// a guess — unless it equals the compile-time repo root of THIS binary.
///
/// Returns the bound (canonicalized) repo root on success. Never falls
/// back to the compile-time root: a silent fallback would reintroduce the
/// RR-T08-F1 wrong-tree binding.
pub fn bound_repo_root() -> Result<PathBuf, String> {
    let compile_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(MANIFEST_DIR_TO_ROOT_ANCESTORS)
        .ok_or_else(|| {
            "binary is not located at rust/crates/<crate> inside a checkout".to_string()
        })?
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize compile-time repo root: {e}"))?;

    let cwd = std::env::current_dir()
        .map_err(|e| format!("cannot read the current working directory: {e}"))?;

    let mut dir: Option<&Path> = Some(cwd.as_path());
    while let Some(d) = dir {
        if d.join(ROOT_MARKER).is_file() {
            let runtime_root = d
                .canonicalize()
                .map_err(|e| format!("cannot canonicalize {}: {e}", d.display()))?;
            if runtime_root != compile_root {
                return Err(format!(
                    "repo-root mismatch: this binary was compiled in {:?}, but the \
                     current working directory belongs to {:?}. Refusing to run a \
                     gate across checkouts (RR-T08-F1): the artifacts of one \
                     checkout must never validate another checkout's tree. Rebuild \
                     inside the checkout you are gating.",
                    compile_root.display(),
                    runtime_root.display()
                ));
            }
            return Ok(runtime_root);
        }
        dir = d.parent();
    }
    Err(format!(
        "the current working directory {:?} is not inside a Lingxi checkout \
         (no {} ancestor found); refusing to guess the repo root",
        cwd.display(),
        ROOT_MARKER
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The only behavior testable without spawning a second compilation:
    /// when the test harness's CWD IS inside this checkout (cargo runs
    /// tests with CWD = the package root), the binding must succeed and
    /// return the real repo root. The mismatch and no-marker refusals are
    /// proven binary-level in artifacts/rust-tauri/R02/T08/f1-hardening/
    /// (a mismatch needs a binary compiled in a different checkout, which
    /// a unit test cannot have).
    #[test]
    fn binds_to_this_checkout_when_cwd_is_inside_it() {
        let root = bound_repo_root().expect("binding must succeed inside the checkout");
        assert!(root.join("rust/Cargo.toml").is_file());
        assert!(root.join("contracts/generated").is_dir());
    }
}

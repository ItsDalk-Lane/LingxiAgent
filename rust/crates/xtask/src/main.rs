//! xtask — the Rust verification orchestrator established by R02-T08
//! (taskbook `05_验收与性能协议.md` §3 "新 Rust 验证接口").
//!
//! Subcommands (all of them execute REAL registered commands and propagate
//! their REAL exit codes — a gate that only counts hand-filled PASS values
//! in some JSON is exactly what this binary must never be):
//!
//! ```text
//! cargo run --manifest-path rust/Cargo.toml -p xtask -- check-contracts
//! cargo run --manifest-path rust/Cargo.toml -p xtask -- check-boundaries
//! cargo run --manifest-path rust/Cargo.toml -p xtask -- verify-stage R02 --evidence artifacts/rust-tauri/R02
//! cargo run --manifest-path rust/Cargo.toml -p xtask -- --help
//! ```
//!
//! - `check-contracts`  — regenerated contracts vs the committed tree
//!   (`scripts/rust-tauri/r01-t02-check-generated.sh`: lingxi-protocol-gen
//!   --check + API surface extract --check).
//! - `check-boundaries` — the machine-enforced ownership/dependency
//!   contract (`docs/rust-tauri/R01/r01_t01_check_ownership.py`:
//!   O1–O8 + D1–D5 incl. the workspace-member reverse closure).
//! - `verify-stage S`   — reads the stage's implementation-acceptance map
//!   (`src/stage_maps/<S>.json`), executes every registered command, and
//!   writes a machine-readable result (real exit codes, platform, tested
//!   SHA, per-command logs) under `--evidence`. Non-zero exit on: unknown
//!   stage, invalid/empty map, missing evidence files, timeout, or any
//!   command failure.
//!
//! RR-T08-F1 hardening (lesson of the R01 wrong-tree green): the repo root
//! is resolved from the CURRENT WORKING DIRECTORY at runtime and compared
//! against the compile-time root — a mismatch is a refusal, never a guess.
//! A binary (or target dir) reused across checkouts must not gate a tree
//! it was not built for.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

pub mod stage_map;
pub mod verify;

pub use stage_map::{parse_stage_map, CommandSpec, Scenario, StageMap};
pub use verify::{verify_stage, CommandOutcome};

/// Result schema version of `verify-stage` output.
pub const RESULT_VERSION: &str = "lingxi.xtask.verify-stage.v1";

/// Embedded per-stage implementation-acceptance maps. A later stage (R03+)
/// adds its own file here in the same PR that registers its scenarios.
pub const STAGE_MAPS: &[(&str, &str)] = &[("R02", include_str!("stage_maps/R02.json"))];

const USAGE: &str = r#"xtask — Lingxi Rust verification orchestrator (R02-T08)

usage:
  cargo run --manifest-path rust/Cargo.toml -p xtask -- <SUBCOMMAND>

subcommands:
  check-contracts                regenerate contracts and diff against the
                                 committed tree (scripts/rust-tauri/
                                 r01-t02-check-generated.sh); exit 0 only if
                                 drift-free
  check-boundaries               machine-enforced ownership + dependency
                                 contract (docs/rust-tauri/R01/
                                 r01_t01_check_ownership.py, O1-O8 + D1-D5);
                                 exit 0 only if no violation
  verify-stage <STAGE> --evidence <DIR>
                                 execute the stage's registered acceptance
                                 commands (stage map: xtask
                                 src/stage_maps/<STAGE>.json), archive each
                                 command's stdout/stderr under DIR, verify
                                 declared evidence files exist, and write
                                 DIR/verify-stage-result.json with real exit
                                 codes, platform and tested SHA. Non-zero on
                                 unknown stage, invalid/empty map, missing
                                 evidence, timeout, or any command failure.
  --help                         print this help and exit 0

notes:
  - every registered command is a REAL command (cargo tests / gate scripts /
    checkers); results are taken from real exit codes, never from PASS
    strings inside JSON;
  - the repo root is bound from the current working directory and must match
    the compile-time root (RR-T08-F1) — cross-checkout reuse is refused.
"#;

/// Resolves the repo root from the current working directory (walking up to
/// the checkout that owns `rust/Cargo.toml`) and refuses unless it equals
/// the compile-time repo root of this binary. See the crate docs (RR-T08-F1).
pub fn bound_repo_root() -> Result<PathBuf, String> {
    let compile_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .ok_or_else(|| "xtask is not located at rust/crates/xtask".to_string())?
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize compile-time repo root: {e}"))?;
    let cwd = std::env::current_dir()
        .map_err(|e| format!("cannot read the current working directory: {e}"))?;
    let mut dir: Option<&Path> = Some(cwd.as_path());
    while let Some(d) = dir {
        if d.join("rust/Cargo.toml").is_file() {
            let runtime_root = d
                .canonicalize()
                .map_err(|e| format!("cannot canonicalize {}: {e}", d.display()))?;
            if runtime_root != compile_root {
                return Err(format!(
                    "repo-root mismatch: this xtask was compiled in {:?}, but the \
                     current working directory belongs to {:?}. Refusing to run \
                     gates across checkouts (RR-T08-F1).",
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
         (no rust/Cargo.toml ancestor)",
        cwd.display()
    ))
}

fn run_gate(root: &Path, argv: &[&str]) -> ExitCode {
    assert!(!argv.is_empty());
    let print = argv.join(" ");
    println!("xtask: running [{print}] in {}", root.display());
    let status = match Command::new(argv[0])
        .args(&argv[1..])
        .current_dir(root)
        .status()
    {
        Ok(status) => status,
        Err(err) => {
            eprintln!("xtask: cannot spawn [{print}]: {err}");
            return ExitCode::FAILURE;
        }
    };
    if status.success() {
        println!("xtask: [{print}] exit 0 (OK)");
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "xtask: [{print}] FAILED with {}",
            status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "<signal>".into())
        );
        ExitCode::FAILURE
    }
}

fn git_head_sha(root: &Path) -> Result<String, String> {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run git rev-parse HEAD: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git rev-parse HEAD failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_worktree_dirty(root: &Path) -> Result<bool, String> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run git status --porcelain: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git status failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(!out.stdout.is_empty())
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--help")
        || args.first().map(String::as_str) == Some("-h")
    {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let Some(sub) = args.first() else {
        eprintln!("error: a subcommand is required\n\n{USAGE}");
        return ExitCode::from(2);
    };

    // RR-T08-F1: bind the checkout before doing anything else.
    let root = match bound_repo_root() {
        Ok(root) => root,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };

    match sub.as_str() {
        "check-contracts" => run_gate(
            &root,
            &["bash", "scripts/rust-tauri/r01-t02-check-generated.sh"],
        ),
        "check-boundaries" => run_gate(
            &root,
            &[
                "python3",
                "-B",
                "docs/rust-tauri/R01/r01_t01_check_ownership.py",
            ],
        ),
        "verify-stage" => cmd_verify_stage(&root, &args[1..]),
        other => {
            eprintln!(
                "error: unknown subcommand {other:?} (expected check-contracts, \
                 check-boundaries, verify-stage or --help)\n\n{USAGE}"
            );
            ExitCode::from(2)
        }
    }
}

fn cmd_verify_stage(root: &Path, rest: &[String]) -> ExitCode {
    let mut stage: Option<&str> = None;
    let mut evidence: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--evidence" => {
                if evidence.is_some() {
                    eprintln!("error: --evidence given twice");
                    return ExitCode::from(2);
                }
                match rest.get(i + 1) {
                    Some(v) => {
                        evidence = Some(v);
                        i += 2;
                    }
                    None => {
                        eprintln!("error: --evidence requires a directory argument");
                        return ExitCode::from(2);
                    }
                }
            }
            "--help" | "-h" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            value if stage.is_none() && !value.starts_with("--") => {
                stage = Some(value);
                i += 1;
            }
            other => {
                eprintln!("error: unexpected verify-stage argument {other:?}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(stage) = stage else {
        eprintln!("error: verify-stage requires a stage id (e.g. verify-stage R02)");
        return ExitCode::from(2);
    };
    let Some(evidence) = evidence else {
        eprintln!("error: verify-stage requires --evidence <DIR>");
        return ExitCode::from(2);
    };

    let Some((_, map_json)) = STAGE_MAPS.iter().find(|(id, _)| *id == stage) else {
        let known: Vec<&str> = STAGE_MAPS.iter().map(|(id, _)| *id).collect();
        eprintln!(
            "error: unknown stage {stage:?}; no implementation-acceptance map is \
             registered in xtask (registered stages: {}). A stage without a map \
             is a hard error — never an empty pass.",
            known.join(", ")
        );
        return ExitCode::from(2);
    };

    let map = match parse_stage_map(map_json) {
        Ok(map) => map,
        Err(err) => {
            eprintln!("error: invalid stage map for {stage}: {err}");
            return ExitCode::from(2);
        }
    };
    if map.scenarios.is_empty() {
        // Defense in depth: parse_stage_map already rejects this; the check
        // stays here because an EMPTY registered scenario set is exactly the
        // "nothing to verify" pass this tool must never produce.
        eprintln!("error: stage map for {stage} registers an EMPTY scenario set");
        return ExitCode::from(2);
    }

    let tested_sha = match git_head_sha(root) {
        Ok(sha) => sha,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };
    let dirty = match git_worktree_dirty(root) {
        Ok(dirty) => dirty,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };

    let evidence_root = root.join(evidence);
    let started = now_ms();
    let report = match verify_stage(&map, root, &evidence_root, &tested_sha, dirty, started) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("error: verify-stage for {stage} aborted: {err}");
            return ExitCode::from(1);
        }
    };

    let overall_pass = report["overall"] == "PASS";
    let result_path = evidence_root.join("verify-stage-result.json");
    let mut result_bytes = serde_json::to_vec_pretty(&report).expect("result JSON serializes");
    result_bytes.push(b'\n');
    if let Err(err) = std::fs::write(&result_path, result_bytes) {
        eprintln!("error: cannot write {}: {err}", result_path.display());
        return ExitCode::from(1);
    }
    println!(
        "xtask: verify-stage {stage} result written to {} (overall: {})",
        result_path.display(),
        report["overall"]
    );
    if overall_pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

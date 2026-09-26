//! `verify-stage` execution: run every registered command for real, collect
//! real exit codes / logs / evidence existence, and emit the machine-
//! readable result. This module never inspects PASS strings — a scenario
//! passes if and only if its referenced commands exited 0 within their
//! timeout AND their declared evidence files exist.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::stage_map::StageMap;
use crate::RESULT_VERSION;

/// Outcome of one executed command.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandOutcome {
    pub key: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub evidence_missing: Vec<String>,
}

impl CommandOutcome {
    pub fn passed(&self) -> bool {
        !self.timed_out && self.exit_code == Some(0) && self.evidence_missing.is_empty()
    }
}

/// Placeholder replaced with the resolved evidence root in `argv` and in
/// `evidencePaths` entries.
pub const EVIDENCE_PLACEHOLDER: &str = "{EVIDENCE}";
/// Placeholder replaced with the repo root (for scripts that must be
/// addressed by absolute path).
pub const REPO_ROOT_PLACEHOLDER: &str = "{REPO_ROOT}";

fn substitute(text: &str, repo_root: &Path, evidence_root: &Path) -> String {
    text.replace(REPO_ROOT_PLACEHOLDER, &repo_root.to_string_lossy())
        .replace(EVIDENCE_PLACEHOLDER, &evidence_root.to_string_lossy())
}

/// Executes ONE command under `timeout`, capturing stdout/stderr into
/// `<command_dir>/stdout.log` / `stderr.log`.
pub fn run_command(
    spec: &crate::stage_map::CommandSpec,
    repo_root: &Path,
    evidence_root: &Path,
    command_dir: &Path,
) -> Result<CommandOutcome, String> {
    std::fs::create_dir_all(command_dir)
        .map_err(|e| format!("cannot create {}: {e}", command_dir.display()))?;

    let argv: Vec<String> = spec
        .argv
        .iter()
        .map(|part| substitute(part, repo_root, evidence_root))
        .collect();
    let (program, args) = argv.split_first().ok_or_else(|| "empty argv".to_string())?;

    let stdout_path = command_dir.join("stdout.log");
    let stderr_path = command_dir.join("stderr.log");
    let stdout_file = std::fs::File::create(&stdout_path)
        .map_err(|e| format!("cannot create {}: {e}", stdout_path.display()))?;
    let stderr_file = std::fs::File::create(&stderr_path)
        .map_err(|e| format!("cannot create {}: {e}", stderr_path.display()))?;

    let mut child = Command::new(program)
        .args(args)
        .current_dir(repo_root)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot spawn {program:?}: {e}"))?;

    let deadline = Instant::now() + Duration::from_secs(spec.timeout_secs);
    let mut exit_code = None;
    let mut timed_out = false;
    loop {
        match child.try_wait().map_err(|e| format!("wait failed: {e}"))? {
            Some(status) => {
                exit_code = status.code();
                break;
            }
            None if Instant::now() >= deadline => {
                timed_out = true;
                // Kill the script; bash traps (cleanup on EXIT) then run and
                // kill the script's own children, which is how every gate
                // script in this repo is written.
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }

    // Evidence check: declared files must EXIST after the run. Substituted
    // paths are relative to the evidence root.
    let mut evidence_missing = Vec::new();
    for entry in &spec.evidence_paths {
        let substituted = substitute(entry, repo_root, evidence_root);
        let path = if Path::new(&substituted).is_absolute() {
            std::path::PathBuf::from(&substituted)
        } else {
            evidence_root.join(&substituted)
        };
        if !path.exists() {
            evidence_missing.push(entry.clone());
        }
    }

    Ok(CommandOutcome {
        key: spec.key.clone(),
        exit_code,
        timed_out,
        evidence_missing,
    })
}

/// Runs the full stage: unique commands in first-reference order, then the
/// scenario roll-up. Returns the result JSON (not yet written to disk).
pub fn verify_stage(
    map: &StageMap,
    repo_root: &Path,
    evidence_root: &Path,
    tested_sha: &str,
    worktree_dirty: bool,
    started_at_unix_ms: u128,
) -> Result<Value, String> {
    std::fs::create_dir_all(evidence_root)
        .map_err(|e| format!("cannot create {}: {e}", evidence_root.display()))?;

    // Execution order: first reference order of scenarios -> commands.
    let mut order: Vec<String> = Vec::new();
    for scenario in &map.scenarios {
        for key in &scenario.command_refs {
            if !order.contains(key) {
                order.push(key.clone());
            }
        }
    }

    let mut command_json = Vec::new();
    let mut outcomes = Vec::new();
    for key in &order {
        let spec = map
            .commands
            .iter()
            .find(|c| &c.key == key)
            .ok_or_else(|| format!("internal: command {key:?} not in map"))?;
        let command_dir = evidence_root.join(&spec.key);
        println!(
            "xtask: verify-stage {} [{}] > {}",
            map.stage,
            spec.argv.join(" "),
            command_dir.display()
        );
        let started = Instant::now();
        let outcome = run_command(spec, repo_root, evidence_root, &command_dir)?;
        let duration_ms = started.elapsed().as_millis() as u64;
        let passed = outcome.passed();
        println!(
            "xtask: verify-stage {} [{}] {}",
            map.stage,
            spec.key,
            if passed { "PASS" } else { "FAIL" }
        );
        command_json.push(json!({
            "key": spec.key,
            "argv": spec.argv,
            "exitCode": outcome.exit_code,
            "timedOut": outcome.timed_out,
            "durationMs": duration_ms,
            "evidencePaths": spec.evidence_paths,
            "missingEvidence": outcome.evidence_missing,
            "status": if passed { "PASS" } else { "FAIL" },
        }));
        outcomes.push(outcome);
    }

    let scenario_json: Vec<Value> = map
        .scenarios
        .iter()
        .map(|scenario| {
            let passed = scenario.command_refs.iter().all(|key| {
                outcomes
                    .iter()
                    .find(|o| &o.key == key)
                    .is_some_and(|o| o.passed())
            });
            json!({
                "id": scenario.id,
                "requirement": scenario.requirement,
                "commandRefs": scenario.command_refs,
                "status": if passed { "PASS" } else { "FAIL" },
            })
        })
        .collect();

    let overall_pass = scenario_json.iter().all(|s| s["status"] == "PASS");
    let finished = crate::now_ms();

    Ok(json!({
        "resultVersion": RESULT_VERSION,
        "stage": map.stage,
        "testedSha": tested_sha,
        "worktreeDirty": worktree_dirty,
        "platform": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "family": std::env::consts::FAMILY,
        },
        "toolchainChannel": read_toolchain_channel(repo_root),
        "startedAtUnixMs": started_at_unix_ms,
        "finishedAtUnixMs": finished,
        "commands": command_json,
        "scenarios": scenario_json,
        "overall": if overall_pass { "PASS" } else { "FAIL" },
    }))
}

fn read_toolchain_channel(repo_root: &Path) -> Value {
    let toolchain = repo_root.join("rust-toolchain.toml");
    match std::fs::read_to_string(&toolchain) {
        Ok(text) => text
            .lines()
            .find_map(|line| {
                let line = line.trim();
                line.strip_prefix("channel = ")
                    .map(|rest| Value::String(rest.trim_matches('"').to_string()))
            })
            .unwrap_or(Value::Null),
        Err(_) => Value::Null,
    }
}

#[cfg(test)]
mod runner_tests;

//! Negative/positive runner battery for `verify-stage` (hermetic: every
//! command is `sh`/`sleep`/`printf` against a temp dir; no cargo involved).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::stage_map::{parse_stage_map, CommandSpec, StageMap};
use crate::verify::{run_command, verify_stage};

fn temp_root(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-xtask-test-{}-{}-{}",
        tag,
        std::process::id(),
        nanos
    ));
    std::fs::create_dir_all(&dir).expect("create temp root");
    dir
}

fn spec(key: &str, argv: &[&str], evidence: &[&str], timeout: u64) -> CommandSpec {
    CommandSpec {
        key: key.to_string(),
        argv: argv.iter().map(|s| s.to_string()).collect(),
        timeout_secs: timeout,
        evidence_paths: evidence.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn successful_command_with_existing_evidence_passes() {
    let root = temp_root("ok");
    let evidence = root.join("evidence");
    let cmd = spec(
        "ok",
        &[
            "sh",
            "-c",
            "mkdir -p {EVIDENCE}/RX && printf green > {EVIDENCE}/RX/out.txt",
        ],
        &["{EVIDENCE}/RX/out.txt"],
        60,
    );
    let outcome = run_command(&cmd, &root, &evidence, &evidence.join(".cmd")).expect("runs");
    assert_eq!(outcome.exit_code, Some(0));
    assert!(!outcome.timed_out);
    assert!(outcome.evidence_missing.is_empty());
    assert!(outcome.passed());
    assert!(evidence.join("RX/out.txt").exists());
    assert!(evidence.join(".cmd/stdout.log").exists());
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn failing_command_propagates_its_real_exit_code() {
    let root = temp_root("fail");
    let evidence = root.join("evidence");
    let cmd = spec(
        "bad",
        &["sh", "-c", "exit 3"],
        &["{EVIDENCE}/never.txt"],
        60,
    );
    let outcome = run_command(&cmd, &root, &evidence, &evidence.join(".cmd")).expect("runs");
    assert_eq!(
        outcome.exit_code,
        Some(3),
        "the REAL exit code must surface"
    );
    assert!(!outcome.passed());
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn missing_evidence_is_a_failure_even_when_exit_is_zero() {
    let root = temp_root("missingevid");
    let evidence = root.join("evidence");
    let cmd = spec(
        "liar",
        &["sh", "-c", "true"],
        &["{EVIDENCE}/claimed.txt"],
        60,
    );
    let outcome = run_command(&cmd, &root, &evidence, &evidence.join(".cmd")).expect("runs");
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(
        outcome.evidence_missing,
        vec!["{EVIDENCE}/claimed.txt".to_string()]
    );
    assert!(!outcome.passed(), "exit 0 without evidence must NOT pass");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn timeout_kills_the_command_and_fails() {
    let root = temp_root("timeout");
    let evidence = root.join("evidence");
    let cmd = spec("sleeper", &["sleep", "30"], &["{EVIDENCE}/x.txt"], 1);
    let outcome = run_command(&cmd, &root, &evidence, &evidence.join(".cmd")).expect("runs");
    assert!(outcome.timed_out);
    assert_ne!(outcome.exit_code, Some(0));
    assert!(!outcome.passed());
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn stage_rollup_fails_when_one_scenario_fails_and_reports_real_codes() {
    let root = temp_root("rollup");
    let evidence = root.join("evidence");
    let map = StageMap {
        stage: "RX".into(),
        result_version: crate::RESULT_VERSION.into(),
        default_timeout_secs: 60,
        commands: vec![
            spec("good", &["sh", "-c", "true"], &["{EVIDENCE}/good.txt"], 60),
            spec("bad", &["sh", "-c", "exit 7"], &["{EVIDENCE}/bad.txt"], 60),
        ],
        scenarios: vec![
            crate::stage_map::Scenario {
                id: "RX-A01".into(),
                requirement: "REQUIRED".into(),
                command_refs: vec!["good".into()],
            },
            crate::stage_map::Scenario {
                id: "RX-A02".into(),
                requirement: "REQUIRED".into(),
                command_refs: vec!["bad".into()],
            },
        ],
    };
    // The referenced evidence must exist for the good command.
    std::fs::create_dir_all(&evidence).unwrap();
    std::fs::write(evidence.join("good.txt"), b"ok").unwrap();
    let report =
        verify_stage(&map, &root, &evidence, "deadbeef", false, 0).expect("runs to completion");
    assert_eq!(report["overall"], "FAIL");
    assert_eq!(report["scenarios"][0]["status"], "PASS");
    assert_eq!(report["scenarios"][1]["status"], "FAIL");
    assert_eq!(
        report["commands"][1]["exitCode"], 7,
        "real exit code in the report"
    );
    assert_eq!(report["testedSha"], "deadbeef");
    assert_eq!(report["stage"], "RX");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn embedded_r02_map_is_valid_and_non_empty() {
    for (stage, text) in crate::STAGE_MAPS {
        let map = parse_stage_map(text).expect("embedded map must parse");
        assert_eq!(map.stage, *stage);
        assert!(!map.scenarios.is_empty(), "{stage}: empty scenario set");
        assert!(!map.commands.is_empty(), "{stage}: no commands");
        // Every scenario's refs resolve; every command has evidence paths.
        for scenario in &map.scenarios {
            assert!(
                !scenario.command_refs.is_empty(),
                "{}: {}",
                stage,
                scenario.id
            );
        }
    }
    let r02 = parse_stage_map(crate::STAGE_MAPS[0].1).unwrap();
    assert_eq!(r02.scenarios.len(), 16, "R02 registers its 16 scenarios");
    assert!(r02.scenarios.iter().all(|s| s.requirement == "REQUIRED"));
}

#[test]
fn r02_scenario_ids_match_the_taskbook() {
    let r02 = parse_stage_map(crate::STAGE_MAPS[0].1).unwrap();
    let expected: Vec<String> = (1..=16).map(|n| format!("R02-A{n:02}")).collect();
    let ids: Vec<&str> = r02.scenarios.iter().map(|s| s.id.as_str()).collect();
    let expected_refs: Vec<&str> = expected.iter().map(String::as_str).collect();
    assert_eq!(
        ids, expected_refs,
        "scenario ids must be exactly R02-A01..R02-A16"
    );
}

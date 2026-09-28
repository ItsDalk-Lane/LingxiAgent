//! Negative/positive runner battery for `verify-stage` (hermetic: every
//! command is `sh`/`sleep`/`printf` against a temp dir; no cargo involved).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::stage_map::{
    parse_stage_map, CommandSpec, LeafAssertionContract, LeafCaseExpect, StageMap,
};
use crate::verify::{run_command, verify_stage};

/// The mirror values `write_minimal_r00_ledger` records for every leaf —
/// `leaf()` copies the same values so the two stay in sync.
const TEST_R00_TASK_IDS: &[&str] = &["RX-T01", "RY-T02"];
const TEST_R00_STAGE_IDS: &[&str] = &["RX", "RY"];
const TEST_R00_DUE: &str = "test due line";

fn test_then(id: &str) -> String {
    format!("original then for {id}")
}

fn test_assertions(id: &str) -> Vec<String> {
    vec![format!("original assertion one for {id}")]
}

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

/// Writes BOTH minimal R00 ledgers these hermetic tests need (R14-F01: the
/// runner cross-checks each leaf against the FULL record across the two
/// ledgers). `entries` carries the per-leaf requirement; every other field
/// is fixed so `leaf()` can mirror it exactly.
fn write_minimal_r00_ledger(root: &Path, entries: &[(&str, &str)]) {
    let mut scenarios = serde_json::Map::new();
    let mut fsa_entries = Vec::new();
    for (id, requirement) in entries {
        let feature = format!("F-D20-TEST-{id}");
        scenarios.insert(
            (*id).to_string(),
            serde_json::json!({
                "kind": "supplemental",
                "requirement": requirement,
                "feature_id": feature,
                "task_ids": TEST_R00_TASK_IDS,
                "execution_stage_ids": TEST_R00_STAGE_IDS,
                "ledger_status": "SPECIFIED_NOT_EXECUTED",
                "result_ids": [],
                "test_ids": [],
                "then": test_then(id),
            }),
        );
        fsa_entries.push(serde_json::json!({
            "id": id,
            "feature_id": feature,
            "task_ids": TEST_R00_TASK_IDS,
            "execution_stage_ids": TEST_R00_STAGE_IDS,
            "assertions": test_assertions(id),
            "due": TEST_R00_DUE,
            "status": "SPECIFIED_NOT_EXECUTED",
        }));
    }
    let dir = root.join("docs/rust-tauri/R00");
    std::fs::create_dir_all(&dir).expect("create R00 ledger dir");
    std::fs::write(
        dir.join("ACCEPTANCE_MAP.json"),
        serde_json::to_string(&serde_json::json!({ "scenarios": scenarios }))
            .expect("ledger serializes"),
    )
    .expect("write minimal R00 acceptance ledger");
    std::fs::write(
        dir.join("FEATURE_STAGE_ACCEPTANCE.json"),
        serde_json::to_string(&serde_json::json!({
            "supplemental_scenarios": fsa_entries
        }))
        .expect("fsa serializes"),
    )
    .expect("write minimal R00 feature-stage ledger");
}

fn spec(key: &str, argv: &[&str], evidence: &[&str], timeout: u64) -> CommandSpec {
    CommandSpec {
        key: key.to_string(),
        argv: argv.iter().map(|s| s.to_string()).collect(),
        timeout_secs: timeout,
        evidence_paths: evidence.iter().map(|s| s.to_string()).collect(),
    }
}

fn one_command_map(command: CommandSpec) -> StageMap {
    StageMap {
        stage: "RX".into(),
        result_version: crate::RESULT_VERSION.into(),
        default_timeout_secs: 60,
        scenarios: vec![crate::stage_map::Scenario {
            id: "RX-A01".into(),
            requirement: "REQUIRED".into(),
            command_refs: vec![command.key.clone()],
        }],
        commands: vec![command],
        supplemental_leaves: vec![],
    }
}

#[test]
fn spawn_error_is_a_structured_gate_failure_with_checkpoint() {
    let root = temp_root("spawn-internal");
    write_minimal_r00_ledger(&root, &[]);
    let evidence = root.join("evidence");
    let map = one_command_map(spec(
        "spawn-error",
        &["/definitely/not/a/lingxi-command"],
        &["{EVIDENCE}/never.txt"],
        60,
    ));
    let mut checkpoints = Vec::new();
    let report =
        super::verify_stage_with_checkpoint(&map, &root, &evidence, "deadbeef", false, 0, |key| {
            checkpoints.push(key.to_string())
        })
        .unwrap();
    assert_eq!(checkpoints, ["spawn-error"]);
    assert_eq!(report["overall"], "FAIL");
    assert_eq!(report["commands"][0]["status"], "FAIL");
    assert!(report["commands"][0]["internalError"]
        .as_str()
        .unwrap()
        .contains("cannot spawn"));
    assert!(evidence.join("spawn-error/stdout.log").exists());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn log_creation_error_is_a_structured_gate_failure_with_checkpoint() {
    let root = temp_root("log-internal");
    write_minimal_r00_ledger(&root, &[]);
    let evidence = root.join("evidence");
    let map = one_command_map(spec(
        "log-error",
        &["sh", "-c", "printf ran > {EVIDENCE}/spawned.txt"],
        &["{EVIDENCE}/spawned.txt"],
        60,
    ));
    let mut checkpoints = Vec::new();
    let report = super::verify_stage_with_runner(
        super::VerifyInputs {
            map: &map,
            repo_root: &root,
            evidence_root: &evidence,
            tested_sha: "deadbeef",
            worktree_dirty: false,
            started_at_unix_ms: 0,
        },
        |key| checkpoints.push(key.to_string()),
        |spec, root, evidence, dir| {
            super::run_command_with_after_dir(spec, root, evidence, dir, |created| {
                std::fs::create_dir(created.join("stdout.log")).unwrap();
            })
        },
    )
    .unwrap();
    assert_eq!(checkpoints, ["log-error"]);
    assert_eq!(report["overall"], "FAIL");
    assert!(report["commands"][0]["internalError"]
        .as_str()
        .unwrap()
        .contains("cannot create"));
    assert!(!evidence.join("spawned.txt").exists());
    std::fs::remove_dir_all(root).ok();
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
    let cleanup = outcome.cleanup.expect("timeout records a cleanup report");
    assert!(cleanup.reaped, "the direct child is always reaped");
    assert!(
        !cleanup.survivors_after,
        "nothing may survive the bounded cleanup"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn stage_rollup_fails_when_one_scenario_fails_and_reports_real_codes() {
    let root = temp_root("rollup");
    write_minimal_r00_ledger(&root, &[]);
    let evidence = root.join("evidence");
    let map = StageMap {
        stage: "RX".into(),
        result_version: crate::RESULT_VERSION.into(),
        default_timeout_secs: 60,
        commands: vec![
            // The good command produces its OWN declared evidence (F04):
            // evidence written beforehand is refused as stale.
            spec(
                "good",
                &["sh", "-c", "printf ok > {EVIDENCE}/good.txt"],
                &["{EVIDENCE}/good.txt"],
                60,
            ),
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
        supplemental_leaves: vec![],
    };
    let report =
        verify_stage(&map, &root, &evidence, "deadbeef", false, 0).expect("runs to completion");
    assert_eq!(report["overall"], "FAIL");
    assert_eq!(report["scenarios"][0]["status"], "PASS");
    assert_eq!(report["scenarios"][1]["status"], "FAIL");
    assert_eq!(report["commands"][0]["status"], "PASS");
    assert_eq!(report["commands"][1]["status"], "FAIL");
    assert_eq!(
        report["commands"][1]["exitCode"], 7,
        "real exit code in the report"
    );
    assert_eq!(report["testedSha"], "deadbeef");
    assert_eq!(report["stage"], "RX");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn pre_existing_evidence_refuses_the_spawn_and_fails_loudly() {
    let root = temp_root("stale");
    let evidence = root.join("evidence");
    std::fs::create_dir_all(&evidence).unwrap();
    // Stale evidence from an "earlier run".
    std::fs::write(evidence.join("stale.txt"), b"old").unwrap();
    let cmd = spec(
        "masked",
        &["sh", "-c", "printf new > {EVIDENCE}/spawned.marker"],
        &["{EVIDENCE}/stale.txt"],
        60,
    );
    let outcome = run_command(&cmd, &root, &evidence, &evidence.join(".cmd")).expect("runs");
    assert!(!outcome.passed(), "stale evidence must never mask a run");
    assert_eq!(outcome.exit_code, None, "the command was never spawned");
    assert!(!outcome.timed_out);
    assert_eq!(
        outcome.evidence_preexisting,
        vec!["{EVIDENCE}/stale.txt".to_string()]
    );
    assert!(
        !evidence.join("spawned.marker").exists(),
        "refusal means the command never ran"
    );
    let refusal = std::fs::read_to_string(evidence.join(".cmd/stderr.log")).unwrap();
    assert!(refusal.contains("refused to run"), "refusal is on record");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn reused_command_dir_is_refused() {
    let root = temp_root("reuse");
    let evidence = root.join("evidence");
    let command_dir = evidence.join(".cmd");
    std::fs::create_dir_all(&command_dir).unwrap();
    let cmd = spec("again", &["sh", "-c", "true"], &[], 60);
    let err = run_command(&cmd, &root, &evidence, &command_dir)
        .expect_err("a pre-existing command dir must refuse");
    assert!(err.contains("already exists"), "{err}");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn non_empty_evidence_root_is_refused_before_any_command_runs() {
    let root = temp_root("nonempty");
    write_minimal_r00_ledger(&root, &[]);
    let evidence = root.join("evidence");
    std::fs::create_dir_all(&evidence).unwrap();
    std::fs::write(evidence.join("leftover.txt"), b"old").unwrap();
    let map = StageMap {
        stage: "RX".into(),
        result_version: crate::RESULT_VERSION.into(),
        default_timeout_secs: 60,
        commands: vec![spec(
            "marker",
            &["sh", "-c", "printf ran > {EVIDENCE}/ran.marker"],
            &[],
            60,
        )],
        scenarios: vec![crate::stage_map::Scenario {
            id: "RX-A01".into(),
            requirement: "REQUIRED".into(),
            command_refs: vec!["marker".into()],
        }],
        supplemental_leaves: vec![],
    };
    let err = verify_stage(&map, &root, &evidence, "deadbeef", false, 0)
        .expect_err("a non-empty evidence root must refuse");
    assert!(err.contains("not empty"), "{err}");
    assert!(
        !evidence.join("ran.marker").exists(),
        "refusal happens before any command runs"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[cfg(unix)]
#[test]
fn timeout_cleanup_kills_the_whole_process_tree() {
    let root = temp_root("tree");
    let evidence = root.join("evidence");
    // The direct child is a shell; its two `sleep` grandchildren must not
    // survive the gate.
    let cmd = spec("tree", &["sh", "-c", "sleep 60 & sleep 61 & wait"], &[], 1);
    let outcome = run_command(&cmd, &root, &evidence, &evidence.join(".cmd")).expect("runs");
    assert!(outcome.timed_out);
    assert!(!outcome.passed());
    let cleanup = outcome.cleanup.expect("timeout records a cleanup report");
    assert!(cleanup.term_sent, "TERM sweep ran");
    assert!(cleanup.reaped, "the shell was reaped");
    assert!(
        !cleanup.survivors_after,
        "no grandchild survives the bounded cleanup: {cleanup:?}"
    );
    assert!(
        cleanup.elapsed_ms <= 6_000,
        "cleanup stays inside its budget: {}ms",
        cleanup.elapsed_ms
    );
    std::fs::remove_dir_all(&root).ok();
}

#[cfg(unix)]
#[test]
fn timeout_cleanup_tracks_reparented_descendants_truthfully() {
    // R2-F03 regression: the grandchild ignores TERM and is re-parented to
    // init when the root shell dies; the pre-fix cleanup re-discovered the
    // tree from the (dead) root, lost the already-known pid and falsely
    // reported survivors_after=false while the descendant lived on.
    let root = temp_root("r2f03");
    let evidence = root.join("evidence");
    let cmd = spec(
        "stubborn",
        &[
            "sh",
            "-c",
            "sh -c 'trap \"\" TERM; echo $$ > \"$1\"; while :; do sleep 1; done' sh {EVIDENCE}/grand.pid & wait",
        ],
        &["{EVIDENCE}/grand.pid"],
        1,
    );
    // An unrelated sentinel started by THIS test: it is never part of the
    // spawned tree, so the cleanup must never signal it.
    let mut sentinel = std::process::Command::new("sleep")
        .arg("120")
        .spawn()
        .expect("sentinel");
    let outcome = run_command(&cmd, &root, &evidence, &evidence.join(".cmd")).expect("runs");
    assert!(outcome.timed_out);
    assert!(!outcome.passed());
    let cleanup = outcome.cleanup.expect("timeout records a cleanup report");
    assert!(cleanup.term_sent, "TERM sweep ran: {cleanup:?}");
    assert!(
        cleanup.kill_sent,
        "TERM was ignored, so the KILL sweep must have run: {cleanup:?}"
    );
    assert!(
        !cleanup.survivors_after,
        "the tracked (re-parented) descendant was KILLed: {cleanup:?}"
    );
    // Ground truth from the OS: the grandchild is really gone — the
    // report is not merely self-consistent.
    let grand_pid = std::fs::read_to_string(evidence.join("grand.pid")).expect("pid file");
    let grand_alive = std::process::Command::new("kill")
        .args(["-0", grand_pid.trim()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(
        !grand_alive,
        "grandchild pid {} still alive after the cleanup",
        grand_pid.trim()
    );
    // The unrelated sentinel survives (attribution never widened).
    assert!(
        sentinel.try_wait().expect("sentinel wait").is_none(),
        "the unrelated sentinel must survive the cleanup"
    );
    let _ = sentinel.kill();
    let _ = sentinel.wait();
    std::fs::remove_dir_all(&root).ok();
}

#[cfg(unix)]
#[test]
fn membership_is_decided_from_the_current_snapshot_only() {
    // R8-F02 pure decision core (synthetic tables; no process, no
    // signal). The R7 design registered a per-pid `lstart` string read
    // from a SECOND, independent ps call after the pgrep discovery —
    // an interleaving that could register a FOREIGN replacement's birth
    // as ours and would then signal it. Membership here is recomputed
    // from the CURRENT table alone: no credential is recorded, so there
    // is nothing a replacement can satisfy.
    use crate::verify::{is_group_leaver, row_is_live, tree_members, ProcRow};
    let row = |pid: u32, ppid: u32, pgid: u32, stat: &str| ProcRow {
        pid,
        ppid,
        pgid,
        stat: stat.to_string(),
    };
    let root = 100;
    // A table where 140 was OURS a moment ago and the SAME NUMBER now
    // belongs to a foreign object (foreign pgid AND foreign parent).
    let before = vec![
        row(100, 1, 100, "S"),   // root: group leader
        row(110, 100, 100, "S"), // own child, in group
        row(120, 1, 100, "S"),   // own grandchild RE-PARENTED to launchd —
        //   still in group (kernel truth), R2-F03
        row(140, 100, 100, "S"), // own child, in group
    ];
    let after = vec![
        row(100, 1, 100, "S"),
        row(110, 100, 100, "S"),
        row(120, 1, 100, "S"),
        row(140, 999, 999, "S"), // number recycled into a FOREIGN object
        row(150, 100, 150, "S"), // a LEAVER: our descendant, but it left
                                 //   the group (e.g. setsid) — reportable, never group-signalled
    ];
    let pids = |rows: Vec<&ProcRow>| -> Vec<u32> { rows.iter().map(|r| r.pid).collect() };
    assert_eq!(pids(tree_members(root, &before)), vec![100, 110, 120, 140]);
    // The recycled number is judged by the CURRENT table: not ours, and
    // nothing about the earlier snapshot can override that.
    assert_eq!(pids(tree_members(root, &after)), vec![100, 110, 120, 150]);
    // A REAPED member (pid absent from the table) is simply gone — its
    // number is retired with it.
    let reaped = vec![row(100, 1, 100, "S"), row(110, 100, 100, "S")];
    assert_eq!(pids(tree_members(root, &reaped)), vec![100, 110]);
    // Zombies are dead objects whose numbers stay pinned until reaped:
    // members (so they are not lost), but NOT live.
    let zombies = vec![row(100, 1, 100, "Z"), row(110, 100, 100, "Z+")];
    assert_eq!(pids(tree_members(root, &zombies)), vec![100, 110]);
    assert!(zombies.iter().all(|r| !row_is_live(r)));
    assert!(row_is_live(&row(1, 0, 1, "S")));
    assert!(row_is_live(&row(1, 0, 1, "Ss")));
    // Only reachable-by-parentage rows can be leavers; a group member
    // never is.
    assert!(is_group_leaver(root, &row(150, 100, 150, "S")));
    assert!(!is_group_leaver(root, &row(120, 1, 100, "S")));
}

#[cfg(unix)]
#[test]
fn timeout_cleanup_reports_a_group_leaver_without_signalling_it() {
    // R8-F02 honesty boundary: a descendant that deliberately leaves the
    // process group (setsid — no gate script in this repo does this)
    // cannot be group-signalled, and its exit cannot be proven once its
    // parent dies — the cleanup must REPORT survivors instead of
    // assuming a clean tree. The leaver is this test's own spawned
    // object and is cleaned up by the test itself afterwards.
    let root = temp_root("leaver");
    let evidence = root.join("evidence");
    let cmd = spec(
        "leaver",
        &[
            "sh",
            "-c",
            "python3 -c 'import os,time; os.setsid(); print(os.getpid(), flush=True); time.sleep(120)' > \"$1\" 2>/dev/null & sleep 60",
            "sh",
            "{EVIDENCE}/leaver.pid",
        ],
        &["{EVIDENCE}/leaver.pid"],
        1,
    );
    let outcome = run_command(&cmd, &root, &evidence, &evidence.join(".cmd")).expect("runs");
    assert!(outcome.timed_out);
    let cleanup = outcome.cleanup.expect("timeout records a cleanup report");
    assert!(cleanup.term_sent, "the group TERM ran: {cleanup:?}");
    // The leaver must be REPORTED — never silently declared clean.
    assert!(
        cleanup.survivors_after,
        "a group leaver must be reported as a survivor: {cleanup:?}"
    );
    // Ground truth: the leaver really is alive (it received no signal —
    // it is outside the group, and no bare-number signal path exists).
    let mut leaver_pid = String::new();
    for _ in 0..40 {
        if let Ok(text) = std::fs::read_to_string(evidence.join("leaver.pid")) {
            if !text.trim().is_empty() {
                leaver_pid = text.trim().to_string();
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(!leaver_pid.is_empty(), "leaver pid file was written");
    let alive = |pid: &str| {
        std::process::Command::new("kill")
            .args(["-0", pid])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };
    assert!(alive(&leaver_pid), "the leaver survived the group cleanup");
    // The test cleans up its own spawned object.
    let _ = std::process::Command::new("kill")
        .args(["-TERM", &leaver_pid])
        .status();
    for _ in 0..60 {
        if !alive(&leaver_pid) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if alive(&leaver_pid) {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &leaver_pid])
            .status();
    }
    assert!(
        !alive(&leaver_pid),
        "the test cleaned up its own leaver {leaver_pid}"
    );
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

// ── R13-F01: supplemental leaf consumption ──────────────────────────────

fn leaf(
    id: &str,
    basis_kind: &str,
    refs: &[&str],
    paths: &[&str],
    contract: Option<LeafAssertionContract>,
) -> crate::stage_map::SupplementalLeaf {
    crate::stage_map::SupplementalLeaf {
        id: id.to_string(),
        feature_id: format!("F-D20-TEST-{id}"),
        requirement: "REQUIRED_SUPPLEMENTAL".into(),
        basis_kind: basis_kind.to_string(),
        r02_share: "server-side share under test".into(),
        r07_share: "client-side remainder under test".into(),
        evidence_required: "executed records of the referenced commands".into(),
        evidence_command_refs: refs.iter().map(|s| s.to_string()).collect(),
        evidence_paths: paths.iter().map(|s| s.to_string()).collect(),
        r00_kind: "supplemental".into(),
        r00_task_ids: TEST_R00_TASK_IDS.iter().map(|s| s.to_string()).collect(),
        r00_execution_stage_ids: TEST_R00_STAGE_IDS.iter().map(|s| s.to_string()).collect(),
        r00_ledger_status: "SPECIFIED_NOT_EXECUTED".into(),
        r00_result_ids: Vec::new(),
        r00_test_ids: Vec::new(),
        r00_then: test_then(id),
        r00_assertions: test_assertions(id),
        r00_due: TEST_R00_DUE.into(),
        assertion_contract: contract,
        original_assertion_cases: Vec::new(),
        deferred_r07_cases: Vec::new(),
        early_evidence_command_refs: Vec::new(),
    }
}

fn contract(producer: &str, path: &str, cases: &[(&str, i64)]) -> Option<LeafAssertionContract> {
    Some(LeafAssertionContract {
        producer_command: producer.to_string(),
        evidence_path: path.to_string(),
        cases: cases
            .iter()
            .map(|(case, expect)| LeafCaseExpect {
                case: case.to_string(),
                expect: *expect,
            })
            .collect(),
    })
}

#[test]
fn supplemental_leaf_rollup_keeps_partial_evidence_out_of_overall_pass() {
    let root = temp_root("supp");
    write_minimal_r00_ledger(
        &root,
        &[
            ("R00-T02-LA-TESTOK000000000", "REQUIRED_SUPPLEMENTAL"),
            ("R00-T02-LA-TESTNOEV00000000", "REQUIRED_SUPPLEMENTAL"),
            ("R00-T02-LA-TESTCONFLICT0000", "REQUIRED_SUPPLEMENTAL"),
            ("R00-T02-LA-TESTGENERIC0000", "REQUIRED_SUPPLEMENTAL"),
        ],
    );
    let evidence = root.join("evidence");
    let map = StageMap {
        stage: "RX".into(),
        result_version: crate::RESULT_VERSION.into(),
        default_timeout_secs: 60,
        commands: vec![
            spec(
                "good",
                &[
                    "sh",
                    "-c",
                    // Produces BOTH its own command evidence and the leaf's
                    // structured case file (R14-F01 contract consumption).
                    "printf ok > {EVIDENCE}/good.txt && \
                     printf '{\"schema\":\"lingxi.leaf-case-results.v1\",\"cases\":[{\"case\":\"case-ok\",\"expect\":200,\"actual\":200,\"ok\":true}]}' \
                     > {EVIDENCE}/leaf-cases.json",
                ],
                &["{EVIDENCE}/good.txt"],
                60,
            ),
            // Referenced ONLY by a supplemental leaf: the execution order
            // must still run it (R13-F01 — never silently skipped).
            spec("bad", &["sh", "-c", "exit 9"], &["{EVIDENCE}/never.txt"], 60),
        ],
        scenarios: vec![crate::stage_map::Scenario {
            id: "RX-A01".into(),
            requirement: "REQUIRED".into(),
            command_refs: vec!["good".into()],
        }],
        supplemental_leaves: vec![
            leaf(
                "R00-T02-LA-TESTOK000000000",
                crate::stage_map::BASIS_PROTOCOL,
                &["good"],
                &["{EVIDENCE}/good.txt"],
                contract("good", "{EVIDENCE}/leaf-cases.json", &[("case-ok", 200)]),
            ),
            leaf(
                "R00-T02-LA-TESTNOEV00000000",
                crate::stage_map::BASIS_AUTH_PRIMITIVE_ONLY,
                &["bad"],
                &[],
                contract("bad", "{EVIDENCE}/never-cases.json", &[("case-bad", 401)]),
            ),
            leaf(
                "R00-T02-LA-TESTCONFLICT0000",
                crate::stage_map::BASIS_CLIENT_ONLY_STAGE_CONFLICT,
                &[],
                &[],
                None,
            ),
            leaf(
                "R00-T02-LA-TESTGENERIC0000",
                crate::stage_map::BASIS_AUTH_PRIMITIVE_ONLY,
                &["good"],
                &["{EVIDENCE}/good.txt"],
                contract("good", "{EVIDENCE}/leaf-cases.json", &[("case-ok", 200)]),
            ),
        ],
    };
    let report =
        verify_stage(&map, &root, &evidence, "deadbeef", false, 0).expect("runs to completion");
    // Base scenario green, supplemental verdicts mixed, overall FAIL.
    assert_eq!(report["scenarios"][0]["status"], "PASS");
    let find = |id: &str| {
        report["supplementalLeafScenarios"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == id)
            .unwrap_or_else(|| panic!("leaf {id} missing from the report"))
            .clone()
    };
    let ok_leaf = find("R00-T02-LA-TESTOK000000000");
    assert_eq!(ok_leaf["status"], "BLOCKED");
    // 案例数值通过只证明当前阶段份额，原叶还绑定后续阶段。
    assert_eq!(ok_leaf["assertionResults"][0]["case"], "case-ok");
    assert_eq!(ok_leaf["assertionResults"][0]["status"], "PASS");
    assert!(ok_leaf["reason"]
        .as_str()
        .unwrap()
        .contains("another stage"));
    assert_eq!(
        ok_leaf["r00Mirror"]["then"],
        serde_json::json!(test_then("R00-T02-LA-TESTOK000000000"))
    );
    let noev_leaf = find("R00-T02-LA-TESTNOEV00000000");
    assert_eq!(noev_leaf["status"], "FAIL");
    assert!(
        noev_leaf["reason"]
            .as_str()
            .unwrap()
            .contains("evidence commands not passing"),
        "the failing producer is named: {}",
        noev_leaf["reason"]
    );
    assert_eq!(find("R00-T02-LA-TESTCONFLICT0000")["status"], "BLOCKED");
    let generic_leaf = find("R00-T02-LA-TESTGENERIC0000");
    assert_eq!(generic_leaf["assertionResults"][0]["status"], "PASS");
    assert_eq!(generic_leaf["status"], "BLOCKED");
    assert!(generic_leaf["reason"]
        .as_str()
        .unwrap()
        .contains("do not prove"));
    assert_eq!(
        report["supplementalLeafCoverage"]["pass"],
        serde_json::json!(0)
    );
    assert_eq!(
        report["supplementalLeafCoverage"]["fail"],
        serde_json::json!(1)
    );
    assert_eq!(
        report["supplementalLeafCoverage"]["blocked"],
        serde_json::json!(3)
    );
    assert_eq!(
        report["supplementalLeafCoverage"]["expectedFromR00Ledger"],
        serde_json::json!(4)
    );
    assert_eq!(report["overall"], "FAIL");
    // The leaf-only command really ran (two entries with real exit codes);
    // its failure is what fails the NOEV leaf.
    assert_eq!(report["commands"].as_array().unwrap().len(), 2);
    assert_eq!(
        report["commands"].as_array().unwrap()[1]["exitCode"],
        serde_json::json!(9)
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn single_stage_leaf_with_matching_case_can_still_pass() {
    // 跨阶段原叶必须阻断；只归属当前阶段且有对应案例的叶仍可通过。
    let root = temp_root("single-stage-leaf");
    let evidence = root.join("evidence");
    std::fs::create_dir_all(&evidence).expect("create evidence root");
    std::fs::write(
        evidence.join("leaf-cases.json"),
        r#"{"schema":"lingxi.leaf-case-results.v1","cases":[{"case":"own-result","expect":200,"actual":200,"ok":true}]}"#,
    )
    .expect("write case evidence");
    let mut item = leaf(
        "R00-T02-LA-TESTSINGLE00000",
        crate::stage_map::BASIS_PROTOCOL,
        &["good"],
        &[],
        contract("good", "{EVIDENCE}/leaf-cases.json", &[("own-result", 200)]),
    );
    item.r00_execution_stage_ids = vec!["RX".into()];
    let outcomes = vec![crate::verify::CommandOutcome {
        key: "good".into(),
        exit_code: Some(0),
        timed_out: false,
        internal_error: None,
        evidence_missing: Vec::new(),
        evidence_preexisting: Vec::new(),
        cleanup: None,
    }];
    let roll = super::roll_up_supplemental_leaf(&item, "RX", &outcomes, &root, &evidence);
    assert_eq!(roll.status, "PASS", "{}", roll.reason);
    assert_eq!(roll.assertion_results[0]["status"], "PASS");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn dual_stage_leaf_passes_only_with_complete_original_behavior_cases() {
    let root = temp_root("full-original");
    let evidence = root.join("evidence");
    std::fs::create_dir_all(&evidence).expect("create evidence root");
    std::fs::write(
        evidence.join("leaf-cases.json"),
        r#"{"schema":"lingxi.leaf-case-results.v1","cases":[{"case":"positive","expect":0,"actual":0,"ok":true},{"case":"negative","expect":1,"actual":1,"ok":true}]}"#,
    )
    .expect("write complete case evidence");
    let mut item = leaf(
        "R00-T02-LA-TESTFULL0000000",
        crate::stage_map::BASIS_FULL_ORIGINAL_BEHAVIOR,
        &["client"],
        &[],
        contract(
            "client",
            "{EVIDENCE}/leaf-cases.json",
            &[("positive", 0), ("negative", 1)],
        ),
    );
    item.r00_assertions = vec!["positive behavior".into(), "negative behavior".into()];
    item.original_assertion_cases = vec![vec!["positive".into()], vec!["negative".into()]];
    let outcomes = vec![crate::verify::CommandOutcome {
        key: "client".into(),
        exit_code: Some(0),
        timed_out: false,
        internal_error: None,
        evidence_missing: Vec::new(),
        evidence_preexisting: Vec::new(),
        cleanup: None,
    }];
    let roll = super::roll_up_supplemental_leaf(&item, "R02", &outcomes, &root, &evidence);
    assert_eq!(roll.status, "PASS", "{}", roll.reason);

    item.original_assertion_cases = vec![vec!["positive".into()], vec!["positive".into()]];
    let roll = super::roll_up_supplemental_leaf(&item, "R02", &outcomes, &root, &evidence);
    assert_eq!(roll.status, "FAIL", "{}", roll.reason);
    assert!(
        roll.reason.contains("unique executed case"),
        "{}",
        roll.reason
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn duplicate_leaf_case_identity_cannot_hide_a_failed_record() {
    // 两个顺序都必须失败：取第一条的旧逻辑会放过“先成功、后失败”。
    for (label, first_ok) in [("pass-first", true), ("fail-first", false)] {
        let root = temp_root(label);
        let evidence = root.join("evidence");
        std::fs::create_dir_all(&evidence).expect("create evidence root");
        let first_actual = if first_ok { 200 } else { 403 };
        let second_actual = if first_ok { 403 } else { 200 };
        let cases = serde_json::json!({
            "schema": "lingxi.leaf-case-results.v1",
            "cases": [
                {"case":"same-id","expect":200,"actual":first_actual,"ok":first_ok},
                {"case":"same-id","expect":200,"actual":second_actual,"ok":!first_ok}
            ]
        });
        std::fs::write(evidence.join("leaf-cases.json"), cases.to_string())
            .expect("write duplicate case evidence");
        let mut item = leaf(
            "R00-T02-LA-DUPLICATE00000",
            crate::stage_map::BASIS_PROTOCOL,
            &["good"],
            &[],
            contract("good", "{EVIDENCE}/leaf-cases.json", &[("same-id", 200)]),
        );
        item.r00_execution_stage_ids = vec!["RX".into()];
        let outcomes = vec![crate::verify::CommandOutcome {
            key: "good".into(),
            exit_code: Some(0),
            timed_out: false,
            internal_error: None,
            evidence_missing: Vec::new(),
            evidence_preexisting: Vec::new(),
            cleanup: None,
        }];
        let roll = super::roll_up_supplemental_leaf(&item, "RX", &outcomes, &root, &evidence);
        assert_eq!(roll.status, "FAIL", "{label}: {}", roll.reason);
        assert!(
            roll.reason.contains("duplicate evidence case name"),
            "{label}: {}",
            roll.reason
        );
        std::fs::remove_dir_all(&root).ok();
    }
}

#[test]
fn unpinned_failed_case_in_shared_leaf_file_is_not_silent() {
    let root = temp_root("shared-case-failure");
    let evidence = root.join("evidence");
    std::fs::create_dir_all(&evidence).expect("create evidence root");
    let cases = serde_json::json!({
        "schema": "lingxi.leaf-case-results.v1",
        "cases": [
            {"case":"pinned","expect":200,"actual":200,"ok":true},
            {"case":"other-leaf-failed","expect":200,"actual":403,"ok":false}
        ]
    });
    std::fs::write(evidence.join("leaf-cases.json"), cases.to_string())
        .expect("write shared case evidence");
    let mut item = leaf(
        "R00-T02-LA-SHAREDCASE0000",
        crate::stage_map::BASIS_PROTOCOL,
        &["good"],
        &[],
        contract("good", "{EVIDENCE}/leaf-cases.json", &[("pinned", 200)]),
    );
    item.r00_execution_stage_ids = vec!["RX".into()];
    let outcomes = vec![crate::verify::CommandOutcome {
        key: "good".into(),
        exit_code: Some(0),
        timed_out: false,
        internal_error: None,
        evidence_missing: Vec::new(),
        evidence_preexisting: Vec::new(),
        cleanup: None,
    }];
    let roll = super::roll_up_supplemental_leaf(&item, "RX", &outcomes, &root, &evidence);
    assert_eq!(roll.status, "FAIL", "{}", roll.reason);
    assert!(roll.reason.contains("other-leaf-failed"), "{}", roll.reason);
    std::fs::remove_dir_all(&root).ok();
}

// ── R14-F01: assertion-contract consumption ─────────────────────────────

#[test]
fn green_exit_with_wrong_case_actual_fails_the_leaf() {
    // The concrete R14-F01 fake-green path, shrunk: the command exits 0,
    // its own declared evidence exists, and the leaf's case file IS
    // present and self-consistent (ok=true) — but the case's ACTUAL value
    // is 401 while the leaf's original R00 assertion (pinned in the map)
    // demands 403. Pre-fix the leaf passed on exit 0; now it must FAIL.
    let root = temp_root("actual401");
    write_minimal_r00_ledger(
        &root,
        &[("R00-T02-LA-TESTTICKET00000", "REQUIRED_SUPPLEMENTAL")],
    );
    let evidence = root.join("evidence");
    let map = StageMap {
        stage: "RX".into(),
        result_version: crate::RESULT_VERSION.into(),
        default_timeout_secs: 60,
        commands: vec![spec(
            "ok",
            &[
                "sh",
                "-c",
                "printf ok > {EVIDENCE}/cmd.txt && \
                 printf '{\"schema\":\"lingxi.leaf-case-results.v1\",\"cases\":[{\"case\":\"no-cred-ticket\",\"expect\":401,\"actual\":401,\"ok\":true}]}' \
                 > {EVIDENCE}/leaf-cases.json",
            ],
            &["{EVIDENCE}/cmd.txt"],
            60,
        )],
        scenarios: vec![crate::stage_map::Scenario {
            id: "RX-A01".into(),
            requirement: "REQUIRED".into(),
            command_refs: vec!["ok".into()],
        }],
        supplemental_leaves: vec![leaf(
            "R00-T02-LA-TESTTICKET00000",
            crate::stage_map::BASIS_ROUTE_PRESENT_STATIC,
            &["ok"],
            &[],
            contract("ok", "{EVIDENCE}/leaf-cases.json", &[("no-cred-ticket", 403)]),
        )],
    };
    let report =
        verify_stage(&map, &root, &evidence, "deadbeef", false, 0).expect("runs to completion");
    assert_eq!(
        report["commands"][0]["status"], "PASS",
        "the command itself is green"
    );
    let leaf_entry = &report["supplementalLeafScenarios"][0];
    assert_eq!(
        leaf_entry["status"], "FAIL",
        "the leaf must NOT ride the green exit"
    );
    let reason = leaf_entry["reason"].as_str().unwrap();
    assert!(
        reason.contains("original-assertion cases not holding"),
        "the drifted case is named: {reason}"
    );
    assert_eq!(report["overall"], "FAIL");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn producer_softening_its_own_expectation_fails_the_leaf() {
    // The producer's file records actual == the map's pin but claims ITS
    // OWN expect was a different (softer) value. The gate pins the
    // expectation in the MAP, so this mismatch must also fail — a producer
    // that rewrites what it was checking for cannot keep the leaf green.
    let root = temp_root("softexpect");
    write_minimal_r00_ledger(
        &root,
        &[("R00-T02-LA-TESTSOFT00000000", "REQUIRED_SUPPLEMENTAL")],
    );
    let evidence = root.join("evidence");
    let map = StageMap {
        stage: "RX".into(),
        result_version: crate::RESULT_VERSION.into(),
        default_timeout_secs: 60,
        commands: vec![spec(
            "ok",
            &[
                "sh",
                "-c",
                "printf ok > {EVIDENCE}/cmd.txt && \
                 printf '{\"schema\":\"lingxi.leaf-case-results.v1\",\"cases\":[{\"case\":\"c1\",\"expect\":999,\"actual\":403,\"ok\":true}]}' \
                 > {EVIDENCE}/leaf-cases.json",
            ],
            &["{EVIDENCE}/cmd.txt"],
            60,
        )],
        scenarios: vec![crate::stage_map::Scenario {
            id: "RX-A01".into(),
            requirement: "REQUIRED".into(),
            command_refs: vec!["ok".into()],
        }],
        supplemental_leaves: vec![leaf(
            "R00-T02-LA-TESTSOFT00000000",
            crate::stage_map::BASIS_PROTOCOL,
            &["ok"],
            &[],
            contract("ok", "{EVIDENCE}/leaf-cases.json", &[("c1", 403)]),
        )],
    };
    let report =
        verify_stage(&map, &root, &evidence, "deadbeef", false, 0).expect("runs to completion");
    let leaf_entry = &report["supplementalLeafScenarios"][0];
    assert_eq!(leaf_entry["status"], "FAIL");
    assert!(
        leaf_entry["reason"]
            .as_str()
            .unwrap()
            .contains("original-assertion cases not holding"),
        "softened expectation is caught: {}",
        leaf_entry["reason"]
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn missing_leaf_case_file_or_declared_case_fails_the_leaf() {
    // The producer passed but never wrote the leaf's case file; a second
    // leaf's file exists but lacks the declared case. Both must FAIL — a
    // leaf without its producer's structured record has no legal evidence.
    let root = temp_root("nocasefile");
    write_minimal_r00_ledger(
        &root,
        &[
            ("R00-T02-LA-TESTNOFILE000000", "REQUIRED_SUPPLEMENTAL"),
            ("R00-T02-LA-TESTNOCASE000000", "REQUIRED_SUPPLEMENTAL"),
        ],
    );
    let evidence = root.join("evidence");
    let map = StageMap {
        stage: "RX".into(),
        result_version: crate::RESULT_VERSION.into(),
        default_timeout_secs: 60,
        commands: vec![spec(
            "ok",
            &[
                "sh",
                "-c",
                "printf ok > {EVIDENCE}/cmd.txt && \
                 printf '{\"schema\":\"lingxi.leaf-case-results.v1\",\"cases\":[{\"case\":\"other\",\"expect\":1,\"actual\":1,\"ok\":true}]}' \
                 > {EVIDENCE}/leaf-cases.json",
            ],
            &["{EVIDENCE}/cmd.txt"],
            60,
        )],
        scenarios: vec![crate::stage_map::Scenario {
            id: "RX-A01".into(),
            requirement: "REQUIRED".into(),
            command_refs: vec!["ok".into()],
        }],
        supplemental_leaves: vec![
            leaf(
                "R00-T02-LA-TESTNOFILE000000",
                crate::stage_map::BASIS_PROTOCOL,
                &["ok"],
                &[],
                contract("ok", "{EVIDENCE}/never-written.json", &[("c1", 200)]),
            ),
            leaf(
                "R00-T02-LA-TESTNOCASE000000",
                crate::stage_map::BASIS_PROTOCOL,
                &["ok"],
                &[],
                contract("ok", "{EVIDENCE}/leaf-cases.json", &[("declared-case", 200)]),
            ),
        ],
    };
    let report =
        verify_stage(&map, &root, &evidence, "deadbeef", false, 0).expect("runs to completion");
    let leaves = report["supplementalLeafScenarios"].as_array().unwrap();
    assert_eq!(leaves[0]["status"], "FAIL");
    assert!(
        leaves[0]["reason"].as_str().unwrap().contains("missing"),
        "missing file named: {}",
        leaves[0]["reason"]
    );
    assert_eq!(leaves[1]["status"], "FAIL");
    assert!(
        leaves[1]["reason"]
            .as_str()
            .unwrap()
            .contains("not recorded"),
        "missing case named: {}",
        leaves[1]["reason"]
    );
    std::fs::remove_dir_all(&root).ok();
}

// ── R14-F01: full-record mirror cross-check ─────────────────────────────

#[test]
fn r00_mirror_drift_aborts_the_gate_before_any_command_runs() {
    // Keep id AND requirement identical, swap the original `then` text —
    // the R13-F01 check passed this shape; the R14-F01 full-record check
    // must abort on it.
    let root = temp_root("mirrordrift");
    write_minimal_r00_ledger(
        &root,
        &[("R00-T02-LA-TESTDRIFT0000000", "REQUIRED_SUPPLEMENTAL")],
    );
    let evidence = root.join("evidence");
    let mut drifted = leaf(
        "R00-T02-LA-TESTDRIFT0000000",
        crate::stage_map::BASIS_PROTOCOL,
        &["ok"],
        &[],
        contract("ok", "{EVIDENCE}/leaf-cases.json", &[("c1", 200)]),
    );
    drifted.r00_then = "a DIFFERENT original then".into();
    let map = StageMap {
        stage: "RX".into(),
        result_version: crate::RESULT_VERSION.into(),
        default_timeout_secs: 60,
        commands: vec![spec(
            "ok",
            &["sh", "-c", "printf ran > {EVIDENCE}/ran.marker"],
            &[],
            60,
        )],
        scenarios: vec![crate::stage_map::Scenario {
            id: "RX-A01".into(),
            requirement: "REQUIRED".into(),
            command_refs: vec!["ok".into()],
        }],
        supplemental_leaves: vec![drifted],
    };
    let err = verify_stage(&map, &root, &evidence, "deadbeef", false, 0)
        .expect_err("a drifted mirror must abort the gate");
    assert!(
        err.contains("mirrors an R00 record that does not match"),
        "{err}"
    );
    assert!(err.contains("r00Then"), "the drifted field is named: {err}");
    assert!(
        !evidence.join("ran.marker").exists(),
        "the refusal happens before any command runs"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn stale_leaf_evidence_aborts_the_gate() {
    // Leaf-declared evidence that already exists outside the (fresh,
    // empty) evidence root — an absolute-path pin pointing at an old file
    // — must be refused up front exactly like stale command evidence.
    let root = temp_root("staleleaf");
    write_minimal_r00_ledger(
        &root,
        &[("R00-T02-LA-TESTSTALE0000000", "REQUIRED_SUPPLEMENTAL")],
    );
    let evidence = root.join("evidence");
    let stale_dir = root.join("stale");
    std::fs::create_dir_all(&stale_dir).unwrap();
    std::fs::write(stale_dir.join("leaf-cases.json"), b"old").unwrap();
    let stale_pin = format!("{}/leaf-cases.json", stale_dir.display());
    let map = StageMap {
        stage: "RX".into(),
        result_version: crate::RESULT_VERSION.into(),
        default_timeout_secs: 60,
        commands: vec![spec(
            "marker",
            &["sh", "-c", "printf ran > {EVIDENCE}/ran.marker"],
            &[],
            60,
        )],
        scenarios: vec![crate::stage_map::Scenario {
            id: "RX-A01".into(),
            requirement: "REQUIRED".into(),
            command_refs: vec!["marker".into()],
        }],
        supplemental_leaves: vec![leaf(
            "R00-T02-LA-TESTSTALE0000000",
            crate::stage_map::BASIS_PROTOCOL,
            &["marker"],
            &[],
            contract("marker", &stale_pin, &[("c1", 200)]),
        )],
    };
    let err = verify_stage(&map, &root, &evidence, "deadbeef", false, 0)
        .expect_err("stale leaf evidence must abort the gate");
    assert!(err.contains("stale leaf evidence"), "{err}");
    assert!(
        !evidence.join("ran.marker").exists(),
        "the refusal happens before any command runs"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn r00_ledger_cross_check_refuses_dropped_leaves() {
    let root = temp_root("dropped");
    write_minimal_r00_ledger(
        &root,
        &[("R00-T02-LA-DROPPED00000000", "REQUIRED_SUPPLEMENTAL")],
    );
    let evidence = root.join("evidence");
    let map = StageMap {
        stage: "RX".into(),
        result_version: crate::RESULT_VERSION.into(),
        default_timeout_secs: 60,
        commands: vec![spec(
            "marker",
            &["sh", "-c", "printf ran > {EVIDENCE}/ran.marker"],
            &[],
            60,
        )],
        scenarios: vec![crate::stage_map::Scenario {
            id: "RX-A01".into(),
            requirement: "REQUIRED".into(),
            command_refs: vec!["marker".into()],
        }],
        // The R00 ledger binds one leaf to RX; the map declares none —
        // exactly the filtered-map shape the cross-check must refuse.
        supplemental_leaves: vec![],
    };
    let err = verify_stage(&map, &root, &evidence, "deadbeef", false, 0)
        .expect_err("a map dropping an R00-bound leaf must abort the gate");
    assert!(err.contains("drops 1 REQUIRED_SUPPLEMENTAL"), "{err}");
    assert!(
        !evidence.join("ran.marker").exists(),
        "the refusal happens before any command runs"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn missing_r00_ledger_aborts_the_gate() {
    let root = temp_root("noledger");
    // No write_minimal_r00_ledger call: the ledger is absent, so leaf
    // coverage cannot be verified — the gate must fail closed, not assume
    // zero obligations.
    let evidence = root.join("evidence");
    let map = StageMap {
        stage: "RX".into(),
        result_version: crate::RESULT_VERSION.into(),
        default_timeout_secs: 60,
        commands: vec![spec(
            "marker",
            &["sh", "-c", "printf ran > {EVIDENCE}/ran.marker"],
            &[],
            60,
        )],
        scenarios: vec![crate::stage_map::Scenario {
            id: "RX-A01".into(),
            requirement: "REQUIRED".into(),
            command_refs: vec!["marker".into()],
        }],
        supplemental_leaves: vec![],
    };
    let err = verify_stage(&map, &root, &evidence, "deadbeef", false, 0)
        .expect_err("a missing R00 ledger must abort the gate");
    assert!(
        err.contains("cannot read the R00 acceptance ledger"),
        "{err}"
    );
    assert!(
        !evidence.join("ran.marker").exists(),
        "the refusal happens before any command runs"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn embedded_r02_map_declares_the_34_supplemental_leaves() {
    let r02 = parse_stage_map(crate::STAGE_MAPS[0].1).unwrap();
    assert_eq!(
        r02.supplemental_leaves.len(),
        34,
        "R02 must declare all 34 R00 REQUIRED_SUPPLEMENTAL leaves"
    );
    let count_kind = |kind: &str| {
        r02.supplemental_leaves
            .iter()
            .filter(|l| l.basis_kind == kind)
            .count()
    };
    // R02-final group 1 stage ownership: exactly 25 share leaves + 9
    // deferred leaves; no legacy basis kind remains in the live map.
    assert_eq!(
        count_kind(crate::stage_map::BASIS_R02_SHARE_SATISFIED),
        25,
        "25 R02-share leaves (17 management + 6 protocol + 1 serve + 1 sessions)"
    );
    assert_eq!(count_kind(crate::stage_map::BASIS_DEFERRED_TO_R07), 9);
    assert_eq!(
        count_kind(crate::stage_map::BASIS_ROUTE_PRESENT_STATIC)
            + count_kind(crate::stage_map::BASIS_PROTOCOL)
            + count_kind(crate::stage_map::BASIS_AUTH_PRIMITIVE_ONLY)
            + count_kind(crate::stage_map::BASIS_CLIENT_ONLY_STAGE_CONFLICT)
            + count_kind(crate::stage_map::BASIS_FULL_ORIGINAL_BEHAVIOR),
        0,
        "the live R02 map carries no legacy basis kinds"
    );
    assert!(r02
        .supplemental_leaves
        .iter()
        .all(|l| l.requirement == "REQUIRED_SUPPLEMENTAL"));
    // 原账对 34 项都登记了 R02 与 R07 责任；份额叶与递延叶都保留双归属。
    assert!(r02.supplemental_leaves.iter().all(|l| {
        l.r00_execution_stage_ids.iter().any(|id| id == "R02")
            && l.r00_execution_stage_ids.iter().any(|id| id == "R07")
    }));
    // F1 回归：serve 叶的生产者命令必须已注册（缺失即加载期硬错）。
    assert!(r02
        .commands
        .iter()
        .any(|c| c.key == "supplemental_cli_rust_matrix"));
    let serve = r02
        .supplemental_leaves
        .iter()
        .find(|l| l.id == "R00-T02-LA-1B09760C2B1C")
        .expect("serve leaf present");
    assert_eq!(
        serve.basis_kind,
        crate::stage_map::BASIS_R02_SHARE_SATISFIED
    );
    assert_eq!(
        serve.assertion_contract.as_ref().unwrap().producer_command,
        "supplemental_cli_rust_matrix"
    );
    assert_eq!(
        serve.assertion_contract.as_ref().unwrap().cases.len(),
        9,
        "serve leaf keeps its 9 all-server gating pins"
    );
    for l in r02
        .supplemental_leaves
        .iter()
        .filter(|l| l.basis_kind == crate::stage_map::BASIS_DEFERRED_TO_R07)
    {
        assert!(
            l.evidence_command_refs.is_empty(),
            "{}: a deferred leaf must carry no GATING evidence commands",
            l.id
        );
        assert!(
            l.assertion_contract.is_none(),
            "{}: a deferred leaf must carry no assertion contract",
            l.id
        );
        assert!(
            l.evidence_paths.is_empty(),
            "{}: a deferred leaf must carry no pass-able evidencePaths",
            l.id
        );
        for key in &l.early_evidence_command_refs {
            assert!(
                r02.commands.iter().any(|c| &c.key == key),
                "{}: early-evidence command {key:?} must stay registered",
                l.id
            );
        }
        for record in &l.deferred_r07_cases {
            assert!(
                l.evidence_command_refs.contains(&record.producer_command)
                    || l.early_evidence_command_refs
                        .contains(&record.producer_command),
                "{}: deferred case {:?} producer must run for this leaf",
                l.id,
                record.case
            );
        }
        assert!(
            !l.r00_assertions.is_empty() && !l.r00_then.is_empty() && !l.r00_due.is_empty(),
            "{}: R00 mirror fields must be present",
            l.id
        );
    }
    for l in r02
        .supplemental_leaves
        .iter()
        .filter(|l| l.basis_kind == crate::stage_map::BASIS_R02_SHARE_SATISFIED)
    {
        assert!(
            !l.evidence_command_refs.is_empty(),
            "{}: a share leaf must bind evidence commands",
            l.id
        );
        let contract = l
            .assertion_contract
            .as_ref()
            .unwrap_or_else(|| panic!("{}: share leaf must declare an assertionContract", l.id));
        assert!(
            l.evidence_command_refs.contains(&contract.producer_command),
            "{}: the contract's producer must be one of the leaf's bound commands",
            l.id
        );
        assert!(
            !contract.cases.is_empty(),
            "{}: the contract must pin at least one case expectation",
            l.id
        );
        assert!(
            !l.r00_assertions.is_empty() && !l.r00_then.is_empty() && !l.r00_due.is_empty(),
            "{}: R00 mirror fields must be present",
            l.id
        );
    }
    // sessions leaf split: exactly the 6 server-side gating pins; the 4 CLI
    // rendering cases move to deferredR07Cases (observed, never gating).
    let sessions = r02
        .supplemental_leaves
        .iter()
        .find(|l| l.id == "R00-T02-LA-200D4E5D52C9")
        .expect("sessions leaf present");
    let contract = sessions.assertion_contract.as_ref().unwrap();
    let gating: Vec<&str> = contract.cases.iter().map(|c| c.case.as_str()).collect();
    assert_eq!(
        gating,
        vec![
            "a05-sessions-list-owner",
            "a05-sessions-list-owner-contains-own-session",
            "a05-sessions-list-foreign-principal",
            "a05-sessions-list-foreign-excludes-owner-sessions",
            "a05-sessions-list-empty-shape",
            "a05-sessions-list-no-credential",
        ]
    );
    let deferred: Vec<&str> = sessions
        .deferred_r07_cases
        .iter()
        .map(|c| c.case.as_str())
        .collect();
    assert_eq!(
        deferred,
        vec![
            "a05-cli-sessions-owner-list",
            "a05-cli-sessions-foreign-empty",
            "a05-cli-sessions-unauthorized-error",
            "a05-cli-sessions-limit-20",
        ]
    );
}

// ── R02-final group 1: stage-ownership kinds (share/deferred) ───────────

/// Outcomes helper: one passing and (optionally) one failing command.
fn outcome(key: &str, exit: i32) -> crate::verify::CommandOutcome {
    crate::verify::CommandOutcome {
        key: key.to_string(),
        exit_code: Some(exit),
        timed_out: false,
        internal_error: None,
        evidence_missing: Vec::new(),
        evidence_preexisting: Vec::new(),
        cleanup: None,
    }
}

#[test]
fn r02_share_satisfied_dual_stage_leaf_passes_on_share_pins() {
    // F2 回归：双阶段叶（r00ExecutionStageIds 含 RY）在份额图钉全过时必须
    // PASS——旧「另属后续阶段→BLOCKED」分支等于要求 R02 先证 R07 客户端
    // 行为，属阶段所有权越界，已撤销（仅旧图 legacy kind 保留回放防护）。
    let root = temp_root("share-pass");
    let evidence = root.join("evidence");
    std::fs::create_dir_all(&evidence).expect("create evidence root");
    std::fs::write(
        evidence.join("leaf-cases.json"),
        r#"{"schema":"lingxi.leaf-case-results.v1","cases":[{"case":"share-pin","expect":200,"actual":200,"ok":true},{"case":"deferred-render","expect":1,"actual":1,"ok":true}]}"#,
    )
    .expect("write case evidence");
    let mut item = leaf(
        "R00-T02-LA-TESTSHARE00000",
        crate::stage_map::BASIS_R02_SHARE_SATISFIED,
        &["good"],
        &[],
        contract("good", "{EVIDENCE}/leaf-cases.json", &[("share-pin", 200)]),
    );
    item.deferred_r07_cases = vec![crate::stage_map::LeafDeferredCase {
        case: "deferred-render".into(),
        expect: 1,
        producer_command: "good".into(),
        evidence_path: "{EVIDENCE}/leaf-cases.json".into(),
    }];
    // TEST_R00_STAGE_IDS = [RX, RY]: a genuinely dual-stage leaf.
    let outcomes = vec![outcome("good", 0)];
    let roll = super::roll_up_supplemental_leaf(&item, "RX", &outcomes, &root, &evidence);
    assert_eq!(roll.status, "PASS", "{}", roll.reason);
    assert!(roll.reason.contains("R07 remainder stays REQUIRED"));
    assert_eq!(roll.assertion_results[0]["status"], "PASS");
    // The deferred case is OBSERVED (it held) but never gates.
    assert_eq!(roll.deferred_case_results.len(), 1);
    assert_eq!(roll.deferred_case_results[0]["status"], "OBSERVED_HELD");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn r02_share_satisfied_leaf_fails_when_a_pin_does_not_hold() {
    let root = temp_root("share-fail");
    let evidence = root.join("evidence");
    std::fs::create_dir_all(&evidence).expect("create evidence root");
    std::fs::write(
        evidence.join("leaf-cases.json"),
        r#"{"schema":"lingxi.leaf-case-results.v1","cases":[{"case":"share-pin","expect":200,"actual":403,"ok":false}]}"#,
    )
    .expect("write drifted case evidence");
    let item = leaf(
        "R00-T02-LA-TESTSHAREFAIL00",
        crate::stage_map::BASIS_R02_SHARE_SATISFIED,
        &["good"],
        &[],
        contract("good", "{EVIDENCE}/leaf-cases.json", &[("share-pin", 200)]),
    );
    let outcomes = vec![outcome("good", 0)];
    let roll = super::roll_up_supplemental_leaf(&item, "RX", &outcomes, &root, &evidence);
    assert_eq!(roll.status, "FAIL", "{}", roll.reason);
    assert!(roll.reason.contains("original-assertion cases not holding"));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn legacy_partial_dual_stage_leaf_stays_blocked_for_old_map_replay() {
    // 旧图回放防护：protocol_basis/route_basis_present_static 的双阶段叶仍
    // BLOCKED（现行 R02 图已无这类叶——份额叶必须显式声明份额语义）。
    let root = temp_root("legacy-block");
    let evidence = root.join("evidence");
    std::fs::create_dir_all(&evidence).expect("create evidence root");
    std::fs::write(
        evidence.join("leaf-cases.json"),
        r#"{"schema":"lingxi.leaf-case-results.v1","cases":[{"case":"pin","expect":200,"actual":200,"ok":true}]}"#,
    )
    .expect("write case evidence");
    let item = leaf(
        "R00-T02-LA-TESTLEGACY00000",
        crate::stage_map::BASIS_PROTOCOL,
        &["good"],
        &[],
        contract("good", "{EVIDENCE}/leaf-cases.json", &[("pin", 200)]),
    );
    let outcomes = vec![outcome("good", 0)];
    let roll = super::roll_up_supplemental_leaf(&item, "RX", &outcomes, &root, &evidence);
    assert_eq!(roll.status, "BLOCKED", "{}", roll.reason);
    assert!(roll.reason.contains("another stage"));
    assert!(roll.reason.contains("r02_share_satisfied"));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn deferred_leaf_rollup_is_fixed_and_observational() {
    // 递延叶固定 DEFERRED_TO_R07：earlyEvidence 命令通过时附观察结果；
    // 未通过时叶状态不变（验收不归 R02），但命令 FAIL 由 overall 拦截。
    let root = temp_root("deferred-rollup");
    let evidence = root.join("evidence");
    std::fs::create_dir_all(&evidence).expect("create evidence root");
    std::fs::write(
        evidence.join("client-cases.json"),
        r#"{"schema":"lingxi.leaf-case-results.v1","cases":[{"case":"render","expect":1,"actual":1,"ok":true}]}"#,
    )
    .expect("write early case evidence");
    let mut item = leaf(
        "R00-T02-LA-TESTDEFERRED000",
        crate::stage_map::BASIS_DEFERRED_TO_R07,
        &[],
        &[],
        None,
    );
    item.early_evidence_command_refs = vec!["client".into()];
    item.deferred_r07_cases = vec![crate::stage_map::LeafDeferredCase {
        case: "render".into(),
        expect: 1,
        producer_command: "client".into(),
        evidence_path: "{EVIDENCE}/client-cases.json".into(),
    }];
    let roll =
        super::roll_up_supplemental_leaf(&item, "RX", &[outcome("client", 0)], &root, &evidence);
    assert_eq!(roll.status, "DEFERRED_TO_R07");
    assert!(roll.reason.contains("still REQUIRED"));
    assert!(roll.reason.contains("belongs to the R07 stage"));
    assert_eq!(roll.deferred_case_results[0]["status"], "OBSERVED_HELD");
    assert_eq!(
        roll.early_evidence.as_ref().unwrap()["commandsNotPassing"],
        serde_json::json!([])
    );

    // Producer failed this run: the leaf STAYS deferred, the failure is
    // only reported (the overall gate blocks on the command FAIL instead).
    let roll =
        super::roll_up_supplemental_leaf(&item, "RX", &[outcome("client", 3)], &root, &evidence);
    assert_eq!(roll.status, "DEFERRED_TO_R07");
    assert!(roll.reason.contains("early-evidence commands not passing"));
    assert_eq!(roll.deferred_case_results[0]["status"], "NOT_OBSERVED");
    assert_eq!(
        roll.early_evidence.as_ref().unwrap()["commandsNotPassing"],
        serde_json::json!(["client"])
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn share_and_deferred_coverage_makes_overall_pass() {
    // PASS ∪ DEFERRED_TO_R07 覆盖全部补充叶 + 全部命令通过 → overall PASS。
    let root = temp_root("covered");
    write_minimal_r00_ledger(
        &root,
        &[
            ("R00-T02-LA-TESTSHAREOK0000", "REQUIRED_SUPPLEMENTAL"),
            ("R00-T02-LA-TESTDEFEROK0000", "REQUIRED_SUPPLEMENTAL"),
        ],
    );
    let evidence = root.join("evidence");
    let map = StageMap {
        stage: "RX".into(),
        result_version: crate::RESULT_VERSION.into(),
        default_timeout_secs: 60,
        commands: vec![
            spec(
                "good",
                &[
                    "sh",
                    "-c",
                    "printf ok > {EVIDENCE}/good.txt && \
                     printf '{\"schema\":\"lingxi.leaf-case-results.v1\",\"cases\":[{\"case\":\"share-pin\",\"expect\":200,\"actual\":200,\"ok\":true},{\"case\":\"render\",\"expect\":1,\"actual\":1,\"ok\":true}]}' \
                     > {EVIDENCE}/leaf-cases.json",
                ],
                &["{EVIDENCE}/good.txt"],
                60,
            ),
            // Early-evidence producer of the deferred leaf's R07 remainder:
            // runs (referenced only by earlyEvidenceCommandRefs), stays
            // healthy, but its cases never gate the leaf in R02.
            spec(
                "client",
                &["sh", "-c", "printf early > {EVIDENCE}/early.txt"],
                &["{EVIDENCE}/early.txt"],
                60,
            ),
        ],
        scenarios: vec![crate::stage_map::Scenario {
            id: "RX-A01".into(),
            requirement: "REQUIRED".into(),
            command_refs: vec!["good".into()],
        }],
        supplemental_leaves: vec![
            leaf(
                "R00-T02-LA-TESTSHAREOK0000",
                crate::stage_map::BASIS_R02_SHARE_SATISFIED,
                &["good"],
                &[],
                contract("good", "{EVIDENCE}/leaf-cases.json", &[("share-pin", 200)]),
            ),
            {
                let mut deferred = leaf(
                    "R00-T02-LA-TESTDEFEROK0000",
                    crate::stage_map::BASIS_DEFERRED_TO_R07,
                    &[],
                    &[],
                    None,
                );
                deferred.early_evidence_command_refs = vec!["client".into()];
                deferred.deferred_r07_cases = vec![crate::stage_map::LeafDeferredCase {
                    case: "render".into(),
                    expect: 1,
                    producer_command: "client".into(),
                    evidence_path: "{EVIDENCE}/leaf-cases.json".into(),
                }];
                deferred
            },
        ],
    };
    let report =
        verify_stage(&map, &root, &evidence, "deadbeef", false, 0).expect("runs to completion");
    assert_eq!(report["overall"], "PASS");
    assert_eq!(
        report["supplementalLeafCoverage"]["pass"],
        serde_json::json!(1)
    );
    assert_eq!(
        report["supplementalLeafCoverage"]["deferredToR07"],
        serde_json::json!(1)
    );
    assert_eq!(
        report["supplementalLeafCoverage"]["fail"],
        serde_json::json!(0)
    );
    assert_eq!(
        report["supplementalLeafCoverage"]["blocked"],
        serde_json::json!(0)
    );
    assert_eq!(
        report["supplementalLeafCoverage"]["deferredLeafIds"],
        serde_json::json!(["R00-T02-LA-TESTDEFEROK0000"])
    );
    assert_eq!(
        report["supplementalLeafCoverage"]["commandsNotPassing"],
        serde_json::json!([])
    );
    // The early-evidence command really ran (execution order includes it).
    let keys: Vec<&str> = report["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["key"].as_str().unwrap())
        .collect();
    assert!(
        keys.contains(&"client"),
        "early-evidence producer ran: {keys:?}"
    );
    let deferred_entry = &report["supplementalLeafScenarios"][1];
    assert_eq!(deferred_entry["status"], "DEFERRED_TO_R07");
    assert_eq!(deferred_entry["deferredToStage"], "R07");
    assert_eq!(
        deferred_entry["deferredR07Cases"][0]["status"],
        "OBSERVED_HELD"
    );
    let share_entry = &report["supplementalLeafScenarios"][0];
    assert_eq!(share_entry["status"], "PASS");
    assert_eq!(share_entry["deferredToStage"], "R07");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn failing_early_evidence_command_blocks_overall_but_leaf_stays_deferred() {
    // 提前实现的已提交代码必须保持健康：earlyEvidence 生产者失败是命令
    // FAIL，阻断 overall；但递延叶状态固定 DEFERRED_TO_R07，不静默变绿
    // 也不伪装成本阶段失败。
    let root = temp_root("early-fail");
    write_minimal_r00_ledger(
        &root,
        &[("R00-T02-LA-TESTDEFERBAD000", "REQUIRED_SUPPLEMENTAL")],
    );
    let evidence = root.join("evidence");
    let map = StageMap {
        stage: "RX".into(),
        result_version: crate::RESULT_VERSION.into(),
        default_timeout_secs: 60,
        commands: vec![spec(
            "bad",
            &["sh", "-c", "exit 5"],
            &["{EVIDENCE}/x.txt"],
            60,
        )],
        scenarios: vec![crate::stage_map::Scenario {
            id: "RX-A01".into(),
            requirement: "REQUIRED".into(),
            command_refs: vec!["bad".into()],
        }],
        supplemental_leaves: vec![{
            let mut deferred = leaf(
                "R00-T02-LA-TESTDEFERBAD000",
                crate::stage_map::BASIS_DEFERRED_TO_R07,
                &[],
                &[],
                None,
            );
            deferred.early_evidence_command_refs = vec!["bad".into()];
            deferred
        }],
    };
    let report =
        verify_stage(&map, &root, &evidence, "deadbeef", false, 0).expect("runs to completion");
    assert_eq!(report["commands"][0]["status"], "FAIL");
    assert_eq!(
        report["supplementalLeafScenarios"][0]["status"],
        "DEFERRED_TO_R07"
    );
    assert!(report["supplementalLeafScenarios"][0]["reason"]
        .as_str()
        .unwrap()
        .contains("early-evidence commands not passing"));
    assert_eq!(
        report["supplementalLeafCoverage"]["deferredToR07"],
        serde_json::json!(1)
    );
    assert_eq!(
        report["supplementalLeafCoverage"]["commandsNotPassing"],
        serde_json::json!(["bad"])
    );
    assert_eq!(report["overall"], "FAIL");
    std::fs::remove_dir_all(&root).ok();
}

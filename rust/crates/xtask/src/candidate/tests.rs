use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn fixture() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "lingxi-xtask-binding-{}-{stamp}-{}",
        std::process::id(),
        FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    assert!(Command::new("git")
        .arg("init")
        .arg("-q")
        .arg(&root)
        .status()
        .unwrap()
        .success());
    root.canonicalize().unwrap()
}

fn add(root: &Path, path: &str) {
    assert!(Command::new("git")
        .args(["add", "--", path])
        .current_dir(root)
        .status()
        .unwrap()
        .success());
}

#[test]
fn binds_tracked_and_nonignored_untracked_bytes_without_own_evidence() {
    let root = fixture();
    fs::write(root.join("tracked.txt"), b"original").unwrap();
    fs::write(root.join(".gitignore"), b"ignored.txt\n").unwrap();
    add(&root, "tracked.txt");
    add(&root, ".gitignore");
    fs::write(root.join("new.txt"), b"new").unwrap();
    fs::write(root.join("ignored.txt"), b"ignored").unwrap();
    let scope = Scope::new(&root, &root.join("evidence")).unwrap();
    let before = scope.snapshot(&root).unwrap();
    assert_eq!(before.entries.len(), 3);
    assert!(before.entries.iter().any(|e| e.display == "new.txt"));
    fs::create_dir_all(root.join("evidence")).unwrap();
    fs::write(root.join("evidence/result.json"), b"self output").unwrap();
    let manifest_sha = write_manifest(&root.join("evidence/before.json"), &before).unwrap();
    assert_eq!(
        manifest_sha,
        hex(&Sha256::digest(
            fs::read(root.join("evidence/before.json")).unwrap()
        ))
    );
    fs::write(root.join("ignored.txt"), b"changed ignored").unwrap();
    assert_eq!(before.digest, scope.snapshot(&root).unwrap().digest);
    fs::write(root.join("tracked.txt"), b"changed").unwrap();
    let after_change = scope.snapshot(&root).unwrap();
    assert_ne!(before.digest, after_change.digest);
    assert_eq!(differences(&before, &after_change).len(), 1);
    fs::remove_file(root.join("tracked.txt")).unwrap();
    assert!(scope.snapshot(&root).unwrap_err().contains("is missing"));
    fs::create_dir(root.join("tracked.txt")).unwrap();
    assert!(scope
        .snapshot(&root)
        .unwrap_err()
        .contains("replaced by a non-file"));
    fs::remove_dir(root.join("tracked.txt")).unwrap();
    fs::write(root.join("tracked.txt"), b"original").unwrap();
    fs::remove_file(root.join("new.txt")).unwrap();
    let after_untracked_remove = scope.snapshot(&root).unwrap();
    assert_eq!(differences(&before, &after_untracked_remove).len(), 1);
    fs::write(root.join("new.txt"), b"new").unwrap();
    fs::write(root.join("late.txt"), b"late").unwrap();
    assert_eq!(
        differences(&before, &scope.snapshot(&root).unwrap()),
        vec![hex(b"late.txt")]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn refuses_evidence_at_repository_root() {
    let root = fixture();
    assert!(Scope::new(&root, &root).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn evidence_outside_repository_does_not_exclude_candidate_paths() {
    let root = fixture();
    fs::write(root.join("source.txt"), b"before").unwrap();
    add(&root, "source.txt");
    let external = fixture();
    let scope = Scope::new(&root, &external.join("run-output")).unwrap();
    assert!(scope.excluded_relative.is_none());
    let before = scope.snapshot(&root).unwrap();
    fs::create_dir(external.join("run-output")).unwrap();
    fs::write(external.join("run-output/log.txt"), b"output").unwrap();
    assert_eq!(before.digest, scope.snapshot(&root).unwrap().digest);
    fs::write(root.join("source.txt"), b"after").unwrap();
    assert_ne!(before.digest, scope.snapshot(&root).unwrap().digest);
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(external).unwrap();
}

#[test]
fn a_changed_checkpoint_cannot_be_erased_by_restoring_final_bytes() {
    let root = fixture();
    fs::write(root.join("source.txt"), b"before").unwrap();
    add(&root, "source.txt");
    let scope = Scope::new(&root, &root.join("evidence")).unwrap();
    let before = scope.snapshot(&root).unwrap();
    fs::write(root.join("source.txt"), b"during").unwrap();
    let during = scope.snapshot(&root).unwrap();
    fs::write(root.join("source.txt"), b"before").unwrap();
    let after = scope.snapshot(&root).unwrap();
    assert_eq!(before.digest, after.digest);
    assert!(!snapshots_stable(
        &before,
        &[("first".into(), 0, Ok(during))],
        &Ok(after.clone())
    ));
    assert!(!snapshots_stable(
        &before,
        &[("first".into(), 0, Err("read denied".into()))],
        &Ok(after)
    ));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn symlink_is_refused_and_non_utf8_path_identity_is_byte_exact() {
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;
    let root = fixture();
    let strange = OsString::from_vec(b"odd-\xff.txt".to_vec());
    let strange_path = path_from_git(b"odd-\xff.txt").unwrap();
    assert_eq!(os_bytes(strange_path.as_os_str()), b"odd-\xff.txt");
    let odd_created = fs::write(root.join(&strange), b"one").is_ok();
    let scope = Scope::new(&root, &root.join("evidence")).unwrap();
    let before = scope.snapshot(&root).unwrap();
    if odd_created {
        assert!(before
            .entries
            .iter()
            .any(|e| e.path_hex == hex(b"odd-\xff.txt")));
    }
    symlink("first", root.join("link")).unwrap();
    assert!(scope
        .snapshot(&root)
        .unwrap_err()
        .contains("crosses a symlink"));
    fs::remove_file(root.join("link")).unwrap();
    assert_eq!(before.digest, scope.snapshot(&root).unwrap().digest);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn evidence_symlink_to_source_or_parent_is_refused() {
    use std::os::unix::fs::symlink;
    let root = fixture();
    fs::create_dir(root.join("source")).unwrap();
    fs::write(root.join("source/file.txt"), b"source").unwrap();
    add(&root, "source/file.txt");
    symlink("source", root.join("evidence-link")).unwrap();
    assert!(Scope::new(&root, &root.join("evidence-link"))
        .unwrap_err()
        .contains("crosses symlink"));
    assert!(Scope::new(&root, &root.join("evidence-link/fresh"))
        .unwrap_err()
        .contains("crosses symlink"));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn tracked_file_parent_symlink_cannot_escape_repository() {
    use std::os::unix::fs::symlink;
    let root = fixture();
    fs::create_dir(root.join("source")).unwrap();
    fs::write(root.join("source/file.txt"), b"original").unwrap();
    add(&root, "source/file.txt");
    let scope = Scope::new(&root, &root.join("evidence")).unwrap();
    let before = scope.snapshot(&root).unwrap();
    let outside = fixture();
    fs::write(outside.join("file.txt"), b"outside").unwrap();
    fs::rename(root.join("source"), root.join("moved-source")).unwrap();
    symlink(&outside, root.join("source")).unwrap();
    let err = scope.snapshot(&root).unwrap_err();
    assert!(err.contains("crosses a symlink"), "{err}");
    fs::remove_file(root.join("source")).unwrap();
    fs::rename(root.join("moved-source"), root.join("source")).unwrap();
    assert_eq!(before.digest, scope.snapshot(&root).unwrap().digest);
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[cfg(unix)]
#[test]
fn real_os_fd_parent_and_child_logs_keep_all_checkpoints_stable() {
    use std::process::Stdio;
    let root = fixture();
    let run = root.join("artifacts/rust-tauri/R05/run001");
    fs::create_dir_all(run.join("child")).unwrap();
    fs::write(run.join("child/old-evidence.json"), b"old").unwrap();
    fs::write(root.join("source.txt"), b"source").unwrap();
    add(&root, "source.txt");
    let result = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "candidate::tests::real_os_fd_scope_process_probe",
            "--nocapture",
        ])
        .env("LINGXI_SCOPE_PROBE_ROOT", &root)
        .env("LINGXI_SCOPE_PROBE_LEVEL", "parent")
        .stdout(Stdio::from(File::create(run.join("stdout.log")).unwrap()))
        .stderr(Stdio::from(File::create(run.join("stderr.log")).unwrap()))
        .status()
        .unwrap();
    assert!(
        result.success(),
        "{}",
        String::from_utf8_lossy(&fs::read(run.join("child/stdout.log")).unwrap())
    );
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("probe-result.json")).unwrap()).unwrap();
    assert_eq!(report["stable"], true);
    assert_eq!(report["checkpointCount"], 3);
    assert_eq!(report["oldEvidenceDetected"], true);
    assert_eq!(report["sourceDetected"], true);
    assert_eq!(
        report["excluded"]["runOutputFiles"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn real_os_fd_scope_process_probe() {
    use std::process::Stdio;
    let Some(root) = std::env::var_os("LINGXI_SCOPE_PROBE_ROOT").map(PathBuf::from) else {
        // 默认腿证明仓库外的真实日志不被错误归属到当前候选。
        let root = fixture();
        let scope = Scope::new(&root, &root.join("evidence")).unwrap();
        assert_eq!(
            scope.exclusion_json()["runOutputFiles"],
            serde_json::json!([])
        );
        fs::remove_dir_all(root).unwrap();
        return;
    };
    let run = root.join("artifacts/rust-tauri/R05/run001");
    if std::env::var("LINGXI_SCOPE_PROBE_LEVEL").unwrap() == "parent" {
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "candidate::tests::real_os_fd_scope_process_probe",
                "--nocapture",
            ])
            .env("LINGXI_SCOPE_PROBE_LEVEL", "child")
            .stdout(Stdio::from(
                File::create(run.join("child/stdout.log")).unwrap(),
            ))
            .stderr(Stdio::from(
                File::create(run.join("child/stderr.log")).unwrap(),
            ))
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let scope = Scope::new(&root, &run.join("child/evidence")).unwrap();
    let before = scope.snapshot(&root).unwrap();
    assert!(before
        .entries
        .iter()
        .any(|e| e.display.ends_with("old-evidence.json")));
    let mut checkpoints = Vec::new();
    for level in ["", "child/", ""] {
        let mut log = fs::OpenOptions::new()
            .append(true)
            .open(run.join(format!("{level}stdout.log")))
            .unwrap();
        writeln!(log, "真实祖先日志增长").unwrap();
        checkpoints.push((level.to_string(), 0, scope.snapshot(&root)));
    }
    let stable = snapshots_stable(&before, &checkpoints, &scope.snapshot(&root));
    fs::write(run.join("child/old-evidence.json"), b"changed").unwrap();
    let old_detected = before.digest != scope.snapshot(&root).unwrap().digest;
    fs::write(run.join("child/old-evidence.json"), b"old").unwrap();
    fs::write(root.join("source.txt"), b"changed").unwrap();
    let source_detected = before.digest != scope.snapshot(&root).unwrap().digest;
    let sink = run.join("child/stdout.log");
    let moved = run.join("child/moved.log");
    fs::rename(&sink, &moved).unwrap();
    fs::write(&sink, b"foreign replacement").unwrap();
    let replacement_rejected = scope.snapshot(&root).unwrap_err().contains("was replaced");
    fs::remove_file(&sink).unwrap();
    fs::rename(&moved, &sink).unwrap();
    let report = serde_json::json!({"stable": stable, "checkpointCount": checkpoints.len(), "oldEvidenceDetected": old_detected, "sourceDetected": source_detected, "replacedSinkRejected": replacement_rejected, "excluded": scope.exclusion_json()});
    fs::write(
        root.join("probe-result.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    assert!(
        stable && old_detected && source_detected && replacement_rejected,
        "{report}"
    );
}

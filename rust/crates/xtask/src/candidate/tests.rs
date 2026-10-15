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

#[test]
fn parallel_full_file_hashes_preserve_serial_order_modes_and_refusals() {
    let root = fixture();
    let scope = Scope::new(&root, &root.join("evidence")).unwrap();
    let mut paths = Vec::new();
    for index in 0..33 {
        let relative = format!("nested/{index:02}/proof.txt");
        let path = root.join(&relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![index as u8; index * 173 + 1]).unwrap();
        paths.push(relative.into_bytes());
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let raw = b"non-utf8-\xff.txt".to_vec();
        match fs::write(
            root.join(OsString::from_vec(raw.clone())),
            b"exact raw path",
        ) {
            Ok(()) => paths.push(raw),
            Err(error)
                if cfg!(target_os = "macos") && error.raw_os_error() == Some(libc::EILSEQ) =>
            {
                // 本机文件系统明确拒绝非法UTF8文件名；仍核对原始路径字节没有被改写。
                assert_eq!(os_bytes(path_from_git(&raw).unwrap().as_os_str()), raw);
                println!("当前macOS文件系统拒绝非法UTF8文件名，真实文件并行比较仅覆盖可创建路径");
            }
            Err(error) => panic!("无法准备原始字节路径：{error}"),
        }
    }
    paths.sort();
    let serial = scope.hash_entries(&root, &paths).unwrap();
    for workers in [1, 2, 4] {
        assert_eq!(
            serial,
            scope.hash_entries_parallel(&root, &paths, workers).unwrap()
        );
    }
    assert!(scope
        .hash_entries_parallel(&root, &[], 4)
        .unwrap()
        .is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            root.join("nested/17/proof.txt"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let changed = scope.hash_entries_parallel(&root, &paths, 4).unwrap();
        assert_eq!(changed, scope.hash_entries(&root, &paths).unwrap());
        assert_eq!(
            changed
                .iter()
                .find(|entry| entry.display == "nested/17/proof.txt")
                .unwrap()
                .mode,
            Some(0o600)
        );
    }
    fs::remove_file(root.join("nested/17/proof.txt")).unwrap();
    assert_eq!(
        scope.hash_entries(&root, &paths).unwrap_err(),
        scope.hash_entries_parallel(&root, &paths, 4).unwrap_err()
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            root.join("nested/18/proof.txt"),
            root.join("nested/17/proof.txt"),
        )
        .unwrap();
        let error = scope.hash_entries_parallel(&root, &paths, 4).unwrap_err();
        assert!(error.contains("symlink"));
        assert_eq!(error, scope.hash_entries(&root, &paths).unwrap_err());
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn identical_checkpoint_storage_is_shared_but_changed_checkpoint_still_fails() {
    let root = fixture();
    fs::write(root.join("proof.txt"), b"before").unwrap();
    let scope = Scope::new(&root, &root.join("evidence")).unwrap();
    let before = scope.snapshot(&root).unwrap();
    let mut same = scope.snapshot(&root).unwrap();
    let original_json = same.to_json();
    assert!(!std::sync::Arc::ptr_eq(&before.entries, &same.entries));
    same.share_unchanged_entries(&before);
    assert!(std::sync::Arc::ptr_eq(&before.entries, &same.entries));
    assert_eq!(same.to_json(), original_json);

    // 即使摘要字段被伪造成相同，条目不同也不能共用原结果。
    let mut different_entries = before.entries.as_ref().clone();
    different_entries[0].sha256 = Some("different-content-hash".into());
    let mut forged_digest = Snapshot {
        digest: before.digest.clone(),
        entries: std::sync::Arc::new(different_entries),
    };
    let forged_json = forged_digest.to_json();
    forged_digest.share_unchanged_entries(&before);
    assert!(!std::sync::Arc::ptr_eq(
        &before.entries,
        &forged_digest.entries
    ));
    assert_eq!(forged_digest.to_json(), forged_json);

    fs::write(root.join("proof.txt"), b"changed during verification").unwrap();
    let mut changed = scope.snapshot(&root).unwrap();
    changed.share_unchanged_entries(&before);
    assert!(!std::sync::Arc::ptr_eq(&before.entries, &changed.entries));
    assert_eq!(differences(&before, &changed), vec![hex(b"proof.txt")]);
    fs::write(root.join("proof.txt"), b"before").unwrap();
    let mut restored = scope.snapshot(&root).unwrap();
    restored.share_unchanged_entries(&before);
    assert!(std::sync::Arc::ptr_eq(&before.entries, &restored.entries));
    assert!(!snapshots_stable(
        &before,
        &[("changed".into(), 0, Ok(changed))],
        &Ok(restored)
    ));

    fs::remove_file(root.join("proof.txt")).unwrap();
    // 未跟踪文件消失必须仍被完整重读发现，不能用已共用的旧结果代替本次核对。
    let mut missing = scope.snapshot(&root).unwrap();
    missing.share_unchanged_entries(&before);
    assert!(!std::sync::Arc::ptr_eq(&before.entries, &missing.entries));
    assert_eq!(differences(&before, &missing), vec![hex(b"proof.txt")]);
    fs::remove_dir_all(root).unwrap();
}

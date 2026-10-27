// 本文件由真实集成测试 include；只替代外部模型响应。
#[tokio::test]
async fn r08_security_real_baseline_never_returns_secret_and_audits_counts() {
    let h = Harness::boot().await;
    let secret = format!("sk-{}", "aB3x".repeat(16));
    std::fs::write(
        h.workspace.join("credential.ts"),
        format!("const OPENAI_API_KEY = '{secret}';\n"),
    )
    .unwrap();
    std::fs::write(h.workspace.join("binary.txt"), b"a\0b").unwrap();
    std::fs::write(
        h.workspace.join("oversized.txt"),
        vec![b'a'; 3 * 1024 * 1024],
    )
    .unwrap();
    let outcome = h.invoke("security_scan", json!({"mode":"path"})).await;
    let d = details(&outcome);
    assert!(d["findingCount"].as_u64().unwrap() > 0, "{outcome:?}");
    assert!(d["filesSkipped"].as_u64().unwrap() >= 2, "{d}");
    assert_eq!(d["auditStatus"], "committed");
    assert!(!format!("{outcome:?}").contains(&secret));
    assert!(text(&outcome).contains("skip"));
    let audit =
        std::fs::read_to_string(h.state.config().data_home.join("logs/security-audit.jsonl"))
            .unwrap();
    assert!(audit.contains("security_scan"));
    assert!(!audit.contains(&secret));
    assert!(!audit.contains("OPENAI_API_KEY"));
    h.close().await;
}
#[tokio::test]
async fn r08_security_outside_root_and_symlink_refused_and_sources_unchanged() {
    let h = Harness::boot().await;
    let outside = h.root.join("outside.txt");
    std::fs::write(&outside, "secret outside sentinel").unwrap();
    let denied = h
        .invoke("security_scan", json!({"mode":"path","path":outside}))
        .await;
    assert!(matches!(denied, ToolOutcome::Failed { .. }), "{denied:?}");
    std::os::unix::fs::symlink(&outside, h.workspace.join("linked.txt")).unwrap();
    let denied = h
        .invoke("security_scan", json!({"mode":"path","path":"linked.txt"}))
        .await;
    assert!(matches!(denied, ToolOutcome::Failed { .. }), "{denied:?}");
    let scan = h.invoke("security_scan", json!({"mode":"path"})).await;
    assert!(!text(&scan).contains("outside sentinel"));
    assert_eq!(
        std::fs::read_to_string(outside).unwrap(),
        "secret outside sentinel"
    );
    h.close().await;
}
#[tokio::test]
async fn r08_security_changes_git_readonly_and_subdirectory_never_enumerates_parent() {
    let h = Harness::boot().await;
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&h.workspace)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q"]);
    std::fs::write(h.workspace.join("tracked.ts"), "let x = 1;\n").unwrap();
    git(&["add", "tracked.ts"]);
    let before = std::fs::read(h.workspace.join(".git/index")).unwrap();
    std::fs::write(h.workspace.join("tracked.ts"), "let x = 2;\n").unwrap();
    let outcome = h.invoke("security_scan", json!({"mode":"changes"})).await;
    assert!(
        matches!(outcome, ToolOutcome::Success { .. }),
        "{outcome:?}"
    );
    assert_eq!(
        std::fs::read(h.workspace.join(".git/index")).unwrap(),
        before,
        "只读扫描不应刷新索引"
    );
    let d = details(&outcome);
    assert_eq!(d["filesScanned"], 1, "{d}");
    h.close().await;
    let sub = Harness::boot_subroot(false, true).await;
    let denied = sub.invoke("security_scan", json!({"mode":"changes"})).await;
    assert!(matches!(denied, ToolOutcome::Failed { .. }), "{denied:?}");
    assert!(text(&denied).contains("SECURITY_GIT_SCOPE_UNAVAILABLE"));
    let scan = sub.invoke("security_scan", json!({"mode":"path"})).await;
    assert_eq!(details(&scan)["filesScanned"], 1, "{scan:?}");
    assert!(!format!("{scan:?}").contains("parent-only-sentinel"));
    sub.close().await;
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn r08_security_external_scanner_cannot_read_or_write_outside_snapshots_and_leaves_no_child()
{
    use std::os::unix::fs::PermissionsExt;
    const CHILD: &str = "LINGXI_R08_SECURITY_CONFINED_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let h = Harness::boot().await;
        std::fs::write(h.workspace.join("code.ts"), "const code=1;").unwrap();
        let result = h.invoke("security_scan", json!({"mode":"path"})).await;
        let d = details(&result);
        assert_eq!(
            d["external"]["semgrep"]["status"], "completed",
            "{result:?}"
        );
        let rule = d["findings"][0]["ruleId"]
            .as_str()
            .expect("扫描器外围替身必须交回可观察的真实孙进程PID");
        let pid = rule
            .strip_prefix("semgrep:r08_child_")
            .unwrap()
            .parse::<i32>()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while unsafe { libc::kill(pid, 0) } == 0 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("扫描器孙进程残留");
        h.close().await;
        return;
    }
    // 只替代第三方扫描器进程，权限、文件快照、内核和持久存储均是真实实现。
    let root = std::env::temp_dir().join(format!(
        "r08-scanner-boundary-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let secret = root.join("outside-secret.txt");
    let write = root.join("outside-write.txt");
    std::fs::write(&secret, "SYNTHETIC_OUTSIDE_SENTINEL").unwrap();
    let script = format!(
        r##"#!/bin/sh
if /bin/cat '{}' >/dev/null 2>&1; then printf '{{"errors":["OUTSIDE_READ_SUCCEEDED"],"results":[]}}'; exit 0; fi
if (printf MUST_NOT_WRITE > '{}') 2>/dev/null; then printf '{{"errors":["OUTSIDE_WRITE_SUCCEEDED"],"results":[]}}'; exit 0; fi
/bin/sleep 120 >/dev/null 2>&1 &
printf '{{"results":[{{"path":"00000/code.ts","start":{{"line":1}},"check_id":"r08_child_%s"}}]}}' "$!"
"##,
        secret.display(),
        write.display()
    );
    std::fs::write(root.join("semgrep"), script).unwrap();
    std::fs::set_permissions(root.join("semgrep"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut paths = vec![root.clone()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let output=tokio::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","r08_security_external_scanner_cannot_read_or_write_outside_snapshots_and_leaves_no_child","--nocapture"]).env(CHILD,"1").env("PATH",std::env::join_paths(paths).unwrap()).output().await.unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&secret).unwrap(),
        "SYNTHETIC_OUTSIDE_SENTINEL"
    );
    assert!(!write.exists());
    std::fs::remove_dir_all(root).unwrap();
}

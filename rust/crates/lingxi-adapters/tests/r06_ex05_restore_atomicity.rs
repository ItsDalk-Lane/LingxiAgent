//! EX05：真实文件失败、进程中断、原文回滚、外部变化与十份修订保留。
#![cfg(target_os = "macos")]
use lingxi_adapters::storage::agent_memory as am;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
fn root(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "r06-ex05-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn flag(path: &Path, on: bool) {
    assert!(std::process::Command::new("chflags")
        .arg(if on { "uchg" } else { "nouchg" })
        .arg(path)
        .status()
        .unwrap()
        .success());
}
fn seed(p: &Path) -> String {
    std::fs::create_dir_all(p.join("daily")).unwrap();
    std::fs::write(p.join("daily/2026-10-14.md"), "OLD daily\n").unwrap();
    let s = am::CompiledMemoryStore::new(p, false);
    s.write_facts("OLD facts").unwrap();
    s.write_today("OLD today").unwrap();
    s.write_longterm("OLD longterm").unwrap();
    s.assemble_week().unwrap();
    s.assemble().unwrap();
    let r = am::DreamRevisionStore::new(p)
        .save("original-target", "manual", "dream", None)
        .unwrap();
    s.write_facts("CURRENT facts").unwrap();
    s.write_today("CURRENT today").unwrap();
    s.write_longterm("CURRENT longterm").unwrap();
    std::fs::write(p.join("daily/2026-10-14.md"), "CURRENT daily\n").unwrap();
    s.assemble_week().unwrap();
    s.assemble().unwrap();
    r["revisionId"].as_str().unwrap().into()
}
fn bytes(p: &Path) -> Vec<Vec<u8>> {
    [
        "facts.md",
        "today.md",
        "longterm.md",
        "week.md",
        "memory.md",
        "daily/2026-10-14.md",
    ]
    .iter()
    .map(|n| std::fs::read(p.join(n)).unwrap())
    .collect()
}
#[test]
fn successful_restore_undo_keep_target_and_safety_at_ten_revisions_without_touching_sources() {
    let p = root("retain");
    let target = seed(&p);
    let r = am::DreamRevisionStore::new(&p);
    let sources = ["facts.db", "facts.jsonl", "tenets.json", "navigation.md"];
    for n in sources {
        std::fs::write(p.join(n), format!("SOURCE {n}")).unwrap();
    }
    am::CompiledMemoryStore::new(&p, false).assemble().unwrap();
    let before = bytes(&p);
    let protected: Vec<_> = sources
        .iter()
        .map(|n| std::fs::read(p.join(n)).unwrap())
        .collect();
    for i in 0..9 {
        r.save(&format!("fill-{i}"), "manual", "dream", None)
            .unwrap();
    }
    assert_eq!(r.list().unwrap().len(), 10);
    let applied = r.restore(&target).unwrap();
    let pre = applied["preRestoreRevisionId"].as_str().unwrap();
    assert_eq!(r.list().unwrap().len(), 10);
    assert!(r.read(&target).is_ok());
    assert!(r.read(pre).is_ok());
    assert_eq!(
        std::fs::read_to_string(p.join("facts.md")).unwrap(),
        "OLD facts\n"
    );
    assert_eq!(
        std::fs::read_to_string(p.join("daily/2026-10-14.md")).unwrap(),
        "OLD daily\n"
    );
    assert_eq!(r.restore_status().unwrap().unwrap()["status"], "succeeded");
    r.restore(pre).unwrap();
    assert_eq!(bytes(&p), before);
    assert_eq!(r.list().unwrap().len(), 10);
    for (i, n) in sources.iter().enumerate() {
        assert_eq!(std::fs::read(p.join(n)).unwrap(), protected[i]);
    }
    assert!(!p.join("dream/pending-apply.json").exists());
    std::fs::remove_dir_all(p).unwrap();
}
#[test]
fn rollback_disk_failure_remains_explicit_and_read_retries_after_real_unlock() {
    let p = root("rollback");
    let target = seed(&p);
    let before = bytes(&p);
    let r = am::DreamRevisionStore::new(&p);
    let hook_path = p.clone();
    let hook = am::memory_restore_test_hook(
        &p,
        Arc::new(move |name| {
            if name == "facts.md" {
                flag(&hook_path.join(name), true)
            }
        }),
    );
    flag(&p.join("today.md"), true);
    let result = r.restore(&target);
    drop(hook);
    assert!(am::CompiledMemoryStore::new(&p, false)
        .read_sections()
        .is_err());
    flag(&p.join("today.md"), false);
    flag(&p.join("facts.md"), false);
    assert!(result.unwrap_err().to_string().contains("回滚失败"));
    assert!(p.join("dream/pending-apply.json").is_file());
    assert_eq!(
        r.restore_status().unwrap().unwrap()["status"],
        "failed_recovery_required"
    );
    assert_eq!(
        am::CompiledMemoryStore::new(&p, false)
            .read_sections()
            .unwrap()
            .facts,
        "CURRENT facts"
    );
    assert_eq!(bytes(&p), before);
    assert_eq!(
        r.restore_status().unwrap().unwrap()["status"],
        "interrupted_rolled_back"
    );
    assert!(!p.join("dream/pending-apply.json").exists());
    std::fs::remove_dir_all(p).unwrap();
}
#[test]
fn external_edit_is_preserved_and_unknown_recovery_blocks_compiled_consumption() {
    let p = root("external");
    let target = seed(&p);
    let before = bytes(&p);
    let r = am::DreamRevisionStore::new(&p);
    let hp = p.clone();
    let hook = am::memory_restore_test_hook(
        &p,
        Arc::new(move |name| {
            if name == "facts.md" {
                std::fs::write(hp.join("today.md"), "EXTERNAL edit\n").unwrap();
            }
        }),
    );
    let result = r.restore(&target);
    drop(hook);
    assert!(result.unwrap_err().to_string().contains("外部修改"));
    assert_eq!(
        std::fs::read_to_string(p.join("today.md")).unwrap(),
        "EXTERNAL edit\n"
    );
    assert_eq!(std::fs::read(p.join("facts.md")).unwrap(), before[0]);
    assert!(am::CompiledMemoryStore::new(&p, false)
        .read_sections()
        .is_err());
    assert!(p.join("dream/pending-apply.json").exists());
    assert_eq!(
        std::fs::read_to_string(p.join("today.md")).unwrap(),
        "EXTERNAL edit\n"
    );
    // 仅用户自行解决冲突后，再验证同一日志可完成；宿主不会自动覆盖外部版本。
    std::fs::write(p.join("today.md"), &before[1]).unwrap();
    assert!(r.recover_pending().unwrap());
    assert_eq!(bytes(&p), before);
    std::fs::remove_dir_all(p).unwrap();
}
#[test]
fn restore_child_driver() {
    let Some(raw) = std::env::var_os("LINGXI_EX05_RESTORE_CHILD") else {
        return;
    };
    let p = PathBuf::from(raw);
    let r = am::DreamRevisionStore::new(&p);
    if std::env::var_os("LINGXI_EX05_RECOVER_CHILD").is_some() {
        assert_eq!(
            am::CompiledMemoryStore::new(&p, false)
                .read_sections()
                .unwrap()
                .facts,
            "CURRENT facts"
        );
        assert_eq!(
            r.restore_status().unwrap().unwrap()["status"],
            "interrupted_rolled_back"
        );
        return;
    }
    let target: Value =
        serde_json::from_slice(&std::fs::read(p.join("target.json")).unwrap()).unwrap();
    let _hook = am::memory_restore_test_hook(
        &p,
        Arc::new(|name| {
            if name == "facts.md" {
                std::process::exit(77)
            }
        }),
    );
    r.restore(target.as_str().unwrap()).unwrap();
    panic!("必须在真实第一份写后退出");
}
#[test]
fn actual_process_exit_after_first_write_recovers_old_bytes_before_next_consumer() {
    // 子进程原始输出逐行保留；前缀避免被门禁误认成父进程的测试结论。
    fn child_status(output: std::process::Output) -> std::process::ExitStatus {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            eprintln!("EX05_CHILD_STDOUT {line}");
        }
        for line in String::from_utf8_lossy(&output.stderr).lines() {
            eprintln!("EX05_CHILD_STDERR {line}");
        }
        output.status
    }
    let p = root("exit");
    let target = seed(&p);
    let before = bytes(&p);
    std::fs::write(p.join("target.json"), json!(target).to_string()).unwrap();
    let status = child_status(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "restore_child_driver", "--nocapture"])
            .env("LINGXI_EX05_RESTORE_CHILD", &p)
            .output()
            .unwrap(),
    );
    assert_eq!(status.code(), Some(77));
    assert_eq!(
        std::fs::read_to_string(p.join("facts.md")).unwrap(),
        "OLD facts\n"
    );
    assert_eq!(
        std::fs::read_to_string(p.join("today.md")).unwrap(),
        "CURRENT today\n"
    );
    assert!(p.join("dream/pending-apply.json").is_file());
    assert_eq!(
        am::DreamRevisionStore::new(&p)
            .restore_status()
            .unwrap()
            .unwrap()["status"],
        "running"
    );
    let restarted = child_status(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "restore_child_driver", "--nocapture"])
            .env("LINGXI_EX05_RESTORE_CHILD", &p)
            .env("LINGXI_EX05_RECOVER_CHILD", "1")
            .output()
            .unwrap(),
    );
    assert!(restarted.success());
    assert_eq!(bytes(&p), before);
    assert!(!p.join("dream/pending-apply.json").exists());
    assert!(!am::DreamRevisionStore::new(&p).recover_pending().unwrap());
    std::fs::remove_dir_all(p).unwrap();
}

#[test]
fn explicit_unknown_journal_operation_is_refused_without_mutation() {
    let p = root("unknown-operation");
    seed(&p);
    let before = bytes(&p);
    let journal = json!({"schemaVersion":1,"operation":"future-unknown","revisionId":"unknown-id","files":{"facts.md":"UNKNOWN OLD facts\n","longterm.md":"UNKNOWN OLD longterm\n","memory.md":"UNKNOWN OLD compiled\n"},"retentionFiles":{}});
    let raw = serde_json::to_vec_pretty(&journal).unwrap();
    std::fs::write(p.join("dream/pending-apply.json"), &raw).unwrap();
    let result = am::DreamRevisionStore::new(&p).recover_pending();
    println!(
        "EX05_UNKNOWN_OPERATION {}",
        json!({"result":format!("{result:?}"),"allBytesUnchanged":bytes(&p)==before,"journalPreserved":std::fs::read(p.join("dream/pending-apply.json")).ok().as_ref()==Some(&raw),"actualFacts":std::fs::read_to_string(p.join("facts.md")).unwrap()})
    );
    assert!(
        result.is_err(),
        "未知operation必须拒绝，不能冒充旧apply执行"
    );
    assert_eq!(bytes(&p), before);
    assert_eq!(
        std::fs::read(p.join("dream/pending-apply.json")).unwrap(),
        raw
    );
    std::fs::remove_dir_all(p).unwrap();
}

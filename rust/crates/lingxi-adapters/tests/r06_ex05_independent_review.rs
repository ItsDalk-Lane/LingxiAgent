//! EX05 独立反例：真实第二段写失败时恢复不得留下混合正文。
#![cfg(target_os = "macos")]
use lingxi_adapters::storage::agent_memory as am;
use serde_json::json;
#[test]
fn restore_second_file_io_failure_preserves_every_current_byte() {
    let root = std::env::temp_dir().join(format!(
        "ex05-restore-review-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let store = am::CompiledMemoryStore::new(&root, false);
    store.write_facts("OLD facts").unwrap();
    store.write_today("OLD today").unwrap();
    store.write_longterm("OLD longterm").unwrap();
    let revisions = am::DreamRevisionStore::new(&root);
    let target = revisions
        .save("independent-target", "manual", "dream", None)
        .unwrap();
    store.write_facts("CURRENT facts").unwrap();
    store.write_today("CURRENT today").unwrap();
    store.write_longterm("CURRENT longterm").unwrap();
    let names = ["facts.md", "today.md", "longterm.md", "memory.md"];
    let before: Vec<_> = names
        .iter()
        .map(|n| std::fs::read(root.join(n)).unwrap())
        .collect();
    assert!(std::process::Command::new("chflags")
        .arg("uchg")
        .arg(root.join("today.md"))
        .status()
        .unwrap()
        .success());
    let result = revisions.restore(target["revisionId"].as_str().unwrap());
    assert!(std::process::Command::new("chflags")
        .arg("nouchg")
        .arg(root.join("today.md"))
        .status()
        .unwrap()
        .success());
    let after: Vec<_> = names
        .iter()
        .map(|n| std::fs::read(root.join(n)).unwrap())
        .collect();
    let facts = json!({"root":root,"result":format!("{result:?}"),"files":names.iter().enumerate().map(|(i,n)|json!({"name":n,"before":String::from_utf8_lossy(&before[i]),"after":String::from_utf8_lossy(&after[i]),"unchanged":before[i]==after[i]})).collect::<Vec<_>>(),"pendingExists":root.join("dream/pending-apply.json").exists(),"revisions":revisions.list().unwrap()});
    std::fs::write(
        "/private/tmp/r06-ex05-delivery/restore-second-write-fixed-facts.json",
        serde_json::to_vec_pretty(&facts).unwrap(),
    )
    .unwrap();
    println!("EX05_RESTORE_FACTS {facts}");
    assert!(result.is_err());
    assert_eq!(after, before, "恢复失败必须保留所有当前正文原字节");
}

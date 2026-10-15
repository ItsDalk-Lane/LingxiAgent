//! 独立审查探针：实际第二连接竞争与根目录替换，不改生产实现。
use lingxi_adapters::storage::knowledge as ks;
use serde_json::json;
use std::sync::{mpsc, Arc};
use std::time::Duration;

#[test]
fn actual_second_connection_cannot_attach_between_gc_check_and_delete() {
    let root = std::env::temp_dir().join(ks::new_id("ex08-independent-lease"));
    let facts =
        Arc::new(ks::KnowledgeFactStore::open_with_clock(&root, Arc::new(|| 1000)).unwrap());
    let book = facts.create_notebook("studio", "旧笔记本").unwrap();
    let keeper = facts.create_notebook("studio", "新笔记本").unwrap();
    let (source, snapshot) = facts
        .create_source_with_snapshot(
            "studio",
            "pasted_text",
            "原文",
            &json!({}),
            b"LEASE-ORIGINAL",
            "text/plain",
        )
        .unwrap();
    facts.add_membership(&book.id, &source.id).unwrap();
    facts.delete_notebook_retained("studio", &book.id).unwrap();
    let second = ks::KnowledgeFactStore::open_with_clock(&root, Arc::new(|| 1000)).unwrap();
    let (checked_tx, checked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (attach_tx, attach_rx) = mpsc::channel();
    let source_id = source.id.clone();
    let lease = Arc::clone(&facts);
    let gc = std::thread::spawn(move || {
        lease.purge_orphan_if_unreferenced("studio", &source_id, 1000, |_| {
            checked_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        })
    });
    checked_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let source_id = source.id.clone();
    let attach = std::thread::spawn(move || {
        let result = second.add_membership(&keeper.id, &source_id);
        attach_tx.send(result.is_ok()).unwrap();
    });
    // 另一连接不能在已获清理租约之后偷偷提交新引用。
    let before_release = attach_rx.recv_timeout(Duration::from_millis(150));
    assert!(
        !matches!(before_release, Ok(true)),
        "租约内不得成功插入引用"
    );
    release_tx.send(()).unwrap();
    assert!(gc.join().unwrap().unwrap().is_none());
    let attached = match before_release {
        Ok(value) => value,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            attach_rx.recv_timeout(Duration::from_secs(5)).unwrap()
        }
        Err(error) => panic!("{error}"),
    };
    assert!(!attached);
    attach.join().unwrap();
    assert!(facts.get_source(&source.id).is_err());
    assert!(!root.join(snapshot.storage_path).exists());
    println!("EX08_INDEPENDENT_LEASE_PROOF {}", root.display());
}

#[cfg(unix)]
#[test]
fn gc_uses_opened_root_after_path_is_replaced_and_never_follows_output_leaf_link() {
    use std::os::unix::fs::symlink;
    let root = std::env::temp_dir().join(ks::new_id("ex08-independent-root"));
    let facts = ks::KnowledgeFactStore::open_with_clock(&root, Arc::new(|| 1000)).unwrap();
    let book = facts.create_notebook("studio", "合成根替换").unwrap();
    let (source, snapshot) = facts
        .create_source_with_snapshot(
            "studio",
            "pasted_text",
            "原文",
            &json!({}),
            b"PINNED-ORIGINAL",
            "text/plain",
        )
        .unwrap();
    facts.add_membership(&book.id, &source.id).unwrap();
    facts.delete_notebook_retained("studio", &book.id).unwrap();
    let held = root.with_extension("held");
    std::fs::rename(&root, &held).unwrap();
    let output = held.join("artifacts").join(&snapshot.id);
    std::fs::create_dir_all(&output).unwrap();
    let outside = held.with_extension("outside");
    std::fs::write(&outside, b"OUTSIDE-MUST-STAY").unwrap();
    symlink(&outside, output.join("linked.txt")).unwrap();
    std::fs::create_dir_all(root.join("snapshots")).unwrap();
    std::fs::write(root.join(&snapshot.storage_path), b"REPLACEMENT-MUST-STAY").unwrap();
    assert!(facts
        .purge_orphan_if_unreferenced("studio", &source.id, 1000, |_| Ok(()))
        .unwrap()
        .is_none());
    assert!(!held.join(&snapshot.storage_path).exists());
    assert!(!output.exists());
    assert_eq!(std::fs::read(&outside).unwrap(), b"OUTSIDE-MUST-STAY");
    assert_eq!(
        std::fs::read(root.join(&snapshot.storage_path)).unwrap(),
        b"REPLACEMENT-MUST-STAY"
    );
    println!(
        "EX08_INDEPENDENT_ROOT_PROOF {} {}",
        root.display(),
        held.display()
    );
}

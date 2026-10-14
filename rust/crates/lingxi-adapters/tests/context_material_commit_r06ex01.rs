//! EX01资料发布栅栏：实际嵌套写入及跨线程发布不能穿过最终保存保留区。
use lingxi_adapters::storage::agent_memory::{context_material_epoch, ContextMaterialMutation};
use std::{
    sync::{mpsc, Arc},
    time::Duration,
};
fn home(tag: &str) -> std::path::PathBuf {
    let root = std::path::PathBuf::from(std::env::var("LINGXI_HOME").expect("合成HOME"));
    let path = root.join(format!("ex01-material-fence-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}
#[test]
fn nested_writers_refuse_commit_until_all_publications_finish() {
    let home = home("nested");
    let epoch = context_material_epoch(&home);
    let before = epoch.stable_revision().unwrap();
    let outer = ContextMaterialMutation::for_home(&home);
    let inner = ContextMaterialMutation::for_home(&home);
    assert!(epoch.stable_revision().is_none());
    assert!(epoch
        .with_stable_commit(|| panic!("写者活动时不能进入保存"))
        .is_none());
    drop(inner);
    assert!(epoch.with_stable_commit(|| ()).is_none());
    drop(outer);
    assert_eq!(epoch.stable_revision(), Some(before + 4));
    assert_eq!(
        epoch.with_stable_commit(|| epoch.stable_revision()),
        Some(Some(before + 4))
    );
}
#[test]
fn actual_new_publication_waits_until_commit_fence_releases() {
    let home = home("parallel");
    let epoch = context_material_epoch(&home);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let commit_epoch = Arc::clone(&epoch);
    let commit = std::thread::spawn(move || {
        commit_epoch
            .with_stable_commit(|| {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                assert_eq!(commit_epoch.stable_revision(), Some(0));
            })
            .expect("没有写者应允许进入最终保存");
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (writer_tx, writer_rx) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        let _publication = ContextMaterialMutation::for_home(&home);
        writer_tx.send(()).unwrap();
    });
    assert!(
        writer_rx.recv_timeout(Duration::from_millis(100)).is_err(),
        "保存保留区未退出时不能发布新资料"
    );
    release_tx.send(()).unwrap();
    commit.join().unwrap();
    writer_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    writer.join().unwrap();
    assert_eq!(epoch.stable_revision(), Some(2));
}

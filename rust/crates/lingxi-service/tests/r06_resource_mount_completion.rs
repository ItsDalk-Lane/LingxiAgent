//! 已登记挂载与本机文件共用真实资源内核，故障点只改变真实文件时序。
use lingxi_service::{
    resourceaccess::ResourceAccess,
    resourceio::{OpContext, ResourceIoService},
    resources::ResourceService,
    sessionfiles::SessionFileService,
};
use serde_json::json;
use std::{path::PathBuf, sync::Arc};
struct Fixture {
    root: PathBuf,
    workspace: PathBuf,
    mount: PathBuf,
    sf: Arc<SessionFileService>,
    io: ResourceIoService,
}
impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "r06-mount-{}-{}",
            std::process::id(),
            lingxi_service::auth::hex_random_public(10).unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let home = root.join("home");
        let workspace = root.join("workspace");
        let mount = root.join("mount");
        for path in [&home, &workspace, &mount] {
            std::fs::create_dir_all(path).unwrap();
        }
        let db = Arc::new(
            lingxi_adapters::storage::RunDatabase::open(&home.join("runs.db"), Default::default())
                .await
                .unwrap(),
        );
        let sf = Arc::new(SessionFileService::new(home.clone(), db, || 1_000));
        let resources = Arc::new(ResourceService::new(
            sf.clone(),
            "studio-test".into(),
            home.clone(),
            || 1_000,
        ));
        let access = Arc::new(ResourceAccess::new(std::slice::from_ref(&workspace)).unwrap());
        let io = ResourceIoService::new(
            sf.clone(),
            resources,
            home.clone(),
            Some((access.clone(), workspace.clone())),
            20,
            Arc::new(|| 1_000),
        );
        io.set_mount_provider(Arc::new(
            lingxi_service::resource_mounts::MountProvider::new(
                &home,
                "studio-test".into(),
                access,
            ),
        ))
        .unwrap();
        std::fs::write(home.join("studio-mounts.json"),serde_json::to_vec(&json!({"schemaVersion":1,"mounts":[{"schemaVersion":1,"mountId":"actual","hostStudioId":"studio-test","sourceKind":"storage","provider":"local_fs","rootLocator":{"path":mount},"label":"挂载","presentation":"folder","status":"active","capabilities":["read","write","list","watch","materialize"]}]})).unwrap()).unwrap();
        println!("MOUNT_PROOF {}", root.display());
        Self {
            root,
            workspace,
            mount,
            sf,
            io,
        }
    }
}
#[cfg(unix)]
#[tokio::test]
async fn actual_post_authorization_symlink_swap_never_returns_outside_bytes() {
    for mounted in [false, true] {
        let fx = Fixture::new().await;
        let victim = if mounted {
            fx.mount.join("victim.txt")
        } else {
            fx.workspace.join("victim.txt")
        };
        let outside = fx.root.join("outside.txt");
        std::fs::write(&victim, "允许内容").unwrap();
        std::fs::write(&outside, "OUTSIDE_PRIVATE_SYNTHETIC").unwrap();
        let swapped = victim.clone();
        let secret = outside.clone();
        fx.sf.set_payload_read_observer(Arc::new(move || {
            std::fs::remove_file(&swapped).unwrap();
            std::os::unix::fs::symlink(&secret, &swapped).unwrap();
        }));
        let reference = if mounted {
            json!({"kind":"mount","mountId":"actual","path":"victim.txt"})
        } else {
            json!({"kind":"local-file","path":victim})
        };
        let result = fx
            .io
            .read(&reference, &OpContext::local_owner("scope-a"))
            .await;
        assert!(result.is_err(), "真实换链接后不得返回外部字节：{result:?}");
        assert_eq!(
            std::fs::read_to_string(&outside).unwrap(),
            "OUTSIDE_PRIVATE_SYNTHETIC"
        );
    }
}

#[tokio::test]
async fn actual_registered_mount_search_root_replacement_and_revocation() {
    let fx = Fixture::new().await;
    let ctx = OpContext::local_owner("session-a");
    std::fs::create_dir_all(fx.mount.join("nested")).unwrap();
    std::fs::write(fx.mount.join("nested/find-me.txt"), "actual needle\n").unwrap();
    let reference = json!({"kind":"mount","mountId":"actual","path":""});
    let result = fx
        .io
        .search(
            &reference,
            Some("find-me".into()),
            Some("name"),
            Some(2),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(result["matches"][0]["relativePath"], "nested/find-me.txt");
    assert_eq!(result["resourceKey"], "mount:actual:");
    let target = json!({"kind":"mount","mountId":"actual","path":"nested/find-me.txt"});
    let materialized = fx.io.materialize(&target, &ctx).await.unwrap();
    assert_eq!(materialized["resource"]["kind"], "mount");
    assert_eq!(
        std::fs::read_to_string(materialized["filePath"].as_str().unwrap()).unwrap(),
        "actual needle\n"
    );
    let registry = fx.root.join("home/studio-mounts.json");
    let prior = std::fs::read(&registry).unwrap();
    let swapped = registry.clone();
    fx.sf.set_payload_read_observer(Arc::new(move || {
        let mut value: serde_json::Value = serde_json::from_slice(&prior).unwrap();
        value["mounts"][0]["status"] = json!("disabled");
        std::fs::write(&swapped, serde_json::to_vec(&value).unwrap()).unwrap();
    }));
    let err = fx.io.read(&target, &ctx).await.unwrap_err();
    assert_eq!(err.status, 404);
    assert_eq!(
        std::fs::read_to_string(fx.mount.join("nested/find-me.txt")).unwrap(),
        "actual needle\n"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn actual_mount_root_inode_replacement_during_read_is_rejected() {
    let fx = Fixture::new().await;
    let file = fx.mount.join("identity.txt");
    std::fs::write(&file, "FIRST-ROOT").unwrap();
    let old_root = fx.root.join("old-mount");
    let mount = fx.mount.clone();
    fx.sf.set_payload_read_observer(Arc::new(move || {
        std::fs::rename(&mount, &old_root).unwrap();
        std::fs::create_dir(&mount).unwrap();
        std::fs::write(mount.join("identity.txt"), "REPLACED-ROOT").unwrap();
    }));
    let err = fx
        .io
        .read(
            &json!({"kind":"mount","mountId":"actual","path":"identity.txt"}),
            &OpContext::local_owner("a"),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, "mount_root_changed");
    assert_eq!(
        std::fs::read_to_string(fx.root.join("old-mount/identity.txt")).unwrap(),
        "FIRST-ROOT"
    );
}

#[test]
fn current_cli_registered_mount_http_success_permissions_events_and_disk() {
    let root = std::env::temp_dir().join(format!(
        "r06-mounted-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let script = repository.join("tests/rust-native/r06-resource-mount-live.py");
    let output = std::process::Command::new("python3")
        .arg(script)
        .env("R06_SERVICE_BIN", env!("CARGO_BIN_EXE_lingxi-service"))
        .env("R06_REPOSITORY", &repository)
        .env(
            "R06_NODE_BIN",
            std::env::var("R06_NODE_BIN").unwrap_or_else(|_| "node".into()),
        )
        .env("R06_EX10_EVIDENCE", &root)
        .output()
        .unwrap();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        println!("mount-child: {line}");
    }
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        println!("mount-child-error: {line}");
    }
    assert!(
        output.status.success(),
        "真实挂载HTTP检查失败，证据：{}",
        root.display()
    );
    let proof: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("result.json")).unwrap()).unwrap();
    assert_eq!(proof["passed"], true);
    assert_eq!(proof["checkCount"], 16);
    assert_eq!(proof["outsideUnchanged"], true);
}

#[cfg(unix)]
#[tokio::test]
async fn actual_registered_session_file_and_resource_read_swap_never_disclose_foreign_bytes() {
    for resource in [false, true] {
        let fx = Fixture::new().await;
        let victim = fx.workspace.join("registered.txt");
        let outside = fx.root.join("private.txt");
        std::fs::write(&victim, "ALLOWED-1").unwrap();
        std::fs::write(&outside, "PRIVATE-1").unwrap();
        let view = fx
            .sf
            .register_file("registered-owner", &victim, None, None, "user_upload")
            .await
            .unwrap();
        let swapped = victim.clone();
        let secret = outside.clone();
        fx.sf.set_payload_read_observer(Arc::new(move || {
            std::fs::remove_file(&swapped).unwrap();
            std::os::unix::fs::symlink(&secret, &swapped).unwrap();
        }));
        let reference = if resource {
            json!({"kind":"resource","resourceId":format!("res_{}",view.file_id)})
        } else {
            json!({"kind":"session-file","fileId":view.file_id})
        };
        let result = fx
            .io
            .read(&reference, &OpContext::local_owner("registered-owner"))
            .await;
        assert!(
            result.is_err(),
            "登记资源在授权后换链接不得读取外部内容：{result:?}"
        );
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "PRIVATE-1");
    }
}

#[tokio::test]
async fn actual_replaced_mount_watch_generation_and_protected_state_remain_isolated() {
    let fx = Fixture::new().await;
    let reference = json!({"kind":"mount","mountId":"actual","path":""});
    let old = fx.io.watch(&reference).await.unwrap();
    std::fs::rename(&fx.mount, fx.root.join("retained-old-mount")).unwrap();
    std::fs::create_dir(&fx.mount).unwrap();
    let current = fx.io.watch(&reference).await.unwrap();
    assert!(!fx.io.unwatch(&old));
    assert_eq!(fx.io.diagnostics()["watches"].as_array().unwrap().len(), 1);
    assert_eq!(fx.io.diagnostics()["watches"][0]["refCount"], 1);
    assert!(fx.io.unwatch(&current));
    let home = fx.root.join("home");
    let protected = home.join("file-recovery-records");
    std::fs::create_dir_all(&protected).unwrap();
    std::fs::write(protected.join("hidden.txt"), "PRIVATE-RECOVERY").unwrap();
    let registry = home.join("studio-mounts.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&registry).unwrap()).unwrap();
    value["mounts"][0]["rootLocator"]["path"] = json!(home);
    std::fs::write(&registry, serde_json::to_vec(&value).unwrap()).unwrap();
    let err = fx
        .io
        .read(
            &json!({"kind":"mount","mountId":"actual","path":"file-recovery-records/hidden.txt"}),
            &OpContext::local_owner("a"),
        )
        .await
        .unwrap_err();
    assert_eq!(err.status, 403);
    let list = fx
        .io
        .list(&reference, &OpContext::local_owner("a"))
        .await
        .unwrap();
    assert!(!list["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["name"] == "file-recovery-records"));
    assert_eq!(
        std::fs::read_to_string(protected.join("hidden.txt")).unwrap(),
        "PRIVATE-RECOVERY"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn actual_trash_namespace_link_is_refused_before_source_or_outside_changes() {
    let fx = Fixture::new().await;
    let file = fx.mount.join("keep.txt");
    std::fs::write(&file, "KEEP").unwrap();
    let outside = fx.root.join("outside-trash");
    std::fs::create_dir(&outside).unwrap();
    let trash = fx.root.join("home/trash");
    std::fs::create_dir(&trash).unwrap();
    std::os::unix::fs::symlink(&outside, trash.join("redirected")).unwrap();
    let error = fx
        .io
        .trash(
            &json!({"kind":"mount","mountId":"actual","path":"keep.txt"}),
            Some("redirected"),
            None,
            &OpContext::local_owner("a"),
        )
        .await
        .unwrap_err();
    assert_eq!(error.status, 400);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "KEEP");
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn actual_case_alias_cannot_expose_protected_recovery_via_mount_or_registration() {
    let fx = Fixture::new().await;
    let home = fx.root.join("home");
    let protected = home.join("file-recovery-records");
    std::fs::create_dir_all(&protected).unwrap();
    std::fs::write(protected.join("hidden.txt"), "PRIVATE-RECOVERY").unwrap();
    let alias = home.join("FiLe-ReCoVeRy-ReCoRdS/hidden.txt");
    if !alias.exists() {
        println!("实际卷区分大小写，别名本身不可读");
        return;
    }
    let registry = home.join("studio-mounts.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&registry).unwrap()).unwrap();
    value["mounts"][0]["rootLocator"]["path"] = json!(home);
    std::fs::write(&registry, serde_json::to_vec(&value).unwrap()).unwrap();
    let read = fx
        .io
        .read(
            &json!({"kind":"mount","mountId":"actual","path":"FiLe-ReCoVeRy-ReCoRdS/hidden.txt"}),
            &OpContext::local_owner("a"),
        )
        .await;
    let registered = fx
        .sf
        .register_file("a", &alias, None, None, "user_upload")
        .await;
    assert!(
        read.is_err() && registered.is_err(),
        "大小写别名不得越过保护，实际读取：{read:?}；实际登记：{registered:?}"
    );
    assert_eq!(
        std::fs::read_to_string(protected.join("hidden.txt")).unwrap(),
        "PRIVATE-RECOVERY"
    );
}

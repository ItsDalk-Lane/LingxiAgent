use base64::{engine::general_purpose::STANDARD, Engine};
use lingxi_resource_desktop::files;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/r06-native-resource-test")
            .join(format!(
                "lingxi-desktop-files-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn real_file_read_write_copy_and_version_boundaries() {
    let temp = Scratch::new();
    let path = temp.path("中文.md");
    assert!(files::write(&path, "# 初始内容\n"));
    let before = fs::read(&path).unwrap();
    let first = files::snapshot(&path).unwrap();
    assert_eq!(first.content, "# 初始内容\n");
    assert_eq!(first.version.size, before.len() as u64);
    assert_eq!(
        STANDARD.decode(files::base64(&path).unwrap()).unwrap(),
        before
    );
    assert_eq!(fs::read(&path).unwrap(), before, "三次读取不写磁盘");
    let updated = files::save(&path, "# 新内容\n", Some(&first.version));
    assert!(updated.ok && !updated.conflict);
    let next = files::snapshot(&path).unwrap();
    assert_eq!(updated.version, Some(next.version.clone()));
    assert_eq!(fs::read_to_string(&path).unwrap(), "# 新内容\n");
    assert_ne!(first.version.sha256, next.version.sha256);
    let stale_mtime = files::FileVersion {
        mtime_ms: next.version.mtime_ms.saturating_add(1),
        ..next.version.clone()
    };
    let rejected = files::save(&path, "版本已过期", Some(&stale_mtime));
    assert!(!rejected.ok && rejected.conflict);
    assert_eq!(fs::read_to_string(&path).unwrap(), "# 新内容\n");
    fs::write(&path, "外部内容，不得覆盖").unwrap();
    let rejected = files::save(&path, "旧编辑", Some(&next.version));
    assert!(!rejected.ok && rejected.conflict);
    assert_eq!(fs::read_to_string(&path).unwrap(), "外部内容，不得覆盖");
    assert!(files::write(&path, "普通写入允许覆盖外部内容"));
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "普通写入允许覆盖外部内容"
    );
    let copied = temp.path("nested/copied.md");
    assert!(files::copy(&path, &copied));
    assert_eq!(fs::read(&path).unwrap(), fs::read(&copied).unwrap());
    assert!(fs::metadata(&path).unwrap().is_file());
    assert!(!files::copy("relative", &copied));
    assert!(!files::copy(&path, "relative"));
    assert!(!files::copy(&path, temp.0.to_str().unwrap()));
    assert!(!files::copy(
        temp.0.to_str().unwrap(),
        &temp.path("bad-dir")
    ));
    assert!(!files::copy(
        &temp.path("missing"),
        &temp.path("bad-missing")
    ));
    #[cfg(unix)]
    {
        let link = temp.path("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(!files::copy(&link, &temp.path("bad-link")));
        assert!(!std::path::Path::new(&temp.path("bad-link")).exists());
    }
    let bytes = vec![0, 1, 255, 10];
    assert!(files::write_binary(
        "~/sub/data.bin",
        &STANDARD.encode(&bytes),
        &temp.0
    ));
    assert_eq!(fs::read(temp.path("sub/data.bin")).unwrap(), bytes);
    assert!(files::snapshot(&temp.path("sub/data.bin")).is_err());
    assert!(!files::write_binary("", "YQ==", &temp.0));
    assert!(!files::write_binary("relative", "YQ==", &temp.0));
    assert!(!files::write_binary(
        &temp.path("bad-base64"),
        "%%%",
        &temp.0
    ));
    assert!(!std::path::Path::new(&temp.path("bad-base64")).exists());
    assert!(!files::write_binary(
        temp.0.to_str().unwrap(),
        "YQ==",
        &temp.0
    ));
    assert!(!files::write("relative", "bad"));
    assert!(!files::write(temp.0.to_str().unwrap(), "bad"));
    assert!(!files::save("relative", "bad", None).ok);
    assert!(!files::save(temp.0.to_str().unwrap(), "bad", None).ok);
    assert!(files::snapshot("relative").is_err());
    assert!(files::snapshot(&temp.path("missing")).is_err());
    let limit = temp.path("limit.bin");
    fs::File::create(&limit)
        .unwrap()
        .set_len(files::MAX_BINARY_BYTES)
        .unwrap();
    assert_eq!(
        STANDARD
            .decode(files::base64(&limit).unwrap())
            .unwrap()
            .len(),
        files::MAX_BINARY_BYTES as usize
    );
    fs::OpenOptions::new()
        .write(true)
        .open(&limit)
        .unwrap()
        .set_len(files::MAX_BINARY_BYTES + 1)
        .unwrap();
    assert!(files::base64(&limit).is_err());
    assert!(files::base64("relative").is_err());
    assert!(files::base64(&temp.path("missing")).is_err());
    assert!(files::open_file("relative").is_err());
    assert!(files::open_file(&temp.path("missing")).is_err());
    assert!(files::open_file(temp.0.to_str().unwrap()).is_err());
    assert!(!files::trash_item("relative"));
    assert!(!files::trash_item(&temp.path("missing")));
}

#[test]
fn edit_allowlist_refuses_system_commands() {
    for allowed in ["cut", "copy", "paste", "selectAll"] {
        assert!(files::edit_script(allowed).is_ok());
    }
    for unknown in [
        "",
        "delete",
        "open",
        "exec",
        "copy; rm -rf /",
        "document.body.innerHTML='x'",
        "Copy",
    ] {
        assert_eq!(
            files::edit_script(unknown).unwrap_err(),
            format!("Unknown edit command: {unknown}")
        );
    }
}

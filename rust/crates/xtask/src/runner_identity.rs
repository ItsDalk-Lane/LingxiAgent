//! 拒绝拿旧 xtask 可执行文件评估已变化的门禁源码或内嵌阶段图。

use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const EMBEDDED: &[(&str, &[u8])] = &[
    ("rust/crates/xtask/src/main.rs", include_bytes!("main.rs")),
    (
        "rust/crates/xtask/src/verify.rs",
        include_bytes!("verify.rs"),
    ),
    (
        "rust/crates/xtask/src/stage_map.rs",
        include_bytes!("stage_map.rs"),
    ),
    (
        "rust/crates/xtask/src/candidate.rs",
        include_bytes!("candidate.rs"),
    ),
    (
        "rust/crates/xtask/src/runner_identity.rs",
        include_bytes!("runner_identity.rs"),
    ),
    (
        "rust/crates/xtask/src/stage_maps/R02.json",
        include_bytes!("stage_maps/R02.json"),
    ),
    (
        "rust/crates/xtask/Cargo.toml",
        include_bytes!("../Cargo.toml"),
    ),
    ("rust/Cargo.toml", include_bytes!("../../../Cargo.toml")),
    (
        "rust-toolchain.toml",
        include_bytes!("../../../../rust-toolchain.toml"),
    ),
];

const SOURCE_INVENTORY: &[&str] = &[
    "main.rs",
    "verify.rs",
    "stage_map.rs",
    "candidate.rs",
    "runner_identity.rs",
    "stage_maps/R02.json",
    "verify/runner_tests.rs",
    "candidate/tests.rs",
];

pub fn check(root: &Path) -> Result<serde_json::Value, String> {
    let mut records = Vec::with_capacity(EMBEDDED.len());
    for (relative, compiled) in EMBEDDED {
        let path = root.join(relative);
        let metadata = fs::symlink_metadata(&path).map_err(|e| {
            format!(
                "runner source {} is missing or inaccessible: {e}",
                path.display()
            )
        })?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(format!(
                "runner source {} is no longer a regular file",
                path.display()
            ));
        }
        let disk = fs::read(&path)
            .map_err(|e| format!("cannot read runner source {}: {e}", path.display()))?;
        if disk != *compiled {
            return Err(format!(
                "stale xtask binary: compiled {:?} differs from current disk bytes; rebuild this runner from the candidate before stage verification",
                relative
            ));
        }
        records.push(serde_json::json!({
            "path": relative,
            "compiledAndDiskSha256": Sha256::digest(compiled).iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        }));
    }
    let source_root = root.join("rust/crates/xtask/src");
    let mut actual = BTreeSet::new();
    collect_source_files(&source_root, &source_root, &mut actual)?;
    let expected: BTreeSet<String> = SOURCE_INVENTORY.iter().map(|s| s.to_string()).collect();
    if actual != expected {
        return Err(format!(
            "stale xtask binary: source inventory differs; added={:?}, missing={:?}",
            actual.difference(&expected).collect::<Vec<_>>(),
            expected.difference(&actual).collect::<Vec<_>>()
        ));
    }
    Ok(serde_json::json!({
        "schema": "lingxi-xtask-runner-source-v1",
        "status": "PASS",
        "compiledSourceFiles": records,
        "sourceInventory": actual,
        "note": "The executable's embedded implementation and R02 stage map exactly match current disk bytes before any registered command runs.",
    }))
}

fn collect_source_files(base: &Path, dir: &Path, out: &mut BTreeSet<String>) -> Result<(), String> {
    for entry in fs::read_dir(dir)
        .map_err(|e| format!("cannot list runner sources {}: {e}", dir.display()))?
    {
        let entry = entry.map_err(|e| format!("cannot list runner source entry: {e}"))?;
        let path: PathBuf = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|e| format!("cannot inspect runner source {}: {e}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "runner source tree contains symlink {}",
                path.display()
            ));
        }
        if metadata.is_dir() {
            collect_source_files(base, &path, out)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext == "rs" || ext == "json")
        {
            let relative = path
                .strip_prefix(base)
                .map_err(|e| format!("runner source path error: {e}"))?;
            out.insert(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn old_embedded_stage_map_rejects_new_disk_map() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "lingxi-runner-identity-{}-{stamp}",
            std::process::id()
        ));
        for (relative, bytes) in EMBEDDED {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        for relative in SOURCE_INVENTORY {
            let path = root.join("rust/crates/xtask/src").join(relative);
            if !path.exists() {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, b"test only").unwrap();
            }
        }
        assert_eq!(check(&root).unwrap()["status"], "PASS");
        fs::write(
            root.join("rust/crates/xtask/src/stage_maps/R02.json"),
            b"{\"new\":true}",
        )
        .unwrap();
        let err = check(&root).unwrap_err();
        assert!(
            err.contains("stale xtask binary") && err.contains("R02.json"),
            "{err}"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

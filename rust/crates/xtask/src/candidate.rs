//! 将阶段门禁绑定到 Git 已跟踪及非忽略新增文件的实际字节。

use sha2::{Digest, Sha256};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path_hex: String,
    pub display: String,
    pub kind: &'static str,
    pub sha256: Option<String>,
    pub mode: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub digest: String,
    pub entries: Vec<Entry>,
}

impl Snapshot {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "schema": "lingxi-candidate-source-v1",
            "digestSha256": self.digest,
            "fileCount": self.entries.len(),
            "pathEncoding": if cfg!(unix) { "unix-bytes-hex" } else { "windows-utf8-hex" },
            "entries": self.entries.iter().map(|e| serde_json::json!({
                "pathBytesHex": e.path_hex,
                "display": e.display,
                "kind": e.kind,
                "sha256": e.sha256,
                "mode": e.mode,
            })).collect::<Vec<_>>(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Scope {
    excluded_relative: Option<PathBuf>,
}

impl Scope {
    pub fn new(root: &Path, evidence_root: &Path) -> Result<Self, String> {
        let root = root
            .canonicalize()
            .map_err(|e| format!("cannot canonicalize repo root: {e}"))?;
        reject_symlink_components(evidence_root)?;
        // 只排除调用者给出的字面输出路径；解析后的链接目标绝不能变成排除范围。
        let evidence = normalize_evidence_path(evidence_root)?;
        let excluded_relative = evidence.strip_prefix(&root).ok().map(Path::to_path_buf);
        if excluded_relative
            .as_ref()
            .is_some_and(|p| p.as_os_str().is_empty())
        {
            return Err(
                "--evidence cannot be the repo root: it would exclude the entire candidate".into(),
            );
        }
        Ok(Self { excluded_relative })
    }

    pub fn exclusion_json(&self) -> serde_json::Value {
        serde_json::json!({
            "evidenceOutputRelativePath": self.excluded_relative.as_ref().map(|p| p.to_string_lossy().to_string()),
            "evidenceOutputPathBytesHex": self.excluded_relative.as_ref().map(|p| hex(&os_bytes(p.as_os_str()))),
            "policy": "Only this invocation's --evidence output subtree is excluded; Git-ignored paths are outside the candidate source set. Tracked paths and non-ignored untracked paths elsewhere are included.",
        })
    }

    pub fn snapshot(&self, root: &Path) -> Result<Snapshot, String> {
        let output = Command::new("git")
            .args([
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "-z",
                "--",
            ])
            .current_dir(root)
            .output()
            .map_err(|e| format!("cannot run git ls-files for candidate binding: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "git ls-files for candidate binding failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let mut paths = Vec::<Vec<u8>>::new();
        for raw in output.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
            paths.push(raw.to_vec());
        }
        paths.sort();
        paths.dedup();
        let mut entries = Vec::with_capacity(paths.len());
        for raw in paths {
            let relative = path_from_git(&raw)?;
            if relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            {
                return Err(format!(
                    "git candidate path is not a plain relative path: {}",
                    hex(&raw)
                ));
            }
            if self
                .excluded_relative
                .as_ref()
                .is_some_and(|p| relative.starts_with(p))
            {
                continue;
            }
            let (sha256, mode) = hash_candidate_file(root, &relative)?;
            entries.push(Entry {
                path_hex: hex(&raw),
                display: relative.to_string_lossy().into_owned(),
                kind: "file",
                sha256: Some(sha256),
                mode,
            });
        }
        let mut hasher = Sha256::new();
        hasher.update(b"lingxi-candidate-source-v1\0");
        for entry in &entries {
            for value in [
                &entry.path_hex,
                entry.kind,
                entry.sha256.as_deref().unwrap_or(""),
                &entry.mode.map(|m| m.to_string()).unwrap_or_default(),
            ] {
                hasher.update((value.len() as u64).to_be_bytes());
                hasher.update(value.as_bytes());
            }
        }
        Ok(Snapshot {
            digest: hex(&hasher.finalize()),
            entries,
        })
    }
}

#[cfg(unix)]
fn hash_candidate_file(root: &Path, relative: &Path) -> Result<(String, Option<u32>), String> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::MetadataExt;

    fn open_nofollow(parent: Option<&File>, path: &Path, flags: i32) -> Result<File, String> {
        let bytes = os_bytes(path.as_os_str());
        let cpath = CString::new(bytes)
            .map_err(|_| format!("candidate path {} contains NUL", path.display()))?;
        let fd = unsafe {
            match parent {
                Some(directory) => libc::openat(directory.as_raw_fd(), cpath.as_ptr(), flags),
                None => libc::open(cpath.as_ptr(), flags),
            }
        };
        if fd < 0 {
            return Err(format!(
                "cannot securely open candidate component {}: {}",
                path.display(),
                std::io::Error::last_os_error()
            ));
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn same_path(path: &Path, opened: &File, want_directory: bool) -> Result<(), String> {
        let by_path = fs::symlink_metadata(path).map_err(|e| {
            format!(
                "candidate path {} changed or disappeared: {e}",
                path.display()
            )
        })?;
        let by_handle = opened
            .metadata()
            .map_err(|e| format!("cannot inspect open candidate {}: {e}", path.display()))?;
        if is_linklike(&by_path)
            || by_path.dev() != by_handle.dev()
            || by_path.ino() != by_handle.ino()
            || by_path.is_dir() != want_directory
            || by_handle.is_dir() != want_directory
        {
            return Err(format!(
                "candidate path {} was replaced while hashing",
                path.display()
            ));
        }
        Ok(())
    }

    let directory_flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let file_flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let mut directory = open_nofollow(None, root, directory_flags)?;
    let mut parents = vec![(
        root.to_path_buf(),
        directory
            .try_clone()
            .map_err(|e| format!("cannot hold repo root: {e}"))?,
    )];
    let mut walked = root.to_path_buf();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(format!(
                "candidate path {} is not a plain relative path",
                relative.display()
            ));
        };
        walked.push(name);
        let before = fs::symlink_metadata(&walked).map_err(|e| {
            format!(
                "candidate path {} is missing or inaccessible: {e}",
                walked.display()
            )
        })?;
        if is_linklike(&before) {
            return Err(format!(
                "candidate path {} crosses a symlink or reparse point",
                walked.display()
            ));
        }
        if components.peek().is_some() {
            if !before.is_dir() {
                return Err(format!(
                    "candidate parent {} is not a directory",
                    walked.display()
                ));
            }
            let next = open_nofollow(Some(&directory), Path::new(name), directory_flags)?;
            same_path(&walked, &next, true)?;
            parents.push((
                walked.clone(),
                next.try_clone()
                    .map_err(|e| format!("cannot hold candidate parent: {e}"))?,
            ));
            directory = next;
            continue;
        }
        if !before.is_file() {
            return Err(format!(
                "candidate file {} was replaced by a non-file",
                walked.display()
            ));
        }
        let mut file = open_nofollow(Some(&directory), Path::new(name), file_flags)?;
        same_path(&walked, &file, false)?;
        let opened_meta = file
            .metadata()
            .map_err(|e| format!("cannot inspect open candidate {}: {e}", walked.display()))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let size = file
                .read(&mut buffer)
                .map_err(|e| format!("cannot hash candidate file {}: {e}", walked.display()))?;
            if size == 0 {
                break;
            }
            hasher.update(&buffer[..size]);
        }
        same_path(&walked, &file, false)?;
        for (path, held) in &parents {
            same_path(path, held, true)?;
        }
        return Ok((hex(&hasher.finalize()), file_mode(&opened_meta)));
    }
    Err("empty candidate relative path".into())
}

#[cfg(windows)]
fn hash_candidate_file(root: &Path, relative: &Path) -> Result<(String, Option<u32>), String> {
    let mut walked = root.to_path_buf();
    let mut parents = Vec::new();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(format!("invalid candidate path {}", relative.display()));
        };
        let (held, identity) = open_windows_path(&walked, true)?;
        parents.push((walked.clone(), held, identity));
        walked.push(name);
    }
    let (mut file, original_id) = open_windows_path(&walked, false)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let size = file
            .read(&mut buffer)
            .map_err(|e| format!("cannot hash candidate file {}: {e}", walked.display()))?;
        if size == 0 {
            break;
        }
        hasher.update(&buffer[..size]);
    }
    let (_, current_id) = open_windows_path(&walked, false)?;
    if current_id != original_id {
        return Err(format!(
            "candidate file {} was replaced while hashing",
            walked.display()
        ));
    }
    for (path, held, original_id) in parents {
        let (_, current_id) = open_windows_path(&path, true)?;
        let held_id = windows_file_id(&held)?;
        if current_id != original_id || held_id != original_id {
            return Err(format!(
                "candidate parent {} was replaced while hashing",
                path.display()
            ));
        }
    }
    Ok((hex(&hasher.finalize()), None))
}

#[cfg(windows)]
fn open_windows_path(path: &Path, want_directory: bool) -> Result<(File, (u32, u64)), String> {
    use std::os::windows::fs::OpenOptionsExt;
    const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const REPARSE_ATTRIBUTE: u32 = 0x400;
    const DIRECTORY_ATTRIBUTE: u32 = 0x10;
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(OPEN_REPARSE_POINT | BACKUP_SEMANTICS)
        .open(path)
        .map_err(|e| {
            format!(
                "cannot securely open candidate component {}: {e}",
                path.display()
            )
        })?;
    let info = windows_file_info(&file)?;
    if info.dwFileAttributes & REPARSE_ATTRIBUTE != 0
        || ((info.dwFileAttributes & DIRECTORY_ATTRIBUTE != 0) != want_directory)
    {
        return Err(format!(
            "candidate component {} is a reparse point or wrong file type",
            path.display()
        ));
    }
    let id = (
        info.dwVolumeSerialNumber,
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    );
    Ok((file, id))
}

#[cfg(windows)]
fn windows_file_id(file: &File) -> Result<(u32, u64), String> {
    let info = windows_file_info(file)?;
    Ok((
        info.dwVolumeSerialNumber,
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

#[cfg(windows)]
fn windows_file_info(
    file: &File,
) -> Result<windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION, String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
    if ok == 0 {
        return Err(format!(
            "cannot identify open candidate handle: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(info)
}

pub fn differences(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let a: std::collections::BTreeMap<_, _> =
        before.entries.iter().map(|e| (&e.path_hex, e)).collect();
    let b: std::collections::BTreeMap<_, _> =
        after.entries.iter().map(|e| (&e.path_hex, e)).collect();
    a.keys()
        .chain(b.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|p| a.get(**p) != b.get(**p))
        .map(|p| (*p).clone())
        .collect()
}

pub fn snapshots_stable(
    before: &Snapshot,
    checkpoints: &[(String, u128, Result<Snapshot, String>)],
    after: &Result<Snapshot, String>,
) -> bool {
    after.as_ref().is_ok_and(|end| end.digest == before.digest)
        && checkpoints.iter().all(|(_, _, result)| {
            result
                .as_ref()
                .is_ok_and(|shot| shot.digest == before.digest)
        })
}

pub fn write_manifest(path: &Path, snapshot: &Snapshot) -> Result<String, String> {
    let mut bytes = serde_json::to_vec_pretty(&snapshot.to_json())
        .map_err(|e| format!("cannot serialize candidate manifest: {e}"))?;
    bytes.push(b'\n');
    let manifest_sha256 = hex(&Sha256::digest(&bytes));
    let mut file =
        File::create(path).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    file.write_all(&bytes)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    file.sync_all()
        .map_err(|e| format!("cannot sync {}: {e}", path.display()))?;
    Ok(manifest_sha256)
}

fn normalize_evidence_path(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("cannot read current dir: {e}"))?
            .join(path)
    };
    let mut clean = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                return Err(format!("--evidence path {} contains '..'", path.display()))
            }
            Component::CurDir => {}
            other => clean.push(other.as_os_str()),
        }
    }
    Ok(clean)
}

fn reject_symlink_components(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err(format!(
                "--evidence path {} contains '..'; use a normalized absolute path",
                path.display()
            ));
        }
        if matches!(component, Component::CurDir) {
            continue;
        }
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_linklike(&metadata) => {
                return Err(format!(
                    "--evidence path {} crosses symlink {}; refusing to exclude its resolved target from candidate binding",
                    path.display(), current.display()
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(format!(
                    "cannot inspect evidence path component {}: {e}",
                    current.display()
                ))
            }
        }
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 15) as usize] as char);
    }
    out
}

#[cfg(unix)]
fn os_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes().to_vec()
}

#[cfg(windows)]
fn os_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().flat_map(u16::to_le_bytes).collect()
}

#[cfg(unix)]
fn path_from_git(raw: &[u8]) -> Result<PathBuf, String> {
    use std::os::unix::ffi::OsStringExt;
    Ok(OsString::from_vec(raw.to_vec()).into())
}

#[cfg(windows)]
fn path_from_git(raw: &[u8]) -> Result<PathBuf, String> {
    String::from_utf8(raw.to_vec())
        .map(PathBuf::from)
        .map_err(|e| format!("git emitted an invalid UTF-8 Windows path: {e}"))
}

#[cfg(unix)]
fn file_mode(metadata: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(metadata.permissions().mode() & 0o7777)
}

#[cfg(windows)]
fn file_mode(_metadata: &fs::Metadata) -> Option<u32> {
    None
}

#[cfg(unix)]
fn is_linklike(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_linklike(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(test)]
mod tests;

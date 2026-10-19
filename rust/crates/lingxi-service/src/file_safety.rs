//! 文件恢复与版本写共用的目录句柄边界。旧检查点只描述内容，不能证明修改来源。
use crate::filetools::fsio::{DirFd, FileIdentity};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub enum WriteError {
    Conflict,
    Failed(String),
    Unknown(String),
}
impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict => write!(f, "file_changed_or_not_authorized"),
            Self::Failed(s) => write!(f, "{s}"),
            Self::Unknown(s) => write!(f, "{s}; write_may_have_occurred"),
        }
    }
}
impl From<std::io::Error> for WriteError {
    fn from(e: std::io::Error) -> Self {
        Self::Failed(e.to_string())
    }
}

pub const MAX_RECOVERABLE_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub(crate) const READ_BOUND_EXCEEDED: &str = "file_read_bound_exceeded";
const MAX_ARTIFACT_FILE_BYTES: u64 = 50 * 1024 * 1024;

/// 预算只能由正式内部入口选定，模型和普通文字接口不能传入上限。
#[derive(Clone, Copy)]
enum WriteBudget {
    Text,
    Artifact,
}
impl WriteBudget {
    fn limit(self) -> u64 {
        match self {
            Self::Text => MAX_RECOVERABLE_FILE_BYTES,
            Self::Artifact => MAX_ARTIFACT_FILE_BYTES,
        }
    }
    fn read(self, path: &Path) -> Result<Snapshot, WriteError> {
        match self {
            Self::Text => Snapshot::read(path),
            Self::Artifact => Snapshot::read_artifact(path),
        }
    }
}

pub struct Snapshot {
    dir: DirFd,
    name: std::ffi::OsString,
    pub bytes: Vec<u8>,
    pub(crate) identity: FileIdentity,
    pub stamp: Value,
}
fn stamp(file: &std::fs::File, bytes: &[u8]) -> std::io::Result<Value> {
    let m = file.metadata()?;
    if !m.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    let mut out = json!({"size":m.len(),"modified":format!("{:?}",m.modified()?),"sha256":lingxi_kernel::session_tree::sha256_hex(bytes)});
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        out["dev"] = json!(m.dev());
        out["ino"] = json!(m.ino());
        out["ctime"] = json!([m.ctime(), m.ctime_nsec()]);
    }
    Ok(out)
}
impl Snapshot {
    pub fn read(path: &Path) -> Result<Self, WriteError> {
        Self::read_bounded(path, MAX_RECOVERABLE_FILE_BYTES).map_err(|error| match error {
            WriteError::Failed(ref text) if text == READ_BOUND_EXCEEDED => WriteError::Conflict,
            other => other,
        })
    }
    /// 只扩大受权读取的字节预算；恢复和写入仍调用固定 8MiB 的 read。
    pub(crate) fn read_bounded(path: &Path, max_bytes: u64) -> Result<Self, WriteError> {
        if max_bytes > 20 * 1024 * 1024 {
            return Err(WriteError::Conflict);
        }
        Self::read_with_limit(path, max_bytes)
    }
    /// 仅用于受权办公字节快照；不放宽 read_bounded 的普通20MiB边界。
    pub(crate) fn read_artifact(path: &Path) -> Result<Self, WriteError> {
        Self::read_with_limit(path, MAX_ARTIFACT_FILE_BYTES)
    }
    fn read_with_limit(path: &Path, max_bytes: u64) -> Result<Self, WriteError> {
        // 拒绝任何路径组件已被换成链接；恢复不自动转向链接的新目标。
        if path.canonicalize().ok().as_deref() != Some(path) {
            return Err(WriteError::Conflict);
        }
        let dir = DirFd::open(path.parent().ok_or(WriteError::Conflict)?)?;
        let name = path.file_name().ok_or(WriteError::Conflict)?.to_os_string();
        let mut file = dir
            .open_attachment_nofollow(&name)
            .ok_or(WriteError::Conflict)?
            .map_err(|_| WriteError::Conflict)?;
        if !file.metadata()?.is_file() {
            return Err(WriteError::Conflict);
        }
        let identity = FileIdentity::of(&file)?;
        if identity.size > max_bytes {
            return Err(WriteError::Failed(READ_BOUND_EXCEEDED.into()));
        }
        let before_stamp = stamp(&file, &[])?;
        let mut bytes = Vec::new();
        (&mut file).take(max_bytes + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > max_bytes {
            return Err(WriteError::Failed(READ_BOUND_EXCEEDED.into()));
        }
        if FileIdentity::of(&file)? != identity || stamp(&file, &[])? != before_stamp {
            return Err(WriteError::Conflict);
        }
        let stamp = stamp(&file, &bytes)?;
        Ok(Self {
            dir,
            name,
            bytes,
            identity,
            stamp,
        })
    }
    /// 读取收据只对应同一真实文件和父目录，不能把旧字节挂到已换路径上。
    pub(crate) fn path_is_current(&self, path: &Path) -> bool {
        (|| -> Result<bool, WriteError> {
            let Some(parent) = path.parent() else {
                return Ok(false);
            };
            if path.file_name() != Some(self.name.as_os_str())
                || !self.dir.same_directory(parent)?
            {
                return Ok(false);
            }
            let Some(file) = self.dir.open_attachment_nofollow(&self.name) else {
                return Ok(false);
            };
            let file = file?;
            Ok(FileIdentity::of(&file)? == self.identity
                && stamp(&file, &self.bytes)? == self.stamp)
        })()
        .unwrap_or(false)
    }
}

pub fn witness_scope(kind: &str, subject: &str, session: &str) -> String {
    format!("{kind}\0{subject}\0{session}")
}

pub type TestHook = Arc<dyn Fn(&Path, &str) -> Result<(), String> + Send + Sync>;
pub struct FileSafety {
    root: PathBuf,
    lock: Mutex<()>,
    hook: Mutex<Option<TestHook>>,
    records: Mutex<HashMap<PathBuf, Value>>,
}
impl FileSafety {
    pub fn new(home: &Path) -> Self {
        Self {
            root: home
                .canonicalize()
                .unwrap_or_else(|_| home.to_path_buf())
                .join("file-write-witnesses"),
            lock: Mutex::new(()),
            hook: Mutex::new(None),
            records: Mutex::new(HashMap::new()),
        }
    }
    #[doc(hidden)]
    pub fn set_test_hook(&self, hook: Option<TestHook>) {
        *self.hook.lock().unwrap() = hook;
    }
    fn step(&self, path: &Path, step: &str) -> Result<(), WriteError> {
        if let Some(h) = self.hook.lock().unwrap().clone() {
            h(path, step).map_err(WriteError::Failed)?;
        }
        Ok(())
    }
    fn key(&self, session: &str, path: &Path) -> PathBuf {
        self.root.join(format!(
            "{}.json",
            lingxi_kernel::session_tree::sha256_hex(
                format!("{session}\0{}", path.display()).as_bytes()
            )
        ))
    }
    pub fn is_protected(&self, path: &Path) -> bool {
        let home = self.root.parent().and_then(|p| p.canonicalize().ok());
        home.map(|home| {
            ["file-write-witnesses", "file-recovery-records"]
                .iter()
                .any(|name| {
                    let root = home.join(name);
                    path.starts_with(&root) || root.starts_with(path)
                })
        })
        .unwrap_or(true)
            || path.components().any(|c| {
                c.as_os_str()
                    .to_string_lossy()
                    .starts_with(".lingxi-recovery-")
            })
    }
    pub fn witnessed(&self, session: &str, path: &Path, snapshot: &Snapshot) -> bool {
        (|| -> Result<Vec<u8>, WriteError> {
            let dir = self.state_dir("file-write-witnesses", false)?;
            let key = self.key(session, path);
            let mut file = dir
                .open_attachment_nofollow(key.file_name().ok_or(WriteError::Conflict)?)
                .ok_or(WriteError::Conflict)??;
            let mut bytes = Vec::new();
            (&mut file).take(64 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 64 * 1024 {
                return Err(WriteError::Conflict);
            }
            Ok(bytes)
        })()
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .map(|v| v == snapshot.stamp)
        .unwrap_or(false)
    }
    /// 写入来源证明仅由成功的应用写路径产生；读取、检查点、轮询不能产生。
    pub fn record_written(
        &self,
        session: &str,
        path: &Path,
        file: &std::fs::File,
        bytes: &[u8],
    ) -> Result<(), WriteError> {
        self.record_written_with_budget(session, path, file, bytes, WriteBudget::Text)
    }
    fn record_written_with_budget(
        &self,
        session: &str,
        path: &Path,
        file: &std::fs::File,
        bytes: &[u8],
        budget: WriteBudget,
    ) -> Result<(), WriteError> {
        let snapshot = budget.read(path)?;
        let written_stamp = stamp(file, bytes)?;
        if snapshot.bytes != bytes || snapshot.stamp != written_stamp {
            return Err(WriteError::Unknown(
                "file changed before source witness".into(),
            ));
        }
        self.record_stamp(session, path, &written_stamp)
    }
    fn record_stamp(&self, session: &str, path: &Path, stamp: &Value) -> Result<(), WriteError> {
        let dir = self.state_dir("file-write-witnesses", true)?;
        let target = self.key(session, path);
        let random =
            crate::auth::hex_random_public(8).map_err(|e| WriteError::Failed(e.to_string()))?;
        let (mut f, temp) = dir.create_temp(&random)?;
        let result = (|| -> Result<(), WriteError> {
            f.write_all(
                &serde_json::to_vec(stamp).map_err(|e| WriteError::Failed(e.to_string()))?,
            )?;
            f.sync_all()?;
            dir.rename_within(&temp, target.file_name().ok_or(WriteError::Conflict)?)?;
            dir.sync_checked()?;
            Ok(())
        })();
        dir.unlink(&temp);
        result
    }
    fn state_dir(&self, name: &str, create: bool) -> Result<DirFd, WriteError> {
        let home = self.root.parent().ok_or(WriteError::Conflict)?;
        Ok(DirFd::open(home)?.child_directory(std::ffi::OsStr::new(name), create)?)
    }
    pub fn create(
        &self,
        session: &str,
        path: &Path,
        bytes: &[u8],
        authorize: impl Fn() -> bool,
    ) -> Result<(), WriteError> {
        self.create_with_budget(session, path, bytes, authorize, WriteBudget::Text)
    }
    pub(crate) fn create_artifact(
        &self,
        session: &str,
        path: &Path,
        bytes: &[u8],
        authorize: impl Fn() -> bool,
    ) -> Result<(), WriteError> {
        self.create_with_budget(session, path, bytes, authorize, WriteBudget::Artifact)
    }
    fn create_with_budget(
        &self,
        session: &str,
        path: &Path,
        bytes: &[u8],
        authorize: impl Fn() -> bool,
        budget: WriteBudget,
    ) -> Result<(), WriteError> {
        let _guard = self.lock.lock().unwrap();
        if bytes.len() as u64 > budget.limit() || self.is_protected(path) {
            return Err(WriteError::Failed(
                "file is protected or exceeds recoverable write limit".into(),
            ));
        }
        let parent = path.parent().ok_or(WriteError::Conflict)?;
        let dir = DirFd::open(parent)?;
        let random =
            crate::auth::hex_random_public(8).map_err(|e| WriteError::Failed(e.to_string()))?;
        let (mut temp, name) = dir.create_temp(&random)?;
        let result = (|| -> Result<(), WriteError> {
            self.step(path, "temp_write")?;
            temp.write_all(bytes)?;
            self.step(path, "temp_sync")?;
            temp.sync_all()?;
            self.step(path, "before_replace")?;
            if !authorize() || !dir.same_directory(parent)? {
                return Err(WriteError::Conflict);
            }
            dir.publish_new(&name, path.file_name().ok_or(WriteError::Conflict)?)
                .map_err(|e| {
                    if e.kind() == std::io::ErrorKind::AlreadyExists {
                        WriteError::Conflict
                    } else {
                        e.into()
                    }
                })?;
            dir.sync_checked()
                .map_err(|e| WriteError::Unknown(e.to_string()))?;
            self.step(path, "after_replace")
                .map_err(|e| WriteError::Unknown(e.to_string()))?;
            // publish_new 使用硬链接。先移除临时名称，再记录来源版本；否则
            // 最后的 unlink 会改变同一 inode 的 ctime，使刚写好的证明立即失效。
            dir.unlink(&name);
            dir.sync_checked()
                .map_err(|e| WriteError::Unknown(e.to_string()))?;
            self.record_written_with_budget(session, path, &temp, bytes, budget)
                .map_err(|e| WriteError::Unknown(e.to_string()))?;
            Ok(())
        })();
        dir.unlink(&name);
        result
    }
    pub fn last_recovery(&self, path: &Path) -> Option<Value> {
        self.records.lock().unwrap().get(path).cloned()
    }
    fn journal(&self, id: &str, record: &Value) -> Result<(), WriteError> {
        let dir = self.state_dir("file-recovery-records", true)?;
        let target = std::ffi::OsString::from(format!("{id}.json"));
        let (mut f, temp) = dir.create_temp(&format!("{id}-journal"))?;
        f.write_all(&serde_json::to_vec(record).map_err(|e| WriteError::Failed(e.to_string()))?)?;
        f.sync_all()?;
        dir.rename_within(&temp, &target)?;
        dir.sync_checked()?;
        Ok(())
    }
    pub fn replace(
        &self,
        session: &str,
        path: &Path,
        snapshot: &Snapshot,
        bytes: &[u8],
        authorize: impl Fn() -> bool,
    ) -> Result<(), WriteError> {
        self.replace_with_budget(
            session,
            path,
            snapshot,
            bytes,
            authorize,
            WriteBudget::Text,
            None,
        )
    }
    /// 私密配置沿同一身份核验与恢复流程替换，仅将新文件权限收紧为0600。
    pub(crate) fn replace_private(
        &self,
        session: &str,
        path: &Path,
        snapshot: &Snapshot,
        bytes: &[u8],
        authorize: impl Fn() -> bool,
    ) -> Result<(), WriteError> {
        self.replace_with_budget(
            session,
            path,
            snapshot,
            bytes,
            authorize,
            WriteBudget::Text,
            Some(0o600),
        )
    }
    pub(crate) fn replace_artifact(
        &self,
        session: &str,
        path: &Path,
        snapshot: &Snapshot,
        bytes: &[u8],
        authorize: impl Fn() -> bool,
    ) -> Result<(), WriteError> {
        self.replace_with_budget(
            session,
            path,
            snapshot,
            bytes,
            authorize,
            WriteBudget::Artifact,
            None,
        )
    }
    // 写入预算和私密权限保留为独立约束，所有公开写入仍共用同一个原子替换流程。
    #[allow(clippy::too_many_arguments)]
    fn replace_with_budget(
        &self,
        session: &str,
        path: &Path,
        snapshot: &Snapshot,
        bytes: &[u8],
        authorize: impl Fn() -> bool,
        budget: WriteBudget,
        private_mode: Option<u32>,
    ) -> Result<(), WriteError> {
        if snapshot.bytes.len() as u64 > budget.limit() {
            return Err(WriteError::Conflict);
        }
        #[cfg(not(unix))]
        let _ = private_mode;
        let _guard = self.lock.lock().unwrap();
        if bytes.len() as u64 > budget.limit() || self.is_protected(path) {
            return Err(WriteError::Failed(
                "file is protected or exceeds recoverable write limit".into(),
            ));
        }
        self.records.lock().unwrap().remove(path);
        if !authorize() {
            return Err(WriteError::Conflict);
        }
        let random =
            crate::auth::hex_random_public(16).map_err(|e| WriteError::Failed(e.to_string()))?;
        let (mut temp, name) = snapshot.dir.create_temp(&random)?;
        let recovery_name = std::ffi::OsString::from(format!(".lingxi-recovery-{random}.original"));
        let recovery_path = path
            .parent()
            .ok_or(WriteError::Conflict)?
            .join(&recovery_name);
        let mut record = json!({"id":random,"filePath":path,"retainedPath":recovery_path,"temporaryPath":path.parent().unwrap().join(&name),"scope":session,"status":"prepared","expected":snapshot.stamp,"replacementSha256":lingxi_kernel::session_tree::sha256_hex(bytes),"writeBudgetBytes":budget.limit()});
        let mut prepared = false;
        let mut exchanged = false;
        let result = (|| -> Result<(), WriteError> {
            self.step(path, "temp_write")?;
            temp.write_all(bytes)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                self.step(path, "permission_copy")?;
                temp.set_permissions(std::fs::Permissions::from_mode(
                    private_mode.unwrap_or(snapshot.identity.mode),
                ))?;
            }
            self.step(path, "temp_sync")?;
            temp.sync_all()?;
            self.step(path, "before_replace")?;
            if !authorize()
                || !snapshot
                    .dir
                    .same_directory(path.parent().ok_or(WriteError::Conflict)?)
                    .map_err(|error| {
                        if crate::filetools::fsio::is_symlink_swap(&error) {
                            WriteError::Conflict
                        } else {
                            error.into()
                        }
                    })?
            {
                return Err(WriteError::Conflict);
            }
            let current = budget.read(path)?;
            if current.identity != snapshot.identity
                || current.stamp != snapshot.stamp
                || current.bytes != snapshot.bytes
            {
                return Err(WriteError::Conflict);
            }
            // 提交前持久登记恢复线索；任何中止都保留可恢复对象，重启不盲目重做。
            self.journal(&random, &record)?;
            prepared = true;
            snapshot.dir.rename_within(&name, &recovery_name)?;
            self.records
                .lock()
                .unwrap()
                .insert(path.to_owned(), record.clone());
            self.step(path, "commit_boundary")?;
            // 此屏障后即使外部替换，交换会保留实际被移出的对象。
            snapshot
                .dir
                .exchange_within(&recovery_name, &snapshot.name)?;
            exchanged = true;
            snapshot
                .dir
                .sync_checked()
                .map_err(|e| WriteError::Unknown(e.to_string()))?;
            let displaced = (|| -> Result<(FileIdentity, Vec<u8>, std::fs::File), WriteError> {
                let mut f = snapshot
                    .dir
                    .open_attachment_nofollow(&recovery_name)
                    .ok_or(WriteError::Conflict)?
                    .map_err(|_| WriteError::Conflict)?;
                if !f.metadata()?.is_file() {
                    return Err(WriteError::Conflict);
                }
                let identity = FileIdentity::of(&f)?;
                if identity.size > budget.limit() {
                    return Err(WriteError::Conflict);
                }
                let mut contents = Vec::new();
                (&mut f)
                    .take(budget.limit() + 1)
                    .read_to_end(&mut contents)?;
                Ok((identity, contents, f))
            })();
            let displaced_matches = displaced
                .as_ref()
                .map(|(identity, contents, _)| {
                    *identity == snapshot.identity && *contents == snapshot.bytes
                })
                .unwrap_or(false);
            if !displaced_matches || !authorize() {
                // 回换前只接受仍属于本操作的新对象；未知目标绝不删、不盲写。
                let new_current = budget.read(path).ok();
                let ours = new_current
                    .as_ref()
                    .map(|s| {
                        s.bytes == bytes && s.stamp == stamp(&temp, bytes).unwrap_or(Value::Null)
                    })
                    .unwrap_or(false);
                if ours
                    && snapshot
                        .dir
                        .same_directory(path.parent().ok_or(WriteError::Conflict)?)?
                {
                    self.step(path, "before_conflict_restore")
                        .map_err(|e| WriteError::Unknown(e.to_string()))?;
                    snapshot
                        .dir
                        .exchange_within(&recovery_name, &snapshot.name)
                        .map_err(|e| WriteError::Unknown(e.to_string()))?;
                    snapshot
                        .dir
                        .sync_checked()
                        .map_err(|e| WriteError::Unknown(e.to_string()))?;
                    // 回换也保留实际移出对象；若又有外部竞争，如实记未知，不丢任一对象。
                    let mut displaced_new = snapshot
                        .dir
                        .open_attachment_nofollow(&recovery_name)
                        .ok_or_else(|| {
                            WriteError::Unknown("conflict recovery object missing".into())
                        })?
                        .map_err(|e| WriteError::Unknown(e.to_string()))?;
                    let mut actual = Vec::new();
                    (&mut displaced_new)
                        .take(budget.limit() + 1)
                        .read_to_end(&mut actual)
                        .map_err(|e| WriteError::Unknown(e.to_string()))?;
                    if actual != bytes
                        || FileIdentity::of(&displaced_new)
                            .map_err(|e| WriteError::Unknown(e.to_string()))?
                            != FileIdentity::of(&temp)
                                .map_err(|e| WriteError::Unknown(e.to_string()))?
                    {
                        return Err(WriteError::Unknown(
                            "concurrent change preserved in recovery object".into(),
                        ));
                    }
                    record["status"] = json!("conflict_restored");
                    self.journal(&random, &record)
                        .map_err(|e| WriteError::Unknown(e.to_string()))?;
                    return Err(WriteError::Conflict);
                }
                return Err(WriteError::Unknown(
                    "external original retained; current target uncertain".into(),
                ));
            }
            #[cfg(unix)]
            if let Some(mode) = private_mode {
                use std::os::unix::fs::PermissionsExt;
                // 原件保留用于恢复，但秘密权限也必须收紧；只修改刚已核验的同一inode。
                let (_, _, original) = displaced
                    .as_ref()
                    .map_err(|error| WriteError::Unknown(error.to_string()))?;
                original
                    .set_permissions(std::fs::Permissions::from_mode(mode))
                    .map_err(|error| {
                        WriteError::Unknown(format!(
                            "retained original privacy unconfirmed: {error}"
                        ))
                    })?;
                original.sync_all().map_err(|error| {
                    WriteError::Unknown(format!(
                        "retained original privacy persistence unconfirmed: {error}"
                    ))
                })?;
                record["retainedPrivateStamp"] = stamp(original, &snapshot.bytes)
                    .map_err(|error| WriteError::Unknown(error.to_string()))?;
                record["retainedMode"] = json!(mode);
            }
            self.step(path, "after_replace")
                .map_err(|e| WriteError::Unknown(e.to_string()))?;
            self.record_written_with_budget(session, path, &temp, bytes, budget)
                .map_err(|e| WriteError::Unknown(e.to_string()))?;
            record["status"] = json!("completed_original_retained");
            self.journal(&random, &record)
                .map_err(|e| WriteError::Unknown(e.to_string()))?;
            Ok(())
        })();
        // 交换之后的任何意外错误都可能已改变目标，不能给出零副作用的失败收据。
        let result = match result {
            Err(WriteError::Failed(error)) if exchanged => Err(WriteError::Unknown(error)),
            other => other,
        };
        if prepared {
            if let Err(WriteError::Unknown(error)) = &result {
                record["status"] = json!("unknown_original_retained");
                record["detail"] = json!(error);
                let _ = self.journal(&random, &record);
            }
            if let Err(WriteError::Failed(error)) = &result {
                record["status"] = json!("failed_before_exchange_retained");
                record["detail"] = json!(error);
                let _ = self.journal(&random, &record);
            }
            self.records.lock().unwrap().insert(path.to_owned(), record);
        } else {
            snapshot.dir.unlink(&name);
        }
        // 交换移出的对象永不自动删除；持久记录指出保留位置。
        result
    }
}

#[cfg(test)]
mod artifact_budget_tests {
    use super::*;
    fn temp() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lingxi-artifact-budget-{}",
            crate::auth::hex_random_public(8).unwrap()
        ));
        std::fs::create_dir(&path).unwrap();
        path.canonicalize().unwrap()
    }
    #[test]
    fn artifact_exact_fifty_mib_succeeds_without_expanding_text_or_json_limits() {
        let root = temp();
        let safety = FileSafety::new(&root);
        let path = root.join("exact.bin");
        let bytes = vec![7; MAX_ARTIFACT_FILE_BYTES as usize];
        safety
            .create_artifact("scope", &path, &bytes, || true)
            .unwrap();
        let snapshot = Snapshot::read_artifact(&path).unwrap();
        assert_eq!(snapshot.bytes, bytes);
        assert!(safety.witnessed("scope", &path, &snapshot));
        assert!(matches!(Snapshot::read(&path), Err(WriteError::Conflict)));
        assert!(Snapshot::read_bounded(&path, 20 * 1024 * 1024).is_err());
        let text_path = root.join("ordinary.txt");
        assert!(safety
            .create(
                "scope",
                &text_path,
                &vec![0; MAX_RECOVERABLE_FILE_BYTES as usize + 1],
                || true
            )
            .is_err());
        assert!(!text_path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn artifact_over_limit_is_rejected_before_target_or_witness_creation() {
        let root = temp();
        let path = root.join("too-large.bin");
        let safety = FileSafety::new(&root);
        assert!(safety
            .create_artifact(
                "scope",
                &path,
                &vec![0; MAX_ARTIFACT_FILE_BYTES as usize + 1],
                || true
            )
            .is_err());
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_ARTIFACT_FILE_BYTES + 1)
            .unwrap();
        assert!(
            matches!(Snapshot::read_artifact(&path), Err(WriteError::Failed(ref text)) if text == READ_BOUND_EXCEEDED)
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            MAX_ARTIFACT_FILE_BYTES + 1
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn artifact_replacement_keeps_large_original_and_unknown_recovery_facts() {
        let root = temp();
        let path = root.join("replace.bin");
        let old = vec![1; 9 * 1024 * 1024];
        let new = vec![2; 10 * 1024 * 1024];
        std::fs::write(&path, &old).unwrap();
        let snapshot = Snapshot::read_artifact(&path).unwrap();
        let safety = FileSafety::new(&root);
        safety.set_test_hook(Some(Arc::new(|_, step| {
            if step == "after_replace" {
                Err("synthetic post-exchange fault".into())
            } else {
                Ok(())
            }
        })));
        assert!(matches!(
            safety.replace_artifact("scope", &path, &snapshot, &new, || true),
            Err(WriteError::Unknown(_))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), new);
        let record = safety.last_recovery(&path).unwrap();
        assert_eq!(record["writeBudgetBytes"], MAX_ARTIFACT_FILE_BYTES);
        assert_eq!(record["status"], "unknown_original_retained");
        assert_eq!(
            std::fs::read(record["retainedPath"].as_str().unwrap()).unwrap(),
            old
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn large_conflict_rollback_uses_artifact_budget_and_preserves_external_original() {
        let root = temp();
        let path = root.join("race.bin");
        let old = vec![1; 9 * 1024 * 1024];
        let replacement = vec![2; 9 * 1024 * 1024];
        let external = Arc::new(vec![3; 9 * 1024 * 1024]);
        std::fs::write(&path, &old).unwrap();
        let snapshot = Snapshot::read_artifact(&path).unwrap();
        let safety = FileSafety::new(&root);
        let changed = external.clone();
        safety.set_test_hook(Some(Arc::new(move |path, step| {
            if step == "commit_boundary" {
                std::fs::write(path, changed.as_slice()).map_err(|e| e.to_string())?;
            }
            Ok(())
        })));
        assert!(matches!(
            safety.replace_artifact("scope", &path, &snapshot, &replacement, || true),
            Err(WriteError::Conflict)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), *external);
        let record = safety.last_recovery(&path).unwrap();
        assert_eq!(record["status"], "conflict_restored");
        assert_eq!(
            std::fs::read(record["retainedPath"].as_str().unwrap()).unwrap(),
            replacement
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(all(test, unix))]
mod private_replace_tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    fn temp() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "lingxi-private-replace-{}",
            crate::auth::hex_random_public(8).unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        root.canonicalize().unwrap()
    }
    #[test]
    fn private_replace_tightens_mode_without_changing_expected_original_identity() {
        let root = temp();
        let path = root.join("config.yaml");
        std::fs::write(&path, "before").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let snapshot = Snapshot::read(&path).unwrap();
        let safety = FileSafety::new(&root);
        safety
            .replace_private("agent-config", &path, &snapshot, b"after", || true)
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"after");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let recovery = safety.last_recovery(&path).unwrap();
        let retained = recovery["retainedPath"].as_str().unwrap();
        assert_eq!(std::fs::read(retained).unwrap(), b"before");
        assert_eq!(
            std::fs::metadata(retained).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(recovery.get("retainedPrivateStamp").is_some());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn private_replace_parent_swap_is_refused_without_writing_outside_or_original() {
        let root = temp();
        let parent = root.join("agent");
        let outside = root.join("outside");
        let retained = root.join("original-agent");
        std::fs::create_dir(&parent).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let path = parent.join("config.yaml");
        std::fs::write(&path, "original").unwrap();
        std::fs::write(outside.join("config.yaml"), "outside").unwrap();
        let snapshot = Snapshot::read(&path).unwrap();
        let safety = FileSafety::new(&root);
        let original_parent = parent.clone();
        let replacement = outside.clone();
        let moved = retained.clone();
        safety.set_test_hook(Some(Arc::new(move |_, step| {
            if step == "before_replace" {
                std::fs::rename(&original_parent, &moved).map_err(|e| e.to_string())?;
                symlink(&replacement, &original_parent).map_err(|e| e.to_string())?;
            }
            Ok(())
        })));
        let actual =
            safety.replace_private("agent-config", &path, &snapshot, b"must-not-write", || true);
        assert_eq!(
            std::fs::read(outside.join("config.yaml")).unwrap(),
            b"outside"
        );
        assert_eq!(
            std::fs::read(retained.join("config.yaml")).unwrap(),
            b"original"
        );
        assert_eq!(
            std::fs::read_dir(&retained).unwrap().count(),
            1,
            "临时文件只从持有的真实目录回收"
        );
        assert!(
            matches!(actual, Err(WriteError::Conflict)),
            "parent swap result={actual:?}; evidence root={}",
            root.display()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

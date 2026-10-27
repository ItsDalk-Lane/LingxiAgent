//! 现有已迁移JSONL的可信定位、双文件移动与重启恢复。
//! 只认真实DB归属和home内header(type=session,id)，不把请求路径变成权威。
//! 真实locator收据和prepared移动计划复用session_operations，保持旧迁移指纹。
use super::{
    migrations::map_rusqlite,
    run_store::{with_write_txn, RunDatabase},
};
use lingxi_kernel::ports::StorageError;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Component, Path, PathBuf},
};
const LOCATOR: &str = "legacy_session_locator";
const MOVE: &str = "legacy_session_file_transition";
fn conflict(detail: impl Into<String>) -> StorageError {
    StorageError::Conflict {
        detail: format!("legacy_session_conflict: {}", detail.into()),
    }
}
fn io(error: impl std::fmt::Display) -> StorageError {
    StorageError::Io {
        detail: format!("legacy session file: {error}"),
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct Mark {
    digest: String,
    identity: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Move {
    from: PathBuf,
    to: PathBuf,
    mark: Mark,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Plan {
    schema: u8,
    created_at_ms: i64,
    operation: String,
    session: String,
    agent: String,
    owner: String,
    before: String,
    after: String,
    source: PathBuf,
    destination: Option<PathBuf>,
    moves: Vec<Move>,
    cleanup: Vec<(PathBuf, Mark)>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Locator {
    session: String,
    agent: String,
    owner: String,
    path: PathBuf,
}
#[derive(Clone, Default)]
pub struct Batch {
    root: Option<PathBuf>,
    plans: Vec<Plan>,
}
#[derive(Clone)]
pub struct ExpectedSource {
    pub agent: String,
    pub owner: String,
    pub path: Option<PathBuf>,
}
#[derive(Clone, Copy)]
pub enum Transition {
    Archive,
    Restore,
    Delete,
}
impl Transition {
    fn phases(self) -> (&'static str, &'static str) {
        match self {
            Self::Archive => ("active", "archived"),
            Self::Restore => ("archived", "active"),
            Self::Delete => ("archived", "deleted"),
        }
    }
}
// 所有文件访问的目录都从可信home句柄逐层no-follow打开。
struct Files {
    root: PathBuf,
}
impl Files {
    fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }
    fn relative(&self, path: &Path) -> Result<PathBuf, StorageError> {
        let rel = path
            .strip_prefix(&self.root)
            .map_err(|_| conflict("legacy session path outside trusted home"))?;
        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            return Err(conflict("legacy session path contains traversal"));
        }
        let parts: Vec<_> = rel.components().collect();
        if !(parts.len() == 4 || parts.len() == 5)
            || parts[0].as_os_str() != "agents"
            || parts[2].as_os_str() != "sessions"
            || (parts.len() == 5 && parts[3].as_os_str() != "archived")
        {
            return Err(conflict(
                "legacy session path is not an agent sessions locator",
            ));
        }
        Ok(rel.to_path_buf())
    }
    #[cfg(unix)]
    fn parent(&self, path: &Path) -> Result<(File, std::ffi::CString), StorageError> {
        use std::os::{
            fd::{AsRawFd, FromRawFd},
            unix::{ffi::OsStrExt, fs::OpenOptionsExt},
        };
        let rel = self.relative(path)?;
        let name = std::ffi::CString::new(
            rel.file_name()
                .ok_or_else(|| conflict("missing filename"))?
                .as_bytes(),
        )
        .map_err(io)?;
        let mut dir = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(&self.root)
            .map_err(io)?;
        for part in rel.parent().unwrap().components() {
            let part = std::ffi::CString::new(part.as_os_str().as_bytes()).map_err(io)?;
            // SAFETY: 目录句柄存活，NUL终止名称有效，flags明确拒绝符号链接。
            let fd = unsafe {
                libc::openat(
                    dir.as_raw_fd(),
                    part.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(io(std::io::Error::last_os_error()));
            }
            // SAFETY: openat返回的新句柄在此唯一接管。
            dir = unsafe { File::from_raw_fd(fd) };
        }
        Ok((dir, name))
    }
    #[cfg(not(unix))]
    fn check_parent(&self, path: &Path) -> Result<(), StorageError> {
        let rel = self.relative(path)?;
        let mut current = self.root.clone();
        for part in rel.parent().unwrap().components() {
            current.push(part.as_os_str());
            let meta = std::fs::symlink_metadata(&current).map_err(io)?;
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(conflict("legacy directory is a link or non-directory"));
            }
        }
        Ok(())
    }
    fn directory_exists(&self, path: &Path) -> Result<bool, StorageError> {
        let rel = self.relative(path)?;
        let mut current = self.root.clone();
        for part in rel.parent().unwrap().components() {
            current.push(part.as_os_str());
            let meta = match std::fs::symlink_metadata(&current) {
                Ok(meta) => meta,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(e) => return Err(io(e)),
            };
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(conflict(
                    "legacy session directory must not be a symlink/non-directory",
                ));
            }
        }
        Ok(true)
    }
    fn ensure_parent(&self, path: &Path) -> Result<(), StorageError> {
        if self.directory_exists(path)? {
            return Ok(());
        }
        let parent = path
            .parent()
            .ok_or_else(|| conflict("legacy parent missing"))?;
        if parent.file_name().is_none_or(|name| name != "archived") {
            return Err(conflict("only sessions/archived may be created"));
        }
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let probe = parent.with_file_name("archive-create-probe");
            let (dir, _) = self.parent(&probe)?;
            let name = std::ffi::CString::new("archived").unwrap();
            // SAFETY: 可信sessions句柄存活，唯一允许创建固定归档目录。
            if unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                return Err(io(std::io::Error::last_os_error()));
            }
            dir.sync_all().map_err(io)?;
        }
        #[cfg(not(unix))]
        {
            self.check_parent(&parent.with_file_name("archive-create-probe"))?;
            std::fs::create_dir(parent).map_err(io)?;
        }
        Ok(())
    }
    fn open(&self, path: &Path) -> Result<Option<File>, StorageError> {
        if !self.directory_exists(path)? {
            return Ok(None);
        }
        #[cfg(unix)]
        {
            use std::os::fd::{AsRawFd, FromRawFd};
            let (dir, name) = self.parent(path)?;
            // SAFETY: 保持父目录句柄存活，拒绝链接，读取仅用于身份与摘要核对。
            let fd = unsafe {
                libc::openat(
                    dir.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                )
            };
            if fd < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::NotFound {
                    return Ok(None);
                }
                return Err(io(err));
            }
            // SAFETY: 唯一接管成功返回的文件句柄。
            let file = unsafe { File::from_raw_fd(fd) };
            if !file.metadata().map_err(io)?.is_file() {
                return Err(conflict("legacy source/target must be a regular file"));
            }
            Ok(Some(file))
        }
        #[cfg(not(unix))]
        {
            self.check_parent(path)?;
            let meta = match std::fs::symlink_metadata(path) {
                Ok(meta) => meta,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(io(e)),
            };
            if meta.file_type().is_symlink() || !meta.is_file() {
                return Err(conflict(
                    "legacy source/target must be regular and not a link",
                ));
            }
            File::open(path).map(Some).map_err(io)
        }
    }
    fn mark(&self, path: &Path) -> Result<Option<Mark>, StorageError> {
        use sha2::{Digest, Sha256};
        let Some(mut file) = self.open(path)? else {
            return Ok(None);
        };
        let meta = file.metadata().map_err(io)?;
        #[cfg(unix)]
        let identity = {
            use std::os::unix::fs::MetadataExt;
            format!("{}:{}", meta.dev(), meta.ino())
        };
        #[cfg(not(unix))]
        let identity = format!("{}:{:?}", meta.len(), meta.created().map_err(io)?);
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let n = file.read(&mut buffer).map_err(io)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        let digest = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        Ok(Some(Mark { digest, identity }))
    }
    fn move_file(&self, from: &Path, to: &Path) -> Result<(), StorageError> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let (src, sn) = self.parent(from)?;
            let (dst, dn) = self.parent(to)?;
            // SAFETY: 名称与两个目录句柄均在调用期间存活；EXCL/NOREPLACE绝不覆盖并发目标。
            #[cfg(target_os = "macos")]
            let result = unsafe {
                libc::renameatx_np(
                    src.as_raw_fd(),
                    sn.as_ptr(),
                    dst.as_raw_fd(),
                    dn.as_ptr(),
                    libc::RENAME_EXCL,
                )
            };
            #[cfg(target_os = "linux")]
            let result = unsafe {
                libc::renameat2(
                    src.as_raw_fd(),
                    sn.as_ptr(),
                    dst.as_raw_fd(),
                    dn.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            };
            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            return Err(conflict(
                "atomic no-replace session rename unavailable on this OS",
            ));
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            if result != 0 {
                let error = std::io::Error::last_os_error();
                return Err(if error.kind() == std::io::ErrorKind::AlreadyExists {
                    conflict("legacy destination appeared during final rename")
                } else {
                    io(error)
                });
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            self.check_parent(from)?;
            self.check_parent(to)?;
            let src: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
            let dst: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
            // SAFETY: 两个宽字符串存活且NUL结尾；不设置REPLACE_EXISTING。
            let moved = unsafe {
                windows_sys::Win32::Storage::FileSystem::MoveFileExW(src.as_ptr(), dst.as_ptr(), 0)
            };
            if moved == 0 {
                let error = std::io::Error::last_os_error();
                return Err(if error.kind() == std::io::ErrorKind::AlreadyExists {
                    conflict("legacy destination appeared during final rename")
                } else {
                    io(error)
                });
            }
        }
        #[cfg(not(any(unix, windows)))]
        return Err(conflict("session rename unavailable on this OS"));
        self.sync_parent(from)?;
        self.sync_parent(to)?;
        Ok(())
    }
    fn remove(&self, path: &Path) -> Result<(), StorageError> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let (dir, name) = self.parent(path)?;
            // SAFETY: 目录句柄与有效名称在调用期间存活，调用者已经核对计划摘要。
            if unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) } != 0 {
                return Err(io(std::io::Error::last_os_error()));
            }
            dir.sync_all().map_err(io)?;
        }
        #[cfg(not(unix))]
        {
            self.check_parent(path)?;
            std::fs::remove_file(path).map_err(io)?;
        }
        Ok(())
    }
    fn sync_parent(&self, path: &Path) -> Result<(), StorageError> {
        #[cfg(unix)]
        {
            self.parent(path)?.0.sync_all().map_err(io)?;
        }
        #[cfg(not(unix))]
        {
            self.check_parent(path)?;
        }
        Ok(())
    }
}
fn filename_ok(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|s| s.ends_with(".jsonl") && !s.contains(".repair.jsonl"))
}
fn header(files: &Files, path: &Path) -> Result<String, StorageError> {
    let file = files
        .open(path)?
        .ok_or_else(|| StorageError::InvalidRequest {
            detail: "legacy_session_file_not_found".into(),
        })?;
    let mut first = String::new();
    BufReader::new(file).read_line(&mut first).map_err(io)?;
    let value: serde_json::Value = serde_json::from_str(&first)
        .map_err(|e| conflict(format!("invalid session header: {e}")))?;
    if value["type"] != "session" {
        return Err(conflict("legacy JSONL header is not a session"));
    }
    value["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| conflict("legacy session header has no id"))
}
fn header_only(
    conn: &Connection,
    files: &Files,
    path: &Path,
    sid: &str,
) -> Result<bool, StorageError> {
    let target_id = header(files, path)?;
    // 旧bug可能留下新铸造的孤立header；真实其他会话的空头仍受DB归属保护。
    if target_id != sid && row(conn, &target_id)?.is_some() {
        return Err(conflict(
            "active restore target belongs to another real DB session",
        ));
    }
    for line in BufReader::new(files.open(path)?.unwrap()).lines() {
        let line = line.map_err(io)?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: serde_json::Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => return Ok(false),
        };
        if entry["type"] == "message" && !entry["message"].is_null() {
            return Ok(false);
        }
    }
    Ok(true)
}
fn row(conn: &Connection, sid: &str) -> Result<Option<(String, String, String)>, StorageError> {
    conn.query_row(
        "SELECT agent_id,owner_user_id,lifecycle FROM sessions WHERE session_id=?1",
        [sid],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()
    .map_err(map_rusqlite)
}
fn locator(conn: &Connection, sid: &str) -> Result<Option<Locator>, StorageError> {
    let text:Option<String>=conn.query_row("SELECT receipt_json FROM session_operations WHERE operation_kind=?1 AND operation_id=?2 AND state='completed'",rusqlite::params![LOCATOR,format!("legacy-locator:{sid}")],|r|r.get(0)).optional().map_err(map_rusqlite)?;
    text.map(|s| {
        serde_json::from_str(&s)
            .map_err(|e| conflict(format!("invalid durable legacy locator: {e}")))
    })
    .transpose()
}
fn validate_agent(agent: &str) -> bool {
    !agent.is_empty()
        && agent
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
fn locate(
    conn: &Connection,
    files: &Files,
    sid: &str,
    agent: &str,
    owner: &str,
    life: &str,
) -> Result<Option<PathBuf>, StorageError> {
    if let Some(loc) = locator(conn, sid)? {
        if loc.session != sid || loc.agent != agent || loc.owner != owner {
            return Err(conflict("legacy locator disagrees with DB owner/agent"));
        }
        files.relative(&loc.path)?;
        let expected = files.root.join("agents").join(agent).join("sessions");
        let parent = if life == "archived" {
            expected.join("archived")
        } else {
            expected
        };
        if loc.path.parent() != Some(parent.as_path()) || header(files, &loc.path)? != sid {
            return Err(conflict("legacy locator lifecycle/header disagreement"));
        }
        return Ok(Some(loc.path));
    }
    if !validate_agent(agent) {
        return Ok(None);
    } // 无文件落点的旧DB种子仍是纯DB会话。
    let sessions = files.root.join("agents").join(agent).join("sessions");
    let dir = if life == "archived" {
        sessions.join("archived")
    } else {
        sessions
    };
    // 缺少整套旧目录表示DB-only；存在目录则所有祖先必须通过no-follow访问。
    if !files.directory_exists(&dir.join("locator-probe.jsonl"))? {
        return Ok(None);
    }
    let mut found = None;
    for entry in std::fs::read_dir(&dir).map_err(io)? {
        let path = entry.map_err(io)?.path();
        if !filename_ok(&path) {
            continue;
        }
        if header(files, &path)? == sid {
            if found.is_some() {
                return Err(conflict("multiple legacy JSONL files claim this session"));
            }
            found = Some(path);
        }
    }
    Ok(found)
}
/// path只用于读取可信header并解析id；随后HTTP仍必须走真实DB归属闸。
pub async fn session_id_for_path(db: &RunDatabase, path: &Path) -> Result<String, StorageError> {
    let root = db
        .legacy_session_root()
        .ok_or_else(|| conflict("legacy home unavailable"))?
        .to_path_buf();
    let path = path.to_path_buf();
    db.queue()
        .submit(move |conn| {
            let files = Files::new(&root);
            files.relative(&path)?;
            if !filename_ok(&path) {
                return Err(conflict("invalid session JSONL filename"));
            }
            let sid = header(&files, &path)?;
            let (agent, owner, life) =
                row(conn, &sid)?.ok_or_else(|| StorageError::InvalidRequest {
                    detail: "legacy_session_file_not_found".into(),
                })?;
            let found = locate(conn, &files, &sid, &agent, &owner, &life)?
                .ok_or_else(|| conflict("path has no authoritative DB session locator"))?;
            if found != path {
                return Err(conflict(
                    "supplied sessionPath disagrees with trusted locator",
                ));
            }
            Ok(sid)
        })
        .await
}
fn add_move(
    files: &Files,
    moves: &mut Vec<Move>,
    from: PathBuf,
    to: PathBuf,
) -> Result<Mark, StorageError> {
    if files.mark(&to)?.is_some() {
        return Err(conflict("legacy session move destination already exists"));
    }
    let mark = files.mark(&from)?.ok_or_else(|| io("source disappeared"))?;
    moves.push(Move {
        from,
        to,
        mark: mark.clone(),
    });
    Ok(mark)
}
impl Batch {
    /// 在任何物理移动之前持久保存计划；所有归属事实来自同一真实DB行。
    pub async fn prepare(
        db: &RunDatabase,
        ids: &[String],
        transition: Transition,
        now: u64,
    ) -> Result<Self, StorageError> {
        Self::prepare_checked(db, ids, transition, now, None).await
    }
    pub async fn prepare_checked(
        db: &RunDatabase,
        ids: &[String],
        transition: Transition,
        now: u64,
        expected: Option<ExpectedSource>,
    ) -> Result<Self, StorageError> {
        let Some(root) = db.legacy_session_root().map(Path::to_path_buf) else {
            return Ok(Self::default());
        };
        let ids = ids.to_vec();
        let busy = db.options().busy_timeout_ms;
        db.queue().submit(move |conn|with_write_txn(conn,busy,|conn|{
            let files=Files::new(&root);let (before,after)=transition.phases();let mut plans=Vec::new();
            for sid in ids {
                // 运行中的其他批次不是遗留操作；同会话未收尾时拒绝重入，留待原批或重启恢复。
                let pending: bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM session_operations WHERE session_id=?1 AND operation_kind=?2 AND state='prepared')",rusqlite::params![sid,MOVE],|r|r.get(0)).map_err(map_rusqlite)?;
                if pending {return Err(conflict("legacy session operation still pending; no recovery attempted during admission"));}
                let Some((agent,owner,life))=row(conn,&sid)? else{continue};
                if expected.as_ref().is_some_and(|e|e.agent!=agent||e.owner!=owner){return Err(conflict("authorized restore owner/agent changed before preparation"));}
                if life!=before {continue;}
                let source=locate(conn,&files,&sid,&agent,&owner,&life)?;
                if let Some(path)=expected.as_ref().and_then(|e|e.path.as_ref()) {if source.as_ref()!=Some(path){return Err(conflict("authorized restore path changed before preparation"));}}
                let Some(source)=source else{continue};
                let operation=format!("legacy-move:{}:{}:{}",sid,std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(io)?.as_nanos());
                let name=source.file_name().unwrap();let sessions=root.join("agents").join(&agent).join("sessions");
                let destination=match transition {Transition::Archive=>Some(sessions.join("archived").join(name)),Transition::Restore=>Some(sessions.join(name)),Transition::Delete=>None};
                let stage=source.with_file_name(format!("{}.lifecycle-{}",name.to_string_lossy(),lingxi_protocol::canon::sha256_hex(operation.as_bytes())));
                let dest=destination.clone().unwrap_or(stage);
                let mut moves=Vec::new();let mut cleanup=Vec::new();
                // 只修复同一会话、无真实消息的旧活跃文件；其sidecar同样先备份。
                if matches!(transition,Transition::Restore) && files.mark(&dest)?.is_some() {
                    if !header_only(conn,&files,&dest,&sid)? {return Err(conflict("active restore target already contains messages"));}
                    let backup=dest.with_file_name(format!("{}.header-backup-{}",name.to_string_lossy(),lingxi_protocol::canon::sha256_hex(operation.as_bytes())));
                    let mark=add_move(&files,&mut moves,dest.clone(),backup.clone())?;cleanup.push((backup,mark));
                    let side=sidecar(&dest);
                    if files.mark(&side)?.is_some() {
                        let backup=sidecar(&cleanup[0].0);let mark=add_move(&files,&mut moves,side,backup.clone())?;cleanup.push((backup,mark));
                    }
                } else if files.mark(&dest)?.is_some() {return Err(conflict("legacy session destination already exists"));}
                if files.mark(&sidecar(&dest))?.is_some() && !moves.iter().any(|m|m.from==sidecar(&dest)) {return Err(conflict("legacy sidecar destination already exists"));}
                let mark=files.mark(&source)?.ok_or_else(||io("session source missing"))?;
                moves.push(Move{from:source.clone(),to:dest.clone(),mark:mark.clone()});
                if matches!(transition,Transition::Delete) {cleanup.push((dest.clone(),mark));}
                let side=sidecar(&source);
                if let Some(mark)=files.mark(&side)? {
                    let to=sidecar(&dest);moves.push(Move{from:side,to:to.clone(),mark:mark.clone()});
                    if matches!(transition,Transition::Delete) {cleanup.push((to,mark));}
                }
                let plan=Plan{schema:1,created_at_ms:if now==0 {std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(io)?.as_millis() as i64}else{now as i64},operation,session:sid.clone(),agent,owner,before:before.into(),after:after.into(),source,destination,moves,cleanup};
                let text=serde_json::to_string(&plan).map_err(io)?;
                conn.execute("INSERT INTO session_operations(operation_id,session_id,owner_kind,owner_subject,operation_kind,input_digest,state,file_effects,snapshot_json,created_at_unix_ms) VALUES(?1,?2,'session_owner',?3,?4,?5,'prepared',1,?6,?7)",rusqlite::params![plan.operation,sid,plan.owner,MOVE,lingxi_protocol::canon::sha256_hex(text.as_bytes()),text,plan.created_at_ms]).map_err(map_rusqlite)?;
                plans.push(plan);
            }
            Ok(Self{root:Some(root.clone()),plans})
        })).await
    }
    /// body仅含真实DB业务逻辑。提交前后失败都倒序保全文件，计划已在前一事务持久化。
    pub(crate) fn transaction<T>(
        &self,
        conn: &Connection,
        busy: u64,
        body: impl FnOnce(&Connection) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        if self.plans.is_empty() {
            return with_write_txn(conn, busy, body);
        }
        let files = Files::new(self.root.as_ref().unwrap());
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| super::migrations::map_rusqlite_busy(e, busy))?;
        let mut attempted = Vec::new();
        let result = (|| {
            for plan in &self.plans {
                verify_prepared(conn, plan)?;
                verify_row(conn, plan, &plan.before)?;
                verify_plan(&files, plan)?;
            }
            // 先执行DB校验和暂存；业务拒绝时尚未移动文件。
            let result = body(conn)?;
            for plan in &self.plans {
                let actual = row(conn, &plan.session)?
                    .map(|r| r.2)
                    .unwrap_or_else(|| "deleted".into());
                if actual != plan.after {
                    return Err(conflict(
                        "legacy lifecycle physical/DB write set disagreement",
                    ));
                }
                for item in &plan.moves {
                    if files.mark(&item.from)? != Some(item.mark.clone()) {
                        return Err(conflict(
                            "legacy source changed while database commit waited",
                        ));
                    }
                    files.ensure_parent(&item.to)?;
                    attempted.push(item.clone());
                    files.move_file(&item.from, &item.to)?;
                    if files.mark(&item.to)? != Some(item.mark.clone()) {
                        return Err(conflict("legacy moved file identity changed"));
                    }
                }
                if let Some(path) = &plan.destination {
                    write_locator(conn, plan, path)?;
                } else {
                    conn.execute("DELETE FROM session_operations WHERE operation_kind=?1 AND operation_id=?2",rusqlite::params![LOCATOR,format!("legacy-locator:{}",plan.session)]).map_err(map_rusqlite)?;
                }
            }
            conn.execute_batch("COMMIT")
                .map_err(|e| super::migrations::map_rusqlite_busy(e, busy))?;
            Ok(result)
        })();
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                let undo = undo_moves(&files, &attempted);
                let db_undo = conn.execute_batch("ROLLBACK").map_err(map_rusqlite);
                match (undo,db_undo) {
                    (Ok(()),Ok(()))=> {
                        // 已证明物理与DB均已回滚，保存失败收据并解除prepared阻断。
                        with_write_txn(conn,busy,|conn| {
                            for plan in &self.plans {
                                verify_prepared(conn, plan)?;
                                conn.execute("UPDATE session_operations SET state='completed',receipt_json=?2,completed_at_unix_ms=created_at_unix_ms WHERE operation_id=?1 AND state='prepared'",rusqlite::params![plan.operation,serde_json::json!({"ok":false,"rolledBack":true,"sessionId":plan.session,"error":error.to_string()}).to_string()]).map_err(map_rusqlite)?;
                            }
                            Ok(())
                        }).map_err(|cleanup|io(format!("operation refused ({error}); rollback confirmed but journal completion pending: {cleanup}")))?;
                        Err(error)
                    },
                    (fs,db)=>Err(io(format!("operation refused ({error}); recovery required: file rollback={fs:?}, DB rollback={db:?}"))),
                }
            }
        }
    }
    /// 已提交后的临时备份/删除收尾失败保留prepared计划，返回明确待恢复错误。
    pub async fn finish(&self, db: &RunDatabase) -> Result<(), StorageError> {
        let Some(root) = self.root.clone() else {
            return Ok(());
        };
        let busy = db.options().busy_timeout_ms;
        let committed = !self.plans.is_empty();
        let plans = self.plans.clone();
        db.queue()
            .submit(move |conn| with_write_txn(conn, busy, |conn| {
                let files = Files::new(&root);
                for plan in &plans {
                    // 只收尾本批已提交的操作，不能按before生命周期撤销另一仍活跃批次。
                    verify_prepared(conn, plan)?;
                    recover_plan(conn, &files, plan, false)?;
                }
                Ok(())
            }))
            .await
            .map_err(|error| {
                io(if committed {format!("legacy lifecycle commit succeeded; physical cleanup/recovery pending: {error}")}
                   else {format!("previous legacy file recovery pending: {error}")})
            })
    }
}
fn sidecar(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.files.json", path.display()))
}
fn verify_row(conn: &Connection, plan: &Plan, phase: &str) -> Result<(), StorageError> {
    let actual = row(conn, &plan.session)?;
    if phase == "deleted" && actual.is_none() {
        return Ok(());
    }
    if actual != Some((plan.agent.clone(), plan.owner.clone(), phase.into())) {
        return Err(conflict(
            "legacy session owner/agent/lifecycle changed before final transaction",
        ));
    }
    Ok(())
}
fn verify_plan(files: &Files, plan: &Plan) -> Result<(), StorageError> {
    if plan.schema != 1 {
        return Err(conflict(
            "unsupported legacy journal schema; no file touched",
        ));
    }
    if !validate_agent(&plan.agent) {
        return Err(conflict("legacy plan invalid agent"));
    }
    let sessions = files.root.join("agents").join(&plan.agent).join("sessions");
    let srcdir = if plan.before == "archived" {
        sessions.join("archived")
    } else {
        sessions.clone()
    };
    if plan.source.parent() != Some(srcdir.as_path()) || !filename_ok(&plan.source) {
        return Err(conflict("legacy plan source boundary invalid"));
    }
    if let Some(dest) = &plan.destination {
        let destdir = if plan.after == "archived" {
            sessions.join("archived")
        } else {
            sessions
        };
        if dest.parent() != Some(destdir.as_path()) || dest.file_name() != plan.source.file_name() {
            return Err(conflict("legacy plan destination boundary invalid"));
        }
    }
    let tag = lingxi_protocol::canon::sha256_hex(plan.operation.as_bytes());
    let name = plan.source.file_name().unwrap().to_string_lossy();
    let dest = plan.destination.clone().unwrap_or_else(|| {
        plan.source
            .with_file_name(format!("{name}.lifecycle-{tag}"))
    });
    let backup = dest.with_file_name(format!("{name}.header-backup-{tag}"));
    for item in &plan.moves {
        files.relative(&item.from)?;
        files.relative(&item.to)?;
        let ordinary = (item.from == plan.source && item.to == dest)
            || (item.from == sidecar(&plan.source) && item.to == sidecar(&dest));
        let repair = plan.before == "archived"
            && plan.after == "active"
            && ((item.from == dest && item.to == backup)
                || (item.from == sidecar(&dest) && item.to == sidecar(&backup)));
        if !ordinary && !repair {
            return Err(conflict(
                "legacy journal move is not derived from the authoritative session",
            ));
        }
    }
    if !plan
        .moves
        .iter()
        .any(|item| item.from == plan.source && item.to == dest)
    {
        return Err(conflict("legacy journal has no primary JSONL move"));
    }
    for (path, mark) in &plan.cleanup {
        if !plan
            .moves
            .iter()
            .any(|item| &item.to == path && &item.mark == mark)
            || !(path == &backup || path == &sidecar(&backup) || plan.after == "deleted")
        {
            return Err(conflict(
                "legacy journal cleanup is not a derived staging file",
            ));
        }
    }
    for (path, _) in &plan.cleanup {
        files.relative(path)?;
    }
    Ok(())
}
fn undo_plan(files: &Files, plan: &Plan) -> Result<(), StorageError> {
    verify_plan(files, plan)?;
    undo_moves(files, &plan.moves)
}
fn undo_moves(files: &Files, moves: &[Move]) -> Result<(), StorageError> {
    let mut failures = Vec::new();
    for item in moves.iter().rev() {
        let undo = (|| {
            match files.mark(&item.from)? {
                Some(mark) if mark == item.mark => return Ok(()), // 本步未移动或已撤回，异物目标原样保留。
                Some(_) => return Err(conflict("legacy rollback source changed; not overwritten")),
                None => {}
            }
            if files.mark(&item.to)? != Some(item.mark.clone()) {
                return Err(conflict(
                    "legacy rollback cannot prove file identity; journal retained",
                ));
            }
            files.move_file(&item.to, &item.from)
        })();
        if let Err(error) = undo {
            failures.push(error.to_string());
        }
    }
    if !failures.is_empty() {
        return Err(io(failures.join("; ")));
    }
    Ok(())
}
fn write_locator(conn: &Connection, plan: &Plan, path: &Path) -> Result<(), StorageError> {
    let loc = Locator {
        session: plan.session.clone(),
        agent: plan.agent.clone(),
        owner: plan.owner.clone(),
        path: path.to_path_buf(),
    };
    conn.execute("INSERT INTO session_operations(operation_id,session_id,owner_kind,owner_subject,operation_kind,input_digest,state,file_effects,snapshot_json,receipt_json,created_at_unix_ms,completed_at_unix_ms) VALUES(?1,?2,'session_owner',?3,?4,'validated-header','completed',0,'{}',?5,?6,?6) ON CONFLICT(operation_id) DO UPDATE SET receipt_json=excluded.receipt_json",rusqlite::params![format!("legacy-locator:{}",plan.session),plan.session,plan.owner,LOCATOR,serde_json::to_string(&loc).map_err(io)?,plan.created_at_ms]).map_err(map_rusqlite)?;
    Ok(())
}
/// 最后写事务和本批收尾都必须持有原始、仍prepared的完整日志，失去凭据时禁止物理动作。
fn verify_prepared(conn: &Connection, plan: &Plan) -> Result<(), StorageError> {
    let text = serde_json::to_string(plan).map_err(io)?;
    let digest = lingxi_protocol::canon::sha256_hex(text.as_bytes());
    let valid: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM session_operations WHERE operation_id=?1 AND session_id=?2 AND owner_kind='session_owner' AND owner_subject=?3 AND operation_kind=?4 AND state='prepared' AND file_effects=1 AND snapshot_json=?5 AND input_digest=?6)",rusqlite::params![plan.operation,plan.session,plan.owner,MOVE,text,digest],|r|r.get(0)).map_err(map_rusqlite)?;
    if !valid {
        return Err(conflict(
            "legacy prepared journal missing/changed; no file touched",
        ));
    }
    Ok(())
}
fn recover_plan(
    conn: &Connection,
    files: &Files,
    plan: &Plan,
    allow_undo: bool,
) -> Result<(), StorageError> {
    verify_plan(files, plan)?;
    let actual = row(conn, &plan.session)?;
    let final_phase = actual.as_ref().map(|r| r.2.as_str()).unwrap_or("deleted");
    if allow_undo && final_phase == plan.before {
        verify_row(conn, plan, &plan.before)?;
        undo_plan(files, plan)?;
    } else if final_phase == plan.after {
        verify_row(conn, plan, &plan.after)?;
        // 已提交移动必须仍是同一原文件；绝不重做未确认的来源动作。
        for item in &plan.moves {
            if plan.cleanup.iter().any(|(path, _)| path == &item.to) {
                continue;
            }
            if files.mark(&item.to)? != Some(item.mark.clone()) {
                return Err(conflict(
                    "committed legacy destination missing/changed; recovery journal retained",
                ));
            }
        }
        for (path, mark) in &plan.cleanup {
            if let Some(actual) = files.mark(path)? {
                if &actual != mark {
                    return Err(conflict(
                        "legacy cleanup file identity changed; not deleted",
                    ));
                }
                files.remove(path)?;
            }
        }
    } else {
        return Err(conflict(
            "legacy journal lifecycle cannot be reconciled; no file touched",
        ));
    }
    conn.execute("UPDATE session_operations SET state='completed',receipt_json=?2,completed_at_unix_ms=created_at_unix_ms WHERE operation_id=?1 AND state='prepared'",rusqlite::params![plan.operation,serde_json::json!({"sessionId":plan.session,"lifecycle":final_phase,"restoredPath":plan.destination,"recovered":true}).to_string()]).map_err(map_rusqlite)?;
    Ok(())
}
fn recover_conn(conn: &Connection, root: &Path) -> Result<(), StorageError> {
    let files = Files::new(root);
    let plans: Vec<(String, String, String)> = {
        let mut stmt=conn.prepare("SELECT operation_id,snapshot_json,input_digest FROM session_operations WHERE operation_kind=?1 AND state='prepared' ORDER BY created_at_unix_ms,operation_id").map_err(map_rusqlite)?;
        let rows = stmt
            .query_map([MOVE], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(map_rusqlite)?
            .collect::<Result<_, _>>()
            .map_err(map_rusqlite)?;
        rows
    };
    for (id, text, digest) in plans {
        if lingxi_protocol::canon::sha256_hex(text.as_bytes()) != digest {
            return Err(conflict("legacy recovery journal digest mismatch"));
        }
        let plan: Plan = serde_json::from_str(&text).map_err(io)?;
        if id != plan.operation {
            return Err(conflict("legacy recovery journal identity mismatch"));
        }
        verify_prepared(conn, &plan)?;
        recover_plan(conn, &files, &plan, true)?;
    }
    Ok(())
}
/// 组合根在接受HTTP/Run前调用；不确定/外部修改保持文件并显式启动失败。
pub async fn recover_pending(db: &RunDatabase) -> Result<(), StorageError> {
    let Some(root) = db.legacy_session_root().map(Path::to_path_buf) else {
        return Ok(());
    };
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| with_write_txn(conn, busy, |conn| recover_conn(conn, &root)))
        .await
}
pub async fn restored_path(db: &RunDatabase, sid: &str) -> Result<Option<PathBuf>, StorageError> {
    let sid = sid.to_string();
    db.queue()
        .submit(move |conn| Ok(locator(conn, &sid)?.map(|loc| loc.path)))
        .await
}

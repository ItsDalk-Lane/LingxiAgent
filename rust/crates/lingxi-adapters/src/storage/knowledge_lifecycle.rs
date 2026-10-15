//! 保留期限只回收派生缓存；孤儿原文另受七天期限和实际引用保护。
use super::*;

pub(super) fn open_root_guard(root: &Path) -> Result<File, KnowledgeError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS);
    }
    options.open(root).map_err(storage_io)
}

#[cfg(unix)]
fn directory_at(parent: &File, name: &std::ffi::CStr) -> std::io::Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    // SAFETY: 名称与父目录句柄均存活；成功句柄只交给 File 一次。
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

#[cfg(unix)]
fn cleanup_snapshot_files(_root: &Path, root_guard: &File, id: &str) -> Result<(), KnowledgeError> {
    use std::os::fd::AsRawFd;
    let root = root_guard.try_clone().map_err(storage_io)?;
    let name =
        std::ffi::CString::new(id).map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
    let snapshots = directory_at(&root, c"snapshots").map_err(storage_io)?;
    cleanup_snapshot_outputs(&root, &name)?;
    // SAFETY: 相对可信目录句柄移除单个名字，不跟随末端链接。
    if unsafe { libc::unlinkat(snapshots.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(storage_io(error));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn cleanup_snapshot_outputs(root: &File, name: &std::ffi::CStr) -> Result<(), KnowledgeError> {
    use std::os::fd::{AsRawFd, IntoRawFd};
    let artifacts = match directory_at(root, c"artifacts") {
        Ok(dir) => dir,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(storage_io(e)),
    };
    let output = match directory_at(&artifacts, name) {
        Ok(dir) => dir,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(storage_io(e)),
    };
    let enumerate = directory_at(&output, c".").map_err(storage_io)?;
    // SAFETY: fdopendir 成功后独占接管这个独立描述符。
    let fd = enumerate.into_raw_fd();
    let raw = unsafe { libc::fdopendir(fd) };
    if raw.is_null() {
        let e = std::io::Error::last_os_error();
        unsafe { libc::close(fd) };
        return Err(storage_io(e));
    }
    struct Stream(*mut libc::DIR);
    impl Drop for Stream {
        fn drop(&mut self) {
            unsafe { libc::closedir(self.0) };
        }
    }
    let stream = Stream(raw);
    let mut count = 0usize;
    loop {
        // SAFETY: stream 活着，当前条目在下一次 readdir 前立即复制并使用。
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            break;
        }
        let child = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
        if child == c"." || child == c".." {
            continue;
        }
        count += 1;
        if count > 4096 {
            return Err(KnowledgeError::storage_invalid(
                "snapshot output cleanup exceeds file bound",
            ));
        }
        // 产物目录按合同只有平面文件；若被换成目录则 unlinkat 拒绝，绝不递归外走。
        if unsafe { libc::unlinkat(output.as_raw_fd(), child.as_ptr(), 0) } != 0 {
            return Err(storage_io(std::io::Error::last_os_error()));
        }
    }
    if unsafe { libc::unlinkat(artifacts.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
        return Err(storage_io(std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(not(unix))]
fn cleanup_snapshot_files(root: &Path, _root_guard: &File, id: &str) -> Result<(), KnowledgeError> {
    let canonical = std::fs::canonicalize(root).map_err(storage_io)?;
    for dir in ["artifacts", "snapshots"] {
        let parent = root.join(dir);
        if !parent.exists() {
            continue;
        }
        if std::fs::canonicalize(&parent).map_err(storage_io)? != canonical.join(dir) {
            return Err(KnowledgeError::storage_invalid(
                "knowledge cleanup parent changed",
            ));
        }
        let path = parent.join(id);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {
                std::fs::remove_dir_all(path).map_err(storage_io)?
            }
            Ok(_) => std::fs::remove_file(path).map_err(storage_io)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(storage_io(e)),
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct VectorUsage {
    pub variant_id: String,
    pub artifact_id: String,
    pub indexed_at_ms: i64,
    pub last_used_at_ms: i64,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct ArtifactRetention {
    pub artifact_id: String,
    pub source_id: String,
    pub latest: bool,
    pub retention_days: Option<i64>,
    pub protected: bool,
}

// 附加表兼容现有缓存 v1，不重写身份、向量或已付费检查点。旧记录从迁移时起
// 开始计时，不能把未知的历史使用时间当成已经过期。
pub(super) fn initialize_vector_usage(conn: &Connection, now: i64) -> Result<(), KnowledgeError> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(storage_sqlite)?;
    check_user_version(&tx, VECTOR_SCHEMA_VERSION)?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS knowledge_vector_usage (variant_id TEXT PRIMARY KEY REFERENCES vector_index_variants(id) ON DELETE CASCADE, metadata_version INTEGER NOT NULL CHECK(metadata_version=1), indexed_at_ms INTEGER NOT NULL, last_used_at_ms INTEGER NOT NULL);") .map_err(storage_sqlite)?;
    let unknown: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_vector_usage WHERE metadata_version<>1 OR indexed_at_ms IS NULL OR last_used_at_ms IS NULL)", [], |r| r.get(0)).map_err(storage_sqlite)?;
    if unknown {
        return Err(KnowledgeError::storage_invalid(
            "unknown vector usage metadata; original preserved",
        ));
    }
    tx.execute(
        "INSERT OR IGNORE INTO knowledge_vector_usage SELECT id,1,?1,?1 FROM vector_index_variants",
        params![now],
    )
    .map_err(storage_sqlite)?;
    tx.commit().map_err(storage_sqlite)
}

pub(super) fn record_vector_use(
    conn: &Connection,
    id: &str,
    now: i64,
) -> Result<(), KnowledgeError> {
    conn.execute("INSERT INTO knowledge_vector_usage SELECT id,1,?2,?2 FROM vector_index_variants WHERE id=?1 ON CONFLICT(variant_id) DO UPDATE SET last_used_at_ms=MAX(knowledge_vector_usage.last_used_at_ms,excluded.last_used_at_ms)", params![id, now]).map_err(storage_sqlite)?;
    Ok(())
}

impl KnowledgeVectorStore {
    pub fn list_usage(&self) -> Result<Vec<VectorUsage>, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let mut stmt = conn.prepare("SELECT v.id,v.parse_artifact_id,u.indexed_at_ms,u.last_used_at_ms,v.status FROM vector_index_variants v JOIN knowledge_vector_usage u ON u.variant_id=v.id ORDER BY u.indexed_at_ms,v.id").map_err(storage_sqlite)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(VectorUsage {
                    variant_id: r.get(0)?,
                    artifact_id: r.get(1)?,
                    indexed_at_ms: r.get(2)?,
                    last_used_at_ms: r.get(3)?,
                    status: r.get(4)?,
                })
            })
            .map_err(storage_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_sqlite);
        rows
    }

    /// 查询与清理使用同一 SQLite 写事务；扫描以后被使用过的缓存不能被旧扫描删除。
    pub fn remove_unused_variant(&self, id: &str, cutoff: i64) -> Result<bool, KnowledgeError> {
        let mut conn = lock_conn(&self.conn);
        let tx = cache_write(&mut conn, VECTOR_SCHEMA_VERSION)?;
        let eligible: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM vector_index_variants v JOIN knowledge_vector_usage u ON u.variant_id=v.id WHERE v.id=?1 AND v.status<>'building' AND u.last_used_at_ms<=?2)",params![id,cutoff],|r|r.get(0)).map_err(storage_sqlite)?;
        if eligible {
            tx.execute(
                "DELETE FROM chunk_vectors WHERE vector_index_variant_id=?1",
                params![id],
            )
            .map_err(storage_sqlite)?;
            tx.execute(
                "DELETE FROM knowledge_derived_algorithms WHERE variant_id=?1",
                params![id],
            )
            .map_err(storage_sqlite)?;
            tx.execute(
                "DELETE FROM knowledge_vector_usage WHERE variant_id=?1",
                params![id],
            )
            .map_err(storage_sqlite)?;
            tx.execute("DELETE FROM vector_index_variants WHERE id=?1", params![id])
                .map_err(storage_sqlite)?;
        }
        tx.commit().map_err(storage_sqlite)?;
        Ok(eligible)
    }
}

fn source_protection(
    conn: &Connection,
    source: &str,
) -> Result<Option<&'static str>, KnowledgeError> {
    for (reason, sql) in [
        ("active-turn-scope-reference", "SELECT EXISTS(SELECT 1 FROM knowledge_turn_scope_sources s JOIN knowledge_turn_scopes t ON t.id=s.scope_id WHERE s.source_id=?1 AND t.status='active')"),
        ("persisted-citation-reference", "SELECT EXISTS(SELECT 1 FROM knowledge_citations c JOIN parse_artifacts p ON p.id=c.parse_artifact_id JOIN content_snapshots s ON s.id=p.content_snapshot_id WHERE s.source_id=?1)"),
        ("active-ingestion-job", "SELECT EXISTS(SELECT 1 FROM ingestion_jobs WHERE source_id=?1 AND status IN ('queued','running','pending_embedding') AND cancelled_at IS NULL)"),
        ("unsettled-paid-request", "SELECT EXISTS(SELECT 1 FROM ingestion_jobs WHERE source_id=?1 AND embedding_stats IS NOT NULL AND CASE WHEN json_valid(embedding_stats) THEN COALESCE(json_extract(embedding_stats,'$.pendingCallIntentCount'),0)>0 OR COALESCE(json_extract(embedding_stats,'$.pendingRequestCount'),0)>0 OR COALESCE(json_extract(embedding_stats,'$.unknownSentRequestCount'),0)>0 OR COALESCE(json_extract(embedding_stats,'$.unknownRequestsFromPreviousAttempt'),0)>0 ELSE 1 END)"),
    ] {
        if conn.query_row(sql,params![source],|r|r.get::<_,bool>(0)).map_err(storage_sqlite)? {return Ok(Some(reason));}
    }
    Ok(None)
}

impl KnowledgeFactStore {
    pub fn prune_orphan_indexes(
        &self,
        studio: &str,
        source: &str,
        clear: impl FnOnce(&[(String, String)]) -> Result<(), KnowledgeError>,
    ) -> Result<bool, KnowledgeError> {
        let mut conn = lock_conn(&self.conn);
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_sqlite)?;
        check_user_version(&tx, FACTS_SCHEMA_VERSION)?;
        let orphan:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM sources s WHERE s.id=?1 AND s.studio_id=?2 AND s.deleted_at IS NULL AND s.orphaned_at IS NOT NULL AND NOT EXISTS(SELECT 1 FROM notebook_sources ns JOIN notebooks n ON n.id=ns.notebook_id WHERE ns.source_id=s.id AND ns.removed_at IS NULL AND n.deleted_at IS NULL))",params![source,studio],|r|r.get(0)).map_err(storage_sqlite)?;
        if !orphan || source_protection(&tx, source)?.is_some() {
            return Ok(false);
        }
        let artifacts = {
            let mut stmt=tx.prepare("SELECT p.id,p.content_snapshot_id FROM parse_artifacts p JOIN content_snapshots s ON s.id=p.content_snapshot_id WHERE s.source_id=?1").map_err(storage_sqlite)?;
            let rows = stmt
                .query_map(params![source], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(storage_sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(storage_sqlite)?;
            rows
        };
        clear(&artifacts)?;
        tx.commit().map_err(storage_sqlite)?;
        Ok(true)
    }

    /// 配置、归属、引用检查与派生回收共用事实库租约，防止扫描后扩大保留期限。
    pub fn with_artifact_retention<T>(
        &self,
        studio: &str,
        artifact: &str,
        operation: impl FnOnce(Option<ArtifactRetention>) -> Result<T, KnowledgeError>,
    ) -> Result<T, KnowledgeError> {
        let mut conn = lock_conn(&self.conn);
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_sqlite)?;
        check_user_version(&tx, FACTS_SCHEMA_VERSION)?;
        let source:Option<String>=tx.query_row("SELECT s.id FROM parse_artifacts p JOIN content_snapshots cs ON cs.id=p.content_snapshot_id JOIN sources s ON s.id=cs.source_id WHERE p.id=?1 AND s.studio_id=?2 AND s.deleted_at IS NULL",params![artifact,studio],|r|r.get(0)).optional().map_err(storage_sqlite)?;
        let owner = if let Some(source) = source {
            let latest:String=tx.query_row("SELECT p.id FROM parse_artifacts p JOIN content_snapshots s ON s.id=p.content_snapshot_id WHERE s.source_id=?1 ORDER BY s.captured_at DESC,p.created_at DESC,p.rowid DESC LIMIT 1",params![source],|r|r.get(0)).map_err(storage_sqlite)?;
            let retention:Option<i64>=tx.query_row("SELECT CASE WHEN COUNT(*)=COUNT(c.vector_retention_days) THEN MAX(c.vector_retention_days) ELSE NULL END FROM notebook_sources ns JOIN notebooks n ON n.id=ns.notebook_id LEFT JOIN notebook_config c ON c.notebook_id=n.id WHERE ns.source_id=?1 AND ns.removed_at IS NULL AND n.deleted_at IS NULL",params![source],|r|r.get(0)).map_err(storage_sqlite)?;
            Some(ArtifactRetention {
                artifact_id: artifact.into(),
                latest: latest == artifact,
                retention_days: retention,
                protected: source_protection(&tx, &source)?.is_some(),
                source_id: source,
            })
        } else {
            None
        };
        let result = operation(owner)?;
        tx.commit().map_err(storage_sqlite)?;
        Ok(result)
    }

    pub fn delete_notebook_retained(
        &self,
        studio: &str,
        notebook: &str,
    ) -> Result<(NotebookRow, Vec<String>, usize), KnowledgeError> {
        let mut conn = lock_conn(&self.conn);
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_sqlite)?;
        let row=tx.query_row("SELECT id,studio_id,name,created_at,updated_at,deleted_at FROM notebooks WHERE id=?1 AND studio_id=?2 AND deleted_at IS NULL",params![notebook,studio],Self::map_notebook).optional().map_err(storage_sqlite)?.ok_or_else(||KnowledgeError::not_found("Knowledge notebook was not found"))?;
        let frozen:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM notebook_sources ns JOIN knowledge_turn_scope_sources s ON s.source_id=ns.source_id JOIN knowledge_turn_scopes t ON t.id=s.scope_id WHERE ns.notebook_id=?1 AND ns.removed_at IS NULL AND t.status='active')",params![notebook],|r|r.get(0)).map_err(storage_sqlite)?;
        if frozen {
            return Err(KnowledgeError::conflict(
                "notebook source is referenced by an active turn scope",
            ));
        }
        let now = now_iso(&*self.clock);
        tx.execute(
            "UPDATE notebooks SET deleted_at=?2,updated_at=?2 WHERE id=?1",
            params![notebook, now],
        )
        .map_err(storage_sqlite)?;
        tx.execute(
            "UPDATE notebook_sources SET removed_at=?2 WHERE notebook_id=?1 AND removed_at IS NULL",
            params![notebook, now],
        )
        .map_err(storage_sqlite)?;
        let cancelled=tx.execute("UPDATE ingestion_jobs SET status='failed',cancelled_at=?2,error='cancelled: notebook deleted',updated_at=?2 WHERE notebook_id=?1 AND status IN ('queued','running','pending_embedding')",params![notebook,now]).map_err(storage_sqlite)?;
        let orphans = {
            let mut stmt=tx.prepare("SELECT s.id FROM sources s JOIN notebook_sources ns ON ns.source_id=s.id WHERE ns.notebook_id=?1 AND s.studio_id=?2 AND s.deleted_at IS NULL AND NOT EXISTS(SELECT 1 FROM notebook_sources active JOIN notebooks n ON n.id=active.notebook_id WHERE active.source_id=s.id AND active.removed_at IS NULL AND n.deleted_at IS NULL)").map_err(storage_sqlite)?;
            let rows = stmt
                .query_map(params![notebook, studio], |r| r.get::<_, String>(0))
                .map_err(storage_sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(storage_sqlite)?;
            rows
        };
        for source in &orphans {
            tx.execute(
                "UPDATE sources SET orphaned_at=COALESCE(orphaned_at,?2) WHERE id=?1",
                params![source, now],
            )
            .map_err(storage_sqlite)?;
        }
        tx.commit().map_err(storage_sqlite)?;
        Ok((
            NotebookRow {
                deleted_at: Some(now.clone()),
                updated_at: now,
                ..row
            },
            orphans,
            cancelled,
        ))
    }

    pub fn orphan_candidates(
        &self,
        studio: &str,
        cutoff_ms: i64,
    ) -> Result<Vec<String>, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let cutoff = now_iso(|| cutoff_ms);
        let mut stmt=conn.prepare("SELECT id FROM sources WHERE studio_id=?1 AND deleted_at IS NULL AND orphaned_at IS NOT NULL AND orphaned_at<=?2 ORDER BY id").map_err(storage_sqlite)?;
        let rows = stmt
            .query_map(params![studio, cutoff], |r| r.get(0))
            .map_err(storage_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_sqlite);
        rows
    }

    /// 检查、回收和摘事实行持有同一事实库事务；重新挂靠、创建引用及新任务无法
    /// 插入检查与删除之间。回调只准操作独立缓存，不能反向访问事实库。
    pub fn purge_orphan_if_unreferenced(
        &self,
        studio: &str,
        source: &str,
        cutoff_ms: i64,
        clear_derived: impl FnOnce(&[(String, String)]) -> Result<(), KnowledgeError>,
    ) -> Result<Option<&'static str>, KnowledgeError> {
        let mut conn = lock_conn(&self.conn);
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_sqlite)?;
        let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM sources WHERE id=?1 AND studio_id=?2 AND deleted_at IS NULL AND orphaned_at<=?3)",params![source,studio,now_iso(||cutoff_ms)],|r|r.get(0)).map_err(storage_sqlite)?;
        if !exists {
            return Ok(Some("not-expired-or-not-owned"));
        }
        let active:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM notebook_sources ns JOIN notebooks n ON n.id=ns.notebook_id WHERE ns.source_id=?1 AND ns.removed_at IS NULL AND n.deleted_at IS NULL)",params![source],|r|r.get(0)).map_err(storage_sqlite)?;
        if active {
            tx.execute(
                "UPDATE sources SET orphaned_at=NULL WHERE id=?1",
                params![source],
            )
            .map_err(storage_sqlite)?;
            tx.commit().map_err(storage_sqlite)?;
            return Ok(Some("active-membership-reappeared"));
        }
        if let Some(reason) = source_protection(&tx, source)? {
            return Ok(Some(reason));
        }
        let snapshots = {
            let mut stmt = tx
                .prepare("SELECT id,storage_path FROM content_snapshots WHERE source_id=?1")
                .map_err(storage_sqlite)?;
            let rows = stmt
                .query_map(params![source], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(storage_sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(storage_sqlite)?;
            rows
        };
        let artifacts = {
            let mut stmt=tx.prepare("SELECT p.id,p.content_snapshot_id FROM parse_artifacts p JOIN content_snapshots s ON s.id=p.content_snapshot_id WHERE s.source_id=?1").map_err(storage_sqlite)?;
            let rows = stmt
                .query_map(params![source], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(storage_sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(storage_sqlite)?;
            rows
        };
        for (id, storage_path) in &snapshots {
            if !id
                .strip_prefix("ksnap_")
                .is_some_and(|tail| tail.len() == 24 && tail.bytes().all(|c| c.is_ascii_hexdigit()))
                || storage_path != &format!("snapshots/{id}")
            {
                return Err(KnowledgeError::storage_invalid(
                    "unsafe snapshot cleanup identity; original preserved",
                ));
            }
        }
        clear_derived(&artifacts)?;
        // canonical 根只用于展开宿主合法的父目录别名；后续逐层目录句柄拒绝链接替换。
        let root = std::fs::canonicalize(&self.root).map_err(storage_io)?;
        for (id, _) in &snapshots {
            cleanup_snapshot_files(&root, &self.root_guard, id)?;
        }
        for sql in [
            "DELETE FROM knowledge_turn_scope_sources WHERE source_id=?1",
            "DELETE FROM ingestion_jobs WHERE source_id=?1",
            "DELETE FROM notebook_sources WHERE source_id=?1",
            "DELETE FROM knowledge_citations WHERE parse_artifact_id IN (SELECT p.id FROM parse_artifacts p JOIN content_snapshots s ON s.id=p.content_snapshot_id WHERE s.source_id=?1)",
            "DELETE FROM knowledge_blocks WHERE parse_artifact_id IN (SELECT p.id FROM parse_artifacts p JOIN content_snapshots s ON s.id=p.content_snapshot_id WHERE s.source_id=?1)",
            "DELETE FROM parse_artifacts WHERE content_snapshot_id IN (SELECT id FROM content_snapshots WHERE source_id=?1)",
        ] {tx.execute(sql,params![source]).map_err(storage_sqlite)?;}
        // EX07 附加的处理事实在合并前可能尚未创建。
        let processing:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='processing_artifacts')",[],|r|r.get(0)).map_err(storage_sqlite)?;
        if processing {
            tx.execute("DELETE FROM processing_artifacts WHERE content_snapshot_id IN (SELECT id FROM content_snapshots WHERE source_id=?1)",params![source]).map_err(storage_sqlite)?;
        }
        tx.execute(
            "DELETE FROM content_snapshots WHERE source_id=?1",
            params![source],
        )
        .map_err(storage_sqlite)?;
        tx.execute("DELETE FROM sources WHERE id=?1", params![source])
            .map_err(storage_sqlite)?;
        tx.commit().map_err(storage_sqlite)?;
        Ok(None)
    }
}

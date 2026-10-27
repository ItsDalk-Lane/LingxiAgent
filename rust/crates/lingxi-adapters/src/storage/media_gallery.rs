//! 工作台媒体任务库（现役 core/media/task-store.ts 的持久化语义）：与媒体运行时
//! 作业一一对应，提供所有者作用域内的筛选列表、单任务读取、批次查询与重试新尝试。
//! 运行时作业状态是唯一事实来源；本表是其工作台投影，随每次作业迁移同步。
use super::{
    media::{MediaJob, MediaPhase},
    migrations::map_rusqlite,
    RunDatabase,
};
use lingxi_kernel::ports::StorageError;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct GalleryTask {
    pub task_id: String,
    pub session_id: String,
    pub owner_subject: String,
    pub batch_id: String,
    pub kind: String,
    pub task_type: String,
    pub prompt: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub status: String,
    pub favorited: bool,
    pub files: Value,
    pub fail_reason: Option<String>,
    pub retry_count: u64,
    pub attempt: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub completed_at: Option<u64>,
}

impl GalleryTask {
    /// 现役任务形状：ISO 时间字段由调用方按毫秒定格换算。
    pub fn to_value(&self) -> Value {
        json!({
            "taskId": self.task_id,
            "batchId": self.batch_id,
            "type": self.task_type,
            "kind": self.kind,
            "prompt": self.prompt,
            "providerId": self.provider,
            "modelId": self.model,
            "sessionId": self.session_id,
            "status": self.status,
            "favorited": self.favorited,
            "files": self.files,
            "failReason": self.fail_reason,
            "retryCount": self.retry_count,
            "attempt": self.attempt,
            "createdAtUnixMs": self.created_at,
            "updatedAtUnixMs": self.updated_at,
            "completedAtUnixMs": self.completed_at,
        })
    }
}

fn invalid(detail: impl Into<String>) -> StorageError {
    StorageError::InvalidRequest {
        detail: detail.into(),
    }
}
fn integer(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| invalid("media gallery integer overflow"))
}

/// 现役任务 type 词表：任务对象的 type 就是媒体类（"image"|"video"|"speech"，
/// core/media/image-task-runner.ts buildImageParams/retryImageTask 的
/// `type === "image"` 判断）；text2image/image2image 是参数层 mode，不是任务字段。
fn task_type(kind: &str) -> Result<String, StorageError> {
    match kind {
        "image" | "video" | "speech" => Ok(kind.to_owned()),
        _ => Err(invalid("media gallery kind damaged")),
    }
}

fn gallery_status(phase: &MediaPhase) -> &'static str {
    match phase {
        MediaPhase::Completed => "done",
        MediaPhase::Failed | MediaPhase::Unknown => "failed",
        MediaPhase::Cancelled => "cancelled",
        _ => "pending",
    }
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<GalleryTask> {
    let files_text: String = row.get(10)?;
    Ok(GalleryTask {
        task_id: row.get(0)?,
        session_id: row.get(1)?,
        owner_subject: row.get(2)?,
        batch_id: row.get(3)?,
        kind: row.get(4)?,
        task_type: row.get(5)?,
        prompt: row.get(6)?,
        provider: row.get(7)?,
        model: row.get(8)?,
        status: row.get(9)?,
        files: serde_json::from_str(&files_text).unwrap_or(Value::Null),
        favorited: row.get::<_, i64>(11)? != 0,
        fail_reason: row.get(12)?,
        retry_count: row.get::<_, i64>(13)? as u64,
        attempt: row.get::<_, i64>(14)? as u64,
        created_at: row.get::<_, i64>(15)? as u64,
        updated_at: row.get::<_, i64>(16)? as u64,
        completed_at: row.get::<_, Option<i64>>(17)?.map(|v| v as u64),
    })
}

const SELECT: &str = "SELECT task_id,session_id,owner_subject,batch_id,kind,type,prompt,provider,model,status,files_json,favorited,fail_reason,retry_count,attempt,created_at,updated_at,completed_at FROM media_gallery_v1";

/// 作业每次状态迁移后同步工作台投影。在媒体作业的同一写事务内调用
/// （claim/transition 均经此），投影与作业行要么一起提交要么一起回滚，
/// 不允许跨表漂移。UPDATE 未命中时补 INSERT，兼作旧行缺失的自愈。
pub(crate) fn sync_from_job_txn(
    conn: &rusqlite::Connection,
    job: &MediaJob,
) -> Result<(), StorageError> {
    let spec = &job.spec;
    let prompt = spec["prompt"].as_str().unwrap_or("").to_owned();
    if prompt.is_empty() || prompt.encode_utf16().count() > 32_000 {
        return Err(invalid("media gallery prompt damaged"));
    }
    let status = gallery_status(&job.phase);
    let files = &job.state["files"];
    let files = if files.is_array() {
        files.clone()
    } else {
        json!([])
    };
    let fail_reason = match job.phase {
        MediaPhase::Failed | MediaPhase::Unknown => Some(
            job.state["message"]
                .as_str()
                .unwrap_or("媒体生成未交付")
                .chars()
                .take(512)
                .collect::<String>(),
        ),
        _ => None,
    };
    let completed_at = if job.phase.terminal() {
        Some(job.updated_at)
    } else {
        None
    };
    let affected = conn
        .execute(
            "UPDATE media_gallery_v1 SET status=?2,files_json=?3,fail_reason=?4,updated_at=?5,completed_at=?6 WHERE task_id=?1",
            params![
                job.operation_id,
                status,
                files.to_string(),
                fail_reason,
                integer(job.updated_at)?,
                completed_at.map(integer).transpose()?,
            ],
        )
        .map_err(map_rusqlite)?;
    if affected > 0 {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO media_gallery_v1(task_id,session_id,owner_subject,batch_id,kind,type,prompt,provider,model,status,files_json,fail_reason,retry_count,attempt,created_at,updated_at,completed_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,0,1,?13,?14,?15)",
        params![
            job.operation_id,
            job.session_id,
            job.owner_subject,
            job.operation_id,
            job.kind,
            task_type(&job.kind)?,
            prompt,
            job.provider,
            job.state["route"]["model"].as_str().map(str::to_owned),
            status,
            files.to_string(),
            fail_reason,
            integer(job.created_at)?,
            integer(job.updated_at)?,
            completed_at.map(integer).transpose()?,
        ],
    )
    .map_err(map_rusqlite)?;
    Ok(())
}

/// 现役 listTasks：所有者作用域 + favorited/images/videos 筛选 + 创建时间倒序。
pub async fn list_owner(
    db: &RunDatabase,
    owner: &str,
    filter: Option<&str>,
) -> Result<Vec<GalleryTask>, StorageError> {
    let owner = owner.to_owned();
    let filter = filter.map(str::to_owned);
    db.queue()
        .submit(move |conn| {
            let mut sql = format!("{SELECT} WHERE owner_subject=?1");
            match filter.as_deref() {
                None => {}
                Some("favorited") => sql.push_str(" AND favorited=1"),
                Some("images") => sql.push_str(" AND kind='image'"),
                Some("videos") => sql.push_str(" AND kind='video'"),
                _ => return Err(invalid("unsupported media task filter")),
            }
            sql.push_str(" ORDER BY created_at DESC, task_id DESC LIMIT 5000");
            let mut stmt = conn.prepare(&sql).map_err(map_rusqlite)?;
            let rows = stmt
                .query_map(params![owner], row_to_task)
                .map_err(map_rusqlite)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(map_rusqlite)?);
            }
            Ok(out)
        })
        .await
}

pub async fn get_owner(
    db: &RunDatabase,
    task_id: &str,
    owner: &str,
) -> Result<Option<GalleryTask>, StorageError> {
    let task_id = task_id.to_owned();
    let owner = owner.to_owned();
    db.queue()
        .submit(move |conn| {
            conn.query_row(
                &format!("{SELECT} WHERE task_id=?1 AND owner_subject=?2"),
                params![task_id, owner],
                row_to_task,
            )
            .optional()
            .map_err(map_rusqlite)
        })
        .await
}

/// 现役 getTasksByBatch：批次内任务（所有者作用域内），按创建时间正序。
pub async fn list_batch(
    db: &RunDatabase,
    batch_id: &str,
    owner: &str,
) -> Result<Vec<GalleryTask>, StorageError> {
    let batch_id = batch_id.to_owned();
    let owner = owner.to_owned();
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "{SELECT} WHERE batch_id=?1 AND owner_subject=?2 ORDER BY created_at ASC, task_id ASC LIMIT 512"
                ))
                .map_err(map_rusqlite)?;
            let rows = stmt
                .query_map(params![batch_id, owner], row_to_task)
                .map_err(map_rusqlite)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(map_rusqlite)?);
            }
            Ok(out)
        })
        .await
}

/// 现役 beginAttempt：仅 failed/cancelled 可重试；同一任务进入新尝试，
/// 清空产物与失败原因，retry_count/attempt 递增。返回 None 表示任务不存在
/// 或不属于此所有者（路由层映射 404）。
pub async fn begin_retry(
    db: &RunDatabase,
    task_id: &str,
    owner: &str,
    now: u64,
) -> Result<Option<GalleryTask>, StorageError> {
    let task_id = task_id.to_owned();
    let owner = owner.to_owned();
    db.queue()
        .submit(move |conn| {
            let Some(task) = conn
                .query_row(
                    &format!("{SELECT} WHERE task_id=?1 AND owner_subject=?2"),
                    params![task_id, owner],
                    row_to_task,
                )
                .optional()
                .map_err(map_rusqlite)?
            else {
                return Ok(None);
            };
            match task.status.as_str() {
                "failed" | "cancelled" => {}
                "pending" => {
                    return Err(StorageError::Conflict {
                        detail: "task is already running".into(),
                    })
                }
                _ => {
                    return Err(StorageError::Conflict {
                        detail: "task is not retryable".into(),
                    })
                }
            }
            conn.execute(
                "UPDATE media_gallery_v1 SET status='pending',files_json='[]',fail_reason=NULL,retry_count=retry_count+1,attempt=attempt+1,updated_at=?2,completed_at=NULL WHERE task_id=?1",
                params![task.task_id, integer(now)?],
            )
            .map_err(map_rusqlite)?;
            conn.query_row(
                &format!("{SELECT} WHERE task_id=?1"),
                params![task.task_id],
                row_to_task,
            )
            .optional()
            .map_err(map_rusqlite)
        })
        .await
}

/// 重试换绑（现役 beginAttempt 后任务保持同一 taskId 的 Rust 等价）：新运行作业
/// 受理会产生新的 operation_id（运行身份反重复语义），这里把工作台任务行换绑到
/// 新作业并删除新作业的初始投影行，attempt/retry_count 连续性留在同一任务上。
/// 必须在 begin_retry 之后、同一所有者范围内调用；换绑目标不存在于作业表时
/// 由外键约束响亮失败，绝不留下悬空任务。
pub async fn rekey_retry(db: &RunDatabase, from: &str, to: &str) -> Result<(), StorageError> {
    if from == to {
        return Err(invalid(
            "media gallery retry rekey requires a new operation",
        ));
    }
    let from = from.to_owned();
    let to = to.to_owned();
    db.queue()
        .submit(move |conn| {
            let txn = conn.unchecked_transaction().map_err(map_rusqlite)?;
            let removed = txn
                .execute("DELETE FROM media_gallery_v1 WHERE task_id=?1", params![to])
                .map_err(map_rusqlite)?;
            if removed != 1 {
                return Err(invalid("media gallery retry target projection missing"));
            }
            let moved = txn
                .execute(
                    "UPDATE media_gallery_v1 SET task_id=?2 WHERE task_id=?1",
                    params![from, to],
                )
                .map_err(map_rusqlite)?;
            if moved != 1 {
                return Err(invalid("media gallery retry source projection missing"));
            }
            txn.commit().map_err(map_rusqlite)?;
            Ok(())
        })
        .await
}

/// 重试提交失败：回到 failed 并记录本次提交失败原因；attempt/retry_count 已
/// 消耗不回退（一次真实尝试发生过），files 保持清空（旧产物已被 begin_retry 让位）。
pub async fn abort_retry(
    db: &RunDatabase,
    task_id: &str,
    reason: &str,
    now: u64,
) -> Result<(), StorageError> {
    let task_id = task_id.to_owned();
    let reason = reason.chars().take(512).collect::<String>();
    db.queue()
        .submit(move |conn| {
            conn.execute(
                "UPDATE media_gallery_v1 SET status='failed',fail_reason=?2,updated_at=?3 WHERE task_id=?1",
                params![task_id, reason, integer(now)?],
            )
            .map_err(map_rusqlite)?;
            Ok(())
        })
        .await
}

/// 所有者作用域内按产物文件名解析 (file_id, task_id)，最新任务优先
/// （与现役平铺 generated 目录同名后写覆盖的读取平价）。文件名只作为
/// JSON 值匹配，绝不参与路径拼接。
pub async fn find_file_owner(
    db: &RunDatabase,
    owner: &str,
    filename: &str,
) -> Result<Option<(String, String)>, StorageError> {
    let owner = owner.to_owned();
    let filename = filename.to_owned();
    db.queue()
        .submit(move |conn| {
            conn.query_row(
                "SELECT json_extract(f.value,'$.fileId'), t.task_id \
                 FROM media_gallery_v1 t, json_each(t.files_json) f \
                 WHERE t.owner_subject=?1 AND json_extract(f.value,'$.filename')=?2 \
                 ORDER BY t.created_at DESC, t.task_id DESC LIMIT 1",
                params![owner, filename],
                |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(map_rusqlite)?
            .map(|(file_id, task_id)| {
                file_id
                    .map(|file_id| (file_id, task_id))
                    .ok_or_else(|| invalid("media gallery file entry carries no fileId"))
            })
            .transpose()
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    // 集成验证在 lingxi-service 的 r07_media_admin 目标：真实库 + 路由 + 运行时同步。
    #[test]
    fn gallery_status_mapping_never_claims_success_for_non_delivery() {
        assert_eq!(gallery_status(&MediaPhase::Submitted), "pending");
        assert_eq!(gallery_status(&MediaPhase::Generating), "pending");
        assert_eq!(gallery_status(&MediaPhase::Downloading), "pending");
        assert_eq!(gallery_status(&MediaPhase::Completed), "done");
        assert_eq!(gallery_status(&MediaPhase::Failed), "failed");
        assert_eq!(gallery_status(&MediaPhase::Cancelled), "cancelled");
        assert_eq!(gallery_status(&MediaPhase::Unknown), "failed");
    }

    #[test]
    fn task_type_is_the_incumbent_media_kind() {
        // 现役任务对象 type === 媒体类（retryImageTask 以 type === "image" 判定）。
        assert_eq!(task_type("image").unwrap(), "image");
        assert_eq!(task_type("video").unwrap(), "video");
        assert_eq!(task_type("speech").unwrap(), "speech");
        assert!(task_type("asr").is_err());
    }
}

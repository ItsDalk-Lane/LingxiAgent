//! 切换状态机 journal（T05_DESIGN §3）。
//!
//! - 落盘 `{work-dir}/journal.json`，schemaVersion 1，每次换相 = 一次
//!   `paths::atomic_write_private`（tmp+rename+fsync，无先删后建窗口）。
//! - `phase` 是恢复的唯一权威；mtime 永不参与裁决。
//! - 任一进相失败 = 写 `refused` 终态后退出，绝不跳相、绝不降格为警告。
//! - A09 断点相位：preflight / backup / import / pointer_switch / selfcheck。

use crate::paths;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

pub const JOURNAL_FILE_NAME: &str = "journal.json";
pub const JOURNAL_SCHEMA_VERSION: u64 = 1;

/// 前向 10 相（序即权威；`refused` 为独立终态不入此列）。
pub const CUTOVER_PHASES: [&str; 10] = [
    "preflight",
    "stop_write_probe",
    "quiesce_snapshot",
    "backup",
    "import",
    "import_verify",
    "epoch_adopt",
    "pointer_switch",
    "selfcheck",
    "committed",
];

/// 终态：fail-closed 拒绝。
pub const PHASE_REFUSED: &str = "refused";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistoryEntry {
    pub phase: String,
    #[serde(rename = "enteredAt")]
    pub entered_at: String,
    #[serde(rename = "leftAt")]
    pub left_at: String,
    /// ok | stopped | refused
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CutoverInputs {
    pub source: String,
    pub target: String,
    #[serde(rename = "workDir")]
    pub work_dir: String,
    #[serde(rename = "backupDir")]
    pub backup_dir: String,
    pub pointer: String,
    /// 预检/静止点阶段登记；进相重验时以此对拍源未变。
    #[serde(rename = "sourceManifestHash", default)]
    pub source_manifest_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImportBookmark {
    #[serde(rename = "runId")]
    pub run_id: String,
    /// planned | running | interrupted | completed | failed
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AdoptBookmark {
    #[serde(rename = "transitionId")]
    pub transition_id: String,
    /// epoch transition journal 的相位快照（对拍用，非权威）。
    pub phase: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Refusal {
    pub check: String,
    pub reason: String,
    #[serde(
        rename = "reportPath",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub report_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CutoverJournal {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u64,
    #[serde(rename = "cutoverId")]
    pub cutover_id: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    pub phase: String,
    pub attempts: u64,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    pub inputs: CutoverInputs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<ImportBookmark>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adopt: Option<AdoptBookmark>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
}

#[derive(Debug)]
pub enum JournalError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Corrupt {
        path: PathBuf,
        detail: String,
    },
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JournalError::Io { path, source } => {
                write!(f, "journal io error at {}: {source}", path.display())
            }
            JournalError::Corrupt { path, detail } => {
                write!(f, "journal corrupt at {}: {detail}", path.display())
            }
        }
    }
}

impl std::error::Error for JournalError {}

pub fn journal_path(work_dir: &Path) -> PathBuf {
    work_dir.join(JOURNAL_FILE_NAME)
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn phase_index(phase: &str) -> Option<usize> {
    CUTOVER_PHASES.iter().position(|p| *p == phase)
}

/// 校验 journal 结构与相位合法性（corrupt = 响亮拒，绝不修）。
fn validate(journal: &CutoverJournal, path: &Path) -> Result<(), JournalError> {
    let corrupt = |detail: String| JournalError::Corrupt {
        path: path.to_path_buf(),
        detail,
    };
    if journal.schema_version != JOURNAL_SCHEMA_VERSION {
        return Err(corrupt(format!(
            "schemaVersion {} != {JOURNAL_SCHEMA_VERSION}",
            journal.schema_version
        )));
    }
    if journal.cutover_id.is_empty() {
        return Err(corrupt("cutoverId is empty".into()));
    }
    if journal.phase != PHASE_REFUSED && phase_index(&journal.phase).is_none() {
        return Err(corrupt(format!("unknown phase {:?}", journal.phase)));
    }
    for entry in &journal.history {
        if phase_index(&entry.phase).is_none() {
            return Err(corrupt(format!(
                "history references unknown phase {:?}",
                entry.phase
            )));
        }
        if !matches!(
            entry.outcome.as_str(),
            "ok" | "stopped" | "refused" | "in-progress" | "reverify-failed"
        ) {
            return Err(corrupt(format!("history outcome {:?}", entry.outcome)));
        }
    }
    Ok(())
}

/// 读取 journal：不存在 = Ok(None)；损坏 = 响亮 Err。
pub fn load(work_dir: &Path) -> Result<Option<CutoverJournal>, JournalError> {
    let path = journal_path(work_dir);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(JournalError::Io { path, source }),
    };
    let journal: CutoverJournal =
        serde_json::from_str(&raw).map_err(|err| JournalError::Corrupt {
            path: path.clone(),
            detail: format!("JSON parse: {err}"),
        })?;
    validate(&journal, &path)?;
    Ok(Some(journal))
}

/// 原子落盘（同目录 tmp+rename+fsync；写后读回校验）。
fn persist(work_dir: &Path, journal: &CutoverJournal) -> Result<(), JournalError> {
    let path = journal_path(work_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| JournalError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let mut body = serde_json::to_string_pretty(journal).map_err(|err| JournalError::Corrupt {
        path: path.clone(),
        detail: format!("serialize: {err}"),
    })?;
    body.push('\n');
    paths::atomic_write_private(&path, body.as_bytes()).map_err(|source| JournalError::Io {
        path: path.clone(),
        source,
    })?;
    // 写后读回：rename 原子语义下读到的必须是刚写的完整内容。
    let readback = std::fs::read(&path).map_err(|source| JournalError::Io {
        path: path.clone(),
        source,
    })?;
    if readback != body.as_bytes() {
        return Err(JournalError::Corrupt {
            path,
            detail: "readback mismatch after atomic write".into(),
        });
    }
    Ok(())
}

impl CutoverJournal {
    /// 新建 journal（相位=preflight，attempts=1）并落盘。
    /// work-dir 已有 journal 时调用方必须先走 resume 裁决，此处直接覆盖
    /// 属于编排层错误——本函数对既有文件响亮拒绝。
    pub fn create(
        work_dir: &Path,
        cutover_id: &str,
        inputs: CutoverInputs,
    ) -> Result<Self, JournalError> {
        let path = journal_path(work_dir);
        if path.exists() {
            return Err(JournalError::Corrupt {
                path,
                detail: "journal already exists; use --resume (double-run guard)".into(),
            });
        }
        let journal = CutoverJournal {
            schema_version: JOURNAL_SCHEMA_VERSION,
            cutover_id: cutover_id.to_string(),
            created_at: now_iso(),
            phase: CUTOVER_PHASES[0].to_string(),
            attempts: 1,
            history: Vec::new(),
            inputs,
            import: None,
            adopt: None,
            refusal: None,
        };
        persist(work_dir, &journal)?;
        Ok(journal)
    }

    /// 续跑开门：同 cutoverId + attempts+1。
    pub fn reopen_for_resume(
        mut self,
        work_dir: &Path,
        cutover_id: &str,
    ) -> Result<Self, JournalError> {
        if self.cutover_id != cutover_id {
            return Err(JournalError::Corrupt {
                path: journal_path(work_dir),
                detail: format!(
                    "--resume {cutover_id:?} does not match journal cutoverId {:?}",
                    self.cutover_id
                ),
            });
        }
        self.attempts += 1;
        persist(work_dir, &self)?;
        Ok(self)
    }

    /// 换相：闭合当前相位历史条目，进入下一相位（可前进、或同相重进）。
    /// 绝不允许倒退或跳相——编排层重验后从失效相续跑时同相重进。
    pub fn advance(
        mut self,
        work_dir: &Path,
        next_phase: &str,
        detail: Option<serde_json::Value>,
    ) -> Result<Self, JournalError> {
        let path = journal_path(work_dir);
        let from_idx = phase_index(&self.phase).ok_or_else(|| JournalError::Corrupt {
            path: path.clone(),
            detail: format!("cannot advance out of terminal phase {:?}", self.phase),
        })?;
        let to_idx = phase_index(next_phase).ok_or_else(|| JournalError::Corrupt {
            path: path.clone(),
            detail: format!("advance to unknown phase {next_phase:?}"),
        })?;
        if to_idx < from_idx {
            return Err(JournalError::Corrupt {
                path,
                detail: format!(
                    "phase regression refused in advance: {:?} -> {next_phase:?} (resume re-entry must use rewind_to)",
                    self.phase
                ),
            });
        }
        if to_idx > from_idx + 1 {
            return Err(JournalError::Corrupt {
                path,
                detail: format!("phase skip refused: {:?} -> {next_phase:?}", self.phase),
            });
        }
        let now = now_iso();
        let entry = self
            .history
            .iter()
            .rev()
            .find(|h| h.phase == self.phase && h.outcome == "in-progress")
            .cloned();
        let entered_at = entry.map(|e| e.entered_at).unwrap_or_else(|| now.clone());
        self.history.retain(|h| h.outcome != "in-progress");
        self.history.push(HistoryEntry {
            phase: self.phase.clone(),
            entered_at,
            left_at: now,
            outcome: "ok".to_string(),
            detail,
        });
        self.phase = next_phase.to_string();
        self.history.push(HistoryEntry {
            phase: next_phase.to_string(),
            entered_at: now_iso(),
            left_at: String::new(),
            outcome: "in-progress".to_string(),
            detail: None,
        });
        persist(work_dir, &self)?;
        Ok(self)
    }

    /// resume 重验发现相位 k 前置失效 → 显式回卷到 k 重跑（唯一允许倒退的
    /// 入口；历史留痕 reverify-failed，phase 回卷本身也是原子落盘）。
    pub fn rewind_to(
        mut self,
        work_dir: &Path,
        phase: &str,
        reason: &str,
    ) -> Result<Self, JournalError> {
        let path = journal_path(work_dir);
        let from_idx = self
            .current_phase_index()
            .ok_or_else(|| JournalError::Corrupt {
                path: path.clone(),
                detail: format!("cannot rewind from terminal phase {:?}", self.phase),
            })?;
        let to_idx = phase_index(phase).ok_or_else(|| JournalError::Corrupt {
            path: path.clone(),
            detail: format!("rewind to unknown phase {phase:?}"),
        })?;
        if to_idx > from_idx {
            return Err(JournalError::Corrupt {
                path,
                detail: format!(
                    "rewind must move backward or stay: {:?} -> {phase:?}",
                    self.phase
                ),
            });
        }
        let now = now_iso();
        self.history.retain(|h| h.outcome != "in-progress");
        self.history.push(HistoryEntry {
            phase: self.phase.clone(),
            entered_at: now.clone(),
            left_at: now.clone(),
            outcome: "reverify-failed".to_string(),
            detail: Some(serde_json::json!({"rewindTo": phase, "reason": reason})),
        });
        self.phase = phase.to_string();
        persist(work_dir, &self)?;
        Ok(self)
    }

    /// `--stop-after-phase` 干净停：把 in-progress 闭合为 stopped，phase 回退
    /// 到刚完成的相位（下次 resume 从下一相续跑由编排层按前置重验决定）。
    pub fn mark_stopped(mut self, work_dir: &Path) -> Result<Self, JournalError> {
        if let Some(last) = self.history.last_mut() {
            if last.outcome == "in-progress" {
                last.outcome = "stopped".to_string();
                last.left_at = now_iso();
            }
        }
        persist(work_dir, &self)?;
        Ok(self)
    }

    /// fail-closed 终态。
    pub fn refuse(
        mut self,
        work_dir: &Path,
        check: &str,
        reason: &str,
        report_path: Option<String>,
    ) -> Result<Self, JournalError> {
        let now = now_iso();
        self.history.retain(|h| h.outcome != "in-progress");
        self.history.push(HistoryEntry {
            phase: self.phase.clone(),
            entered_at: now.clone(),
            left_at: now,
            outcome: "refused".to_string(),
            detail: None,
        });
        self.phase = PHASE_REFUSED.to_string();
        self.refusal = Some(Refusal {
            check: check.to_string(),
            reason: reason.to_string(),
            report_path,
        });
        persist(work_dir, &self)?;
        Ok(self)
    }

    /// import 书签登记（相位不变，原子重写）。
    pub fn record_import(
        mut self,
        work_dir: &Path,
        bookmark: ImportBookmark,
    ) -> Result<Self, JournalError> {
        self.import = Some(bookmark);
        persist(work_dir, &self)?;
        Ok(self)
    }

    /// epoch 采用书签登记。
    pub fn record_adopt(
        mut self,
        work_dir: &Path,
        bookmark: AdoptBookmark,
    ) -> Result<Self, JournalError> {
        self.adopt = Some(bookmark);
        persist(work_dir, &self)?;
        Ok(self)
    }

    /// 源清单哈希登记（预检/静止点取证）。
    pub fn record_source_manifest_hash(
        mut self,
        work_dir: &Path,
        hash: String,
    ) -> Result<Self, JournalError> {
        self.inputs.source_manifest_hash = Some(hash);
        persist(work_dir, &self)?;
        Ok(self)
    }

    pub fn is_terminal(&self) -> bool {
        self.phase == PHASE_REFUSED || self.phase == "committed"
    }

    pub fn current_phase_index(&self) -> Option<usize> {
        phase_index(&self.phase)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_work(tag: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "t05-journal-test-{}-{}-{tag}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn inputs(work: &Path) -> CutoverInputs {
        CutoverInputs {
            source: "/tmp/src".into(),
            target: "/tmp/tgt".into(),
            work_dir: work.display().to_string(),
            backup_dir: "/tmp/bak".into(),
            pointer: "/tmp/ptr.json".into(),
            source_manifest_hash: None,
        }
    }

    #[test]
    fn create_advance_refuse_roundtrip() {
        let work = temp_work("roundtrip");
        let j = CutoverJournal::create(&work, "cut-1", inputs(&work)).unwrap();
        assert_eq!(j.phase, "preflight");
        let j = j.advance(&work, "stop_write_probe", None).unwrap();
        let j = j
            .advance(
                &work,
                "quiesce_snapshot",
                Some(serde_json::json!({"stores":69})),
            )
            .unwrap();
        let loaded = load(&work).unwrap().unwrap();
        assert_eq!(loaded, j);
        assert_eq!(loaded.phase, "quiesce_snapshot");
        assert_eq!(loaded.history.len(), 3); // preflight ok, stop_write_probe ok, quiesce in-progress
        let j = loaded
            .refuse(
                &work,
                "disk-space",
                "avail < required",
                Some("/tmp/report.json".into()),
            )
            .unwrap();
        assert!(j.is_terminal());
        assert_eq!(j.refusal.as_ref().unwrap().check, "disk-space");
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn double_create_is_loud_refusal() {
        let work = temp_work("double");
        CutoverJournal::create(&work, "cut-1", inputs(&work)).unwrap();
        let err = CutoverJournal::create(&work, "cut-2", inputs(&work)).unwrap_err();
        assert!(err.to_string().contains("double-run guard"), "{err}");
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn resume_requires_matching_cutover_id() {
        let work = temp_work("resume");
        let j = CutoverJournal::create(&work, "cut-1", inputs(&work)).unwrap();
        let err = j.clone().reopen_for_resume(&work, "cut-other").unwrap_err();
        assert!(err.to_string().contains("does not match"), "{err}");
        let j = j.reopen_for_resume(&work, "cut-1").unwrap();
        assert_eq!(j.attempts, 2);
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn phase_skip_and_unknown_phase_are_corrupt() {
        let work = temp_work("skip");
        let j = CutoverJournal::create(&work, "cut-1", inputs(&work)).unwrap();
        let err = j.clone().advance(&work, "backup", None).unwrap_err();
        assert!(err.to_string().contains("phase skip refused"), "{err}");
        let j = j.advance(&work, "stop_write_probe", None).unwrap();
        let err = j.clone().advance(&work, "preflight", None).unwrap_err();
        assert!(
            err.to_string().contains("phase regression refused"),
            "{err}"
        );
        let err = j.advance(&work, "bogus", None).unwrap_err();
        assert!(err.to_string().contains("unknown phase"), "{err}");
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn rewind_reenters_earlier_phase_with_audit_trail() {
        let work = temp_work("rewind");
        let j = CutoverJournal::create(&work, "cut-1", inputs(&work)).unwrap();
        let j = j.advance(&work, "stop_write_probe", None).unwrap();
        let j = j.advance(&work, "quiesce_snapshot", None).unwrap();
        let j = j
            .rewind_to(&work, "stop_write_probe", "source manifest drifted")
            .unwrap();
        assert_eq!(j.phase, "stop_write_probe");
        assert!(j.history.iter().any(|h| h.outcome == "reverify-failed"
            && h.detail.as_ref().unwrap()["rewindTo"] == "stop_write_probe"));
        let err = j.rewind_to(&work, "backup", "forward rewind").unwrap_err();
        assert!(
            err.to_string().contains("rewind must move backward"),
            "{err}"
        );
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn corrupt_journal_is_loud_not_repaired() {
        let work = temp_work("corrupt");
        CutoverJournal::create(&work, "cut-1", inputs(&work)).unwrap();
        std::fs::write(journal_path(&work), b"{not json").unwrap();
        let err = load(&work).unwrap_err();
        assert!(err.to_string().contains("corrupt"), "{err}");
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn stop_marks_history_stopped() {
        let work = temp_work("stop");
        let j = CutoverJournal::create(&work, "cut-1", inputs(&work)).unwrap();
        let j = j.advance(&work, "stop_write_probe", None).unwrap();
        let j = j.mark_stopped(&work).unwrap();
        let last = j.history.last().unwrap();
        assert_eq!(last.outcome, "stopped");
        assert!(!last.left_at.is_empty());
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn bookmarks_and_manifest_hash_persist() {
        let work = temp_work("bookmarks");
        let j = CutoverJournal::create(&work, "cut-1", inputs(&work)).unwrap();
        let j = j
            .record_import(
                &work,
                ImportBookmark {
                    run_id: "import:abc:1".into(),
                    status: "interrupted".into(),
                },
            )
            .unwrap();
        let j = j
            .record_adopt(
                &work,
                AdoptBookmark {
                    transition_id: "cut-1".into(),
                    phase: "barrier_raised".into(),
                },
            )
            .unwrap();
        let _j = j
            .record_source_manifest_hash(&work, "deadbeef".into())
            .unwrap();
        let loaded = load(&work).unwrap().unwrap();
        assert_eq!(loaded.import.as_ref().unwrap().status, "interrupted");
        assert_eq!(loaded.adopt.as_ref().unwrap().phase, "barrier_raised");
        assert_eq!(
            loaded.inputs.source_manifest_hash.as_deref(),
            Some("deadbeef")
        );
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn journal_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let work = temp_work("private");
        CutoverJournal::create(&work, "cut-1", inputs(&work)).unwrap();
        let mode = std::fs::metadata(journal_path(&work))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "mode={mode:o}");
        std::fs::remove_dir_all(&work).unwrap();
    }
}

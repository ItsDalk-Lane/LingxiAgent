//! Storage adapter: the new run/message SQLite database (R02-T04).
//!
//! Layout of this module:
//! - [`migrations`]: versioned, fingerprinted schema migrations with
//!   on-disk receipts and loud tamper/downgrade rejection; plus the
//!   open-time integrity check (R02-T06).
//! - [`queue`]: the bounded DB work queue (one dedicated synchronous
//!   worker thread; WAL, busy timeout, autocheckpoint negotiated up front;
//!   database files tightened to 0600 — R02-T06 F04 follow-up).
//! - [`run_store`]: [`RunDatabase`], the [`lingxi_kernel::ports::StoragePort`]
//!   implementation with same-transaction terminal+event semantics.
//! - [`backup`]: Online-Backup-API snapshots with manifest-verified
//!   restores (R02-T06; never a plain copy of the active main file).

pub mod agent_memory;
pub mod automation;
pub mod automation_activity;
pub mod automation_import;
pub mod automation_suggestions;
pub mod backup;
pub mod beautify;
pub mod bridge;
pub mod bridge_effect;
pub mod bridge_notice;
pub mod browser;
pub mod canonical_message_store;
pub mod conversation_discovery;
pub mod conversation_status;
pub mod conversations;
pub mod conversations_collab;
pub mod knowledge;
pub mod legacy_import;
pub mod legacy_session_files;
pub mod media;
pub mod media_gallery;
pub mod migrations;
pub mod queue;
/// R08-T06：回滚增量包——旧根冻结身份集推导 + 新根差集导出/回灌。
pub mod rollback_delta;
pub mod run_store;
pub mod session_admin;
pub mod session_files;
pub mod session_tree;
/// R08-T05：statvfs/GetDiskFreeSpaceExW 可用空间探测（预检磁盘空间项）。
pub mod statfs;
pub mod skill_library;
pub mod subagent_threads;
#[cfg(windows)]
pub mod windows_acl;

pub use backup::{
    backup_database, restore_backup, BackupManifest, BackupOptions, BackupOutcome, RestoreOutcome,
    MANIFEST_SCHEMA_VERSION,
};
pub use migrations::{
    fingerprint_sql, supported_version, verify_integrity, Migration, MigrationOutcome, MIGRATIONS,
};
pub use queue::{
    checkpoint_truncate, DbQueue, StoreOptions, MAX_BUSY_TIMEOUT_MS, MAX_CHECKPOINT_PAGES,
    MAX_QUEUE_CAPACITY, MAX_QUEUE_WAIT_TIMEOUT_MS,
};
pub use run_store::{
    wal_sidecar_path, ActiveRunFacts, RunDatabase, RunSummaryRow, SessionRow, RUNS_DB_FILE_NAME,
};

pub mod session_operations;

pub mod bridge_reads;

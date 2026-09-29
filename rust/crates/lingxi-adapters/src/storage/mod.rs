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

pub mod backup;
pub mod migrations;
pub mod queue;
pub mod run_store;
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

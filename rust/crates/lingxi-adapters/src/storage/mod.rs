//! Storage adapter: the new run/message SQLite database (R02-T04).
//!
//! Layout of this module:
//! - [`migrations`]: versioned, fingerprinted schema migrations with
//!   on-disk receipts and loud tamper/downgrade rejection.
//! - [`queue`]: the bounded DB work queue (one dedicated synchronous
//!   worker thread; WAL, busy timeout, autocheckpoint negotiated up front).
//! - [`run_store`]: [`RunDatabase`], the [`lingxi_kernel::ports::StoragePort`]
//!   implementation with same-transaction terminal+event semantics.

pub mod migrations;
pub mod queue;
pub mod run_store;

pub use migrations::{fingerprint_sql, supported_version, Migration, MigrationOutcome, MIGRATIONS};
pub use queue::{checkpoint_truncate, DbQueue, StoreOptions};
pub use run_store::{RunDatabase, RunSummaryRow, SessionRow, RUNS_DB_FILE_NAME};

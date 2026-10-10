//! R01-T03 spike crate library: the shared pieces of the dependency-locking
//! prototype.
//!
//! * [`queue`] — the bounded work queue that isolates synchronous SQLite
//!   access (taskbook R01-T03 step 2: "默认 SQLite 用独立有界工作队列隔离同步访问").
//! * schema-validation helper used by `spike_schema_validate` and unit tests.

// R06-T06: dev-dependency referenced only under cfg(test) — this links the
// test guard's pre-main hook into this crate's test binaries. Never part of
// a normal build or release (dev-dependencies only).
#[cfg(test)]
#[allow(unused_imports)]
use lingxi_test_guard as _;

pub mod queue;
pub mod schema;

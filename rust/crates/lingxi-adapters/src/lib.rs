//! lingxi-adapters — implementations of the kernel's ports (R02-T04
//! establishes the crate per DEPENDENCY_RULES.json `establish_stage: R02`).
//!
//! Contract anchors:
//! - Port traits live in `lingxi-kernel`; this crate implements them and is
//!   injected by `lingxi-service` at the composition root. It must NEVER
//!   depend on `lingxi-service` (DEP-09) and never on any desktop stack
//!   (DEP-07).
//! - Current surface: the storage port ([`storage::RunDatabase`]) backed by
//!   the new Rust run/message SQLite database. Model/tool/browser/
//!   integrations adapters arrive with their owning stages (R04/R05/R07).

pub mod storage;

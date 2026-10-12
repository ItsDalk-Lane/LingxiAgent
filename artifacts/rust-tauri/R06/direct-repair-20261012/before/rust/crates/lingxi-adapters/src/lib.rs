//! lingxi-adapters — implementations of the kernel's ports (R02-T04
//! establishes the crate per DEPENDENCY_RULES.json `establish_stage: R02`).
//!
//! Contract anchors:
//! - Port traits live in `lingxi-kernel`; this crate implements them and is
//!   injected by `lingxi-service` at the composition root. It must NEVER
//!   depend on `lingxi-service` (DEP-09) and never on any desktop stack
//!   (DEP-07).
//! - Current surface: the storage port ([`storage::RunDatabase`]) backed by
//!   the new Rust run/message SQLite database; the model plane
//!   ([`models`]) — the config-backed model gateway, the openai-completions
//!   real HTTP adapter and the gateway-backed turn provider (R05-T01).
//!   Browser/integrations adapters arrive with their owning stages (R07).

pub mod models;
pub mod storage;

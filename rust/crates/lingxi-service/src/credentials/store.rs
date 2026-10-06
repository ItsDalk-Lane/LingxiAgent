//! The credential store (R05-T02): `{runtime_dir}/credentials.json` — the
//! ONLY place OAuth token material persists (config files carry static
//! seeds and flow descriptors, never OAuth tokens).
//!
//! Durability and compatibility contract (C06/C11):
//! - ONE writer: every mutation is a read-modify-write of the full in-memory
//!   state behind a single mutex, persisted with
//!   [`crate::paths::atomic_write_private`] (temp file + fsync + rename +
//!   directory fsync, 0600 from creation). A crash mid-write leaves at most
//!   a temp file, never a torn store.
//! - The in-memory state changes ONLY after the persist succeeded — on a
//!   write failure the store keeps the on-disk state and the failure is
//!   reported (never reported as safely saved).
//! - `version` is checked loudly: an unknown version is a load error, never
//!   a silent migration attempt.
//! - Unknown fields (top-level and per-provider) are preserved verbatim
//!   through read-modify-write (`#[serde(flatten)]` catch-alls) — a newer
//!   build's fields survive an older build's write.
//!
//! Fault injection: [`StoreIo`] abstracts the read/write primitives so
//! tests can fail writes deterministically without touching a real home.

use std::collections::BTreeMap;
use std::path::PathBuf;

use lingxi_adapters::models::oauth::OAuthTokens;

/// The on-disk schema version this build reads and writes.
pub const CREDENTIALS_STORE_VERSION: u32 = 1;
/// The fixed file name inside the private runtime dir.
pub const CREDENTIALS_FILE_NAME: &str = "credentials.json";

/// The store file shape. `extra` preserves unknown top-level fields
/// verbatim across writes (C11 — no whole-file rewrite ever drops a field
/// this build does not know).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoreFile {
    version: u32,
    #[serde(default)]
    providers: BTreeMap<String, ProviderEntry>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

/// One provider's persisted row; unknown per-provider fields are preserved
/// verbatim as well. R05 RR1 F04: `tokens` is OPTIONAL — a logged-out
/// provider keeps its CUSTOM MODEL REGISTRY (`customModels`) on disk, and
/// a row may exist with no token material at all. Old (v1) files whose
/// rows always carry `tokens` load unchanged.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderEntry {
    #[serde(default)]
    tokens: Option<OAuthTokens>,
    /// The provider's custom model registry (R05 RR1 F04, ordered, unique).
    #[serde(default)]
    custom_models: Vec<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

/// Store load failures. Every one is loud at startup — a store that cannot
/// be understood is never replaced by an empty one silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreLoadError {
    /// The file exists but could not be read.
    Io { detail: String },
    /// The file is not valid JSON for the store shape.
    Malformed { detail: String },
    /// The file's `version` is not this build's version — an explicit
    /// version conflict, never a guessed migration (C11).
    VersionConflict { found: u32 },
}

impl std::fmt::Display for StoreLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreLoadError::Io { detail } => write!(f, "credential store cannot be read: {detail}"),
            StoreLoadError::Malformed { detail } => {
                write!(f, "credential store is not valid for its schema: {detail}")
            }
            StoreLoadError::VersionConflict { found } => write!(
                f,
                "credential store version {found} is not supported by this build \
                 (version {CREDENTIALS_STORE_VERSION}); refusing to guess a migration"
            ),
        }
    }
}

impl std::error::Error for StoreLoadError {}

/// The read/write primitives of the store (fault-injection seam).
pub trait StoreIo: Send + Sync {
    /// The current file content (`None` = no file yet).
    fn read(&self) -> Result<Option<String>, String>;
    /// Atomically replaces the file content (temp + fsync + rename).
    fn write(&self, content: &str) -> Result<(), String>;
}

/// The production IO: the store file inside the private runtime dir.
pub struct FsStoreIo {
    path: PathBuf,
}

impl FsStoreIo {
    pub fn new(runtime_dir: &std::path::Path) -> Self {
        Self {
            path: runtime_dir.join(CREDENTIALS_FILE_NAME),
        }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl StoreIo for FsStoreIo {
    fn read(&self) -> Result<Option<String>, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(content) => Ok(Some(content)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(format!("read {}: {err}", self.path.display())),
        }
    }

    fn write(&self, content: &str) -> Result<(), String> {
        crate::paths::atomic_write_private(&self.path, content.as_bytes())
            .map_err(|err| format!("atomic write {}: {err}", self.path.display()))
    }
}

/// The persistent credential store. Clone-cheap to share with refresh
/// flights; internally serialized by ONE mutex (the single writer — C11's
/// concurrent-operations rule).
#[derive(Clone)]
pub struct CredentialStore {
    io: std::sync::Arc<dyn StoreIo>,
    state: std::sync::Arc<std::sync::Mutex<StoreFile>>,
}

impl std::fmt::Debug for CredentialStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialStore").finish_non_exhaustive()
    }
}

impl CredentialStore {
    /// Loads the store (a missing file starts as an empty version-1 store).
    /// Every malformed/conflicting shape is a loud [`StoreLoadError`].
    pub fn load(io: std::sync::Arc<dyn StoreIo>) -> Result<Self, StoreLoadError> {
        let state = match io.read().map_err(|detail| StoreLoadError::Io { detail })? {
            None => StoreFile {
                version: CREDENTIALS_STORE_VERSION,
                providers: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
            Some(content) => {
                let parsed: StoreFile =
                    serde_json::from_str(&content).map_err(|err| StoreLoadError::Malformed {
                        detail: err.to_string(),
                    })?;
                if parsed.version != CREDENTIALS_STORE_VERSION {
                    return Err(StoreLoadError::VersionConflict {
                        found: parsed.version,
                    });
                }
                parsed
            }
        };
        Ok(Self {
            io,
            state: std::sync::Arc::new(std::sync::Mutex::new(state)),
        })
    }

    /// The persisted token set of one provider (None = never logged in,
    /// logged out, or revoked).
    pub fn tokens_for(&self, provider: &str) -> Option<OAuthTokens> {
        self.state
            .lock()
            .expect("credential store lock")
            .providers
            .get(provider)
            .and_then(|entry| entry.tokens.clone())
    }

    /// Persists a provider's token set (read-modify-write of the full state,
    /// one atomic file replace). The in-memory state changes ONLY after the
    /// persist succeeded; a failure keeps the on-disk state and returns the
    /// error (C06).
    pub fn put_tokens(&self, provider: &str, tokens: &OAuthTokens) -> Result<(), String> {
        let mut guard = self.state.lock().expect("credential store lock");
        let mut candidate = guard.clone();
        candidate
            .providers
            .entry(provider.to_string())
            .and_modify(|entry| entry.tokens = Some(tokens.clone()))
            .or_insert_with(|| ProviderEntry {
                tokens: Some(tokens.clone()),
                custom_models: Vec::new(),
                extra: BTreeMap::new(),
            });
        self.persist(&candidate)?;
        *guard = candidate;
        Ok(())
    }

    /// Removes a provider's token set (revocation). Same persist-first
    /// discipline as [`Self::put_tokens`]. R05 RR1 F04: a row that still
    /// carries a custom model registry (or unknown fields) SURVIVES with
    /// `tokens: null` — the registry is not credential material; a bare row
    /// is dropped entirely (the pre-F04 shape).
    pub fn remove_tokens(&self, provider: &str) -> Result<(), String> {
        let mut guard = self.state.lock().expect("credential store lock");
        let Some(entry) = guard.providers.get(provider) else {
            return Ok(());
        };
        let mut candidate = guard.clone();
        if entry.custom_models.is_empty() && entry.extra.is_empty() {
            candidate.providers.remove(provider);
        } else {
            candidate
                .providers
                .entry(provider.to_string())
                .and_modify(|entry| entry.tokens = None);
        }
        self.persist(&candidate)?;
        *guard = candidate;
        Ok(())
    }

    /// The provider's custom model registry (R05 RR1 F04) — ordered, unique,
    /// material-free. An unknown/logged-out provider has an empty registry.
    pub fn custom_models_for(&self, provider: &str) -> Vec<String> {
        self.state
            .lock()
            .expect("credential store lock")
            .providers
            .get(provider)
            .map(|entry| entry.custom_models.clone())
            .unwrap_or_default()
    }

    /// Adds one model id to the provider's custom registry (persist-first;
    /// a duplicate or the write failure changes nothing and reports). A
    /// bare row is created for a provider that had none.
    pub fn add_custom_model(&self, provider: &str, model_id: &str) -> Result<(), String> {
        let mut guard = self.state.lock().expect("credential store lock");
        let mut candidate = guard.clone();
        let entry = candidate
            .providers
            .entry(provider.to_string())
            .or_insert_with(|| ProviderEntry {
                tokens: None,
                custom_models: Vec::new(),
                extra: BTreeMap::new(),
            });
        if entry.custom_models.iter().any(|id| id == model_id) {
            return Err(format!(
                "model id {model_id:?} is already in the custom registry of provider \
                 {provider:?}"
            ));
        }
        entry.custom_models.push(model_id.to_string());
        self.persist(&candidate)?;
        *guard = candidate;
        Ok(())
    }

    /// Removes one model id from the provider's custom registry
    /// (persist-first; an absent id is a loud no-op-with-error, and a bare
    /// row disappears again).
    pub fn remove_custom_model(&self, provider: &str, model_id: &str) -> Result<(), String> {
        let mut guard = self.state.lock().expect("credential store lock");
        let Some(entry) = guard.providers.get(provider) else {
            return Err(format!(
                "model id {model_id:?} is not in the custom registry of provider {provider:?} \
                 (the provider has no registry row)"
            ));
        };
        if !entry.custom_models.iter().any(|id| id == model_id) {
            return Err(format!(
                "model id {model_id:?} is not in the custom registry of provider {provider:?}"
            ));
        }
        let mut candidate = guard.clone();
        let entry = candidate
            .providers
            .get_mut(provider)
            .expect("presence checked above");
        entry.custom_models.retain(|id| id != model_id);
        if entry.tokens.is_none() && entry.custom_models.is_empty() && entry.extra.is_empty() {
            candidate.providers.remove(provider);
        }
        self.persist(&candidate)?;
        *guard = candidate;
        Ok(())
    }

    fn persist(&self, candidate: &StoreFile) -> Result<(), String> {
        let content = serde_json::to_string_pretty(candidate)
            .map_err(|err| format!("credential store serialize failed: {err}"))?;
        self.io.write(&content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MemoryIo {
        content: std::sync::Mutex<Option<String>>,
        fail_writes: std::sync::atomic::AtomicBool,
    }

    impl StoreIo for MemoryIo {
        fn read(&self) -> Result<Option<String>, String> {
            Ok(self.content.lock().expect("mem").clone())
        }
        fn write(&self, content: &str) -> Result<(), String> {
            if self.fail_writes.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("injected write failure".to_string());
            }
            *self.content.lock().expect("mem") = Some(content.to_string());
            Ok(())
        }
    }

    fn tokens(access: &str) -> OAuthTokens {
        OAuthTokens {
            access_token: access.to_string(),
            refresh_token: "rt".to_string(),
            expires_at_unix_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn missing_file_loads_empty_and_round_trips() {
        let io = std::sync::Arc::new(MemoryIo {
            content: std::sync::Mutex::new(None),
            fail_writes: std::sync::atomic::AtomicBool::new(false),
        });
        let store = CredentialStore::load(io.clone()).expect("load");
        assert!(store.tokens_for("main").is_none());
        store.put_tokens("main", &tokens("at-1")).expect("put");
        assert_eq!(
            store.tokens_for("main").expect("stored").access_token,
            "at-1"
        );
        // A fresh load (a restart) sees the same state.
        let reloaded = CredentialStore::load(io).expect("reload");
        assert_eq!(
            reloaded.tokens_for("main").expect("stored").access_token,
            "at-1"
        );
        reloaded.remove_tokens("main").expect("remove");
        assert!(reloaded.tokens_for("main").is_none());
    }

    #[test]
    fn unknown_fields_survive_a_write() {
        let original = r#"{
            "version": 1,
            "futureTopLevel": {"keep": true},
            "providers": {
                "main": {"tokens": {"accessToken": "at-1", "refreshToken": "rt", "expiresAtUnixMs": 1700000000000}, "futureField": 42}
            }
        }"#;
        let io = std::sync::Arc::new(MemoryIo {
            content: std::sync::Mutex::new(Some(original.to_string())),
            fail_writes: std::sync::atomic::AtomicBool::new(false),
        });
        let store = CredentialStore::load(io.clone()).expect("load");
        store.put_tokens("main", &tokens("at-2")).expect("put");
        let written = io.content.lock().expect("mem").clone().expect("written");
        let value: serde_json::Value = serde_json::from_str(&written).expect("json");
        assert_eq!(value["futureTopLevel"]["keep"], true);
        assert_eq!(value["providers"]["main"]["futureField"], 42);
        assert_eq!(value["providers"]["main"]["tokens"]["accessToken"], "at-2");
    }

    #[test]
    fn a_version_conflict_is_loud() {
        let io = std::sync::Arc::new(MemoryIo {
            content: std::sync::Mutex::new(Some(r#"{"version": 99, "providers": {}}"#.to_string())),
            fail_writes: std::sync::atomic::AtomicBool::new(false),
        });
        assert!(matches!(
            CredentialStore::load(io),
            Err(StoreLoadError::VersionConflict { found: 99 })
        ));
    }

    #[test]
    fn a_failed_write_keeps_the_previous_state_and_reports() {
        let io = std::sync::Arc::new(MemoryIo {
            content: std::sync::Mutex::new(None),
            fail_writes: std::sync::atomic::AtomicBool::new(false),
        });
        let store = CredentialStore::load(io.clone()).expect("load");
        store.put_tokens("main", &tokens("at-1")).expect("put");
        io.fail_writes
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let err = store.put_tokens("main", &tokens("at-2")).unwrap_err();
        assert!(err.contains("injected write failure"));
        // The failed write changed nothing — memory and disk agree on at-1.
        assert_eq!(
            store.tokens_for("main").expect("stored").access_token,
            "at-1"
        );
        io.fail_writes
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let reloaded = CredentialStore::load(io).expect("reload");
        assert_eq!(
            reloaded.tokens_for("main").expect("stored").access_token,
            "at-1"
        );
    }
}

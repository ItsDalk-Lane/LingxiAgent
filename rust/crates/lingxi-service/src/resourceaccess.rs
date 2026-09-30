//! Resource authorization for native file tools (R04-T04).
//!
//! The incumbent's path authorization is `lib/sandbox/path-guard.ts`: every
//! path is resolved through the REAL filesystem (symlinks followed via
//! `realpathSync`; a missing target resolves through its nearest EXISTING
//! ancestor so `mkdir -p`-style targets are judged on the real parent) and
//! the ACCESS LEVEL is decided on the RESOLVED path, never on a string
//! prefix of the raw argument. The Rust mapping keeps exactly that shape:
//!
//! - [`ResourceAccess::resolve`] — lexical absolutization against the
//!   tool's working directory, then `fs::canonicalize`; an `ENOENT` target
//!   walks UP to the nearest existing ancestor, canonicalizes THAT, and
//!   rejoins the still-missing tail (the incumbent `_resolveReal`).
//! - [`ResourceAccess::authorize`] — the resolved REAL path is matched
//!   against the authorized roots BY COMPONENTS (`Path::strip_prefix`,
//!   never `String::starts_with`: `/tmp/ws` does not authorize
//!   `/tmp/ws-secret`), under the workspace + principal grant model:
//!   workspace roots are read-write for the service instance;
//!   per-(principal, session) grants add explicitly authorized roots with
//!   their own access level (an external read root never widens into a
//!   write). A path in no root is refused for every operation.
//! - The returned [`ResourceScope`] (canonical path + operation) is the
//!   single resource judgment every later stage binds to: the T02 gateway
//!   records it on the prepared invocation, the T03 approval record
//!   carries it (the approver sees WHICH real file), and the executor
//!   re-derives and re-verifies it at execution time.
//!
//! # Symlink / junction honesty (R04-A08)
//!
//! Authorization happens on the REAL target of a symlink chain, not on the
//! link's own path: a workspace link pointing at a restricted file outside
//! the authorized roots is refused by the REAL target's judgment (zero
//! side effects), and a link whose real target IS authorized operates on
//! the real target. The executor layer additionally opens the final path
//! with `O_NOFOLLOW` relative to the parent directory handle so a link
//! swapped in AFTER authorization cannot redirect the open (see
//! `filetools.rs`).
//!
//! Windows forms (junction / drive-letter / UNC paths) resolve through the
//! same `fs::canonicalize` + component matching; junction detection is
//! registered under `#[cfg(windows)]` with its own tests. This machine is
//! macOS — the Windows legs are compiled-and-registered, not
//! real-machine-verified (registered in the task report, per the stage's
//! Windows deferral).
//!
//! # Mapping decisions vs the incumbent (registered, honest)
//!
//! - The incumbent's `BLOCKED_FILES`/`BLOCKED_DIRS` (Lingxi home
//!   internals), agent read-only areas and `full-access` mode belong to
//!   the sandbox policy surface (T06) — this layer owns the
//!   workspace + grant judgment the FILE tools need; the PathGuard-shaped
//!   sandbox integration is deliberately T06's task (per the T04
//!   dispatch: "PathGuard 语义按 T06 深化，本 Task 资源判定即可").
//! - The incumbent's `allowExternalReads` global switch is replaced by
//!   EXPLICIT per-(principal, session) read grants: external reads are
//!   opt-in facts, never a blanket default.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// The operation a resource scope authorizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceOp {
    Read,
    Write,
}

impl ResourceOp {
    pub fn wire_name(self) -> &'static str {
        match self {
            ResourceOp::Read => "read",
            ResourceOp::Write => "write",
        }
    }
}

/// One authorized resource: the CANONICAL real path plus the operation
/// authorized on it. This is the value prepared invocations and approval
/// records bind to (the approver approves exactly this scope).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceScope {
    pub path: PathBuf,
    pub op: ResourceOp,
}

impl ResourceScope {
    pub fn new(path: PathBuf, op: ResourceOp) -> Self {
        Self { path, op }
    }
}

/// Why a resource access was refused. The refusal vocabulary mirrors the
/// incumbent's causes (`outside_write_scope` / `blocked` /
/// `unresolvable`) so the model-visible reason distinguishes "this path
/// is read-only for writes" from "this path is not in any authorized
/// root" — the same distinction `path-guard.ts` makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessRefusalCause {
    /// The real target is authorized for reads only (a read grant or an
    /// external read root) — a write/delete cannot happen there.
    OutsideWriteScope,
    /// The real target is in no authorized root at all.
    Blocked,
    /// The path could not be resolved to a real filesystem location
    /// (nonexistent ancestor chain, permission failure on the walk).
    Unresolvable,
}

impl AccessRefusalCause {
    pub fn wire_name(&self) -> &'static str {
        match self {
            AccessRefusalCause::OutsideWriteScope => "outside_write_scope",
            AccessRefusalCause::Blocked => "blocked",
            AccessRefusalCause::Unresolvable => "unresolvable",
        }
    }
}

/// A structured resource-access refusal (loud, zero side effects).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessRefusal {
    pub cause: AccessRefusalCause,
    pub requested_path: String,
    pub resolved_path: Option<String>,
    pub op: ResourceOp,
    pub message: String,
}

impl AccessRefusal {
    /// The stable refusal code for the gateway/executor vocabulary.
    pub fn code(&self) -> &'static str {
        match self.cause {
            AccessRefusalCause::OutsideWriteScope => "resource_outside_write_scope",
            AccessRefusalCause::Blocked => "resource_access_denied",
            AccessRefusalCause::Unresolvable => "resource_unresolvable",
        }
    }
}

impl std::fmt::Display for AccessRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message)
    }
}

impl std::error::Error for AccessRefusal {}

/// One per-(principal, session) granted root and its access level.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GrantedRoot {
    path: PathBuf,
    writable: bool,
}

/// The principal/session identity a grant is scoped to (the kernel
/// principal's storage identity — never model data).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GrantKey {
    principal_kind: &'static str,
    principal_subject: String,
    session_id: String,
}

#[derive(Default)]
struct AccessState {
    grants: HashMap<GrantKey, Vec<GrantedRoot>>,
}

/// The resource authorization boundary of the native file tools.
/// Interior-mutable, `Send + Sync`, owned by the service composition root
/// and shared by the file executors and the gateway's resource-scope
/// derivation.
///
/// Roots are stored CANONICAL: construction/grant time resolves them once
/// so later judgments compare real paths against real roots (an
/// authorization judged against a non-canonical root would be a string
/// comparison in disguise).
pub struct ResourceAccess {
    workspace_roots: Vec<PathBuf>,
    state: std::sync::Mutex<AccessState>,
}

impl ResourceAccess {
    /// Builds the access with the workspace roots (read-write). Roots are
    /// canonicalized; a root that cannot be resolved is a LOUD error — an
    /// unresolvable authorization root is a misconfiguration, never a
    /// silently-dropped scope.
    pub fn new(workspace_roots: &[PathBuf]) -> Result<Self, AccessRefusal> {
        let mut canonical = Vec::with_capacity(workspace_roots.len());
        for root in workspace_roots {
            let resolved = resolve_real(root).ok_or_else(|| AccessRefusal {
                cause: AccessRefusalCause::Unresolvable,
                requested_path: root.display().to_string(),
                resolved_path: None,
                op: ResourceOp::Read,
                message: format!(
                    "workspace root {} cannot be resolved to a real path; refusing to build \
                     the resource access on an unusable root",
                    root.display()
                ),
            })?;
            canonical.push(resolved);
        }
        Ok(Self {
            workspace_roots: canonical,
            state: std::sync::Mutex::new(AccessState::default()),
        })
    }

    /// Grants one additional root to exactly one (principal, session) —
    /// the incumbent's session-scoped `authorizedFolders` /
    /// `getExternalReadPaths` posture: one session's grant never widens
    /// another session's or another principal's access.
    pub fn grant_root(
        &self,
        principal_kind: &'static str,
        principal_subject: &str,
        session_id: &str,
        root: &Path,
        op: ResourceOp,
    ) -> Result<(), AccessRefusal> {
        let resolved = resolve_real(root).ok_or_else(|| AccessRefusal {
            cause: AccessRefusalCause::Unresolvable,
            requested_path: root.display().to_string(),
            resolved_path: None,
            op,
            message: format!(
                "granted root {} cannot be resolved to a real path; the grant is refused \
                 rather than recorded against a guessed location",
                root.display()
            ),
        })?;
        let key = GrantKey {
            principal_kind,
            principal_subject: principal_subject.to_string(),
            session_id: session_id.to_string(),
        };
        let granted = GrantedRoot {
            path: resolved,
            writable: matches!(op, ResourceOp::Write),
        };
        let mut state = self.lock();
        let roots = state.grants.entry(key).or_default();
        // Re-granting the same root at the same level is idempotent; a
        // WRITE grant subsumes a read grant for the same root.
        roots.retain(|existing| {
            !(existing.path == granted.path && existing.writable <= granted.writable)
        });
        roots.push(granted);
        Ok(())
    }

    /// Revokes every grant of one (principal, session).
    pub fn revoke_grants(
        &self,
        principal_kind: &'static str,
        principal_subject: &str,
        session_id: &str,
    ) {
        self.lock().grants.remove(&GrantKey {
            principal_kind,
            principal_subject: principal_subject.to_string(),
            session_id: session_id.to_string(),
        });
    }

    /// Resolves a raw tool path (absolute or relative to `cwd`) to the
    /// REAL filesystem path — symlinks followed, missing tails joined
    /// onto the nearest existing canonical ancestor. `None` when no
    /// ancestor of the path exists at all (or the walk fails).
    pub fn resolve(&self, raw: &Path, cwd: &Path) -> Option<PathBuf> {
        resolve_real(&absolutize(raw, cwd))
    }

    /// The single resource judgment: resolve the REAL target, then match
    /// it against the workspace roots and the (principal, session)
    /// grants. Authorization is BY COMPONENT on canonical paths —
    /// `/tmp/ws` never authorizes `/tmp/ws-secret/x`.
    pub fn authorize(
        &self,
        principal_kind: &'static str,
        principal_subject: &str,
        session_id: &str,
        raw_path: &Path,
        cwd: &Path,
        op: ResourceOp,
    ) -> Result<ResourceScope, AccessRefusal> {
        let requested = absolutize(raw_path, cwd);
        let Some(resolved) = resolve_real(&requested) else {
            return Err(AccessRefusal {
                cause: AccessRefusalCause::Unresolvable,
                requested_path: requested.display().to_string(),
                resolved_path: None,
                op,
                message: format!(
                    "{} {} cannot be resolved to a real filesystem path (no existing \
                     ancestor); the operation is refused",
                    op.wire_name(),
                    requested.display()
                ),
            });
        };
        // 1) Workspace roots: read-write for the instance.
        for root in &self.workspace_roots {
            if is_inside(&resolved, root) {
                return Ok(ResourceScope::new(resolved, op));
            }
        }
        // 2) This (principal, session)'s grants.
        let grants = {
            let state = self.lock();
            state
                .grants
                .get(&GrantKey {
                    principal_kind,
                    principal_subject: principal_subject.to_string(),
                    session_id: session_id.to_string(),
                })
                .cloned()
                .unwrap_or_default()
        };
        let mut read_authorized = false;
        for grant in &grants {
            if is_inside(&resolved, &grant.path) {
                if grant.writable {
                    return Ok(ResourceScope::new(resolved, op));
                }
                read_authorized = true;
            }
        }
        // A read grant authorizes READS of its tree (writes fall through
        // to the outside-write-scope refusal below).
        if read_authorized && matches!(op, ResourceOp::Read) {
            return Ok(ResourceScope::new(resolved, op));
        }
        let refusal_cause = if read_authorized && matches!(op, ResourceOp::Write) {
            AccessRefusalCause::OutsideWriteScope
        } else {
            AccessRefusalCause::Blocked
        };
        let message = match refusal_cause {
            AccessRefusalCause::OutsideWriteScope => format!(
                "write {} is outside the write scope: its real target {} is authorized for \
                 reads only; move the file into the workspace or grant write access to its \
                 directory",
                requested.display(),
                resolved.display()
            ),
            AccessRefusalCause::Blocked => format!(
                "{} {} is not inside any authorized root (real target {}); the operation is \
                 refused with zero side effects",
                op.wire_name(),
                requested.display(),
                resolved.display()
            ),
            AccessRefusalCause::Unresolvable => unreachable!("resolved above"),
        };
        Err(AccessRefusal {
            cause: refusal_cause,
            requested_path: requested.display().to_string(),
            resolved_path: Some(resolved.display().to_string()),
            op,
            message,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, AccessState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Lexical absolutization (no filesystem access): relative paths resolve
/// against `cwd`; `.`/`..`/root handling follows `Path` components.
fn absolutize(raw: &Path, cwd: &Path) -> PathBuf {
    if raw.is_absolute() {
        normalize_lexical(raw)
    } else {
        normalize_lexical(&cwd.join(raw))
    }
}

/// Lexically removes `.` components and resolves `..` against previous
/// components (a `..` that would climb above an absolute root is kept
/// literal — the later REAL resolution decides what it means; no
/// authorization is made on the lexical form).
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// Resolves to the REAL path (symlinks followed). When the exact path
/// does not exist, walks up to the nearest EXISTING ancestor, canonicalizes
/// it, and rejoins the missing tail — the incumbent `_resolveReal`, which
/// keeps `mkdir -p`-style targets judged on their real parent.
pub fn resolve_real(path: &Path) -> Option<PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(real) => Some(real),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let mut pending: Vec<std::ffi::OsString> = Vec::new();
            let mut current = path.to_path_buf();
            loop {
                let parent = current.parent()?.to_path_buf();
                pending.push(current.file_name()?.to_os_string());
                match std::fs::canonicalize(&parent) {
                    Ok(real_parent) => {
                        let mut real = real_parent;
                        for segment in pending.iter().rev() {
                            real.push(segment);
                        }
                        return Some(real);
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        current = parent;
                    }
                    Err(_) => return None,
                }
            }
        }
        Err(_) => None,
    }
}

/// Whether `target` is `base` itself or lies INSIDE `base` — compared by
/// COMPONENTS on canonical paths, never as a raw string prefix
/// (`/tmp/ws` must not authorize `/tmp/ws-secret`).
fn is_inside(target: &Path, base: &Path) -> bool {
    target == base || target.strip_prefix(base).is_ok()
}

// ── Windows forms (registered under cfg; not real-machine-verified here) ───

/// Windows reparse-point (junction / symlink) detection for the resource
/// boundary. On Windows `fs::canonicalize` resolves junctions and
/// symlinks alike, so the REAL-target judgment above needs no separate
/// path — this helper exists so the Windows build carries an explicit,
/// tested detection surface for diagnostics and the T06 sandbox work
/// (real-machine verification is the stage's Windows deferral).
#[cfg(windows)]
pub fn is_reparse_point(path: &Path) -> std::io::Result<bool> {
    use std::os::windows::fs::MetadataExt;
    // FILE_ATTRIBUTE_REPARSE_POINT = 0x400 (junctions and symlinks both
    // carry it).
    Ok(std::fs::symlink_metadata(path)?.file_attributes() & 0x400 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "lingxi-r04t04-access-{}-{}-{}",
            tag,
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp root");
        dir
    }

    #[test]
    fn workspace_authorizes_read_and_write_by_components() {
        let root = temp_root("ws");
        let ws = root.join("ws");
        std::fs::create_dir_all(ws.join("sub")).expect("dirs");
        let access = ResourceAccess::new(std::slice::from_ref(&ws)).expect("access");
        let cwd = ws.clone();
        for (raw, op, ok) in [
            ("sub/file.txt", ResourceOp::Read, true),
            ("sub/file.txt", ResourceOp::Write, true),
            ("./sub/../sub/file.txt", ResourceOp::Write, true),
            ("../sibling/file.txt", ResourceOp::Read, false),
            ("../ws-secret/file.txt", ResourceOp::Read, false),
        ] {
            let result =
                access.authorize("local_user", "user_local", "s1", Path::new(raw), &cwd, op);
            assert_eq!(result.is_ok(), ok, "{raw} {op:?} -> {result:?}");
        }
        // The workspace ROOT itself is inside the scope.
        assert!(access
            .authorize(
                "local_user",
                "user_local",
                "s1",
                Path::new("."),
                &cwd,
                ResourceOp::Write
            )
            .is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn similar_string_prefix_is_not_component_containment() {
        let root = temp_root("prefix");
        let ws = root.join("ws");
        std::fs::create_dir_all(&ws).expect("ws");
        let secret = root.join("ws-secret");
        std::fs::create_dir_all(&secret).expect("sibling");
        std::fs::write(secret.join("f.txt"), b"x").expect("file");
        let access = ResourceAccess::new(std::slice::from_ref(&ws)).expect("access");
        let refusal = access
            .authorize(
                "local_user",
                "user_local",
                "s1",
                &secret.join("f.txt"),
                &ws,
                ResourceOp::Read,
            )
            .expect_err("string-prefix sibling must be refused");
        assert_eq!(refusal.cause, AccessRefusalCause::Blocked);
        assert_eq!(refusal.code(), "resource_access_denied");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn symlink_is_judged_by_its_real_target() {
        let root = temp_root("symlink");
        let ws = root.join("ws");
        let restricted = root.join("restricted");
        std::fs::create_dir_all(&ws).expect("ws");
        std::fs::create_dir_all(&restricted).expect("restricted");
        std::fs::write(restricted.join("sentinel.txt"), b"secret").expect("sentinel");
        #[cfg(unix)]
        std::os::unix::fs::symlink("../restricted/sentinel.txt", ws.join("leak.txt"))
            .expect("symlink");
        #[cfg(unix)]
        {
            let access = ResourceAccess::new(std::slice::from_ref(&ws)).expect("access");
            for op in [ResourceOp::Read, ResourceOp::Write] {
                let refusal = access
                    .authorize(
                        "local_user",
                        "user_local",
                        "s1",
                        &ws.join("leak.txt"),
                        &ws,
                        op,
                    )
                    .expect_err("the real target is outside every root");
                assert_eq!(refusal.cause, AccessRefusalCause::Blocked, "{op:?}");
                assert!(
                    refusal
                        .resolved_path
                        .as_deref()
                        .unwrap_or_default()
                        .contains("restricted"),
                    "the refusal names the REAL target: {refusal:?}"
                );
            }
            // A link whose real target IS inside the workspace authorizes
            // and resolves to the REAL path.
            std::fs::write(ws.join("real.txt"), b"ok").expect("real");
            std::os::unix::fs::symlink("real.txt", ws.join("alias.txt")).expect("inner link");
            let scope = access
                .authorize(
                    "local_user",
                    "user_local",
                    "s1",
                    &ws.join("alias.txt"),
                    &ws,
                    ResourceOp::Write,
                )
                .expect("real target inside the workspace");
            assert!(scope.path.ends_with("real.txt"), "{:?}", scope.path);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn per_principal_grants_scoped_and_leveled() {
        let root = temp_root("grants");
        let ws = root.join("ws");
        let external = root.join("external");
        std::fs::create_dir_all(&ws).expect("ws");
        std::fs::create_dir_all(&external).expect("external");
        std::fs::write(external.join("doc.txt"), b"doc").expect("doc");
        let access = ResourceAccess::new(std::slice::from_ref(&ws)).expect("access");
        access
            .grant_root(
                "local_user",
                "user_local",
                "s1",
                &external,
                ResourceOp::Read,
            )
            .expect("grant");
        // The granted principal+session can read the external root…
        assert!(access
            .authorize(
                "local_user",
                "user_local",
                "s1",
                &external.join("doc.txt"),
                &ws,
                ResourceOp::Read
            )
            .is_ok());
        // …cannot write it (read grant only)…
        let refusal = access
            .authorize(
                "local_user",
                "user_local",
                "s1",
                &external.join("doc.txt"),
                &ws,
                ResourceOp::Write,
            )
            .expect_err("read grant never widens to write");
        assert_eq!(refusal.cause, AccessRefusalCause::OutsideWriteScope);
        assert_eq!(refusal.code(), "resource_outside_write_scope");
        // …and neither can another session (the grant is session-scoped).
        let refusal = access
            .authorize(
                "local_user",
                "user_local",
                "s2",
                &external.join("doc.txt"),
                &ws,
                ResourceOp::Read,
            )
            .expect_err("another session has no grant");
        assert_eq!(refusal.cause, AccessRefusalCause::Blocked);
        // Revocation removes the grant entirely.
        access.revoke_grants("local_user", "user_local", "s1");
        assert!(access
            .authorize(
                "local_user",
                "user_local",
                "s1",
                &external.join("doc.txt"),
                &ws,
                ResourceOp::Read
            )
            .is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_target_resolves_through_its_real_parent() {
        let root = temp_root("missing");
        let ws = root.join("ws");
        std::fs::create_dir_all(&ws).expect("ws");
        let access = ResourceAccess::new(std::slice::from_ref(&ws)).expect("access");
        // The tail does not exist yet — the judgment lands on the nearest
        // existing ancestor (the workspace), so a write that will CREATE
        // the file is authorizable.
        let scope = access
            .authorize(
                "local_user",
                "user_local",
                "s1",
                Path::new("deep/new/file.txt"),
                &ws,
                ResourceOp::Write,
            )
            .expect("missing tail inside an existing root");
        assert!(scope.path.ends_with("deep/new/file.txt"));
        // A missing tail outside every root stays refused.
        assert!(access
            .authorize(
                "local_user",
                "user_local",
                "s1",
                Path::new("../missing-outside/f.txt"),
                &ws,
                ResourceOp::Read
            )
            .is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn case_insensitive_filesystems_resolve_to_the_real_casing() {
        // macOS default filesystems are case-insensitive: a differently
        // cased path canonicalizes to the ON-DISK casing, so the scope
        // judgment and the executor operate on the real file identity.
        let root = temp_root("case");
        let ws = root.join("ws");
        std::fs::create_dir_all(&ws).expect("ws");
        std::fs::write(ws.join("Note.TXT"), b"casing").expect("file");
        let access = ResourceAccess::new(std::slice::from_ref(&ws)).expect("access");
        let scope = access
            .authorize(
                "local_user",
                "user_local",
                "s1",
                Path::new("note.txt"),
                &ws,
                ResourceOp::Read,
            )
            .expect("case-insensitive resolution finds the real file");
        assert!(scope.path.ends_with("Note.TXT"), "{:?}", scope.path);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unresolvable_root_is_a_loud_construction_error() {
        // /.. climbs past the root lexically; on the real FS this resolves
        // to "/" (existing) on unix, so instead point at a nonexistent
        // chain whose ancestor walk bottoms out at an existing file (not a
        // directory): canonicalize of "file/x" fails with NotADirectory —
        // not NotFound — and must surface as unresolvable.
        let root = temp_root("badroot");
        let ws = root.join("ws");
        std::fs::create_dir_all(&ws).expect("ws");
        std::fs::write(ws.join("plain.txt"), b"f").expect("file");
        assert!(ResourceAccess::new(&[ws.join("plain.txt").join("deeper")]).is_err());
        // And the authorize surface maps the same shape to a refusal.
        let access = ResourceAccess::new(std::slice::from_ref(&ws)).expect("access");
        let refusal = access
            .authorize(
                "local_user",
                "user_local",
                "s1",
                Path::new("plain.txt/under/a/file.txt"),
                &ws,
                ResourceOp::Read,
            )
            .expect_err("a file cannot contain a directory");
        assert_eq!(refusal.cause, AccessRefusalCause::Unresolvable);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn windows_junctions_are_detectable() {
        let root = temp_root("junction");
        let target = root.join("target");
        std::fs::create_dir_all(&target).expect("target");
        let junction = root.join("junction");
        // Create a junction via `mklink /J` semantics is shell-level; the
        // detection surface is exercised against a SYMLINK here (both are
        // reparse points). Registered form: real-machine verification is
        // the stage's Windows deferral.
        std::os::windows::fs::symlink_dir(&target, &junction).expect("symlink dir");
        assert!(is_reparse_point(&junction).expect("reparse check"));
        assert!(!is_reparse_point(&target).expect("plain dir"));
        let _ = std::fs::remove_dir_all(&root);
    }
}

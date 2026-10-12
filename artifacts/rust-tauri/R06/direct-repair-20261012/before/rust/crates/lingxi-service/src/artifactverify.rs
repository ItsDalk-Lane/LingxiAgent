//! R04-T08 / R04-A15: verification of DELIVERED file artifacts.
//!
//! "Success" must have observable evidence: a tool (or the untrusted
//! worker/MCP process behind it) merely CLAIMING a file was produced is
//! never enough to register that file as a valid deliverable. This module
//! owns the two verification layers the stage book demands:
//!
//! 1. **Claimed-artifact verification** (the untrusted boundary): before a
//!    worker's `claimed_files` entry becomes a local [`ResourceRef`], the
//!    path must (a) sit inside one of the invocation's GRANTED resource
//!    scopes, (b) exist, (c) be a REGULAR file (a directory or a dangling
//!    symlink is not a file deliverable), and (d) satisfy the tool's
//!    claimed-file CONTRACT (minimum size / basic format — the
//!    "基本格式/内容条件" of the stage book). A claim failing any of
//!    these is refused loudly and nothing is minted.
//!
//! 2. **Gateway registration audit** (the single delivery-registration
//!    point): when the gateway settles a SUCCESS, every `file://`
//!    resource reference the executor produced is re-verified against the
//!    filesystem RIGHT THEN — it must exist and be a regular file. The
//!    executor already ran, so a failure here does NOT pretend zero
//!    dispatch: the outcome is converted to an explicit FAILED whose
//!    message states that the execution happened, that side effects may
//!    exist, and that the artifact is NOT registered as a valid
//!    deliverable ("请求已发送"绝不冒称"文件已生成"——反向同理:已执行
//!    也不冒称已交付).
//!
//! Remote resources are deliberately NOT forced through the local-file
//! audit: an MCP `file:///…` URI from an untrusted server never becomes a
//! local ResourceRef in the first place (the T07 bridge maps remote
//! resources to text descriptions), so there is nothing local to verify —
//! that is the "远端资源按其类型验证" posture: remote identity is
//! verified by its own type (validated structured output / honest text),
//! never by pretending it is a local file.

use std::path::Path;

use crate::resourceaccess::ResourceScope;

/// How many bytes of a claimed artifact the content conditions may read
/// (bounded — a claimed contract never gives an untrusted claim a way to
/// make the host read unbounded data).
pub const ARTIFACT_CONTRACT_READ_CAP: usize = 64 * 1024;

/// Why a delivered/claimed artifact was refused (stable codes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactRejection {
    /// The path does not exist on disk.
    Missing { path: String },
    /// The path exists but is not a regular file (directory, fifo…).
    NotARegularFile { path: String },
    /// The path is outside every granted scope of the invocation.
    OutOfScope { path: String },
    /// The file exists but violates the claimed-file contract.
    ContentConditionFailed { path: String, detail: String },
}

impl ArtifactRejection {
    pub fn code(&self) -> &'static str {
        match self {
            ArtifactRejection::Missing { .. } => "artifact_missing",
            ArtifactRejection::NotARegularFile { .. } => "artifact_not_a_regular_file",
            ArtifactRejection::OutOfScope { .. } => "artifact_out_of_scope",
            ArtifactRejection::ContentConditionFailed { .. } => "artifact_content_condition_failed",
        }
    }

    pub fn message(&self) -> String {
        match self {
            ArtifactRejection::Missing { path } => {
                format!("claimed artifact {path:?} does not exist on disk")
            }
            ArtifactRejection::NotARegularFile { path } => format!(
                "claimed artifact {path:?} is not a regular file (a directory or special file \
                 is not a file deliverable)"
            ),
            ArtifactRejection::OutOfScope { path } => format!(
                "claimed artifact {path:?} is outside every granted resource scope of this \
                 invocation"
            ),
            ArtifactRejection::ContentConditionFailed { path, detail } => {
                format!("claimed artifact {path:?} violates its content contract: {detail}")
            }
        }
    }
}

/// The basic format a claimed artifact must have (the per-tool
/// "基本格式" condition; `Any` = existence + regular file only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClaimedFileFormat {
    #[default]
    Any,
    /// The bytes must be valid UTF-8.
    Utf8Text,
    /// The bytes must parse as one JSON value.
    Json,
}

impl ClaimedFileFormat {
    pub fn wire_name(self) -> &'static str {
        match self {
            ClaimedFileFormat::Any => "any",
            ClaimedFileFormat::Utf8Text => "utf8-text",
            ClaimedFileFormat::Json => "json",
        }
    }
}

/// The content contract of a claimed file artifact (the per-tool
/// "内容条件"). `min_bytes == 0` allows an empty file — emptiness is a
/// legitimate product of, e.g., a truncation; only a tool that PROMISES
/// non-empty/structured output declares the stricter contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClaimedFileContract {
    pub min_bytes: u64,
    pub format: ClaimedFileFormat,
}

/// Whether `path` sits inside one of the granted scopes. Component-wise
/// (`Path::starts_with` compares components, never string prefixes), with
/// a `canonicalize` fallback so a claim through a symlink inside the
/// workspace is judged by its real target — the same fail-closed shape
/// the T07 worker executor used, factored here as the one authority.
pub fn path_within_scopes(path: &Path, scopes: &[ResourceScope]) -> bool {
    scopes.iter().any(|scope| {
        path.starts_with(&scope.path)
            || std::fs::canonicalize(path)
                .map(|real| real.starts_with(&scope.path))
                .unwrap_or(false)
    })
}

/// Verifies one CLAIMED artifact against the invocation's grants and the
/// tool's contract (layer 1). `Ok(())` means the claim may be minted as a
/// local ResourceRef; every rejection is loud and mints nothing.
pub fn verify_claimed_artifact(
    path: &Path,
    scopes: &[ResourceScope],
    contract: &ClaimedFileContract,
) -> Result<(), ArtifactRejection> {
    let path_text = path.display().to_string();
    if !path_within_scopes(path, scopes) {
        return Err(ArtifactRejection::OutOfScope { path: path_text });
    }
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return Err(ArtifactRejection::Missing { path: path_text }),
    };
    // `metadata` follows symlinks: the REAL target must be a regular file.
    if !metadata.is_file() {
        return Err(ArtifactRejection::NotARegularFile { path: path_text });
    }
    let size = metadata.len();
    if size < contract.min_bytes {
        return Err(ArtifactRejection::ContentConditionFailed {
            path: path_text,
            detail: format!(
                "{size} bytes is under the {} byte minimum",
                contract.min_bytes
            ),
        });
    }
    match contract.format {
        ClaimedFileFormat::Any => Ok(()),
        ClaimedFileFormat::Utf8Text => {
            let bytes = read_bounded(path).map_err(|_| ArtifactRejection::Missing {
                path: path_text.clone(),
            })?;
            match std::str::from_utf8(&bytes) {
                Ok(_) => Ok(()),
                Err(err) => Err(ArtifactRejection::ContentConditionFailed {
                    path: path_text,
                    detail: format!("not valid UTF-8 ({err})"),
                }),
            }
        }
        ClaimedFileFormat::Json => {
            let bytes = read_bounded(path).map_err(|_| ArtifactRejection::Missing {
                path: path_text.clone(),
            })?;
            match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(_) => Ok(()),
                Err(err) => Err(ArtifactRejection::ContentConditionFailed {
                    path: path_text,
                    detail: format!("not valid JSON ({err})"),
                }),
            }
        }
    }
}

/// Reads at most [`ARTIFACT_CONTRACT_READ_CAP`] bytes (a content contract
/// is a BOUNDED check; an untrusted claim never widens host reads).
fn read_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(ARTIFACT_CONTRACT_READ_CAP.try_into().unwrap_or(u64::MAX))
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// The file URI scheme every local file ResourceRef uses.
pub const FILE_URI_PREFIX: &str = "file://";

/// Extracts the local path of a `file://` resource URI (`None` for any
/// other URI — remote resources are verified by their own type, never
/// forced through a local-file audit).
pub fn local_path_of_uri(uri: &str) -> Option<&str> {
    uri.strip_prefix(FILE_URI_PREFIX)
}

/// The gateway registration audit (layer 2): one delivered `file://`
/// reference must exist NOW and be a regular file. Returns the observed
/// size on success. This is deliberately the MINIMAL universal check the
/// gateway can derive on its own; scope/contract conditions belong to the
/// boundary that owns them (the worker grant check, the file tools'
/// resource extractor).
pub fn audit_delivered_file(path: &Path) -> Result<u64, ArtifactRejection> {
    let path_text = path.display().to_string();
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return Err(ArtifactRejection::Missing { path: path_text }),
    };
    if !metadata.is_file() {
        return Err(ArtifactRejection::NotARegularFile { path: path_text });
    }
    Ok(metadata.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resourceaccess::ResourceOp;

    fn scope(path: &std::path::Path) -> ResourceScope {
        ResourceScope {
            op: ResourceOp::Write,
            path: path.to_path_buf(),
        }
    }

    #[test]
    fn directory_claims_are_not_file_deliverables() {
        let dir = std::env::temp_dir().join(format!(
            "artifactverify-dir-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("dir");
        let scopes = vec![scope(&dir)];
        // The directory itself claimed as an artifact: exists, but not a
        // regular file — refused (the pre-A15 `exists()`-only check would
        // have minted it).
        let rejection = verify_claimed_artifact(&dir, &scopes, &ClaimedFileContract::default())
            .expect_err("a directory is not a file artifact");
        assert_eq!(rejection.code(), "artifact_not_a_regular_file");
        // A file INSIDE the scope passes the basic contract.
        let file = dir.join("out.txt");
        std::fs::write(&file, "data").expect("file");
        assert!(verify_claimed_artifact(&file, &scopes, &ClaimedFileContract::default()).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn out_of_scope_and_missing_claims_are_refused() {
        let root = std::env::temp_dir().join(format!(
            "artifactverify-scope-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("granted")).expect("granted");
        std::fs::create_dir_all(root.join("outside")).expect("outside");
        let scopes = vec![scope(&root.join("granted"))];
        let outside = root.join("outside").join("x.txt");
        std::fs::write(&outside, "x").expect("outside file");
        let rejection = verify_claimed_artifact(&outside, &scopes, &ClaimedFileContract::default())
            .expect_err("outside the grant");
        assert_eq!(rejection.code(), "artifact_out_of_scope");
        let missing = root.join("granted").join("never.txt");
        let rejection = verify_claimed_artifact(&missing, &scopes, &ClaimedFileContract::default())
            .expect_err("missing");
        assert_eq!(rejection.code(), "artifact_missing");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn content_conditions_are_enforced_bounded() {
        let root = std::env::temp_dir().join(format!(
            "artifactverify-contract-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("root");
        let scopes = vec![scope(&root)];
        let json_file = root.join("good.json");
        std::fs::write(&json_file, "{\"ok\": true}").expect("json");
        let text_file = root.join("plain.txt");
        std::fs::write(&text_file, "not json").expect("text");
        let empty_file = root.join("empty.bin");
        std::fs::write(&empty_file, b"").expect("empty");

        let json_contract = ClaimedFileContract {
            min_bytes: 1,
            format: ClaimedFileFormat::Json,
        };
        assert!(verify_claimed_artifact(&json_file, &scopes, &json_contract).is_ok());
        let rejection =
            verify_claimed_artifact(&text_file, &scopes, &json_contract).expect_err("not json");
        assert_eq!(rejection.code(), "artifact_content_condition_failed");
        let rejection = verify_claimed_artifact(&empty_file, &scopes, &json_contract)
            .expect_err("empty under the minimum");
        assert_eq!(rejection.code(), "artifact_content_condition_failed");
        // An empty file with NO contract declared is a legal deliverable
        // (emptiness can be the honest product of a real execution).
        assert!(
            verify_claimed_artifact(&empty_file, &scopes, &ClaimedFileContract::default()).is_ok()
        );
        // Invalid UTF-8 fails the Utf8Text contract but not Any.
        let bin_file = root.join("bytes.bin");
        std::fs::write(&bin_file, [0xff, 0xfe, 0x00]).expect("binary");
        let utf8_contract = ClaimedFileContract {
            min_bytes: 0,
            format: ClaimedFileFormat::Utf8Text,
        };
        assert!(
            verify_claimed_artifact(&bin_file, &scopes, &utf8_contract).is_err(),
            "invalid UTF-8 fails the utf8 contract"
        );
        assert!(
            verify_claimed_artifact(&bin_file, &scopes, &ClaimedFileContract::default()).is_ok()
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn uri_extraction_and_gateway_audit_shape() {
        assert_eq!(local_path_of_uri("file:///tmp/x.txt"), Some("/tmp/x.txt"));
        assert_eq!(local_path_of_uri("https://example.com/x"), None);
        let root = std::env::temp_dir().join(format!(
            "artifactverify-audit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("root");
        let file = root.join("delivered.txt");
        std::fs::write(&file, "12345").expect("file");
        assert_eq!(audit_delivered_file(&file), Ok(5));
        let rejection = audit_delivered_file(&root).expect_err("a directory is not delivered");
        assert_eq!(rejection.code(), "artifact_not_a_regular_file");
        let rejection = audit_delivered_file(&root.join("gone.txt")).expect_err("missing");
        assert_eq!(rejection.code(), "artifact_missing");
        std::fs::remove_dir_all(&root).ok();
    }
}

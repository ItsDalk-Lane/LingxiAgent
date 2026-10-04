//! The credential resolution port (R05-T02 C01): the SINGLE material exit
//! of the whole model plane. The production implementation is the
//! service-side `CredentialService`; adapters consume it through this
//! trait. No caller may hold a backup key path — [`crate::models::gateway`]
//! deliberately has no credential-material accessor anymore.
//!
//! Trust split: kernel types carry
//! [`lingxi_kernel::model_exchange::CredentialReference`] (identity + kind);
//! the material ([`ApplicableAuth`]) exists only behind this port, and every
//! error text produced here is scrubbed of material before it can travel
//! into kernel messages, events or logs (C09).

use lingxi_kernel::model_exchange::ResolvedModelRoute;

/// The material for ONE dispatch of ONE resolved route. Cloning is
/// deliberate (the adapter may need one bounded retry after a refresh) but
/// this type never leaves the service/adapter boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplicableAuth {
    /// Explicitly keyless route (`auth: {"kind": "none"}` — the local
    /// no-credential contract, never defaulted onto a remote endpoint).
    None,
    /// `Authorization: Bearer <token>` — a static API key or an OAuth
    /// access token.
    Bearer(String),
    /// One custom auth header sent verbatim.
    Header { name: String, value: String },
}

impl ApplicableAuth {
    /// The material strings this auth carries (for exact-match scrubbing of
    /// any text that crosses a trust boundary — C09).
    pub fn materials(&self) -> Vec<&str> {
        match self {
            ApplicableAuth::None => Vec::new(),
            ApplicableAuth::Bearer(token) => vec![token.as_str()],
            ApplicableAuth::Header { value, .. } => vec![value.as_str()],
        }
    }
}

/// Credential failures, classified so the caller never conflates them
/// (R05-T02): a configuration gap, a revoked/expired grant needing
/// RE-AUTHORIZATION, a transient transport failure and a persistence
/// failure are different stories with different retry semantics. No
/// variant's text ever carries credential material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialError {
    /// The provider/route has no credential configuration (explicit
    /// unconfigured state — never an anonymous request).
    NotConfigured { provider: String },
    /// The credential was revoked or deleted (locally or observed from the
    /// server). Re-authorization is required; no automatic retry.
    Revoked { provider: String },
    /// The refresh grant is dead (`invalid_grant` family) — the user must
    /// re-authorize. Distinct from `Revoked`: the local store may still
    /// hold a token, it just stopped being honored.
    ReauthorizationRequired { provider: String, detail: String },
    /// The OAuth login flow is required but no token exists yet.
    NotLoggedIn { provider: String },
    /// Network/5xx/timeout while talking to the auth endpoint. Bounded
    /// retry is the caller's own policy; there is no unbounded 401 loop
    /// anywhere (C07).
    Transient { provider: String, detail: String },
    /// The new credential was minted but could NOT be persisted (C06):
    /// never reported as safely saved; the previous on-disk state remains
    /// the recoverable one.
    PersistenceFailed { provider: String, detail: String },
    /// A credential handle presented for resolution is unknown, expired,
    /// stale-generation or bound to another provider (C12: forged or
    /// misused handles are refused loudly).
    HandleRefused { provider: String, detail: String },
}

impl CredentialError {
    /// The provider this failure is scoped to (failures never cross
    /// providers — there is no cross-provider fallback, C02/C07).
    pub fn provider(&self) -> &str {
        match self {
            CredentialError::NotConfigured { provider }
            | CredentialError::Revoked { provider }
            | CredentialError::ReauthorizationRequired { provider, .. }
            | CredentialError::NotLoggedIn { provider }
            | CredentialError::Transient { provider, .. }
            | CredentialError::PersistenceFailed { provider, .. }
            | CredentialError::HandleRefused { provider, .. } => provider,
        }
    }
}

impl std::fmt::Display for CredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CredentialError::NotConfigured { provider } => write!(
                f,
                "provider {provider:?} has no credential configuration (explicit state, \
                 never an anonymous request)"
            ),
            CredentialError::Revoked { provider } => write!(
                f,
                "provider {provider:?} credential was revoked; re-authorization required"
            ),
            CredentialError::ReauthorizationRequired { provider, detail } => write!(
                f,
                "provider {provider:?} refresh grant is no longer honored \
                 (re-authorization required): {detail}"
            ),
            CredentialError::NotLoggedIn { provider } => write!(
                f,
                "provider {provider:?} uses OAuth but holds no token (login required)"
            ),
            CredentialError::Transient { provider, detail } => write!(
                f,
                "provider {provider:?} credential refresh failed transiently: {detail}"
            ),
            CredentialError::PersistenceFailed { provider, detail } => write!(
                f,
                "provider {provider:?} credential was refreshed but could NOT be \
                 persisted: {detail} (the previous on-disk credential remains the \
                 recoverable state)"
            ),
            CredentialError::HandleRefused { provider, detail } => write!(
                f,
                "provider {provider:?} credential handle refused: {detail}"
            ),
        }
    }
}

impl std::error::Error for CredentialError {}

/// The verdict of one coordinated refresh wait (C02/C04/C05/C06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshVerdict {
    /// A credential minted at or after the caller's stale generation is now
    /// active. `persisted: false` means the on-disk state is still the OLD
    /// credential (C06 — never reported as safely saved; the in-memory
    /// credential is real and usable, the failure is surfaced, and the
    /// status surface keeps reporting it until a later persist succeeds).
    Refreshed { generation: u64, persisted: bool },
    /// The credential was revoked while the caller waited — a late token
    /// was discarded, never written back (C05). No retry.
    Revoked,
    /// The refresh grant is dead — re-authorization required. No retry.
    ReauthorizationRequired { detail: String },
    /// The auth endpoint failed transiently (network/5xx/timeout). The
    /// caller's own bounded retry policy applies.
    Transient { detail: String },
    /// This credential shape cannot refresh (a static API key / auth
    /// header): a 401 is terminal for it.
    NotRefreshable,
}

/// The single credential-resolution entry of the model plane (C01). The
/// production implementation (service-side CredentialService) is seeded
/// from the SAME provider config section the gateway resolves routes from —
/// the provider is the credential owner; every model/purpose references the
/// provider and never stores a key of its own.
pub trait ProviderCredentialPort: Send + Sync {
    /// Resolves the CURRENT material for one dispatch of one resolved
    /// route. For OAuth providers a locally-expired token triggers the
    /// coordinated refresh here (same single-flight as the 401 path).
    fn resolve<'a>(
        &'a self,
        route: &'a ResolvedModelRoute,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ApplicableAuth, CredentialError>> + Send + 'a>,
    >;

    /// Reports that the server rejected (401) the credential that was just
    /// used for `route`. Joins (or starts) the provider's single-flight
    /// refresh and waits for its verdict. If the active credential already
    /// moved past the one the caller used, the verdict is `Refreshed`
    /// WITHOUT a second network refresh (the incumbent force-refresh
    /// semantics: someone else already rotated — never burn the fresh
    /// refresh token).
    fn report_unauthorized<'a>(
        &'a self,
        route: &'a ResolvedModelRoute,
        used: &'a ApplicableAuth,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<RefreshVerdict, CredentialError>> + Send + 'a>,
    >;
}

/// Exact-match scrub of KNOWN credential material from a text that is about
/// to cross a trust boundary (a provider error echo, a diagnostic, an event
/// payload — C09). This is the primary defense at the production site; the
/// service's pattern-based log redactor is the second net.
pub fn scrub_materials(text: &str, materials: &[&str]) -> String {
    let mut out = text.to_string();
    for material in materials {
        if material.is_empty() {
            continue;
        }
        out = out.replace(*material, "[redacted]");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_materials_removes_every_occurrence_and_ignores_empty() {
        let text = "echo access=AT-1 refresh=RT-1 again AT-1";
        let scrubbed = scrub_materials(text, &["AT-1", "RT-1", ""]);
        assert_eq!(
            scrubbed,
            "echo access=[redacted] refresh=[redacted] again [redacted]"
        );
        // Non-material survives untouched.
        assert_eq!(scrub_materials("plain", &["X"]), "plain");
    }

    #[test]
    fn applicable_auth_lists_its_materials() {
        assert!(ApplicableAuth::None.materials().is_empty());
        assert_eq!(ApplicableAuth::Bearer("t".into()).materials(), vec!["t"]);
        assert_eq!(
            ApplicableAuth::Header {
                name: "X-Key".into(),
                value: "v".into()
            }
            .materials(),
            vec!["v"]
        );
    }

    #[test]
    fn credential_error_text_never_names_material_shape_fields() {
        // The Display texts are the ones that can land in events/logs; they
        // carry provider ids and classifications, never values.
        let err = CredentialError::ReauthorizationRequired {
            provider: "main".into(),
            detail: "invalid_grant".into(),
        };
        let text = err.to_string();
        assert!(text.contains("main"));
        assert!(text.contains("re-authorization"));
        assert_eq!(err.provider(), "main");
    }
}

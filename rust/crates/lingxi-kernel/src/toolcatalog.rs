//! Tool catalog and parameter contract (R04-T01).
//!
//! Discovery, description, permission and execution must point at ONE real
//! tool, never at "a few similar names". This module owns that single
//! authority for the Rust stack:
//!
//! - [`ToolTargetId`] — the stable, namespaced target identity (origin +
//!   local name), deliberately separate from the display name. The byte
//!   format mirrors the incumbent TypeScript registry
//!   (`lib/tools/invocation/identity.ts`) so one tool is the SAME identity
//!   on both sides of the migration.
//! - [`ToolManifest`] — source, names, aliases, version, input/output
//!   schema document, permission contract, availability, timeout,
//!   concurrency bound, source-DECLARED permission claim and the VERIFIED
//!   recovery capability.
//! - [`ToolRegistry`] — registration, alias resolution, name-collision
//!   handling (same name from two sources never overwrites — both are
//!   indexed and a name-only reference to the contested name is an
//!   explicit ambiguity; an ALIAS colliding with any other target's
//!   name/alias is refused outright), the catalog GENERATION ladder and
//!   the lifecycle operations (update / disable / uninstall) that advance
//!   it, plus the pinned-generation prepare path.
//! - [`EffectiveArguments`] — the complete, immutable, validated and
//!   normalized argument object every executor consumes; digests and
//!   summaries are DERIVED from it by this trusted boundary, never
//!   accepted from the model.
//!
//! Boundary rules this module enforces (taskbook R04-T01 怎么做 3/5):
//! - schema size, nesting depth, property counts and validation cost are
//!   BOUNDED ([`SchemaBudget`]); exceeding a bound is a loud error, never
//!   a silent clamp;
//! - the schema dialect is checked against the protocol's
//!   [`lingxi_protocol::SUPPORTED_SCHEMA_DIALECTS`]; an unknown dialect is
//!   an explicit `unknown_schema_dialect` failure;
//! - `$ref` / `$id` / `$defs` / recursive-reference vocabulary is
//!   explicitly unsupported — NO reference resolution exists here, and no
//!   network fetch will ever be added implicitly;
//! - validation keywords this boundary cannot honor exactly (`pattern`,
//!   `if/then/else`, combinators, …) make the SCHEMA itself incompatible
//!   (registering it fails) — silently ignoring a validation keyword would
//!   be lenient parsing, which contract §8 forbids; vendor annotation
//!   keys (`x-…` and unknown non-validation keys) are preserved verbatim
//!   and simply not interpreted;
//! - a tool source DECLARING itself read-only never grants the read-only
//!   recovery classification: [`ToolManifest::recovery`] is supplied by
//!   the registrar as a VERIFIED fact and defaults to
//!   [`crate::invocation::ToolRecoveryCapability::CONSERVATIVE`];
//! - effective arguments are integer-safe: floats and integers outside
//!   the ±2^53 safe range are rejected at this boundary (the
//!   cross-language canonicalization ruling — RR-T02-F2), and object keys
//!   sort in UTF-16 code-unit order in the canonical profile so Rust and
//!   TypeScript digests agree for ALL key shapes (RR-T02-F1).
//!
//! R04-T02's gateway is the consumer of [`ToolRegistry::prepare_invocation`]:
//! the returned [`PreparedToolCall`] is exactly the identity + generation +
//! effective-arguments + permission-summary input the gateway, approvals
//! and the run driver bind to. This module never executes anything.

use std::collections::BTreeMap;
use std::fmt;

use lingxi_protocol::canon::CANONICALIZATION_ID;
use lingxi_protocol::{check_schema_dialect, ArgsDigest, ToolSchemaDocument};

use crate::invocation::ToolRecoveryCapability;

// ── Errors ──────────────────────────────────────────────────────────────────

/// The loud failure vocabulary of the catalog boundary. Every variant is a
/// caller-visible, machine-diagnosable refusal; none is ever degraded into
/// success or a guessed default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCatalogError {
    /// The schema dialect is not one this boundary can validate.
    UnknownDialect {
        dialect: String,
        supported: Vec<&'static str>,
    },
    /// The schema uses a keyword this boundary refuses to pretend to
    /// understand (silently ignoring it would widen validation).
    UnsupportedSchemaFeature { keyword: String, at: String },
    /// A reference keyword (`$ref`/`$id`/`$defs`/…) appeared. Reference
    /// resolution — local OR remote — is not implemented and remote
    /// resolution will never be implicit.
    SchemaReferenceUnsupported { keyword: String, at: String },
    /// The schema document exceeds its size/depth/property budget.
    SchemaBudgetExceeded {
        bound: &'static str,
        found: usize,
        max: usize,
    },
    /// The schema document is malformed for a supported dialect (e.g. a
    /// keyword holds a value of the wrong shape).
    SchemaMalformed { at: String, detail: String },
    /// The manifest itself is invalid (empty names, missing pieces).
    ManifestInvalid { detail: String },
    /// A target with the same [`ToolTargetId`] is already registered.
    DuplicateTarget { target_id: String },
    /// An ALIAS of the new manifest collides with another target's
    /// name/alias, or the new PRIMARY name collides with another target's
    /// ALIAS: one visible name must never mint a second identity. The
    /// collision is reported, never overwritten. (Two PRIMARY names equal
    /// across different SOURCES is legal: both are indexed and a
    /// name-only reference to the contested name is an explicit
    /// [`ToolCatalogError::TargetAmbiguous`].)
    NameCollision {
        name: String,
        existing_target_id: String,
        incoming_target_id: String,
    },
    /// No target matches the reference.
    TargetNotFound { reference: String },
    /// The name alone matches MORE than one target (same primary name from
    /// different sources): the caller must qualify the source.
    TargetAmbiguous {
        name: String,
        target_ids: Vec<String>,
    },
    /// The target exists but is not callable in its current lifecycle
    /// state (disabled / not installed / future capability honestly not
    /// implemented yet).
    TargetNotCallable {
        target_id: String,
        availability: &'static str,
        reason: String,
    },
    /// The caller's catalog view (pinned generation) is stale — the
    /// registry has advanced. The request must be re-prepared against a
    /// FRESH snapshot; executing the new object under the old description
    /// is forbidden (R04-A02).
    StaleCatalog {
        held_generation: u64,
        current_generation: u64,
        target_id: String,
    },
    /// The raw arguments are not a JSON object.
    ArgumentsNotObject,
    /// The arguments violate the input schema (each violation named).
    ArgumentsInvalid { violations: Vec<String> },
    /// The arguments exceed their size/depth budget.
    ArgumentsBudgetExceeded {
        bound: &'static str,
        found: usize,
        max: usize,
    },
    /// A number in the arguments is not a safe integer (float or beyond
    /// ±2^53). Cross-language canonical digests cannot represent it — the
    /// TS side hard-fails, so this boundary hard-fails too (RR-T02-F2).
    ArgumentsNotSafeInteger { at: String, found: String },
}

impl ToolCatalogError {
    /// Stable machine-readable code (audit/evidence vocabulary).
    pub fn code(&self) -> &'static str {
        match self {
            ToolCatalogError::UnknownDialect { .. } => "unknown_schema_dialect",
            ToolCatalogError::UnsupportedSchemaFeature { .. } => "schema_feature_unsupported",
            ToolCatalogError::SchemaReferenceUnsupported { .. } => "schema_reference_unsupported",
            ToolCatalogError::SchemaBudgetExceeded { .. } => "schema_budget_exceeded",
            ToolCatalogError::SchemaMalformed { .. } => "schema_malformed",
            ToolCatalogError::ManifestInvalid { .. } => "manifest_invalid",
            ToolCatalogError::DuplicateTarget { .. } => "duplicate_target",
            ToolCatalogError::NameCollision { .. } => "name_collision",
            ToolCatalogError::TargetNotFound { .. } => "target_not_found",
            ToolCatalogError::TargetAmbiguous { .. } => "target_ambiguous",
            ToolCatalogError::TargetNotCallable { .. } => "target_not_callable",
            ToolCatalogError::StaleCatalog { .. } => "stale_catalog_generation",
            ToolCatalogError::ArgumentsNotObject => "arguments_not_object",
            ToolCatalogError::ArgumentsInvalid { .. } => "arguments_invalid",
            ToolCatalogError::ArgumentsBudgetExceeded { .. } => "arguments_budget_exceeded",
            ToolCatalogError::ArgumentsNotSafeInteger { .. } => "arguments_not_safe_integer",
        }
    }
}

impl fmt::Display for ToolCatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolCatalogError::UnknownDialect { dialect, supported } => write!(
                f,
                "unknown schema dialect {dialect:?}; this boundary validates only {supported:?} \
                 and never falls back to lenient JSON Schema parsing"
            ),
            ToolCatalogError::UnsupportedSchemaFeature { keyword, at } => write!(
                f,
                "schema keyword {keyword:?} at {at} is not supported by this boundary; a schema \
                 using it is explicitly incompatible instead of being validated loosely"
            ),
            ToolCatalogError::SchemaReferenceUnsupported { keyword, at } => write!(
                f,
                "schema keyword {keyword:?} at {at} requires reference resolution, which this \
                 boundary does not perform (remote resolution is never automatic)"
            ),
            ToolCatalogError::SchemaBudgetExceeded { bound, found, max } => write!(
                f,
                "schema budget {bound} exceeded: found {found}, max {max}"
            ),
            ToolCatalogError::SchemaMalformed { at, detail } => {
                write!(f, "malformed schema at {at}: {detail}")
            }
            ToolCatalogError::ManifestInvalid { detail } => {
                write!(f, "invalid tool manifest: {detail}")
            }
            ToolCatalogError::DuplicateTarget { target_id } => write!(
                f,
                "tool target {target_id} is already registered; registration never overwrites"
            ),
            ToolCatalogError::NameCollision {
                name,
                existing_target_id,
                incoming_target_id,
            } => {
                write!(
                    f,
                    "name/alias {name:?} of {incoming_target_id} collides with the registered \
                     target {existing_target_id}; the collision is reported, never overwritten"
                )
            }
            ToolCatalogError::TargetNotFound { reference } => {
                write!(f, "no registered tool target matches {reference}")
            }
            ToolCatalogError::TargetAmbiguous { name, target_ids } => write!(
                f,
                "tool name {name:?} matches several targets {target_ids:?}; qualify the source"
            ),
            ToolCatalogError::TargetNotCallable {
                target_id,
                availability,
                reason,
            } => write!(
                f,
                "tool target {target_id} is {availability} and not callable: {reason}"
            ),
            ToolCatalogError::StaleCatalog {
                held_generation,
                current_generation,
                target_id,
            } => {
                write!(
                    f,
                    "stale catalog: request pinned at generation {held_generation} but the \
                     registry is at {current_generation} (target {target_id}); re-describe and \
                     re-prepare against a fresh snapshot — the new object is not executed under \
                     the old description"
                )
            }
            ToolCatalogError::ArgumentsNotObject => {
                write!(f, "tool arguments must be a JSON object")
            }
            ToolCatalogError::ArgumentsInvalid { violations } => write!(
                f,
                "tool arguments violate the input schema: {}",
                violations.join("; ")
            ),
            ToolCatalogError::ArgumentsBudgetExceeded { bound, found, max } => write!(
                f,
                "arguments budget {bound} exceeded: found {found}, max {max}"
            ),
            ToolCatalogError::ArgumentsNotSafeInteger { at, found } => write!(
                f,
                "argument number at {at} is {found}: only safe integers (±2^53) are \
                 representable in the cross-language canonical profile; floats and larger \
                 integers are rejected here, exactly like the TypeScript consumer"
            ),
        }
    }
}

impl std::error::Error for ToolCatalogError {}

// ── Identity ────────────────────────────────────────────────────────────────

/// The stable identity of one tool target: namespaced by ORIGIN (which
/// source owns it), not by its display name. Two sources may each expose a
/// tool called `read`; they are two targets with two ids, and neither
/// overwrites the other.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToolTargetId(String);

impl ToolTargetId {
    fn from_raw(raw: String) -> Self {
        Self(raw)
    }

    /// Parses an existing target id string for LOOKUP purposes (journal
    /// rows, prepared handles). This does NOT validate derivation — the
    /// canonical way to obtain an id is [`tool_target_id`]; an id parsed
    /// here that no registry knows simply fails resolution.
    pub fn parse(raw: &str) -> Self {
        Self(raw.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolTargetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a tool target comes from. The origin is the namespace of its
/// identity; the SAME local name under different origins is a DIFFERENT
/// target (never an overwrite).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ToolOrigin {
    FirstParty,
    Plugin { plugin_id: String },
    Mcp { server_id: String },
}

impl ToolOrigin {
    /// The source id the incumbent registry indexes by
    /// (`identity.sourceId` in `lib/tools/invocation/identity.ts`).
    pub fn source_id(&self) -> &str {
        match self {
            ToolOrigin::FirstParty => "first-party",
            ToolOrigin::Plugin { plugin_id } => plugin_id,
            ToolOrigin::Mcp { server_id } => server_id,
        }
    }

    /// The namespace fragment of the target id.
    pub fn kind(&self) -> &'static str {
        match self {
            ToolOrigin::FirstParty => "first-party",
            ToolOrigin::Plugin { .. } => "plugin",
            ToolOrigin::Mcp { .. } => "mcp",
        }
    }
}

/// `encodeURIComponent`-compatible percent-encoding of one identity
/// fragment, byte-compatible with the incumbent TS identity builder so a
/// tool's id is identical on both sides.
fn encode_fragment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        let c = *byte as char;
        if c.is_ascii_alphanumeric()
            || matches!(c, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
        {
            out.push(c);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Derives the stable target id from origin + local name
/// (`tool:first-party:{name}` / `tool:plugin:{plugin}:{name}` /
/// `tool:mcp:{server}:{name}`).
pub fn tool_target_id(
    origin: &ToolOrigin,
    local_name: &str,
) -> Result<ToolTargetId, ToolCatalogError> {
    let name = validate_tool_name(local_name)?;
    let encoded = match origin {
        ToolOrigin::FirstParty => format!("tool:first-party:{}", encode_fragment(&name)),
        ToolOrigin::Plugin { plugin_id } => {
            let plugin = validate_source_id(plugin_id, "pluginId")?;
            format!(
                "tool:plugin:{}:{}",
                encode_fragment(&plugin),
                encode_fragment(&name)
            )
        }
        ToolOrigin::Mcp { server_id } => {
            let server = validate_source_id(server_id, "serverId")?;
            format!(
                "tool:mcp:{}:{}",
                encode_fragment(&server),
                encode_fragment(&name)
            )
        }
    };
    Ok(ToolTargetId::from_raw(encoded))
}

/// The local (model-facing) tool name: non-empty, trimmed, no control
/// characters, bounded length (the model's parameter-name compatibility
/// surface stays exactly the incumbent's plain names).
fn validate_tool_name(name: &str) -> Result<String, ToolCatalogError> {
    if name.trim() != name || name.is_empty() || name.len() > 128 {
        return Err(ToolCatalogError::ManifestInvalid {
            detail: format!("tool local name {name:?} must be non-empty, trimmed and <=128 bytes"),
        });
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(ToolCatalogError::ManifestInvalid {
            detail: format!("tool local name {name:?} must not contain control characters"),
        });
    }
    Ok(name.to_string())
}

fn validate_source_id(id: &str, field: &str) -> Result<String, ToolCatalogError> {
    if id.trim() != id || id.is_empty() || id.len() > 128 {
        return Err(ToolCatalogError::ManifestInvalid {
            detail: format!("{field} {id:?} must be non-empty, trimmed and <=128 bytes"),
        });
    }
    Ok(id.to_string())
}

// ── Permission / availability / manifest ────────────────────────────────────

/// The permission CONTRACT of a target, mirroring the incumbent
/// first-party contract kinds (`FIRST_PARTY_DEFERRED_PERMISSION_CONTRACTS`
/// in `shared/tool-categories.ts`). Aliases resolve to the SAME manifest,
/// so alias and primary name always share this contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionContract {
    pub kind: PermissionKind,
    /// Capability base (`{target}.read` / `{target}.execute`), mirroring
    /// the incumbent `capabilityBase`.
    pub capability_base: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionKind {
    /// Allowed in every mode (information tools).
    Read,
    /// Allowed in operate, blocked in read-only, prompted otherwise.
    Execute,
    /// Argument-aware (the `file` tool): read-only sub-actions map to
    /// Read, everything else to Execute.
    ArgumentAwareFile,
    /// Argument-aware (the `session_folders` tool).
    ArgumentAwareSessionFolders,
}

impl PermissionKind {
    pub fn wire_name(self) -> &'static str {
        match self {
            PermissionKind::Read => "read",
            PermissionKind::Execute => "execute",
            PermissionKind::ArgumentAwareFile => "file",
            PermissionKind::ArgumentAwareSessionFolders => "session-folders",
        }
    }
}

/// What the tool SOURCE declares about itself. This is UNVERIFIED input:
/// a source declaring `read_only` does NOT grant the read-only recovery
/// classification (only the registrar-supplied verified
/// [`ToolManifest::recovery`] does). Stored for audit/description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredPermission {
    None,
    ReadOnly,
    Execute,
    Other,
}

impl DeclaredPermission {
    pub fn wire_name(self) -> &'static str {
        match self {
            DeclaredPermission::None => "none",
            DeclaredPermission::ReadOnly => "read_only",
            DeclaredPermission::Execute => "execute",
            DeclaredPermission::Other => "other",
        }
    }
}

/// Lifecycle availability of one target. The catalog must express future
/// capabilities honestly: an R04-SUP-04 tool whose business body is not
/// migrated yet registers as [`Availability::Future`], never as available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    /// Callable and in the resident surface.
    Available,
    /// Callable through the on-demand catalog (discovery/describe/call),
    /// not resident in the system prompt.
    Deferred,
    /// Disabled by user or policy: discoverable (with reason) but NOT
    /// callable.
    Disabled { reason: String },
    /// Registered shape of a capability whose execution body belongs to a
    /// later stage: discoverable, NOT callable, never presented as
    /// `available` to the model.
    Future { reason: String },
}

impl Availability {
    /// Whether the target may be EXECUTED now.
    pub fn callable(&self) -> bool {
        matches!(self, Availability::Available | Availability::Deferred)
    }

    /// Whether the target appears in discovery listings at all.
    pub fn discoverable(&self) -> bool {
        true
    }

    pub fn wire_name(&self) -> &'static str {
        match self {
            Availability::Available => "available",
            Availability::Deferred => "deferred",
            Availability::Disabled { .. } => "disabled",
            Availability::Future { .. } => "future",
        }
    }

    fn refusal_reason(&self) -> String {
        match self {
            Availability::Disabled { reason } => format!("disabled: {reason}"),
            Availability::Future { reason } => format!("not implemented yet: {reason}"),
            Availability::Available | Availability::Deferred => String::new(),
        }
    }
}

/// One tool's manifest: the SINGLE description source for discovery,
/// permission and execution. The registry stores it verbatim; description
/// snapshots are immutable copies of it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolManifest {
    pub origin: ToolOrigin,
    /// Model-facing parameter name (kept compatible with the incumbent).
    pub local_name: String,
    /// User-visible display name (may equal `local_name`; NEVER used as
    /// the identity).
    pub display_name: String,
    /// Alternative names resolving to THIS target (same authority, same
    /// permission contract).
    pub aliases: Vec<String>,
    /// The tool's own version string (free-form, shown in descriptions).
    pub version: String,
    /// Bounded human-facing description.
    pub description: String,
    /// Input schema document (dialect + verbatim schema). Validated
    /// against the [`SchemaBudget`] at registration.
    pub input_schema: ToolSchemaDocument,
    /// Optional output schema document (same dialect/budget rules).
    pub output_schema: Option<ToolSchemaDocument>,
    pub permission: PermissionContract,
    pub availability: Availability,
    /// Execution timeout in milliseconds (None = the run's own bound).
    pub timeout_ms: Option<u64>,
    /// Max concurrent invocations of this target (None = unbounded).
    pub max_concurrency: Option<u32>,
    /// What the SOURCE claims about its own permission surface. NOT a
    /// grant; never consulted for recovery classification.
    pub declared_permission: DeclaredPermission,
    /// The VERIFIED recovery classification (registrar-supplied). Defaults
    /// to CONSERVATIVE — an unverified tool never gets automatic recovery
    /// of unknown outcomes.
    pub recovery: ToolRecoveryCapability,
}

impl ToolManifest {
    /// Validates the manifest and derives its stable target id. A manifest
    /// that passes this is registrable.
    pub fn validate_and_derive_id(
        &self,
        budget: &SchemaBudget,
    ) -> Result<ToolTargetId, ToolCatalogError> {
        let local_name = validate_tool_name(&self.local_name)?;
        if self.display_name.trim().is_empty() || self.display_name.len() > 256 {
            return Err(ToolCatalogError::ManifestInvalid {
                detail: "display name must be non-empty and <=256 bytes".to_string(),
            });
        }
        if self.description.len() > 4096 {
            return Err(ToolCatalogError::ManifestInvalid {
                detail: "description must be <=4096 bytes".to_string(),
            });
        }
        if self.version.trim().is_empty() || self.version.len() > 64 {
            return Err(ToolCatalogError::ManifestInvalid {
                detail: "version must be non-empty and <=64 bytes".to_string(),
            });
        }
        if self.permission.capability_base.trim().is_empty()
            || self.permission.capability_base.len() > 256
        {
            return Err(ToolCatalogError::ManifestInvalid {
                detail: "permission capability base must be non-empty and <=256 bytes".to_string(),
            });
        }
        let mut unique_aliases = std::collections::BTreeSet::new();
        for alias in &self.aliases {
            let alias = validate_tool_name(alias)?;
            if alias == local_name {
                return Err(ToolCatalogError::ManifestInvalid {
                    detail: format!("alias {alias:?} equals the primary local name"),
                });
            }
            if !unique_aliases.insert(alias.clone()) {
                return Err(ToolCatalogError::ManifestInvalid {
                    detail: format!("duplicate alias {alias:?}"),
                });
            }
        }
        check_schema_dialect(&self.input_schema.dialect).map_err(|_| {
            ToolCatalogError::UnknownDialect {
                dialect: self.input_schema.dialect.clone(),
                supported: lingxi_protocol::SUPPORTED_SCHEMA_DIALECTS.to_vec(),
            }
        })?;
        validate_schema_document(&self.input_schema.schema, "input", budget)?;
        if let Some(output) = &self.output_schema {
            check_schema_dialect(&output.dialect).map_err(|_| {
                ToolCatalogError::UnknownDialect {
                    dialect: output.dialect.clone(),
                    supported: lingxi_protocol::SUPPORTED_SCHEMA_DIALECTS.to_vec(),
                }
            })?;
            validate_schema_document(&output.schema, "output", budget)?;
        }
        tool_target_id(&self.origin, &local_name)
    }
}

// ── Schema budget and subset validation ─────────────────────────────────────

/// Hard bounds for schema documents and argument payloads. Default bounds
/// are deliberately generous for real tool schemas and tight enough that a
/// hostile schema cannot burn unbounded CPU or memory at validation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaBudget {
    /// Max canonical size of one schema document.
    pub max_schema_bytes: usize,
    /// Max nesting depth of a schema document.
    pub max_schema_depth: usize,
    /// Max properties in one schema object (and max enum entries).
    pub max_schema_properties: usize,
    /// Max canonical size of one normalized argument object.
    pub max_arguments_bytes: usize,
    /// Max nesting depth of arguments.
    pub max_arguments_depth: usize,
    /// Max number of schema/value nodes one validation may visit
    /// (validation cost bound).
    pub max_validation_nodes: usize,
    /// Max properties in one argument object.
    pub max_arguments_properties: usize,
}

impl Default for SchemaBudget {
    fn default() -> Self {
        SchemaBudget {
            max_schema_bytes: 64 * 1024,
            max_schema_depth: 16,
            max_schema_properties: 128,
            max_arguments_bytes: 256 * 1024,
            max_arguments_depth: 16,
            max_validation_nodes: 100_000,
            max_arguments_properties: 128,
        }
    }
}

impl SchemaBudget {
    /// Rejects degenerate configurations loudly (a zero bound would make
    /// every schema incompatible by accident).
    pub fn validate(&self) -> Result<(), ToolCatalogError> {
        let positive = [
            (self.max_schema_bytes, "max_schema_bytes"),
            (self.max_schema_depth, "max_schema_depth"),
            (self.max_schema_properties, "max_schema_properties"),
            (self.max_arguments_bytes, "max_arguments_bytes"),
            (self.max_arguments_depth, "max_arguments_depth"),
            (self.max_validation_nodes, "max_validation_nodes"),
            (self.max_arguments_properties, "max_arguments_properties"),
        ];
        for (value, name) in positive {
            if value == 0 {
                return Err(ToolCatalogError::ManifestInvalid {
                    detail: format!("SchemaBudget.{name} must be > 0"),
                });
            }
        }
        Ok(())
    }
}

/// Validation keywords this boundary refuses to interpret (registering a
/// schema that uses one of them fails with
/// [`ToolCatalogError::UnsupportedSchemaFeature`] — it must not be
/// validated loosely).
const REJECTED_VALIDATION_KEYWORDS: &[&str] = &[
    "pattern",
    "patternProperties",
    "propertyNames",
    "format",
    "if",
    "then",
    "else",
    "not",
    "allOf",
    "anyOf",
    "oneOf",
    "contains",
    "dependencies",
    "dependentRequired",
    "dependentSchemas",
    "unevaluatedProperties",
    "unevaluatedItems",
    "multipleOf",
    "uniqueItems",
    "minContains",
    "maxContains",
];

/// Reference vocabulary: never resolved here (neither local nor remote).
const REFERENCE_KEYWORDS: &[&str] = &[
    "$ref",
    "$id",
    "$defs",
    "$anchor",
    "$dynamicRef",
    "$dynamicAnchor",
    "$recursiveRef",
    "$recursiveAnchor",
    "$schema",
    "$comment",
    "$vocabulary",
];

/// Keywords this boundary actively interprets.
const SUPPORTED_KEYWORDS: &[&str] = &[
    "type",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "enum",
    "const",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "minLength",
    "maxLength",
    "minItems",
    "maxItems",
    "minProperties",
    "maxProperties",
    "default",
    "title",
    "description",
];

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Validates one schema document against the budget and the supported
/// keyword subset. Returns Err on any structure this boundary cannot
/// validate EXACTLY.
pub fn validate_schema_document(
    schema: &serde_json::Value,
    role: &str,
    budget: &SchemaBudget,
) -> Result<(), ToolCatalogError> {
    let canonical = lingxi_protocol::canon::canonical_json_bytes(schema);
    if canonical.len() > budget.max_schema_bytes {
        return Err(ToolCatalogError::SchemaBudgetExceeded {
            bound: "max_schema_bytes",
            found: canonical.len(),
            max: budget.max_schema_bytes,
        });
    }
    let mut nodes = budget.max_validation_nodes;
    validate_schema_node(schema, &format!("{role} schema"), budget, 1, &mut nodes)
}

fn validate_schema_node(
    node: &serde_json::Value,
    at: &str,
    budget: &SchemaBudget,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), ToolCatalogError> {
    if depth > budget.max_schema_depth {
        return Err(ToolCatalogError::SchemaBudgetExceeded {
            bound: "max_schema_depth",
            found: depth,
            max: budget.max_schema_depth,
        });
    }
    *nodes = nodes
        .checked_sub(1)
        .ok_or(ToolCatalogError::SchemaBudgetExceeded {
            bound: "max_validation_nodes",
            found: budget.max_validation_nodes,
            max: budget.max_validation_nodes,
        })?;
    let Some(object) = node.as_object() else {
        return Err(ToolCatalogError::SchemaMalformed {
            at: at.to_string(),
            detail: format!("a schema node must be an object, found {node:?}"),
        });
    };
    if object.len() > budget.max_schema_properties {
        return Err(ToolCatalogError::SchemaBudgetExceeded {
            bound: "max_schema_properties",
            found: object.len(),
            max: budget.max_schema_properties,
        });
    }
    for (keyword, value) in object {
        if REFERENCE_KEYWORDS.contains(&keyword.as_str()) {
            return Err(ToolCatalogError::SchemaReferenceUnsupported {
                keyword: keyword.clone(),
                at: at.to_string(),
            });
        }
        if REJECTED_VALIDATION_KEYWORDS.contains(&keyword.as_str()) {
            return Err(ToolCatalogError::UnsupportedSchemaFeature {
                keyword: keyword.clone(),
                at: at.to_string(),
            });
        }
        if !SUPPORTED_KEYWORDS.contains(&keyword.as_str()) {
            // Unknown keys are annotations (vendor extensions etc.):
            // preserved verbatim, never interpreted. Ignoring an
            // ANNOTATION is safe; the two sets above exist exactly so no
            // VALIDATION keyword ever reaches this arm silently.
            continue;
        }
        match keyword.as_str() {
            "type" => match value {
                serde_json::Value::String(t) => check_schema_type_name(t, at)?,
                serde_json::Value::Array(entries) => {
                    if entries.len() > 8 {
                        return Err(ToolCatalogError::SchemaBudgetExceeded {
                            bound: "type alternatives",
                            found: entries.len(),
                            max: 8,
                        });
                    }
                    for entry in entries {
                        let Some(t) = entry.as_str() else {
                            return Err(ToolCatalogError::SchemaMalformed {
                                at: format!("{at}.type"),
                                detail: "type array entries must be strings".to_string(),
                            });
                        };
                        check_schema_type_name(t, at)?;
                    }
                }
                _ => {
                    return Err(ToolCatalogError::SchemaMalformed {
                        at: format!("{at}.type"),
                        detail: "type must be a string or an array of strings".to_string(),
                    })
                }
            },
            "properties" => {
                let Some(properties) = value.as_object() else {
                    return Err(ToolCatalogError::SchemaMalformed {
                        at: format!("{at}.properties"),
                        detail: "properties must be an object".to_string(),
                    });
                };
                if properties.len() > budget.max_schema_properties {
                    return Err(ToolCatalogError::SchemaBudgetExceeded {
                        bound: "max_schema_properties",
                        found: properties.len(),
                        max: budget.max_schema_properties,
                    });
                }
                for (name, sub) in properties {
                    validate_schema_node(
                        sub,
                        &format!("{at}.properties.{name}"),
                        budget,
                        depth + 1,
                        nodes,
                    )?;
                }
            }
            "required" => {
                let Some(entries) = value.as_array() else {
                    return Err(ToolCatalogError::SchemaMalformed {
                        at: format!("{at}.required"),
                        detail: "required must be an array of strings".to_string(),
                    });
                };
                if entries.len() > budget.max_schema_properties {
                    return Err(ToolCatalogError::SchemaBudgetExceeded {
                        bound: "max_schema_properties",
                        found: entries.len(),
                        max: budget.max_schema_properties,
                    });
                }
                for entry in entries {
                    if entry.as_str().is_none() {
                        return Err(ToolCatalogError::SchemaMalformed {
                            at: format!("{at}.required"),
                            detail: "required entries must be strings".to_string(),
                        });
                    }
                }
            }
            "items" => {
                validate_schema_node(value, &format!("{at}.items"), budget, depth + 1, nodes)?;
            }
            "additionalProperties" => {
                if !value.is_boolean() {
                    // A schema-valued additionalProperties is a full schema:
                    // support the boolean form only; the schema form is an
                    // explicit unsupported feature rather than a guess.
                    return Err(ToolCatalogError::UnsupportedSchemaFeature {
                        keyword: "additionalProperties (schema form)".to_string(),
                        at: at.to_string(),
                    });
                }
            }
            "enum" => {
                let Some(entries) = value.as_array() else {
                    return Err(ToolCatalogError::SchemaMalformed {
                        at: format!("{at}.enum"),
                        detail: "enum must be an array".to_string(),
                    });
                };
                if entries.is_empty() {
                    return Err(ToolCatalogError::SchemaMalformed {
                        at: format!("{at}.enum"),
                        detail: "enum must not be empty (an empty enum matches nothing)"
                            .to_string(),
                    });
                }
                if entries.len() > budget.max_schema_properties {
                    return Err(ToolCatalogError::SchemaBudgetExceeded {
                        bound: "max_schema_properties",
                        found: entries.len(),
                        max: budget.max_schema_properties,
                    });
                }
            }
            "const" | "default" | "title" | "description" => {}
            "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum" => {
                let valid = value
                    .as_i64()
                    .map(|_| true)
                    .or_else(|| value.as_f64().map(|_| false));
                match valid {
                    Some(true) => {}
                    Some(false) => {
                        return Err(ToolCatalogError::UnsupportedSchemaFeature {
                            keyword: format!("{keyword} (non-integer bound)"),
                            at: at.to_string(),
                        })
                    }
                    None => {
                        return Err(ToolCatalogError::SchemaMalformed {
                            at: format!("{at}.{keyword}"),
                            detail: "numeric bound must be a number".to_string(),
                        })
                    }
                }
            }
            "minLength" | "maxLength" | "minItems" | "maxItems" | "minProperties"
            | "maxProperties" => {
                if value.as_u64().is_none() {
                    return Err(ToolCatalogError::SchemaMalformed {
                        at: format!("{at}.{keyword}"),
                        detail: "bound must be a non-negative integer".to_string(),
                    });
                }
            }
            _ => unreachable!("keyword membership checked above"),
        }
    }
    Ok(())
}

fn check_schema_type_name(t: &str, at: &str) -> Result<(), ToolCatalogError> {
    if matches!(
        t,
        "object" | "array" | "string" | "integer" | "number" | "boolean" | "null"
    ) {
        Ok(())
    } else {
        Err(ToolCatalogError::SchemaMalformed {
            at: at.to_string(),
            detail: format!("unknown JSON Schema type {t:?}"),
        })
    }
}

// ── Effective arguments ─────────────────────────────────────────────────────

/// The complete, immutable, schema-validated and normalized arguments of
/// one tool call (R04-T01: the R03 digest-only request shape is replaced
/// by the REAL payload). Executors consume THIS object; digests and
/// summaries are derived from it by this boundary.
///
/// Value invariants (checked on construction):
/// - a JSON object at the top level;
/// - every number is a SAFE INTEGER (integers within ±2^53): floats and
///   larger integers are rejected with
///   [`ToolCatalogError::ArgumentsNotSafeInteger`] — the TypeScript
///   canonical consumer hard-fails on them, so accepting them here would
///   produce digests only one language can compute (RR-T02-F2 ruling);
/// - nesting depth and canonical size bounded by the [`SchemaBudget`].
#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveArguments {
    canonical: serde_json::Value,
    canonical_bytes: Vec<u8>,
}

/// All integers in the canonical profile must fit the JS safe-integer
/// range — the exact boundary the TS consumer enforces.
pub const SAFE_INTEGER_MAX: i64 = 9_007_199_254_740_991; // 2^53 - 1

impl EffectiveArguments {
    /// Builds effective arguments from a normalized JSON object, enforcing
    /// the integer-safety and budget invariants. Errors are loud.
    pub fn from_value(
        value: serde_json::Value,
        budget: &SchemaBudget,
    ) -> Result<Self, ToolCatalogError> {
        let Some(map) = value.as_object() else {
            return Err(ToolCatalogError::ArgumentsNotObject);
        };
        if map.len() > budget.max_arguments_properties {
            return Err(ToolCatalogError::ArgumentsBudgetExceeded {
                bound: "max_arguments_properties",
                found: map.len(),
                max: budget.max_arguments_properties,
            });
        }
        let mut nodes = budget.max_validation_nodes;
        check_argument_value(&value, "$", budget, 1, &mut nodes)?;
        let canonical_bytes = lingxi_protocol::canon::canonical_json_bytes(&value);
        if canonical_bytes.len() > budget.max_arguments_bytes {
            return Err(ToolCatalogError::ArgumentsBudgetExceeded {
                bound: "max_arguments_bytes",
                found: canonical_bytes.len(),
                max: budget.max_arguments_bytes,
            });
        }
        Ok(Self {
            canonical: value,
            canonical_bytes,
        })
    }

    /// The immutable effective argument object.
    pub fn as_value(&self) -> &serde_json::Value {
        &self.canonical
    }

    /// Canonical JSON bytes of the effective arguments (the exact bytes
    /// the digest covers).
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    /// The digest approvals and receipts bind to, computed by THIS
    /// trusted boundary over the canonical bytes.
    pub fn digest(&self) -> ArgsDigest {
        ArgsDigest {
            algorithm: "sha256".to_string(),
            canonicalization: CANONICALIZATION_ID.to_string(),
            hex: lingxi_protocol::canon::sha256_hex(&self.canonical_bytes),
        }
    }

    /// Whether `other` is exactly the digest of these effective arguments
    /// (the run driver's anti-forgery check on every request).
    pub fn digest_matches(&self, other: &ArgsDigest) -> bool {
        let computed = self.digest();
        computed.algorithm == other.algorithm
            && computed.canonicalization == other.canonicalization
            && computed.hex == other.hex
    }
}

fn check_argument_value(
    value: &serde_json::Value,
    at: &str,
    budget: &SchemaBudget,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), ToolCatalogError> {
    if depth > budget.max_arguments_depth {
        return Err(ToolCatalogError::ArgumentsBudgetExceeded {
            bound: "max_arguments_depth",
            found: depth,
            max: budget.max_arguments_depth,
        });
    }
    *nodes = nodes
        .checked_sub(1)
        .ok_or(ToolCatalogError::ArgumentsBudgetExceeded {
            bound: "max_validation_nodes",
            found: budget.max_validation_nodes,
            max: budget.max_validation_nodes,
        })?;
    match value {
        number if number.is_number() => {
            // Safe-integer boundary: floats and out-of-range integers are
            // rejected (the TS canonical consumer throws on them; parity
            // is mandatory, not optional).
            if let Some(i) = number.as_i64() {
                if !(-SAFE_INTEGER_MAX..=SAFE_INTEGER_MAX).contains(&i) {
                    return Err(ToolCatalogError::ArgumentsNotSafeInteger {
                        at: at.to_string(),
                        found: i.to_string(),
                    });
                }
                Ok(())
            } else if let Some(u) = number.as_u64() {
                Err(ToolCatalogError::ArgumentsNotSafeInteger {
                    at: at.to_string(),
                    found: u.to_string(),
                })
            } else {
                Err(ToolCatalogError::ArgumentsNotSafeInteger {
                    at: at.to_string(),
                    found: number.to_string(),
                })
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                check_argument_value(item, &format!("{at}/{index}"), budget, depth + 1, nodes)?;
            }
            Ok(())
        }
        serde_json::Value::Object(map) => {
            if map.len() > budget.max_arguments_properties {
                return Err(ToolCatalogError::ArgumentsBudgetExceeded {
                    bound: "max_arguments_properties",
                    found: map.len(),
                    max: budget.max_arguments_properties,
                });
            }
            for (key, sub) in map {
                check_argument_value(sub, &format!("{at}/{key}"), budget, depth + 1, nodes)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Produces the bounded SHAPE summary of effective arguments: key names
/// and value TYPES only — never the values themselves. A summary produced
/// by this boundary cannot be reversed into file paths, command lines or
/// bodies (the digest is one-way by construction; the summary carries no
/// payload at all).
pub fn summarize_arguments(args: &EffectiveArguments) -> String {
    const MAX_SUMMARY_KEYS: usize = 24;
    fn type_tag(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Null => "null".to_string(),
            serde_json::Value::Bool(_) => "bool".to_string(),
            serde_json::Value::Number(n) => {
                if n.is_i64() || n.is_u64() {
                    "int".to_string()
                } else {
                    "num".to_string()
                }
            }
            serde_json::Value::String(_) => "str".to_string(),
            serde_json::Value::Array(items) => format!("arr[{}]", items.len()),
            serde_json::Value::Object(map) => format!("obj{{{}}}", map.len()),
        }
    }
    let Some(map) = args.as_value().as_object() else {
        return "(non-object)".to_string();
    };
    let mut parts: Vec<String> = map
        .iter()
        .take(MAX_SUMMARY_KEYS)
        .map(|(k, v)| format!("{k}:{}", type_tag(v)))
        .collect();
    if map.len() > MAX_SUMMARY_KEYS {
        parts.push(format!("…+{}", map.len() - MAX_SUMMARY_KEYS));
    }
    format!("{{{}}}", parts.join(","))
}

// ── Argument normalization (validate → defaults → normalize) ────────────────

/// Validates `raw` against `schema`, applies schema DEFAULTS for missing
/// properties (at every object level the schema describes) and returns the
/// normalized object. Unknown properties are rejected when
/// `additionalProperties` is `false`; when absent they are PRESERVED
/// (default JSON Schema semantics) — normalization never drops data.
pub fn normalize_arguments(
    schema: &serde_json::Value,
    raw: &serde_json::Value,
    budget: &SchemaBudget,
) -> Result<serde_json::Value, ToolCatalogError> {
    let Some(raw_map) = raw.as_object() else {
        return Err(ToolCatalogError::ArgumentsNotObject);
    };
    if raw_map.len() > budget.max_arguments_properties {
        return Err(ToolCatalogError::ArgumentsBudgetExceeded {
            bound: "max_arguments_properties",
            found: raw_map.len(),
            max: budget.max_arguments_properties,
        });
    }
    let mut nodes = budget.max_validation_nodes;
    let mut violations = Vec::new();
    let normalized = normalize_value(schema, raw, "$", budget, 1, &mut nodes, &mut violations)?;
    if violations.is_empty() {
        Ok(normalized)
    } else {
        Err(ToolCatalogError::ArgumentsInvalid { violations })
    }
}

fn normalize_value(
    schema: &serde_json::Value,
    value: &serde_json::Value,
    at: &str,
    budget: &SchemaBudget,
    depth: usize,
    nodes: &mut usize,
    violations: &mut Vec<String>,
) -> Result<serde_json::Value, ToolCatalogError> {
    if depth > budget.max_arguments_depth {
        return Err(ToolCatalogError::ArgumentsBudgetExceeded {
            bound: "max_arguments_depth",
            found: depth,
            max: budget.max_arguments_depth,
        });
    }
    *nodes = nodes
        .checked_sub(1)
        .ok_or(ToolCatalogError::ArgumentsBudgetExceeded {
            bound: "max_validation_nodes",
            found: budget.max_validation_nodes,
            max: budget.max_validation_nodes,
        })?;
    let schema_object = schema.as_object().cloned().unwrap_or_default();

    // type / enum / const / numeric / length checks
    if let Some(type_spec) = schema_object.get("type") {
        let allowed: Vec<&str> = match type_spec {
            serde_json::Value::String(t) => vec![t.as_str()],
            serde_json::Value::Array(entries) => {
                entries.iter().filter_map(|entry| entry.as_str()).collect()
            }
            _ => Vec::new(),
        };
        let actual = json_type_name(value);
        let ok = allowed.iter().any(|t| {
            t == &actual
                || (*t == "integer" && actual == "number" && value.as_i64().is_some())
                || *t == "number"
        });
        if !ok && !allowed.is_empty() {
            violations.push(format!("{at}: expected type {allowed:?}, found {actual}"));
        }
    }
    if let Some(entries) = schema_object.get("enum").and_then(|e| e.as_array()) {
        if !entries.contains(value) {
            violations.push(format!("{at}: value not in enum"));
        }
    }
    if let Some(expected) = schema_object.get("const") {
        if expected != value {
            violations.push(format!("{at}: value differs from const"));
        }
    }
    if let Some(number) = value.as_i64() {
        if let Some(min) = schema_object.get("minimum").and_then(|v| v.as_i64()) {
            if number < min {
                violations.push(format!("{at}: {number} < minimum {min}"));
            }
        }
        if let Some(min) = schema_object
            .get("exclusiveMinimum")
            .and_then(|v| v.as_i64())
        {
            if number <= min {
                violations.push(format!("{at}: {number} <= exclusiveMinimum {min}"));
            }
        }
        if let Some(max) = schema_object.get("maximum").and_then(|v| v.as_i64()) {
            if number > max {
                violations.push(format!("{at}: {number} > maximum {max}"));
            }
        }
        if let Some(max) = schema_object
            .get("exclusiveMaximum")
            .and_then(|v| v.as_i64())
        {
            if number >= max {
                violations.push(format!("{at}: {number} >= exclusiveMaximum {max}"));
            }
        }
    }
    if let Some(text) = value.as_str() {
        if let Some(min) = schema_object.get("minLength").and_then(|v| v.as_u64()) {
            if (text.chars().count() as u64) < min {
                violations.push(format!("{at}: string shorter than minLength {min}"));
            }
        }
        if let Some(max) = schema_object.get("maxLength").and_then(|v| v.as_u64()) {
            if (text.chars().count() as u64) > max {
                violations.push(format!("{at}: string longer than maxLength {max}"));
            }
        }
    }

    let Some(map) = value.as_object() else {
        // Non-object at this node. Array-length bounds FIRST:
        // minItems/maxItems constrain the ARRAY VALUE and must apply
        // whenever the value is an array, whether or not the schema also
        // carries an `items` sub-schema (`{"type":"array","maxItems":N}`
        // is a legal, common shape). Gating them on `items` silently
        // skipped the bound for items-less schemas (R04-T01-R1-F01) —
        // lenient parsing, which this boundary forbids. Mirrors the object
        // side, where min/maxProperties run without a `properties` gate.
        if let Some(items) = value.as_array() {
            if let Some(max) = schema_object.get("maxItems").and_then(|v| v.as_u64()) {
                if items.len() as u64 > max {
                    violations.push(format!("{at}: more than maxItems {max}"));
                }
            }
            if let Some(min) = schema_object.get("minItems").and_then(|v| v.as_u64()) {
                if (items.len() as u64) < min {
                    violations.push(format!("{at}: fewer than minItems {min}"));
                }
            }
            // Per-element validation only exists when an `items`
            // sub-schema does; absent `items` means no per-element schema
            // to validate against (the array still passed the length
            // bounds above, and its elements remain subject to the
            // safe-integer/budget walk in EffectiveArguments).
            if let Some(items_schema) = schema_object.get("items") {
                let mut normalized_items = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    normalized_items.push(normalize_value(
                        items_schema,
                        item,
                        &format!("{at}/{index}"),
                        budget,
                        depth + 1,
                        nodes,
                        violations,
                    )?);
                }
                return Ok(serde_json::Value::Array(normalized_items));
            }
        }
        return Ok(value.clone());
    };

    if let Some(min) = schema_object.get("minProperties").and_then(|v| v.as_u64()) {
        if (map.len() as u64) < min {
            violations.push(format!("{at}: fewer than minProperties {min}"));
        }
    }
    if let Some(max) = schema_object.get("maxProperties").and_then(|v| v.as_u64()) {
        if (map.len() as u64) > max {
            violations.push(format!("{at}: more than maxProperties {max}"));
        }
    }
    let properties = schema_object
        .get("properties")
        .and_then(|p| p.as_object())
        .cloned()
        .unwrap_or_default();
    for name in schema_object
        .get("required")
        .and_then(|r| r.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|e| e.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
    {
        if !map.contains_key(&name) {
            violations.push(format!("{at}: missing required property {name:?}"));
        }
    }
    let reject_unknown = matches!(
        schema_object.get("additionalProperties"),
        Some(serde_json::Value::Bool(false))
    );
    if reject_unknown {
        for key in map.keys() {
            if !properties.contains_key(key) {
                violations.push(format!(
                    "{at}: unknown property {key:?} (additionalProperties:false)"
                ));
            }
        }
    }
    let mut normalized_map = serde_json::Map::new();
    for (key, sub) in map {
        let sub_schema = properties
            .get(key)
            .cloned()
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        normalized_map.insert(
            key.clone(),
            normalize_value(
                &sub_schema,
                sub,
                &format!("{at}/{key}"),
                budget,
                depth + 1,
                nodes,
                violations,
            )?,
        );
    }
    // Defaults for MISSING properties: filled from the schema, then
    // validated like any other value (a default that violates its own
    // schema surfaces as a violation here, never silently accepted).
    for (name, sub_schema) in &properties {
        if !normalized_map.contains_key(name) {
            if let Some(default_value) = sub_schema.get("default") {
                let normalized_default = normalize_value(
                    sub_schema,
                    default_value,
                    &format!("{at}/{name}"),
                    budget,
                    depth + 1,
                    nodes,
                    violations,
                )?;
                normalized_map.insert(name.clone(), normalized_default);
            }
        }
    }
    Ok(serde_json::Value::Object(normalized_map))
}

// ── Registry ────────────────────────────────────────────────────────────────

/// Whether a registered name is the target's PRIMARY name or an ALIAS.
/// Collision rules depend on the role: two PRIMARY names may be equal
/// across different sources (indexed; name-only resolution is explicitly
/// ambiguous); an ALIAS must be unique across the whole catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameRole {
    Primary,
    Alias,
}

/// One registered target: its manifest plus the per-target lifecycle
/// generation (advanced by update/disable/enable; removed by uninstall).
#[derive(Debug, Clone, PartialEq)]
pub struct RegisteredTool {
    pub manifest: ToolManifest,
    pub target_generation: u64,
}

/// The discovery/description record of one target (what a catalog listing
/// and a cached description snapshot carry).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDescription {
    pub target_id: ToolTargetId,
    pub origin: ToolOrigin,
    pub source_id: String,
    pub local_name: String,
    pub display_name: String,
    pub aliases: Vec<String>,
    pub version: String,
    pub target_generation: u64,
    pub permission: PermissionContract,
    pub availability: Availability,
    pub timeout_ms: Option<u64>,
    pub declared_permission: DeclaredPermission,
    pub recovery: ToolRecoveryCapability,
    /// SHA-256 of the canonical input schema — the "schema digest" a
    /// listing carries (the full schema comes from `describe`).
    pub input_schema_digest: String,
    pub input_dialect: String,
}

/// The full description of one target (listing record + the verbatim
/// input schema document).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolFullDescription {
    pub listing: ToolDescription,
    pub input_schema: ToolSchemaDocument,
}

/// An immutable catalog snapshot: the generation it was taken at plus the
/// description records. A request pinned to an older generation is
/// refused with [`ToolCatalogError::StaleCatalog`] — the old description
/// must never silently point at a new object's meaning (R04-A02).
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogSnapshot {
    pub catalog_generation: u64,
    pub tools: Vec<ToolDescription>,
}

/// A caller's pinned catalog view (the generation its cached
/// descriptions/snapshots were taken at).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogPin {
    pub catalog_generation: u64,
}

/// A reference to a target: name-only (resolved through aliases; a name
/// contested by several sources is an explicit ambiguity) or
/// source-qualified (source id + name, the authoritative form).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolTargetRef {
    ByName { name: String },
    BySourceAndName { source_id: String, name: String },
    ByTargetId { target_id: ToolTargetId },
}

impl ToolTargetRef {
    fn describe(&self) -> String {
        match self {
            ToolTargetRef::ByName { name } => format!("name {name:?}"),
            ToolTargetRef::BySourceAndName { source_id, name } => {
                format!("source {source_id:?} name {name:?}")
            }
            ToolTargetRef::ByTargetId { target_id } => format!("target {target_id}"),
        }
    }
}

/// The output of [`ToolRegistry::prepare_invocation`]: everything the
/// gateway (R04-T02), the approval surface (R04-T03) and the executor
/// bind to — identity, generations, the immutable effective arguments,
/// the trusted-boundary digest and shape summary, the permission contract
/// and the verified recovery capability.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedToolCall {
    pub target_id: ToolTargetId,
    /// The target's ORIGIN (the namespace of its identity). R04-T02-R1-F01
    /// same-root-cause closure: the delegation-family match in the run
    /// driver routes by LOCAL NAME, so it MUST be able to see the origin
    /// and refuse to route a plugin/MCP-origin name collision into the
    /// first-party child-run launcher.
    pub origin: ToolOrigin,
    pub local_name: String,
    pub display_name: String,
    pub version: String,
    pub catalog_generation: u64,
    pub target_generation: u64,
    pub effective: EffectiveArguments,
    pub args_digest: ArgsDigest,
    pub args_summary: String,
    pub permission: PermissionContract,
    pub recovery: ToolRecoveryCapability,
    pub timeout_ms: Option<u64>,
}

impl PreparedToolCall {
    /// Converts the prepared call into the run driver's [`crate::ports::ToolRequest`]
    /// shape (the executor's input). The digest travels WITH the
    /// arguments it was computed from; the driver re-verifies the pair.
    pub fn into_tool_request(self) -> crate::ports::ToolRequest {
        crate::ports::ToolRequest {
            target: self.target_id.as_str().to_string(),
            arguments: self.effective,
            args_digest: self.args_digest,
            args_summary: Some(self.args_summary),
            delegation: None,
        }
    }
}

/// The receipt of a successful registry mutation (what generation the
/// catalog and the target moved to).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationReceipt {
    pub target_id: ToolTargetId,
    pub target_generation: u64,
    pub catalog_generation: u64,
}

/// The tool registry: the single authority mapping names/aliases/target
/// ids to manifests at a generation. Interior-mutable and sync (no I/O,
/// no clock, no network — a domain structure the service layer owns and
/// injects).
#[derive(Debug, Default)]
pub struct ToolRegistry {
    state: std::sync::Mutex<RegistryState>,
}

#[derive(Debug, Default)]
struct RegistryState {
    catalog_generation: u64,
    targets: BTreeMap<ToolTargetId, RegisteredTool>,
    /// Every name/alias → (owner, role). A PRIMARY name may have several
    /// owners (one per source); an ALIAS has exactly one owner, enforced
    /// at registration.
    names: BTreeMap<String, Vec<(ToolTargetId, NameRole)>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a manifest. Refuses: duplicate target ids, alias/name
    /// collisions that would mint a second identity for one visible name,
    /// invalid manifests, over-budget or unsupported schemas. Mutations
    /// never overwrite an existing target; the catalog generation
    /// advances on success.
    pub fn register(
        &self,
        manifest: ToolManifest,
        budget: &SchemaBudget,
    ) -> Result<MutationReceipt, ToolCatalogError> {
        budget.validate()?;
        let target_id = manifest.validate_and_derive_id(budget)?;
        let mut state = self.lock();
        if state.targets.contains_key(&target_id) {
            return Err(ToolCatalogError::DuplicateTarget {
                target_id: target_id.as_str().to_string(),
            });
        }
        let incoming: Vec<(String, NameRole)> =
            std::iter::once((manifest.local_name.clone(), NameRole::Primary))
                .chain(
                    manifest
                        .aliases
                        .iter()
                        .map(|a| (a.clone(), NameRole::Alias)),
                )
                .collect();
        check_name_collisions(&state, &incoming, &target_id)?;
        for (name, _role) in &incoming {
            state
                .names
                .entry(name.clone())
                .or_default()
                .push((target_id.clone(), *_role));
        }
        state.catalog_generation += 1;
        let receipt = MutationReceipt {
            target_id: target_id.clone(),
            target_generation: 1,
            catalog_generation: state.catalog_generation,
        };
        state.targets.insert(
            target_id,
            RegisteredTool {
                manifest,
                target_generation: 1,
            },
        );
        Ok(receipt)
    }

    /// Resolves a reference to exactly one registered target. Name-only
    /// references resolve through aliases to the authoritative identity; a
    /// name owned by several sources is an explicit ambiguity listing the
    /// target ids (never a silent pick).
    pub fn resolve(&self, reference: &ToolTargetRef) -> Result<ToolTargetId, ToolCatalogError> {
        let state = self.lock();
        resolve_locked(&state, reference)
    }

    /// The listing record of one target id.
    pub fn describe(&self, target_id: &ToolTargetId) -> Result<ToolDescription, ToolCatalogError> {
        let state = self.lock();
        let tool =
            state
                .targets
                .get(target_id)
                .ok_or_else(|| ToolCatalogError::TargetNotFound {
                    reference: format!("target {target_id}"),
                })?;
        Ok(description_of(target_id, tool))
    }

    /// The full description (listing + verbatim input schema).
    pub fn describe_full(
        &self,
        target_id: &ToolTargetId,
    ) -> Result<ToolFullDescription, ToolCatalogError> {
        let state = self.lock();
        let tool =
            state
                .targets
                .get(target_id)
                .ok_or_else(|| ToolCatalogError::TargetNotFound {
                    reference: format!("target {target_id}"),
                })?;
        Ok(ToolFullDescription {
            listing: description_of(target_id, tool),
            input_schema: tool.manifest.input_schema.clone(),
        })
    }

    /// Discovery: every discoverable target whose local name, display
    /// name or alias contains `query` (case-insensitive; empty query =
    /// all), sorted by target id.
    pub fn search(&self, query: &str) -> Vec<ToolDescription> {
        let state = self.lock();
        let needle = query.to_lowercase();
        let mut out: Vec<ToolDescription> = state
            .targets
            .iter()
            .filter(|(_, tool)| {
                if needle.is_empty() {
                    return true;
                }
                tool.manifest.local_name.to_lowercase().contains(&needle)
                    || tool.manifest.display_name.to_lowercase().contains(&needle)
                    || tool
                        .manifest
                        .aliases
                        .iter()
                        .any(|alias| alias.to_lowercase().contains(&needle))
            })
            .map(|(id, tool)| description_of(id, tool))
            .collect();
        out.sort_by(|a, b| a.target_id.cmp(&b.target_id));
        out
    }

    /// The immutable catalog snapshot at the current generation (what a
    /// "client" caches). Snapshots are frozen copies: a later registry
    /// mutation never rewrites them.
    pub fn snapshot(&self) -> CatalogSnapshot {
        let state = self.lock();
        CatalogSnapshot {
            catalog_generation: state.catalog_generation,
            tools: state
                .targets
                .iter()
                .map(|(id, tool)| description_of(id, tool))
                .collect(),
        }
    }

    /// Current catalog generation.
    pub fn catalog_generation(&self) -> u64 {
        self.lock().catalog_generation
    }

    /// Updates a target's manifest (version, schema, description, …).
    /// The target's lifecycle generation AND the catalog generation
    /// advance: every description snapshot taken before the update is
    /// stale and pinned requests against it are refused
    /// ([`ToolCatalogError::StaleCatalog`]) — an old description can never
    /// silently point at a new meaning.
    pub fn update(
        &self,
        target_id: &ToolTargetId,
        manifest: ToolManifest,
        budget: &SchemaBudget,
    ) -> Result<MutationReceipt, ToolCatalogError> {
        budget.validate()?;
        let derived = manifest.validate_and_derive_id(budget)?;
        if derived != *target_id {
            return Err(ToolCatalogError::ManifestInvalid {
                detail: format!(
                    "update changes the target identity: {target_id} -> {derived}; register the \
                     new target and uninstall the old one instead"
                ),
            });
        }
        let mut state = self.lock();
        let old: Vec<(String, NameRole)> =
            {
                let tool = state.targets.get(target_id).ok_or_else(|| {
                    ToolCatalogError::TargetNotFound {
                        reference: format!("target {target_id}"),
                    }
                })?;
                std::iter::once((tool.manifest.local_name.clone(), NameRole::Primary))
                    .chain(
                        tool.manifest
                            .aliases
                            .iter()
                            .map(|a| (a.clone(), NameRole::Alias)),
                    )
                    .collect()
            };
        // Swap names/aliases when they changed (new names must not
        // collide with other targets either). Collisions are evaluated
        // against the index WITHOUT this target's own outgoing entries.
        let incoming: Vec<(String, NameRole)> =
            std::iter::once((manifest.local_name.clone(), NameRole::Primary))
                .chain(
                    manifest
                        .aliases
                        .iter()
                        .map(|a| (a.clone(), NameRole::Alias)),
                )
                .collect();
        for (name, role) in &incoming {
            if old.iter().any(|(old_name, _)| old_name == name) {
                continue;
            }
            let incoming_alone = [(name.clone(), *role)];
            check_name_collisions_except(&state, &incoming_alone, target_id, name)?;
        }
        for (name, _) in &old {
            if let Some(entries) = state.names.get_mut(name) {
                entries.retain(|(id, _)| id != target_id);
                if entries.is_empty() {
                    state.names.remove(name);
                }
            }
        }
        for (name, role) in &incoming {
            state
                .names
                .entry(name.clone())
                .or_default()
                .push((target_id.clone(), *role));
        }
        let tool =
            state
                .targets
                .get_mut(target_id)
                .ok_or_else(|| ToolCatalogError::TargetNotFound {
                    reference: format!("target {target_id}"),
                })?;
        tool.manifest = manifest;
        tool.target_generation += 1;
        let receipt = MutationReceipt {
            target_id: target_id.clone(),
            target_generation: tool.target_generation,
            catalog_generation: state.catalog_generation + 1,
        };
        state.catalog_generation += 1;
        Ok(receipt)
    }

    /// Sets the availability of one target (install/enable = Available or
    /// Deferred; disable = Disabled). Both generations advance; a target
    /// that is not callable is refused at prepare time with its reason.
    pub fn set_availability(
        &self,
        target_id: &ToolTargetId,
        availability: Availability,
    ) -> Result<MutationReceipt, ToolCatalogError> {
        let mut state = self.lock();
        let target_generation = {
            let tool = state.targets.get_mut(target_id).ok_or_else(|| {
                ToolCatalogError::TargetNotFound {
                    reference: format!("target {target_id}"),
                }
            })?;
            tool.manifest.availability = availability;
            tool.target_generation += 1;
            tool.target_generation
        };
        state.catalog_generation += 1;
        Ok(MutationReceipt {
            target_id: target_id.clone(),
            target_generation,
            catalog_generation: state.catalog_generation,
        })
    }

    /// Uninstalls a target: removed from every index; both generations
    /// advance. Later references fail with `TargetNotFound`/ambiguity as
    /// appropriate, and pinned old-generation requests fail with
    /// `StaleCatalog` — the target is never executed again under an old
    /// description.
    pub fn uninstall(&self, target_id: &ToolTargetId) -> Result<MutationReceipt, ToolCatalogError> {
        let mut state = self.lock();
        let tool =
            state
                .targets
                .remove(target_id)
                .ok_or_else(|| ToolCatalogError::TargetNotFound {
                    reference: format!("target {target_id}"),
                })?;
        let mut names = vec![tool.manifest.local_name.clone()];
        names.extend(tool.manifest.aliases.iter().cloned());
        for name in &names {
            if let Some(entries) = state.names.get_mut(name) {
                entries.retain(|(id, _)| id != target_id);
                if entries.is_empty() {
                    state.names.remove(name);
                }
            }
        }
        state.catalog_generation += 1;
        Ok(MutationReceipt {
            target_id: target_id.clone(),
            target_generation: tool.target_generation,
            catalog_generation: state.catalog_generation,
        })
    }

    /// The full prepare pipeline (R04-T01 怎么做 3): resolve the reference
    /// (aliases → authority; contested names need the source), verify the
    /// caller's pinned catalog generation (R04-A02), verify callability,
    /// validate + normalize the raw arguments against the CURRENT input
    /// schema, and derive the effective arguments, digest and shape
    /// summary — the single input the T02 gateway and T03 approvals bind
    /// to. Nothing here executes anything.
    pub fn prepare_invocation(
        &self,
        reference: &ToolTargetRef,
        pin: Option<CatalogPin>,
        raw_arguments: &serde_json::Value,
        budget: &SchemaBudget,
    ) -> Result<PreparedToolCall, ToolCatalogError> {
        budget.validate()?;
        let state = self.lock();
        // R04-A02 FIRST: a pinned (cached) catalog view must be the
        // CURRENT one. The stale-generation refusal wins over any
        // per-target outcome (not-found, uninstalled, renamed…) because
        // the caller's whole view is outdated: the only correct next step
        // is a fresh snapshot, after which the real per-target state is
        // visible. An old description never reaches the new object.
        if let Some(pin) = pin {
            if pin.catalog_generation != state.catalog_generation {
                return Err(ToolCatalogError::StaleCatalog {
                    held_generation: pin.catalog_generation,
                    current_generation: state.catalog_generation,
                    target_id: reference.describe(),
                });
            }
        }
        let target_id = resolve_locked(&state, reference)?;
        let tool = state
            .targets
            .get(&target_id)
            .expect("resolution proved the target exists");
        if !tool.manifest.availability.callable() {
            return Err(ToolCatalogError::TargetNotCallable {
                target_id: target_id.as_str().to_string(),
                availability: tool.manifest.availability.wire_name(),
                reason: tool.manifest.availability.refusal_reason(),
            });
        }
        let normalized =
            normalize_arguments(&tool.manifest.input_schema.schema, raw_arguments, budget)?;
        let effective = EffectiveArguments::from_value(normalized, budget)?;
        let args_digest = effective.digest();
        let args_summary = summarize_arguments(&effective);
        let prepared = PreparedToolCall {
            target_id,
            origin: tool.manifest.origin.clone(),
            local_name: tool.manifest.local_name.clone(),
            display_name: tool.manifest.display_name.clone(),
            version: tool.manifest.version.clone(),
            catalog_generation: state.catalog_generation,
            target_generation: tool.target_generation,
            effective,
            args_digest,
            args_summary,
            permission: tool.manifest.permission.clone(),
            recovery: tool.manifest.recovery,
            timeout_ms: tool.manifest.timeout_ms,
        };
        Ok(prepared)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RegistryState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Collision policy for incoming names (R04-T01 怎么做 5):
/// - an incoming ALIAS colliding with ANY existing name/alias of another
///   target → [`ToolCatalogError::NameCollision`] (one visible name never
///   mints a second identity);
/// - an incoming PRIMARY name colliding with another target's ALIAS →
///   collision;
/// - an incoming PRIMARY name colliding with other targets' PRIMARY names
///   (different sources) → LEGAL: both are indexed and name-only
///   resolution reports the explicit ambiguity.
fn check_name_collisions(
    state: &RegistryState,
    incoming: &[(String, NameRole)],
    incoming_id: &ToolTargetId,
) -> Result<(), ToolCatalogError> {
    for (name, role) in incoming {
        check_name_collisions_except(state, &[(name.clone(), *role)], incoming_id, name)?;
    }
    Ok(())
}

fn check_name_collisions_except(
    state: &RegistryState,
    entries: &[(String, NameRole)],
    incoming_id: &ToolTargetId,
    name: &str,
) -> Result<(), ToolCatalogError> {
    for (incoming_name, incoming_role) in entries {
        let Some(existing) = state.names.get(incoming_name) else {
            continue;
        };
        for (existing_id, existing_role) in existing {
            if existing_id == incoming_id {
                continue;
            }
            let collides = *incoming_role == NameRole::Alias || *existing_role == NameRole::Alias;
            if collides {
                return Err(ToolCatalogError::NameCollision {
                    name: name.to_string(),
                    existing_target_id: existing_id.as_str().to_string(),
                    incoming_target_id: incoming_id.as_str().to_string(),
                });
            }
        }
    }
    Ok(())
}

/// Resolution under a held lock (shared by `resolve` and
/// `prepare_invocation` so resolution + generation check are atomic).
fn resolve_locked(
    state: &RegistryState,
    reference: &ToolTargetRef,
) -> Result<ToolTargetId, ToolCatalogError> {
    match reference {
        ToolTargetRef::ByTargetId { target_id } => {
            if state.targets.contains_key(target_id) {
                Ok(target_id.clone())
            } else {
                Err(ToolCatalogError::TargetNotFound {
                    reference: reference.describe(),
                })
            }
        }
        ToolTargetRef::ByName { name } => {
            let matches =
                state
                    .names
                    .get(name)
                    .ok_or_else(|| ToolCatalogError::TargetNotFound {
                        reference: reference.describe(),
                    })?;
            unique_target_or_ambiguous(name, &matches.iter().map(|(id, _)| id).collect::<Vec<_>>())
        }
        ToolTargetRef::BySourceAndName { source_id, name } => {
            let matches =
                state
                    .names
                    .get(name)
                    .ok_or_else(|| ToolCatalogError::TargetNotFound {
                        reference: reference.describe(),
                    })?;
            let filtered: Vec<&ToolTargetId> = matches
                .iter()
                .filter(|(id, _)| {
                    state
                        .targets
                        .get(id)
                        .map(|tool| tool.manifest.origin.source_id() == source_id.as_str())
                        .unwrap_or(false)
                })
                .map(|(id, _)| id)
                .collect();
            if filtered.is_empty() {
                // The name exists but belongs to other sources: several
                // owners -> the explicit ambiguity (the caller must pick
                // a real source); ONE owner under a different source ->
                // this qualified reference genuinely matches nothing.
                let all: Vec<&ToolTargetId> = matches.iter().map(|(id, _)| id).collect();
                return match unique_target_or_ambiguous(name, &all) {
                    Err(ambiguity) => Err(ambiguity),
                    Ok(_) => Err(ToolCatalogError::TargetNotFound {
                        reference: reference.describe(),
                    }),
                };
            }
            unique_target_or_ambiguous(name, &filtered)
        }
    }
}

fn unique_target_or_ambiguous(
    name: &str,
    matches: &[&ToolTargetId],
) -> Result<ToolTargetId, ToolCatalogError> {
    let mut unique: Vec<&ToolTargetId> = matches.to_vec();
    unique.sort();
    unique.dedup();
    match unique.len() {
        1 => Ok(unique[0].clone()),
        _ => {
            let mut ids: Vec<String> = unique.iter().map(|id| id.as_str().to_string()).collect();
            ids.sort();
            Err(ToolCatalogError::TargetAmbiguous {
                name: name.to_string(),
                target_ids: ids,
            })
        }
    }
}

fn description_of(target_id: &ToolTargetId, tool: &RegisteredTool) -> ToolDescription {
    let manifest = &tool.manifest;
    let schema_bytes = lingxi_protocol::canon::canonical_json_bytes(&manifest.input_schema.schema);
    ToolDescription {
        target_id: target_id.clone(),
        origin: manifest.origin.clone(),
        source_id: manifest.origin.source_id().to_string(),
        local_name: manifest.local_name.clone(),
        display_name: manifest.display_name.clone(),
        aliases: manifest.aliases.clone(),
        version: manifest.version.clone(),
        target_generation: tool.target_generation,
        permission: manifest.permission.clone(),
        availability: manifest.availability.clone(),
        timeout_ms: manifest.timeout_ms,
        declared_permission: manifest.declared_permission,
        recovery: manifest.recovery,
        input_schema_digest: lingxi_protocol::canon::sha256_hex(&schema_bytes),
        input_dialect: manifest.input_schema.dialect.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lingxi_protocol::digest_arguments;
    use serde_json::json;

    fn budget() -> SchemaBudget {
        SchemaBudget::default()
    }

    fn schema(properties: serde_json::Value) -> ToolSchemaDocument {
        ToolSchemaDocument {
            dialect: "json-schema/2020-12".to_string(),
            schema: properties,
        }
    }

    fn read_manifest(origin: ToolOrigin, local_name: &str, display: &str) -> ToolManifest {
        ToolManifest {
            origin,
            local_name: local_name.to_string(),
            display_name: display.to_string(),
            aliases: Vec::new(),
            version: "1.0.0".to_string(),
            description: "test manifest".to_string(),
            input_schema: schema(json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "minLength": 1},
                    "mode": {"type": "string", "enum": ["stat", "read"], "default": "read"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 1000},
                },
                "required": ["path"],
                "additionalProperties": false,
            })),
            output_schema: None,
            permission: PermissionContract {
                kind: PermissionKind::Read,
                capability_base: format!("{local_name}.read"),
            },
            availability: Availability::Available,
            timeout_ms: Some(30_000),
            max_concurrency: Some(4),
            declared_permission: DeclaredPermission::ReadOnly,
            recovery: ToolRecoveryCapability::CONSERVATIVE,
        }
    }

    fn register_one(manifest: ToolManifest) -> Result<MutationReceipt, ToolCatalogError> {
        let registry = ToolRegistry::new();
        registry.register(manifest, &budget())
    }

    #[test]
    fn target_ids_are_origin_namespaced_and_ts_compatible() {
        let fp = tool_target_id(&ToolOrigin::FirstParty, "read").expect("first-party id");
        assert_eq!(fp.as_str(), "tool:first-party:read");
        let mcp = tool_target_id(
            &ToolOrigin::Mcp {
                server_id: "acme".to_string(),
            },
            "read",
        )
        .expect("mcp id");
        assert_eq!(mcp.as_str(), "tool:mcp:acme:read");
        let plugin = tool_target_id(
            &ToolOrigin::Plugin {
                plugin_id: "office".to_string(),
            },
            "to_pdf",
        )
        .expect("plugin id");
        assert_eq!(plugin.as_str(), "tool:plugin:office:to_pdf");
        // encodeURIComponent-compatible escaping of unsafe bytes.
        let spaced = tool_target_id(&ToolOrigin::FirstParty, "a b/c").expect("encoded");
        assert_eq!(spaced.as_str(), "tool:first-party:a%20b%2Fc");
        assert_ne!(fp, mcp, "same name from two sources = two identities");
    }

    #[test]
    fn manifest_names_are_validated() {
        let bad = read_manifest(ToolOrigin::FirstParty, " read", "Read");
        assert!(matches!(
            bad.validate_and_derive_id(&budget()),
            Err(ToolCatalogError::ManifestInvalid { .. })
        ));
    }

    #[test]
    fn same_name_two_sources_never_overwrites_and_name_only_is_ambiguous() {
        let registry = ToolRegistry::new();
        let fp = registry
            .register(
                read_manifest(ToolOrigin::FirstParty, "read", "Read"),
                &budget(),
            )
            .expect("first-party read registers");
        let mcp = registry
            .register(
                read_manifest(
                    ToolOrigin::Mcp {
                        server_id: "acme".to_string(),
                    },
                    "read",
                    "Read",
                ),
                &budget(),
            )
            .expect("mcp read registers under its own identity");
        assert_ne!(fp.target_id, mcp.target_id);
        // Name-only resolution of a contested name: explicit ambiguity
        // listing BOTH ids, never a silent pick.
        let err = registry
            .resolve(&ToolTargetRef::ByName {
                name: "read".to_string(),
            })
            .expect_err("contested name must be ambiguous");
        match err {
            ToolCatalogError::TargetAmbiguous { name, target_ids } => {
                assert_eq!(name, "read");
                assert_eq!(target_ids.len(), 2);
                assert!(target_ids.contains(&fp.target_id.as_str().to_string()));
                assert!(target_ids.contains(&mcp.target_id.as_str().to_string()));
            }
            other => panic!("expected ambiguity, got {other:?}"),
        }
        // Source-qualified resolution hits exactly one.
        assert_eq!(
            registry
                .resolve(&ToolTargetRef::BySourceAndName {
                    source_id: "acme".to_string(),
                    name: "read".to_string(),
                })
                .expect("mcp read resolves"),
            mcp.target_id
        );
        assert_eq!(
            registry
                .resolve(&ToolTargetRef::BySourceAndName {
                    source_id: "first-party".to_string(),
                    name: "read".to_string(),
                })
                .expect("first-party read resolves"),
            fp.target_id
        );
        // Wrong source for an existing name: the ambiguity, not a bare
        // not-found (the tool EXISTS under other sources).
        assert!(matches!(
            registry.resolve(&ToolTargetRef::BySourceAndName {
                source_id: "nope".to_string(),
                name: "read".to_string(),
            }),
            Err(ToolCatalogError::TargetAmbiguous { .. })
        ));
        // Discovery lists both with distinct ids; display names equal.
        let found = registry.search("read");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].display_name, found[1].display_name);
        // Duplicate registration of the SAME identity is a refusal.
        assert!(matches!(
            registry.register(
                read_manifest(ToolOrigin::FirstParty, "read", "Read"),
                &budget()
            ),
            Err(ToolCatalogError::DuplicateTarget { .. })
        ));
    }

    #[test]
    fn aliases_resolve_to_the_authoritative_identity_and_share_permissions() {
        let registry = ToolRegistry::new();
        let mut manifest = read_manifest(ToolOrigin::FirstParty, "web_search", "Web Search");
        manifest.aliases = vec!["search_web".to_string(), "netsearch".to_string()];
        let receipt = registry.register(manifest, &budget()).expect("registers");
        for name in ["web_search", "search_web", "netsearch"] {
            let resolved = registry.resolve(&ToolTargetRef::ByName {
                name: name.to_string(),
            });
            assert_eq!(
                resolved.expect("alias resolves"),
                receipt.target_id,
                "alias {name}"
            );
        }
        // Alias and primary name carry the SAME permission contract.
        let by_alias = registry
            .describe(
                &registry
                    .resolve(&ToolTargetRef::ByName {
                        name: "netsearch".to_string(),
                    })
                    .expect("alias"),
            )
            .expect("describe");
        let by_primary = registry.describe(&receipt.target_id).expect("describe");
        assert_eq!(by_alias.permission, by_primary.permission);
        assert_eq!(by_alias.target_id, by_primary.target_id);

        // An alias may never collide with ANOTHER target's name.
        let mut colliding = read_manifest(ToolOrigin::FirstParty, "other_tool", "Other");
        colliding.aliases = vec!["netsearch".to_string()];
        let err = registry
            .register(colliding, &budget())
            .expect_err("alias collision refused");
        assert!(matches!(err, ToolCatalogError::NameCollision { .. }));
        // And a local NAME may not collide with another target's alias.
        let colliding_name = read_manifest(ToolOrigin::FirstParty, "search_web", "X");
        assert!(matches!(
            registry.register(colliding_name, &budget()),
            Err(ToolCatalogError::NameCollision { .. })
        ));
        // Same primary name from another SOURCE stays legal (the A01
        // shape): both indexed, name-only resolution ambiguous.
        registry
            .register(
                read_manifest(ToolOrigin::FirstParty, "other_tool", "Other"),
                &budget(),
            )
            .expect("first-party primary name registers");
        let other_source = read_manifest(
            ToolOrigin::Mcp {
                server_id: "acme".to_string(),
            },
            "other_tool",
            "Other",
        );
        registry
            .register(other_source, &budget())
            .expect("cross-source primary name is legal");
        assert!(matches!(
            registry.resolve(&ToolTargetRef::ByName {
                name: "other_tool".to_string()
            }),
            Err(ToolCatalogError::TargetAmbiguous { .. })
        ));
    }

    #[test]
    fn prepare_validates_applies_defaults_and_derives_digest_and_summary() {
        let registry = ToolRegistry::new();
        registry
            .register(
                read_manifest(ToolOrigin::FirstParty, "read", "Read"),
                &budget(),
            )
            .expect("registers");
        let prepared = registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "read".to_string(),
                },
                None,
                &json!({"path": "/data/secret-file.txt"}),
                &budget(),
            )
            .expect("prepares");
        // Default applied: mode defaulted to "read".
        assert_eq!(
            prepared.effective.as_value().get("mode"),
            Some(&json!("read"))
        );
        // Digest is the trusted-boundary digest over canonical effective
        // arguments (matches lingxi-protocol::digest_arguments exactly).
        let expected = digest_arguments(prepared.effective.as_value());
        assert!(prepared.effective.digest_matches(&expected));
        assert_eq!(prepared.args_digest.hex, expected.hex);
        // Summary is shape-only: no path leakage.
        let summary = &prepared.args_summary;
        assert!(
            summary.contains("path:str"),
            "summary carries the key: {summary}"
        );
        assert!(
            !summary.contains("/data/secret-file.txt"),
            "summary must not leak values: {summary}"
        );
        // Invalid: missing required property.
        let err = registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "read".to_string(),
                },
                None,
                &json!({"mode": "stat"}),
                &budget(),
            )
            .expect_err("missing required");
        assert!(matches!(err, ToolCatalogError::ArgumentsInvalid { .. }));
        // Invalid: wrong type for a required property.
        assert!(matches!(
            registry.prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "read".to_string()
                },
                None,
                &json!({"path": 42}),
                &budget(),
            ),
            Err(ToolCatalogError::ArgumentsInvalid { .. })
        ));
        // Invalid: out-of-range integer.
        assert!(matches!(
            registry.prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "read".to_string()
                },
                None,
                &json!({"path": "x", "limit": 1001}),
                &budget(),
            ),
            Err(ToolCatalogError::ArgumentsInvalid { .. })
        ));
        // Unknown property rejected under additionalProperties:false.
        assert!(matches!(
            registry.prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "read".to_string()
                },
                None,
                &json!({"path": "x", "surprise": 1}),
                &budget(),
            ),
            Err(ToolCatalogError::ArgumentsInvalid { .. })
        ));
    }

    #[test]
    fn floats_and_unsafe_integers_are_rejected_parity_with_ts() {
        let registry = ToolRegistry::new();
        let mut manifest = read_manifest(ToolOrigin::FirstParty, "probe", "Probe");
        manifest.input_schema = schema(json!({
            "type": "object",
            "properties": {"n": {"type": "number"}},
        }));
        registry.register(manifest, &budget()).expect("registers");
        let err = registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "probe".to_string(),
                },
                None,
                &json!({"n": 1.5}),
                &budget(),
            )
            .expect_err("float rejected");
        assert!(matches!(
            err,
            ToolCatalogError::ArgumentsNotSafeInteger { .. }
        ));
        let err = registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "probe".to_string(),
                },
                None,
                &json!({"n": 9007199254740993i64}), // 2^53 + 1
                &budget(),
            )
            .expect_err("beyond safe range rejected");
        assert!(matches!(
            err,
            ToolCatalogError::ArgumentsNotSafeInteger { .. }
        ));
        // A safe integer passes.
        assert!(registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "probe".to_string()
                },
                None,
                &json!({"n": 9007199254740991i64}), // 2^53 - 1
                &budget(),
            )
            .is_ok());
    }

    #[test]
    fn schema_budget_and_depth_are_enforced() {
        // Nesting deeper than max_schema_depth: a chain of nested
        // property schemas.
        let mut deep = json!({"type": "string"});
        for level in 0..32 {
            deep = json!({"type": "object", "properties": {format!("l{level}"): deep}});
        }
        let mut manifest = read_manifest(ToolOrigin::FirstParty, "deep", "Deep");
        manifest.input_schema = schema(deep);
        assert!(matches!(
            register_one(manifest),
            Err(ToolCatalogError::SchemaBudgetExceeded {
                bound: "max_schema_depth",
                ..
            })
        ));

        // Oversized schema document.
        let mut properties = serde_json::Map::new();
        for i in 0..2048 {
            properties.insert(
                format!("p{i}"),
                json!({"type": "string", "description": "x".repeat(40)}),
            );
        }
        let mut big = read_manifest(ToolOrigin::FirstParty, "big", "Big");
        big.input_schema =
            schema(json!({"type": "object", "properties": serde_json::Value::Object(properties)}));
        assert!(matches!(
            register_one(big),
            Err(ToolCatalogError::SchemaBudgetExceeded {
                bound: "max_schema_bytes",
                ..
            })
        ));
    }

    #[test]
    fn reference_and_unsupported_keywords_fail_loudly() {
        // $ref: no reference resolution, ever (no network either).
        let mut with_ref = read_manifest(ToolOrigin::FirstParty, "ref_tool", "Ref");
        with_ref.input_schema =
            schema(json!({"type": "object", "properties": {"a": {"$ref": "#/$defs/a"}}}));
        assert!(matches!(
            register_one(with_ref),
            Err(ToolCatalogError::SchemaReferenceUnsupported { keyword, .. }) if keyword == "$ref"
        ));
        // pattern: cannot be honored exactly -> the SCHEMA is incompatible.
        let mut with_pattern = read_manifest(ToolOrigin::FirstParty, "pat_tool", "Pat");
        with_pattern.input_schema = schema(json!({
            "type": "object",
            "properties": {"a": {"type": "string", "pattern": "^x-"}}
        }));
        assert!(matches!(
            register_one(with_pattern),
            Err(ToolCatalogError::UnsupportedSchemaFeature { keyword, .. }) if keyword == "pattern"
        ));
        // Unknown dialect.
        let mut weird = read_manifest(ToolOrigin::FirstParty, "weird", "Weird");
        weird.input_schema.dialect = "typescript-tcomb".to_string();
        assert!(matches!(
            register_one(weird),
            Err(ToolCatalogError::UnknownDialect { .. })
        ));
        // Vendor annotations survive and are ignored for validation.
        let mut vendor = read_manifest(ToolOrigin::FirstParty, "vendor", "Vendor");
        vendor.input_schema = schema(json!({
            "type": "object",
            "properties": {"a": {"type": "string", "x-vendor-extension": {"any": true}}},
            "x-tool-category": "info",
        }));
        assert!(register_one(vendor).is_ok());
    }

    #[test]
    fn overly_deep_arguments_are_refused() {
        let registry = ToolRegistry::new();
        let mut manifest = read_manifest(ToolOrigin::FirstParty, "deep_args", "DeepArgs");
        manifest.input_schema = schema(json!({"type": "object"}));
        registry.register(manifest, &budget()).expect("registers");
        // Build nesting deeper than max_arguments_depth.
        let mut value = json!({"leaf": true});
        for level in 0..24 {
            value = json!({format!("l{level}"): value});
        }
        let err = registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "deep_args".to_string(),
                },
                None,
                &value,
                &budget(),
            )
            .expect_err("deep args refused");
        assert!(matches!(
            err,
            ToolCatalogError::ArgumentsBudgetExceeded {
                bound: "max_arguments_depth",
                ..
            }
        ));
    }

    #[test]
    fn stale_catalog_generation_refuses_and_requires_refresh() {
        let registry = ToolRegistry::new();
        let receipt = registry
            .register(
                read_manifest(ToolOrigin::FirstParty, "read", "Read"),
                &budget(),
            )
            .expect("registers");
        let snapshot = registry.snapshot();
        assert_eq!(snapshot.catalog_generation, receipt.catalog_generation);

        // Client caches generation 1 and prepares: OK.
        let pinned = CatalogPin {
            catalog_generation: snapshot.catalog_generation,
        };
        assert!(registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "read".to_string()
                },
                Some(pinned),
                &json!({"path": "x"}),
                &budget(),
            )
            .is_ok());

        // The tool updates to a new semantic (schema: `path` renamed to
        // `target_path`) — generation 2.
        let mut v2 = read_manifest(ToolOrigin::FirstParty, "read", "Read");
        v2.version = "2.0.0".to_string();
        v2.input_schema = schema(json!({
            "type": "object",
            "properties": {"target_path": {"type": "string", "minLength": 1}},
            "required": ["target_path"],
            "additionalProperties": false,
        }));
        let update = registry
            .update(&receipt.target_id, v2, &budget())
            .expect("updates");
        assert_eq!(update.target_generation, 2);
        assert!(update.catalog_generation > snapshot.catalog_generation);

        // Old pinned view submitting the OLD request shape: refused with
        // the explicit stale-generation error (not executed, not guessed).
        let err = registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "read".to_string(),
                },
                Some(pinned),
                &json!({"path": "x"}),
                &budget(),
            )
            .expect_err("stale pin refused");
        match err {
            ToolCatalogError::StaleCatalog {
                held_generation,
                current_generation,
                target_id,
            } => {
                assert_eq!(held_generation, snapshot.catalog_generation);
                assert!(current_generation > held_generation);
                // The refusal names the reference the caller used (the
                // actionable fact is the generation pair).
                assert_eq!(target_id, "name \"read\"");
            }
            other => panic!("expected StaleCatalog, got {other:?}"),
        }
        // Refreshed view (fresh pin) prepares the NEW shape fine.
        let fresh = registry.snapshot();
        assert!(registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "read".to_string()
                },
                Some(CatalogPin {
                    catalog_generation: fresh.catalog_generation,
                }),
                &json!({"target_path": "x"}),
                &budget(),
            )
            .is_ok());
    }

    #[test]
    fn disable_and_uninstall_change_callability_and_generation() {
        let registry = ToolRegistry::new();
        let receipt = registry
            .register(
                read_manifest(ToolOrigin::FirstParty, "write", "Write"),
                &budget(),
            )
            .expect("registers");
        let gen_after_register = registry.catalog_generation();

        // Disabled: discoverable, not callable.
        registry
            .set_availability(
                &receipt.target_id,
                Availability::Disabled {
                    reason: "user".to_string(),
                },
            )
            .expect("disables");
        assert!(registry.catalog_generation() > gen_after_register);
        let err = registry
            .prepare_invocation(
                &ToolTargetRef::ByTargetId {
                    target_id: receipt.target_id.clone(),
                },
                None,
                &json!({"path": "x"}),
                &budget(),
            )
            .expect_err("disabled not callable");
        assert!(matches!(err, ToolCatalogError::TargetNotCallable { .. }));
        let described = registry
            .describe(&receipt.target_id)
            .expect("still described");
        assert!(!described.availability.callable());
        assert!(described.availability.discoverable());

        // Re-enabled: callable again.
        registry
            .set_availability(&receipt.target_id, Availability::Available)
            .expect("enables");
        assert!(registry
            .prepare_invocation(
                &ToolTargetRef::ByTargetId {
                    target_id: receipt.target_id.clone(),
                },
                None,
                &json!({"path": "x"}),
                &budget(),
            )
            .is_ok());

        // Uninstalled: gone from every index; stale pins still refused by
        // the catalog generation alone.
        let pinned_before_uninstall = CatalogPin {
            catalog_generation: registry.catalog_generation(),
        };
        registry.uninstall(&receipt.target_id).expect("uninstalls");
        assert!(matches!(
            registry.resolve(&ToolTargetRef::ByName {
                name: "write".to_string()
            }),
            Err(ToolCatalogError::TargetNotFound { .. })
        ));
        assert!(matches!(
            registry.prepare_invocation(
                &ToolTargetRef::ByTargetId {
                    target_id: receipt.target_id.clone(),
                },
                None,
                &json!({"path": "x"}),
                &budget(),
            ),
            Err(ToolCatalogError::TargetNotFound { .. })
        ));
        assert!(matches!(
            registry.prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "write".to_string()
                },
                Some(pinned_before_uninstall),
                &json!({"path": "x"}),
                &budget(),
            ),
            Err(ToolCatalogError::StaleCatalog { .. })
        ));
    }

    #[test]
    fn future_capabilities_register_as_not_callable() {
        let registry = ToolRegistry::new();
        let mut manifest = read_manifest(ToolOrigin::FirstParty, "ask_user", "Ask User");
        manifest.availability = Availability::Future {
            reason: "business body lands in R07; catalog shape is honest".to_string(),
        };
        let receipt = registry.register(manifest, &budget()).expect("registers");
        let described = registry.describe(&receipt.target_id).expect("described");
        assert_eq!(described.availability.wire_name(), "future");
        assert!(!described.availability.callable());
        assert!(matches!(
            registry.prepare_invocation(
                &ToolTargetRef::ByTargetId {
                    target_id: receipt.target_id.clone(),
                },
                None,
                &json!({"path": "x"}),
                &budget(),
            ),
            Err(ToolCatalogError::TargetNotCallable { availability, .. }) if availability == "future"
        ));
    }

    #[test]
    fn declared_read_only_does_not_grant_verified_recovery() {
        let registry = ToolRegistry::new();
        // The MCP source DECLARES read-only…
        let mut manifest = read_manifest(
            ToolOrigin::Mcp {
                server_id: "acme".to_string(),
            },
            "read",
            "Read",
        );
        manifest.declared_permission = DeclaredPermission::ReadOnly;
        // …but the registrar supplies the VERIFIED (conservative)
        // capability: a claim is not a grant.
        assert_eq!(manifest.recovery, ToolRecoveryCapability::CONSERVATIVE);
        let receipt = registry.register(manifest, &budget()).expect("registers");
        let described = registry.describe(&receipt.target_id).expect("described");
        assert_eq!(described.declared_permission, DeclaredPermission::ReadOnly);
        assert_eq!(described.recovery, ToolRecoveryCapability::CONSERVATIVE);
        assert!(!described.recovery.read_only);
    }

    #[test]
    fn update_cannot_change_the_target_identity() {
        let registry = ToolRegistry::new();
        let receipt = registry
            .register(
                read_manifest(ToolOrigin::FirstParty, "read", "Read"),
                &budget(),
            )
            .expect("registers");
        let moved = read_manifest(ToolOrigin::FirstParty, "read2", "Read2");
        assert!(matches!(
            registry.update(&receipt.target_id, moved, &budget()),
            Err(ToolCatalogError::ManifestInvalid { .. })
        ));
    }

    #[test]
    fn snapshots_are_frozen_copies() {
        let registry = ToolRegistry::new();
        registry
            .register(
                read_manifest(ToolOrigin::FirstParty, "read", "Read"),
                &budget(),
            )
            .expect("registers");
        let snapshot = registry.snapshot();
        let captured_version = snapshot.tools[0].version.clone();
        let target = snapshot.tools[0].target_id.clone();
        let mut v2 = read_manifest(ToolOrigin::FirstParty, "read", "Read");
        v2.version = "9.9.9".to_string();
        registry.update(&target, v2, &budget()).expect("updates");
        // The old snapshot still describes the old version: descriptions
        // are copies, not live views.
        assert_eq!(snapshot.tools[0].version, captured_version);
        assert_eq!(captured_version, "1.0.0");
        let current = registry.describe(&target).expect("current");
        assert_eq!(current.version, "9.9.9");
    }

    #[test]
    fn summary_never_carries_values() {
        let args = EffectiveArguments::from_value(
            json!({
                "path": "/Users/secret/计划.md",
                "command": "rm -rf /",
                "body": "top secret contents",
                "count": 3,
                "flag": true,
                "list": [1, 2, 3],
            }),
            &budget(),
        )
        .expect("effective args");
        let summary = summarize_arguments(&args);
        for secret in ["/Users/secret", "rm -rf", "top secret"] {
            assert!(
                !summary.contains(secret),
                "summary leaked {secret:?}: {summary}"
            );
        }
        assert!(summary.contains("path:str"));
        assert!(summary.contains("count:int"));
        assert!(summary.contains("list:arr[3]"));
    }

    /// Adversarial probe: explicitly passing a schema default and omitting
    /// it must normalize to the SAME effective arguments (and therefore
    /// the SAME approval digest) — the whole point of defaults +
    /// canonicalization — while any actually different payload digests
    /// differently (no normalization collision mints one identity for two
    /// meanings).
    #[test]
    fn explicit_default_and_omitted_default_share_one_identity() {
        let registry = ToolRegistry::new();
        registry
            .register(
                read_manifest(ToolOrigin::FirstParty, "read", "Read"),
                &budget(),
            )
            .expect("registers");
        let omitted = registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "read".to_string(),
                },
                None,
                &json!({"path": "x"}),
                &budget(),
            )
            .expect("omitted default prepares");
        let explicit = registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "read".to_string(),
                },
                None,
                &json!({"path": "x", "mode": "read"}),
                &budget(),
            )
            .expect("explicit default prepares");
        assert_eq!(omitted.effective, explicit.effective);
        assert_eq!(omitted.args_digest.hex, explicit.args_digest.hex);
        let different = registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "read".to_string(),
                },
                None,
                &json!({"path": "x", "mode": "stat"}),
                &budget(),
            )
            .expect("different value prepares");
        assert_ne!(
            different.args_digest.hex, explicit.args_digest.hex,
            "a different payload must never share the approval digest"
        );
    }

    /// R04-T01-R1-F01 permanent regression: `minItems`/`maxItems` are
    /// constraints on the ARRAY VALUE, not on the `items` sub-schema. An
    /// items-less array schema (`{"type":"array","maxItems":N}` — a legal,
    /// common third-party/MCP shape) must still enforce them. Before the
    /// repair the checks were nested inside the `items` branch, so an
    /// over-/under-length array silently passed normalization — lenient
    /// parsing this boundary forbids.
    #[test]
    fn array_length_bounds_are_enforced_without_items_subschema() {
        let registry = ToolRegistry::new();
        let mut manifest = read_manifest(ToolOrigin::FirstParty, "tagged", "Tagged");
        manifest.input_schema = schema(json!({
            "type": "object",
            "properties": {
                "tags": {"type": "array", "minItems": 1, "maxItems": 2},
            },
            "required": ["tags"],
            "additionalProperties": false,
        }));
        registry
            .register(manifest, &budget())
            .expect("items-less array schema registers (minItems/maxItems are supported keywords)");

        // Over maxItems with NO items sub-schema: refused (was accepted).
        let err = registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "tagged".to_string(),
                },
                None,
                &json!({"tags": [1, 2, 3, 4, 5]}),
                &budget(),
            )
            .expect_err("maxItems must bind without an items sub-schema");
        match &err {
            ToolCatalogError::ArgumentsInvalid { violations } => {
                assert!(
                    violations.iter().any(|v| v.contains("maxItems")),
                    "violation must name maxItems: {violations:?}"
                );
            }
            other => panic!("expected ArgumentsInvalid, got {other:?}"),
        }

        // Under minItems with NO items sub-schema: refused.
        let err = registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "tagged".to_string(),
                },
                None,
                &json!({"tags": []}),
                &budget(),
            )
            .expect_err("minItems must bind without an items sub-schema");
        assert!(matches!(
            &err,
            ToolCatalogError::ArgumentsInvalid { violations }
                if violations.iter().any(|v| v.contains("minItems"))
        ));

        // Within bounds: accepted, payload preserved verbatim.
        let prepared = registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "tagged".to_string(),
                },
                None,
                &json!({"tags": [1, 2]}),
                &budget(),
            )
            .expect("in-bounds items-less array prepares");
        assert_eq!(
            prepared.effective.as_value().get("tags"),
            Some(&json!([1, 2]))
        );

        // Control: the WITH-items shape keeps enforcing the same bounds
        // (guards the moved code, not just the gap it closed).
        let mut with_items = read_manifest(ToolOrigin::FirstParty, "tagged2", "Tagged2");
        with_items.input_schema = schema(json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 2,
                    "items": {"type": "integer"},
                },
            },
            "required": ["tags"],
            "additionalProperties": false,
        }));
        registry
            .register(with_items, &budget())
            .expect("with-items array schema registers");
        assert!(matches!(
            registry.prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "tagged2".to_string()
                },
                None,
                &json!({"tags": [1, 2, 3]}),
                &budget(),
            ),
            Err(ToolCatalogError::ArgumentsInvalid { .. })
        ));
        assert!(registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "tagged2".to_string()
                },
                None,
                &json!({"tags": [1, 2]}),
                &budget(),
            )
            .is_ok());
    }

    /// Adversarial probe: a schema with no required properties accepts an
    /// empty object (explicitly passing NOTHING is a legitimate call), and
    /// the result is still immutable, digest-bearing effective arguments.
    #[test]
    fn empty_arguments_on_defaultless_schema_are_legal() {
        let registry = ToolRegistry::new();
        let mut manifest = read_manifest(ToolOrigin::FirstParty, "no_args", "NoArgs");
        manifest.input_schema = schema(json!({"type": "object"}));
        registry.register(manifest, &budget()).expect("registers");
        let prepared = registry
            .prepare_invocation(
                &ToolTargetRef::ByName {
                    name: "no_args".to_string(),
                },
                None,
                &json!({}),
                &budget(),
            )
            .expect("empty object prepares");
        assert_eq!(prepared.effective.as_value(), &json!({}));
        assert!(!prepared.args_digest.hex.is_empty());
    }
}

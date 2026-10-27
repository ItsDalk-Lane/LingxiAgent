//! R06-T06 skill library store: catalog scanning, package installation and
//! bundle persistence (SkillService port implementations).
//!
//! Incumbent anchors (RC-2; design doc §3):
//! - SkillManager discovery/whitelist: `core/skill-manager.ts:126-131,141-197,199-252,344-411,447-453`
//! - workspace dirs/policy: `shared/workspace-skill-paths.ts:14-26,75-118`
//! - package install: `lib/skills/skill-package-installer.ts:67-139,141-202,204-250`
//! - metadata rewrite: `skill-package-installer.ts:82-115`
//! - removal/scrub: `lib/skills/skill-removal.ts:22-138`
//! - bundles store: `lib/skill-bundles/store.ts:5-106,130-238`

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use lingxi_kernel::memory::{
    parse_skill_metadata, resolve_workspace_skill_candidate_states, sanitize_skill_name,
    WorkspaceSkillCandidate, WorkspaceSkillPolicy,
};

use super::agent_memory::{atomic_write, now_iso, MemoryError};

// ── catalog ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub file_path: PathBuf,
    pub base_dir: PathBuf,
    /// "user" | "external" | "plugin" | "workspace"
    pub source: String,
    pub hidden: bool,
    pub default_enabled: bool,
    pub disable_model_invocation: bool,
    pub external_label: Option<String>,
    pub external_path: Option<String>,
    pub readonly: bool,
    /// true when the skill comes from a plugin-provided dir
    pub plugin_skill: bool,
    /// true when the skill comes from a workspace dir
    pub workspace_skill: bool,
    pub workspace_category: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExternalSkillPath {
    pub dir_path: PathBuf,
    pub label: String,
    /// "workspace" | "external"
    pub scope: String,
    pub category: String,
}

/// Heavy directory names the incumbent watcher skips (skill-manager.ts:23-26);
/// the scanner mirrors the exclusion for catalog parity.
pub const HEAVY_DIR_NAMES: [&str; 9] = [
    "node_modules",
    "target",
    "build",
    "dist",
    "out",
    "__pycache__",
    "coverage",
    "venv",
    ".venv",
];

pub struct SkillCatalog;

impl SkillCatalog {
    fn scan_dir(
        dir_path: &Path,
        transform: impl Fn(&str, &Path, &str, &str, bool, bool, bool) -> SkillEntry,
    ) -> Vec<SkillEntry> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir_path) else {
            return out;
        };
        let mut dirs: Vec<String> = entries
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        dirs.sort();
        for name in dirs {
            let skill_file = dir_path.join(&name).join("SKILL.md");
            let Ok(content) = std::fs::read_to_string(&skill_file) else {
                continue;
            };
            let meta = parse_skill_metadata(&content, &name);
            let base_dir = dir_path.join(&name);
            out.push(transform(
                &name,
                &base_dir,
                meta.name.as_deref().unwrap_or(&name),
                &meta.description,
                meta.default_enabled,
                meta.disable_model_invocation,
                true,
            ));
        }
        out
    }

    /// Skills of the user/global pool (skillsDir) — user-owned, deletable.
    pub fn scan_user_pool(
        skills_dir: &Path,
        hidden: &std::collections::HashSet<String>,
    ) -> Vec<SkillEntry> {
        Self::scan_dir(
            skills_dir,
            |_dir_name,
             base_dir,
             meta_name,
             description,
             default_enabled,
             disable_model_invocation,
             _ok| {
                let name = meta_name.to_string();
                SkillEntry {
                    name: name.clone(),
                    description: description.to_string(),
                    file_path: base_dir.join("SKILL.md"),
                    base_dir: base_dir.to_path_buf(),
                    source: "user".into(),
                    hidden: hidden.contains(&name),
                    default_enabled,
                    disable_model_invocation,
                    external_label: None,
                    external_path: None,
                    readonly: false,
                    plugin_skill: false,
                    workspace_skill: false,
                    workspace_category: None,
                }
            },
        )
    }

    /// Built-in seed pool (skills2set/, shipped with the product).
    pub fn scan_builtin(seed_dir: &Path) -> Vec<SkillEntry> {
        Self::scan_dir(
            seed_dir,
            |_dir_name,
             base_dir,
             meta_name,
             description,
             default_enabled,
             disable_model_invocation,
             _ok| {
                let name = meta_name.to_string();
                SkillEntry {
                    name,
                    description: description.to_string(),
                    file_path: base_dir.join("SKILL.md"),
                    base_dir: base_dir.to_path_buf(),
                    source: "user".into(),
                    hidden: false,
                    default_enabled,
                    disable_model_invocation,
                    external_label: Some("builtin".into()),
                    external_path: Some(seed_dir.to_string_lossy().to_string()),
                    readonly: true,
                    plugin_skill: false,
                    workspace_skill: false,
                    workspace_category: None,
                }
            },
        )
    }

    /// External/plugin/workspace-compatible directory scan
    /// (skill-manager.ts:350-399): readonly for non-workspace scopes.
    pub fn scan_external(paths: &[ExternalSkillPath]) -> Vec<SkillEntry> {
        let mut out = Vec::new();
        for entry_path in paths {
            let owner = if entry_path.scope == "workspace" {
                "workspace"
            } else if entry_path.label.starts_with("plugin:") {
                "plugin"
            } else {
                "external"
            };
            let readonly = owner != "workspace";
            let plugin = entry_path.label.starts_with("plugin:");
            let workspace = entry_path.scope == "workspace";
            let scanned = Self::scan_dir(
                &entry_path.dir_path,
                |_dir_name,
                 base_dir,
                 meta_name,
                 description,
                 default_enabled,
                 disable_model_invocation,
                 _ok| {
                    SkillEntry {
                        name: meta_name.to_string(),
                        description: description.to_string(),
                        file_path: base_dir.join("SKILL.md"),
                        base_dir: base_dir.to_path_buf(),
                        source: "external".into(),
                        hidden: false,
                        default_enabled,
                        disable_model_invocation,
                        external_label: Some(entry_path.label.clone()),
                        external_path: Some(entry_path.dir_path.to_string_lossy().to_string()),
                        readonly,
                        plugin_skill: plugin,
                        workspace_skill: workspace,
                        workspace_category: if workspace {
                            Some(entry_path.category.clone())
                        } else {
                            None
                        },
                    }
                },
            );
            out.extend(scanned);
        }
        out
    }
}

/// Runtime selection (`skill-manager.ts:199-252`): non-workspace skills are
/// active iff plugin or in the agent's enabled set; workspace skills resolve
/// through the policy/shadow contract. `entries` mirrors the per-skill view.
pub struct RuntimeSelection {
    pub active: Vec<String>,
    /// Active ENTRIES in resolution order (non-workspace catalog order first,
    /// then resolved workspace states). Name-keyed consumers MUST use this —
    /// filtering the catalog by `active` names would double-activate
    /// same-name copies across pools (the shadowed copy would leak into the
    /// prompt; R06-T06 R1 repair found this through the compiler consumer).
    pub active_entries: Vec<SkillEntry>,
    pub entries: Vec<Value>,
}

pub fn resolve_runtime_selection(
    all: &[SkillEntry],
    enabled: &std::collections::HashSet<String>,
    workspace_disabled: &std::collections::HashSet<String>,
    policy: &WorkspaceSkillPolicy,
) -> RuntimeSelection {
    let mut claimed: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut active = Vec::new();
    let mut active_entries = Vec::new();
    let mut entries = Vec::new();
    let mut workspace_candidates = Vec::new();
    let mut workspace_candidate_entries = Vec::new();
    for skill in all {
        if skill.workspace_skill {
            workspace_candidates.push(WorkspaceSkillCandidate {
                name: skill.name.clone(),
                source_category: skill
                    .workspace_category
                    .clone()
                    .unwrap_or_else(|| "standard".into()),
            });
            workspace_candidate_entries.push(skill.clone());
            continue;
        }
        let runtime_enabled = skill.plugin_skill || enabled.contains(&skill.name);
        claimed
            .entry(skill.name.clone())
            .or_insert_with(|| skill.name.clone());
        if runtime_enabled {
            active.push(skill.name.clone());
            active_entries.push(skill.clone());
        }
        entries.push(json!({
            "name": skill.name,
            "active": runtime_enabled,
            "shadowed": false,
            "shadowedBy": Value::Null,
            "inactiveReason": if runtime_enabled { Value::Null } else { json!("disabled") },
        }));
    }
    let resolved = resolve_workspace_skill_candidate_states(
        &workspace_candidates,
        policy,
        &mut claimed,
        workspace_disabled,
    );
    for (state, entry) in resolved.into_iter().zip(workspace_candidate_entries) {
        if state.active {
            active.push(state.candidate.name.clone());
            active_entries.push(entry);
        }
        entries.push(json!({
            "name": state.candidate.name,
            "active": state.active,
            "shadowed": state.shadowed,
            "shadowedBy": state.shadowed_by,
            "inactiveReason": state.inactive_reason,
        }));
    }
    RuntimeSelection {
        active,
        active_entries,
        entries,
    }
}

/// New-agent default enabled set (skill-manager.ts:260-264): every
/// non-external, non-opt-out global skill name.
pub fn compute_default_enabled(all: &[SkillEntry]) -> Vec<String> {
    all.iter()
        .filter(|s| s.source != "external" && s.default_enabled)
        .map(|s| s.name.clone())
        .collect()
}

// ── package installation ────────────────────────────────────────────────────

pub const SKILL_MISSING_SKILL_MD: &str = "SKILL_MISSING_SKILL_MD";
pub const SKILL_MISSING_NAME: &str = "SKILL_MISSING_NAME";
pub const SKILL_INVALID_NAME: &str = "SKILL_INVALID_NAME";
pub const SKILL_PACKAGE_SYMLINK: &str = "SKILL_PACKAGE_SYMLINK";
pub const SKILL_INSTALL_TARGET_OUTSIDE_ROOT: &str = "SKILL_INSTALL_TARGET_OUTSIDE_ROOT";
pub const SKILL_SOURCE_MUST_BE_ABSOLUTE: &str = "SKILL_SOURCE_MUST_BE_ABSOLUTE";
pub const SKILL_SOURCE_NOT_FOUND: &str = "SKILL_SOURCE_NOT_FOUND";
pub const SKILL_UNSUPPORTED_FORMAT: &str = "SKILL_UNSUPPORTED_FORMAT";

#[derive(Debug)]
pub struct InstalledSkill {
    pub name: String,
    pub dir: PathBuf,
    pub file_path: PathBuf,
}

fn visible_subdirectories(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names
}

/// findSkillPackageRoot (skill-package-installer.ts:50-74).
pub fn find_skill_package_root(root_dir: &Path, subpath: &str) -> Option<PathBuf> {
    let normalized = subpath.replace('\\', "/");
    let normalized = normalized.trim_matches('/').to_string();
    let mut candidates = Vec::new();
    if !normalized.is_empty() {
        candidates.push(root_dir.join(&normalized));
        for name in visible_subdirectories(root_dir) {
            candidates.push(root_dir.join(name).join(&normalized));
        }
    } else {
        candidates.push(root_dir.to_path_buf());
        for name in visible_subdirectories(root_dir) {
            candidates.push(root_dir.join(name));
        }
    }
    candidates
        .into_iter()
        .find(|c| c.join("SKILL.md").is_file())
}

fn assert_no_symlink_entries(dir: &Path) -> Result<(), MemoryError> {
    let entries = std::fs::read_dir(dir).map_err(|e| MemoryError::Io(e.to_string()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| MemoryError::Io(e.to_string()))?;
        // symlink check: file_type from readdir is resolved on unix for
        // dir/file but NOT for symlinks (is_symlink); use symlink_metadata.
        let meta = std::fs::symlink_metadata(&path).map_err(|e| MemoryError::Io(e.to_string()))?;
        if meta.file_type().is_symlink() {
            return Err(MemoryError::Conflict {
                code: SKILL_PACKAGE_SYMLINK.into(),
                message: format!("skill package cannot contain symlink: {}", path.display()),
            });
        }
        if file_type.is_dir() {
            assert_no_symlink_entries(&path)?;
        }
    }
    Ok(())
}

fn assert_install_target_inside_root(target: &Path, install_dir: &Path) -> Result<(), MemoryError> {
    // The root is canonicalized (symlink-alias roots like /var vs
    // /private/var on macOS must not false-positive); the target resolves
    // THROUGH the canonical root (the name is already sanitized to
    // [A-Za-z0-9_-], so any escape attempt shows up here).
    let root = install_dir
        .canonicalize()
        .unwrap_or_else(|_| install_dir.to_path_buf());
    let Some(relative) = target.file_name() else {
        return Err(MemoryError::Conflict {
            code: SKILL_INSTALL_TARGET_OUTSIDE_ROOT.into(),
            message: format!("invalid skill install target: {}", target.display()),
        });
    };
    let resolved = root.join(relative);
    if resolved != root && resolved.starts_with(&root) {
        return Ok(());
    }
    Err(MemoryError::Conflict {
        code: SKILL_INSTALL_TARGET_OUTSIDE_ROOT.into(),
        message: format!("invalid skill install target: {}", target.display()),
    })
}

/// escapeYamlScalar + upsertFrontmatterLine (skill-package-installer.ts:82-95):
/// safe names render bare, anything else JSON-quoted.
fn escape_yaml_scalar(value: &str) -> String {
    if sanitize_skill_name(value).as_deref() == Some(value) {
        value.to_string()
    } else {
        json!(value).to_string()
    }
}

/// rewriteSkillInstallMetadata (skill-package-installer.ts:97-115).
pub fn rewrite_skill_install_metadata(
    content: &str,
    skill_name: &str,
    default_enabled: Option<bool>,
) -> String {
    let body = content.to_string();
    let mut lines = vec![format!("name: {}", escape_yaml_scalar(skill_name))];
    if let Some(flag) = default_enabled {
        lines.push(format!("default-enabled: {flag}"));
    }
    let trimmed = body.strip_prefix("---").and_then(|rest| {
        let rest = rest.strip_prefix('\u{feff}').unwrap_or(rest);
        let after_newline = rest
            .strip_prefix('\n')
            .unwrap_or_else(|| rest.strip_prefix("\r\n").unwrap_or(rest));
        let end = after_newline.find("\n---").map(|idx| {
            let fm = &after_newline[..idx];
            let tail_start = idx + 1 + 3;
            let tail = &after_newline[tail_start..];
            let tail = tail
                .strip_prefix('\n')
                .unwrap_or(tail.strip_prefix('\r').unwrap_or(tail));
            (fm, tail)
        });
        end
    });
    match trimmed {
        Some((frontmatter, rest)) => {
            let mut fm = frontmatter.to_string();
            fm = {
                let mut out = Vec::new();
                let mut name_done = false;
                let mut flag_done = default_enabled.is_none();
                for l in fm.lines() {
                    if let Some((k, _)) = l.split_once(':') {
                        if k.trim() == "name" && !name_done {
                            out.push(format!("name: {}", escape_yaml_scalar(skill_name)));
                            name_done = true;
                            continue;
                        }
                        if k.trim() == "default-enabled" && !flag_done {
                            out.push(format!("default-enabled: {}", default_enabled.unwrap()));
                            flag_done = true;
                            continue;
                        }
                    }
                    out.push(l.to_string());
                }
                if !name_done {
                    out.push(format!("name: {}", escape_yaml_scalar(skill_name)));
                }
                if !flag_done {
                    out.push(format!("default-enabled: {}", default_enabled.unwrap()));
                }
                out.join("\n")
            };
            format!("---\n{fm}\n---\n{rest}")
        }
        None => format!("---\n{}\n---\n\n{body}", lines.join("\n")),
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), MemoryError> {
    std::fs::create_dir_all(dst).map_err(|e| MemoryError::Io(e.to_string()))?;
    let entries = std::fs::read_dir(src).map_err(|e| MemoryError::Io(e.to_string()))?;
    for entry in entries.flatten() {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let meta = std::fs::symlink_metadata(&from).map_err(|e| MemoryError::Io(e.to_string()))?;
        if meta.file_type().is_symlink() {
            return Err(MemoryError::Conflict {
                code: SKILL_PACKAGE_SYMLINK.into(),
                message: format!("skill package cannot contain symlink: {}", from.display()),
            });
        }
        if meta.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| MemoryError::Io(e.to_string()))?;
        }
    }
    Ok(())
}

/// installSkillPackageFromDirectory (skill-package-installer.ts:141-202).
pub fn install_skill_package_from_directory(
    source_dir: &Path,
    install_dir: &Path,
    subpath: &str,
    default_enabled: Option<bool>,
) -> Result<InstalledSkill, MemoryError> {
    let Some(skill_dir) = find_skill_package_root(source_dir, subpath) else {
        return Err(MemoryError::Conflict {
            code: SKILL_MISSING_SKILL_MD.into(),
            message: "skill package missing SKILL.md".into(),
        });
    };
    assert_no_symlink_entries(&skill_dir)?;
    let content = std::fs::read_to_string(skill_dir.join("SKILL.md"))
        .map_err(|e| MemoryError::Io(e.to_string()))?;
    let meta = parse_skill_metadata(&content, "");
    let raw_name =
        meta.name
            .filter(|n| !n.trim().is_empty())
            .ok_or_else(|| MemoryError::Conflict {
                code: SKILL_MISSING_NAME.into(),
                message: "skill package missing frontmatter name".into(),
            })?;
    let Some(safe_name) = sanitize_skill_name(&raw_name) else {
        return Err(MemoryError::Conflict {
            code: SKILL_INVALID_NAME.into(),
            message: format!("invalid skill name: {raw_name}"),
        });
    };
    std::fs::create_dir_all(install_dir).map_err(|e| MemoryError::Io(e.to_string()))?;
    let dst_dir = install_dir.join(&safe_name);
    assert_install_target_inside_root(&dst_dir, install_dir)?;
    copy_dir_recursive(&skill_dir, &dst_dir)?;
    let skill_file_path = dst_dir.join("SKILL.md");
    if default_enabled.is_some() || safe_name != raw_name {
        let current = std::fs::read_to_string(&skill_file_path)
            .map_err(|e| MemoryError::Io(e.to_string()))?;
        std::fs::write(
            &skill_file_path,
            rewrite_skill_install_metadata(&current, &safe_name, default_enabled),
        )
        .map_err(|e| MemoryError::Io(e.to_string()))?;
    }
    Ok(InstalledSkill {
        name: safe_name,
        dir: dst_dir,
        file_path: skill_file_path,
    })
}

/// installSkillPackageFromPath (skill-package-installer.ts:204-250):
/// directory sources install in place; .zip/.skill are rejected with the
/// incumbent code (archive extraction has no locked dependency — diff entry).
pub fn install_skill_package_from_path(
    source_path: &Path,
    install_dir: &Path,
    default_enabled: Option<bool>,
) -> Result<InstalledSkill, MemoryError> {
    if !source_path.is_absolute() {
        return Err(MemoryError::Conflict {
            code: SKILL_SOURCE_MUST_BE_ABSOLUTE.into(),
            message: "skill source path must be absolute".into(),
        });
    }
    if !source_path.exists() {
        return Err(MemoryError::Conflict {
            code: SKILL_SOURCE_NOT_FOUND.into(),
            message: "skill source path does not exist".into(),
        });
    }
    let meta =
        std::fs::symlink_metadata(source_path).map_err(|e| MemoryError::Io(e.to_string()))?;
    if meta.is_dir() {
        return install_skill_package_from_directory(source_path, install_dir, "", default_enabled);
    }
    let ext = source_path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let _ = ext;
    Err(MemoryError::Conflict {
        code: SKILL_UNSUPPORTED_FORMAT.into(),
        message: "unsupported skill package format".into(),
    })
}

/// prepareLocalSkillPackage (skill-package-installer.ts:252-288): directory
/// source validation only (no copy) — used by install_skill's local branch.
pub fn prepare_local_skill_package(source_path: &Path) -> Result<PathBuf, MemoryError> {
    if !source_path.is_absolute() {
        return Err(MemoryError::Conflict {
            code: SKILL_SOURCE_MUST_BE_ABSOLUTE.into(),
            message: "skill source path must be absolute".into(),
        });
    }
    if !source_path.exists() {
        return Err(MemoryError::Conflict {
            code: SKILL_SOURCE_NOT_FOUND.into(),
            message: "skill source path does not exist".into(),
        });
    }
    let meta =
        std::fs::symlink_metadata(source_path).map_err(|e| MemoryError::Io(e.to_string()))?;
    if meta.is_dir() {
        let Some(skill_dir) = find_skill_package_root(source_path, "") else {
            return Err(MemoryError::Conflict {
                code: SKILL_MISSING_SKILL_MD.into(),
                message: "skill package missing SKILL.md".into(),
            });
        };
        assert_no_symlink_entries(&skill_dir)?;
        return Ok(skill_dir);
    }
    Err(MemoryError::Conflict {
        code: SKILL_UNSUPPORTED_FORMAT.into(),
        message: "unsupported skill package format".into(),
    })
}

/// removeUserSkills physical step (skill-removal.ts:104-137): rm -r the pool
/// dir; reference scrubbing is the caller's job.
pub fn remove_skill_directory(skills_dir: &Path, name: &str) -> Result<bool, MemoryError> {
    let dir = skills_dir.join(name);
    if !dir.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(&dir).map_err(|e| MemoryError::Io(e.to_string()))?;
    Ok(true)
}

/// listSeededSkillNames (skill-removal.ts:22-28).
pub fn list_seeded_skill_names(seed_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(seed_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names
}

/// collectEnabledSkillsOfOtherAgents (skill-removal.ts:42-65): read every
/// surviving agent's config.yaml skills.enabled, skipping tombstones.
pub fn collect_enabled_skills_of_other_agents(
    agents_root: &Path,
    exclude_agent_id: &str,
) -> Result<std::collections::HashSet<String>, MemoryError> {
    let mut used = std::collections::HashSet::new();
    let entries = match std::fs::read_dir(agents_root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(used),
        Err(err) => return Err(MemoryError::Io(err.to_string())),
    };
    for entry in entries {
        let entry = entry.map_err(|err| MemoryError::Io(err.to_string()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == exclude_agent_id
            || !entry
                .file_type()
                .map_err(|err| MemoryError::Io(err.to_string()))?
                .is_dir()
        {
            continue;
        }
        let agent_dir = agents_root.join(&name);
        if agent_dir.join(".deleted-agent.json").exists() {
            continue;
        }
        let config_path = agent_dir.join("config.yaml");
        let text = match super::agent_memory::config::read_config_text(&config_path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(MemoryError::Io(err.to_string())),
        };
        let (enabled, _) = super::agent_memory::AgentConfigPatch::read_skill_lists(&text)?;
        used.extend(enabled);
    }
    Ok(used)
}

// ── bundles (skill-bundles/store.ts) ────────────────────────────────────────

pub const BUNDLE_STORE_FILE: &str = "skill-bundles.json";
pub const BUNDLE_SCHEMA_VERSION: u64 = 1;

fn unique_strings(values: &[Value]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for value in values {
        let Some(name) = value.as_str().map(str::trim) else {
            continue;
        };
        if name.is_empty() || !seen.insert(name.to_string()) {
            continue;
        }
        out.push(name.to_string());
    }
    out
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    let sliced: String = trimmed.chars().take(56).collect();
    if sliced.is_empty() {
        "bundle".to_string()
    } else {
        sliced
    }
}

fn random_suffix() -> String {
    // 6 hex chars from the OS RNG (store.ts:37-39 randomBytes(3)).
    let mut bytes = [0u8; 3];
    getrandom::getrandom(&mut bytes).expect("OS RNG unavailable");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn normalize_bundle(bundle: &Value) -> Value {
    let fallback_now = now_iso();
    let created_at = bundle
        .get("createdAt")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(&fallback_now)
        .to_string();
    let updated_at = bundle
        .get("updatedAt")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(&created_at)
        .to_string();
    let name = bundle
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or("Skill Bundle");
    let id = bundle
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|i| !i.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}-{}", slugify(name), random_suffix()));
    let empty = Vec::new();
    let skill_names = bundle
        .get("skillNames")
        .and_then(Value::as_array)
        .map(|a| unique_strings(a))
        .unwrap_or_else(|| unique_strings(&empty));
    json!({
        "id": id,
        "name": name,
        "skillNames": skill_names,
        "source": bundle.get("source").and_then(Value::as_str).unwrap_or("user"),
        "agentId": bundle.get("agentId").and_then(Value::as_str).map(|v| json!(v)).unwrap_or(Value::Null),
        "sourcePackage": bundle.get("sourcePackage").and_then(Value::as_str).map(|v| json!(v)).unwrap_or(Value::Null),
        "createdAt": created_at,
        "updatedAt": updated_at,
    })
}

pub struct BundlesStore {
    file_path: PathBuf,
}

impl BundlesStore {
    pub fn new(home: &Path) -> Self {
        Self {
            file_path: home.join(BUNDLE_STORE_FILE),
        }
    }

    pub fn load(&self) -> Vec<Value> {
        let raw = std::fs::read_to_string(&self.file_path).unwrap_or_default();
        if raw.trim().is_empty() {
            return Vec::new();
        }
        match serde_json::from_str::<Value>(&raw) {
            Ok(value) => value
                .get("bundles")
                .and_then(Value::as_array)
                .map(|arr| arr.iter().map(normalize_bundle).collect())
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    /// 删除前不能把损坏或读失败的组合资料当空表，避免随后保存抹掉用户内容。
    pub fn load_checked(&self) -> Result<Vec<Value>, MemoryError> {
        let raw = match std::fs::read_to_string(&self.file_path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(MemoryError::Io(format!("read skill bundles: {err}"))),
        };
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        let value: Value = serde_json::from_str(&raw)
            .map_err(|err| MemoryError::InvalidRequest(format!("invalid skill bundles: {err}")))?;
        let bundles = value
            .get("bundles")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                MemoryError::InvalidRequest(
                    "invalid skill bundles: bundles must be an array".into(),
                )
            })?;
        Ok(bundles.iter().map(normalize_bundle).collect())
    }

    pub fn save(&self, bundles: &[Value]) -> Result<(), MemoryError> {
        let normalized: Vec<Value> = bundles.iter().map(normalize_bundle).collect();
        let value = json!({"schemaVersion": BUNDLE_SCHEMA_VERSION, "bundles": normalized});
        let text =
            serde_json::to_string_pretty(&value).map_err(|e| MemoryError::Store(e.to_string()))?;
        atomic_write(&self.file_path, &text).map_err(MemoryError::from)
    }

    pub fn create(
        &self,
        name: &str,
        skill_names: &[String],
        source: &str,
        agent_id: Option<&str>,
    ) -> Result<Value, MemoryError> {
        let mut bundles = self.load();
        let mut store = json!({"schemaVersion": BUNDLE_SCHEMA_VERSION, "bundles": bundles});
        let id = allocate_bundle_id(&mut store, name)?;
        let record = normalize_bundle(&json!({
            "id": id,
            "name": name,
            "skillNames": skill_names,
            "source": source,
            "agentId": agent_id,
            "createdAt": now_iso(),
            "updatedAt": now_iso(),
        }));
        bundles.push(record.clone());
        self.save(&bundles)?;
        Ok(record)
    }

    pub fn update(
        &self,
        id: &str,
        name: Option<&str>,
        skill_names: Option<&[String]>,
    ) -> Result<Value, MemoryError> {
        if id.trim().is_empty() {
            return Err(MemoryError::InvalidRequest("bundle id is required".into()));
        }
        let mut bundles = self.load();
        let idx = bundles
            .iter()
            .position(|b| b.get("id").and_then(Value::as_str) == Some(id))
            .ok_or_else(|| MemoryError::NotFound("skill bundle not found".into()))?;
        let current = bundles[idx].clone();
        let mut next = current.clone();
        if let Some(n) = name {
            next["name"] = json!(n);
        }
        if let Some(names) = skill_names {
            next["skillNames"] = json!(names);
        }
        next["updatedAt"] = json!(now_iso());
        let next = normalize_bundle(&next);
        bundles[idx] = next.clone();
        self.save(&bundles)?;
        Ok(next)
    }

    pub fn reorder(&self, bundle_ids: &[String]) -> Result<Vec<Value>, MemoryError> {
        if bundle_ids.is_empty() {
            return Err(MemoryError::InvalidRequest(
                "bundleIds must be a non-empty array".into(),
            ));
        }
        let bundles = self.load();
        if bundle_ids.len() != bundles.len() {
            return Err(MemoryError::InvalidRequest(
                "bundleIds must include every skill bundle exactly once".into(),
            ));
        }
        let mut ordered = Vec::new();
        for id in bundle_ids {
            let bundle = bundles
                .iter()
                .find(|b| b.get("id").and_then(Value::as_str) == Some(id.as_str()))
                .ok_or_else(|| {
                    MemoryError::InvalidRequest(format!("unknown skill bundle: {id}"))
                })?;
            let mut bundle = bundle.clone();
            bundle["updatedAt"] = json!(now_iso());
            ordered.push(bundle);
        }
        self.save(&ordered)?;
        Ok(ordered)
    }

    pub fn delete(&self, id: &str) -> Result<bool, MemoryError> {
        let bundles = self.load();
        let before = bundles.len();
        let remaining: Vec<Value> = bundles
            .into_iter()
            .filter(|b| b.get("id").and_then(Value::as_str) != Some(id))
            .collect();
        if remaining.len() == before {
            return Ok(false);
        }
        self.save(&remaining)?;
        Ok(true)
    }

    pub fn remove_skills(&self, names: &[String]) -> Result<Vec<Value>, MemoryError> {
        let bundles = self.load_checked()?;
        let updated: Vec<Value> = bundles
            .into_iter()
            .map(|b| {
                let filtered: Vec<String> = b["skillNames"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(Value::as_str)
                            .filter(|n| !names.contains(&n.to_string()))
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                let mut b = b.clone();
                b["skillNames"] = json!(filtered);
                b["updatedAt"] = json!(now_iso());
                b
            })
            .filter(|b| {
                !b["skillNames"]
                    .as_array()
                    .map(|a| a.is_empty())
                    .unwrap_or(false)
            })
            .collect();
        self.save(&updated)?;
        Ok(updated)
    }

    pub fn detach_agent(&self, agent_id: &str) -> Result<Vec<Value>, MemoryError> {
        let bundles = self.load();
        let updated: Vec<Value> = bundles
            .into_iter()
            .map(|b| {
                if b.get("agentId").and_then(Value::as_str) == Some(agent_id) {
                    let mut b = b.clone();
                    b["agentId"] = Value::Null;
                    b["updatedAt"] = json!(now_iso());
                    b
                } else {
                    b.clone()
                }
            })
            .collect();
        self.save(&updated)?;
        Ok(updated)
    }
}

fn allocate_bundle_id(store: &mut Value, base_name: &str) -> Result<String, MemoryError> {
    let base = slugify(base_name);
    let used: std::collections::HashSet<String> = store["bundles"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|b| b.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if !used.contains(&base) {
        return Ok(base);
    }
    for _ in 0..20 {
        let candidate = format!("{base}-{}", random_suffix());
        if !used.contains(&candidate) {
            return Ok(candidate);
        }
    }
    Err(MemoryError::Store(format!(
        "cannot allocate unique skill bundle id for {base_name}"
    )))
}

/// bundleForResponse (skills.ts:81-97): per-agent enabled flag per skill.
pub fn bundle_for_response(
    bundle: &Value,
    skill_by_name: &std::collections::HashMap<String, (SkillEntry, bool)>,
) -> Value {
    let mut bundle = bundle.clone();
    let skills: Vec<Value> = bundle["skillNames"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(
                    |name| match name.as_str().and_then(|n| skill_by_name.get(n)) {
                        Some((entry, enabled)) => json!({
                            "name": name,
                            "enabled": enabled,
                            "source": entry.source,
                            "missing": false,
                        }),
                        None => json!({
                            "name": name,
                            "enabled": false,
                            "source": Value::Null,
                            "missing": true,
                        }),
                    },
                )
                .collect()
        })
        .unwrap_or_default();
    bundle["skills"] = Value::Array(skills);
    bundle
}

/// getRuntimeSkillInfos per-skill projection (skill-manager.ts:159-183).
pub fn skill_info_json(skill: &SkillEntry, selection_state: Option<&Value>) -> Value {
    json!({
        "name": skill.name,
        "description": skill.description,
        "filePath": skill.file_path.to_string_lossy(),
        "baseDir": skill.base_dir.to_string_lossy(),
        "source": if skill.workspace_skill { "workspace".to_string() } else { skill.source.clone() },
        "hidden": skill.hidden,
        "enabled": skill.plugin_skill || skill.workspace_skill,
        "active": selection_state.and_then(|s| s.get("active")).and_then(Value::as_bool).unwrap_or(false),
        "shadowed": selection_state.and_then(|s| s.get("shadowed")).and_then(Value::as_bool).unwrap_or(false),
        "shadowedBy": selection_state.and_then(|s| s.get("shadowedBy")).cloned().unwrap_or(Value::Null),
        "inactiveReason": selection_state.and_then(|s| s.get("inactiveReason")).cloned().unwrap_or(Value::Null),
        "externalLabel": skill.external_label,
        "externalPath": skill.external_path,
        "readonly": skill.readonly,
        "managedBy": if skill.workspace_skill { json!("workspace") } else { Value::Null },
        "sourceCategory": skill.workspace_category,
    })
}

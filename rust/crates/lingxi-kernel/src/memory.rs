//! R06-T06 memory & skill domain rules (PURE — zero IO, zero clock).
//!
//! Incumbent anchors (RC-2, base `e89c91a2…`):
//! - fact search text / FTS query construction: `lib/memory/fact-store.ts:50-92`
//! - tenets store rules: `lib/memory/tenets.ts:24-65,103-105,137-182,232-357,436-512`
//! - SKILL.md frontmatter metadata: `lib/skills/skill-metadata.ts:41-65`
//! - skill name sanitation: `lib/skills/skill-package-installer.ts:9,23-28`
//! - workspace skill policy: `shared/workspace-skill-paths.ts:4-26,75-118`
//! - install_skill risk-confirmation protocol: `lib/tools/install-skill.ts:43-46,225-279`
//! - agent loop guard state machine: `lib/extensions/agent-loop-guard-ext.ts:41-77,118-205`
//! - L1 tool_result truncation: `core/compaction-utils.ts:99-131` +
//!   `lib/extensions/compaction-guard-ext.ts:133-138`
//! - compiled-memory normalization & assembly: `lib/memory/compiled-memory-state.ts:46-83`
//!   + `lib/memory/compile.ts:1054-1091` + `lib/memory/navigation.ts:14-76`

use serde_json::Value;

use crate::session_tree::sha256_hex;

// ── Fact search text (fact-store.ts:37-97) ──────────────────────────────────

fn is_cjk_char(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF      // CJK Unified Ideographs (Script=Han core)
        | 0x3400..=0x4DBF    // Extension A (Script=Han)
        | 0x3040..=0x309F    // Hiragana
        | 0x30A0..=0x30FF    // Katakana
        | 0xAC00..=0xD7AF    // Hangul syllables
        | 0x1100..=0x11FF    // Hangul Jamo
    )
}

fn normalize_search_text(text: &str) -> String {
    text.trim().to_string()
}

/// CJK 2/3-grams of every CJK run in the text (fact-store.ts:50-63).
pub fn cjk_ngrams(text: &str) -> Vec<String> {
    let normalized = normalize_search_text(text);
    let mut tokens = Vec::new();
    let mut run: Vec<char> = Vec::new();
    let flush = |run: &mut Vec<char>, tokens: &mut Vec<String>| {
        for size in [2usize, 3] {
            if run.len() < size {
                continue;
            }
            for i in 0..=(run.len() - size) {
                tokens.push(run[i..i + size].iter().collect());
            }
        }
        run.clear();
    };
    for c in normalized.chars() {
        if is_cjk_char(c) {
            run.push(c);
        } else {
            flush(&mut run, &mut tokens);
        }
    }
    flush(&mut run, &mut tokens);
    tokens
}

fn unique_tokens(tokens: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for token in tokens {
        let normalized = normalize_search_text(&token);
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }
        out.push(normalized);
    }
    out
}

/// Search text stored alongside a fact: base tokens (fact + tags) plus CJK
/// grams, unique, space-joined (fact-store.ts:77-81).
pub fn build_fact_search_text(fact: &str, tags: &[String]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let base = [fact]
        .into_iter()
        .chain(tags.iter().map(|s| s.as_str()))
        .map(normalize_search_text)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    parts.push(base.clone());
    parts.extend(cjk_ngrams(&base));
    unique_tokens(parts).join(" ")
}

pub fn has_cjk(text: &str) -> bool {
    normalize_search_text(text).chars().any(is_cjk_char)
}

/// FTS MATCH query: lexical whitespace tokens + CJK grams, each double-quoted
/// (inner quotes doubled), joined with OR (fact-store.ts:83-92).
pub fn build_fts_query(query: &str) -> String {
    let normalized = normalize_search_text(query);
    if normalized.is_empty() {
        return String::new();
    }
    let mut terms: Vec<String> = normalized
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();
    terms.extend(cjk_ngrams(&normalized));
    unique_tokens(terms)
        .into_iter()
        .map(|w| format!("\"{}\"", w.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// Zero-result diagnostics: unique whitespace terms, capped (memory-search.ts:70-94).
pub fn zero_result_diagnostic_terms(query: &str, max_terms: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for term in query
        .split_whitespace()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if seen.insert(term.to_string()) {
            out.push(term.to_string());
            if out.len() >= max_terms {
                break;
            }
        }
    }
    out
}

pub const ZERO_RESULT_DIAGNOSTIC_MAX_TERMS: usize = 6;

// ── Tenets (tenets.ts) ───────────────────────────────────────────────────────

pub const TENET_PRIORITIES: [&str; 4] = ["critical", "high", "medium", "low"];
pub const MAX_ACTIVE_TENETS: usize = 20;
pub const MAX_ACTIVE_USER_TENETS: usize = 200;
pub const MAX_PENDING_TENETS: usize = 30;
pub const MAX_TENET_CONTENT_CHARS: usize = 300;
pub const TENETS_SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenetStatus {
    Pending,
    Active,
    Rejected,
}

impl TenetStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TenetStatus::Pending => "pending",
            TenetStatus::Active => "active",
            TenetStatus::Rejected => "rejected",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(TenetStatus::Pending),
            "active" => Some(TenetStatus::Active),
            "rejected" => Some(TenetStatus::Rejected),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenetSource {
    ModelProposed,
    UserDirect,
}

impl TenetSource {
    pub fn as_str(self) -> &'static str {
        match self {
            TenetSource::ModelProposed => "model_proposed",
            TenetSource::UserDirect => "user_direct",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "model_proposed" => Some(TenetSource::ModelProposed),
            "user_direct" => Some(TenetSource::UserDirect),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenetErrorCode {
    LimitReached,
    PendingFull,
    Invalid,
    NotFound,
    StoreCorrupted,
    StoreUnsupportedSchema,
    StoreReadFailed,
}

impl TenetErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            TenetErrorCode::LimitReached => "TENET_LIMIT_REACHED",
            TenetErrorCode::PendingFull => "TENET_PENDING_FULL",
            TenetErrorCode::Invalid => "TENET_INVALID",
            TenetErrorCode::NotFound => "TENET_NOT_FOUND",
            TenetErrorCode::StoreCorrupted => "TENET_STORE_CORRUPTED",
            TenetErrorCode::StoreUnsupportedSchema => "TENET_STORE_UNSUPPORTED_SCHEMA",
            TenetErrorCode::StoreReadFailed => "TENET_STORE_READ_FAILED",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tenet {
    pub id: String,
    pub content: String,
    pub priority: String,
    pub status: TenetStatus,
    pub source: TenetSource,
    pub session_id: Option<String>,
    pub created_at: String,
    pub decided_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TenetsFile {
    pub schema_version: u64,
    pub tenets: Vec<Tenet>,
}

impl TenetsFile {
    pub fn empty() -> Self {
        Self {
            schema_version: TENETS_SCHEMA_VERSION,
            tenets: Vec::new(),
        }
    }
}

pub fn normalize_tenet_priority(value: &str) -> String {
    if TENET_PRIORITIES.contains(&value) {
        value.to_string()
    } else {
        "medium".to_string()
    }
}

pub fn normalize_tenet_content(value: &str) -> String {
    let unified = value.replace("\r\n", "\n").replace('\r', "\n");
    unified.trim().to_string()
}

/// Normalized dedup key: lowercase + trailing punctuation strip
/// (tenets.ts:103-105).
pub fn tenet_dedup_key(content: &str) -> String {
    let trimmed = content.trim().to_lowercase();
    trimmed
        .trim_end_matches(|c| {
            matches!(
                c,
                '。' | '.' | '！' | '!' | '？' | '?' | '；' | ';' | '，' | ','
            )
        })
        .trim()
        .to_string()
}

/// Strict read validation (tenets.ts:137-182): corrupt JSON / bad shape /
/// future schema / invalid entries / duplicate ids are loud errors, never a
/// silent empty-store overwrite.
pub fn validate_tenets_strict(raw: &Value) -> Result<TenetsFile, TenetErrorCode> {
    let Some(obj) = raw.as_object() else {
        return Err(TenetErrorCode::StoreCorrupted);
    };
    let Some(schema_version) = obj.get("schemaVersion").and_then(Value::as_u64) else {
        return Err(TenetErrorCode::StoreCorrupted);
    };
    if schema_version < 1 {
        return Err(TenetErrorCode::StoreCorrupted);
    }
    if schema_version > TENETS_SCHEMA_VERSION {
        return Err(TenetErrorCode::StoreUnsupportedSchema);
    }
    let empty = Vec::new();
    let raw_tenets: &[Value] = match obj.get("tenets") {
        None => &empty,
        Some(v) if v.is_array() => v.as_array().unwrap(),
        Some(_) => return Err(TenetErrorCode::StoreCorrupted),
    };
    let mut tenets = Vec::new();
    for entry in raw_tenets {
        let Some(e) = entry.as_object() else {
            return Err(TenetErrorCode::StoreCorrupted);
        };
        let id_ok = e
            .get("id")
            .and_then(Value::as_str)
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        let content_ok = e
            .get("content")
            .and_then(Value::as_str)
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let status_ok = e
            .get("status")
            .and_then(Value::as_str)
            .map(|s| TenetStatus::parse(s).is_some())
            .unwrap_or(false);
        let source_ok = e
            .get("source")
            .and_then(Value::as_str)
            .map(|s| TenetSource::parse(s).is_some())
            .unwrap_or(false);
        let created_ok = e
            .get("createdAt")
            .and_then(Value::as_str)
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        // createdAt is validated for parseability at the store boundary; the
        // pure kernel only demands non-empty (ISO parse is IO-adjacent and is
        // enforced by the adapters' strict reader using the same error code).
        if !id_ok || !content_ok || !status_ok || !source_ok || !created_ok {
            return Err(TenetErrorCode::StoreCorrupted);
        }
        tenets.push(Tenet {
            id: e["id"].as_str().unwrap().to_string(),
            content: normalize_tenet_content(e["content"].as_str().unwrap()),
            priority: normalize_tenet_priority(
                e.get("priority").and_then(Value::as_str).unwrap_or(""),
            ),
            status: TenetStatus::parse(e["status"].as_str().unwrap()).unwrap(),
            source: TenetSource::parse(e["source"].as_str().unwrap()).unwrap(),
            session_id: e
                .get("sessionId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            created_at: e["createdAt"].as_str().unwrap().to_string(),
            decided_at: e
                .get("decidedAt")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        });
    }
    let ids: std::collections::HashSet<&str> = tenets.iter().map(|t| t.id.as_str()).collect();
    if ids.len() != tenets.len() {
        return Err(TenetErrorCode::StoreCorrupted);
    }
    Ok(TenetsFile {
        schema_version,
        tenets,
    })
}

fn priority_weight(priority: &str) -> u8 {
    match priority {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        _ => 3,
    }
}

/// Full list ordering: priority asc (critical first), then createdAt asc
/// (tenets.ts:232-239).
pub fn sort_tenets(tenets: &mut [Tenet]) {
    tenets.sort_by(|a, b| {
        priority_weight(&a.priority)
            .cmp(&priority_weight(&b.priority))
            .then_with(|| a.created_at.cmp(&b.created_at))
    });
}

pub fn count_active_by_source(tenets: &[Tenet]) -> (usize, usize) {
    let mut model_proposed = 0usize;
    let mut user_direct = 0usize;
    for tenet in tenets {
        if tenet.status != TenetStatus::Active {
            continue;
        }
        match tenet.source {
            TenetSource::UserDirect => user_direct += 1,
            TenetSource::ModelProposed => model_proposed += 1,
        }
    }
    (model_proposed, user_direct)
}

pub struct NewTenetInput<'a> {
    pub content: &'a str,
    pub priority: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub new_id: String,
    pub now_iso: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TenetOpError {
    Code(TenetErrorCode),
}

/// Model proposal → pending (tenets.ts:253-285). Duplicate (any status) returns
/// Ok with `duplicate` without mutating.
pub fn tenets_add_proposal(
    file: &mut TenetsFile,
    input: &NewTenetInput<'_>,
) -> Result<(Tenet, bool, Option<TenetStatus>), TenetErrorCode> {
    let content = normalize_tenet_content(input.content);
    assert_tenet_content(&content)?;
    let key = tenet_dedup_key(&content);
    if let Some(existing) = file
        .tenets
        .iter()
        .find(|t| tenet_dedup_key(&t.content) == key)
    {
        return Ok((existing.clone(), true, Some(existing.status)));
    }
    let pending = file
        .tenets
        .iter()
        .filter(|t| t.status == TenetStatus::Pending)
        .count();
    if pending >= MAX_PENDING_TENETS {
        return Err(TenetErrorCode::PendingFull);
    }
    let tenet = Tenet {
        id: input.new_id.clone(),
        content,
        priority: normalize_tenet_priority(input.priority.unwrap_or("")),
        status: TenetStatus::Pending,
        source: TenetSource::ModelProposed,
        session_id: input
            .session_id
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        created_at: input.now_iso.clone(),
        decided_at: None,
    };
    file.tenets.push(tenet.clone());
    Ok((tenet, false, None))
}

/// Direct pin → active/user_direct (tenets.ts:293-324). Only an ACTIVE
/// normalized duplicate is idempotent; a pending/rejected duplicate still
/// produces a new active entry.
pub fn tenets_add_direct(
    file: &mut TenetsFile,
    input: &NewTenetInput<'_>,
) -> Result<(Tenet, bool), TenetErrorCode> {
    let content = normalize_tenet_content(input.content);
    assert_tenet_content(&content)?;
    let key = tenet_dedup_key(&content);
    if let Some(existing) = file
        .tenets
        .iter()
        .find(|t| t.status == TenetStatus::Active && tenet_dedup_key(&t.content) == key)
    {
        return Ok((existing.clone(), true));
    }
    let (_, user_direct) = count_active_by_source(&file.tenets);
    if user_direct >= MAX_ACTIVE_USER_TENETS {
        return Err(TenetErrorCode::LimitReached);
    }
    let tenet = Tenet {
        id: input.new_id.clone(),
        content,
        priority: normalize_tenet_priority(input.priority.unwrap_or("")),
        status: TenetStatus::Active,
        source: TenetSource::UserDirect,
        session_id: None,
        created_at: input.now_iso.clone(),
        decided_at: Some(input.now_iso.clone()),
    };
    file.tenets.push(tenet.clone());
    Ok((tenet, false))
}

fn assert_tenet_content(content: &str) -> Result<(), TenetErrorCode> {
    if content.is_empty() {
        return Err(TenetErrorCode::Invalid);
    }
    if content.chars().count() > MAX_TENET_CONTENT_CHARS {
        return Err(TenetErrorCode::Invalid);
    }
    Ok(())
}

/// Approve/reject a pending proposal (tenets.ts:327-348).
pub fn tenets_decide(
    file: &mut TenetsFile,
    tenet_id: &str,
    approve: bool,
    now_iso: &str,
) -> Result<Tenet, TenetErrorCode> {
    let idx = file
        .tenets
        .iter()
        .position(|t| t.id == tenet_id)
        .ok_or(TenetErrorCode::NotFound)?;
    if file.tenets[idx].status != TenetStatus::Pending {
        return Err(TenetErrorCode::Invalid);
    }
    if approve {
        let (model_proposed, _) = count_active_by_source(&file.tenets);
        if model_proposed >= MAX_ACTIVE_TENETS {
            return Err(TenetErrorCode::LimitReached);
        }
    }
    let tenet = &mut file.tenets[idx];
    tenet.status = if approve {
        TenetStatus::Active
    } else {
        TenetStatus::Rejected
    };
    tenet.decided_at = Some(now_iso.to_string());
    Ok(tenet.clone())
}

pub fn tenets_remove(file: &mut TenetsFile, tenet_id: &str) -> bool {
    let before = file.tenets.len();
    file.tenets.retain(|t| t.id != tenet_id);
    file.tenets.len() < before
}

fn pinned_key(content: &str) -> String {
    // PUT /pinned 的归一键（agents.ts:970/:979）：空白折叠 + 小写（无标点剥离）。
    content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// PUT /pinned full replace (agents.ts:954-988): remove every ACTIVE entry
/// (model_proposed included) whose normalized content is not in the incoming
/// list, then add missing entries as user_direct active (addTenetDirect quota
/// + duplicate semantics). Err(LimitReached) aborts the additions and returns
///   what was created so far; the caller surfaces the error.
pub fn tenets_replace_pinned(
    file: &mut TenetsFile,
    pins: &[String],
    id_factory: &mut dyn FnMut() -> String,
    now_iso: &str,
) -> Result<Vec<Tenet>, TenetErrorCode> {
    let contents: Vec<String> = pins
        .iter()
        .map(|p| normalize_tenet_content(p))
        .filter(|p| !p.is_empty())
        .collect();
    let wanted: std::collections::HashSet<String> =
        contents.iter().map(|c| pinned_key(c)).collect();
    file.tenets
        .retain(|t| t.status != TenetStatus::Active || wanted.contains(&pinned_key(&t.content)));
    let mut created = Vec::new();
    let existing_keys: std::collections::HashSet<String> =
        file.tenets.iter().map(|t| pinned_key(&t.content)).collect();
    for content in contents {
        if existing_keys.contains(&pinned_key(&content)) {
            continue;
        }
        let (tenet, duplicate) = tenets_add_direct(
            file,
            &NewTenetInput {
                content: &content,
                priority: None,
                session_id: None,
                new_id: id_factory(),
                now_iso: now_iso.to_string(),
            },
        )?;
        if !duplicate {
            created.push(tenet);
        }
    }
    Ok(created)
}

// ── SKILL.md metadata (skill-metadata.ts + skill-package-installer.ts) ──────

pub const MAX_SKILL_DESCRIPTION_LENGTH: usize = 1024;

/// `^[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}$` (skill-package-installer.ts:9).
pub fn sanitize_skill_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    if !(first.is_ascii_alphanumeric()) {
        return None;
    }
    let rest = chars.count();
    if rest > 63 {
        return None;
    }
    if trimmed[1..]
        .chars()
        .any(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    {
        return None;
    }
    Some(trimmed.to_string())
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SkillMetadata {
    pub name: Option<String>,
    pub description: String,
    pub disable_model_invocation: bool,
    pub default_enabled: bool,
}

/// Minimal YAML frontmatter subset covering exactly the fields the incumbent
/// consumes (name / description / disable-model-invocation / default-enabled
/// / defaultEnabled, top level or under a `metadata:` block). Everything else
/// is ignored — matching skill-metadata.ts's "only frontmatter contributes"
/// trust boundary with a registered subset parser (diff ledger).
pub fn parse_skill_metadata(content: &str, fallback_name: &str) -> SkillMetadata {
    let mut meta = SkillMetadata {
        name: if fallback_name.is_empty() {
            None
        } else {
            Some(fallback_name.to_string())
        },
        description: String::new(),
        disable_model_invocation: false,
        default_enabled: true,
    };
    let Some(frontmatter) = extract_frontmatter(content) else {
        return meta;
    };
    let mut section = String::new();
    for line in frontmatter.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() || trimmed.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let body = line.trim();
        if indent == 0 {
            if let Some(name) = body.strip_suffix(':') {
                section = name.trim().to_string();
                continue;
            }
            section.clear();
        }
        let Some((key, value)) = body.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = clean_yaml_scalar(value);
        let in_metadata = section == "metadata";
        match (in_metadata, key) {
            (false, "name") | (true, "name") if !value.is_empty() => {
                meta.name = Some(value.clone());
            }
            (false, "description") | (true, "description") => {
                meta.description = collapse_description(value);
            }
            (false, "disable-model-invocation") | (true, "disable-model-invocation") => {
                meta.disable_model_invocation = value == "true";
            }
            (false, "default-enabled") | (true, "default-enabled") => {
                meta.default_enabled = value != "false";
            }
            (false, "defaultEnabled") | (true, "defaultEnabled") => {
                meta.default_enabled = value != "false";
            }
            _ => {}
        }
    }
    meta
}

fn extract_frontmatter(content: &str) -> Option<&str> {
    let rest = content.strip_prefix("---")?;
    let rest = rest.strip_prefix(['\r', '\n']).unwrap_or(rest);
    let mut end = None;
    let mut offset = 0usize;
    for line in rest.lines() {
        let trimmed = line.trim_end();
        if trimmed == "---" {
            end = Some(offset);
            break;
        }
        offset += line.len() + 1;
    }
    let end = end?;
    Some(&rest[..end])
}

fn clean_yaml_scalar(raw: &str) -> String {
    let without_comment = raw.split(" #").next().unwrap_or(raw);
    let trimmed = without_comment.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
        || (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2)
    {
        return trimmed[1..trimmed.len() - 1].to_string();
    }
    trimmed.to_string()
}

fn collapse_description(value: String) -> String {
    let mut collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > MAX_SKILL_DESCRIPTION_LENGTH {
        collapsed = collapsed
            .chars()
            .take(MAX_SKILL_DESCRIPTION_LENGTH)
            .collect();
    }
    collapsed
}

// ── Workspace skill policy (workspace-skill-paths.ts) ────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceSkillPolicy {
    pub discover_project_skills: bool,
    pub discover_compatible_project_skills: bool,
}

pub const DEFAULT_WORKSPACE_SKILL_POLICY: WorkspaceSkillPolicy = WorkspaceSkillPolicy {
    discover_project_skills: true,
    discover_compatible_project_skills: false,
};

pub const WORKSPACE_SKILL_DIRS: [(&str, &str, &str); 4] = [
    (".agents/skills", "Agents", "standard"),
    (".claude/skills", "Claude Code", "compatible"),
    (".codex/skills", "Codex", "compatible"),
    (".openclaw/skills", "OpenClaw", "compatible"),
];

pub fn workspace_skill_policy_from_config(
    discover_project_skills: Option<bool>,
    discover_compatible_project_skills: Option<bool>,
) -> WorkspaceSkillPolicy {
    WorkspaceSkillPolicy {
        discover_project_skills: discover_project_skills.unwrap_or(true),
        discover_compatible_project_skills: discover_compatible_project_skills.unwrap_or(false),
    }
}

pub fn workspace_skill_category_enabled(category: &str, policy: &WorkspaceSkillPolicy) -> bool {
    if category == "compatible" {
        policy.discover_compatible_project_skills
    } else {
        policy.discover_project_skills
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceSkillCandidate {
    pub name: String,
    pub source_category: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceSkillState {
    pub candidate: WorkspaceSkillCandidate,
    pub active: bool,
    pub shadowed: bool,
    pub shadowed_by: Option<String>,
    pub inactive_reason: Option<&'static str>,
}

/// Ordered candidate resolution with the winner/shadow contract
/// (workspace-skill-paths.ts:75-118).
pub fn resolve_workspace_skill_candidate_states(
    candidates: &[WorkspaceSkillCandidate],
    policy: &WorkspaceSkillPolicy,
    claimed_by_name: &mut std::collections::HashMap<String, String>,
    disabled_names: &std::collections::HashSet<String>,
) -> Vec<WorkspaceSkillState> {
    candidates
        .iter()
        .map(|candidate| {
            if !workspace_skill_category_enabled(&candidate.source_category, policy) {
                return WorkspaceSkillState {
                    candidate: candidate.clone(),
                    active: false,
                    shadowed: false,
                    shadowed_by: None,
                    inactive_reason: Some("policy-disabled"),
                };
            }
            if disabled_names.contains(&candidate.name) {
                return WorkspaceSkillState {
                    candidate: candidate.clone(),
                    active: false,
                    shadowed: false,
                    shadowed_by: None,
                    inactive_reason: Some("user-disabled"),
                };
            }
            if let Some(prior) = claimed_by_name.get(&candidate.name) {
                return WorkspaceSkillState {
                    candidate: candidate.clone(),
                    active: false,
                    shadowed: true,
                    shadowed_by: Some(prior.clone()),
                    inactive_reason: Some("shadowed"),
                };
            }
            claimed_by_name.insert(candidate.name.clone(), candidate.name.clone());
            WorkspaceSkillState {
                candidate: candidate.clone(),
                active: true,
                shadowed: false,
                shadowed_by: None,
                inactive_reason: None,
            }
        })
        .collect()
}

// ── install_skill risk-confirmation protocol (install-skill.ts:225-279) ─────

pub const MAX_SKILL_SIZE_BYTES: usize = 50_000;
pub const RISK_CONFIRMATION_TTL_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone, PartialEq)]
pub struct RiskConfirmationEntry {
    pub source_key: String,
    pub digest: String,
    pub reason: String,
    pub expires_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RiskAcceptanceConsumption {
    NotAccepted,
    Accepted,
    Rejected(&'static str),
}

#[derive(Debug, Clone, Default)]
pub struct RiskConfirmationStore {
    pub pending: std::collections::HashMap<String, RiskConfirmationEntry>,
}

pub fn risk_confirmation_digest(skill_content: &str) -> String {
    sha256_hex(skill_content.as_bytes())
}

impl RiskConfirmationStore {
    pub fn prune_expired(&mut self, now_unix_ms: u64) {
        self.pending
            .retain(|_, entry| entry.expires_at_unix_ms > now_unix_ms);
    }

    /// Creates a one-shot token bound to (sourceKey, sha256(content))
    /// (install-skill.ts:236-246). Randomness is caller-supplied.
    pub fn create_token(
        &mut self,
        source_key: &str,
        skill_content: &str,
        reason: &str,
        token_suffix: &str,
        now_unix_ms: u64,
    ) -> String {
        self.prune_expired(now_unix_ms);
        let token = format!("risk_{token_suffix}");
        self.pending.insert(
            token.clone(),
            RiskConfirmationEntry {
                source_key: source_key.to_string(),
                digest: risk_confirmation_digest(skill_content),
                reason: reason.to_string(),
                expires_at_unix_ms: now_unix_ms + RISK_CONFIRMATION_TTL_MS,
            },
        );
        token
    }

    /// Consumes a risk acceptance (install-skill.ts:248-264).
    pub fn consume_acceptance(
        &mut self,
        risk_accepted: bool,
        risk_confirmation_token: Option<&str>,
        source_key: &str,
        skill_content: &str,
        now_unix_ms: u64,
    ) -> RiskAcceptanceConsumption {
        if !risk_accepted {
            return RiskAcceptanceConsumption::NotAccepted;
        }
        self.prune_expired(now_unix_ms);
        let Some(token) = risk_confirmation_token
            .map(str::trim)
            .filter(|t| !t.is_empty())
        else {
            return RiskAcceptanceConsumption::Rejected("missing_confirmation_token");
        };
        let Some(entry) = self.pending.get(token) else {
            return RiskAcceptanceConsumption::Rejected("invalid_or_expired_confirmation_token");
        };
        let digest = risk_confirmation_digest(skill_content);
        if entry.source_key != source_key || entry.digest != digest {
            self.pending.remove(token);
            return RiskAcceptanceConsumption::Rejected("confirmation_target_changed");
        }
        self.pending.remove(token);
        RiskAcceptanceConsumption::Accepted
    }
}

// ── Agent loop guard (agent-loop-guard-ext.ts) ───────────────────────────────

pub const LOOP_GUARD_WARN_POINTS: [usize; 2] = [3, 5];
pub const LOOP_GUARD_BLOCK_THRESHOLD: usize = 7;
pub const LOOP_GUARD_FAILURE_BLOCK_THRESHOLD: usize = 5;

/// Deterministic stable serialization of a tool input for signature purposes
/// (agent-loop-guard-ext.ts:19-39): objects with sorted keys, arrays in order,
/// scalar JSON forms.
pub fn stable_serialize(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => Value::String(s.clone()).to_string(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(stable_serialize).collect();
            format!("[{}]", inner.join(","))
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let inner: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", Value::String(k.clone()), stable_serialize(&map[k])))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
    }
}

fn read_signature(input: &Value) -> String {
    let obj = input.as_object();
    let pick = |key: &str| obj.and_then(|o| o.get(key)).cloned().unwrap_or(Value::Null);
    stable_serialize(&serde_json::json!({
        "path": pick("path"),
        "offset": pick("offset"),
        "limit": pick("limit"),
    }))
}

fn repetition_warning(kind: &str, count: usize) -> String {
    let subject = if kind == "read" {
        "the same file range"
    } else {
        "this exact tool call"
    };
    let action = if count >= 5 {
        "Stop repeating it and change the approach or arguments now; continued repetition will be blocked at the seventh call."
    } else {
        "Inspect the result and change the approach or arguments before retrying."
    };
    format!("⚠ Agent loop guard: {subject} has been requested {count} consecutive times. {action}")
}

pub fn failure_warning(tool_name: &str, count: usize) -> String {
    let action = if count >= 5 {
        "The next call to this tool will be blocked until another tool succeeds."
    } else {
        "Inspect the error and change the approach before retrying."
    };
    format!(
        "⚠ Agent loop guard: tool \"{tool_name}\" has failed {count} consecutive times. {action}"
    )
}

#[derive(Debug, Clone, PartialEq)]
pub enum GuardDecision {
    Allow,
    /// Prepend these warnings to the tool RESULT (tool_call side-channel).
    Warn(Vec<String>),
    /// Block the call before execution with this reason.
    Block(String),
}

#[derive(Debug, Default)]
pub struct AgentLoopGuard {
    repeated_signature: Option<String>,
    repeated_count: usize,
    reread_signature: Option<String>,
    reread_count: usize,
    pending_warnings: std::collections::HashMap<String, Vec<String>>,
    failed_tool: Option<String>,
    failed_count: usize,
}

impl AgentLoopGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// session_start / agent_start / session_shutdown (ext :105-116): the
    /// user's next question starts with fresh retry budgets.
    pub fn reset(&mut self) {
        self.repeated_signature = None;
        self.repeated_count = 0;
        self.reread_signature = None;
        self.reread_count = 0;
        self.pending_warnings.clear();
        self.failed_tool = None;
        self.failed_count = 0;
    }

    pub fn on_tool_call(
        &mut self,
        tool_name: &str,
        tool_call_id: &str,
        input: &Value,
    ) -> GuardDecision {
        if self.failed_tool.as_deref() == Some(tool_name)
            && self.failed_count >= LOOP_GUARD_FAILURE_BLOCK_THRESHOLD
        {
            return GuardDecision::Block(format!(
                "Agent loop guard blocked tool \"{tool_name}\" after {} consecutive failures. Use a different tool or approach before retrying.",
                self.failed_count
            ));
        }
        let (count, kind) = if tool_name == "read" {
            self.repeated_signature = None;
            self.repeated_count = 0;
            let signature = read_signature(input);
            if self.reread_signature.as_deref() == Some(signature.as_str()) {
                self.reread_count += 1;
            } else {
                self.reread_signature = Some(signature);
                self.reread_count = 1;
            }
            (self.reread_count, "read")
        } else {
            self.reread_signature = None;
            self.reread_count = 0;
            let signature = format!("{tool_name}\u{0}{}", stable_serialize(input));
            if self.repeated_signature.as_deref() == Some(signature.as_str()) {
                self.repeated_count += 1;
            } else {
                self.repeated_signature = Some(signature);
                self.repeated_count = 1;
            }
            (self.repeated_count, "tool")
        };
        if count >= LOOP_GUARD_BLOCK_THRESHOLD {
            return GuardDecision::Block(if kind == "read" {
                "Agent loop guard blocked the seventh consecutive read of the same file range. Change path, offset, or limit before retrying.".to_string()
            } else {
                format!(
                    "Agent loop guard blocked the seventh consecutive identical call to tool \"{tool_name}\". Change the approach or arguments before retrying."
                )
            });
        }
        if LOOP_GUARD_WARN_POINTS.contains(&count) && !tool_call_id.is_empty() {
            self.pending_warnings.insert(
                tool_call_id.to_string(),
                vec![repetition_warning(kind, count)],
            );
        }
        GuardDecision::Allow
    }

    /// tool_result (ext :164-205): failure streak tracking + queued warnings.
    /// Injection-scan warnings are layered by the service-side hook pipeline
    /// (regex lives in lingxi-service; see hooks.rs).
    pub fn on_tool_result(
        &mut self,
        tool_name: &str,
        tool_call_id: &str,
        is_error: bool,
    ) -> Vec<String> {
        let mut warnings = self
            .pending_warnings
            .remove(tool_call_id)
            .unwrap_or_default();
        if is_error {
            if self.failed_tool.as_deref() == Some(tool_name) {
                self.failed_count += 1;
            } else {
                self.failed_tool = Some(tool_name.to_string());
                self.failed_count = 1;
            }
            if self.failed_count == 2 || self.failed_count == 5 {
                warnings.push(failure_warning(tool_name, self.failed_count));
            }
        } else {
            self.failed_tool = None;
            self.failed_count = 0;
        }
        warnings
    }
}

// ── L1 tool_result truncation (compaction-utils.ts:99-131) ──────────────────

pub const DEFAULT_MAX_TOOL_RESULT_BYTES: usize = 32_768;

fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn floor_char_boundary(buf: &[u8], mut i: usize) -> usize {
    while i > 0 && (buf[i] & 0xC0) == 0x80 {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(buf: &[u8], mut i: usize) -> usize {
    while i < buf.len() && (buf[i] & 0xC0) == 0x80 {
        i += 1;
    }
    i
}

/// Head/tail truncation keeping both ends (compaction-utils.ts:99-119):
/// head = floor(maxBytes*0.4), tail = floor(maxBytes*0.4), UTF-8 safe.
pub fn truncate_text_head_tail(text: &str, max_bytes: usize) -> (String, bool, usize) {
    let original_bytes = text.len();
    if original_bytes <= max_bytes {
        return (text.to_string(), false, original_bytes);
    }
    let head_bytes = max_bytes * 4 / 10;
    let tail_bytes = max_bytes * 4 / 10;
    let buf = text.as_bytes();
    let head_end = floor_char_boundary(buf, head_bytes.min(buf.len()));
    let tail_start = ceil_char_boundary(buf, buf.len().saturating_sub(tail_bytes));
    let head = &text[..head_end];
    let tail = &text[tail_start..];
    let omitted = original_bytes - head.len() - tail.len();
    let marker = format!(
        "\n\n[... {} 已省略 (原始长度 {}) ...]\n\n",
        format_bytes(omitted),
        format_bytes(original_bytes)
    );
    (format!("{head}{marker}{tail}"), true, original_bytes)
}

// ── Compiled memory normalization & assembly ────────────────────────────────

/// Strip think blocks (compiled-memory-state.ts:60-66): remove paired
/// `<think>…</think>` / `<thinking>…</thinking>` blocks, drop a dangling
/// leading block, and remove stray closing tags.
pub fn strip_think_tag_blocks(value: &str) -> String {
    let lower = value.to_lowercase();
    let mut out = String::new();
    let mut i = 0usize;
    let mut dropped_block = false;
    while i < value.len() {
        let open_think = lower[i..].find("<think>");
        let open_thinking = lower[i..].find("<thinking>");
        let (open_pos, open_tag_len) = match (open_think, open_thinking) {
            (Some(a), Some(b)) => {
                if a <= b {
                    (i + a, "<think>".len())
                } else {
                    (i + b, "<thinking>".len())
                }
            }
            (Some(a), None) => (i + a, "<think>".len()),
            (None, Some(b)) => (i + b, "<thinking>".len()),
            (None, None) => {
                out.push_str(&value[i..]);
                break;
            }
        };
        out.push_str(&value[i..open_pos]);
        let after_open = open_pos + open_tag_len;
        let close_think = lower[after_open..].find("</think>");
        let close_thinking = lower[after_open..].find("</thinking>");
        match (close_think, close_thinking) {
            (Some(a), Some(b)) => {
                let (rel, tag_len) = if a + "<think>".len() <= b + "<thinking>".len() {
                    // pick the earlier closing tag position
                    if a <= b {
                        (a, "</think>".len())
                    } else {
                        (b, "</thinking>".len())
                    }
                } else if a <= b {
                    (a, "</think>".len())
                } else {
                    (b, "</thinking>".len())
                };
                i = after_open + rel + tag_len;
                dropped_block = true;
            }
            (Some(a), None) => {
                i = after_open + a + "</think>".len();
                dropped_block = true;
            }
            (None, Some(b)) => {
                i = after_open + b + "</thinking>".len();
                dropped_block = true;
            }
            (None, None) => {
                // Unterminated block: drop the tail entirely (:62).
                i = value.len();
                dropped_block = true;
            }
        }
    }
    let _ = dropped_block;
    out
}

fn parse_string_array(raw: &str) -> Option<Vec<String>> {
    let parsed: Value = serde_json::from_str(raw).ok()?;
    let arr = parsed.as_array()?;
    let mut out = Vec::new();
    for item in arr {
        let s = item.as_str()?.trim();
        if !s.is_empty() {
            out.push(s.to_string());
        }
    }
    Some(out)
}

/// Section body normalization (compiled-memory-state.ts:46-58): strip think
/// blocks, JSON string-array → "- item" list, drop heading lines, collapse
/// 3+ newlines, trim.
pub fn normalize_compiled_section_body(value: &str) -> String {
    let raw = strip_think_tag_blocks(value).trim().to_string();
    if raw.is_empty() {
        return String::new();
    }
    let text = parse_string_array(&raw)
        .map(|items| {
            items
                .iter()
                .map(|i| format!("- {i}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or(raw);
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| {
            // Mirrors /^#{1,6}\s+\S/: 1..6 leading '#' then whitespace then non-space.
            let t = line.trim();
            let hashes = t.chars().take_while(|c| *c == '#').count();
            if !(1..=6).contains(&hashes) {
                return true;
            }
            let rest = &t[hashes..];
            !(rest.starts_with(' ') && rest.len() > 1)
        })
        .collect();
    let joined = lines.join("\n");
    collapse_blank_runs(&joined).trim().to_string()
}

fn collapse_blank_runs(text: &str) -> String {
    let mut out = String::new();
    let mut blanks = 0usize;
    for line in text.lines() {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks >= 3 {
                continue;
            }
        } else {
            blanks = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim_end_matches('\n').to_string()
}

/// Week section normalization keeps `## {date}` headings
/// (compile.ts:528-566 normalizeCompiledWeekSectionBody subset: strip think
/// blocks, collapse blank runs, keep date headings).
pub fn normalize_compiled_week_section_body(value: &str) -> String {
    let raw = strip_think_tag_blocks(value).trim().to_string();
    if raw.is_empty() {
        return String::new();
    }
    collapse_blank_runs(&raw).trim().to_string()
}

/// memory.md assembly (compile.ts:1066-1091): four headings always present,
/// empty slots get the placeholder, navigation section appended only if
/// non-empty.
pub fn build_compiled_memory_markdown(
    facts: &str,
    today: &str,
    week: &str,
    longterm: &str,
    navigation: &str,
    is_zh: bool,
) -> String {
    let empty = if is_zh { "（暂无）" } else { "(none)" };
    let section = |title: &str, content: &str| {
        format!("## {title}\n\n{}", {
            let normalized = normalize_compiled_section_body(content);
            if normalized.is_empty() {
                empty.to_string()
            } else {
                normalized
            }
        })
    };
    let week_section = |title: &str, content: &str| {
        format!("## {title}\n\n{}", {
            let normalized = normalize_compiled_week_section_body(content);
            if normalized.is_empty() {
                empty.to_string()
            } else {
                normalized
            }
        })
    };
    let mut sections = vec![
        section(if is_zh { "重要事实" } else { "Key facts" }, facts),
        section(if is_zh { "今天" } else { "Today" }, today),
        week_section(
            if is_zh {
                "本周早些时候"
            } else {
                "Earlier this week"
            },
            week,
        ),
        section(
            if is_zh {
                "长期情况"
            } else {
                "Long-term context"
            },
            longterm,
        ),
    ];
    let nav_body = normalize_compiled_section_body(navigation);
    if !nav_body.is_empty() {
        sections.push(format!(
            "## {}\n\n{nav_body}",
            if is_zh {
                "记忆检索导航"
            } else {
                "Memory retrieval navigation"
            }
        ));
    }
    format!("{}\n", sections.join("\n\n"))
}

pub const DAILY_WINDOW_RETENTION_DAYS: usize = 6;
pub const WEEK_ASSEMBLY_MAX_CHARS: usize = 1200;

/// Week assembly from daily blocks (compile.ts:586-611): keep at most the last
/// `max_days` blocks; drop oldest until the join fits `max_chars`; a single
/// oversized block keeps its head.
pub fn assemble_week_blocks(blocks: &[String], max_days: usize, max_chars: usize) -> String {
    let kept: Vec<&str> = blocks
        .iter()
        .rev()
        .take(max_days)
        .map(|b| b.trim())
        .filter(|b| !b.is_empty())
        .collect::<Vec<_>>();
    let mut kept: Vec<&str> = kept.into_iter().rev().collect();
    let joined_len = |parts: &[&str]| parts.join("\n\n").chars().count();
    let mut content = kept.join("\n\n");
    if joined_len(&kept) > max_chars {
        while kept.len() > 1 && joined_len(&kept) > max_chars {
            kept.remove(0);
        }
        content = kept.join("\n\n");
        if joined_len(&kept) > max_chars {
            content = content.chars().take(max_chars).collect();
        }
    }
    content
}

pub const NAVIGATION_MAX_SESSIONS: usize = 5;
pub const NAVIGATION_MAX_TAGS: usize = 10;
pub const NAVIGATION_TITLE_MAX_CHARS: usize = 30;
pub const NAVIGATION_MAX_CHARS: usize = 500;

/// Navigation section body (navigation.ts:42-76) — pure builder.
pub fn build_navigation_section(
    recent_sessions: &[(Option<String>, Option<String>)],
    tag_counts: &[(String, usize)],
    is_zh: bool,
) -> String {
    let sessions: Vec<(String, String)> = recent_sessions
        .iter()
        .take(NAVIGATION_MAX_SESSIONS)
        .map(|(title, id)| {
            (
                title.clone().unwrap_or_default().trim().to_string(),
                id.clone().unwrap_or_default().trim().to_string(),
            )
        })
        .filter(|(title, id)| !title.is_empty() || !id.is_empty())
        .collect();
    let tags: Vec<(String, usize)> = tag_counts
        .iter()
        .take(NAVIGATION_MAX_TAGS)
        .filter(|(tag, _)| !tag.trim().is_empty())
        .cloned()
        .collect();
    if sessions.is_empty() && tags.is_empty() {
        return String::new();
    }
    let mut lines: Vec<String> = Vec::new();
    if !sessions.is_empty() {
        lines.push(if is_zh {
            "最近会话（用 session 工具按 sessionId 可读取详情）：".to_string()
        } else {
            "Recent sessions (use the session tool with sessionId to read details):".to_string()
        });
        for (title, id) in sessions {
            let title_chars: Vec<char> = title.chars().take(NAVIGATION_TITLE_MAX_CHARS).collect();
            let title: String = if title.is_empty() {
                if is_zh {
                    "（无标题）".to_string()
                } else {
                    "(untitled)".to_string()
                }
            } else if title.chars().count() > NAVIGATION_TITLE_MAX_CHARS {
                title_chars.into_iter().collect()
            } else {
                title
            };
            if id.chars().count() >= 8 {
                let short: String = id.chars().take(8).collect();
                lines.push(if is_zh {
                    format!("- {title}（{short}）")
                } else {
                    format!("- {title} ({short})")
                });
            } else {
                lines.push(format!("- {title}"));
            }
        }
    }
    if !tags.is_empty() {
        lines.push(String::new());
        lines.push(if is_zh {
            "事实库标签（search_memory 可按 tags 过滤）：".to_string()
        } else {
            "Fact-store tags (search_memory accepts tags filtering):".to_string()
        });
        let joined = tags
            .iter()
            .map(|(tag, count)| format!("{tag}({count})"))
            .collect::<Vec<_>>()
            .join(if is_zh { "、" } else { ", " });
        lines.push(joined);
    }
    lines.push(String::new());
    lines.push(
        if is_zh {
            "以上未注入的记忆可用工具检索：search_memory 查事实库，session read 读历史会话。".to_string()
        } else {
            "Memories not injected above can be retrieved via tools: search_memory for the fact store, session read for past sessions.".to_string()
        },
    );
    let mut body = lines.join("\n");
    if body.chars().count() > NAVIGATION_MAX_CHARS {
        let mut chars: Vec<char> = body.chars().take(NAVIGATION_MAX_CHARS).collect();
        // trim_end then ellipsis
        while matches!(chars.last(), Some(' ') | Some('\n')) {
            chars.pop();
        }
        body = format!(
            "{}{}",
            chars.into_iter().collect::<String>(),
            if is_zh { "…" } else { "..." }
        );
    }
    body
}

// ── Experience index (experience.ts:29-97 subset, pure parts) ───────────────

pub const EXPERIENCE_DESCRIPTION_SNIPPET_CHARS: usize = 20;
pub const EXPERIENCE_DESCRIPTION_MAX_CHARS: usize = 120;

/// Index block for one category document (experience.ts:39-57).
pub fn build_experience_index_block(
    title: &str,
    entries: &[String],
    file_name: &str,
) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let snippets: Vec<String> = entries
        .iter()
        .map(|e| {
            let chars: Vec<char> = e.chars().collect();
            if chars.len() > EXPERIENCE_DESCRIPTION_SNIPPET_CHARS {
                format!(
                    "{}…",
                    chars[..EXPERIENCE_DESCRIPTION_SNIPPET_CHARS]
                        .iter()
                        .collect::<String>()
                )
            } else {
                e.clone()
            }
        })
        .collect();
    let mut desc = snippets.join("; ");
    if desc.chars().count() > EXPERIENCE_DESCRIPTION_MAX_CHARS {
        let mut chars: Vec<char> = desc
            .chars()
            .take(EXPERIENCE_DESCRIPTION_MAX_CHARS - 3)
            .collect();
        chars.push('…');
        desc = chars.into_iter().collect();
    }
    Some(format!(
        "# {title}（{} 条）\n{desc}\n→ experience/{file_name}",
        entries.len(),
    ))
}

/// Next numbered entry line for a category body (experience.ts:84-91).
pub fn experience_next_entry(existing: &str, content: &str) -> Option<String> {
    let next_num = existing
        .lines()
        .filter(|l| {
            let t = l.trim();
            let mut chars = t.chars();
            chars.next().map(|c| c.is_ascii_digit()).unwrap_or(false) && t.contains(". ")
        })
        .count()
        + 1;
    let content = content.trim();
    if content.is_empty() {
        return None;
    }
    Some(format!("{next_num}. {content}"))
}

/// Category title validation (experience.ts normalizeExperienceCategory):
/// any non-empty title without path-escape shapes; NOT a fixed list.
pub fn normalize_experience_category(category: &str) -> Option<String> {
    let title = category.trim();
    if title.is_empty() {
        return None;
    }
    if title.contains(['\0', '\r', '\n']) {
        return None;
    }
    if title == "." || title == ".." {
        return None;
    }
    if title.contains('/') || title.contains('\\') || title.contains("..") {
        return None;
    }
    // ^[A-Za-z]: (windows drive)
    let bytes = title.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return None;
    }
    Some(title.to_string())
}

/// Storage file stem (experience.ts buildExperienceStorageFileName): lowercase
/// letter/number runs joined by '-', ≤48 chars, then a 10-char sha256 suffix
/// of the title. NFKC is unavailable in this locked dependency set — the hash
/// suffix keeps distinct titles collision-free (registered diff).
pub fn experience_storage_stem(title: &str) -> String {
    let mut stem = String::new();
    for c in title.chars() {
        if c.is_alphanumeric() {
            for l in c.to_lowercase() {
                stem.push(l);
            }
        } else if !stem.ends_with('-') {
            stem.push('-');
        }
    }
    let trimmed = stem.trim_matches('-');
    let mut sliced: String = trimmed.chars().take(48).collect();
    if sliced.is_empty() {
        sliced = "category".to_string();
    }
    let hash = sha256_hex(title.as_bytes());
    let hash10: String = hash.chars().take(10).collect();
    format!("{sliced}-{hash10}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── fact search text (fact-store.ts golden) ──
    #[test]
    fn fact_search_text_matches_incumbent_shape() {
        // CJK run of 4 → 2-grams×3 + 3-grams×2 = 5 grams; latin words kept whole.
        let text = build_fact_search_text("用户喜欢 Rust 语言", &["偏好".to_string()]);
        assert!(text.contains("用户喜欢 Rust 语言"));
        assert!(text.contains("用户"));
        assert!(text.contains("户喜"));
        assert!(text.contains("喜欢"));
        assert!(text.contains("语言"));
        assert!(text.contains("偏好"));
        // Unique tokens: the 2-gram of a 2-char run appears once.
        let two = build_fact_search_text("记忆", &[]);
        assert_eq!(two.split_whitespace().filter(|t| *t == "记忆").count(), 1);
    }

    #[test]
    fn fts_query_quotes_terms_with_or() {
        let q = build_fts_query("记住 Rust");
        assert!(q.starts_with('"'));
        assert!(q.contains("\" OR \""));
        assert!(q.contains("记住"));
        assert_eq!(build_fts_query("   "), "");
        // inner quotes doubled
        let q2 = build_fts_query("a\"b");
        assert!(q2.contains("\"a\"\"b\""));
    }

    #[test]
    fn zero_result_terms_are_unique_and_capped() {
        let terms = zero_result_diagnostic_terms(
            "alpha beta alpha gamma delta epsilon zeta",
            ZERO_RESULT_DIAGNOSTIC_MAX_TERMS,
        );
        assert_eq!(terms.len(), ZERO_RESULT_DIAGNOSTIC_MAX_TERMS);
        assert_eq!(terms[0], "alpha");
    }

    // ── tenets ──
    fn tenet_json(id: &str, content: &str, status: &str, source: &str, created: &str) -> Value {
        json!({"id": id, "content": content, "status": status, "source": source, "createdAt": created})
    }

    #[test]
    fn tenets_strict_read_rejects_corrupt_and_future_schema() {
        let ok = json!({"schemaVersion": 1, "tenets": [tenet_json("a", "内容", "active", "user_direct", "2026-01-01T00:00:00Z")]});
        assert!(validate_tenets_strict(&ok).is_ok());
        let future = json!({"schemaVersion": 2, "tenets": []});
        assert_eq!(
            validate_tenets_strict(&future),
            Err(TenetErrorCode::StoreUnsupportedSchema)
        );
        let corrupt = json!({"schemaVersion": 1, "tenets": "nope"});
        assert_eq!(
            validate_tenets_strict(&corrupt),
            Err(TenetErrorCode::StoreCorrupted)
        );
        let dup_ids = json!({"schemaVersion": 1, "tenets": [
            tenet_json("a", "one", "active", "user_direct", "2026-01-01T00:00:00Z"),
            tenet_json("a", "two", "pending", "model_proposed", "2026-01-02T00:00:00Z")]});
        assert_eq!(
            validate_tenets_strict(&dup_ids),
            Err(TenetErrorCode::StoreCorrupted)
        );
        let top_level = json!("nope");
        assert_eq!(
            validate_tenets_strict(&top_level),
            Err(TenetErrorCode::StoreCorrupted)
        );
    }

    #[test]
    fn tenet_quota_and_dedup_semantics_match_incumbent() {
        let mut file = TenetsFile::empty();
        // direct add → active
        let (t1, dup) = tenets_add_direct(
            &mut file,
            &NewTenetInput {
                content: "永远用中文回复",
                priority: Some("high"),
                session_id: None,
                new_id: "id-1".into(),
                now_iso: "2026-01-01T00:00:00Z".into(),
            },
        )
        .unwrap();
        assert!(!dup);
        assert_eq!(t1.status, TenetStatus::Active);
        assert_eq!(t1.source, TenetSource::UserDirect);
        // normalized duplicate (punctuation-insensitive, case-insensitive)
        let (_, dup) = tenets_add_direct(
            &mut file,
            &NewTenetInput {
                content: "永远用中文回复。",
                priority: None,
                session_id: None,
                new_id: "id-2".into(),
                now_iso: "2026-01-02T00:00:00Z".into(),
            },
        )
        .unwrap();
        assert!(dup);
        // proposal of same content (any-status duplicate) → duplicate, no new row
        let (_, dup, existing) = tenets_add_proposal(
            &mut file,
            &NewTenetInput {
                content: "永远用中文回复",
                priority: None,
                session_id: Some("s1"),
                new_id: "id-3".into(),
                now_iso: "2026-01-03T00:00:00Z".into(),
            },
        )
        .unwrap();
        assert!(dup);
        assert_eq!(existing, Some(TenetStatus::Active));
        // proposal of new content → pending
        let (t4, dup, _) = tenets_add_proposal(
            &mut file,
            &NewTenetInput {
                content: "提案内容",
                priority: None,
                session_id: None,
                new_id: "id-4".into(),
                now_iso: "2026-01-04T00:00:00Z".into(),
            },
        )
        .unwrap();
        assert!(!dup);
        assert_eq!(t4.status, TenetStatus::Pending);
        // decide approve → active; second decide → Invalid
        let decided = tenets_decide(&mut file, "id-4", true, "2026-01-05T00:00:00Z").unwrap();
        assert_eq!(decided.status, TenetStatus::Active);
        assert_eq!(
            tenets_decide(&mut file, "id-4", false, "x"),
            Err(TenetErrorCode::Invalid)
        );
        assert_eq!(
            tenets_decide(&mut file, "nope", true, "x"),
            Err(TenetErrorCode::NotFound)
        );
        // empty content invalid; overlong content invalid
        assert_eq!(
            tenets_add_direct(
                &mut file,
                &NewTenetInput {
                    content: "   ",
                    priority: None,
                    session_id: None,
                    new_id: "x".into(),
                    now_iso: "x".into(),
                }
            ),
            Err(TenetErrorCode::Invalid)
        );
        let long = "长".repeat(MAX_TENET_CONTENT_CHARS + 1);
        assert_eq!(
            tenets_add_direct(
                &mut file,
                &NewTenetInput {
                    content: &long,
                    priority: None,
                    session_id: None,
                    new_id: "x".into(),
                    now_iso: "x".into(),
                }
            ),
            Err(TenetErrorCode::Invalid)
        );
        // remove
        assert!(tenets_remove(&mut file, "id-4"));
        assert!(!tenets_remove(&mut file, "id-4"));
    }

    #[test]
    fn tenets_pending_cap_is_loud() {
        let mut file = TenetsFile::empty();
        for i in 0..MAX_PENDING_TENETS {
            tenets_add_proposal(
                &mut file,
                &NewTenetInput {
                    content: &format!("提案{i}"),
                    priority: None,
                    session_id: None,
                    new_id: format!("p{i}"),
                    now_iso: "2026-01-01T00:00:00Z".into(),
                },
            )
            .unwrap();
        }
        assert_eq!(
            tenets_add_proposal(
                &mut file,
                &NewTenetInput {
                    content: "又一条",
                    priority: None,
                    session_id: None,
                    new_id: "over".into(),
                    now_iso: "x".into(),
                }
            ),
            Err(TenetErrorCode::PendingFull)
        );
    }

    #[test]
    fn tenets_sort_is_priority_then_created() {
        let mut file = TenetsFile::empty();
        for (content, priority, created) in [
            ("低优先", "low", "2026-01-03T00:00:00Z"),
            ("关键", "critical", "2026-01-02T00:00:00Z"),
            ("中", "medium", "2026-01-01T00:00:00Z"),
        ] {
            file.tenets.push(Tenet {
                id: content.into(),
                content: content.into(),
                priority: priority.into(),
                status: TenetStatus::Active,
                source: TenetSource::UserDirect,
                session_id: None,
                created_at: created.into(),
                decided_at: None,
            });
        }
        sort_tenets(&mut file.tenets);
        let order: Vec<&str> = file.tenets.iter().map(|t| t.content.as_str()).collect();
        assert_eq!(order, vec!["关键", "中", "低优先"]);
    }

    #[test]
    fn pinned_full_replace_keeps_still_listed_model_proposals() {
        let mut file = TenetsFile::empty();
        tenets_add_proposal(
            &mut file,
            &NewTenetInput {
                content: "模型原则",
                priority: None,
                session_id: None,
                new_id: "m1".into(),
                now_iso: "2026-01-01T00:00:00Z".into(),
            },
        )
        .unwrap();
        tenets_decide(&mut file, "m1", true, "2026-01-01T00:00:00Z").unwrap();
        tenets_add_direct(
            &mut file,
            &NewTenetInput {
                content: "用户钉的旧条目",
                priority: None,
                session_id: None,
                new_id: "u1".into(),
                now_iso: "2026-01-01T00:00:00Z".into(),
            },
        )
        .unwrap();
        let mut counter = 0;
        let created = tenets_replace_pinned(
            &mut file,
            &["模型原则".to_string(), "新钉".to_string()],
            &mut || {
                counter += 1;
                format!("n{counter}")
            },
            "2026-01-02T00:00:00Z",
        )
        .unwrap();
        // model proposal retained (still pinned), old user_direct removed, new one created
        let contents: Vec<&str> = file.tenets.iter().map(|t| t.content.as_str()).collect();
        assert!(contents.contains(&"模型原则"));
        assert!(!contents.contains(&"用户钉的旧条目"));
        assert!(contents.contains(&"新钉"));
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].source, TenetSource::UserDirect);
    }

    // ── skills ──
    #[test]
    fn skill_name_sanitization_matches_incumbent_regex() {
        assert_eq!(
            sanitize_skill_name("ok-name_1"),
            Some("ok-name_1".to_string())
        );
        assert_eq!(
            sanitize_skill_name("  spaced  "),
            Some("spaced".to_string())
        );
        assert_eq!(sanitize_skill_name("-leading"), None);
        assert_eq!(sanitize_skill_name("has space"), None);
        assert_eq!(sanitize_skill_name(""), None);
        let long = format!("a{}", "b".repeat(64));
        assert_eq!(sanitize_skill_name(&long), None);
        let ok_len = format!("a{}", "b".repeat(63));
        assert_eq!(sanitize_skill_name(&ok_len), Some(ok_len.clone()));
        assert_eq!(sanitize_skill_name("中文"), None);
    }

    #[test]
    fn skill_metadata_frontmatter_golden() {
        let md = "---\nname: my-skill\ndescription: \"Does things,  carefully\"\ndefault-enabled: false\ndisable-model-invocation: true\n---\n\nBody here.\n";
        let meta = parse_skill_metadata(md, "fallback");
        assert_eq!(meta.name.as_deref(), Some("my-skill"));
        assert_eq!(meta.description, "Does things, carefully");
        assert!(!meta.default_enabled);
        assert!(meta.disable_model_invocation);
        // metadata block also accepted (skill-metadata.ts frontmatterMetadata)
        let md2 = "---\nmetadata:\n  defaultEnabled: false\n  name: named\n---\nbody";
        let meta2 = parse_skill_metadata(md2, "fb");
        assert_eq!(meta2.name.as_deref(), Some("named"));
        assert!(!meta2.default_enabled);
        // no frontmatter → fallback + defaults
        let meta3 = parse_skill_metadata("no frontmatter", "fb");
        assert_eq!(meta3.name.as_deref(), Some("fb"));
        assert!(meta3.default_enabled);
        assert!(!meta3.disable_model_invocation);
    }

    #[test]
    fn workspace_candidate_states_shadow_and_policy() {
        let mut claimed = std::collections::HashMap::new();
        claimed.insert("shared".to_string(), "user-pool".to_string());
        let disabled = ["optout".to_string()]
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        let candidates = vec![
            WorkspaceSkillCandidate {
                name: "shared".into(),
                source_category: "standard".into(),
            },
            WorkspaceSkillCandidate {
                name: "optout".into(),
                source_category: "standard".into(),
            },
            WorkspaceSkillCandidate {
                name: "plain".into(),
                source_category: "standard".into(),
            },
            WorkspaceSkillCandidate {
                name: "compat".into(),
                source_category: "compatible".into(),
            },
        ];
        let policy = DEFAULT_WORKSPACE_SKILL_POLICY;
        let states =
            resolve_workspace_skill_candidate_states(&candidates, &policy, &mut claimed, &disabled);
        assert_eq!(states[0].inactive_reason, Some("shadowed"));
        assert_eq!(states[0].shadowed_by.as_deref(), Some("user-pool"));
        assert_eq!(states[1].inactive_reason, Some("user-disabled"));
        assert!(states[2].active);
        assert_eq!(states[2].inactive_reason, None);
        assert_eq!(states[3].inactive_reason, Some("policy-disabled"));
        // compatible policy on
        let policy_on = WorkspaceSkillPolicy {
            discover_project_skills: true,
            discover_compatible_project_skills: true,
        };
        let states_on = resolve_workspace_skill_candidate_states(
            &candidates,
            &policy_on,
            &mut claimed,
            &disabled,
        );
        assert!(states_on[3].active);
    }

    // ── risk confirmation ──
    #[test]
    fn risk_token_protocol_is_one_shot_content_bound_and_ttl() {
        let mut store = RiskConfirmationStore::default();
        let content = "SKILL content v1";
        let token = store.create_token("path:/x", content, "suspicious: reason", "abc123", 1_000);
        assert!(token.starts_with("risk_"));
        // not accepted
        assert_eq!(
            store.consume_acceptance(false, Some(&token), "path:/x", content, 2_000),
            RiskAcceptanceConsumption::NotAccepted
        );
        // accepted without token
        assert_eq!(
            store.consume_acceptance(true, None, "path:/x", content, 2_000),
            RiskAcceptanceConsumption::Rejected("missing_confirmation_token")
        );
        // wrong content
        assert_eq!(
            store.consume_acceptance(true, Some(&token), "path:/x", "TAMPERED", 2_000),
            RiskAcceptanceConsumption::Rejected("confirmation_target_changed")
        );
        // same token cannot double-consume a failed attempt that deleted it (target-changed deletes)
        assert_eq!(
            store.consume_acceptance(true, Some(&token), "path:/x", content, 3_000),
            RiskAcceptanceConsumption::Rejected("invalid_or_expired_confirmation_token")
        );
        // fresh token, TTL expiry
        let t2 = store.create_token("path:/x", content, "r", "def456", 10_000);
        assert_eq!(
            store.consume_acceptance(
                true,
                Some(&t2),
                "path:/x",
                content,
                10_000 + RISK_CONFIRMATION_TTL_MS
            ),
            RiskAcceptanceConsumption::Rejected("invalid_or_expired_confirmation_token")
        );
        let t3 = store.create_token("path:/x", content, "r", "ghi789", 20_000);
        assert_eq!(
            store.consume_acceptance(true, Some(&t3), "path:/x", content, 20_000),
            RiskAcceptanceConsumption::Accepted
        );
        // one-shot: second use fails
        assert_eq!(
            store.consume_acceptance(true, Some(&t3), "path:/x", content, 20_001),
            RiskAcceptanceConsumption::Rejected("invalid_or_expired_confirmation_token")
        );
        // digest is sha256 hex
        assert_eq!(risk_confirmation_digest("x").len(), 64);
    }

    // ── loop guard ──
    #[test]
    fn loop_guard_blocks_seventh_repeated_call_and_warns_at_3_5() {
        let mut guard = AgentLoopGuard::new();
        let input = json!({"path": "/a.txt"});
        for i in 1..=6 {
            match guard.on_tool_call("grep", &format!("tc{i}"), &input) {
                GuardDecision::Allow => {}
                GuardDecision::Warn(_) => {}
                GuardDecision::Block(_) => panic!("blocked early at {i}"),
            }
        }
        assert!(matches!(
            guard.on_tool_call("grep", "tc7", &input),
            GuardDecision::Block(_)
        ));
        // warns at 3 and 5 are queued for the matching tool_call_id
        let mut guard2 = AgentLoopGuard::new();
        for i in 1..=3 {
            let _ = guard2.on_tool_call("grep", &format!("w{i}"), &input);
        }
        let warnings = guard2.on_tool_result("grep", "w3", false);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("3 consecutive times"));
        // different args reset the streak
        let mut guard3 = AgentLoopGuard::new();
        for i in 1..=6 {
            let _ =
                guard3.on_tool_call("grep", &format!("x{i}"), &json!({"path": format!("/a{i}")}));
        }
        assert!(matches!(
            guard3.on_tool_call("grep", "x7", &json!({"path": "/b"})),
            GuardDecision::Allow
        ));
    }

    #[test]
    fn loop_guard_read_signature_ignores_other_fields_and_counts_failures() {
        let mut guard = AgentLoopGuard::new();
        let a = json!({"path": "/a", "limit": 5, "extra": "ignored"});
        let b = json!({"limit": 5, "path": "/a"});
        let _ = guard.on_tool_call("read", "r1", &a);
        // read input signature = {path,offset,limit} only
        let _ = guard.on_tool_call("read", "r2", &b);
        // failures: warn at 2, block eligibility at 5
        for i in 1..=2 {
            let _ = guard.on_tool_call("web_search", &format!("f{i}"), &json!({}));
            let warnings = guard.on_tool_result("web_search", &format!("f{i}"), true);
            if i == 2 {
                assert_eq!(warnings.len(), 1);
                assert!(warnings[0].contains("failed 2 consecutive times"));
            }
        }
        for i in 3..=5 {
            let _ = guard.on_tool_call("web_search", &format!("f{i}"), &json!({}));
            let _ = guard.on_tool_result("web_search", &format!("f{i}"), true);
        }
        // 6th failure warning at 5 already emitted; next call blocked
        assert!(matches!(
            guard.on_tool_call("web_search", "f6", &json!({})),
            GuardDecision::Block(_)
        ));
        // success resets the failure streak
        let _ = guard.on_tool_result("web_search", "f6", false);
        assert!(matches!(
            guard.on_tool_call("web_search", "f7", &json!({})),
            GuardDecision::Allow
        ));
        // agent_start resets everything
        guard.reset();
        assert!(matches!(
            guard.on_tool_call("web_search", "f8", &json!({})),
            GuardDecision::Allow
        ));
    }

    #[test]
    fn stable_serialization_sorts_object_keys() {
        assert_eq!(
            stable_serialize(&json!({"b": 1, "a": [1, "x"]})),
            r#"{"a":[1,"x"],"b":1}"#
        );
        assert_eq!(stable_serialize(&Value::Null), "null");
    }

    // ── L1 truncation ──
    #[test]
    fn truncation_head_tail_matches_incumbent_semantics() {
        let text = "x".repeat(100);
        let (out, truncated, original) = truncate_text_head_tail(&text, 50);
        assert!(truncated);
        assert_eq!(original, 100);
        // head 20 + tail 20 + marker; marker contains the CJK wording
        assert!(out.contains("已省略"));
        assert!(out.starts_with("xxxx"));
        assert!(out.ends_with("xxxx"));
        // under limit untouched
        let (same, truncated2, _) = truncate_text_head_tail("abc", 50);
        assert!(!truncated2);
        assert_eq!(same, "abc");
        // multibyte safety: cut points land on char boundaries
        let cjk = "记".repeat(2000);
        let (out2, truncated3, _) = truncate_text_head_tail(&cjk, 100);
        assert!(truncated3);
        assert!(out2.contains('记'));
    }

    // ── compiled memory ──
    #[test]
    fn section_normalization_strips_think_and_headings() {
        let raw = "<think>hidden</think>\n## Header\nfact one\n\n\n\nfact two\n";
        let out = normalize_compiled_section_body(raw);
        assert!(!out.contains("hidden"));
        assert!(!out.contains("Header"));
        assert!(out.contains("fact one"));
        assert!(out.contains("fact two"));
        // JSON string array → list
        let list = normalize_compiled_section_body("[\"a\",\"b\"]");
        assert_eq!(list, "- a\n- b");
        // dangling think block dropped
        let dangling = normalize_compiled_section_body("<think>never closed");
        assert_eq!(dangling, "");
    }

    #[test]
    fn compiled_markdown_golden_zh_and_en() {
        let zh =
            build_compiled_memory_markdown("事实", "", "## 2026-01-01\n日记", "长期", "", true);
        assert!(zh.starts_with("## 重要事实\n\n事实\n\n## 今天\n\n（暂无）\n\n## 本周早些时候\n\n## 2026-01-01\n日记\n\n## 长期情况\n\n长期\n"));
        assert!(!zh.contains("记忆检索导航"));
        let en = build_compiled_memory_markdown("facts", "today", "", "long", "nav line", false);
        assert!(en.contains("## Key facts\n\nfacts"));
        assert!(en.contains("## Today\n\ntoday"));
        assert!(en.contains("## Earlier this week\n\n(none)"));
        assert!(en.contains("## Memory retrieval navigation\n\nnav line"));
        assert!(en.ends_with('\n'));
    }

    #[test]
    fn week_assembly_drops_oldest_first_and_caps_single_block() {
        let blocks: Vec<String> = (0..8)
            .map(|i| format!("## 2026-01-0{i}\nbody{i}"))
            .collect();
        let out = assemble_week_blocks(&blocks, 6, 1200);
        assert!(out.contains("body7"));
        assert!(out.contains("body2"));
        assert!(!out.contains("body1"));
        assert!(!out.contains("body0"));
        // single oversized block: head kept
        let big = vec!["y".repeat(2000)];
        let out2 = assemble_week_blocks(&big, 6, 100);
        assert_eq!(out2.chars().count(), 100);
    }

    #[test]
    fn navigation_section_caps_and_formats() {
        let sessions: Vec<(Option<String>, Option<String>)> = (0..7)
            .map(|i| (Some(format!("标题{i}")), Some(format!("sess-{i:024}"))))
            .collect();
        let nav = build_navigation_section(&sessions, &[("工作".to_string(), 3)], true);
        assert!(nav.contains("最近会话"));
        assert!(nav.contains("sess-00（截断".split('（').next().unwrap_or("")));
        assert!(nav.chars().count() <= 500 + 1);
        let empty = build_navigation_section(&[], &[], true);
        assert_eq!(empty, "");
    }

    #[test]
    fn experience_index_block_and_entry_numbering() {
        let entries = vec!["先读文档再动手".to_string(), "第二条经验".to_string()];
        let block = build_experience_index_block("coding", &entries, "coding.md").unwrap();
        assert!(block.starts_with("# coding（2 条）"));
        assert!(block.contains("→ experience/coding.md"));
        assert_eq!(build_experience_index_block("empty", &[], "e.md"), None);
        let next = experience_next_entry("1. 旧的\n2. 另一条", "新经验").unwrap();
        assert_eq!(next, "3. 新经验");
        assert!(experience_next_entry("", "   ").is_none());
        assert_eq!(
            normalize_experience_category("Coding"),
            Some("Coding".to_string())
        );
        assert_eq!(
            normalize_experience_category("用户认可的做法"),
            Some("用户认可的做法".to_string())
        );
        assert_eq!(normalize_experience_category("../escape"), None);
        assert_eq!(normalize_experience_category("a/b"), None);
        assert_eq!(normalize_experience_category("C:/x"), None);
        assert_eq!(normalize_experience_category("  "), None);
        let stem = experience_storage_stem("用户认可的做法");
        assert!(stem.len() <= 64);
        assert_eq!(experience_storage_stem("a"), experience_storage_stem("a"));
    }
}

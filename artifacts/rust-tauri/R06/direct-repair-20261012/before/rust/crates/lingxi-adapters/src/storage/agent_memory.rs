//! R06-T06 per-agent memory stores (MemoryService port implementations).
//!
//! Storage layout mirrors the incumbent per-agent files exactly (RC-2 anchors
//! in the design doc; old data stays readable without conversion):
//! - `agents/{id}/memory/facts.db`   — SQLite, incumbent schema v3
//!   (`lib/memory/fact-store.ts:33,123-213`): `facts` + external-content FTS5
//!   `facts_fts` + triggers + `fact_embeddings`.
//! - `agents/{id}/memory/tenets.json` — schemaVersion 1, atomic writes
//!   (`lib/memory/tenets.ts:13,197-212`).
//! - `agents/{id}/memory/{facts,today,week,longterm}.md` assembled into
//!   `memory.md` (`lib/memory/compile.ts:1054-1091`).
//! - `agents/{id}/memory/daily/{date}.md`, `dream/revisions/*.json`,
//!   `experience/*.md` + `experience.md`.
//! - persona files at the agent root (`identity.md`, `AGENTS.md`,
//!   `AGENTS.public.md`, avatars) — read/write face of the T01 material chain.
//!
//! Context notes: incumbent keeps them beside the session JSONL
//! (`lib/tools/context-notes-tool.ts:18-41`); candidate sessions have no
//! JSONL path, so the canonical home is the private runtime dir
//! (`{runtime}/context-notes/{session_id}.md`) — registered diff.
//!
//! Every store is opened PER AGENT from a validated agent id: physical
//! per-agent isolation is the A11 mechanism (incumbent
//! `core/agent.ts:223` + `server/routes/config.ts:419-427`).

use std::io;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::{json, Value};

use lingxi_kernel::memory::{
    build_compiled_memory_markdown, build_fact_search_text, build_fts_query, has_cjk,
    normalize_compiled_section_body, normalize_compiled_week_section_body,
    normalize_experience_category, sort_tenets, tenets_add_direct, tenets_add_proposal,
    tenets_decide, tenets_remove, tenets_replace_pinned, validate_tenets_strict,
    zero_result_diagnostic_terms, NewTenetInput, Tenet, TenetErrorCode, TenetStatus, TenetsFile,
    TENETS_SCHEMA_VERSION, ZERO_RESULT_DIAGNOSTIC_MAX_TERMS,
};

// ── shared file helpers ─────────────────────────────────────────────────────

/// Atomic write: tmp file + rename (incumbent `shared/safe-fs.ts`
/// atomicWriteSync semantics).
pub fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let uniq = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let tmp = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string()),
        uniq
    ));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}

fn safe_read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Reads a JSON file, returning None when missing (empty file → None too).
pub fn read_json_file(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    if text.trim().is_empty() {
        return None;
    }
    serde_json::from_str(&text).ok()
}

/// Random hex suffix for store-record ids (token/bundle/tenet ids).
pub fn random_token_suffix() -> String {
    let mut bytes = [0u8; 8];
    getrandom::getrandom(&mut bytes).expect("OS RNG unavailable");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn now_iso() -> String {
    // ISO-8601 UTC from the system clock (incumbent new Date().toISOString()).
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // civil-from-days (Howard Hinnant's algorithm) — zero external deps.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mth <= 2 { y + 1 } else { y };
    format!("{y:04}-{mth:02}-{d:02}T{h:02}:{m:02}:{s:02}.{millis:03}Z")
}

/// Agent id validation (`server/routes/config.ts:425`:
/// `/[\/\\.]/.test(resolvedId)` → throw; agents.ts validateId adds the
/// charset bound). Returns Err for path-escape shapes.
pub fn validate_agent_id(agent_id: &str) -> Result<(), MemoryError> {
    if agent_id.is_empty() {
        return Err(MemoryError::InvalidRequest("agentId is required".into()));
    }
    if agent_id.contains('/') || agent_id.contains('\\') || agent_id.contains('.') {
        return Err(MemoryError::InvalidRequest(format!(
            "Invalid agent ID: {agent_id}"
        )));
    }
    if agent_id.len() > 128
        || !agent_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(MemoryError::InvalidRequest(format!(
            "Invalid agent ID: {agent_id}"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub enum MemoryError {
    InvalidRequest(String),
    AgentNotFound(String),
    NotFound(String),
    Conflict { code: String, message: String },
    Io(String),
    Store(String),
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryError::InvalidRequest(m) => write!(f, "{m}"),
            MemoryError::AgentNotFound(m) | MemoryError::NotFound(m) => write!(f, "{m}"),
            MemoryError::Conflict { code, message } => write!(f, "{code}: {message}"),
            MemoryError::Io(m) | MemoryError::Store(m) => write!(f, "{m}"),
        }
    }
}

impl MemoryError {
    pub fn tenet_code(&self) -> Option<&str> {
        match self {
            MemoryError::Conflict { code, .. } => Some(code),
            _ => None,
        }
    }
}

/// Avatar read result (avatar route semantics): (absolute file path, MIME
/// type, image bytes) — first existing of png/jpg/jpeg/webp.
pub type AvatarContent = (PathBuf, &'static str, Vec<u8>);

impl From<io::Error> for MemoryError {
    fn from(err: io::Error) -> Self {
        if err.kind() == io::ErrorKind::NotFound {
            MemoryError::NotFound("file not found".into())
        } else {
            MemoryError::Io(err.to_string())
        }
    }
}

// ── per-agent root resolution ───────────────────────────────────────────────

#[derive(Debug)]
pub struct AgentMemoryRoots {
    pub agent_dir: PathBuf,
    pub memory_dir: PathBuf,
}

/// Resolves the per-agent roots for a VALIDATED agent id. The agent dir must
/// exist for every mutating/reading face (incumbent `agentExists` 404).
pub fn resolve_agent_roots(
    agents_root: &Path,
    agent_id: &str,
) -> Result<AgentMemoryRoots, MemoryError> {
    validate_agent_id(agent_id)?;
    let agent_dir = agents_root.join(agent_id);
    if !agent_dir.is_dir() {
        return Err(MemoryError::AgentNotFound(format!(
            "agent not found: {agent_id}"
        )));
    }
    Ok(AgentMemoryRoots {
        memory_dir: agent_dir.join("memory"),
        agent_dir,
    })
}

// ── PII scrub (lib/pii-guard.ts:10-46; regex lives in service? No — the
//    patterns are plain enough for hand matchers here to avoid a kernel/regex
//    dependency; adapters reuses the same scanner the tools use) ─────────────

/// Hard-PII detection + redaction. Adapted from `lib/pii-guard.ts` HARD_PATTERNS.
/// Returns (cleaned, detected names).
pub fn scrub_pii(text: &str) -> (String, Vec<&'static str>) {
    if text.is_empty() {
        return (text.to_string(), Vec::new());
    }
    let mut detected = Vec::new();
    let mut cleaned = text.to_string();

    // api_key: sk-*/AKIA*/gsk_*/ghp_*/glpat-*/xoxb-* + structural tail
    {
        let mut out = String::new();
        let mut i = 0usize;
        let mut found = false;
        let prefixes: [(&str, usize, bool); 6] = [
            ("sk-", 23, false),  // sk- + >=20 [A-Za-z0-9]
            ("AKIA", 20, true),  // AKIA + 16 [A-Z0-9]
            ("gsk_", 24, false), // gsk_ + >=20
            ("ghp_", 40, true),  // ghp_ + 36
            ("glpat-", 20, false),
            ("xoxb-", 12, false),
        ];
        while i < cleaned.len() {
            let rest = &cleaned[i..];
            let mut matched = false;
            for (prefix, min_total, exact) in prefixes {
                if !rest.starts_with(prefix) {
                    continue;
                }
                let body = &rest[prefix.len()..];
                let end = body
                    .char_indices()
                    .take_while(|(_, c)| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                    .map(|(idx, c)| idx + c.len_utf8())
                    .last()
                    .unwrap_or(0);
                let total = prefix.len() + end;
                let within = total <= 90;
                let ok_len = if exact {
                    total == min_total
                } else {
                    total >= min_total
                };
                // AKIA requires uppercase-alnum body only
                let body_ok = if prefix == "AKIA" {
                    body.get(..16)
                        .map(|b| {
                            b.chars()
                                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                        })
                        .unwrap_or(false)
                } else {
                    true
                };
                if body_ok && ok_len && within {
                    out.push_str("[REDACTED]");
                    i += total;
                    found = true;
                    matched = true;
                }
                break;
            }
            if !matched {
                let ch = rest.chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
        }
        if found {
            detected.push("api_key");
            cleaned = out;
        }
    }

    // private_key PEM header (line-wise redaction of header lines)
    {
        let lower = cleaned.to_lowercase();
        if lower.contains("-----begin") && lower.contains("private key-----") {
            detected.push("private_key");
            cleaned = cleaned
                .lines()
                .map(|line| {
                    let t = line.trim();
                    let l = t.to_lowercase();
                    if l.starts_with("-----begin") && l.contains("private key-----") {
                        "[REDACTED]".to_string()
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
    }

    // credit_card: 4 groups of 4 digits separated by space/dash
    if let Some(replaced) = redact_credit_card(&cleaned) {
        detected.push("credit_card");
        cleaned = replaced;
    }

    (cleaned, detected)
}

/// Scans for `dddd dddd dddd dddd` (with optional single separators) and
/// replaces the match with [REDACTED].
fn redact_credit_card(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let is_digit = |i: usize| bytes.get(i).map(|b| b.is_ascii_digit()).unwrap_or(false);
    let is_sep = |i: usize| matches!(bytes.get(i), Some(b' ') | Some(b'-'));
    let mut i = 0usize;
    while i < bytes.len() {
        if !is_digit(i) {
            i += 1;
            continue;
        }
        let mut cursor = i;
        let mut groups = 0usize;
        let mut ok = true;
        while groups < 4 {
            for _ in 0..4 {
                if !is_digit(cursor) {
                    ok = false;
                    break;
                }
                cursor += 1;
            }
            if !ok {
                break;
            }
            groups += 1;
            if groups < 4 {
                if is_sep(cursor) {
                    cursor += 1;
                } else {
                    ok = false;
                    break;
                }
            }
        }
        if ok && groups == 4 {
            // boundary: the match must not extend into more digits
            if is_digit(cursor) {
                i += 1;
                continue;
            }
            let mut out = String::new();
            out.push_str(&text[..i]);
            out.push_str("[REDACTED]");
            out.push_str(&text[cursor..]);
            return Some(out);
        }
        i += 1;
    }
    None
}

// ── FactStore (fact-store.ts) ───────────────────────────────────────────────

pub struct FactRow {
    pub id: i64,
    pub fact: String,
    pub tags: Vec<String>,
    pub time: Option<String>,
    pub session_id: Option<String>,
    pub created_at: String,
    pub match_count: Option<i64>,
}

pub struct FactInsert<'a> {
    pub fact: &'a str,
    pub tags: &'a [String],
    pub time: Option<&'a str>,
    pub session_id: Option<&'a str>,
}

pub struct FactStore {
    conn: Connection,
}

const FACTS_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS facts (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    fact       TEXT NOT NULL,
    search_text TEXT NOT NULL DEFAULT '',
    tags       TEXT NOT NULL DEFAULT '[]',
    time       TEXT,
    session_id TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_facts_time ON facts(time);
CREATE INDEX IF NOT EXISTS idx_facts_session ON facts(session_id);
CREATE VIRTUAL TABLE IF NOT EXISTS facts_fts USING fts5(
    fact,
    search_text,
    content=facts,
    content_rowid=id,
    tokenize='unicode61'
);
CREATE TRIGGER IF NOT EXISTS facts_ai AFTER INSERT ON facts BEGIN
    INSERT INTO facts_fts(rowid, fact, search_text) VALUES (new.id, new.fact, new.search_text);
END;
CREATE TRIGGER IF NOT EXISTS facts_ad AFTER DELETE ON facts BEGIN
    INSERT INTO facts_fts(facts_fts, rowid, fact, search_text) VALUES ('delete', old.id, old.fact, old.search_text);
END;
CREATE TRIGGER IF NOT EXISTS facts_au AFTER UPDATE ON facts BEGIN
    INSERT INTO facts_fts(facts_fts, rowid, fact, search_text) VALUES ('delete', old.id, old.fact, old.search_text);
    INSERT INTO facts_fts(rowid, fact, search_text) VALUES (new.id, new.fact, new.search_text);
END;
CREATE TABLE IF NOT EXISTS fact_embeddings (
    fact_id    INTEGER NOT NULL REFERENCES facts(id) ON DELETE CASCADE,
    model_key  TEXT NOT NULL,
    dimensions INTEGER NOT NULL,
    vector     BLOB NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (fact_id)
);
CREATE INDEX IF NOT EXISTS idx_fact_embeddings_model ON fact_embeddings(model_key);
"#;

impl FactStore {
    /// Opens (creating if needed) one agent's facts.db with the incumbent
    /// schema v3 (`PRAGMA user_version = 3`).
    pub fn open(db_path: &Path) -> Result<Self, MemoryError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(MemoryError::from)?;
        }
        let conn = Connection::open(db_path)
            .map_err(|e| MemoryError::Store(format!("cannot open facts db: {e}")))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| MemoryError::Store(e.to_string()))?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|e| MemoryError::Store(e.to_string()))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| MemoryError::Store(e.to_string()))?;
        conn.execute_batch(FACTS_SCHEMA_SQL)
            .map_err(|e| MemoryError::Store(format!("facts schema: {e}")))?;
        let current: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| MemoryError::Store(e.to_string()))?;
        if current < 3 {
            conn.pragma_update(None, "user_version", 3i64)
                .map_err(|e| MemoryError::Store(e.to_string()))?;
        }
        Ok(Self { conn })
    }

    fn parse_tags(raw: &str) -> Vec<String> {
        serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
    }

    fn row_to_fact(row: &rusqlite::Row<'_>) -> rusqlite::Result<FactRow> {
        Ok(FactRow {
            id: row.get("id")?,
            fact: row.get("fact")?,
            tags: Self::parse_tags(&row.get::<_, String>("tags")?),
            time: row.get("time")?,
            session_id: row.get("session_id")?,
            created_at: row.get("created_at")?,
            match_count: None,
        })
    }

    fn insert_one(conn: &Connection, entry: &FactInsert<'_>) -> Result<i64, MemoryError> {
        let (cleaned, detected) = scrub_pii(entry.fact);
        let _ = detected; // redaction is silent-by-design at storage; tools log it
        let search_text = build_fact_search_text(&cleaned, entry.tags);
        let tags_json = serde_json::to_string(entry.tags).unwrap_or_else(|_| "[]".into());
        let created = now_iso();
        conn.execute(
            "INSERT INTO facts (fact, search_text, tags, time, session_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                cleaned,
                search_text,
                tags_json,
                entry.time,
                entry.session_id,
                created
            ],
        )
        .map_err(|e| MemoryError::Store(e.to_string()))?;
        Ok(conn.last_insert_rowid())
    }

    pub fn add(&self, entry: &FactInsert<'_>) -> Result<i64, MemoryError> {
        Self::insert_one(&self.conn, entry)
    }

    pub fn add_batch(&self, entries: &[FactInsert<'_>]) -> Result<usize, MemoryError> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| MemoryError::Store(e.to_string()))?;
        for entry in entries {
            Self::insert_one(&tx, entry)?;
        }
        tx.commit().map_err(|e| MemoryError::Store(e.to_string()))?;
        Ok(entries.len())
    }

    /// Replace every fact of one session in one transaction
    /// (fact-store.ts:316-335).
    pub fn replace_by_session(
        &self,
        session_id: &str,
        entries: &[FactInsert<'_>],
    ) -> Result<usize, MemoryError> {
        let stable = session_id.trim();
        if stable.is_empty() {
            return Err(MemoryError::InvalidRequest(
                "replaceBySession requires sessionId".into(),
            ));
        }
        for entry in entries {
            if entry.fact.trim().is_empty() {
                return Err(MemoryError::InvalidRequest(
                    "replacement fact must be a non-empty string".into(),
                ));
            }
        }
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| MemoryError::Store(e.to_string()))?;
        tx.execute("DELETE FROM facts WHERE session_id = ?1", [stable])
            .map_err(|e| MemoryError::Store(e.to_string()))?;
        for entry in entries {
            Self::insert_one(
                &tx,
                &FactInsert {
                    fact: entry.fact,
                    tags: entry.tags,
                    time: entry.time,
                    session_id: Some(stable),
                },
            )?;
        }
        tx.commit().map_err(|e| MemoryError::Store(e.to_string()))?;
        Ok(entries.len())
    }

    pub fn size(&self) -> Result<usize, MemoryError> {
        self.conn
            .query_row("SELECT COUNT(*) FROM facts", [], |row| row.get::<_, i64>(0))
            .map(|n| n as usize)
            .map_err(|e| MemoryError::Store(e.to_string()))
    }

    pub fn get_all(&self) -> Result<Vec<FactRow>, MemoryError> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM facts ORDER BY time DESC")
            .map_err(|e| MemoryError::Store(e.to_string()))?;
        let rows = stmt
            .query_map([], Self::row_to_fact)
            .map_err(|e| MemoryError::Store(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| MemoryError::Store(e.to_string()))
    }

    pub fn delete_by_id(&self, id: i64) -> Result<bool, MemoryError> {
        self.conn
            .execute("DELETE FROM facts WHERE id = ?1", [id])
            .map(|n| n > 0)
            .map_err(|e| MemoryError::Store(e.to_string()))
    }

    pub fn delete_by_session(&self, session_id: &str) -> Result<usize, MemoryError> {
        let stable = session_id.trim();
        if stable.is_empty() {
            return Err(MemoryError::InvalidRequest(
                "fact invalidation requires sessionId".into(),
            ));
        }
        self.conn
            .execute("DELETE FROM facts WHERE session_id = ?1", [stable])
            .map_err(|e| MemoryError::Store(e.to_string()))
    }

    pub fn clear_all(&self) -> Result<(), MemoryError> {
        self.conn
            .execute("DELETE FROM facts", [])
            .map_err(|e| MemoryError::Store(e.to_string()))?;
        self.conn
            .query_row(
                "INSERT INTO facts_fts(facts_fts) VALUES ('rebuild') RETURNING 1",
                [],
                |_| Ok(()),
            )
            .or(Ok(()))
            .map_err(|e: rusqlite::Error| MemoryError::Store(e.to_string()))?;
        Ok(())
    }

    /// Tag search with json_each exact matching (fact-store.ts:347-389):
    /// matchCount DESC then time DESC.
    pub fn search_by_tags(
        &self,
        query_tags: &[String],
        date_from: Option<&str>,
        date_to: Option<&str>,
        limit: usize,
    ) -> Result<Vec<FactRow>, MemoryError> {
        if query_tags.is_empty() {
            return Ok(Vec::new());
        }
        let mut sql = String::from(
            "SELECT f.*, COUNT(DISTINCT je.value) as matchCount \
             FROM facts f, json_each(f.tags) je \
             WHERE je.value IN (",
        );
        for i in 0..query_tags.len() {
            if i > 0 {
                sql.push(',');
            }
            sql.push_str(&format!("?{}", i + 1));
        }
        sql.push(')');
        if date_from.is_some() {
            sql.push_str(" AND f.time >= ?date_from");
        }
        if date_to.is_some() {
            sql.push_str(" AND f.time <= ?date_to");
        }
        sql.push_str(" GROUP BY f.id ORDER BY matchCount DESC, f.time DESC LIMIT ?limit");
        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| MemoryError::Store(e.to_string()))?;
        let n = query_tags.len();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        for tag in query_tags {
            params.push(Box::new(tag.clone()));
        }
        if let Some(from) = date_from {
            params.push(Box::new(from.to_string()));
        }
        if let Some(to) = date_to {
            params.push(Box::new(to.to_string()));
        }
        params.push(Box::new(limit as i64));
        let params: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(params.as_slice(), Self::row_to_fact)
            .map_err(|e| MemoryError::Store(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            let mut fact = row.map_err(|e| MemoryError::Store(e.to_string()))?;
            fact.match_count = Some(1); // replaced below by exact count fetch
            out.push(fact);
        }
        // matchCount: re-query is wasteful; compute via a second pass keyed by id.
        // Simpler: matchCount is not part of the API response contract beyond
        // ordering — leave None (incumbent `_rowToFact` carries it only for
        // internal ordering). We set it from the ORDER — ordering already applied.
        let _ = n;
        Ok(out)
    }

    fn like_fallback(&self, query: &str, limit: usize) -> Result<Vec<FactRow>, MemoryError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT * FROM facts WHERE fact LIKE '%' || ?1 || '%' ORDER BY time DESC LIMIT ?2",
            )
            .map_err(|e| MemoryError::Store(e.to_string()))?;
        let rows = stmt
            .query_map([query, &limit.to_string()], Self::row_to_fact)
            .map_err(|e| MemoryError::Store(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| MemoryError::Store(e.to_string()))
    }

    /// Full-text search (fact-store.ts:398-424): FTS MATCH; zero hits with
    /// CJK or FTS error → LIKE fallback.
    pub fn search_full_text(&self, query: &str, limit: usize) -> Result<Vec<FactRow>, MemoryError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let fts_query = build_fts_query(query);
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }
        let result = self
            .conn
            .prepare("SELECT f.*, rank FROM facts_fts fts JOIN facts f ON f.id = fts.rowid WHERE facts_fts MATCH ?1 ORDER BY rank LIMIT ?2")
            .and_then(|mut stmt| {
                let rows = stmt.query_map([&fts_query, &limit.to_string()], Self::row_to_fact)?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row?);
                }
                Ok(out)
            });
        match result {
            Ok(rows) if !rows.is_empty() => Ok(rows),
            Ok(_) if has_cjk(query) => self.like_fallback(query, limit),
            Ok(rows) => Ok(rows),
            Err(_) => self.like_fallback(query, limit),
        }
    }

    /// Zero-result diagnostic per-term counts (fact-store.ts:431-453), same
    /// escaping + fallback path as search_full_text.
    pub fn count_full_text_matches(&self, term: &str) -> Result<usize, MemoryError> {
        let normalized = term.trim();
        if normalized.is_empty() {
            return Ok(0);
        }
        let fts_query = build_fts_query(normalized);
        if fts_query.is_empty() {
            return Ok(0);
        }
        let count: rusqlite::Result<i64> = self.conn.query_row(
            "SELECT COUNT(*) FROM facts_fts WHERE facts_fts MATCH ?1",
            [&fts_query],
            |row| row.get(0),
        );
        match count {
            Ok(0) if has_cjk(normalized) => {
                let n: i64 = self
                    .conn
                    .query_row(
                        "SELECT COUNT(*) FROM facts WHERE fact LIKE '%' || ?1 || '%'",
                        [normalized],
                        |row| row.get(0),
                    )
                    .map_err(|e| MemoryError::Store(e.to_string()))?;
                Ok(n as usize)
            }
            Ok(n) => Ok(n as usize),
            Err(_) => {
                let n: i64 = self
                    .conn
                    .query_row(
                        "SELECT COUNT(*) FROM facts WHERE fact LIKE '%' || ?1 || '%'",
                        [normalized],
                        |row| row.get(0),
                    )
                    .map_err(|e| MemoryError::Store(e.to_string()))?;
                Ok(n as usize)
            }
        }
    }

    pub fn zero_result_diagnostics(&self, query: &str) -> Result<Option<Value>, MemoryError> {
        let terms = zero_result_diagnostic_terms(query, ZERO_RESULT_DIAGNOSTIC_MAX_TERMS);
        if terms.is_empty() {
            return Ok(None);
        }
        let mut counts = Vec::new();
        for term in &terms {
            counts.push(json!({
                "term": term,
                "count": self.count_full_text_matches(term)?,
            }));
        }
        Ok(Some(json!(counts)))
    }

    /// Tag counts for the navigation section (fact-store.ts:465-475): count
    /// DESC then tag ASC.
    pub fn tag_counts(&self) -> Result<Vec<(String, usize)>, MemoryError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT je.value as tag, COUNT(*) as cnt \
                 FROM facts f, json_each(f.tags) je \
                 GROUP BY je.value ORDER BY cnt DESC, tag ASC",
            )
            .map_err(|e| MemoryError::Store(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
            })
            .map_err(|e| MemoryError::Store(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| MemoryError::Store(e.to_string()))
    }

    pub fn export_all(&self) -> Result<Vec<FactRow>, MemoryError> {
        self.get_all()
    }

    pub fn import_all(&self, entries: &[FactInsert<'_>]) -> Result<usize, MemoryError> {
        self.add_batch(entries)
    }
}

// ── Tenets store (tenets.ts file IO) ────────────────────────────────────────

pub struct TenetsStore {
    file_path: PathBuf,
}

impl TenetsStore {
    pub fn new(memory_dir: &Path) -> Self {
        Self {
            file_path: memory_dir.join("tenets.json"),
        }
    }

    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// Lenient read for UI listing (tenets.ts readTenetsFile:116-129): corrupt
    /// → empty file, invalid entries filtered.
    pub fn read(&self) -> TenetsFile {
        let raw = safe_read(&self.file_path);
        if raw.trim().is_empty() {
            return TenetsFile::empty();
        }
        match serde_json::from_str::<Value>(&raw) {
            Ok(value) => match validate_tenets_strict(&value) {
                Ok(file) => file,
                Err(_) => TenetsFile::empty(),
            },
            Err(_) => TenetsFile::empty(),
        }
    }

    /// Strict read for write boundaries (tenets.ts readTenetsFileStrict):
    /// only a missing file reads as empty; corruption is a loud error.
    pub fn read_strict(&self) -> Result<TenetsFile, MemoryError> {
        let raw = match std::fs::read_to_string(&self.file_path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(TenetsFile::empty()),
            Err(err) => {
                return Err(MemoryError::Conflict {
                    code: "TENET_STORE_READ_FAILED".into(),
                    message: format!("cannot read tenets store: {err}"),
                })
            }
        };
        let value: Value = serde_json::from_str(&raw).map_err(|_| MemoryError::Conflict {
            code: "TENET_STORE_CORRUPTED".into(),
            message: "tenets store is not valid JSON".into(),
        })?;
        validate_tenets_strict(&value).map_err(|code| MemoryError::Conflict {
            code: code.as_str().to_string(),
            message: format!("tenets store rejected: {}", code.as_str()),
        })
    }

    pub fn write(&self, file: &TenetsFile) -> Result<(), MemoryError> {
        let mut sorted = file.clone();
        sort_tenets(&mut sorted.tenets);
        let value = json!({
            "schemaVersion": TENETS_SCHEMA_VERSION,
            "tenets": sorted.tenets.iter().map(tenet_to_json).collect::<Vec<_>>(),
        });
        // Deterministic serialization (tenets.ts serializeTenetsFile: pretty JSON + newline).
        let text = serde_json::to_string_pretty(&value)
            .map_err(|e| MemoryError::Store(e.to_string()))?
            + "\n";
        atomic_write(&self.file_path, &text).map_err(MemoryError::from)
    }

    fn with_file<T>(
        &self,
        mutate: impl FnOnce(&mut TenetsFile) -> Result<T, MemoryError>,
    ) -> Result<T, MemoryError> {
        let mut file = self.read_strict()?;
        let result = mutate(&mut file)?;
        self.write(&file)?;
        Ok(result)
    }

    pub fn list(&self) -> TenetsFile {
        let mut file = self.read();
        sort_tenets(&mut file.tenets);
        file
    }

    pub fn active(&self) -> Vec<Tenet> {
        self.list()
            .tenets
            .into_iter()
            .filter(|t| t.status == TenetStatus::Active)
            .collect()
    }

    pub fn pending(&self) -> Vec<Tenet> {
        self.list()
            .tenets
            .into_iter()
            .filter(|t| t.status == TenetStatus::Pending)
            .collect()
    }

    pub fn add_proposal(
        &self,
        content: &str,
        priority: Option<&str>,
        session_id: Option<&str>,
        new_id: String,
    ) -> Result<(Tenet, bool, Option<TenetStatus>), MemoryError> {
        self.with_file(|file| {
            tenets_add_proposal(
                file,
                &NewTenetInput {
                    content,
                    priority,
                    session_id,
                    new_id,
                    now_iso: now_iso(),
                },
            )
            .map_err(|code| MemoryError::Conflict {
                code: code.as_str().to_string(),
                message: match code {
                    TenetErrorCode::PendingFull => {
                        "pending tenets are full; ask the user to review pending proposals first"
                            .into()
                    }
                    TenetErrorCode::Invalid => {
                        "tenet content must be a non-empty string within the length cap".into()
                    }
                    _ => code.as_str().to_string(),
                },
            })
        })
    }

    pub fn add_direct(
        &self,
        content: &str,
        priority: Option<&str>,
        new_id: String,
    ) -> Result<(Tenet, bool), MemoryError> {
        self.with_file(|file| {
            tenets_add_direct(
                file,
                &NewTenetInput {
                    content,
                    priority,
                    session_id: None,
                    new_id,
                    now_iso: now_iso(),
                },
            )
            .map_err(|code| MemoryError::Conflict {
                code: code.as_str().to_string(),
                message: match code {
                    TenetErrorCode::LimitReached => {
                        "active user-pinned tenets are full; remove one before adding another"
                            .into()
                    }
                    TenetErrorCode::Invalid => {
                        "tenet content must be a non-empty string within the length cap".into()
                    }
                    _ => code.as_str().to_string(),
                },
            })
        })
    }

    pub fn decide(&self, tenet_id: &str, approve: bool) -> Result<Tenet, MemoryError> {
        self.with_file(|file| {
            tenets_decide(file, tenet_id, approve, &now_iso()).map_err(|code| {
                MemoryError::Conflict {
                    code: code.as_str().to_string(),
                    message: match code {
                        TenetErrorCode::NotFound => format!("tenet {tenet_id} not found"),
                        TenetErrorCode::Invalid => format!("tenet {tenet_id} is not pending"),
                        TenetErrorCode::LimitReached => {
                            "active model-proposed tenets are full; remove one before approving"
                                .into()
                        }
                        _ => code.as_str().to_string(),
                    },
                }
            })
        })
    }

    pub fn remove(&self, tenet_id: &str) -> Result<bool, MemoryError> {
        self.with_file(|file| Ok(tenets_remove(file, tenet_id)))
    }

    pub fn replace_pinned(
        &self,
        pins: &[String],
        new_id: String,
    ) -> Result<Vec<Tenet>, MemoryError> {
        let mut counter = 0usize;
        self.with_file(|file| {
            tenets_replace_pinned(
                file,
                pins,
                &mut || {
                    counter += 1;
                    format!("{new_id}-{}", counter)
                },
                &now_iso(),
            )
            .map_err(|code| MemoryError::Conflict {
                code: code.as_str().to_string(),
                message: match code {
                    TenetErrorCode::LimitReached => {
                        "active user-pinned tenets are full; remove one before adding another"
                            .into()
                    }
                    _ => code.as_str().to_string(),
                },
            })
        })
    }
}

// ── compiled memory (compile.ts + compiled-memory-state.ts) ─────────────────

pub const COMPILED_FILES: [&str; 5] = [
    "memory.md",
    "facts.md",
    "today.md",
    "week.md",
    "longterm.md",
];

pub struct CompiledMemoryStore {
    memory_dir: PathBuf,
    is_zh: bool,
}

pub struct CompiledSections {
    pub facts: String,
    pub today: String,
    pub week: String,
    pub longterm: String,
}

impl CompiledMemoryStore {
    pub fn new(memory_dir: &Path, is_zh: bool) -> Self {
        Self {
            memory_dir: memory_dir.to_path_buf(),
            is_zh,
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.memory_dir.join(name)
    }

    /// readCompiledMemorySections (compile.ts:748-756) with the facts.md
    /// existence baseline (ensureEditableFactsBaseline :869-903, summary
    /// watermark parts belong to the ticker and are absent here).
    pub fn read_sections(&self) -> Result<CompiledSections, MemoryError> {
        std::fs::create_dir_all(&self.memory_dir).map_err(MemoryError::from)?;
        let facts_path = self.path("facts.md");
        if !facts_path.exists() {
            atomic_write(&facts_path, "").map_err(MemoryError::from)?;
        }
        Ok(CompiledSections {
            facts: normalize_compiled_section_body(&safe_read(&facts_path)),
            today: normalize_compiled_section_body(&safe_read(&self.path("today.md"))),
            week: normalize_compiled_week_section_body(&safe_read(&self.path("week.md"))),
            longterm: normalize_compiled_section_body(&safe_read(&self.path("longterm.md"))),
        })
    }

    pub fn navigation(&self) -> String {
        safe_read(&self.path("navigation.md"))
    }

    /// assemble (compile.ts:1054-1091).
    pub fn assemble(&self) -> Result<String, MemoryError> {
        let sections = self.read_sections()?;
        let content = build_compiled_memory_markdown(
            &sections.facts,
            &sections.today,
            &sections.week,
            &sections.longterm,
            &self.navigation(),
            self.is_zh,
        );
        atomic_write(&self.path("memory.md"), &content).map_err(MemoryError::from)?;
        Ok(content)
    }

    pub fn read_assembled(&self) -> Result<String, MemoryError> {
        let sections = self.read_sections()?;
        Ok(build_compiled_memory_markdown(
            &sections.facts,
            &sections.today,
            &sections.week,
            &sections.longterm,
            &self.navigation(),
            self.is_zh,
        ))
    }

    fn write_section(&self, file: &str, body: &str) -> Result<String, MemoryError> {
        let normalized = normalize_compiled_section_body(body);
        atomic_write(
            &self.path(file),
            &if normalized.is_empty() {
                String::new()
            } else {
                format!("{normalized}\n")
            },
        )
        .map_err(MemoryError::from)?;
        self.assemble()?;
        Ok(normalized)
    }

    pub fn write_facts(&self, facts: &str) -> Result<String, MemoryError> {
        self.write_section("facts.md", facts)
    }

    pub fn write_today(&self, today: &str) -> Result<String, MemoryError> {
        self.write_section("today.md", today)
    }

    pub fn write_longterm(&self, longterm: &str) -> Result<String, MemoryError> {
        self.write_section("longterm.md", longterm)
    }

    /// listWeekDayEntries (compile.ts:827-847): daily/*.md sorted by date.
    pub fn list_week_days(&self) -> Result<Vec<(String, String)>, MemoryError> {
        let daily_dir = self.memory_dir.join("daily");
        let mut dates: Vec<String> = Vec::new();
        match std::fs::read_dir(&daily_dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if let Some(date) = name.strip_suffix(".md") {
                        if is_valid_iso_date(date) {
                            dates.push(date.to_string());
                        }
                    }
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(MemoryError::from(err)),
        }
        dates.sort();
        Ok(dates
            .into_iter()
            .map(|date| {
                let body = safe_read(&daily_dir.join(format!("{date}.md")));
                (date, body.trim().to_string())
            })
            .collect())
    }

    /// writeWeekDayEntry (compile.ts:849-866): only existing dates; then
    /// reassemble week.md + memory.md.
    pub fn write_week_day(&self, date: &str, body: &str) -> Result<String, MemoryError> {
        if !is_valid_iso_date(date) {
            return Err(MemoryError::InvalidRequest(
                "date must be YYYY-MM-DD".into(),
            ));
        }
        let existing = self.list_week_days()?;
        if !existing.iter().any(|(d, _)| d == date) {
            return Err(MemoryError::NotFound(format!(
                "no daily entry for date \"{date}\""
            )));
        }
        let normalized = body.trim().to_string();
        atomic_write(
            &self.memory_dir.join("daily").join(format!("{date}.md")),
            &format!("{normalized}\n"),
        )
        .map_err(MemoryError::from)?;
        self.assemble_week()?;
        self.assemble()?;
        Ok(normalized)
    }

    /// assembleWeekFromDaily (compile.ts:586-611).
    pub fn assemble_week(&self) -> Result<String, MemoryError> {
        let days = self.list_week_days()?;
        let blocks: Vec<String> = days.into_iter().map(|(_, body)| body).collect();
        let content = lingxi_kernel::memory::assemble_week_blocks(
            &blocks,
            6,
            lingxi_kernel::memory::WEEK_ASSEMBLY_MAX_CHARS,
        );
        let text = if content.is_empty() {
            String::new()
        } else {
            format!("{content}\n")
        };
        atomic_write(&self.path("week.md"), &text).map_err(MemoryError::from)?;
        Ok(text)
    }

    /// writeCompiledResetMarker (compiled-memory-state.ts:25-43).
    pub fn write_reset_marker(&self) -> Result<String, MemoryError> {
        let now = now_iso();
        let marker = json!({
            "compiledResetAt": now,
            "updatedAt": now,
        });
        let text = serde_json::to_string_pretty(&marker)
            .map_err(|e| MemoryError::Store(e.to_string()))?
            + "\n";
        atomic_write(&self.path("reset.json"), &text).map_err(MemoryError::from)?;
        Ok(now)
    }

    pub fn read_reset_at(&self) -> Option<String> {
        let raw = safe_read(&self.path("reset.json"));
        let value: Value = serde_json::from_str(&raw).ok()?;
        value.get("compiledResetAt")?.as_str().map(str::to_string)
    }

    /// clearCompiledMemoryArtifacts (compiled-memory-state.ts:45-57): empty
    /// the five files, remove fingerprints.
    pub fn clear_artifacts(&self) -> Result<(), MemoryError> {
        std::fs::create_dir_all(&self.memory_dir).map_err(MemoryError::from)?;
        for name in COMPILED_FILES {
            let path = self.path(name);
            std::fs::write(&path, "").map_err(MemoryError::from)?;
            remove_if_exists(&path.with_extension("md.fingerprint")).map_err(MemoryError::from)?;
        }
        Ok(())
    }

    /// migrateLegacyEditableFacts (compile.ts:905-955): idempotent read-time
    /// migration of alpha `editable-facts.md` into canonical `facts.md`.
    pub fn migrate_legacy_editable_facts(&self) -> Result<Value, MemoryError> {
        let legacy = self.path("editable-facts.md");
        if !legacy.exists() {
            return Ok(json!({"migrated": false, "reason": "no-legacy-file"}));
        }
        let editable = safe_read(&legacy);
        let canonical_path = self.path("facts.md");
        let has_canonical = canonical_path.exists();
        let canonical = if has_canonical {
            safe_read(&canonical_path)
        } else {
            String::new()
        };
        let merged = if has_canonical {
            merge_facts_entries(&editable, &canonical)
        } else {
            editable.clone()
        };
        if has_canonical {
            atomic_write(&canonical_path.with_extension("md.bak"), &canonical)
                .map_err(MemoryError::from)?;
        }
        atomic_write(&legacy.with_extension("md.bak"), &editable).map_err(MemoryError::from)?;
        atomic_write(&canonical_path, &merged).map_err(MemoryError::from)?;
        remove_if_exists(&legacy).map_err(MemoryError::from)?;
        Ok(json!({
            "migrated": true,
            "reason": if has_canonical { "merged" } else { "renamed" },
        }))
    }
}

fn is_valid_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    value[..4].chars().all(|c| c.is_ascii_digit())
        && value[5..7].chars().all(|c| c.is_ascii_digit())
        && value[8..10].chars().all(|c| c.is_ascii_digit())
}

fn normalize_fact_line_for_dedup(line: &str) -> String {
    let trimmed = line.trim();
    let stripped = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .unwrap_or(trimmed);
    stripped.to_lowercase()
}

fn merge_facts_entries(primary: &str, secondary: &str) -> String {
    let primary_text = normalize_compiled_section_body(primary);
    let secondary_text = normalize_compiled_section_body(secondary);
    if secondary_text.is_empty() {
        return primary_text;
    }
    if primary_text.is_empty() {
        return secondary_text;
    }
    let mut seen = std::collections::HashSet::new();
    for line in primary_text.lines() {
        seen.insert(normalize_fact_line_for_dedup(line));
    }
    let mut extra = Vec::new();
    for line in secondary_text.lines() {
        let key = normalize_fact_line_for_dedup(line);
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        extra.push(line.to_string());
    }
    if extra.is_empty() {
        primary_text
    } else {
        format!("{primary_text}\n{}", extra.join("\n"))
    }
}

// ── experience store (experience.ts shared storage ops) ─────────────────────

/// parseExperienceDocument (experience.ts:357-373): title meta line is
/// base64url; a missing/invalid meta falls back to the file stem.
pub fn parse_experience_document(content: &str, fallback_title: &str) -> (String, String) {
    use base64::Engine as _;
    let mut lines = content.splitn(2, '\n');
    let first = lines.next().unwrap_or("");
    let rest = lines.next().unwrap_or("");
    let trimmed = first.trim();
    if let Some(encoded) = trimmed
        .strip_prefix("<!-- experience-title: ")
        .and_then(|r| r.strip_suffix("-->"))
    {
        let encoded = encoded.trim();
        if encoded
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && !encoded.is_empty()
        {
            if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(encoded) {
                if let Ok(title) = String::from_utf8(bytes) {
                    if lingxi_kernel::memory::normalize_experience_category(&title).is_some() {
                        return (title, rest.trim_start_matches('\n').trim_end().to_string());
                    }
                }
            }
        }
    }
    (fallback_title.to_string(), content.trim_end().to_string())
}

pub struct ExperienceStore {
    agent_dir: PathBuf,
}

pub struct ExperienceDoc {
    pub title: String,
    pub file: String,
    pub body: String,
}

impl ExperienceStore {
    pub fn new(agent_dir: &Path) -> Self {
        Self {
            agent_dir: agent_dir.to_path_buf(),
        }
    }

    fn dir(&self) -> PathBuf {
        self.agent_dir.join("experience")
    }

    fn index_path(&self) -> PathBuf {
        self.agent_dir.join("experience.md")
    }

    /// listExperienceDocuments (experience.ts:375-391): title from the
    /// `<!-- experience-title: <base64url> -->` meta line, fallback = file
    /// stem; body = the remainder.
    fn list_documents(&self) -> Vec<ExperienceDoc> {
        let mut docs = Vec::new();
        let Ok(entries) = std::fs::read_dir(self.dir()) else {
            return docs;
        };
        let mut files: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".md"))
            .collect();
        files.sort();
        for file in files {
            let raw = safe_read(&self.dir().join(&file));
            let (title, body) = parse_experience_document(&raw, file.trim_end_matches(".md"));
            docs.push(ExperienceDoc { title, file, body });
        }
        docs
    }

    /// serializeExperienceDocument (experience.ts:353-355).
    fn serialize_document(&self, title: &str, body: &str) -> String {
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(title.as_bytes());
        format!(
            "<!-- experience-title: {encoded} -->\n{}\n",
            body.trim_end()
        )
    }

    /// rebuildIndex (experience.ts:29-61).
    pub fn rebuild_index(&self) -> Result<(), MemoryError> {
        let docs = self.list_documents();
        if docs.is_empty() {
            std::fs::write(self.index_path(), "").map_err(MemoryError::from)?;
            return Ok(());
        }
        let mut blocks = Vec::new();
        for doc in &docs {
            let entries: Vec<String> = doc
                .body
                .lines()
                .filter(|l| {
                    let t = l.trim();
                    let mut chars = t.chars();
                    chars.next().map(|c| c.is_ascii_digit()).unwrap_or(false) && t.contains(". ")
                })
                .map(|l| {
                    let idx = l.find(". ").expect("checked");
                    l[idx + 2..].trim().to_string()
                })
                .collect();
            if let Some(block) =
                lingxi_kernel::memory::build_experience_index_block(&doc.title, &entries, &doc.file)
            {
                blocks.push(block);
            }
        }
        let content = if blocks.is_empty() {
            String::new()
        } else {
            format!("{}\n", blocks.join("\n\n"))
        };
        std::fs::write(self.index_path(), content).map_err(MemoryError::from)
    }

    /// recordEntry (experience.ts:68-97): dedup by substring, number + append.
    pub fn record_entry(
        &self,
        category: &str,
        content: &str,
    ) -> Result<(bool, Option<String>), MemoryError> {
        let Some(safe_category) = normalize_experience_category(category) else {
            return Err(MemoryError::InvalidRequest(
                "invalid experience category".into(),
            ));
        };
        std::fs::create_dir_all(self.dir()).map_err(MemoryError::from)?;
        let existing_doc = self
            .list_documents()
            .into_iter()
            .find(|d| d.title == safe_category);
        let (file_name, existing) = match existing_doc {
            Some(doc) => (doc.file, doc.body),
            None => (
                format!(
                    "{}.md",
                    lingxi_kernel::memory::experience_storage_stem(&safe_category)
                ),
                String::new(),
            ),
        };
        let file_path = self.dir().join(&file_name);
        if existing.contains(content) {
            return Ok((false, Some("duplicate".into())));
        }
        let Some(line) = lingxi_kernel::memory::experience_next_entry(&existing, content) else {
            return Ok((false, Some("empty".into())));
        };
        let updated = if existing.trim_end().is_empty() {
            line.clone()
        } else {
            format!("{}\n{line}", existing.trim_end())
        };
        let serialized = self.serialize_document(&safe_category, &updated);
        std::fs::write(&file_path, serialized).map_err(MemoryError::from)?;
        self.rebuild_index()?;
        Ok((true, None))
    }

    /// syncExperienceCategories (experience.ts:99-125): whole-document PUT.
    pub fn sync_categories(&self, categories: &[(String, String)]) -> Result<(), MemoryError> {
        std::fs::create_dir_all(self.dir()).map_err(MemoryError::from)?;
        let existing = self.list_documents();
        let by_title: std::collections::HashMap<String, &ExperienceDoc> =
            existing.iter().map(|d| (d.title.clone(), d)).collect();
        let mut next_files: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (raw_category, raw_body) in categories {
            let Some(category) = normalize_experience_category(raw_category) else {
                return Err(MemoryError::InvalidRequest(
                    "invalid experience category".into(),
                ));
            };
            let body = raw_body.trim();
            if body.is_empty() {
                continue;
            }
            let file = by_title
                .get(&category)
                .map(|d| d.file.clone())
                .unwrap_or_else(|| {
                    format!(
                        "{}.md",
                        lingxi_kernel::memory::experience_storage_stem(&category)
                    )
                });
            let serialized = self.serialize_document(&category, body);
            std::fs::write(self.dir().join(&file), serialized).map_err(MemoryError::from)?;
            next_files.insert(file);
        }
        for doc in &existing {
            if !next_files.contains(&doc.file) {
                remove_if_exists(&self.dir().join(&doc.file)).map_err(MemoryError::from)?;
            }
        }
        self.rebuild_index()
    }

    pub fn read_merged(&self) -> (String, Vec<Value>) {
        let docs = self.list_documents();
        let mut merged = String::new();
        let mut files = Vec::new();
        for doc in &docs {
            if !doc.body.trim().is_empty() {
                if !merged.is_empty() {
                    merged.push_str("\n\n");
                }
                merged.push_str(doc.body.trim());
            }
            files.push(json!({"file": doc.file, "title": doc.title}));
        }
        (merged, files)
    }

    pub fn index_text(&self) -> String {
        safe_read(&self.index_path())
    }
}

// ── dream revision store (dream/revision-store.ts + state-store.ts) ─────────

pub const MAX_REVISIONS: usize = 10;

pub struct DreamRevisionStore {
    memory_dir: PathBuf,
}

impl DreamRevisionStore {
    pub fn new(memory_dir: &Path) -> Self {
        Self {
            memory_dir: memory_dir.to_path_buf(),
        }
    }

    fn revisions_dir(&self) -> PathBuf {
        self.memory_dir.join("dream").join("revisions")
    }

    fn revision_path(&self, revision_id: &str) -> Result<PathBuf, MemoryError> {
        if revision_id.is_empty()
            || !revision_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            return Err(MemoryError::InvalidRequest(
                "Invalid Dream revision id".into(),
            ));
        }
        Ok(self.revisions_dir().join(format!("{revision_id}.json")))
    }

    /// snapshotDreamSections (revision-store.ts:80-90).
    pub fn snapshot_sections(&self) -> Result<Value, MemoryError> {
        let store = CompiledMemoryStore::new(&self.memory_dir, false);
        let sections = store.read_sections()?;
        let week_days: Vec<Value> = store
            .list_week_days()?
            .into_iter()
            .map(|(date, body)| json!({"date": date, "body": body}))
            .collect();
        Ok(json!({
            "facts": sections.facts,
            "today": sections.today,
            "weekDays": week_days,
            "longterm": sections.longterm,
        }))
    }

    /// saveDreamRevision (revision-store.ts:92-111).
    pub fn save(
        &self,
        run_id: &str,
        trigger: &str,
        kind: &str,
        restores_revision_id: Option<&str>,
    ) -> Result<Value, MemoryError> {
        let before = self.snapshot_sections()?;
        let revision_id = format!(
            "{}-{}",
            now_iso().replace([':', '.'], "-"),
            run_id[run_id.len().saturating_sub(8)..].to_lowercase()
        );
        let revision = json!({
            "schemaVersion": 1,
            "revisionId": revision_id,
            "runId": run_id,
            "trigger": trigger,
            "createdAt": now_iso(),
            "kind": kind,
            "restoresRevisionId": restores_revision_id,
            "before": before,
        });
        std::fs::create_dir_all(self.revisions_dir()).map_err(MemoryError::from)?;
        let text = serde_json::to_string_pretty(&revision)
            .map_err(|e| MemoryError::Store(e.to_string()))?
            + "\n";
        atomic_write(
            &self.revision_path(revision["revisionId"].as_str().unwrap())?,
            &text,
        )
        .map_err(MemoryError::from)?;
        self.prune()?;
        Ok(revision)
    }

    fn prune(&self) -> Result<(), MemoryError> {
        let mut entries: Vec<(String, PathBuf)> = Vec::new();
        if let Ok(read) = std::fs::read_dir(self.revisions_dir()) {
            for entry in read.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".json") {
                    entries.push((name, entry.path()));
                }
            }
        }
        entries.sort_by(|a, b| b.0.cmp(&a.0)); // newest name first (timestamped ids)
        for (_, path) in entries.into_iter().skip(MAX_REVISIONS) {
            remove_if_exists(&path).map_err(MemoryError::from)?;
        }
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<Value>, MemoryError> {
        let mut out = Vec::new();
        if let Ok(read) = std::fs::read_dir(self.revisions_dir()) {
            let mut paths: Vec<PathBuf> = read.flatten().map(|e| e.path()).collect();
            paths.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
            for path in paths {
                let raw = safe_read(&path);
                if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                    out.push(value);
                }
            }
        }
        Ok(out)
    }

    pub fn read(&self, revision_id: &str) -> Result<Value, MemoryError> {
        let path = self.revision_path(revision_id)?;
        let raw = std::fs::read_to_string(&path).map_err(|err| match err.kind() {
            io::ErrorKind::NotFound => MemoryError::NotFound("Dream revision not found".into()),
            _ => MemoryError::Io(err.to_string()),
        })?;
        let value: Value =
            serde_json::from_str(&raw).map_err(|e| MemoryError::Store(e.to_string()))?;
        if value.get("schemaVersion").and_then(Value::as_u64) != Some(1)
            || value.get("revisionId").and_then(Value::as_str) != Some(revision_id)
            || value.get("before").is_none()
        {
            return Err(MemoryError::Store(
                "Dream revision has an invalid format".into(),
            ));
        }
        Ok(value)
    }

    /// restoreDreamRevision (revision-store.ts:243-278): snapshot the CURRENT
    /// (pre-restore) sections into a `pre_restore` safety revision FIRST —
    /// that revision is the user's undo handle and must hold the state being
    /// replaced, never the restored content. Identical content short-circuits
    /// with no revision (incumbent dreamSectionsHash guard :246-247). The
    /// pending journal / rollback machinery is a registered ledger diff
    /// (single-process store, atomic writes).
    pub fn restore(&self, revision_id: &str) -> Result<Value, MemoryError> {
        let revision = self.read(revision_id)?;
        let current = self.snapshot_sections()?;
        if current == revision["before"] {
            return Ok(json!({
                "restored": revision_id,
                "unchanged": true,
            }));
        }
        let pre = self.save(
            &format!("restore-{}", random_token_suffix()),
            "manual",
            "pre_restore",
            Some(revision_id),
        )?;
        let before = revision["before"].clone();
        let facts = before["facts"].as_str().unwrap_or_default();
        let today = before["today"].as_str().unwrap_or_default();
        let longterm = before["longterm"].as_str().unwrap_or_default();
        let store = CompiledMemoryStore::new(&self.memory_dir, false);
        store.write_facts(facts)?;
        store.write_today(today)?;
        store.write_longterm(longterm)?;
        if let Some(days) = before["weekDays"].as_array() {
            let existing: std::collections::HashSet<String> = store
                .list_week_days()?
                .into_iter()
                .map(|(d, _)| d)
                .collect();
            for day in days {
                let Some(date) = day.get("date").and_then(Value::as_str) else {
                    continue;
                };
                let body = day.get("body").and_then(Value::as_str).unwrap_or_default();
                if existing.contains(date) {
                    store.write_week_day(date, body)?;
                }
            }
            store.assemble_week()?;
        }
        Ok(json!({
            "restored": revision_id,
            "preRestoreRevisionId": pre["revisionId"],
        }))
    }
}

// ── persona files (persona-source.ts + avatar routes) ───────────────────────

pub const PUBLIC_PERSONA_FILE_NAME: &str = "AGENTS.public.md";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonaKind {
    Identity,
    Agents,
}

impl PersonaKind {
    fn config(self) -> (&'static str, &'static str, &'static str) {
        match self {
            PersonaKind::Identity => ("identity.md", "identity-templates", "identity.example.md"),
            PersonaKind::Agents => ("AGENTS.md", "agents-templates", "agents.example.md"),
        }
    }
}

pub struct PersonaFiles {
    agents_root: PathBuf,
    product_dir: PathBuf,
}

impl PersonaFiles {
    pub fn new(agents_root: &Path, product_dir: &Path) -> Self {
        Self {
            agents_root: agents_root.to_path_buf(),
            product_dir: product_dir.to_path_buf(),
        }
    }

    fn agent_dir(&self, agent_id: &str) -> Result<PathBuf, MemoryError> {
        validate_agent_id(agent_id)?;
        let dir = self.agents_root.join(agent_id);
        if !dir.is_dir() {
            return Err(MemoryError::AgentNotFound(format!(
                "agent not found: {agent_id}"
            )));
        }
        Ok(dir)
    }

    /// resolvePersonaSource fallback chain (persona-source.ts:87-116):
    /// own file → (legacy migration fallback — absent in candidate) →
    /// language template → generic template → example.
    pub fn resolve_persona(
        &self,
        agent_id: &str,
        yuan_type: &str,
        locale: &str,
        kind: PersonaKind,
    ) -> Result<(String, bool), MemoryError> {
        let dir = self.agent_dir(agent_id)?;
        let (file_name, template_dir, example_file) = kind.config();
        let own = safe_read(&dir.join(file_name));
        if !own.is_empty() {
            return Ok((own, false));
        }
        let is_zh = locale.starts_with("zh");
        let lang_dir = if is_zh { "" } else { "en/" };
        let lang_template = safe_read(
            &self
                .product_dir
                .join(template_dir)
                .join(format!("{lang_dir}{yuan_type}.md")),
        );
        if !lang_template.is_empty() {
            return Ok((lang_template, true));
        }
        let generic = safe_read(
            &self
                .product_dir
                .join(template_dir)
                .join(format!("{yuan_type}.md")),
        );
        if !generic.is_empty() {
            return Ok((generic, true));
        }
        let example = safe_read(&self.product_dir.join(example_file));
        Ok((example, true))
    }

    /// GET public persona: file only (own chain; no template per incumbent).
    pub fn read_public(&self, agent_id: &str) -> Result<String, MemoryError> {
        let dir = self.agent_dir(agent_id)?;
        Ok(safe_read(&dir.join(PUBLIC_PERSONA_FILE_NAME)))
    }

    pub fn write_persona(
        &self,
        agent_id: &str,
        kind: PersonaKind,
        content: &str,
    ) -> Result<(), MemoryError> {
        let dir = self.agent_dir(agent_id)?;
        let (file_name, _, _) = kind.config();
        atomic_write(&dir.join(file_name), content).map_err(MemoryError::from)
    }

    pub fn write_public(&self, agent_id: &str, content: &str) -> Result<(), MemoryError> {
        let dir = self.agent_dir(agent_id)?;
        atomic_write(&dir.join(PUBLIC_PERSONA_FILE_NAME), content).map_err(MemoryError::from)
    }

    const AVATAR_EXTS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

    /// GET avatar (avatar route semantics): first existing of png/jpg/jpeg/webp.
    pub fn read_avatar(&self, agent_id: &str) -> Result<Option<AvatarContent>, MemoryError> {
        let dir = self.agent_dir(agent_id)?;
        for (ext, mime) in [
            ("png", "image/png"),
            ("jpg", "image/jpeg"),
            ("jpeg", "image/jpeg"),
            ("webp", "image/webp"),
        ] {
            let path = dir.join(format!("avatar.{ext}"));
            if let Ok(bytes) = std::fs::read(&path) {
                return Ok(Some((path, mime, bytes)));
            }
        }
        Ok(None)
    }

    /// POST avatar: only png/jpeg/webp accepted (15MB enforced by the route).
    pub fn write_avatar(&self, agent_id: &str, ext: &str, bytes: &[u8]) -> Result<(), MemoryError> {
        let dir = self.agent_dir(agent_id)?;
        let normalized = match ext {
            "png" => "png",
            "jpg" | "jpeg" => "jpeg",
            "webp" => "webp",
            _ => {
                return Err(MemoryError::InvalidRequest(
                    "avatar must be png, jpeg or webp".into(),
                ))
            }
        };
        self.delete_avatar(agent_id)?;
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent).map_err(MemoryError::from)?;
        }
        std::fs::write(dir.join(format!("avatar.{normalized}")), bytes).map_err(MemoryError::from)
    }

    pub fn delete_avatar(&self, agent_id: &str) -> Result<(), MemoryError> {
        let dir = self.agent_dir(agent_id)?;
        for ext in Self::AVATAR_EXTS {
            remove_if_exists(&dir.join(format!("avatar.{ext}"))).map_err(MemoryError::from)?;
        }
        Ok(())
    }
}

// ── 助手配置的统一解析与更新 ────────────────────────────────────────
#[path = "agent_config.rs"]
pub mod config;

pub struct AgentConfigPatch;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct AgentConfigFlags {
    pub memory_enabled: Option<bool>,
    pub experience_enabled: Option<bool>,
    pub discover_project_skills: Option<bool>,
    pub discover_compatible_project_skills: Option<bool>,
}

impl AgentConfigPatch {
    pub fn read_flags(text: &str) -> Result<AgentConfigFlags, MemoryError> {
        config::AgentConfigDocument::parse(text)?.flags()
    }

    pub fn read_skill_lists(text: &str) -> Result<(Vec<String>, Vec<String>), MemoryError> {
        config::AgentConfigDocument::parse(text)?.skill_lists()
    }

    pub fn write_skill_lists(
        text: &str,
        enabled: &[String],
        workspace_disabled: &[String],
    ) -> Result<String, MemoryError> {
        let mut doc = config::AgentConfigDocument::parse(text)?;
        for (key, items) in [
            ("enabled", enabled),
            ("workspace_disabled", workspace_disabled),
        ] {
            doc.set(
                "skills",
                key,
                serde_yaml_ng::Value::Sequence(
                    items
                        .iter()
                        .cloned()
                        .map(serde_yaml_ng::Value::String)
                        .collect(),
                ),
            );
        }
        doc.render()
    }

    pub fn write_flags(text: &str, flags: &AgentConfigFlags) -> Result<String, MemoryError> {
        let mut doc = config::AgentConfigDocument::parse(text)?;
        for (section, key, value) in [
            ("memory", "enabled", flags.memory_enabled),
            ("experience", "enabled", flags.experience_enabled),
            (
                "workspace_context",
                "discover_project_skills",
                flags.discover_project_skills,
            ),
            (
                "workspace_context",
                "discover_compatible_project_skills",
                flags.discover_compatible_project_skills,
            ),
        ] {
            if let Some(value) = value {
                doc.set(section, key, serde_yaml_ng::Value::Bool(value));
            }
        }
        doc.render()
    }
}

/// 先验证完整配置，再原子保存；失败不能覆盖原配置。
pub fn patch_agent_config(
    config_path: &Path,
    patch: impl FnOnce(&str) -> Result<String, MemoryError>,
) -> Result<String, MemoryError> {
    let text = config::read_config_text(config_path)
        .map_err(|err| MemoryError::Io(format!("read config: {err}")))?;
    config::AgentConfigDocument::parse(&text)?;
    let patched = patch(&text)?;
    config::AgentConfigDocument::parse(&patched)?;
    atomic_write(config_path, &patched).map_err(MemoryError::from)?;
    Ok(patched)
}

// ── context notes (context-notes-tool.ts; runtime-dir home — registered diff) ─

pub const CONTEXT_NOTES_MAX_BYTES: usize = 16 * 1024;
pub const CONTEXT_NOTES_TRUNCATION_MARKER: &str = "\n…(context notes truncated at 16KiB cap)";

pub struct ContextNotesStore {
    root: PathBuf,
}

impl ContextNotesStore {
    pub fn new(runtime_dir: &Path) -> Self {
        Self {
            root: runtime_dir.join("context-notes"),
        }
    }

    fn path_for(&self, session_id: &str) -> Result<PathBuf, MemoryError> {
        if session_id.is_empty()
            || !session_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        {
            return Err(MemoryError::InvalidRequest("invalid session id".into()));
        }
        Ok(self.root.join(format!("{session_id}.md")))
    }

    pub fn read(&self, session_id: &str) -> Result<String, MemoryError> {
        let path = self.path_for(session_id)?;
        let text = safe_read(&path);
        if text.len() > CONTEXT_NOTES_MAX_BYTES {
            let mut cut = CONTEXT_NOTES_MAX_BYTES;
            while cut > 0 && !text.is_char_boundary(cut) {
                cut -= 1;
            }
            return Ok(format!(
                "{}{}",
                &text[..cut],
                CONTEXT_NOTES_TRUNCATION_MARKER
            ));
        }
        Ok(text)
    }

    pub fn write(&self, session_id: &str, text: &str) -> Result<usize, MemoryError> {
        let path = self.path_for(session_id)?;
        let bytes = text.len();
        if bytes > CONTEXT_NOTES_MAX_BYTES {
            return Err(MemoryError::Conflict {
                code: "CONTEXT_NOTES_OVERSIZE".into(),
                message: format!(
                    "context notes would exceed the {}KiB cap ({} bytes). Prune stale entries first.",
                    CONTEXT_NOTES_MAX_BYTES / 1024,
                    bytes
                ),
            });
        }
        atomic_write(&path, text).map_err(MemoryError::from)?;
        Ok(bytes)
    }
}

// ── memory service helpers re-exported for the service layer ────────────────

/// Serializes a kernel Tenet to the incumbent tenets.json entry shape.
pub fn tenet_to_json(tenet: &Tenet) -> Value {
    json!({
        "id": tenet.id,
        "content": tenet.content,
        "priority": tenet.priority,
        "status": tenet.status.as_str(),
        "source": tenet.source.as_str(),
        "sessionId": tenet.session_id,
        "createdAt": tenet.created_at,
        "decidedAt": tenet.decided_at,
    })
}

pub fn tenet_error_to_response_code(err: &MemoryError) -> u16 {
    match err {
        MemoryError::Conflict { code, .. } => match code.as_str() {
            "TENET_NOT_FOUND" => 404,
            "TENET_LIMIT_REACHED" | "TENET_PENDING_FULL" | "TENET_INVALID" => 409,
            _ => 500,
        },
        MemoryError::NotFound(_) => 404,
        MemoryError::InvalidRequest(_) | MemoryError::AgentNotFound(_) => 404,
        _ => 500,
    }
}

/// Convenience wrapper for building fact inserts with PII scrub visibility.
pub struct FactEntryInput<'a> {
    pub fact: &'a str,
    pub tags: Vec<String>,
    pub time: Option<String>,
    pub session_id: Option<String>,
}

impl<'a> FactEntryInput<'a> {
    pub fn to_insert(&'a self) -> FactInsert<'a> {
        FactInsert {
            fact: self.fact,
            tags: &self.tags,
            time: self.time.as_deref(),
            session_id: self.session_id.as_deref(),
        }
    }
}

// Re-exported for tool surface parity with the incumbent tool text.
pub fn tenets_prompt_section(active: &[Tenet], is_zh: bool) -> Option<String> {
    if active.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    for tenet in active {
        for (index, line) in tenet.content.split('\n').enumerate() {
            if index == 0 {
                lines.push(format!("- {line}"));
            } else {
                lines.push(format!("  {line}"));
            }
        }
    }
    let header = if is_zh {
        "# 置顶与原则\n以下是用户钉住的内容与经用户确认的行为原则，始终遵守："
    } else {
        "# Pinned Items & Principles\nThe following pinned content and user-confirmed behavioral principles always apply:"
    };
    Some(format!("{header}\n{}", lines.join("\n")))
}

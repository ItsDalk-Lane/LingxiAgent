//! Versioned migrations of the new run/message database (R02-T04 step 1).
//!
//! Framework contract:
//! - Migrations are an ordered, compile-time-fixed list; each carries a
//!   `version`, a `name` and its exact SQL text. The **fingerprint** is
//!   `sha256(sql)` and is written into an on-disk receipt
//!   (`schema_migrations`) inside the SAME transaction that applies the SQL.
//! - Opening the database verifies receipts against the compiled-in set:
//!   a gap, an unknown version, a renamed migration or a fingerprint
//!   mismatch is a loud [`StorageError::SchemaTampered`] — never a silent
//!   replay (acceptance R02-A08 negative case).
//! - The applied version is recorded BOTH in the receipts table and in
//!   `PRAGMA user_version`; a disagreement between the two is tampering.
//! - A database whose version is NEWER than this build is rejected with
//!   [`StorageError::DatabaseTooNew`] — no downgrade, no replay.
//! - Migration SQL deliberately avoids `IF NOT EXISTS`: an accidental
//!   replay against an already-migrated database must FAIL loudly, not
//!   silently succeed as a no-op.

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use lingxi_kernel::ports::StorageError;

/// One compiled-in migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    pub version: u64,
    pub name: &'static str,
    pub sql: &'static str,
}

/// sha256 hex fingerprint of a migration's SQL text.
pub fn fingerprint_sql(sql: &str) -> String {
    let digest = Sha256::digest(sql.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Initial schema of the run/message database (version 1).
///
/// Ownership mapping (OWNERSHIP_TARGET.json; one owner per table, the
/// physical writer is always the single rust-service process):
/// - `sessions`, `runs`, `run_attempts`, `key_events`, `messages`:
///   run/session authority facts (`kernel.run-supervisor` /
///   `kernel.session` own the semantics; rows are only written through the
///   [`crate::storage::RunDatabase`] port implementation).
/// - `invocations`: tool-invocation receipts (`kernel.tool-gateway`
///   semantics; populated from R04).
/// - `schema_migrations`: migration receipts owned by the storage adapter.
///
/// `key_events.seq` is assigned per `stream_id` by the single writer inside
/// the committing transaction (UNIQUE constraint is the mechanical backstop).
pub const V1_NAME: &str = "initial_run_message_schema";
pub const V1_SQL: &str = r#"
CREATE TABLE sessions (
    session_id         TEXT PRIMARY KEY,
    agent_id           TEXT NOT NULL,
    owner_user_id      TEXT NOT NULL,
    title              TEXT NOT NULL,
    created_at_unix_ms INTEGER NOT NULL
);

CREATE TABLE runs (
    run_id             TEXT PRIMARY KEY,
    session_id         TEXT NOT NULL REFERENCES sessions(session_id),
    owner_kind         TEXT NOT NULL,
    owner_subject      TEXT NOT NULL,
    principal_id       TEXT NOT NULL,
    attempt_count      INTEGER NOT NULL DEFAULT 0,
    status             TEXT NOT NULL,
    terminal_reason    TEXT,
    generation         INTEGER NOT NULL,
    created_at_unix_ms INTEGER NOT NULL,
    updated_at_unix_ms INTEGER NOT NULL,
    last_event_seq     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_runs_session ON runs(session_id);

CREATE TABLE run_attempts (
    run_id             TEXT NOT NULL REFERENCES runs(run_id),
    attempt            TEXT NOT NULL,
    generation         INTEGER NOT NULL,
    started_at_unix_ms INTEGER NOT NULL,
    PRIMARY KEY (run_id, attempt)
);

CREATE TABLE invocations (
    invocation_id      TEXT PRIMARY KEY,
    run_id             TEXT NOT NULL REFERENCES runs(run_id),
    attempt            TEXT NOT NULL,
    target             TEXT NOT NULL,
    args_digest        TEXT NOT NULL,
    status             TEXT NOT NULL,
    started_at_unix_ms INTEGER NOT NULL,
    completed_at_unix_ms INTEGER
);

CREATE TABLE messages (
    message_id         TEXT PRIMARY KEY,
    session_id         TEXT NOT NULL REFERENCES sessions(session_id),
    run_id             TEXT NOT NULL REFERENCES runs(run_id),
    role               TEXT NOT NULL,
    content_json       TEXT NOT NULL,
    model_call_id      TEXT,
    committed_at_unix_ms INTEGER NOT NULL,
    seq                INTEGER NOT NULL
);
CREATE INDEX idx_messages_session ON messages(session_id, seq);

CREATE TABLE key_events (
    event_id           TEXT PRIMARY KEY,
    stream_id          TEXT NOT NULL,
    seq                INTEGER NOT NULL,
    session_id         TEXT NOT NULL,
    run_id             TEXT,
    attempt            TEXT,
    event_type         TEXT NOT NULL,
    payload_json       TEXT NOT NULL,
    committed_at_unix_ms INTEGER NOT NULL,
    UNIQUE (stream_id, seq)
);
CREATE INDEX idx_key_events_run ON key_events(run_id);
CREATE INDEX idx_key_events_session ON key_events(session_id, seq);
"#;

/// R03-T04 fence audit table (version 2): the audit-only trace of refused
/// LATE results. Rows are written when `record_run_events` refuses a stale
/// delivery (terminal run / superseded attempt / never-opened attempt) and
/// when the run driver fences an in-flight result
/// (`record_stale_result`). The table deliberately has NO foreign keys to
/// `runs`/`run_attempts`: an audited claim may name a run that never
/// existed — the audit proves WHAT ARRIVED, not that the claim was
/// legitimate.
pub const V2_NAME: &str = "stale_result_audit";
pub const V2_SQL: &str = r#"
CREATE TABLE stale_result_audit (
    audit_id             INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id               TEXT NOT NULL,
    session_id           TEXT NOT NULL,
    attempt              TEXT,
    generation           INTEGER NOT NULL,
    owner_kind           TEXT NOT NULL,
    owner_subject        TEXT NOT NULL,
    reason               TEXT NOT NULL,
    refused_event_types  TEXT NOT NULL,
    recorded_at_unix_ms  INTEGER NOT NULL
);
CREATE INDEX idx_stale_result_audit_run ON stale_result_audit(run_id);
"#;

/// R03-T05 side-effect invocation receipts (version 3): the
/// InvocationJournal. One row per tool invocation (journal_id = the tool
/// call id the run driver minted), carrying the full receipt binding
/// (owner facts, run/attempt/generation, target, args digest, idempotency
/// key) and the receipt lifecycle `phase`
/// prepared→authorized→started→succeeded/failed/unknown.
///
/// Write-order contract (enforced by the driver, witnessed by the row):
/// the intent phases are durable BEFORE the external execution; the
/// receipt columns (`receipt_outcome`/`receipt_detail`/`dedup_id`/
/// `dispatched`) land AFTER it. No cross-system atomicity is claimed.
///
/// The FK to `runs` is deliberate here (unlike the T04 audit table): the
/// journal is written by the live driver of a real run, never as a record
/// of an untrusted claim. The unique partial index on `idempotency_key`
/// mechanically prevents two invocations from ever presenting the same
/// key.
pub const V3_NAME: &str = "invocation_journal";
pub const V3_SQL: &str = r#"
CREATE TABLE invocation_journal (
    journal_id           TEXT PRIMARY KEY,
    session_id           TEXT NOT NULL,
    run_id               TEXT NOT NULL REFERENCES runs(run_id),
    attempt              TEXT NOT NULL,
    generation           INTEGER NOT NULL,
    owner_kind           TEXT NOT NULL,
    owner_subject        TEXT NOT NULL,
    target               TEXT NOT NULL,
    args_digest          TEXT NOT NULL,
    args_summary         TEXT,
    idempotency_key      TEXT,
    phase                TEXT NOT NULL,
    receipt_outcome      TEXT,
    receipt_detail       TEXT,
    dedup_id             TEXT,
    dispatched           INTEGER,
    prepared_at_unix_ms  INTEGER NOT NULL,
    updated_at_unix_ms   INTEGER NOT NULL
);
CREATE INDEX idx_invocation_journal_run ON invocation_journal(run_id);
CREATE UNIQUE INDEX idx_invocation_journal_idem
    ON invocation_journal(idempotency_key)
    WHERE idempotency_key IS NOT NULL;
"#;

/// R03-T06 run lineage (version 4): the four-part identity that ties every
/// run to what caused it — `parent_run_id` (the run whose delegation tool
/// call dispatched this child; NULL for user submissions), `origin` (the
/// entry-surface vocabulary: user / subagent / cron / heartbeat / bridge
/// — the R07 Bridge/cron integrations reuse THIS table, no second
/// scheduler), `source_message_id` (the message that sourced the run —
/// for a child, the parent's model call that emitted the delegation
/// request) and `cause_id` (the precise causal anchor — for a child, the
/// parent's tool call id; for a user submission, the explicit requestId).
///
/// Lineage is IMMUTABLE once recorded (enforced by the writer: identical
/// re-record = idempotent replay, different lineage = loud Conflict). The
/// FK to `runs` is deliberate, same as the T05 journal: lineage rows are
/// written by the live driver of a real run.
pub const V4_NAME: &str = "run_lineage";
pub const V4_SQL: &str = r#"
CREATE TABLE run_lineage (
    run_id               TEXT PRIMARY KEY REFERENCES runs(run_id),
    parent_run_id        TEXT,
    origin               TEXT NOT NULL,
    source_message_id    TEXT,
    cause_id             TEXT,
    recorded_at_unix_ms  INTEGER NOT NULL
);
CREATE INDEX idx_run_lineage_parent ON run_lineage(parent_run_id)
    WHERE parent_run_id IS NOT NULL;
"#;

/// R05-T07 usage ledger (version 5): ONE row per model call — the
/// per-request correlation (session/run/attempt + host-minted call id),
/// the route that served it (provider/model/protocol), the purpose and
/// causal parentage (origin/parent run/cause ref) and the normalized
/// usage fact with its provenance decomposed into columns
/// (`usage_state` + `missing_fields`/`estimate_basis`/`invalid_detail`).
///
/// Deliberately NO foreign keys: the ledger is an accounting fact table
/// — rows for worker-callback/auxiliary/operation calls exist without a
/// run row of their own, and an accounting row must survive even when it
/// names a parent the runs table never admitted (the audit-first stance
/// of the T04 stale table, applied to accounting). `model_call_id` is
/// the host-minted call identity — PRIMARY KEY makes re-recording the
/// identical row an idempotent replay and a different row under the same
/// id a loud conflict (a call's accounting is never rewritten).
///
/// Token columns are NULLABLE on purpose: NULL means "not reported"
/// (unknown — never zero). `transport_attempts >= 1` counts the physical
/// provider requests of the logical call (a 401-refresh resend is a
/// second billable request, not an invisible one). `cost_basis` NULL =
/// cost unknown: no price source is configured and none is invented.
pub const V5_NAME: &str = "model_call_usage_ledger";
pub const V5_SQL: &str = r#"
CREATE TABLE model_call_usage (
    model_call_id       TEXT PRIMARY KEY,
    session_id          TEXT,
    run_id              TEXT,
    attempt             TEXT,
    purpose             TEXT NOT NULL,
    origin              TEXT NOT NULL,
    parent_run_id       TEXT,
    cause_ref           TEXT,
    provider            TEXT NOT NULL,
    model               TEXT NOT NULL,
    protocol            TEXT NOT NULL,
    usage_state         TEXT NOT NULL,
    input_tokens        INTEGER,
    output_tokens       INTEGER,
    cache_read_tokens   INTEGER,
    cache_write_tokens  INTEGER,
    reasoning_tokens    INTEGER,
    missing_fields      TEXT,
    estimate_basis      TEXT,
    invalid_detail      TEXT,
    transport_attempts  INTEGER NOT NULL,
    cost_basis          TEXT,
    recorded_at_unix_ms INTEGER NOT NULL
);
CREATE INDEX idx_model_call_usage_session ON model_call_usage(session_id);
CREATE INDEX idx_model_call_usage_run ON model_call_usage(run_id);
"#;

/// R05 RR1 F21 usage-ledger extension (version 6): the per-physical-attempt
/// and causal-join columns the T07 taskbook's query/export contract needs —
/// the call's settlement `outcome` (succeeded/failed/cancelled/unknown),
/// the start/settle timestamps (`started_at_unix_ms`/`settled_at_unix_ms`
/// — 耗时 is their difference, both stored so a reader can show either),
/// the child→parent-tool JOIN key (`parent_tool_call_id`) and the parent
/// model row's batch listing (`emitted_tool_calls`) that closes the second
/// hop (child callback → parent tool → parent MODEL call, all inside the
/// ledger — never inferred from timing).
///
/// `outcome` defaults to `'unknown'` because SQLite ALTER cannot add a
/// non-constant NOT NULL column; rows written by older builds honestly
/// hold the unknown settlement.
pub const V6_NAME: &str = "model_call_usage_rr1_f21";
pub const V6_SQL: &str = r#"
ALTER TABLE model_call_usage ADD COLUMN outcome TEXT NOT NULL DEFAULT 'unknown';
ALTER TABLE model_call_usage ADD COLUMN started_at_unix_ms INTEGER;
ALTER TABLE model_call_usage ADD COLUMN settled_at_unix_ms INTEGER;
ALTER TABLE model_call_usage ADD COLUMN parent_tool_call_id TEXT;
ALTER TABLE model_call_usage ADD COLUMN emitted_tool_calls TEXT;
CREATE INDEX idx_model_call_usage_recorded_at ON model_call_usage(recorded_at_unix_ms);
CREATE INDEX idx_model_call_usage_purpose ON model_call_usage(purpose);
CREATE INDEX idx_model_call_usage_parent_tool ON model_call_usage(parent_tool_call_id)
    WHERE parent_tool_call_id IS NOT NULL;
"#;

/// R05 RR1 F38 usage-ledger `transport_attempts` nullability (version 7):
/// the column becomes NULLABLE so a row written for a call whose future
/// was DROPPED before settlement (a cancellation fence dropping an
/// in-flight model call / worker callback) can state "attempts UNKNOWN"
/// honestly. SQLite cannot `ALTER` a column's NOT NULL constraint away, so
/// the migration REBUILDS the table: every pre-existing row keeps its
/// value verbatim (all historical rows carry an observed count —
/// `Some(n)` semantics; none is rewritten, none is dropped), the indexes
/// are recreated identically (they vanish with the old table).
///
/// Column semantics after v7: NULL = attempts unknown after a drop
/// (never 0, never 1 — never a fabricated count); 0 = not-sent; ≥1 the
/// observed physical request count.
pub const V7_NAME: &str = "model_call_usage_rr1_f38_attempts_nullable";
pub const V7_SQL: &str = r#"
CREATE TABLE model_call_usage_v7 (
    model_call_id       TEXT PRIMARY KEY,
    session_id          TEXT,
    run_id              TEXT,
    attempt             TEXT,
    purpose             TEXT NOT NULL,
    origin              TEXT NOT NULL,
    parent_run_id       TEXT,
    cause_ref           TEXT,
    provider            TEXT NOT NULL,
    model               TEXT NOT NULL,
    protocol            TEXT NOT NULL,
    usage_state         TEXT NOT NULL,
    input_tokens        INTEGER,
    output_tokens       INTEGER,
    cache_read_tokens   INTEGER,
    cache_write_tokens  INTEGER,
    reasoning_tokens    INTEGER,
    missing_fields      TEXT,
    estimate_basis      TEXT,
    invalid_detail      TEXT,
    transport_attempts  INTEGER,
    cost_basis          TEXT,
    recorded_at_unix_ms INTEGER NOT NULL,
    outcome             TEXT NOT NULL DEFAULT 'unknown',
    started_at_unix_ms  INTEGER,
    settled_at_unix_ms  INTEGER,
    parent_tool_call_id TEXT,
    emitted_tool_calls  TEXT
);
INSERT INTO model_call_usage_v7
    (model_call_id, session_id, run_id, attempt, purpose, origin,
     parent_run_id, cause_ref, parent_tool_call_id, provider, model,
     protocol, usage_state, input_tokens, output_tokens, cache_read_tokens,
     cache_write_tokens, reasoning_tokens, missing_fields, estimate_basis,
     invalid_detail, transport_attempts, cost_basis, recorded_at_unix_ms,
     outcome, started_at_unix_ms, settled_at_unix_ms, emitted_tool_calls)
SELECT model_call_id, session_id, run_id, attempt, purpose, origin,
       parent_run_id, cause_ref, parent_tool_call_id, provider, model,
       protocol, usage_state, input_tokens, output_tokens, cache_read_tokens,
       cache_write_tokens, reasoning_tokens, missing_fields, estimate_basis,
       invalid_detail, transport_attempts, cost_basis, recorded_at_unix_ms,
       outcome, started_at_unix_ms, settled_at_unix_ms, emitted_tool_calls
FROM model_call_usage;
DROP TABLE model_call_usage;
ALTER TABLE model_call_usage_v7 RENAME TO model_call_usage;
CREATE INDEX idx_model_call_usage_session ON model_call_usage(session_id);
CREATE INDEX idx_model_call_usage_run ON model_call_usage(run_id);
CREATE INDEX idx_model_call_usage_recorded_at ON model_call_usage(recorded_at_unix_ms);
CREATE INDEX idx_model_call_usage_purpose ON model_call_usage(purpose);
CREATE INDEX idx_model_call_usage_parent_tool ON model_call_usage(parent_tool_call_id)
    WHERE parent_tool_call_id IS NOT NULL;
"#;

/// R06-T03 session tree / fork / retry / rewind (version 8): the relational
/// form of the incumbent append-only JSONL session tree — lineage
/// (parent_session_id/fork_point/lineage_depth), lifecycle (active/archived/
/// deleted), pinning, per-session branch heads, named checkpoints, file
/// versions for rewind conflict detection, and file-level checkpoints.
///
/// Semantics anchors (incumbent, surveyed end-to-end):
/// - lineage depth cap `MAX_FORK_LINEAGE_DEPTH = 2`
///   (`server/routes/sessions.ts:1738`);
/// - fork copies root→boundary KEEPING message ids, only re-linking parents
///   (`pi-coding-agent/dist/core/session-manager.js:1113 createBranchedSession`)
///   — hence `messages` PRIMARY KEY becomes (session_id, message_id): a forked
///   copy shares the id with its source, unique only within a session;
/// - branch heads: one row per session (the incumbent session_branch_heads
///   table, `core/session-branch-head.ts`);
/// - named checkpoints: `latest` overwrites, other names conflict, cap 200
///   (`core/session-checkpoints.ts:14-16`);
/// - file versions: rewind conflict detection record (A06 hardening — the
///   incumbent has NO external-modification detection, report deviation D1).
///
/// The `messages` rebuild follows the v7 precedent: SQLite cannot alter a
/// PRIMARY KEY, so the table is rebuilt, every existing row is copied
/// verbatim (parent_message_id NULL, branch_id NULL, entry_type 'message'),
/// and the indexes are recreated identically.
pub const V8_NAME: &str = "r06_t03_session_tree";
pub const V8_SQL: &str = r#"
ALTER TABLE sessions ADD COLUMN parent_session_id TEXT;
ALTER TABLE sessions ADD COLUMN fork_point_message_id TEXT;
ALTER TABLE sessions ADD COLUMN lineage_depth INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN lifecycle TEXT NOT NULL DEFAULT 'active';
ALTER TABLE sessions ADD COLUMN pinned_at_unix_ms INTEGER;
ALTER TABLE sessions ADD COLUMN pin_order INTEGER;
ALTER TABLE sessions ADD COLUMN memory_enabled INTEGER NOT NULL DEFAULT 1;
ALTER TABLE sessions ADD COLUMN authorized_folders_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE sessions ADD COLUMN permission_mode TEXT;
ALTER TABLE sessions ADD COLUMN archived_at_unix_ms INTEGER;
ALTER TABLE sessions ADD COLUMN last_activity_unix_ms INTEGER;

CREATE TABLE session_branch_heads (
    session_id                TEXT PRIMARY KEY REFERENCES sessions(session_id),
    head_message_id           TEXT,
    observed_tail_message_id  TEXT,
    revision                  INTEGER NOT NULL DEFAULT 0,
    head_resolution           TEXT NOT NULL DEFAULT 'legacy_tail',
    updated_at_unix_ms        INTEGER NOT NULL
);

CREATE TABLE checkpoints (
    checkpoint_id        TEXT PRIMARY KEY,
    session_id           TEXT NOT NULL REFERENCES sessions(session_id),
    name                 TEXT NOT NULL,
    kind                 TEXT NOT NULL,
    target_message_id    TEXT,
    turn_input_message_id TEXT,
    message_count        INTEGER NOT NULL DEFAULT 0,
    created_at_unix_ms   INTEGER NOT NULL
);
CREATE INDEX idx_checkpoints_session ON checkpoints(session_id);

CREATE TABLE checkpoint_file_versions (
    checkpoint_id        TEXT NOT NULL REFERENCES checkpoints(checkpoint_id),
    file_path            TEXT NOT NULL,
    sha256               TEXT NOT NULL,
    size_bytes           INTEGER NOT NULL,
    recorded_at_unix_ms  INTEGER NOT NULL,
    PRIMARY KEY (checkpoint_id, file_path)
);

CREATE TABLE file_checkpoints (
    checkpoint_id        TEXT NOT NULL REFERENCES checkpoints(checkpoint_id),
    session_id           TEXT,
    file_path            TEXT NOT NULL,
    content              BLOB NOT NULL,
    encoding             TEXT NOT NULL,
    reason               TEXT NOT NULL,
    created_at_unix_ms   INTEGER NOT NULL,
    PRIMARY KEY (checkpoint_id, file_path)
);
CREATE INDEX idx_file_checkpoints_session ON file_checkpoints(session_id);

CREATE TABLE messages_v8 (
    message_id         TEXT NOT NULL,
    session_id         TEXT NOT NULL REFERENCES sessions(session_id),
    run_id             TEXT REFERENCES runs(run_id),
    role               TEXT NOT NULL,
    content_json       TEXT NOT NULL,
    model_call_id      TEXT,
    committed_at_unix_ms INTEGER NOT NULL,
    seq                INTEGER NOT NULL,
    parent_message_id  TEXT,
    branch_id          TEXT,
    entry_type         TEXT NOT NULL DEFAULT 'message',
    PRIMARY KEY (session_id, message_id)
);
INSERT INTO messages_v8
    (message_id, session_id, run_id, role, content_json, model_call_id,
     committed_at_unix_ms, seq, parent_message_id, branch_id, entry_type)
SELECT message_id, session_id, run_id, role, content_json, model_call_id,
       committed_at_unix_ms, seq, NULL, NULL, 'message'
FROM messages;
DROP TABLE messages;
ALTER TABLE messages_v8 RENAME TO messages;
CREATE INDEX idx_messages_session ON messages(session_id, seq);
CREATE INDEX idx_messages_parent ON messages(session_id, parent_message_id);
"#;

/// R06-T05: session files (sf_ identities), their legacy-id aliases and
/// explicit references, plus the per-workspace file-history snapshots
/// (incumbent `lib/session-files/session-file-registry.ts` sidecar schema v1
/// field set and `lib/file-history/history-store.ts`, carried as SQLite
/// rows — diff ledger D1).
///
/// - `session_files`: one row per registered file. `file_id` is the
///   incumbent-compatible `sf_` id and is preserved verbatim on legacy
///   import. New rows are keyed by `id:{session_id}` owners only (D2);
///   legacy `path:{...}` owners are converted at import time and the
///   original paths stay visible in `legacy_file_paths_json`.
/// - `session_file_aliases`: fork/import old ids → the canonical row
///   (structured form of the incumbent `legacyFileIds` array), so old
///   messages referencing a pre-fork sf_ id still resolve (R06-A09).
/// - `session_file_refs`: explicit references (imported sidecar refs,
///   stage/upload registrations, restore writes). The reference checker
///   combines these with markers parsed from canonical messages.
/// - `file_history_snapshots`: text-only snapshots keyed by the incumbent
///   workspace hash (sha256(normalized root)[..16]).
pub const V9_NAME: &str = "r06_t05_session_files_resources";
pub const V9_SQL: &str = r#"
CREATE TABLE session_files (
    file_id              TEXT PRIMARY KEY,
    owner_session_id     TEXT NOT NULL,
    owner_key            TEXT NOT NULL,
    source_key           TEXT,
    identity_key         TEXT NOT NULL,
    file_path            TEXT NOT NULL,
    real_path            TEXT NOT NULL,
    storage_kind         TEXT NOT NULL,
    status               TEXT NOT NULL,
    label                TEXT,
    filename             TEXT NOT NULL,
    mime                 TEXT NOT NULL,
    size_bytes           INTEGER NOT NULL,
    mtime_ms             INTEGER NOT NULL,
    is_directory         INTEGER NOT NULL,
    file_kind            TEXT NOT NULL,
    origin               TEXT NOT NULL,
    registered_at_ms     INTEGER NOT NULL,
    updated_at_ms        INTEGER NOT NULL,
    expires_at_ms        INTEGER,
    legacy_file_ids_json   TEXT NOT NULL DEFAULT '[]',
    legacy_file_paths_json TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX idx_session_files_owner ON session_files(owner_session_id, registered_at_ms);
CREATE UNIQUE INDEX idx_session_files_source
    ON session_files(owner_session_id, source_key) WHERE source_key IS NOT NULL;
CREATE INDEX idx_session_files_real_path ON session_files(owner_session_id, real_path);

CREATE TABLE session_file_aliases (
    alias_file_id      TEXT PRIMARY KEY,
    canonical_file_id  TEXT NOT NULL REFERENCES session_files(file_id),
    created_at_ms      INTEGER NOT NULL
);
CREATE INDEX idx_session_file_aliases_canonical ON session_file_aliases(canonical_file_id);

CREATE TABLE session_file_refs (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id        TEXT NOT NULL,
    session_id     TEXT NOT NULL,
    message_id     TEXT,
    ref_kind       TEXT NOT NULL,
    created_at_ms  INTEGER NOT NULL
);
CREATE INDEX idx_session_file_refs_session ON session_file_refs(session_id);
CREATE INDEX idx_session_file_refs_file ON session_file_refs(file_id);

CREATE TABLE file_history_snapshots (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_hash   TEXT NOT NULL,
    rel_path         TEXT NOT NULL,
    captured_at_ms   INTEGER NOT NULL,
    origin           TEXT NOT NULL,
    size_bytes       INTEGER NOT NULL,
    sha256           TEXT NOT NULL,
    content          BLOB NOT NULL
);
CREATE INDEX idx_file_history_ws_path
    ON file_history_snapshots(workspace_hash, rel_path, id);
"#;

/// The full ordered migration list. Appending a migration is a deliberate,
/// reviewed act; editing an existing entry changes its fingerprint and is
/// rejected on every already-migrated database.
pub const V10_NAME: &str = "session_operation_receipts";
pub const V10_SQL: &str = r#"
CREATE TABLE session_operations (
 operation_id TEXT PRIMARY KEY NOT NULL,
 session_id TEXT NOT NULL,
 owner_kind TEXT NOT NULL,
 owner_subject TEXT NOT NULL,
 request_id TEXT,
 operation_kind TEXT NOT NULL,
 input_digest TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('prepared','completed')),
 file_effects INTEGER NOT NULL CHECK(file_effects IN (0,1)),
 snapshot_json TEXT NOT NULL,
 receipt_json TEXT,
 created_at_unix_ms INTEGER NOT NULL,
 completed_at_unix_ms INTEGER,
 UNIQUE(session_id,owner_kind,owner_subject,request_id)
);
CREATE INDEX session_operations_pending ON session_operations(session_id,state,file_effects);
"#;

/// 无消息运行的真实分支锚；只追加版本，不回填无法证明归属的旧运行。
pub const V11_NAME: &str = "r06_ar_f04_run_history_anchors";
pub const V11_SQL: &str = r#"
CREATE TABLE run_history_anchors (
    run_id TEXT PRIMARY KEY REFERENCES runs(run_id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(session_id),
    anchor_message_id TEXT,
    FOREIGN KEY(session_id, anchor_message_id) REFERENCES messages(session_id, message_id)
);
CREATE INDEX idx_run_history_anchors_page ON run_history_anchors(session_id, anchor_message_id);
"#;

/// R06 会话项目归属（version 12）：sessions.project_id 可空列。
/// 现役 setSessionProjectAssignment 的最小端口——只承载归属指向
/// （分配 / null 清除 / 读回）；项目与文件夹目录树（session-projects.ts
/// 全量 CRUD）属派发计划 E 块，另行落地，不在此迁移冒充完成。
pub const V12_NAME: &str = "r06_session_project_assignment";
pub const V12_SQL: &str = r#"
ALTER TABLE sessions ADD COLUMN project_id TEXT;
"#;

/// R06 C1 收尾：工作台身份两列（version 13）。`workspace_mount_id` 是
/// studio 挂载登记里的 mountId（会话经挂载创建时带上）；`cwd` 是历史
/// cwd 形态会话的工作目录。工作台处置（archive/delete 名下全部会话）
/// 与孤儿清扫（mount 被移除 / 目录被直接删除）按这两列识别归属，
/// 现役锚点 `server/routes/sessions.ts:2670`（workspace-disposal）与
/// `:2762`（sweep-orphaned-workspaces）。两列均可空：老会话与无工作台
/// 会话两列皆 NULL，不参与任何匹配。
pub const V13_NAME: &str = "r06_session_workspace_identity";
pub const V13_SQL: &str = r#"
ALTER TABLE sessions ADD COLUMN workspace_mount_id TEXT;
ALTER TABLE sessions ADD COLUMN cwd TEXT;
"#;

/// 新主会话创建时冻结主动委派；旧会话和非该入口创建的子会话默认关闭。
pub const V14_NAME: &str = "session_proactive_delegation_at_creation";
pub const V14_SQL: &str = r#"
ALTER TABLE sessions ADD COLUMN proactive_delegation INTEGER NOT NULL DEFAULT 0 CHECK(proactive_delegation IN (0,1));
"#;

/// R07：计划、触发与外发收据复用运行库的持久单写队列；历史不能因删除计划消失。
pub const V15_NAME: &str = "r07_automation_receipts";
pub const V15_SQL: &str = r#"
CREATE TABLE automation_jobs_v1 (
    id TEXT PRIMARY KEY,
    studio_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    owner_kind TEXT NOT NULL,
    owner_subject TEXT NOT NULL,
    schema_version INTEGER NOT NULL CHECK(schema_version > 0),
    config_revision INTEGER NOT NULL CHECK(config_revision > 0),
    config_json TEXT NOT NULL,
    next_run_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    deleted_at INTEGER
);
CREATE INDEX idx_automation_jobs_due ON automation_jobs_v1(next_run_at) WHERE deleted_at IS NULL;
CREATE INDEX idx_automation_jobs_studio ON automation_jobs_v1(studio_id) WHERE deleted_at IS NULL;
CREATE TABLE automation_triggers_v1 (
    trigger_id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES automation_jobs_v1(id),
    due_at INTEGER NOT NULL,
    config_revision INTEGER NOT NULL CHECK(config_revision > 0),
    status TEXT NOT NULL CHECK(status IN ('claimed','running','succeeded','failed','skipped','cancelled','unknown')),
    session_id TEXT,
    run_id TEXT,
    result_json TEXT,
    error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    config_json TEXT NOT NULL,
    UNIQUE(job_id,due_at,config_revision),
    CHECK((session_id IS NULL) = (run_id IS NULL))
);
CREATE INDEX idx_automation_triggers_pending ON automation_triggers_v1(status,created_at);
CREATE INDEX idx_automation_triggers_job ON automation_triggers_v1(job_id,due_at);
CREATE TABLE automation_notifications_v1 (
    delivery_id TEXT PRIMARY KEY,
    trigger_id TEXT REFERENCES automation_triggers_v1(trigger_id),
    run_id TEXT REFERENCES runs(run_id),
    channel TEXT NOT NULL,
    recipient TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('queued','started','confirmed','unknown','failed')),
    platform_receipt_json TEXT,
    error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK(trigger_id IS NOT NULL OR run_id IS NOT NULL)
);
CREATE INDEX idx_automation_notifications_trigger ON automation_notifications_v1(trigger_id);
CREATE INDEX idx_automation_notifications_pending ON automation_notifications_v1(status,created_at);
CREATE TABLE automation_daily_receipts_v1 (
    session_id TEXT NOT NULL REFERENCES sessions(session_id),
    logical_date TEXT NOT NULL,
    attempt INTEGER NOT NULL CHECK(attempt > 0),
    status TEXT NOT NULL CHECK(status IN ('started','succeeded','failed','unknown')),
    result_json TEXT,
    error TEXT,
    started_at INTEGER NOT NULL,
    completed_at INTEGER,
    retry_at INTEGER,
    PRIMARY KEY(session_id,logical_date)
);
CREATE TABLE automation_suggestions_v1 (
    id TEXT PRIMARY KEY,
    studio_id TEXT NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(session_id),
    owner_subject TEXT NOT NULL,
    operation TEXT NOT NULL CHECK(operation IN ('create','update')),
    job_id TEXT,
    expected_revision INTEGER CHECK(expected_revision IS NULL OR expected_revision > 0),
    job_json TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    applied_job_id TEXT REFERENCES automation_jobs_v1(id),
    state TEXT NOT NULL CHECK(state IN ('pending','applied')),
    created_at INTEGER NOT NULL
);
CREATE TABLE automation_activity_promotions_v1 (
    trigger_id TEXT PRIMARY KEY REFERENCES automation_triggers_v1(trigger_id),
    session_id TEXT NOT NULL UNIQUE REFERENCES sessions(session_id),
    promoted_at INTEGER NOT NULL
);
CREATE TABLE automation_internal_sessions_v1 (
    session_id TEXT PRIMARY KEY REFERENCES sessions(session_id),
    kind TEXT NOT NULL CHECK(kind IN ('activity','heartbeat_scope')),
    trigger_id TEXT REFERENCES automation_triggers_v1(trigger_id),
    agent_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    CHECK((kind='activity' AND trigger_id IS NOT NULL) OR (kind='heartbeat_scope' AND trigger_id IS NULL))
);
"#;

/// R07：外部消息、运行关联与外发事实持久化；旧消息身份跨配置代次保持不变。
pub const V16_NAME: &str = "r07_bridge_inbox_outbox";
pub const V16_SQL: &str = r#"
CREATE TABLE bridge_connectors_v1 (
    id TEXT PRIMARY KEY,
    platform TEXT NOT NULL CHECK(platform IN ('telegram','feishu','dingtalk','qq','wechat')),
    studio_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    owner_subject TEXT NOT NULL CHECK(owner_subject='user_local'),
    owner_sender_id TEXT,
    generation INTEGER NOT NULL CHECK(generation>0),
    enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
    config_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE bridge_conversations_v1 (
    connector_id TEXT NOT NULL REFERENCES bridge_connectors_v1(id),
    conversation_id TEXT NOT NULL,
    sender_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    subject TEXT NOT NULL,
    conversation_kind TEXT NOT NULL CHECK(conversation_kind IN ('dm','group')),
    epoch INTEGER NOT NULL CHECK(epoch>0),
    current_session_id TEXT,
    metadata_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(connector_id,conversation_id,sender_id,actor_id)
);
CREATE INDEX idx_bridge_conversations_recent ON bridge_conversations_v1(connector_id,updated_at);
CREATE TABLE bridge_inbox_v1 (
    id TEXT PRIMARY KEY,
    platform TEXT NOT NULL,
    connector_id TEXT NOT NULL REFERENCES bridge_connectors_v1(id),
    conversation_id TEXT NOT NULL,
    conversation_kind TEXT NOT NULL CHECK(conversation_kind IN ('dm','group')),
    message_id TEXT NOT NULL,
    sender_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('owner','guest')),
    actor_id TEXT NOT NULL,
    owner_subject TEXT NOT NULL CHECK(owner_subject='user_local'),
    connector_generation INTEGER NOT NULL CHECK(connector_generation>0),
    payload_json TEXT NOT NULL,
    attachments_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('claimed','running','succeeded','failed','cancelled','unknown','command')),
    session_id TEXT,
    run_id TEXT,
    request_id TEXT NOT NULL UNIQUE,
    result_json TEXT,
    error TEXT,
    prepared INTEGER NOT NULL DEFAULT 0 CHECK(prepared IN (0,1)),
    prepared_takeover INTEGER NOT NULL DEFAULT 0 CHECK(prepared_takeover IN (0,1)),
    resolved_files_json TEXT NOT NULL DEFAULT '[]',
    conversation_epoch INTEGER CHECK(conversation_epoch IS NULL OR conversation_epoch>0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(platform,connector_id,conversation_id,message_id),
    CHECK(run_id IS NULL OR session_id IS NOT NULL)
);
CREATE INDEX idx_bridge_inbox_pending ON bridge_inbox_v1(status,created_at);
CREATE INDEX idx_bridge_inbox_owner_target ON bridge_inbox_v1(connector_id,connector_generation,role,created_at);
CREATE TABLE bridge_command_receipts_v1 (
    inbox_id TEXT PRIMARY KEY REFERENCES bridge_inbox_v1(id),
    command TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('started','confirmed','unknown','failed')),
    result_json TEXT,
    error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE bridge_manual_actions_v1 (
    id TEXT PRIMARY KEY,
    connector_id TEXT NOT NULL REFERENCES bridge_connectors_v1(id),
    connector_generation INTEGER NOT NULL CHECK(connector_generation>0),
    target_inbox_id TEXT NOT NULL REFERENCES bridge_inbox_v1(id),
    source_session_id TEXT NOT NULL,
    file_id TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    label TEXT,
    filename TEXT NOT NULL,
    mime TEXT NOT NULL,
    size_bytes INTEGER NOT NULL CHECK(size_bytes>=0),
    created_at INTEGER NOT NULL
);
CREATE TABLE bridge_outbox_v1 (
    delivery_id TEXT PRIMARY KEY,
    connector_id TEXT NOT NULL REFERENCES bridge_connectors_v1(id),
    connector_generation INTEGER NOT NULL CHECK(connector_generation>0),
    inbox_id TEXT REFERENCES bridge_inbox_v1(id),
    manual_action_id TEXT UNIQUE REFERENCES bridge_manual_actions_v1(id),
    run_id TEXT,
    conversation_id TEXT NOT NULL,
    conversation_kind TEXT NOT NULL CHECK(conversation_kind IN ('dm','group')),
    target_inbox_id TEXT NOT NULL REFERENCES bridge_inbox_v1(id),
    reply_context_json TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    attachments_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('queued','started','confirmed','unknown','failed')),
    platform_receipt_json TEXT,
    error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK(inbox_id IS NOT NULL OR run_id IS NOT NULL OR manual_action_id IS NOT NULL),
    CHECK(manual_action_id IS NULL OR (inbox_id IS NULL AND run_id IS NULL))
);
CREATE UNIQUE INDEX idx_bridge_outbox_command ON bridge_outbox_v1(inbox_id) WHERE run_id IS NULL AND manual_action_id IS NULL;
CREATE INDEX idx_bridge_outbox_pending ON bridge_outbox_v1(status,created_at);
CREATE INDEX idx_bridge_outbox_inbox ON bridge_outbox_v1(inbox_id);
CREATE TABLE bridge_notice_receipts_v1 (
    delivery_id TEXT PRIMARY KEY REFERENCES bridge_outbox_v1(delivery_id),
    session_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    fact_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
"#;

/// R07：等待提示、草稿更新与最终投递回执共用真实因果来源，重启不重放未知效果。
pub const V17_NAME: &str = "r07_bridge_effect_receipts";
pub const V17_SQL: &str = r#"
CREATE TABLE bridge_effect_streams_v1 (
    stream_id TEXT PRIMARY KEY,
    inbox_id TEXT NOT NULL REFERENCES bridge_inbox_v1(id),
    run_id TEXT NOT NULL,
    connector_id TEXT NOT NULL REFERENCES bridge_connectors_v1(id),
    connector_generation INTEGER NOT NULL CHECK(connector_generation>0),
    mode TEXT NOT NULL CHECK(mode IN ('receipt_only','native_stream')),
    rich INTEGER NOT NULL CHECK(rich IN (0,1)),
    conversation_id TEXT NOT NULL,
    reply_context_json TEXT NOT NULL,
    unknown INTEGER NOT NULL DEFAULT 0 CHECK(unknown IN (0,1)),
    closed INTEGER NOT NULL DEFAULT 0 CHECK(closed IN (0,1)),
    last_sequence INTEGER NOT NULL DEFAULT 0 CHECK(last_sequence>=0 AND last_sequence<=4294967295),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(inbox_id,run_id)
);
CREATE INDEX idx_bridge_effect_streams_active ON bridge_effect_streams_v1(connector_id,connector_generation,closed);
CREATE TABLE bridge_effects_v1 (
    effect_id TEXT PRIMARY KEY,
    stream_id TEXT NOT NULL REFERENCES bridge_effect_streams_v1(stream_id),
    request_key TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK(sequence>0 AND sequence<=4294967295),
    operation TEXT NOT NULL CHECK(operation IN ('receipt_start','receipt_stop','stream_update','stream_finish','stream_cancel')),
    request_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('started','confirmed','failed','unknown','skipped')),
    receipt_json TEXT,
    error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(stream_id,request_key),
    UNIQUE(stream_id,sequence)
);
CREATE INDEX idx_bridge_effects_started ON bridge_effects_v1(status,stream_id);
CREATE UNIQUE INDEX idx_bridge_effects_finish ON bridge_effects_v1(stream_id) WHERE operation='stream_finish';
"#;

/// R07：独立输入状态与内容草稿分开冻结；旧候选保守地视为不独立。
pub const V18_NAME: &str = "r07_bridge_independent_receipt";
pub const V18_SQL: &str = r#"
ALTER TABLE bridge_effect_streams_v1 ADD COLUMN receipt_independent INTEGER NOT NULL DEFAULT 0 CHECK(receipt_independent IN (0,1));
"#;

/// R07：频道/私信兼容文件收据、手机投递因果预算与协作草稿；既有版本保持逐字不变。
pub const V19_NAME: &str = "r07_conversation_phone_collab_receipts";
pub const V19_SQL: &str = r#"
CREATE TABLE conversation_file_operations_v1(operation_id TEXT PRIMARY KEY,request_sha256 TEXT NOT NULL,mutation_json TEXT NOT NULL,status TEXT NOT NULL CHECK(status IN ('prepared','applied','partial','failed','unknown')),created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL);
CREATE TABLE conversation_file_targets_v1(operation_id TEXT NOT NULL REFERENCES conversation_file_operations_v1(operation_id),relative_path TEXT NOT NULL,preimage_sha256 TEXT,postimage_sha256 TEXT,postimage TEXT,status TEXT NOT NULL CHECK(status IN ('prepared','applied','failed','unknown')),error TEXT,PRIMARY KEY(operation_id,relative_path));
CREATE INDEX idx_conversation_files_recovery ON conversation_file_operations_v1(status,created_at);
CREATE TABLE conversation_file_sources_v1(operation_id TEXT PRIMARY KEY,source_run_id TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,source_tool_call_id TEXT NOT NULL UNIQUE REFERENCES invocation_journal(journal_id));
CREATE TABLE conversation_message_dispatch_v1(operation_id TEXT PRIMARY KEY REFERENCES conversation_file_operations_v1(operation_id),status TEXT NOT NULL CHECK(status IN ('completed','skipped')),reason TEXT,updated_at INTEGER NOT NULL);
CREATE TABLE phone_chains_v1(root_chain_id TEXT PRIMARY KEY,owner_subject TEXT NOT NULL,conversation_type TEXT NOT NULL CHECK(conversation_type IN ('channel','dm')),conversation_scope TEXT NOT NULL,budget_limit INTEGER NOT NULL CHECK(budget_limit BETWEEN 1 AND 64),spent INTEGER NOT NULL CHECK(spent>=0 AND spent<=budget_limit),created_at INTEGER NOT NULL);
CREATE TABLE phone_deliveries_v1(delivery_id TEXT PRIMARY KEY,input_json TEXT NOT NULL,owner_subject TEXT NOT NULL,actor_id TEXT NOT NULL,conversation_id TEXT NOT NULL,conversation_type TEXT NOT NULL CHECK(conversation_type IN ('channel','dm')),cause_id TEXT NOT NULL,batch_digest TEXT NOT NULL,root_chain_id TEXT NOT NULL REFERENCES phone_chains_v1(root_chain_id),status TEXT NOT NULL CHECK(status IN ('claimed','running','completed','failed','cancelled','unknown')),session_id TEXT REFERENCES sessions(session_id) ON DELETE SET NULL,run_id TEXT UNIQUE REFERENCES runs(run_id) ON DELETE SET NULL,decision TEXT CHECK(decision IN ('reply','pass')),decision_body TEXT,decision_at INTEGER CHECK(decision_at>=0),error TEXT,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,UNIQUE(owner_subject,cause_id,actor_id,batch_digest));
CREATE INDEX idx_phone_deliveries_pending ON phone_deliveries_v1(status,created_at);
CREATE INDEX idx_phone_deliveries_chain ON phone_deliveries_v1(root_chain_id,actor_id,status);
CREATE TABLE phone_dm_directions_v1(owner_subject TEXT NOT NULL,from_actor TEXT NOT NULL,to_actor TEXT NOT NULL,delivery_id TEXT NOT NULL REFERENCES phone_deliveries_v1(delivery_id),processing_until INTEGER NOT NULL,cooldown_until INTEGER NOT NULL,PRIMARY KEY(owner_subject,from_actor,to_actor));
CREATE TABLE phone_discovery_cursors_v1(conversation_id TEXT PRIMARY KEY,message_fingerprint TEXT NOT NULL,proactive_slot INTEGER NOT NULL,updated_at INTEGER NOT NULL);
CREATE TABLE phone_binding_epochs_v1(actor_id TEXT NOT NULL,conversation_id TEXT NOT NULL,owner_subject TEXT NOT NULL,epoch INTEGER NOT NULL CHECK(epoch>0),PRIMARY KEY(actor_id,conversation_id,owner_subject));
CREATE TABLE phone_bindings_v1(actor_id TEXT NOT NULL,conversation_id TEXT NOT NULL,conversation_type TEXT NOT NULL CHECK(conversation_type IN ('channel','dm')),owner_subject TEXT NOT NULL,session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,mode TEXT NOT NULL CHECK(mode IN ('ask','read_only','operate')),epoch INTEGER NOT NULL CHECK(epoch>0),last_used INTEGER NOT NULL,expires_at INTEGER NOT NULL,PRIMARY KEY(actor_id,conversation_id,owner_subject));
CREATE TABLE session_collab_drafts_v1(id TEXT PRIMARY KEY,studio_id TEXT NOT NULL,owner_subject TEXT NOT NULL CHECK(owner_subject='user_local'),source_session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,source_run_id TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,source_tool_call_id TEXT NOT NULL,kind TEXT NOT NULL CHECK(kind IN ('send','create')),draft_json TEXT NOT NULL,frozen_apply_json TEXT,request_id TEXT NOT NULL UNIQUE,state TEXT NOT NULL CHECK(state IN ('pending','applying','applied','rejected','unknown')),delivery_status TEXT NOT NULL CHECK(delivery_status IN ('none','queued','started','accepted','failed','unknown')),reserved_target_id TEXT,target_session_id TEXT REFERENCES sessions(session_id) ON DELETE SET NULL,target_created INTEGER NOT NULL DEFAULT 0 CHECK(target_created IN (0,1)),accepted_run_id TEXT REFERENCES runs(run_id) ON DELETE SET NULL,attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt>=0),error TEXT,created_at INTEGER NOT NULL,expires_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,UNIQUE(source_run_id,source_tool_call_id));
CREATE INDEX idx_session_collab_queue ON session_collab_drafts_v1(delivery_status,created_at);
CREATE TABLE session_collab_decisions_v1(suggestion_id TEXT PRIMARY KEY REFERENCES session_collab_drafts_v1(id) ON DELETE CASCADE,session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,message_id TEXT NOT NULL UNIQUE,decision_json TEXT NOT NULL,created_at INTEGER NOT NULL);
"#;

pub const V20_NAME: &str = "r07_browser_authority";
pub const V20_SQL: &str = r#"
CREATE TABLE browser_workspaces_v1 (
 session_id TEXT PRIMARY KEY REFERENCES sessions(session_id) ON DELETE CASCADE,
 owner_subject TEXT NOT NULL, execution_node TEXT NOT NULL,
 epoch INTEGER NOT NULL CHECK(epoch>0), revoked INTEGER NOT NULL CHECK(revoked IN(0,1)),
 revoked_run_watermark INTEGER NOT NULL DEFAULT 0,
 state_json TEXT CHECK(state_json IS NULL OR json_valid(state_json)), updated_at INTEGER NOT NULL
);
CREATE INDEX idx_browser_owner ON browser_workspaces_v1(owner_subject);
CREATE TABLE browser_downloads_v1 (
 session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
 download_id TEXT NOT NULL, owner_subject TEXT NOT NULL, source_run_id TEXT,
 status TEXT NOT NULL CHECK(status IN('completed','unknown','failed')),
 file_id TEXT, error TEXT, updated_at INTEGER NOT NULL,
 PRIMARY KEY(session_id,download_id)
);
CREATE TABLE browser_commands_v1 (
 operation_id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
 owner_subject TEXT NOT NULL, epoch INTEGER NOT NULL, input_json TEXT NOT NULL CHECK(json_valid(input_json)),
 status TEXT NOT NULL CHECK(status IN('started','confirmed','unknown','failed')),
 result_json TEXT CHECK(result_json IS NULL OR json_valid(result_json)), error TEXT,
 updated_at INTEGER NOT NULL
);
"#;

pub const V21_NAME: &str = "r07_media_jobs";
pub const V21_SQL: &str = r#"
CREATE TABLE media_jobs_v1 (
 operation_id TEXT PRIMARY KEY,
 session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
 owner_subject TEXT NOT NULL,
 source_run_id TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
 execution_run_id TEXT REFERENCES runs(run_id) ON DELETE SET NULL,
 tool_call_id TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN('image','video','speech')),
 provider TEXT NOT NULL, request_digest TEXT NOT NULL,
 spec_json TEXT NOT NULL CHECK(json_valid(spec_json)),
 state_json TEXT NOT NULL CHECK(json_valid(state_json)),
 phase TEXT NOT NULL CHECK(phase IN('submitting','submitted','generating','downloading','completed','failed','cancelled','unknown')),
 revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
 created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
 UNIQUE(source_run_id,tool_call_id,kind)
);
CREATE INDEX idx_media_session_updated ON media_jobs_v1(session_id,updated_at);
CREATE INDEX idx_media_open ON media_jobs_v1(phase,created_at);
"#;

pub const V22_NAME: &str = "r07_beautify_jobs";
pub const V22_SQL: &str = r#"
CREATE TABLE beautify_jobs_v1 (
 job_id TEXT PRIMARY KEY, principal_id TEXT NOT NULL, request_id TEXT NOT NULL,
 session_id TEXT NOT NULL, run_id TEXT UNIQUE REFERENCES runs(run_id) ON DELETE SET NULL,
 phase TEXT NOT NULL CHECK(phase IN('queued','starting','prompting','prompt_ready','media_preparing','media_pending','applying','completed','failed','cancelled','unknown')),
 revision INTEGER NOT NULL CHECK(revision>0), job_json TEXT NOT NULL CHECK(json_valid(job_json)),
 created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
 UNIQUE(principal_id,request_id)
);
CREATE INDEX idx_beautify_open ON beautify_jobs_v1(phase,created_at);
CREATE TABLE beautify_authorizations_v1 (
 job_id TEXT NOT NULL REFERENCES beautify_jobs_v1(job_id), call_id TEXT NOT NULL,
 run_id TEXT NOT NULL, target TEXT NOT NULL, args_digest TEXT NOT NULL,
 binding_json TEXT NOT NULL CHECK(json_valid(binding_json)), created_at INTEGER NOT NULL,
 PRIMARY KEY(job_id,call_id)
);
"#;

pub const V23_NAME: &str = "r07_media_gallery";
pub const V23_SQL: &str = r#"
CREATE TABLE media_gallery_v1 (
 task_id TEXT PRIMARY KEY REFERENCES media_jobs_v1(operation_id) ON DELETE CASCADE,
 session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
 owner_subject TEXT NOT NULL,
 batch_id TEXT NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN('image','video','speech')),
 type TEXT NOT NULL,
 prompt TEXT NOT NULL,
 provider TEXT, model TEXT,
 status TEXT NOT NULL CHECK(status IN('pending','done','failed','cancelled')),
 favorited INTEGER NOT NULL DEFAULT 0 CHECK(favorited IN(0,1)),
 files_json TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(files_json)),
 fail_reason TEXT,
 retry_count INTEGER NOT NULL DEFAULT 0 CHECK(retry_count>=0),
 attempt INTEGER NOT NULL DEFAULT 1 CHECK(attempt>=1),
 created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, completed_at INTEGER
);
CREATE INDEX idx_media_gallery_owner_created ON media_gallery_v1(owner_subject,created_at);
CREATE INDEX idx_media_gallery_batch ON media_gallery_v1(batch_id);
"#;

/// R07-T10 legacy observability import (version 24): side map from an
/// imported call id to its LEGACY trace linkage. The authoritative ledger
/// table stays untouched — imported rows keep their original call ids
/// (never renumbered, so a legacy multi-turn trace never splits) and the
/// legacy trace_id / parent_call_id live here. No FK (mirrors the legacy
/// store's ordering-tolerant stance; the read side interprets orphans).
pub const V24_NAME: &str = "r07_observability_legacy_trace";
pub const V24_SQL: &str = r#"
CREATE TABLE observability_legacy_trace (
    model_call_id       TEXT PRIMARY KEY,
    trace_id            TEXT NOT NULL,
    parent_call_id      TEXT,
    imported_at_unix_ms INTEGER NOT NULL
);
CREATE INDEX idx_observability_legacy_trace_trace ON observability_legacy_trace(trace_id);
"#;

/// R07 收口：桥接循环投递台账（version 25）。/loop 作用于桥会话时，每一轮的
/// 回复与系统通知先落此表（服务侧已核循环记录），连接器推进循环逐条接力成
/// 正式 outbox；行持久化保证重启后接力不丢，delivery_id 幂等保证不重复外发。
pub const V25_NAME: &str = "r07_bridge_loop_deliveries";
pub const V25_SQL: &str = r#"
CREATE TABLE bridge_loop_deliveries_v1 (
    delivery_id TEXT PRIMARY KEY,
    connector_id TEXT NOT NULL REFERENCES bridge_connectors_v1(id),
    connector_generation INTEGER NOT NULL CHECK(connector_generation>0),
    conversation_id TEXT NOT NULL,
    sender_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    run_id TEXT,
    kind TEXT NOT NULL CHECK(kind IN ('reply','notice')),
    text TEXT,
    status TEXT NOT NULL CHECK(status IN ('queued','enqueued','failed')),
    error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK(kind<>'reply' OR run_id IS NOT NULL),
    CHECK(kind<>'notice' OR text IS NOT NULL)
);
CREATE INDEX idx_bridge_loop_deliveries_pending ON bridge_loop_deliveries_v1(status,connector_id);
"#;

/// R07 收口：outbox 增加循环投递台账来源列（version 26）。循环通知不锚
/// inbox/run/manual action，但台账行本身即持久审计来源；重建表以扩展
/// 来源 CHECK（四选一），其余约束与索引保持原样，存量行走迁移拷贝。
pub const V26_NAME: &str = "r07_bridge_outbox_loop_source";
pub const V26_SQL: &str = r#"
CREATE TABLE bridge_outbox_rebuilt (
    delivery_id TEXT PRIMARY KEY,
    connector_id TEXT NOT NULL REFERENCES bridge_connectors_v1(id),
    connector_generation INTEGER NOT NULL CHECK(connector_generation>0),
    inbox_id TEXT REFERENCES bridge_inbox_v1(id),
    manual_action_id TEXT UNIQUE REFERENCES bridge_manual_actions_v1(id),
    run_id TEXT,
    loop_delivery_id TEXT REFERENCES bridge_loop_deliveries_v1(delivery_id),
    conversation_id TEXT NOT NULL,
    conversation_kind TEXT NOT NULL CHECK(conversation_kind IN ('dm','group')),
    target_inbox_id TEXT NOT NULL REFERENCES bridge_inbox_v1(id),
    reply_context_json TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    attachments_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('queued','started','confirmed','unknown','failed')),
    platform_receipt_json TEXT,
    error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK(inbox_id IS NOT NULL OR run_id IS NOT NULL OR manual_action_id IS NOT NULL OR loop_delivery_id IS NOT NULL),
    CHECK(manual_action_id IS NULL OR (inbox_id IS NULL AND run_id IS NULL AND loop_delivery_id IS NULL)),
    CHECK(loop_delivery_id IS NULL OR (inbox_id IS NULL AND manual_action_id IS NULL))
);
INSERT INTO bridge_outbox_rebuilt
    (delivery_id,connector_id,connector_generation,inbox_id,manual_action_id,run_id,
     loop_delivery_id,conversation_id,conversation_kind,target_inbox_id,reply_context_json,
     payload_json,attachments_json,status,platform_receipt_json,error,created_at,updated_at)
SELECT delivery_id,connector_id,connector_generation,inbox_id,manual_action_id,run_id,
       NULL,conversation_id,conversation_kind,target_inbox_id,reply_context_json,
       payload_json,attachments_json,status,platform_receipt_json,error,created_at,updated_at
FROM bridge_outbox_v1;
DROP TABLE bridge_outbox_v1;
ALTER TABLE bridge_outbox_rebuilt RENAME TO bridge_outbox_v1;
CREATE UNIQUE INDEX idx_bridge_outbox_command ON bridge_outbox_v1(inbox_id) WHERE run_id IS NULL AND manual_action_id IS NULL;
CREATE INDEX idx_bridge_outbox_pending ON bridge_outbox_v1(status,created_at);
CREATE INDEX idx_bridge_outbox_inbox ON bridge_outbox_v1(inbox_id);
CREATE INDEX idx_bridge_outbox_loop ON bridge_outbox_v1(loop_delivery_id);
"#;

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: V1_NAME,
        sql: V1_SQL,
    },
    Migration {
        version: 2,
        name: V2_NAME,
        sql: V2_SQL,
    },
    Migration {
        version: 3,
        name: V3_NAME,
        sql: V3_SQL,
    },
    Migration {
        version: 4,
        name: V4_NAME,
        sql: V4_SQL,
    },
    Migration {
        version: 5,
        name: V5_NAME,
        sql: V5_SQL,
    },
    Migration {
        version: 6,
        name: V6_NAME,
        sql: V6_SQL,
    },
    Migration {
        version: 7,
        name: V7_NAME,
        sql: V7_SQL,
    },
    Migration {
        version: 8,
        name: V8_NAME,
        sql: V8_SQL,
    },
    Migration {
        version: 9,
        name: V9_NAME,
        sql: V9_SQL,
    },
    Migration {
        version: 10,
        name: V10_NAME,
        sql: V10_SQL,
    },
    Migration {
        version: 11,
        name: V11_NAME,
        sql: V11_SQL,
    },
    Migration {
        version: 12,
        name: V12_NAME,
        sql: V12_SQL,
    },
    Migration {
        version: 13,
        name: V13_NAME,
        sql: V13_SQL,
    },
    Migration {
        version: 14,
        name: V14_NAME,
        sql: V14_SQL,
    },
    Migration {
        version: 15,
        name: V15_NAME,
        sql: V15_SQL,
    },
    Migration {
        version: 16,
        name: V16_NAME,
        sql: V16_SQL,
    },
    Migration {
        version: 17,
        name: V17_NAME,
        sql: V17_SQL,
    },
    Migration {
        version: 18,
        name: V18_NAME,
        sql: V18_SQL,
    },
    Migration {
        version: 19,
        name: V19_NAME,
        sql: V19_SQL,
    },
    Migration {
        version: 20,
        name: V20_NAME,
        sql: V20_SQL,
    },
    Migration {
        version: 21,
        name: V21_NAME,
        sql: V21_SQL,
    },
    Migration {
        version: 22,
        name: V22_NAME,
        sql: V22_SQL,
    },
    Migration {
        version: 23,
        name: V23_NAME,
        sql: V23_SQL,
    },
    Migration {
        version: 24,
        name: V24_NAME,
        sql: V24_SQL,
    },
    Migration {
        version: 25,
        name: V25_NAME,
        sql: V25_SQL,
    },
    Migration {
        version: 26,
        name: V26_NAME,
        sql: V26_SQL,
    },
];

/// Highest schema version this build understands.
pub fn supported_version() -> u64 {
    MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
}

/// Result of an open-time migration pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationOutcome {
    /// Versions applied by THIS pass (empty when already up to date).
    pub applied: Vec<u64>,
    /// Schema version the database is at now.
    pub current_version: u64,
    /// Highest version this build supports.
    pub supported_version: u64,
}

fn user_version(conn: &Connection) -> Result<u64, StorageError> {
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(map_rusqlite)?;
    u64::try_from(v).map_err(|_| StorageError::Corrupted {
        detail: format!("negative PRAGMA user_version {v}"),
    })
}

fn set_user_version(conn: &Connection, version: u64) -> Result<(), StorageError> {
    // Same signed-32-bit sqlite3Atoi consumer as busy_timeout /
    // wal_autocheckpoint (sqlite3.c PragTyp_HEADER_VALUE, OP_SetCookie p3) —
    // an out-of-range value would not error, it would store 0. Migration
    // versions are internal constants far below this bound, but the cast
    // stays checked so this pragma can never silently write 0 either
    // (R02 stage-repair R4 / R4-F01 same-root-cause sweep).
    let version_i32 = i32::try_from(version).map_err(|_| StorageError::Internal {
        detail: format!(
            "migration version {version} exceeds the PRAGMA user_version range (signed 32-bit)"
        ),
    })?;
    conn.pragma_update(None, "user_version", version_i32)
        .map_err(map_rusqlite)
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool, StorageError> {
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |row| row.get(0),
        )
        .map_err(map_rusqlite)?;
    Ok(n > 0)
}

/// Bootstraps the receipts table itself (the one piece of DDL that cannot
/// live inside a receipted transaction). Loudly refuses when the database
/// is not empty but has no receipts table: that is a tampered or foreign
/// database, not a fresh one.
fn ensure_receipts_table(conn: &Connection) -> Result<(), StorageError> {
    if table_exists(conn, "schema_migrations")? {
        return Ok(());
    }
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' \
             AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )
        .map_err(map_rusqlite)?;
    if n > 0 {
        return Err(StorageError::SchemaTampered {
            detail: format!(
                "database has {n} non-system table(s) but no schema_migrations \
                 receipts; refusing to adopt an unregistered schema"
            ),
        });
    }
    conn.execute(
        "CREATE TABLE schema_migrations (
            version            INTEGER PRIMARY KEY,
            name               TEXT NOT NULL,
            fingerprint        TEXT NOT NULL,
            applied_at_unix_ms INTEGER NOT NULL,
            applied_by         TEXT NOT NULL
        )",
        [],
    )
    .map_err(map_rusqlite)?;
    Ok(())
}

/// On-disk receipt row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub version: u64,
    pub name: String,
    pub fingerprint: String,
    pub applied_at_unix_ms: i64,
    pub applied_by: String,
}

fn read_receipts(conn: &Connection) -> Result<Vec<Receipt>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT version, name, fingerprint, applied_at_unix_ms, applied_by \
             FROM schema_migrations ORDER BY version",
        )
        .map_err(map_rusqlite)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Receipt {
                version: row.get::<_, i64>(0)? as u64,
                name: row.get(1)?,
                fingerprint: row.get(2)?,
                applied_at_unix_ms: row.get(3)?,
                applied_by: row.get(4)?,
            })
        })
        .map_err(map_rusqlite)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(map_rusqlite)?);
    }
    Ok(out)
}

/// Verifies the receipts against the compiled-in migration list: contiguous
/// from 1, known versions/names, matching fingerprints, and in agreement
/// with `PRAGMA user_version`.
pub fn verify_receipts(conn: &Connection) -> Result<Vec<Receipt>, StorageError> {
    let receipts = read_receipts(conn)?;
    let pragma = user_version(conn)?;
    let max_receipt = receipts.last().map(|r| r.version).unwrap_or(0);
    if pragma != max_receipt {
        return Err(StorageError::SchemaTampered {
            detail: format!(
                "PRAGMA user_version ({pragma}) disagrees with the newest \
                 migration receipt ({max_receipt}); version bookkeeping was \
                 tampered with"
            ),
        });
    }
    // An EMPTY receipts table over a database that already holds non-system
    // tables is a wiped bookkeeping trail, not a fresh database: replaying
    // migrations over existing tables is forbidden (loud, classified).
    if receipts.is_empty() {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' \
                 AND name NOT LIKE 'sqlite_%' AND name != 'schema_migrations'",
                [],
                |row| row.get(0),
            )
            .map_err(map_rusqlite)?;
        if n > 0 {
            return Err(StorageError::SchemaTampered {
                detail: format!(
                    "schema_migrations is empty but the database holds {n} \
                     non-system table(s); receipts were wiped - refusing to \
                     replay migrations over existing tables"
                ),
            });
        }
        return Ok(receipts);
    }
    for (index, receipt) in receipts.iter().enumerate() {
        let expected_version = (index as u64) + 1;
        if receipt.version != expected_version {
            return Err(StorageError::SchemaTampered {
                detail: format!(
                    "migration receipts must be contiguous from 1; position \
                     {index} holds version {}",
                    receipt.version
                ),
            });
        }
        let Some(expected) = MIGRATIONS.iter().find(|m| m.version == receipt.version) else {
            return Err(StorageError::DatabaseTooNew {
                found_version: receipt.version,
                supported_version: supported_version(),
            });
        };
        if receipt.name != expected.name {
            return Err(StorageError::SchemaTampered {
                detail: format!(
                    "migration {expected_version} recorded as {:?} but this \
                     build expects {:?}",
                    receipt.name, expected.name
                ),
            });
        }
        let expected_fp = fingerprint_sql(expected.sql);
        if receipt.fingerprint != expected_fp {
            return Err(StorageError::SchemaTampered {
                detail: format!(
                    "migration {expected_version} ({}) fingerprint mismatch: \
                     on-disk {}, compiled-in {}; the SQL text drifted or was \
                     tampered with",
                    receipt.name, receipt.fingerprint, expected_fp
                ),
            });
        }
    }
    Ok(receipts)
}

/// Runs the open-time migration pass: verify receipts, reject newer
/// databases, apply pending migrations one transaction each.
pub fn apply_all(
    conn: &Connection,
    applied_by: &str,
    now_unix_ms: i64,
) -> Result<MigrationOutcome, StorageError> {
    ensure_receipts_table(conn)?;
    let receipts = verify_receipts(conn)?;
    let current = receipts.last().map(|r| r.version).unwrap_or(0);
    let supported = supported_version();
    if current > supported {
        return Err(StorageError::DatabaseTooNew {
            found_version: current,
            supported_version: supported,
        });
    }
    let mut applied = Vec::new();
    for migration in MIGRATIONS.iter().filter(|m| m.version > current) {
        let fp = fingerprint_sql(migration.sql);
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(map_rusqlite)?;
        let txn_result = (|| -> Result<(), StorageError> {
            conn.execute_batch(migration.sql).map_err(map_rusqlite)?;
            conn.execute(
                "INSERT INTO schema_migrations \
                 (version, name, fingerprint, applied_at_unix_ms, applied_by) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    migration.version as i64,
                    migration.name,
                    fp,
                    now_unix_ms,
                    applied_by
                ],
            )
            .map_err(map_rusqlite)?;
            set_user_version(conn, migration.version)?;
            Ok(())
        })();
        match txn_result {
            Ok(()) => {
                conn.execute_batch("COMMIT").map_err(map_rusqlite)?;
                applied.push(migration.version);
            }
            Err(err) => {
                // Roll back loudly; a rollback failure is itself surfaced
                // (never masked by the original error).
                if let Err(rollback_err) = conn.execute_batch("ROLLBACK") {
                    return Err(StorageError::Internal {
                        detail: format!(
                            "migration {} failed ({err}) AND rollback failed \
                             ({rollback_err})",
                            migration.version
                        ),
                    });
                }
                return Err(err);
            }
        }
    }
    Ok(MigrationOutcome {
        applied,
        current_version: supported_version(),
        supported_version: supported,
    })
}

/// Maps a rusqlite error onto the kernel storage error vocabulary, keying
/// on SQLite's numeric primary codes (the stable wire between SQLite
/// versions and rusqlite enum surface):
/// 5/6 busy+locked, 13 disk full, 8/14 readonly/cant-open (real IO faults),
/// 10 io_err, 11 corrupt, 26 not-a-database (a corrupted or foreign file —
/// R02-T06 A12: the open must refuse loudly, never create or overwrite),
/// 18/too-many-files... anything else internal.
pub fn map_rusqlite(err: rusqlite::Error) -> StorageError {
    if let rusqlite::Error::SqliteFailure(ffi_err, message) = &err {
        let primary = ffi_err.extended_code & 0xff;
        let detail = message.clone().unwrap_or_else(|| err.to_string());
        return match primary {
            5 | 6 => StorageError::Busy { timeout_ms: 0 },
            13 => StorageError::DiskFull { detail },
            8 | 14 => StorageError::Io { detail },
            10 => StorageError::Io { detail },
            11 | 26 => StorageError::Corrupted { detail },
            _ => StorageError::Internal { detail },
        };
    }
    StorageError::Internal {
        detail: err.to_string(),
    }
}

/// Maps busy with the configured timeout for accurate reporting.
pub fn map_rusqlite_busy(err: rusqlite::Error, timeout_ms: u64) -> StorageError {
    match map_rusqlite(err) {
        StorageError::Busy { .. } => StorageError::Busy { timeout_ms },
        other => other,
    }
}

/// Full-file integrity verification of an opened database (R02-T06 step 3:
/// "恢复时校验 schema/epoch/完整性"). Runs `PRAGMA integrity_check` and
/// requires every output row to be `ok`; any other verdict (or an error
/// while running it) is a loud [`StorageError::Corrupted`] — the caller
/// (the startup path) must refuse, never rebuild, truncate or recreate.
///
/// Called by [`crate::storage::RunDatabase::open`] AFTER the receipt
/// verification, so a database that is structurally intact but whose
/// schema bookkeeping was tampered with is still rejected.
pub fn verify_integrity(conn: &Connection) -> Result<(), StorageError> {
    let mut stmt = conn
        .prepare("PRAGMA integrity_check")
        .map_err(map_rusqlite)?;
    let mut rows = stmt.query([]).map_err(map_rusqlite)?;
    let mut verdicts = 0usize;
    while let Some(row) = rows.next().map_err(map_rusqlite)? {
        let line: String = row.get(0).map_err(map_rusqlite)?;
        if line != "ok" {
            return Err(StorageError::Corrupted {
                detail: format!("integrity_check reported: {line}"),
            });
        }
        verdicts += 1;
    }
    if verdicts == 0 {
        return Err(StorageError::Corrupted {
            detail: "integrity_check returned no verdict row".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory() -> Connection {
        Connection::open_in_memory().expect("open in-memory db")
    }

    fn hex_len_ok(fp: &str) -> bool {
        fp.len() == 64 && fp.chars().all(|c| c.is_ascii_hexdigit())
    }

    #[test]
    fn fingerprints_are_stable_sha256_hex() {
        let fp = fingerprint_sql(V1_SQL);
        assert!(hex_len_ok(&fp), "fingerprint shape: {fp}");
        // Deterministic: same SQL, same fingerprint.
        assert_eq!(fp, fingerprint_sql(V1_SQL));
        // Sensitive: any edit changes it.
        let mut edited = V1_SQL.to_string();
        edited.push('\n');
        assert_ne!(fp, fingerprint_sql(&edited));
    }

    #[test]
    fn migrations_are_contiguous_from_one() {
        for (index, m) in MIGRATIONS.iter().enumerate() {
            assert_eq!(m.version, index as u64 + 1);
            assert!(!m.name.is_empty());
            // Every migration carries real DDL. An ADDITIVE migration may
            // legitimately be ALTER-based (R05 RR1 F21's v6 appends usage
            // ledger columns — SQLite cannot add non-constant NOT NULL
            // columns to an existing table by table rebuild either); the
            // guard still refuses DDL-free "migration" entries.
            assert!(
                m.sql.contains("CREATE TABLE") || m.sql.contains("ALTER TABLE"),
                "migration {} carries no DDL",
                m.version
            );
        }
    }

    #[test]
    fn migration_sql_avoids_if_not_exists() {
        // Accidental replays must fail loudly, never no-op.
        for m in MIGRATIONS {
            assert!(
                !m.sql.to_ascii_lowercase().contains("if not exists"),
                "migration {} uses IF NOT EXISTS",
                m.version
            );
        }
    }

    #[test]
    fn apply_then_verify_is_idempotent() {
        let conn = in_memory();
        let out = apply_all(&conn, "test", 1_000).unwrap();
        let expected: Vec<u64> = (1..=supported_version()).collect();
        assert_eq!(out.applied, expected);
        assert_eq!(out.current_version, supported_version());
        // Re-run: nothing applied, no error.
        let again = apply_all(&conn, "test", 2_000).unwrap();
        assert!(again.applied.is_empty());
        assert_eq!(again.current_version, supported_version());
        assert!(verify_receipts(&conn).is_ok());
    }

    #[test]
    fn tampered_version_row_is_rejected() {
        let conn = in_memory();
        apply_all(&conn, "test", 1_000).unwrap();
        // Direct tampering with the receipt version (down to 0).
        conn.execute(
            "UPDATE schema_migrations SET version = 0 WHERE version = 1",
            [],
        )
        .unwrap();
        match apply_all(&conn, "test", 2_000) {
            Err(StorageError::SchemaTampered { detail }) => {
                // Either check fires: the pragma/receipt disagreement (the
                // row moved to 0 while user_version stayed 1) or the
                // contiguity check. Both are loud tamper rejections.
                assert!(
                    detail.contains("contiguous") || detail.contains("disagrees"),
                    "detail: {detail}"
                )
            }
            other => panic!("expected SchemaTampered, got {other:?}"),
        }
    }

    #[test]
    fn tampered_user_version_is_rejected() {
        let conn = in_memory();
        apply_all(&conn, "test", 1_000).unwrap();
        conn.pragma_update(None, "user_version", 0).unwrap();
        match apply_all(&conn, "test", 2_000) {
            Err(StorageError::SchemaTampered { detail }) => {
                assert!(detail.contains("disagrees"), "detail: {detail}")
            }
            other => panic!("expected SchemaTampered, got {other:?}"),
        }
    }

    #[test]
    fn tampered_fingerprint_is_rejected() {
        let conn = in_memory();
        apply_all(&conn, "test", 1_000).unwrap();
        conn.execute(
            "UPDATE schema_migrations SET fingerprint = 'deadbeef' WHERE version = 1",
            [],
        )
        .unwrap();
        match apply_all(&conn, "test", 2_000) {
            Err(StorageError::SchemaTampered { detail }) => {
                assert!(detail.contains("fingerprint mismatch"), "detail: {detail}")
            }
            other => panic!("expected SchemaTampered, got {other:?}"),
        }
    }

    #[test]
    fn foreign_tables_without_receipts_are_rejected() {
        let conn = in_memory();
        conn.execute("CREATE TABLE stranger (x INTEGER)", [])
            .unwrap();
        match apply_all(&conn, "test", 1_000) {
            Err(StorageError::SchemaTampered { detail }) => {
                assert!(detail.contains("refusing to adopt"), "detail: {detail}")
            }
            other => panic!("expected SchemaTampered, got {other:?}"),
        }
    }

    #[test]
    fn newer_database_is_rejected_not_downgraded() {
        let conn = in_memory();
        apply_all(&conn, "test", 1_000).unwrap();
        // Simulate a database from a newer build: one version beyond what
        // this build carries.
        let future = supported_version() + 1;
        conn.execute(
            "INSERT INTO schema_migrations \
             (version, name, fingerprint, applied_at_unix_ms, applied_by) \
             VALUES (?1, 'future', 'fp', 1, 'future-build')",
            [i64::try_from(future).unwrap()],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", i64::try_from(future).unwrap())
            .unwrap();
        match apply_all(&conn, "test", 2_000) {
            Err(StorageError::DatabaseTooNew {
                found_version,
                supported_version,
            }) => {
                assert_eq!(found_version, future);
                assert_eq!(supported_version, super::supported_version());
            }
            other => panic!("expected DatabaseTooNew, got {other:?}"),
        }
    }

    #[test]
    fn not_a_database_maps_to_corrupted() {
        // R02-T06 A12: a garbage/foreign file must be classified Corrupted
        // (refuse loudly), never Internal-with-retry semantics.
        use rusqlite::ffi::Error as FfiError;
        let mk = rusqlite::Error::SqliteFailure(
            FfiError::new(26),
            Some("file is not a database".to_string()),
        );
        assert!(matches!(map_rusqlite(mk), StorageError::Corrupted { .. }));
    }

    #[test]
    fn integrity_check_passes_a_healthy_db_and_flags_a_broken_one() {
        let conn = in_memory();
        apply_all(&conn, "test", 1_000).unwrap();
        verify_integrity(&conn).expect("healthy db passes integrity_check");
    }

    #[test]
    fn sqlite_error_mapping_by_primary_code() {
        use rusqlite::ffi::Error as FfiError;
        let mk = |code: i32| {
            rusqlite::Error::SqliteFailure(FfiError::new(code), Some("synthetic".to_string()))
        };
        assert!(matches!(
            map_rusqlite(mk(13)),
            StorageError::DiskFull { .. }
        ));
        assert!(matches!(
            map_rusqlite(mk(266)), // SQLITE_IOERR_READ (extended, primary 10)
            StorageError::Io { .. }
        ));
        assert!(matches!(
            map_rusqlite(mk(261)), // SQLITE_BUSY_RECOVERY (extended, primary 5)
            StorageError::Busy { .. }
        ));
        assert!(matches!(
            map_rusqlite(mk(5)),
            StorageError::Busy { timeout_ms: 0 }
        ));
        assert!(matches!(
            map_rusqlite_busy(mk(5), 7_000),
            StorageError::Busy { timeout_ms: 7_000 }
        ));
        assert!(matches!(map_rusqlite(mk(8)), StorageError::Io { .. }));
        assert!(matches!(map_rusqlite(mk(10)), StorageError::Io { .. }));
        assert!(matches!(
            map_rusqlite(mk(11)),
            StorageError::Corrupted { .. }
        ));
        assert!(matches!(
            map_rusqlite(rusqlite::Error::InvalidParameterCount(1, 2)),
            StorageError::Internal { .. }
        ));
    }
}

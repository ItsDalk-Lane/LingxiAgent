import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { createRequire } from "node:module";

import { KnowledgeError } from "./errors.ts";
import type {
  ContentSnapshot,
  ImportedKnowledgeSource,
  KnowledgeBlock,
  KnowledgeCitation,
  KnowledgeNotebook,
  KnowledgeParseArtifact,
  KnowledgeParseStatus,
  KnowledgeQueryMode,
  KnowledgeRun,
  KnowledgeRunCitationRef,
  KnowledgeRunRetrieval,
  KnowledgeScopeSnapshot,
  KnowledgeSource,
  KnowledgeSourceType,
  NotebookSourceMembership,
  ResolvedKnowledgeCitation,
} from "./types.ts";
import type { KnowledgeBlockDraft } from "./source-adapters.ts";

export const KNOWLEDGE_SCHEMA_VERSION = 5;

const SOURCE_TYPES = new Set<KnowledgeSourceType>(["file", "pasted_text", "web_snapshot"]);
const PARSE_STATUSES = new Set<KnowledgeParseStatus>(["parsing", "ready", "needs_ocr", "failed"]);
const QUERY_MODES = new Set<KnowledgeQueryMode>(["quick", "research"]);
const require = createRequire(import.meta.url);
let BetterSqliteDatabase: any = null;

function loadDatabase() {
  if (!BetterSqliteDatabase) {
    const mod = require("better-sqlite3");
    BetterSqliteDatabase = mod?.default || mod;
  }
  return BetterSqliteDatabase;
}

function requiredString(value: unknown, field: string, maxLength: number): string {
  if (typeof value !== "string") {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must be a string`);
  }
  const normalized = value.trim();
  if (!normalized) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must not be empty`);
  }
  if (normalized.length > maxLength) {
    throw new KnowledgeError(
      "KNOWLEDGE_INVALID_ARGUMENT",
      `${field} exceeds the ${maxLength} character limit`,
      { field, maxLength },
    );
  }
  return normalized;
}

function parseObjectJson(value: unknown, field: string): Record<string, unknown> {
  try {
    const parsed = JSON.parse(typeof value === "string" ? value : "");
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("not an object");
    return parsed;
  } catch {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", `${field} is corrupt`);
  }
}

function serializeObjectJson(value: unknown, field: string): string {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must be an object`);
  }
  const serialized = JSON.stringify(value);
  if (Buffer.byteLength(serialized, "utf8") > 64 * 1024) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} is too large`);
  }
  return serialized;
}

function parseStringArrayJson(value: unknown, field: string): string[] {
  try {
    const parsed = JSON.parse(typeof value === "string" ? value : "");
    if (!Array.isArray(parsed) || parsed.some((entry) => typeof entry !== "string")) {
      throw new Error("not a string array");
    }
    return parsed;
  } catch {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", `${field} is corrupt`);
  }
}

function serializeStringArray(value: unknown, field: string): string {
  if (!Array.isArray(value) || value.some((entry) => typeof entry !== "string" || entry.length > 256)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must be a string array`);
  }
  return JSON.stringify(value);
}

function storagePath(value: unknown): string {
  const normalized = requiredString(value, "storagePath", 1024);
  if (
    path.isAbsolute(normalized)
    || normalized.includes("\\")
    || normalized.split("/").some((segment) => segment === ".." || segment === "")
  ) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "storagePath must be a safe relative path");
  }
  return normalized;
}

function sourceType(value: unknown): KnowledgeSourceType {
  if (typeof value !== "string" || !SOURCE_TYPES.has(value as KnowledgeSourceType)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "sourceType is unsupported");
  }
  return value as KnowledgeSourceType;
}

function sha256(value: unknown): string {
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/u.test(value)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "sha256 must be 64 lowercase hex characters");
  }
  return value;
}

function byteSize(value: unknown): number {
  if (!Number.isSafeInteger(value) || Number(value) < 0) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "byteSize must be a non-negative integer");
  }
  return Number(value);
}

function toNotebook(row: any): KnowledgeNotebook | null {
  if (!row) return null;
  return {
    id: row.id,
    studioId: row.studio_id,
    name: row.name,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
    deletedAt: row.deleted_at || null,
  };
}

function toSource(row: any): KnowledgeSource | null {
  if (!row) return null;
  return {
    id: row.id,
    studioId: row.studio_id,
    sourceType: row.source_type,
    displayName: row.display_name,
    originMetadata: parseObjectJson(row.origin_metadata_json, "origin metadata"),
    createdAt: row.created_at,
    deletedAt: row.deleted_at || null,
  };
}

function toMembership(row: any): NotebookSourceMembership | null {
  if (!row) return null;
  return {
    notebookId: row.notebook_id,
    sourceId: row.source_id,
    addedAt: row.added_at,
    removedAt: row.removed_at || null,
  };
}

function toSnapshot(row: any): ContentSnapshot | null {
  if (!row) return null;
  return {
    id: row.id,
    sourceId: row.source_id,
    sha256: row.sha256,
    mimeType: row.mime_type,
    byteSize: Number(row.byte_size),
    storagePath: row.storage_path,
    capturedAt: row.captured_at,
  };
}

function toParseArtifact(row: any): KnowledgeParseArtifact | null {
  if (!row) return null;
  return {
    id: row.id,
    contentSnapshotId: row.content_snapshot_id,
    parserId: row.parser_id,
    parserVersion: row.parser_version,
    parserConfigHash: row.parser_config_hash,
    status: row.status,
    warnings: parseStringArrayJson(row.warnings_json, "parse warnings"),
    semanticArtifactPath: row.semantic_artifact_path || null,
    createdAt: row.created_at,
    completedAt: row.completed_at || null,
  };
}

function toBlock(row: any): KnowledgeBlock | null {
  if (!row) return null;
  return {
    id: row.id,
    parseArtifactId: row.parse_artifact_id,
    ordinal: Number(row.ordinal),
    text: row.text,
    textSha256: row.text_sha256,
    locatorType: row.locator_type,
    locator: parseObjectJson(row.locator_payload_json, "block locator"),
  };
}

function toCitation(row: any): KnowledgeCitation | null {
  if (!row) return null;
  return {
    id: row.id,
    parseArtifactId: row.parse_artifact_id,
    blockId: row.block_id,
    startOffset: Number(row.start_offset),
    endOffset: Number(row.end_offset),
    canonicalText: row.canonical_text,
    canonicalTextSha256: row.canonical_text_sha256,
    createdAt: row.created_at,
  };
}

export interface KnowledgeStoreOptions {
  dbPath: string;
  Database?: any;
  now?: () => string;
  idGenerator?: (prefix: string) => string;
}

/** Knowledge 领域事实库；索引和大文件字节不写入这里。 */
export class KnowledgeStore {
  declare db: any;
  private readonly now: () => string;
  private readonly idGenerator: (prefix: string) => string;

  constructor(options: KnowledgeStoreOptions) {
    if (!options?.dbPath || !path.isAbsolute(options.dbPath)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "KnowledgeStore requires an absolute dbPath");
    }
    fs.mkdirSync(path.dirname(options.dbPath), { recursive: true, mode: 0o700 });
    const Database = options.Database || loadDatabase();
    this.db = new Database(options.dbPath);
    this.now = options.now || (() => new Date().toISOString());
    this.idGenerator = options.idGenerator || ((prefix) => `${prefix}_${crypto.randomUUID()}`);

    try {
      this.db.pragma("journal_mode = WAL");
      this.db.pragma("synchronous = NORMAL");
      this.db.pragma("foreign_keys = ON");
      this.db.pragma("busy_timeout = 5000");
      this.migrate();
    } catch (error) {
      try {
        this.db.close();
      } catch {
        // 保留原始建库错误。
      }
      this.db = null;
      throw error;
    }
  }

  private migrate() {
    const current = Number(this.db.pragma("user_version", { simple: true }));
    if (!Number.isSafeInteger(current) || current < 0) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Knowledge database schema version is invalid");
    }
    if (current > KNOWLEDGE_SCHEMA_VERSION) {
      throw new KnowledgeError(
        "KNOWLEDGE_SCHEMA_NEWER",
        "Knowledge database uses a newer schema",
        { supportedVersion: KNOWLEDGE_SCHEMA_VERSION },
      );
    }
    if (current === KNOWLEDGE_SCHEMA_VERSION) return;

    this.db.transaction(() => {
      let version = current;
      while (version < KNOWLEDGE_SCHEMA_VERSION) {
        if (version === 0) this.createSchemaV1();
        if (version === 1) this.createSchemaV2();
        if (version === 2) this.createSchemaV3();
        if (version === 3) this.createSchemaV4();
        if (version === 4) this.createSchemaV5();
        version += 1;
      }
      this.db.pragma(`user_version = ${KNOWLEDGE_SCHEMA_VERSION}`);
    })();
  }

  private createSchemaV1() {
    this.db.exec(`
      CREATE TABLE notebooks (
        id TEXT PRIMARY KEY,
        studio_id TEXT NOT NULL,
        name TEXT NOT NULL CHECK(length(trim(name)) > 0),
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        deleted_at TEXT
      );

      CREATE INDEX idx_notebooks_studio_active
        ON notebooks(studio_id, deleted_at, updated_at DESC);

      CREATE TABLE sources (
        id TEXT PRIMARY KEY,
        studio_id TEXT NOT NULL,
        source_type TEXT NOT NULL CHECK(source_type IN ('file', 'pasted_text', 'web_snapshot')),
        display_name TEXT NOT NULL CHECK(length(trim(display_name)) > 0),
        origin_metadata_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        deleted_at TEXT
      );

      CREATE INDEX idx_sources_studio_active
        ON sources(studio_id, deleted_at, created_at DESC);

      CREATE TABLE notebook_sources (
        notebook_id TEXT NOT NULL,
        source_id TEXT NOT NULL,
        added_at TEXT NOT NULL,
        removed_at TEXT,
        PRIMARY KEY(notebook_id, source_id),
        FOREIGN KEY(notebook_id) REFERENCES notebooks(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_notebook_sources_active
        ON notebook_sources(notebook_id, removed_at, added_at);
      CREATE INDEX idx_source_notebooks_active
        ON notebook_sources(source_id, removed_at, added_at);

      CREATE TABLE content_snapshots (
        id TEXT PRIMARY KEY,
        source_id TEXT NOT NULL,
        sha256 TEXT NOT NULL CHECK(length(sha256) = 64),
        mime_type TEXT NOT NULL,
        byte_size INTEGER NOT NULL CHECK(byte_size >= 0),
        storage_path TEXT NOT NULL UNIQUE,
        captured_at TEXT NOT NULL,
        UNIQUE(source_id, sha256),
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_content_snapshots_source
        ON content_snapshots(source_id, captured_at DESC);
    `);
  }

  private createSchemaV2() {
    this.db.exec(`
      CREATE TABLE parse_artifacts (
        id TEXT PRIMARY KEY,
        content_snapshot_id TEXT NOT NULL,
        parser_id TEXT NOT NULL,
        parser_version TEXT NOT NULL,
        parser_config_hash TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('parsing', 'ready', 'needs_ocr', 'failed')),
        warnings_json TEXT NOT NULL,
        semantic_artifact_path TEXT,
        created_at TEXT NOT NULL,
        completed_at TEXT,
        UNIQUE(content_snapshot_id, parser_id, parser_version, parser_config_hash),
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_parse_artifacts_snapshot
        ON parse_artifacts(content_snapshot_id, created_at DESC);
      CREATE INDEX idx_parse_artifacts_status
        ON parse_artifacts(status, created_at);

      CREATE TABLE knowledge_blocks (
        id TEXT PRIMARY KEY,
        parse_artifact_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        text TEXT NOT NULL,
        text_sha256 TEXT NOT NULL CHECK(length(text_sha256) = 64),
        locator_type TEXT NOT NULL CHECK(locator_type IN ('text', 'markdown', 'pdf', 'html')),
        locator_payload_json TEXT NOT NULL,
        UNIQUE(parse_artifact_id, ordinal),
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_knowledge_blocks_artifact
        ON knowledge_blocks(parse_artifact_id, ordinal);

      CREATE TABLE knowledge_citations (
        id TEXT PRIMARY KEY,
        parse_artifact_id TEXT NOT NULL,
        block_id TEXT NOT NULL,
        start_offset INTEGER NOT NULL CHECK(start_offset >= 0),
        end_offset INTEGER NOT NULL CHECK(end_offset > start_offset),
        canonical_text TEXT NOT NULL,
        canonical_text_sha256 TEXT NOT NULL CHECK(length(canonical_text_sha256) = 64),
        created_at TEXT NOT NULL,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT,
        FOREIGN KEY(block_id) REFERENCES knowledge_blocks(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_knowledge_citations_artifact
        ON knowledge_citations(parse_artifact_id, created_at);
      CREATE INDEX idx_knowledge_citations_block
        ON knowledge_citations(block_id, start_offset, end_offset);
    `);
  }

  private createSchemaV3() {
    this.db.exec(`
      CREATE TABLE scope_snapshots (
        id TEXT PRIMARY KEY,
        studio_id TEXT NOT NULL,
        mode TEXT NOT NULL CHECK(mode IN ('quick', 'research')),
        created_at TEXT NOT NULL
      );

      CREATE INDEX idx_scope_snapshots_studio
        ON scope_snapshots(studio_id, created_at DESC);

      CREATE TABLE scope_notebooks (
        scope_snapshot_id TEXT NOT NULL,
        notebook_id TEXT NOT NULL,
        notebook_name TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        PRIMARY KEY(scope_snapshot_id, notebook_id),
        UNIQUE(scope_snapshot_id, ordinal),
        FOREIGN KEY(scope_snapshot_id) REFERENCES scope_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(notebook_id) REFERENCES notebooks(id) ON DELETE RESTRICT
      );

      CREATE TABLE scope_sources (
        scope_snapshot_id TEXT NOT NULL,
        notebook_id TEXT NOT NULL,
        source_id TEXT NOT NULL,
        source_display_name TEXT NOT NULL,
        content_snapshot_id TEXT NOT NULL,
        parse_artifact_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        PRIMARY KEY(scope_snapshot_id, notebook_id, source_id),
        UNIQUE(scope_snapshot_id, ordinal),
        FOREIGN KEY(scope_snapshot_id) REFERENCES scope_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(notebook_id) REFERENCES notebooks(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT,
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_scope_sources_artifact
        ON scope_sources(scope_snapshot_id, parse_artifact_id);
      CREATE INDEX idx_scope_sources_snapshot
        ON scope_sources(content_snapshot_id);

      CREATE TABLE knowledge_runs (
        id TEXT PRIMARY KEY,
        studio_id TEXT NOT NULL,
        mode TEXT NOT NULL CHECK(mode IN ('quick', 'research')),
        question TEXT NOT NULL,
        scope_snapshot_id TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'failed', 'cancelled')),
        retrieval_mode TEXT NOT NULL CHECK(retrieval_mode IN ('fts', 'hybrid')),
        answer_text TEXT,
        error_code TEXT,
        created_at TEXT NOT NULL,
        completed_at TEXT,
        FOREIGN KEY(scope_snapshot_id) REFERENCES scope_snapshots(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_knowledge_runs_studio
        ON knowledge_runs(studio_id, created_at DESC);
      CREATE INDEX idx_knowledge_runs_status
        ON knowledge_runs(status, created_at);

      CREATE TABLE knowledge_run_retrievals (
        run_id TEXT NOT NULL,
        rank INTEGER NOT NULL CHECK(rank > 0),
        chunk_id TEXT NOT NULL,
        parse_artifact_id TEXT NOT NULL,
        score REAL NOT NULL,
        PRIMARY KEY(run_id, rank),
        FOREIGN KEY(run_id) REFERENCES knowledge_runs(id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );

      CREATE TABLE knowledge_run_citations (
        run_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        marker INTEGER NOT NULL CHECK(marker > 0),
        citation_id TEXT NOT NULL,
        candidate_ref TEXT NOT NULL,
        PRIMARY KEY(run_id, ordinal),
        UNIQUE(run_id, marker),
        FOREIGN KEY(run_id) REFERENCES knowledge_runs(id) ON DELETE RESTRICT,
        FOREIGN KEY(citation_id) REFERENCES knowledge_citations(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_knowledge_run_citations_citation
        ON knowledge_run_citations(citation_id);
    `);
  }

  private createSchemaV4() {
    this.db.exec(`
      CREATE TABLE research_runs (
        run_id TEXT PRIMARY KEY,
        host_task_id TEXT NOT NULL UNIQUE,
        state TEXT NOT NULL CHECK(state IN (
          'queued', 'preparing_scope', 'building_manifest', 'scanning',
          'building_claims', 'checking_contradictions', 'synthesizing',
          'completed', 'recovering', 'partial', 'failed', 'canceled'
        )),
        spec_json TEXT NOT NULL,
        error_code TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        completed_at TEXT,
        FOREIGN KEY(run_id) REFERENCES knowledge_runs(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_runs_state
        ON research_runs(state, updated_at);

      CREATE TABLE analysis_manifests (
        run_id TEXT PRIMARY KEY,
        source_count INTEGER NOT NULL CHECK(source_count >= 0),
        parse_artifact_count INTEGER NOT NULL CHECK(parse_artifact_count >= 0),
        block_count INTEGER NOT NULL CHECK(block_count >= 0),
        unit_count INTEGER NOT NULL CHECK(unit_count > 0),
        primary_char_count INTEGER NOT NULL CHECK(primary_char_count > 0),
        created_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE TABLE analysis_units (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        parse_artifact_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        priority INTEGER NOT NULL CHECK(priority >= 0),
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        primary_char_count INTEGER NOT NULL CHECK(primary_char_count > 0),
        context_char_count INTEGER NOT NULL CHECK(context_char_count >= 0),
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, parse_artifact_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_analysis_units_run_status
        ON analysis_units(run_id, status, priority, ordinal);

      CREATE TABLE analysis_unit_spans (
        unit_id TEXT NOT NULL,
        kind TEXT NOT NULL CHECK(kind IN ('primary', 'context')),
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        block_id TEXT NOT NULL,
        block_ordinal INTEGER NOT NULL CHECK(block_ordinal >= 0),
        start_offset INTEGER NOT NULL CHECK(start_offset >= 0),
        end_offset INTEGER NOT NULL CHECK(end_offset > start_offset),
        PRIMARY KEY(unit_id, kind, ordinal),
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT,
        FOREIGN KEY(block_id) REFERENCES knowledge_blocks(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_analysis_unit_spans_block
        ON analysis_unit_spans(block_id, kind, start_offset, end_offset);

      CREATE TABLE execution_batches (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        estimated_chars INTEGER NOT NULL CHECK(estimated_chars > 0),
        created_at TEXT NOT NULL,
        started_at TEXT,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_execution_batches_run_status
        ON execution_batches(run_id, status, ordinal);

      CREATE TABLE execution_batch_units (
        batch_id TEXT NOT NULL,
        unit_id TEXT NOT NULL UNIQUE,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        PRIMARY KEY(batch_id, ordinal),
        FOREIGN KEY(batch_id) REFERENCES execution_batches(id) ON DELETE RESTRICT,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT
      );

      CREATE TABLE research_jobs (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        phase TEXT NOT NULL CHECK(phase IN ('claim_build', 'final_synthesis')),
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        input_refs_json TEXT NOT NULL,
        output_json TEXT,
        created_at TEXT NOT NULL,
        started_at TEXT,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, phase, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE TABLE task_attempts (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        work_type TEXT NOT NULL CHECK(work_type IN ('scan_batch', 'claim_job', 'contradiction_check', 'synthesis_job')),
        work_id TEXT NOT NULL,
        attempt_number INTEGER NOT NULL CHECK(attempt_number > 0),
        status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'failed', 'canceled')),
        error_code TEXT,
        output_json TEXT,
        started_at TEXT NOT NULL,
        completed_at TEXT,
        UNIQUE(work_type, work_id, attempt_number),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_task_attempts_run
        ON task_attempts(run_id, work_type, started_at);

      CREATE TABLE analysis_unit_results (
        unit_id TEXT PRIMARY KEY,
        attempt_id TEXT NOT NULL,
        result_json TEXT NOT NULL,
        completed_at TEXT NOT NULL,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT,
        FOREIGN KEY(attempt_id) REFERENCES task_attempts(id) ON DELETE RESTRICT
      );

      CREATE TABLE evidence_validations (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        unit_id TEXT NOT NULL,
        origin_type TEXT NOT NULL CHECK(origin_type IN ('analysis', 'contradiction')),
        origin_id TEXT NOT NULL,
        candidate_ordinal INTEGER NOT NULL CHECK(candidate_ordinal >= 0),
        status TEXT NOT NULL CHECK(status IN ('validated', 'invalid')),
        reason_code TEXT,
        citation_id TEXT,
        canonical_quote TEXT,
        quote_checksum TEXT,
        epistemic_basis TEXT NOT NULL CHECK(epistemic_basis IN ('explicit', 'inferred', 'mixed')),
        created_at TEXT NOT NULL,
        UNIQUE(origin_type, origin_id, candidate_ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT,
        FOREIGN KEY(citation_id) REFERENCES knowledge_citations(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_evidence_validations_run
        ON evidence_validations(run_id, status, created_at);

      CREATE TABLE research_evidence (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        unit_id TEXT NOT NULL,
        validation_id TEXT NOT NULL UNIQUE,
        citation_id TEXT NOT NULL,
        content_snapshot_id TEXT NOT NULL,
        parse_artifact_id TEXT NOT NULL,
        block_id TEXT NOT NULL,
        start_offset INTEGER NOT NULL CHECK(start_offset >= 0),
        end_offset INTEGER NOT NULL CHECK(end_offset > start_offset),
        canonical_quote TEXT NOT NULL,
        quote_checksum TEXT NOT NULL CHECK(length(quote_checksum) = 64),
        epistemic_basis TEXT NOT NULL CHECK(epistemic_basis IN ('explicit', 'inferred', 'mixed')),
        created_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT,
        FOREIGN KEY(validation_id) REFERENCES evidence_validations(id) ON DELETE RESTRICT,
        FOREIGN KEY(citation_id) REFERENCES knowledge_citations(id) ON DELETE RESTRICT,
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT,
        FOREIGN KEY(block_id) REFERENCES knowledge_blocks(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_evidence_run
        ON research_evidence(run_id, created_at);

      CREATE TABLE research_claims (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        origin_job_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        text TEXT NOT NULL,
        support_status TEXT NOT NULL CHECK(support_status IN ('supported', 'partial', 'disputed', 'insufficient')),
        epistemic_basis TEXT NOT NULL CHECK(epistemic_basis IN ('explicit', 'inferred', 'mixed')),
        created_at TEXT NOT NULL,
        UNIQUE(origin_job_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(origin_job_id) REFERENCES research_jobs(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_claims_run
        ON research_claims(run_id, ordinal);

      CREATE TABLE claim_evidence (
        claim_id TEXT NOT NULL,
        evidence_id TEXT NOT NULL,
        relation TEXT NOT NULL CHECK(relation IN ('supports', 'contradicts', 'context')),
        PRIMARY KEY(claim_id, evidence_id, relation),
        FOREIGN KEY(claim_id) REFERENCES research_claims(id) ON DELETE RESTRICT,
        FOREIGN KEY(evidence_id) REFERENCES research_evidence(id) ON DELETE RESTRICT
      );

      CREATE TABLE claim_packs (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        claim_ids_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        UNIQUE(run_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE TABLE contradiction_manifests (
        run_id TEXT PRIMARY KEY,
        unit_count INTEGER NOT NULL CHECK(unit_count >= 0),
        claim_pack_count INTEGER NOT NULL CHECK(claim_pack_count >= 0),
        total_check_count INTEGER NOT NULL CHECK(total_check_count >= 0),
        created_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE TABLE contradiction_checks (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        unit_id TEXT NOT NULL,
        claim_pack_id TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        attempt_id TEXT,
        result_json TEXT,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, unit_id, claim_pack_id),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT,
        FOREIGN KEY(claim_pack_id) REFERENCES claim_packs(id) ON DELETE RESTRICT,
        FOREIGN KEY(attempt_id) REFERENCES task_attempts(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_contradiction_checks_run_status
        ON contradiction_checks(run_id, status, unit_id);

      CREATE TABLE research_contradictions (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        check_id TEXT NOT NULL,
        claim_id TEXT NOT NULL,
        evidence_id TEXT NOT NULL,
        relation TEXT NOT NULL CHECK(relation IN ('contradicts', 'context')),
        explanation TEXT NOT NULL,
        created_at TEXT NOT NULL,
        UNIQUE(check_id, claim_id, evidence_id),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(check_id) REFERENCES contradiction_checks(id) ON DELETE RESTRICT,
        FOREIGN KEY(claim_id) REFERENCES research_claims(id) ON DELETE RESTRICT,
        FOREIGN KEY(evidence_id) REFERENCES research_evidence(id) ON DELETE RESTRICT
      );

      CREATE TABLE research_reports (
        run_id TEXT PRIMARY KEY,
        synthesis_job_id TEXT NOT NULL UNIQUE,
        report_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(synthesis_job_id) REFERENCES research_jobs(id) ON DELETE RESTRICT
      );

      CREATE TABLE research_report_citations (
        run_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        marker INTEGER NOT NULL CHECK(marker > 0),
        evidence_id TEXT NOT NULL,
        citation_id TEXT NOT NULL,
        PRIMARY KEY(run_id, ordinal),
        UNIQUE(run_id, marker),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(evidence_id) REFERENCES research_evidence(id) ON DELETE RESTRICT,
        FOREIGN KEY(citation_id) REFERENCES knowledge_citations(id) ON DELETE RESTRICT
      );
    `);
  }

  private createSchemaV5() {
    this.db.exec(`
      CREATE TABLE research_verification_steps (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        trigger_synthesis_job_id TEXT NOT NULL UNIQUE,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        requests_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        started_at TEXT,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(trigger_synthesis_job_id) REFERENCES research_jobs(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_verification_steps_run
        ON research_verification_steps(run_id, status, ordinal);

      CREATE TABLE research_verification_cells (
        id TEXT PRIMARY KEY,
        step_id TEXT NOT NULL,
        unit_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        result_json TEXT,
        created_at TEXT NOT NULL,
        started_at TEXT,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(step_id, unit_id),
        UNIQUE(step_id, ordinal),
        FOREIGN KEY(step_id) REFERENCES research_verification_steps(id) ON DELETE RESTRICT,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_verification_cells_step
        ON research_verification_cells(step_id, status, ordinal);

      CREATE TABLE research_verification_attempts (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        step_id TEXT NOT NULL,
        cell_id TEXT NOT NULL,
        attempt_number INTEGER NOT NULL CHECK(attempt_number > 0),
        status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'failed', 'canceled')),
        error_code TEXT,
        output_json TEXT,
        started_at TEXT NOT NULL,
        completed_at TEXT,
        UNIQUE(cell_id, attempt_number),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(step_id) REFERENCES research_verification_steps(id) ON DELETE RESTRICT,
        FOREIGN KEY(cell_id) REFERENCES research_verification_cells(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_verification_attempts_run
        ON research_verification_attempts(run_id, started_at);

      CREATE TABLE research_verification_relations (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        step_id TEXT NOT NULL,
        cell_id TEXT NOT NULL,
        claim_id TEXT NOT NULL,
        evidence_id TEXT NOT NULL,
        relation TEXT NOT NULL CHECK(relation IN ('supports', 'contradicts', 'context')),
        explanation TEXT NOT NULL,
        created_at TEXT NOT NULL,
        UNIQUE(cell_id, claim_id, evidence_id, relation),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(step_id) REFERENCES research_verification_steps(id) ON DELETE RESTRICT,
        FOREIGN KEY(cell_id) REFERENCES research_verification_cells(id) ON DELETE RESTRICT,
        FOREIGN KEY(claim_id) REFERENCES research_claims(id) ON DELETE RESTRICT,
        FOREIGN KEY(evidence_id) REFERENCES research_evidence(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_verification_relations_run
        ON research_verification_relations(run_id, relation, created_at);
    `);
  }

  private newId(prefix: string): string {
    return requiredString(this.idGenerator(prefix), `${prefix} id`, 128);
  }

  private activeNotebook(studioId: string, notebookId: string): KnowledgeNotebook {
    const row = this.db.prepare(`
      SELECT * FROM notebooks
      WHERE id = ? AND studio_id = ? AND deleted_at IS NULL
    `).get(notebookId, studioId);
    const notebook = toNotebook(row);
    if (!notebook) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Notebook not found");
    return notebook;
  }

  private activeSource(studioId: string, sourceId: string): KnowledgeSource {
    const row = this.db.prepare(`
      SELECT * FROM sources
      WHERE id = ? AND studio_id = ? AND deleted_at IS NULL
    `).get(sourceId, studioId);
    const source = toSource(row);
    if (!source) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Knowledge source not found");
    return source;
  }

  createNotebook(input: { studioId: unknown; name: unknown }): KnowledgeNotebook {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const name = requiredString(input?.name, "name", 120);
    const id = this.newId("nb");
    const now = this.now();
    this.db.prepare(`
      INSERT INTO notebooks (id, studio_id, name, created_at, updated_at, deleted_at)
      VALUES (?, ?, ?, ?, ?, NULL)
    `).run(id, studioId, name, now, now);
    return this.getNotebook({ studioId, notebookId: id });
  }

  listNotebooks(input: { studioId: unknown }): KnowledgeNotebook[] {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    return this.db.prepare(`
      SELECT * FROM notebooks
      WHERE studio_id = ? AND deleted_at IS NULL
      ORDER BY updated_at DESC, id ASC
    `).all(studioId).map(toNotebook);
  }

  getNotebook(input: { studioId: unknown; notebookId: unknown }): KnowledgeNotebook {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    return this.activeNotebook(studioId, notebookId);
  }

  renameNotebook(input: { studioId: unknown; notebookId: unknown; name: unknown }): KnowledgeNotebook {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const name = requiredString(input?.name, "name", 120);
    this.activeNotebook(studioId, notebookId);
    this.db.prepare(`
      UPDATE notebooks SET name = ?, updated_at = ?
      WHERE id = ? AND studio_id = ? AND deleted_at IS NULL
    `).run(name, this.now(), notebookId, studioId);
    return this.activeNotebook(studioId, notebookId);
  }

  deleteNotebook(input: { studioId: unknown; notebookId: unknown }): KnowledgeNotebook {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const notebook = this.activeNotebook(studioId, notebookId);
    const deletedAt = this.now();
    this.db.transaction(() => {
      this.db.prepare(`
        UPDATE notebooks SET deleted_at = ?, updated_at = ?
        WHERE id = ? AND studio_id = ? AND deleted_at IS NULL
      `).run(deletedAt, deletedAt, notebookId, studioId);
      this.db.prepare(`
        UPDATE notebook_sources SET removed_at = ?
        WHERE notebook_id = ? AND removed_at IS NULL
      `).run(deletedAt, notebookId);
    })();
    return { ...notebook, updatedAt: deletedAt, deletedAt };
  }

  createSourceWithSnapshot(input: {
    studioId: unknown;
    notebookId: unknown;
    sourceId?: unknown;
    snapshotId?: unknown;
    sourceType: unknown;
    displayName: unknown;
    originMetadata: unknown;
    snapshot: {
      sha256: unknown;
      mimeType: unknown;
      byteSize: unknown;
      storagePath: unknown;
    };
  }): ImportedKnowledgeSource {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    this.activeNotebook(studioId, notebookId);
    const id = input.sourceId == null
      ? this.newId("src")
      : requiredString(input.sourceId, "sourceId", 128);
    const snapshotId = input.snapshotId == null
      ? this.newId("snap")
      : requiredString(input.snapshotId, "snapshotId", 128);
    const normalizedType = sourceType(input.sourceType);
    const displayName = requiredString(input.displayName, "displayName", 255);
    const originMetadataJson = serializeObjectJson(input.originMetadata, "originMetadata");
    const snapshotSha = sha256(input.snapshot?.sha256);
    const mimeType = requiredString(input.snapshot?.mimeType, "mimeType", 255).toLowerCase();
    const snapshotBytes = byteSize(input.snapshot?.byteSize);
    const snapshotStoragePath = storagePath(input.snapshot?.storagePath);
    const now = this.now();

    this.db.transaction(() => {
      this.db.prepare(`
        INSERT INTO sources (
          id, studio_id, source_type, display_name, origin_metadata_json, created_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, NULL)
      `).run(id, studioId, normalizedType, displayName, originMetadataJson, now);
      this.db.prepare(`
        INSERT INTO content_snapshots (
          id, source_id, sha256, mime_type, byte_size, storage_path, captured_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
      `).run(snapshotId, id, snapshotSha, mimeType, snapshotBytes, snapshotStoragePath, now);
      this.db.prepare(`
        INSERT INTO notebook_sources (notebook_id, source_id, added_at, removed_at)
        VALUES (?, ?, ?, NULL)
      `).run(notebookId, id, now);
      this.db.prepare(`UPDATE notebooks SET updated_at = ? WHERE id = ?`).run(now, notebookId);
    })();

    return {
      source: this.activeSource(studioId, id),
      snapshot: this.getContentSnapshot({ studioId, snapshotId }),
      membership: this.getMembership(notebookId, id),
    };
  }

  private getMembership(notebookId: string, sourceId: string): NotebookSourceMembership {
    const membership = toMembership(this.db.prepare(`
      SELECT * FROM notebook_sources WHERE notebook_id = ? AND source_id = ?
    `).get(notebookId, sourceId));
    if (!membership) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Notebook source membership not found");
    return membership;
  }

  addSourceToNotebook(input: { studioId: unknown; notebookId: unknown; sourceId: unknown }) {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeNotebook(studioId, notebookId);
    this.activeSource(studioId, sourceId);
    const existing = toMembership(this.db.prepare(`
      SELECT * FROM notebook_sources WHERE notebook_id = ? AND source_id = ?
    `).get(notebookId, sourceId));
    if (existing && existing.removedAt === null) return existing;

    const now = this.now();
    this.db.transaction(() => {
      this.db.prepare(`
        INSERT INTO notebook_sources (notebook_id, source_id, added_at, removed_at)
        VALUES (?, ?, ?, NULL)
        ON CONFLICT(notebook_id, source_id) DO UPDATE SET
          added_at = excluded.added_at,
          removed_at = NULL
      `).run(notebookId, sourceId, now);
      this.db.prepare(`UPDATE notebooks SET updated_at = ? WHERE id = ?`).run(now, notebookId);
    })();
    return this.getMembership(notebookId, sourceId);
  }

  removeSourceFromNotebook(input: { studioId: unknown; notebookId: unknown; sourceId: unknown }) {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeNotebook(studioId, notebookId);
    this.activeSource(studioId, sourceId);
    const membership = this.getMembership(notebookId, sourceId);
    if (membership.removedAt !== null) {
      throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Notebook source membership not found");
    }
    const now = this.now();
    this.db.transaction(() => {
      this.db.prepare(`
        UPDATE notebook_sources SET removed_at = ?
        WHERE notebook_id = ? AND source_id = ? AND removed_at IS NULL
      `).run(now, notebookId, sourceId);
      this.db.prepare(`UPDATE notebooks SET updated_at = ? WHERE id = ?`).run(now, notebookId);
    })();
    return { ...membership, removedAt: now };
  }

  listNotebookSources(input: { studioId: unknown; notebookId: unknown }): Array<{
    source: KnowledgeSource;
    snapshot: ContentSnapshot;
    membership: NotebookSourceMembership;
    parseArtifact: KnowledgeParseArtifact | null;
  }> {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    this.activeNotebook(studioId, notebookId);
    const rows = this.db.prepare(`
      SELECT
        s.*,
        ns.notebook_id AS membership_notebook_id,
        ns.source_id AS membership_source_id,
        ns.added_at AS membership_added_at,
        ns.removed_at AS membership_removed_at,
        cs.id AS snapshot_id,
        cs.sha256 AS snapshot_sha256,
        cs.mime_type AS snapshot_mime_type,
        cs.byte_size AS snapshot_byte_size,
        cs.storage_path AS snapshot_storage_path,
        cs.captured_at AS snapshot_captured_at,
        pa.id AS parse_id,
        pa.content_snapshot_id AS parse_content_snapshot_id,
        pa.parser_id AS parse_parser_id,
        pa.parser_version AS parse_parser_version,
        pa.parser_config_hash AS parse_parser_config_hash,
        pa.status AS parse_status,
        pa.warnings_json AS parse_warnings_json,
        pa.semantic_artifact_path AS parse_semantic_artifact_path,
        pa.created_at AS parse_created_at,
        pa.completed_at AS parse_completed_at
      FROM notebook_sources ns
      JOIN sources s ON s.id = ns.source_id
      JOIN content_snapshots cs ON cs.id = (
        SELECT inner_cs.id FROM content_snapshots inner_cs
        WHERE inner_cs.source_id = s.id
        ORDER BY inner_cs.captured_at DESC, inner_cs.id DESC
        LIMIT 1
      )
      LEFT JOIN parse_artifacts pa ON pa.id = (
        SELECT inner_pa.id FROM parse_artifacts inner_pa
        WHERE inner_pa.content_snapshot_id = cs.id
        ORDER BY inner_pa.created_at DESC, inner_pa.id DESC
        LIMIT 1
      )
      WHERE ns.notebook_id = ?
        AND ns.removed_at IS NULL
        AND s.studio_id = ?
        AND s.deleted_at IS NULL
      ORDER BY ns.added_at ASC, s.id ASC
    `).all(notebookId, studioId);

    return rows.map((row: any) => ({
      source: toSource(row)!,
      membership: {
        notebookId: row.membership_notebook_id,
        sourceId: row.membership_source_id,
        addedAt: row.membership_added_at,
        removedAt: row.membership_removed_at || null,
      },
      snapshot: {
        id: row.snapshot_id,
        sourceId: row.id,
        sha256: row.snapshot_sha256,
        mimeType: row.snapshot_mime_type,
        byteSize: Number(row.snapshot_byte_size),
        storagePath: row.snapshot_storage_path,
        capturedAt: row.snapshot_captured_at,
      },
      parseArtifact: row.parse_id ? {
        id: row.parse_id,
        contentSnapshotId: row.parse_content_snapshot_id,
        parserId: row.parse_parser_id,
        parserVersion: row.parse_parser_version,
        parserConfigHash: row.parse_parser_config_hash,
        status: row.parse_status,
        warnings: parseStringArrayJson(row.parse_warnings_json, "parse warnings"),
        semanticArtifactPath: row.parse_semantic_artifact_path || null,
        createdAt: row.parse_created_at,
        completedAt: row.parse_completed_at || null,
      } : null,
    }));
  }

  getContentSnapshot(input: { studioId: unknown; snapshotId: unknown }): ContentSnapshot {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const snapshotId = requiredString(input?.snapshotId, "snapshotId", 128);
    const snapshot = toSnapshot(this.db.prepare(`
      SELECT cs.*
      FROM content_snapshots cs
      JOIN sources s ON s.id = cs.source_id
      WHERE cs.id = ? AND s.studio_id = ?
    `).get(snapshotId, studioId));
    if (!snapshot) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Content snapshot not found");
    return snapshot;
  }

  createContentSnapshot(input: {
    studioId: unknown;
    sourceId: unknown;
    snapshotId?: unknown;
    sha256: unknown;
    mimeType: unknown;
    byteSize: unknown;
    storagePath: unknown;
  }): ContentSnapshot {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeSource(studioId, sourceId);
    const snapshotSha = sha256(input.sha256);
    const existing = toSnapshot(this.db.prepare(`
      SELECT * FROM content_snapshots WHERE source_id = ? AND sha256 = ?
    `).get(sourceId, snapshotSha));
    if (existing) return existing;
    const snapshotId = input.snapshotId == null
      ? this.newId("snap")
      : requiredString(input.snapshotId, "snapshotId", 128);
    const mimeType = requiredString(input.mimeType, "mimeType", 255).toLowerCase();
    const snapshotBytes = byteSize(input.byteSize);
    const snapshotStoragePath = storagePath(input.storagePath);
    this.db.prepare(`
      INSERT INTO content_snapshots (
        id, source_id, sha256, mime_type, byte_size, storage_path, captured_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?)
    `).run(
      snapshotId,
      sourceId,
      snapshotSha,
      mimeType,
      snapshotBytes,
      snapshotStoragePath,
      this.now(),
    );
    return this.getContentSnapshot({ studioId, snapshotId });
  }

  getSource(input: { studioId: unknown; sourceId: unknown }): KnowledgeSource {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    return this.activeSource(studioId, sourceId);
  }

  getLatestContentSnapshotForSource(input: { studioId: unknown; sourceId: unknown }): ContentSnapshot {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeSource(studioId, sourceId);
    const snapshot = toSnapshot(this.db.prepare(`
      SELECT * FROM content_snapshots
      WHERE source_id = ?
      ORDER BY captured_at DESC, id DESC
      LIMIT 1
    `).get(sourceId));
    if (!snapshot) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Content snapshot not found");
    return snapshot;
  }

  findParseArtifactByIdentity(input: {
    studioId: unknown;
    contentSnapshotId: unknown;
    parserId: unknown;
    parserVersion: unknown;
    parserConfigHash: unknown;
  }): KnowledgeParseArtifact | null {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const contentSnapshotId = requiredString(input?.contentSnapshotId, "contentSnapshotId", 128);
    const parserId = requiredString(input?.parserId, "parserId", 128);
    const parserVersion = requiredString(input?.parserVersion, "parserVersion", 64);
    const parserConfigHash = sha256(input?.parserConfigHash);
    return toParseArtifact(this.db.prepare(`
      SELECT pa.*
      FROM parse_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id
      WHERE pa.content_snapshot_id = ?
        AND pa.parser_id = ?
        AND pa.parser_version = ?
        AND pa.parser_config_hash = ?
        AND s.studio_id = ?
    `).get(contentSnapshotId, parserId, parserVersion, parserConfigHash, studioId));
  }

  beginParseArtifact(input: {
    studioId: unknown;
    contentSnapshotId: unknown;
    parseArtifactId?: unknown;
    parserId: unknown;
    parserVersion: unknown;
    parserConfigHash: unknown;
  }): KnowledgeParseArtifact {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const contentSnapshotId = requiredString(input?.contentSnapshotId, "contentSnapshotId", 128);
    // 先经过 studio 归属检查，不能靠外键存在性代替授权。
    this.getContentSnapshot({ studioId, snapshotId: contentSnapshotId });
    const parserId = requiredString(input?.parserId, "parserId", 128);
    const parserVersion = requiredString(input?.parserVersion, "parserVersion", 64);
    const parserConfigHash = sha256(input?.parserConfigHash);
    const existing = this.findParseArtifactByIdentity({
      studioId,
      contentSnapshotId,
      parserId,
      parserVersion,
      parserConfigHash,
    });
    if (existing) {
      this.db.transaction(() => {
        this.db.prepare(`DELETE FROM knowledge_citations WHERE parse_artifact_id = ?`).run(existing.id);
        this.db.prepare(`DELETE FROM knowledge_blocks WHERE parse_artifact_id = ?`).run(existing.id);
        this.db.prepare(`
          UPDATE parse_artifacts
          SET status = 'parsing', warnings_json = '[]', semantic_artifact_path = NULL, completed_at = NULL
          WHERE id = ?
        `).run(existing.id);
      })();
      return this.getParseArtifact({ studioId, parseArtifactId: existing.id });
    }

    const id = input.parseArtifactId == null
      ? this.newId("parse")
      : requiredString(input.parseArtifactId, "parseArtifactId", 128);
    const now = this.now();
    this.db.prepare(`
      INSERT INTO parse_artifacts (
        id, content_snapshot_id, parser_id, parser_version, parser_config_hash,
        status, warnings_json, semantic_artifact_path, created_at, completed_at
      ) VALUES (?, ?, ?, ?, ?, 'parsing', '[]', NULL, ?, NULL)
    `).run(id, contentSnapshotId, parserId, parserVersion, parserConfigHash, now);
    return this.getParseArtifact({ studioId, parseArtifactId: id });
  }

  completeParseArtifact(input: {
    studioId: unknown;
    parseArtifactId: unknown;
    status: "ready" | "needs_ocr";
    warnings: unknown;
    semanticArtifactPath: unknown;
    blocks: KnowledgeBlockDraft[];
  }): KnowledgeParseArtifact {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    this.getParseArtifact({ studioId, parseArtifactId });
    if (input.status !== "ready" && input.status !== "needs_ocr") {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Parse completion status is invalid");
    }
    const warningsJson = serializeStringArray(input.warnings, "warnings");
    const semanticArtifactPath = storagePath(input.semanticArtifactPath);
    if (!Array.isArray(input.blocks)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "blocks must be an array");
    }
    if (input.status === "needs_ocr" && input.blocks.length > 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "needs_ocr artifacts cannot contain blocks");
    }
    if (input.status === "ready" && input.blocks.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "ready artifacts require at least one block");
    }

    const normalizedBlocks = input.blocks.map((block, index) => {
      if (block.ordinal !== index) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Block ordinals must be contiguous");
      }
      const text = requiredString(block.text, "block text", 2_000_000);
      const locatorType = block.locatorType;
      if (!new Set(["text", "markdown", "pdf", "html"]).has(locatorType)) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Block locator type is invalid");
      }
      return {
        id: this.newId("block"),
        ordinal: index,
        text,
        textSha256: crypto.createHash("sha256").update(text, "utf8").digest("hex"),
        locatorType,
        locatorJson: serializeObjectJson(block.locator, "block locator"),
      };
    });
    const completedAt = this.now();
    this.db.transaction(() => {
      this.db.prepare(`DELETE FROM knowledge_citations WHERE parse_artifact_id = ?`).run(parseArtifactId);
      this.db.prepare(`DELETE FROM knowledge_blocks WHERE parse_artifact_id = ?`).run(parseArtifactId);
      const insert = this.db.prepare(`
        INSERT INTO knowledge_blocks (
          id, parse_artifact_id, ordinal, text, text_sha256, locator_type, locator_payload_json
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
      `);
      for (const block of normalizedBlocks) {
        insert.run(
          block.id,
          parseArtifactId,
          block.ordinal,
          block.text,
          block.textSha256,
          block.locatorType,
          block.locatorJson,
        );
      }
      this.db.prepare(`
        UPDATE parse_artifacts
        SET status = ?, warnings_json = ?, semantic_artifact_path = ?, completed_at = ?
        WHERE id = ?
      `).run(input.status, warningsJson, semanticArtifactPath, completedAt, parseArtifactId);
    })();
    return this.getParseArtifact({ studioId, parseArtifactId });
  }

  failParseArtifact(input: {
    studioId: unknown;
    parseArtifactId: unknown;
    warnings?: unknown;
  }): KnowledgeParseArtifact {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    this.getParseArtifact({ studioId, parseArtifactId });
    const warningsJson = serializeStringArray(input.warnings ?? ["parse_failed"], "warnings");
    const completedAt = this.now();
    this.db.transaction(() => {
      this.db.prepare(`DELETE FROM knowledge_citations WHERE parse_artifact_id = ?`).run(parseArtifactId);
      this.db.prepare(`DELETE FROM knowledge_blocks WHERE parse_artifact_id = ?`).run(parseArtifactId);
      this.db.prepare(`
        UPDATE parse_artifacts
        SET status = 'failed', warnings_json = ?, semantic_artifact_path = NULL, completed_at = ?
        WHERE id = ?
      `).run(warningsJson, completedAt, parseArtifactId);
    })();
    return this.getParseArtifact({ studioId, parseArtifactId });
  }

  getParseArtifact(input: { studioId: unknown; parseArtifactId: unknown }): KnowledgeParseArtifact {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    const artifact = toParseArtifact(this.db.prepare(`
      SELECT pa.*
      FROM parse_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id
      WHERE pa.id = ? AND s.studio_id = ?
    `).get(parseArtifactId, studioId));
    if (!artifact) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Parse artifact not found");
    if (!PARSE_STATUSES.has(artifact.status)) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Parse artifact status is invalid");
    }
    return artifact;
  }

  listArtifactBlocks(input: { studioId: unknown; parseArtifactId: unknown }): KnowledgeBlock[] {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    this.getParseArtifact({ studioId, parseArtifactId });
    return this.db.prepare(`
      SELECT * FROM knowledge_blocks
      WHERE parse_artifact_id = ?
      ORDER BY ordinal ASC
    `).all(parseArtifactId).map(toBlock);
  }

  createCitation(input: {
    studioId: unknown;
    parseArtifactId: unknown;
    blockId: unknown;
    startOffset: unknown;
    endOffset: unknown;
  }): KnowledgeCitation {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    const artifact = this.getParseArtifact({ studioId, parseArtifactId });
    if (artifact.status !== "ready") {
      throw new KnowledgeError("KNOWLEDGE_PARSE_NOT_READY", "Parse artifact is not ready for citation");
    }
    const blockId = requiredString(input?.blockId, "blockId", 128);
    const block = toBlock(this.db.prepare(`
      SELECT * FROM knowledge_blocks WHERE id = ? AND parse_artifact_id = ?
    `).get(blockId, parseArtifactId));
    if (!block) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Knowledge block not found");
    if (!Number.isSafeInteger(input.startOffset) || !Number.isSafeInteger(input.endOffset)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Citation offsets must be integers");
    }
    const startOffset = Number(input.startOffset);
    const endOffset = Number(input.endOffset);
    if (startOffset < 0 || endOffset <= startOffset || endOffset > block.text.length) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Citation offsets are outside the block");
    }
    const canonicalText = block.text.slice(startOffset, endOffset);
    const id = this.newId("cite");
    const createdAt = this.now();
    const canonicalTextSha256 = crypto.createHash("sha256").update(canonicalText, "utf8").digest("hex");
    this.db.prepare(`
      INSERT INTO knowledge_citations (
        id, parse_artifact_id, block_id, start_offset, end_offset,
        canonical_text, canonical_text_sha256, created_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
    `).run(
      id,
      parseArtifactId,
      blockId,
      startOffset,
      endOffset,
      canonicalText,
      canonicalTextSha256,
      createdAt,
    );
    return toCitation(this.db.prepare(`SELECT * FROM knowledge_citations WHERE id = ?`).get(id))!;
  }

  resolveCitation(input: { studioId: unknown; citationId: unknown }): ResolvedKnowledgeCitation {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const citationId = requiredString(input?.citationId, "citationId", 128);
    const row = this.db.prepare(`
      SELECT
        kc.id AS citation_id,
        kc.parse_artifact_id AS citation_parse_artifact_id,
        kc.block_id AS citation_block_id,
        kc.start_offset AS citation_start_offset,
        kc.end_offset AS citation_end_offset,
        kc.canonical_text AS citation_canonical_text,
        kc.canonical_text_sha256 AS citation_canonical_text_sha256,
        kc.created_at AS citation_created_at,
        kb.id AS block_id,
        kb.parse_artifact_id AS block_parse_artifact_id,
        kb.ordinal AS block_ordinal,
        kb.text AS block_text,
        kb.text_sha256 AS block_text_sha256,
        kb.locator_type AS block_locator_type,
        kb.locator_payload_json AS block_locator_payload_json,
        pa.id AS artifact_id,
        pa.content_snapshot_id AS artifact_content_snapshot_id,
        pa.parser_id AS artifact_parser_id,
        pa.parser_version AS artifact_parser_version,
        pa.parser_config_hash AS artifact_parser_config_hash,
        pa.status AS artifact_status,
        pa.warnings_json AS artifact_warnings_json,
        pa.semantic_artifact_path AS artifact_semantic_artifact_path,
        pa.created_at AS artifact_created_at,
        pa.completed_at AS artifact_completed_at,
        cs.id AS snapshot_id,
        cs.source_id AS snapshot_source_id,
        cs.sha256 AS snapshot_sha256,
        cs.mime_type AS snapshot_mime_type,
        cs.byte_size AS snapshot_byte_size,
        cs.storage_path AS snapshot_storage_path,
        cs.captured_at AS snapshot_captured_at,
        s.id AS source_id,
        s.studio_id AS source_studio_id,
        s.source_type AS source_type,
        s.display_name AS source_display_name,
        s.origin_metadata_json AS source_origin_metadata_json,
        s.created_at AS source_created_at,
        s.deleted_at AS source_deleted_at
      FROM knowledge_citations kc
      JOIN knowledge_blocks kb ON kb.id = kc.block_id AND kb.parse_artifact_id = kc.parse_artifact_id
      JOIN parse_artifacts pa ON pa.id = kc.parse_artifact_id
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id
      WHERE kc.id = ? AND s.studio_id = ?
    `).get(citationId, studioId);
    if (!row) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Knowledge citation not found");

    return {
      citation: {
        id: row.citation_id,
        parseArtifactId: row.citation_parse_artifact_id,
        blockId: row.citation_block_id,
        startOffset: Number(row.citation_start_offset),
        endOffset: Number(row.citation_end_offset),
        canonicalText: row.citation_canonical_text,
        canonicalTextSha256: row.citation_canonical_text_sha256,
        createdAt: row.citation_created_at,
      },
      block: {
        id: row.block_id,
        parseArtifactId: row.block_parse_artifact_id,
        ordinal: Number(row.block_ordinal),
        text: row.block_text,
        textSha256: row.block_text_sha256,
        locatorType: row.block_locator_type,
        locator: parseObjectJson(row.block_locator_payload_json, "block locator"),
      },
      artifact: {
        id: row.artifact_id,
        contentSnapshotId: row.artifact_content_snapshot_id,
        parserId: row.artifact_parser_id,
        parserVersion: row.artifact_parser_version,
        parserConfigHash: row.artifact_parser_config_hash,
        status: row.artifact_status,
        warnings: parseStringArrayJson(row.artifact_warnings_json, "parse warnings"),
        semanticArtifactPath: row.artifact_semantic_artifact_path || null,
        createdAt: row.artifact_created_at,
        completedAt: row.artifact_completed_at || null,
      },
      snapshot: {
        id: row.snapshot_id,
        sourceId: row.snapshot_source_id,
        sha256: row.snapshot_sha256,
        mimeType: row.snapshot_mime_type,
        byteSize: Number(row.snapshot_byte_size),
        storagePath: row.snapshot_storage_path,
        capturedAt: row.snapshot_captured_at,
      },
      source: {
        id: row.source_id,
        studioId: row.source_studio_id,
        sourceType: row.source_type,
        displayName: row.source_display_name,
        originMetadata: parseObjectJson(row.source_origin_metadata_json, "origin metadata"),
        createdAt: row.source_created_at,
        deletedAt: row.source_deleted_at || null,
      },
    };
  }

  createScopeSnapshot(input: {
    studioId: unknown;
    notebookIds: unknown;
    mode: unknown;
  }): KnowledgeScopeSnapshot {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    if (!Array.isArray(input?.notebookIds) || input.notebookIds.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_SCOPE_EMPTY", "At least one Notebook is required");
    }
    if (input.notebookIds.length > 16) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge query selects too many Notebooks");
    }
    const notebookIds = input.notebookIds.map(id => requiredString(id, "notebookId", 128));
    if (new Set(notebookIds).size !== notebookIds.length) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge query contains duplicate Notebooks");
    }
    if (typeof input.mode !== "string" || !QUERY_MODES.has(input.mode as KnowledgeQueryMode)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge query mode is invalid");
    }
    const mode = input.mode as KnowledgeQueryMode;
    const scopeSnapshotId = this.newId("scope");
    const createdAt = this.now();

    this.db.transaction(() => {
      this.db.prepare(`
        INSERT INTO scope_snapshots (id, studio_id, mode, created_at)
        VALUES (?, ?, ?, ?)
      `).run(scopeSnapshotId, studioId, mode, createdAt);

      const insertNotebook = this.db.prepare(`
        INSERT INTO scope_notebooks (
          scope_snapshot_id, notebook_id, notebook_name, ordinal
        ) VALUES (?, ?, ?, ?)
      `);
      const insertSource = this.db.prepare(`
        INSERT INTO scope_sources (
          scope_snapshot_id, notebook_id, source_id, source_display_name,
          content_snapshot_id, parse_artifact_id, ordinal
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
      `);
      const notReady: Array<{ sourceId: string; displayName: string; status: string }> = [];
      let sourceOrdinal = 0;

      notebookIds.forEach((notebookId, notebookOrdinal) => {
        const notebook = this.activeNotebook(studioId, notebookId);
        insertNotebook.run(scopeSnapshotId, notebook.id, notebook.name, notebookOrdinal);
        const entries = this.listNotebookSources({ studioId, notebookId });
        for (const entry of entries) {
          const status = entry.parseArtifact?.status || "not_parsed";
          if (status !== "ready") {
            notReady.push({
              sourceId: entry.source.id,
              displayName: entry.source.displayName,
              status,
            });
            continue;
          }
          insertSource.run(
            scopeSnapshotId,
            notebook.id,
            entry.source.id,
            entry.source.displayName,
            entry.snapshot.id,
            entry.parseArtifact!.id,
            sourceOrdinal,
          );
          sourceOrdinal += 1;
        }
      });

      if (notReady.length > 0) {
        throw new KnowledgeError(
          "KNOWLEDGE_SCOPE_NOT_READY",
          "Selected Notebooks contain sources that are not ready",
          { unreadyCount: notReady.length, sources: notReady.slice(0, 50) },
        );
      }
      if (sourceOrdinal === 0) {
        throw new KnowledgeError("KNOWLEDGE_SCOPE_EMPTY", "Selected Notebooks contain no ready sources");
      }
    })();

    return this.getScopeSnapshot({ studioId, scopeSnapshotId });
  }

  getScopeSnapshot(input: { studioId: unknown; scopeSnapshotId: unknown }): KnowledgeScopeSnapshot {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const scopeSnapshotId = requiredString(input?.scopeSnapshotId, "scopeSnapshotId", 128);
    const row = this.db.prepare(`
      SELECT * FROM scope_snapshots WHERE id = ? AND studio_id = ?
    `).get(scopeSnapshotId, studioId);
    if (!row) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Knowledge scope snapshot not found");
    const notebooks = this.db.prepare(`
      SELECT * FROM scope_notebooks
      WHERE scope_snapshot_id = ?
      ORDER BY ordinal ASC
    `).all(scopeSnapshotId).map((entry: any) => ({
      scopeSnapshotId: entry.scope_snapshot_id,
      notebookId: entry.notebook_id,
      notebookName: entry.notebook_name,
      ordinal: Number(entry.ordinal),
    }));
    const sources = this.db.prepare(`
      SELECT * FROM scope_sources
      WHERE scope_snapshot_id = ?
      ORDER BY ordinal ASC
    `).all(scopeSnapshotId).map((entry: any) => ({
      scopeSnapshotId: entry.scope_snapshot_id,
      notebookId: entry.notebook_id,
      sourceId: entry.source_id,
      sourceDisplayName: entry.source_display_name,
      contentSnapshotId: entry.content_snapshot_id,
      parseArtifactId: entry.parse_artifact_id,
      ordinal: Number(entry.ordinal),
    }));
    return {
      id: row.id,
      studioId: row.studio_id,
      mode: row.mode,
      createdAt: row.created_at,
      notebooks,
      sources,
    };
  }

  createKnowledgeRun(input: {
    studioId: unknown;
    mode: unknown;
    question: unknown;
    scopeSnapshotId: unknown;
    retrievalMode?: unknown;
  }): KnowledgeRun {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    if (typeof input.mode !== "string" || !QUERY_MODES.has(input.mode as KnowledgeQueryMode)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge query mode is invalid");
    }
    const mode = input.mode as KnowledgeQueryMode;
    const question = requiredString(input.question, "question", 4000);
    const scopeSnapshotId = requiredString(input.scopeSnapshotId, "scopeSnapshotId", 128);
    const scope = this.getScopeSnapshot({ studioId, scopeSnapshotId });
    if (scope.mode !== mode) {
      throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Knowledge scope mode does not match the run mode");
    }
    const retrievalMode = input.retrievalMode == null ? "fts" : input.retrievalMode;
    if (retrievalMode !== "fts" && retrievalMode !== "hybrid") {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge retrieval mode is invalid");
    }
    const id = this.newId("krun");
    const createdAt = this.now();
    this.db.prepare(`
      INSERT INTO knowledge_runs (
        id, studio_id, mode, question, scope_snapshot_id, status,
        retrieval_mode, answer_text, error_code, created_at, completed_at
      ) VALUES (?, ?, ?, ?, ?, 'running', ?, NULL, NULL, ?, NULL)
    `).run(id, studioId, mode, question, scopeSnapshotId, retrievalMode, createdAt);
    return this.getKnowledgeRun({ studioId, runId: id });
  }

  setKnowledgeRunRetrievalMode(input: {
    studioId: unknown;
    runId: unknown;
    retrievalMode: unknown;
  }): KnowledgeRun {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const runId = requiredString(input?.runId, "runId", 128);
    if (input.retrievalMode !== "fts" && input.retrievalMode !== "hybrid") {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge retrieval mode is invalid");
    }
    const run = this.getKnowledgeRun({ studioId, runId });
    if (run.status !== "running") {
      throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Knowledge run is not active");
    }
    this.db.prepare(`
      UPDATE knowledge_runs SET retrieval_mode = ?
      WHERE id = ? AND studio_id = ? AND status = 'running'
    `).run(input.retrievalMode, runId, studioId);
    return this.getKnowledgeRun({ studioId, runId });
  }

  recordRunRetrievals(input: {
    studioId: unknown;
    runId: unknown;
    retrievals: Array<{ chunkId: unknown; parseArtifactId: unknown; score: unknown }>;
  }): KnowledgeRunRetrieval[] {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const runId = requiredString(input?.runId, "runId", 128);
    const run = this.getKnowledgeRun({ studioId, runId });
    if (run.status !== "running") {
      throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Knowledge run is not active");
    }
    if (!Array.isArray(input.retrievals) || input.retrievals.length === 0 || input.retrievals.length > 50) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge retrieval result is invalid");
    }
    const allowedArtifacts = new Set(
      this.getScopeSnapshot({ studioId, scopeSnapshotId: run.scopeSnapshotId })
        .sources.map(source => source.parseArtifactId),
    );
    const normalized = input.retrievals.map((entry, index) => {
      const chunkId = requiredString(entry.chunkId, "chunkId", 128);
      const parseArtifactId = requiredString(entry.parseArtifactId, "parseArtifactId", 128);
      const score = Number(entry.score);
      if (!allowedArtifacts.has(parseArtifactId) || !Number.isFinite(score)) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge retrieval escaped its frozen scope");
      }
      return { runId, rank: index + 1, chunkId, parseArtifactId, score };
    });
    this.db.transaction(() => {
      this.db.prepare(`DELETE FROM knowledge_run_retrievals WHERE run_id = ?`).run(runId);
      const insert = this.db.prepare(`
        INSERT INTO knowledge_run_retrievals (
          run_id, rank, chunk_id, parse_artifact_id, score
        ) VALUES (?, ?, ?, ?, ?)
      `);
      for (const entry of normalized) {
        insert.run(entry.runId, entry.rank, entry.chunkId, entry.parseArtifactId, entry.score);
      }
    })();
    return normalized;
  }

  commitQuickRun(input: {
    studioId: unknown;
    runId: unknown;
    answerText: unknown;
    citations: Array<{
      marker: unknown;
      candidateRef: unknown;
      parseArtifactId: unknown;
      blockId: unknown;
      startOffset: unknown;
      endOffset: unknown;
    }>;
  }): KnowledgeRun {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const runId = requiredString(input?.runId, "runId", 128);
    const answerText = requiredString(input?.answerText, "answerText", 200_000);
    const run = this.getKnowledgeRun({ studioId, runId });
    if (run.mode !== "quick" || run.status !== "running") {
      throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Quick Answer run is not active");
    }
    if (!Array.isArray(input.citations) || input.citations.length === 0 || input.citations.length > 100) {
      throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Quick Answer requires validated citations");
    }
    const allowedArtifacts = new Set(
      this.getScopeSnapshot({ studioId, scopeSnapshotId: run.scopeSnapshotId })
        .sources.map(source => source.parseArtifactId),
    );
    const seenMarkers = new Set<number>();
    const normalized = input.citations.map(entry => {
      const marker = Number(entry.marker);
      const parseArtifactId = requiredString(entry.parseArtifactId, "parseArtifactId", 128);
      if (
        !Number.isSafeInteger(marker)
        || marker <= 0
        || seenMarkers.has(marker)
        || !answerText.includes(`[${marker}]`)
        || !allowedArtifacts.has(parseArtifactId)
      ) {
        throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Quick Answer citation is invalid");
      }
      seenMarkers.add(marker);
      return {
        marker,
        candidateRef: requiredString(entry.candidateRef, "candidateRef", 64),
        parseArtifactId,
        blockId: requiredString(entry.blockId, "blockId", 128),
        startOffset: entry.startOffset,
        endOffset: entry.endOffset,
      };
    });
    const completedAt = this.now();

    this.db.transaction(() => {
      const insertRef = this.db.prepare(`
        INSERT INTO knowledge_run_citations (
          run_id, ordinal, marker, citation_id, candidate_ref
        ) VALUES (?, ?, ?, ?, ?)
      `);
      normalized.forEach((entry, ordinal) => {
        const citation = this.createCitation({
          studioId,
          parseArtifactId: entry.parseArtifactId,
          blockId: entry.blockId,
          startOffset: entry.startOffset,
          endOffset: entry.endOffset,
        });
        insertRef.run(runId, ordinal, entry.marker, citation.id, entry.candidateRef);
      });
      this.db.prepare(`
        UPDATE knowledge_runs
        SET status = 'completed', answer_text = ?, error_code = NULL, completed_at = ?
        WHERE id = ? AND status = 'running'
      `).run(answerText, completedAt, runId);
    })();
    return this.getKnowledgeRun({ studioId, runId });
  }

  failKnowledgeRun(input: { studioId: unknown; runId: unknown; errorCode: unknown }): KnowledgeRun {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const runId = requiredString(input?.runId, "runId", 128);
    const errorCode = requiredString(input?.errorCode, "errorCode", 128);
    const run = this.getKnowledgeRun({ studioId, runId });
    if (run.status !== "running") return run;
    this.db.prepare(`
      UPDATE knowledge_runs
      SET status = 'failed', error_code = ?, completed_at = ?
      WHERE id = ? AND studio_id = ? AND status = 'running'
    `).run(errorCode, this.now(), runId, studioId);
    return this.getKnowledgeRun({ studioId, runId });
  }

  getKnowledgeRun(input: { studioId: unknown; runId: unknown }): KnowledgeRun {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const runId = requiredString(input?.runId, "runId", 128);
    const row = this.db.prepare(`
      SELECT * FROM knowledge_runs WHERE id = ? AND studio_id = ?
    `).get(runId, studioId);
    if (!row) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Knowledge run not found");
    const citations: KnowledgeRunCitationRef[] = this.db.prepare(`
      SELECT * FROM knowledge_run_citations
      WHERE run_id = ?
      ORDER BY ordinal ASC
    `).all(runId).map((entry: any) => ({
      runId: entry.run_id,
      ordinal: Number(entry.ordinal),
      marker: Number(entry.marker),
      citationId: entry.citation_id,
      candidateRef: entry.candidate_ref,
    }));
    const retrievals: KnowledgeRunRetrieval[] = this.db.prepare(`
      SELECT * FROM knowledge_run_retrievals
      WHERE run_id = ?
      ORDER BY rank ASC
    `).all(runId).map((entry: any) => ({
      runId: entry.run_id,
      rank: Number(entry.rank),
      chunkId: entry.chunk_id,
      parseArtifactId: entry.parse_artifact_id,
      score: Number(entry.score),
    }));
    return {
      id: row.id,
      studioId: row.studio_id,
      mode: row.mode,
      question: row.question,
      scopeSnapshotId: row.scope_snapshot_id,
      status: row.status,
      retrievalMode: row.retrieval_mode,
      answerText: row.answer_text || null,
      errorCode: row.error_code || null,
      createdAt: row.created_at,
      completedAt: row.completed_at || null,
      citations,
      retrievals,
    };
  }

  countContentSnapshots(input: { studioId: unknown; sourceId: unknown }): number {
    const source = this.getSource(input);
    return Number(this.db.prepare(`
      SELECT COUNT(*) AS count FROM content_snapshots WHERE source_id = ?
    `).get(source.id).count);
  }

  countParseArtifacts(input: { studioId: unknown; sourceId: unknown }): number {
    const source = this.getSource(input);
    return Number(this.db.prepare(`
      SELECT COUNT(*) AS count
      FROM parse_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      WHERE cs.source_id = ?
    `).get(source.id).count);
  }

  close() {
    if (!this.db) return;
    this.db.close();
    this.db = null;
  }
}

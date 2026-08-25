import type {
  KnowledgeAnalysisManifest,
  KnowledgeAnalysisUnit,
  KnowledgeAnalysisUnitSpan,
  KnowledgeClaimEvidenceRelation,
  KnowledgeEpistemicBasis,
  KnowledgeResearchCoverage,
  KnowledgeResearchReport,
  KnowledgeResearchRun,
  KnowledgeResearchSpec,
  KnowledgeResearchState,
  KnowledgeSupportStatus,
} from "./types.ts";
import { KnowledgeError } from "./errors.ts";

const ACTIVE_RESEARCH_STATES = new Set<KnowledgeResearchState>([
  "queued",
  "preparing_scope",
  "building_manifest",
  "scanning",
  "building_claims",
  "checking_contradictions",
  "synthesizing",
  "recovering",
]);

function requiredText(value: unknown, field: string, maxLength = 256): string {
  if (typeof value !== "string" || !value.trim() || value.trim().length > maxLength) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} is invalid`);
  }
  return value.trim();
}

function safeJson(value: unknown, field: string, maxBytes = 2_000_000): string {
  const serialized = JSON.stringify(value);
  if (typeof serialized !== "string" || Buffer.byteLength(serialized, "utf8") > maxBytes) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} is too large`);
  }
  return serialized;
}

function parseJson<T>(value: unknown, field: string): T {
  try {
    return JSON.parse(typeof value === "string" ? value : "") as T;
  } catch {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", `${field} is corrupt`);
  }
}

function integer(value: unknown, field: string, minimum = 0): number {
  const normalized = Number(value);
  if (!Number.isSafeInteger(normalized) || normalized < minimum) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} is invalid`);
  }
  return normalized;
}

export interface ResearchUnitDraft {
  id: string;
  parseArtifactId: string;
  ordinal: number;
  priority: number;
  primaryCharCount: number;
  contextCharCount: number;
  spans: KnowledgeAnalysisUnitSpan[];
}

export interface ResearchBatchDraft {
  id: string;
  ordinal: number;
  estimatedChars: number;
  unitIds: string[];
}

export interface ResearchExecutionBatch {
  id: string;
  runId: string;
  ordinal: number;
  status: "pending" | "running" | "completed" | "failed" | "canceled";
  estimatedChars: number;
  unitIds: string[];
  startedAt: string | null;
  completedAt: string | null;
  errorCode: string | null;
}

export interface ResearchTaskAttempt {
  id: string;
  runId: string;
  workType: "scan_batch" | "claim_job" | "contradiction_check" | "synthesis_job";
  workId: string;
  attemptNumber: number;
  status: "running" | "completed" | "failed" | "canceled";
  errorCode: string | null;
  startedAt: string;
  completedAt: string | null;
}

export interface ResearchJob {
  id: string;
  runId: string;
  phase: "claim_build" | "final_synthesis";
  ordinal: number;
  status: "pending" | "running" | "completed" | "failed" | "canceled";
  inputRefs: unknown;
  output: unknown | null;
  errorCode: string | null;
}

export interface ResearchEvidence {
  id: string;
  runId: string;
  unitId: string;
  validationId: string;
  citationId: string;
  contentSnapshotId: string;
  parseArtifactId: string;
  blockId: string;
  startOffset: number;
  endOffset: number;
  canonicalQuote: string;
  quoteChecksum: string;
  epistemicBasis: KnowledgeEpistemicBasis;
  createdAt: string;
}

export interface ResearchClaim {
  id: string;
  runId: string;
  originJobId: string;
  ordinal: number;
  text: string;
  supportStatus: KnowledgeSupportStatus;
  epistemicBasis: KnowledgeEpistemicBasis;
  evidence: Array<{ evidenceId: string; relation: KnowledgeClaimEvidenceRelation }>;
}

export interface ResearchClaimPack {
  id: string;
  runId: string;
  ordinal: number;
  claimIds: string[];
}

export interface ResearchContradictionCheck {
  id: string;
  runId: string;
  unitId: string;
  claimPackId: string;
  status: "pending" | "running" | "completed" | "failed" | "canceled";
  attemptId: string | null;
  result: unknown | null;
  errorCode: string | null;
}

export interface ResearchVerificationStep {
  id: string;
  runId: string;
  triggerSynthesisJobId: string;
  ordinal: number;
  status: "pending" | "running" | "completed" | "failed" | "canceled";
  requests: Array<{ claimId: string; reason: string }>;
  errorCode: string | null;
}

export interface ResearchVerificationCell {
  id: string;
  stepId: string;
  unitId: string;
  ordinal: number;
  status: "pending" | "running" | "completed" | "failed" | "canceled";
  result: unknown | null;
  errorCode: string | null;
}

export interface ResearchVerificationAttempt {
  id: string;
  runId: string;
  stepId: string;
  cellId: string;
  attemptNumber: number;
  status: "running" | "completed" | "failed" | "canceled";
  errorCode: string | null;
}

interface ResearchStoreOptions {
  db: any;
  now: () => string;
  idGenerator: (prefix: string) => string;
}

/** 全文研究的持久化账本；与宿主任务登记器保持职责分离。 */
export class KnowledgeResearchStore {
  private readonly db: any;
  private readonly now: () => string;
  private readonly idGenerator: (prefix: string) => string;

  constructor(options: ResearchStoreOptions) {
    this.db = options.db;
    this.now = options.now;
    this.idGenerator = options.idGenerator;
  }

  transaction<T>(work: () => T): T {
    return this.db.transaction(work)();
  }

  private newId(prefix: string): string {
    return requiredText(this.idGenerator(prefix), `${prefix} id`, 128);
  }

  createResearchRun(input: {
    studioId: unknown;
    runId: unknown;
    spec: KnowledgeResearchSpec;
  }): KnowledgeResearchRun {
    const studioId = requiredText(input.studioId, "studioId");
    const runId = requiredText(input.runId, "runId", 128);
    const base = this.db.prepare(`
      SELECT id, mode FROM knowledge_runs WHERE id = ? AND studio_id = ?
    `).get(runId, studioId);
    if (!base || base.mode !== "research") {
      throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Research run base record is invalid");
    }
    const now = this.now();
    this.db.prepare(`
      INSERT INTO research_runs (
        run_id, host_task_id, state, spec_json, error_code, created_at, updated_at, completed_at
      ) VALUES (?, ?, 'building_manifest', ?, NULL, ?, ?, NULL)
    `).run(runId, `knowledge-research:${runId}`, safeJson(input.spec, "ResearchSpec"), now, now);
    return this.getResearchRun({ studioId, runId });
  }

  setState(input: {
    studioId: unknown;
    runId: unknown;
    state: KnowledgeResearchState;
    errorCode?: string | null;
  }): KnowledgeResearchRun {
    const studioId = requiredText(input.studioId, "studioId");
    const runId = requiredText(input.runId, "runId", 128);
    this.assertStudioRun(studioId, runId);
    const terminal = ["completed", "partial", "failed", "canceled"].includes(input.state);
    this.db.prepare(`
      UPDATE research_runs
      SET state = ?, error_code = ?, updated_at = ?, completed_at = ?
      WHERE run_id = ?
    `).run(
      input.state,
      input.errorCode || null,
      this.now(),
      terminal ? this.now() : null,
      runId,
    );
    return this.getResearchRun({ studioId, runId });
  }

  private assertStudioRun(studioId: string, runId: string) {
    const row = this.db.prepare(`
      SELECT rr.run_id FROM research_runs rr
      JOIN knowledge_runs kr ON kr.id = rr.run_id
      WHERE rr.run_id = ? AND kr.studio_id = ?
    `).get(runId, studioId);
    if (!row) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Research run not found");
  }

  getResearchRun(input: { studioId: unknown; runId: unknown }): KnowledgeResearchRun {
    const studioId = requiredText(input.studioId, "studioId");
    const runId = requiredText(input.runId, "runId", 128);
    const row = this.db.prepare(`
      SELECT rr.* FROM research_runs rr
      JOIN knowledge_runs kr ON kr.id = rr.run_id
      WHERE rr.run_id = ? AND kr.studio_id = ?
    `).get(runId, studioId);
    if (!row) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Research run not found");
    return {
      runId: row.run_id,
      hostTaskId: row.host_task_id,
      state: row.state,
      spec: parseJson<KnowledgeResearchSpec>(row.spec_json, "ResearchSpec"),
      manifest: this.getManifest(runId),
      coverage: this.getCoverage(runId),
      reportAvailable: Boolean(this.db.prepare(
        "SELECT 1 FROM research_reports WHERE run_id = ?",
      ).get(runId)),
      errorCode: row.error_code || null,
      createdAt: row.created_at,
      updatedAt: row.updated_at,
      completedAt: row.completed_at || null,
    };
  }

  listActiveResearchRuns(): Array<{ studioId: string; runId: string; hostTaskId: string }> {
    const states = [...ACTIVE_RESEARCH_STATES];
    const placeholders = states.map(() => "?").join(", ");
    return this.db.prepare(`
      SELECT kr.studio_id, rr.run_id, rr.host_task_id
      FROM research_runs rr
      JOIN knowledge_runs kr ON kr.id = rr.run_id
      WHERE rr.state IN (${placeholders}) AND kr.status = 'running'
      ORDER BY rr.created_at ASC
    `).all(...states).map((row: any) => ({
      studioId: row.studio_id,
      runId: row.run_id,
      hostTaskId: row.host_task_id,
    }));
  }

  listActiveResearchRunsForStudio(input: { studioId: unknown }): KnowledgeResearchRun[] {
    const studioId = requiredText(input.studioId, "studioId");
    const states = [...ACTIVE_RESEARCH_STATES];
    const placeholders = states.map(() => "?").join(", ");
    const runIds = this.db.prepare(`
      SELECT rr.run_id
      FROM research_runs rr
      JOIN knowledge_runs kr ON kr.id = rr.run_id
      WHERE kr.studio_id = ? AND rr.state IN (${placeholders}) AND kr.status = 'running'
      ORDER BY rr.created_at DESC, rr.run_id DESC
    `).all(studioId, ...states).map((row: any) => row.run_id as string);
    return runIds.map(runId => this.getResearchRun({ studioId, runId }));
  }

  createManifest(input: {
    runId: string;
    sourceCount: number;
    parseArtifactCount: number;
    blockCount: number;
    primaryCharCount: number;
    units: ResearchUnitDraft[];
    batches: ResearchBatchDraft[];
  }): KnowledgeAnalysisManifest {
    const runId = requiredText(input.runId, "runId", 128);
    if (!Array.isArray(input.units) || input.units.length === 0 || !Array.isArray(input.batches) || input.batches.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Research manifest must contain units and batches");
    }
    const unitIds = new Set(input.units.map(unit => unit.id));
    const batchedIds = input.batches.flatMap(batch => batch.unitIds);
    if (unitIds.size !== input.units.length || new Set(batchedIds).size !== unitIds.size || batchedIds.some(id => !unitIds.has(id))) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Research batches do not cover every unit exactly once");
    }
    const createdAt = this.now();
    this.transaction(() => {
      this.db.prepare(`
        INSERT INTO analysis_manifests (
          run_id, source_count, parse_artifact_count, block_count,
          unit_count, primary_char_count, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
      `).run(
        runId,
        integer(input.sourceCount, "sourceCount"),
        integer(input.parseArtifactCount, "parseArtifactCount"),
        integer(input.blockCount, "blockCount"),
        input.units.length,
        integer(input.primaryCharCount, "primaryCharCount", 1),
        createdAt,
      );
      const insertUnit = this.db.prepare(`
        INSERT INTO analysis_units (
          id, run_id, parse_artifact_id, ordinal, priority, status,
          primary_char_count, context_char_count, completed_at, error_code
        ) VALUES (?, ?, ?, ?, ?, 'pending', ?, ?, NULL, NULL)
      `);
      const insertSpan = this.db.prepare(`
        INSERT INTO analysis_unit_spans (
          unit_id, kind, ordinal, block_id, block_ordinal, start_offset, end_offset
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
      `);
      for (const unit of input.units) {
        insertUnit.run(
          requiredText(unit.id, "unitId", 128),
          runId,
          requiredText(unit.parseArtifactId, "parseArtifactId", 128),
          integer(unit.ordinal, "unit ordinal"),
          integer(unit.priority, "unit priority"),
          integer(unit.primaryCharCount, "primaryCharCount", 1),
          integer(unit.contextCharCount, "contextCharCount"),
        );
        for (const span of unit.spans) {
          insertSpan.run(
            unit.id,
            span.kind,
            integer(span.ordinal, "span ordinal"),
            requiredText(span.blockId, "blockId", 128),
            integer(span.blockOrdinal, "block ordinal"),
            integer(span.startOffset, "startOffset"),
            integer(span.endOffset, "endOffset", 1),
          );
        }
      }
      const insertBatch = this.db.prepare(`
        INSERT INTO execution_batches (
          id, run_id, ordinal, status, estimated_chars, created_at,
          started_at, completed_at, error_code
        ) VALUES (?, ?, ?, 'pending', ?, ?, NULL, NULL, NULL)
      `);
      const insertBatchUnit = this.db.prepare(`
        INSERT INTO execution_batch_units (batch_id, unit_id, ordinal)
        VALUES (?, ?, ?)
      `);
      for (const batch of input.batches) {
        insertBatch.run(batch.id, runId, batch.ordinal, batch.estimatedChars, createdAt);
        batch.unitIds.forEach((unitId, ordinal) => insertBatchUnit.run(batch.id, unitId, ordinal));
      }
      this.db.prepare(`
        UPDATE research_runs SET state = 'scanning', updated_at = ? WHERE run_id = ?
      `).run(createdAt, runId);
    });
    return this.getManifest(runId)!;
  }

  getManifest(runId: string): KnowledgeAnalysisManifest | null {
    const row = this.db.prepare("SELECT * FROM analysis_manifests WHERE run_id = ?").get(runId);
    if (!row) return null;
    return {
      runId: row.run_id,
      sourceCount: Number(row.source_count),
      parseArtifactCount: Number(row.parse_artifact_count),
      blockCount: Number(row.block_count),
      unitCount: Number(row.unit_count),
      primaryCharCount: Number(row.primary_char_count),
      createdAt: row.created_at,
    };
  }

  listUnits(runId: string): KnowledgeAnalysisUnit[] {
    const rows = this.db.prepare(`
      SELECT * FROM analysis_units WHERE run_id = ?
      ORDER BY priority ASC, parse_artifact_id ASC, ordinal ASC
    `).all(runId);
    const spans = this.db.prepare(`
      SELECT aus.* FROM analysis_unit_spans aus
      JOIN analysis_units au ON au.id = aus.unit_id
      WHERE au.run_id = ?
      ORDER BY aus.unit_id ASC, aus.kind DESC, aus.ordinal ASC
    `).all(runId);
    const spansByUnit = new Map<string, KnowledgeAnalysisUnitSpan[]>();
    for (const row of spans) {
      const values = spansByUnit.get(row.unit_id) || [];
      values.push({
        kind: row.kind,
        ordinal: Number(row.ordinal),
        blockId: row.block_id,
        blockOrdinal: Number(row.block_ordinal),
        startOffset: Number(row.start_offset),
        endOffset: Number(row.end_offset),
      });
      spansByUnit.set(row.unit_id, values);
    }
    return rows.map((row: any) => ({
      id: row.id,
      runId: row.run_id,
      parseArtifactId: row.parse_artifact_id,
      ordinal: Number(row.ordinal),
      priority: Number(row.priority),
      status: row.status,
      primaryCharCount: Number(row.primary_char_count),
      contextCharCount: Number(row.context_char_count),
      completedAt: row.completed_at || null,
      errorCode: row.error_code || null,
      spans: spansByUnit.get(row.id) || [],
    }));
  }

  getUnit(runId: string, unitId: string): KnowledgeAnalysisUnit {
    const unit = this.listUnits(runId).find(entry => entry.id === unitId);
    if (!unit) throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Research unit is missing");
    return unit;
  }

  listBatches(runId: string): ResearchExecutionBatch[] {
    return this.db.prepare(`
      SELECT * FROM execution_batches WHERE run_id = ? ORDER BY ordinal ASC
    `).all(runId).map((row: any) => ({
      id: row.id,
      runId: row.run_id,
      ordinal: Number(row.ordinal),
      status: row.status,
      estimatedChars: Number(row.estimated_chars),
      unitIds: this.db.prepare(`
        SELECT unit_id FROM execution_batch_units WHERE batch_id = ? ORDER BY ordinal ASC
      `).all(row.id).map((entry: any) => entry.unit_id),
      startedAt: row.started_at || null,
      completedAt: row.completed_at || null,
      errorCode: row.error_code || null,
    }));
  }

  beginBatch(batchId: string) {
    const now = this.now();
    this.transaction(() => {
      const changed = this.db.prepare(`
        UPDATE execution_batches
        SET status = 'running', started_at = ?, completed_at = NULL, error_code = NULL
        WHERE id = ? AND status IN ('pending', 'failed')
      `).run(now, batchId).changes;
      if (changed !== 1) throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Research batch is not runnable");
      this.db.prepare(`
        UPDATE analysis_units SET status = 'running', completed_at = NULL, error_code = NULL
        WHERE id IN (SELECT unit_id FROM execution_batch_units WHERE batch_id = ?)
          AND status IN ('pending', 'failed')
      `).run(batchId);
    });
  }

  beginAttempt(input: {
    runId: string;
    workType: ResearchTaskAttempt["workType"];
    workId: string;
  }): ResearchTaskAttempt {
    const attemptNumber = Number(this.db.prepare(`
      SELECT COALESCE(MAX(attempt_number), 0) + 1 AS next
      FROM task_attempts WHERE work_type = ? AND work_id = ?
    `).get(input.workType, input.workId).next);
    const id = this.newId("attempt");
    this.db.prepare(`
      INSERT INTO task_attempts (
        id, run_id, work_type, work_id, attempt_number, status,
        error_code, output_json, started_at, completed_at
      ) VALUES (?, ?, ?, ?, ?, 'running', NULL, NULL, ?, NULL)
    `).run(id, input.runId, input.workType, input.workId, attemptNumber, this.now());
    return this.getAttempt(id);
  }

  private getAttempt(id: string): ResearchTaskAttempt {
    const row = this.db.prepare("SELECT * FROM task_attempts WHERE id = ?").get(id);
    if (!row) throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Research attempt is missing");
    return {
      id: row.id,
      runId: row.run_id,
      workType: row.work_type,
      workId: row.work_id,
      attemptNumber: Number(row.attempt_number),
      status: row.status,
      errorCode: row.error_code || null,
      startedAt: row.started_at,
      completedAt: row.completed_at || null,
    };
  }

  failAttempt(input: {
    attemptId: string;
    errorCode: string;
    retry: boolean;
  }) {
    const attempt = this.getAttempt(input.attemptId);
    const now = this.now();
    this.transaction(() => {
      this.db.prepare(`
        UPDATE task_attempts SET status = 'failed', error_code = ?, completed_at = ?
        WHERE id = ? AND status = 'running'
      `).run(input.errorCode, now, input.attemptId);
      if (attempt.workType === "scan_batch") {
        if (!input.retry) {
          this.db.prepare(`
            UPDATE execution_batches SET status = 'failed', error_code = ?, completed_at = ? WHERE id = ?
          `).run(input.errorCode, now, attempt.workId);
          this.db.prepare(`
            UPDATE analysis_units SET status = 'failed', error_code = ?, completed_at = ?
            WHERE id IN (SELECT unit_id FROM execution_batch_units WHERE batch_id = ?)
              AND status = 'running'
          `).run(input.errorCode, now, attempt.workId);
        }
      } else if (attempt.workType === "claim_job" || attempt.workType === "synthesis_job") {
        if (!input.retry) {
          this.db.prepare(`
            UPDATE research_jobs SET status = 'failed', error_code = ?, completed_at = ? WHERE id = ?
          `).run(input.errorCode, now, attempt.workId);
        }
      } else {
        if (!input.retry) {
          this.db.prepare(`
            UPDATE contradiction_checks SET status = 'failed', error_code = ?, completed_at = ? WHERE id = ?
          `).run(input.errorCode, now, attempt.workId);
        }
      }
    });
  }

  completeScanAttempt(input: {
    attemptId: string;
    batchId: string;
    results: Array<{ unitId: string; value: unknown }>;
    rawOutput: unknown;
  }) {
    const expected = this.listBatches(this.getAttempt(input.attemptId).runId)
      .find(batch => batch.id === input.batchId)?.unitIds || [];
    if (
      expected.length !== input.results.length
      || new Set(input.results.map(result => result.unitId)).size !== expected.length
      || input.results.some(result => !expected.includes(result.unitId))
    ) {
      throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Research output does not cover its batch exactly once");
    }
    const now = this.now();
    this.transaction(() => {
      const insertResult = this.db.prepare(`
        INSERT INTO analysis_unit_results (unit_id, attempt_id, result_json, completed_at)
        VALUES (?, ?, ?, ?)
      `);
      for (const result of input.results) {
        insertResult.run(result.unitId, input.attemptId, safeJson(result.value, "analysis result"), now);
        this.db.prepare(`
          UPDATE analysis_units SET status = 'completed', completed_at = ?, error_code = NULL
          WHERE id = ? AND status = 'running'
        `).run(now, result.unitId);
      }
      this.db.prepare(`
        UPDATE execution_batches SET status = 'completed', completed_at = ?, error_code = NULL
        WHERE id = ? AND status = 'running'
      `).run(now, input.batchId);
      this.db.prepare(`
        UPDATE task_attempts SET status = 'completed', output_json = ?, completed_at = ?, error_code = NULL
        WHERE id = ? AND status = 'running'
      `).run(safeJson(input.rawOutput, "analysis output"), now, input.attemptId);
    });
  }

  listUnitResults(runId: string): Array<{ unitId: string; result: any }> {
    return this.db.prepare(`
      SELECT aur.unit_id, aur.result_json
      FROM analysis_unit_results aur
      JOIN analysis_units au ON au.id = aur.unit_id
      WHERE au.run_id = ?
      ORDER BY au.parse_artifact_id ASC, au.ordinal ASC
    `).all(runId).map((row: any) => ({
      unitId: row.unit_id,
      result: parseJson(row.result_json, "analysis result"),
    }));
  }

  listAttempts(runId: string): ResearchTaskAttempt[] {
    return this.db.prepare(`
      SELECT id FROM task_attempts WHERE run_id = ? ORDER BY started_at ASC, attempt_number ASC
    `).all(runId).map((row: any) => this.getAttempt(row.id));
  }

  countAttempts(workType: ResearchTaskAttempt["workType"], workId: string): number {
    return Number(this.db.prepare(`
      SELECT COUNT(*) AS count FROM task_attempts WHERE work_type = ? AND work_id = ?
    `).get(workType, workId).count);
  }

  exhaustWork(input: {
    workType: ResearchTaskAttempt["workType"];
    workId: string;
    errorCode: string;
  }) {
    const workId = requiredText(input.workId, "workId", 128);
    const errorCode = requiredText(input.errorCode, "errorCode", 128);
    const now = this.now();
    this.transaction(() => {
      if (input.workType === "scan_batch") {
        this.db.prepare(`
          UPDATE execution_batches SET status = 'failed', error_code = ?, completed_at = ?
          WHERE id = ? AND status <> 'completed'
        `).run(errorCode, now, workId);
        this.db.prepare(`
          UPDATE analysis_units SET status = 'failed', error_code = ?, completed_at = ?
          WHERE id IN (SELECT unit_id FROM execution_batch_units WHERE batch_id = ?)
            AND status <> 'completed'
        `).run(errorCode, now, workId);
        return;
      }
      if (input.workType === "claim_job" || input.workType === "synthesis_job") {
        this.db.prepare(`
          UPDATE research_jobs SET status = 'failed', error_code = ?, completed_at = ?
          WHERE id = ? AND status <> 'completed'
        `).run(errorCode, now, workId);
        return;
      }
      this.db.prepare(`
        UPDATE contradiction_checks SET status = 'failed', error_code = ?, completed_at = ?
        WHERE id = ? AND status <> 'completed'
      `).run(errorCode, now, workId);
    });
  }

  createJobs(input: {
    runId: string;
    phase: ResearchJob["phase"];
    refs: unknown[];
  }): ResearchJob[] {
    const now = this.now();
    const startOrdinal = Number(this.db.prepare(`
      SELECT COALESCE(MAX(ordinal), -1) + 1 AS next
      FROM research_jobs WHERE run_id = ? AND phase = ?
    `).get(input.runId, input.phase).next);
    this.transaction(() => {
      const insert = this.db.prepare(`
        INSERT INTO research_jobs (
          id, run_id, phase, ordinal, status, input_refs_json, output_json,
          created_at, started_at, completed_at, error_code
        ) VALUES (?, ?, ?, ?, 'pending', ?, NULL, ?, NULL, NULL, NULL)
      `);
      input.refs.forEach((refs, ordinal) => {
        insert.run(
          this.newId("rjob"),
          input.runId,
          input.phase,
          startOrdinal + ordinal,
          safeJson(refs, "research job refs"),
          now,
        );
      });
    });
    return this.listJobs(input.runId, input.phase);
  }

  listJobs(runId: string, phase: ResearchJob["phase"]): ResearchJob[] {
    return this.db.prepare(`
      SELECT * FROM research_jobs WHERE run_id = ? AND phase = ? ORDER BY ordinal ASC
    `).all(runId, phase).map((row: any) => ({
      id: row.id,
      runId: row.run_id,
      phase: row.phase,
      ordinal: Number(row.ordinal),
      status: row.status,
      inputRefs: parseJson(row.input_refs_json, "research job refs"),
      output: row.output_json ? parseJson(row.output_json, "research job output") : null,
      errorCode: row.error_code || null,
    }));
  }

  beginJob(jobId: string) {
    const changed = this.db.prepare(`
      UPDATE research_jobs SET status = 'running', started_at = ?, completed_at = NULL, error_code = NULL
      WHERE id = ? AND status IN ('pending', 'failed')
    `).run(this.now(), jobId).changes;
    if (changed !== 1) throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Research job is not runnable");
  }

  completeJob(input: { attemptId: string; jobId: string; output: unknown }) {
    const now = this.now();
    const serialized = safeJson(input.output, "research job output");
    this.transaction(() => {
      this.db.prepare(`
        UPDATE research_jobs SET status = 'completed', output_json = ?, completed_at = ?, error_code = NULL
        WHERE id = ? AND status = 'running'
      `).run(serialized, now, input.jobId);
      this.db.prepare(`
        UPDATE task_attempts SET status = 'completed', output_json = ?, completed_at = ?, error_code = NULL
        WHERE id = ? AND status = 'running'
      `).run(serialized, now, input.attemptId);
    });
  }

  createVerificationStep(input: {
    runId: string;
    triggerSynthesisJobId: string;
    requests: Array<{ claimId: string; reason: string }>;
  }): ResearchVerificationStep {
    const existing = this.listVerificationSteps(input.runId)
      .find(step => step.triggerSynthesisJobId === input.triggerSynthesisJobId);
    if (existing) return existing;
    if (!Array.isArray(input.requests) || input.requests.length === 0 || input.requests.length > 20) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Verification requests are invalid");
    }
    const requests = input.requests.map(request => ({
      claimId: requiredText(request.claimId, "verification claimId", 128),
      reason: requiredText(request.reason, "verification reason", 2_000),
    }));
    if (new Set(requests.map(request => request.claimId)).size !== requests.length) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Verification requests contain duplicate claims");
    }
    const trigger = this.db.prepare(`
      SELECT id FROM research_jobs
      WHERE id = ? AND run_id = ? AND phase = 'final_synthesis' AND status = 'completed'
    `).get(input.triggerSynthesisJobId, input.runId);
    if (!trigger) throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Verification trigger is invalid");
    for (const request of requests) {
      if (!this.db.prepare("SELECT 1 FROM research_claims WHERE id = ? AND run_id = ?").get(request.claimId, input.runId)) {
        throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Verification claim is outside the research run");
      }
    }
    const units = this.listUnits(input.runId);
    if (units.length === 0) throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Verification has no AnalysisUnits");
    const ordinal = Number(this.db.prepare(`
      SELECT COALESCE(MAX(ordinal), -1) + 1 AS next
      FROM research_verification_steps WHERE run_id = ?
    `).get(input.runId).next);
    const stepId = this.newId("vstep");
    const now = this.now();
    this.transaction(() => {
      this.db.prepare(`
        INSERT INTO research_verification_steps (
          id, run_id, trigger_synthesis_job_id, ordinal, status, requests_json,
          created_at, started_at, completed_at, error_code
        ) VALUES (?, ?, ?, ?, 'pending', ?, ?, NULL, NULL, NULL)
      `).run(
        stepId,
        input.runId,
        input.triggerSynthesisJobId,
        ordinal,
        safeJson(requests, "verification requests"),
        now,
      );
      const insertCell = this.db.prepare(`
        INSERT INTO research_verification_cells (
          id, step_id, unit_id, ordinal, status, result_json,
          created_at, started_at, completed_at, error_code
        ) VALUES (?, ?, ?, ?, 'pending', NULL, ?, NULL, NULL, NULL)
      `);
      units.forEach((unit, unitOrdinal) => {
        insertCell.run(this.newId("vcell"), stepId, unit.id, unitOrdinal, now);
      });
    });
    return this.listVerificationSteps(input.runId).find(step => step.id === stepId)!;
  }

  listVerificationSteps(runId: string): ResearchVerificationStep[] {
    return this.db.prepare(`
      SELECT * FROM research_verification_steps WHERE run_id = ? ORDER BY ordinal ASC
    `).all(runId).map((row: any) => {
      const requests = parseJson<Array<{ claimId: string; reason: string }>>(row.requests_json, "verification requests");
      if (!Array.isArray(requests)) {
        throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Verification requests are corrupt");
      }
      return {
        id: row.id,
        runId: row.run_id,
        triggerSynthesisJobId: row.trigger_synthesis_job_id,
        ordinal: Number(row.ordinal),
        status: row.status,
        requests,
        errorCode: row.error_code || null,
      };
    });
  }

  listVerificationCells(stepId: string): ResearchVerificationCell[] {
    return this.db.prepare(`
      SELECT * FROM research_verification_cells WHERE step_id = ? ORDER BY ordinal ASC
    `).all(stepId).map((row: any) => ({
      id: row.id,
      stepId: row.step_id,
      unitId: row.unit_id,
      ordinal: Number(row.ordinal),
      status: row.status,
      result: row.result_json ? parseJson(row.result_json, "verification result") : null,
      errorCode: row.error_code || null,
    }));
  }

  beginVerificationStep(stepId: string) {
    const changed = this.db.prepare(`
      UPDATE research_verification_steps
      SET status = 'running', started_at = COALESCE(started_at, ?), completed_at = NULL, error_code = NULL
      WHERE id = ? AND status IN ('pending', 'failed')
    `).run(this.now(), stepId).changes;
    if (changed !== 1) throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Verification Step is not runnable");
  }

  beginVerificationCell(cellId: string) {
    const changed = this.db.prepare(`
      UPDATE research_verification_cells
      SET status = 'running', started_at = COALESCE(started_at, ?), completed_at = NULL, error_code = NULL
      WHERE id = ? AND status IN ('pending', 'failed')
    `).run(this.now(), cellId).changes;
    if (changed !== 1) throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Verification cell is not runnable");
  }

  countVerificationAttempts(cellId: string): number {
    return Number(this.db.prepare(`
      SELECT COUNT(*) AS count FROM research_verification_attempts WHERE cell_id = ?
    `).get(cellId).count);
  }

  beginVerificationAttempt(input: {
    runId: string;
    stepId: string;
    cellId: string;
  }): ResearchVerificationAttempt {
    const attemptNumber = this.countVerificationAttempts(input.cellId) + 1;
    const id = this.newId("vattempt");
    this.db.prepare(`
      INSERT INTO research_verification_attempts (
        id, run_id, step_id, cell_id, attempt_number, status,
        error_code, output_json, started_at, completed_at
      ) VALUES (?, ?, ?, ?, ?, 'running', NULL, NULL, ?, NULL)
    `).run(id, input.runId, input.stepId, input.cellId, attemptNumber, this.now());
    return this.getVerificationAttempt(id);
  }

  private getVerificationAttempt(id: string): ResearchVerificationAttempt {
    const row = this.db.prepare("SELECT * FROM research_verification_attempts WHERE id = ?").get(id);
    if (!row) throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Verification attempt is missing");
    return {
      id: row.id,
      runId: row.run_id,
      stepId: row.step_id,
      cellId: row.cell_id,
      attemptNumber: Number(row.attempt_number),
      status: row.status,
      errorCode: row.error_code || null,
    };
  }

  failVerificationAttempt(input: {
    attemptId: string;
    errorCode: string;
    retry: boolean;
  }) {
    const attempt = this.getVerificationAttempt(input.attemptId);
    const now = this.now();
    this.transaction(() => {
      this.db.prepare(`
        UPDATE research_verification_attempts
        SET status = 'failed', error_code = ?, completed_at = ?
        WHERE id = ? AND status = 'running'
      `).run(input.errorCode, now, attempt.id);
      if (!input.retry) {
        this.db.prepare(`
          UPDATE research_verification_cells
          SET status = 'failed', error_code = ?, completed_at = ? WHERE id = ?
        `).run(input.errorCode, now, attempt.cellId);
        this.db.prepare(`
          UPDATE research_verification_steps
          SET status = 'failed', error_code = ?, completed_at = ? WHERE id = ?
        `).run(input.errorCode, now, attempt.stepId);
      }
    });
  }

  exhaustVerificationCell(input: { stepId: string; cellId: string; errorCode: string }) {
    const now = this.now();
    this.transaction(() => {
      this.db.prepare(`
        UPDATE research_verification_cells
        SET status = 'failed', error_code = ?, completed_at = ?
        WHERE id = ? AND status <> 'completed'
      `).run(input.errorCode, now, input.cellId);
      this.db.prepare(`
        UPDATE research_verification_steps
        SET status = 'failed', error_code = ?, completed_at = ?
        WHERE id = ? AND status <> 'completed'
      `).run(input.errorCode, now, input.stepId);
    });
  }

  completeVerificationCell(input: {
    attemptId: string;
    cellId: string;
    output: unknown;
    relations: Array<{
      claimId: string;
      evidenceId: string;
      relation: KnowledgeClaimEvidenceRelation;
      explanation: string;
    }>;
  }) {
    const attempt = this.getVerificationAttempt(input.attemptId);
    if (attempt.cellId !== input.cellId) {
      throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Verification attempt does not own this cell");
    }
    const serialized = safeJson(input.output, "verification output");
    const now = this.now();
    this.transaction(() => {
      const insertRelation = this.db.prepare(`
        INSERT INTO research_verification_relations (
          id, run_id, step_id, cell_id, claim_id, evidence_id,
          relation, explanation, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
      `);
      for (const relation of input.relations) {
        if (!this.db.prepare("SELECT 1 FROM research_claims WHERE id = ? AND run_id = ?").get(relation.claimId, attempt.runId)) {
          throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Verification relation references an unknown claim");
        }
        if (!this.db.prepare("SELECT 1 FROM research_evidence WHERE id = ? AND run_id = ?").get(relation.evidenceId, attempt.runId)) {
          throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Verification relation references unknown evidence");
        }
        insertRelation.run(
          this.newId("vrel"),
          attempt.runId,
          attempt.stepId,
          attempt.cellId,
          relation.claimId,
          relation.evidenceId,
          relation.relation,
          requiredText(relation.explanation, "verification explanation", 20_000),
          now,
        );
        this.db.prepare(`
          INSERT OR IGNORE INTO claim_evidence (claim_id, evidence_id, relation)
          VALUES (?, ?, ?)
        `).run(relation.claimId, relation.evidenceId, relation.relation);
      }
      this.db.prepare(`
        UPDATE research_verification_cells
        SET status = 'completed', result_json = ?, completed_at = ?, error_code = NULL
        WHERE id = ? AND status = 'running'
      `).run(serialized, now, input.cellId);
      this.db.prepare(`
        UPDATE research_verification_attempts
        SET status = 'completed', output_json = ?, completed_at = ?, error_code = NULL
        WHERE id = ? AND status = 'running'
      `).run(serialized, now, input.attemptId);
    });
  }

  completeVerificationStep(stepId: string) {
    const coverage = this.db.prepare(`
      SELECT COUNT(*) AS total,
        SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) AS completed
      FROM research_verification_cells WHERE step_id = ?
    `).get(stepId);
    if (Number(coverage.total || 0) === 0 || Number(coverage.completed || 0) !== Number(coverage.total)) {
      throw new KnowledgeError("KNOWLEDGE_RESEARCH_INCOMPLETE", "Verification Step did not cover every AnalysisUnit");
    }
    this.db.prepare(`
      UPDATE research_verification_steps
      SET status = 'completed', completed_at = ?, error_code = NULL
      WHERE id = ? AND status = 'running'
    `).run(this.now(), stepId);
  }

  applyVerificationSupportStatus(runId: string) {
    this.db.prepare(`
      UPDATE research_claims
      SET support_status = 'supported'
      WHERE run_id = ? AND support_status = 'insufficient'
        AND id IN (SELECT DISTINCT claim_id FROM claim_evidence WHERE relation = 'supports')
        AND id NOT IN (SELECT DISTINCT claim_id FROM claim_evidence WHERE relation = 'contradicts')
    `).run(runId);
  }

  recordEvidenceValidation(input: {
    runId: string;
    unitId: string;
    originType: "analysis" | "contradiction";
    originId: string;
    candidateOrdinal: number;
    status: "validated" | "invalid";
    reasonCode?: string | null;
    citationId?: string | null;
    canonicalQuote?: string | null;
    quoteChecksum?: string | null;
    epistemicBasis: KnowledgeEpistemicBasis;
    evidence?: {
      contentSnapshotId: string;
      parseArtifactId: string;
      blockId: string;
      startOffset: number;
      endOffset: number;
    };
  }): ResearchEvidence | null {
    const validationId = this.newId("evalid");
    const evidenceId = input.status === "validated" ? this.newId("evidence") : null;
    const now = this.now();
    this.transaction(() => {
      this.db.prepare(`
        INSERT INTO evidence_validations (
          id, run_id, unit_id, origin_type, origin_id, candidate_ordinal,
          status, reason_code, citation_id, canonical_quote, quote_checksum,
          epistemic_basis, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
      `).run(
        validationId,
        input.runId,
        input.unitId,
        input.originType,
        input.originId,
        input.candidateOrdinal,
        input.status,
        input.reasonCode || null,
        input.citationId || null,
        input.canonicalQuote || null,
        input.quoteChecksum || null,
        input.epistemicBasis,
        now,
      );
      if (evidenceId && input.evidence && input.citationId && input.canonicalQuote && input.quoteChecksum) {
        this.db.prepare(`
          INSERT INTO research_evidence (
            id, run_id, unit_id, validation_id, citation_id, content_snapshot_id,
            parse_artifact_id, block_id, start_offset, end_offset, canonical_quote,
            quote_checksum, epistemic_basis, created_at
          ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        `).run(
          evidenceId,
          input.runId,
          input.unitId,
          validationId,
          input.citationId,
          input.evidence.contentSnapshotId,
          input.evidence.parseArtifactId,
          input.evidence.blockId,
          input.evidence.startOffset,
          input.evidence.endOffset,
          input.canonicalQuote,
          input.quoteChecksum,
          input.epistemicBasis,
          now,
        );
      }
    });
    return evidenceId ? this.getEvidence(evidenceId) : null;
  }

  hasEvidenceValidations(originType: "analysis" | "contradiction", originId: string): boolean {
    return Boolean(this.db.prepare(`
      SELECT 1 FROM evidence_validations WHERE origin_type = ? AND origin_id = ? LIMIT 1
    `).get(originType, originId));
  }

  getEvidence(id: string): ResearchEvidence {
    const row = this.db.prepare("SELECT * FROM research_evidence WHERE id = ?").get(id);
    if (!row) throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Research evidence is missing");
    return {
      id: row.id,
      runId: row.run_id,
      unitId: row.unit_id,
      validationId: row.validation_id,
      citationId: row.citation_id,
      contentSnapshotId: row.content_snapshot_id,
      parseArtifactId: row.parse_artifact_id,
      blockId: row.block_id,
      startOffset: Number(row.start_offset),
      endOffset: Number(row.end_offset),
      canonicalQuote: row.canonical_quote,
      quoteChecksum: row.quote_checksum,
      epistemicBasis: row.epistemic_basis,
      createdAt: row.created_at,
    };
  }

  listEvidence(runId: string): ResearchEvidence[] {
    return this.db.prepare(`
      SELECT id FROM research_evidence WHERE run_id = ? ORDER BY created_at ASC, id ASC
    `).all(runId).map((row: any) => this.getEvidence(row.id));
  }

  listEvidenceForUnit(unitId: string): ResearchEvidence[] {
    return this.db.prepare(`
      SELECT id FROM research_evidence WHERE unit_id = ? ORDER BY created_at ASC, id ASC
    `).all(unitId).map((row: any) => this.getEvidence(row.id));
  }

  getAnalysisEvidenceByCandidate(unitId: string): Map<number, ResearchEvidence> {
    const rows = this.db.prepare(`
      SELECT ev.candidate_ordinal, re.id
      FROM evidence_validations ev
      JOIN research_evidence re ON re.validation_id = ev.id
      WHERE ev.origin_type = 'analysis' AND ev.origin_id = ? AND ev.status = 'validated'
      ORDER BY ev.candidate_ordinal ASC
    `).all(unitId);
    return new Map(rows.map((row: any) => [Number(row.candidate_ordinal), this.getEvidence(row.id)]));
  }

  completeClaimJob(input: {
    attemptId: string;
    jobId: string;
    runId: string;
    claims: Array<{
      text: string;
      supportStatus: KnowledgeSupportStatus;
      epistemicBasis: KnowledgeEpistemicBasis;
      evidence: Array<{ evidenceId: string; relation: KnowledgeClaimEvidenceRelation }>;
    }>;
    output: unknown;
  }): ResearchClaim[] {
    const allowedEvidence = new Set(this.listEvidence(input.runId).map(entry => entry.id));
    const now = this.now();
    const created: string[] = [];
    this.transaction(() => {
      const insertClaim = this.db.prepare(`
        INSERT INTO research_claims (
          id, run_id, origin_job_id, ordinal, text, support_status, epistemic_basis, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
      `);
      const insertRelation = this.db.prepare(`
        INSERT INTO claim_evidence (claim_id, evidence_id, relation) VALUES (?, ?, ?)
      `);
      input.claims.forEach((claim, ordinal) => {
        if (claim.evidence.some(entry => !allowedEvidence.has(entry.evidenceId))) {
          throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Claim references unknown evidence");
        }
        const claimId = this.newId("claim");
        created.push(claimId);
        insertClaim.run(
          claimId,
          input.runId,
          input.jobId,
          ordinal,
          requiredText(claim.text, "claim text", 20_000),
          claim.supportStatus,
          claim.epistemicBasis,
          now,
        );
        claim.evidence.forEach(entry => insertRelation.run(claimId, entry.evidenceId, entry.relation));
      });
      const serialized = safeJson(input.output, "claim output");
      this.db.prepare(`
        UPDATE research_jobs SET status = 'completed', output_json = ?, completed_at = ?, error_code = NULL
        WHERE id = ? AND status = 'running'
      `).run(serialized, now, input.jobId);
      this.db.prepare(`
        UPDATE task_attempts SET status = 'completed', output_json = ?, completed_at = ?, error_code = NULL
        WHERE id = ? AND status = 'running'
      `).run(serialized, now, input.attemptId);
    });
    return created.map(id => this.getClaim(id));
  }

  getClaim(id: string): ResearchClaim {
    const row = this.db.prepare("SELECT * FROM research_claims WHERE id = ?").get(id);
    if (!row) throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Research claim is missing");
    return {
      id: row.id,
      runId: row.run_id,
      originJobId: row.origin_job_id,
      ordinal: Number(row.ordinal),
      text: row.text,
      supportStatus: row.support_status,
      epistemicBasis: row.epistemic_basis,
      evidence: this.db.prepare(`
        SELECT evidence_id, relation FROM claim_evidence WHERE claim_id = ? ORDER BY evidence_id ASC
      `).all(id).map((entry: any) => ({ evidenceId: entry.evidence_id, relation: entry.relation })),
    };
  }

  listClaims(runId: string): ResearchClaim[] {
    return this.db.prepare(`
      SELECT id FROM research_claims WHERE run_id = ? ORDER BY origin_job_id ASC, ordinal ASC
    `).all(runId).map((row: any) => this.getClaim(row.id));
  }

  createContradictionManifest(input: {
    runId: string;
    packs: Array<{ id: string; claimIds: string[] }>;
  }) {
    const units = this.listUnits(input.runId);
    const now = this.now();
    this.transaction(() => {
      const insertPack = this.db.prepare(`
        INSERT INTO claim_packs (id, run_id, ordinal, claim_ids_json, created_at)
        VALUES (?, ?, ?, ?, ?)
      `);
      input.packs.forEach((pack, ordinal) => {
        insertPack.run(pack.id, input.runId, ordinal, safeJson(pack.claimIds, "claim pack"), now);
      });
      this.db.prepare(`
        INSERT INTO contradiction_manifests (
          run_id, unit_count, claim_pack_count, total_check_count, created_at
        ) VALUES (?, ?, ?, ?, ?)
      `).run(input.runId, units.length, input.packs.length, units.length * input.packs.length, now);
      const insertCheck = this.db.prepare(`
        INSERT INTO contradiction_checks (
          id, run_id, unit_id, claim_pack_id, status, attempt_id,
          result_json, completed_at, error_code
        ) VALUES (?, ?, ?, ?, 'pending', NULL, NULL, NULL, NULL)
      `);
      for (const unit of units) {
        for (const pack of input.packs) {
          insertCheck.run(this.newId("ccheck"), input.runId, unit.id, pack.id);
        }
      }
    });
  }

  hasContradictionManifest(runId: string): boolean {
    return Boolean(this.db.prepare(
      "SELECT 1 FROM contradiction_manifests WHERE run_id = ?",
    ).get(runId));
  }

  listClaimPacks(runId: string): ResearchClaimPack[] {
    return this.db.prepare(`SELECT * FROM claim_packs WHERE run_id = ? ORDER BY ordinal ASC`)
      .all(runId).map((row: any) => ({
        id: row.id,
        runId: row.run_id,
        ordinal: Number(row.ordinal),
        claimIds: parseJson<string[]>(row.claim_ids_json, "claim pack"),
      }));
  }

  listContradictionChecks(runId: string): ResearchContradictionCheck[] {
    return this.db.prepare(`
      SELECT * FROM contradiction_checks WHERE run_id = ? ORDER BY unit_id ASC, claim_pack_id ASC
    `).all(runId).map((row: any) => ({
      id: row.id,
      runId: row.run_id,
      unitId: row.unit_id,
      claimPackId: row.claim_pack_id,
      status: row.status,
      attemptId: row.attempt_id || null,
      result: row.result_json ? parseJson(row.result_json, "contradiction result") : null,
      errorCode: row.error_code || null,
    }));
  }

  beginContradictionCheck(checkId: string) {
    const changed = this.db.prepare(`
      UPDATE contradiction_checks SET status = 'running', completed_at = NULL, error_code = NULL
      WHERE id = ? AND status IN ('pending', 'failed')
    `).run(checkId).changes;
    if (changed !== 1) throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Contradiction check is not runnable");
  }

  completeContradictionCheck(input: {
    attemptId: string;
    checkId: string;
    output: unknown;
    contradictions: Array<{
      claimId: string;
      evidenceId: string;
      relation: "contradicts" | "context";
      explanation: string;
    }>;
  }) {
    const attempt = this.getAttempt(input.attemptId);
    const now = this.now();
    const serialized = safeJson(input.output, "contradiction output");
    this.transaction(() => {
      const insert = this.db.prepare(`
        INSERT INTO research_contradictions (
          id, run_id, check_id, claim_id, evidence_id, relation, explanation, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
      `);
      for (const contradiction of input.contradictions) {
        insert.run(
          this.newId("contradiction"),
          attempt.runId,
          input.checkId,
          contradiction.claimId,
          contradiction.evidenceId,
          contradiction.relation,
          requiredText(contradiction.explanation, "contradiction explanation", 20_000),
          now,
        );
        this.db.prepare(`
          INSERT OR IGNORE INTO claim_evidence (claim_id, evidence_id, relation)
          VALUES (?, ?, ?)
        `).run(contradiction.claimId, contradiction.evidenceId, contradiction.relation);
      }
      this.db.prepare(`
        UPDATE contradiction_checks
        SET status = 'completed', attempt_id = ?, result_json = ?, completed_at = ?, error_code = NULL
        WHERE id = ? AND status = 'running'
      `).run(input.attemptId, serialized, now, input.checkId);
      this.db.prepare(`
        UPDATE task_attempts SET status = 'completed', output_json = ?, completed_at = ?, error_code = NULL
        WHERE id = ? AND status = 'running'
      `).run(serialized, now, input.attemptId);
    });
  }

  listContradictions(runId: string) {
    const contradictionRows = this.db.prepare(`
      SELECT * FROM research_contradictions WHERE run_id = ? ORDER BY created_at ASC, id ASC
    `).all(runId).map((row: any) => ({
      id: row.id,
      runId: row.run_id,
      checkId: row.check_id,
      claimId: row.claim_id,
      evidenceId: row.evidence_id,
      relation: row.relation,
      explanation: row.explanation,
      createdAt: row.created_at,
    }));
    const verificationRows = this.db.prepare(`
      SELECT * FROM research_verification_relations
      WHERE run_id = ? AND relation IN ('contradicts', 'context')
      ORDER BY created_at ASC, id ASC
    `).all(runId).map((row: any) => ({
      id: row.id,
      runId: row.run_id,
      checkId: row.cell_id,
      claimId: row.claim_id,
      evidenceId: row.evidence_id,
      relation: row.relation,
      explanation: row.explanation,
      createdAt: row.created_at,
    }));
    return [...contradictionRows, ...verificationRows]
      .sort((left, right) => left.createdAt.localeCompare(right.createdAt) || left.id.localeCompare(right.id))
      .map(({ createdAt: _createdAt, ...entry }) => entry);
  }

  applyContradictionSupportStatus(runId: string) {
    this.db.prepare(`
      UPDATE research_claims
      SET support_status = 'disputed'
      WHERE run_id = ? AND id IN (
        SELECT DISTINCT claim_id FROM claim_evidence WHERE relation = 'contradicts'
      )
    `).run(runId);
  }

  completeReport(input: {
    studioId: string;
    runId: string;
    attemptId: string;
    jobId: string;
    report: Omit<KnowledgeResearchReport, "createdAt" | "coverage">;
  }): KnowledgeResearchReport {
    this.assertStudioRun(input.studioId, input.runId);
    const now = this.now();
    const coverage = this.getCoverage(input.runId);
    if (
      coverage.sourceReadiness.total === 0
      || coverage.sourceReadiness.completed !== coverage.sourceReadiness.total
      || coverage.extraction.total === 0
      || coverage.extraction.completed !== coverage.extraction.total
      || coverage.primaryScan.total === 0
      || coverage.primaryScan.completed !== coverage.primaryScan.total
      || coverage.contradiction.completed !== coverage.contradiction.total
      || coverage.citationValidation.completed !== coverage.citationValidation.total
    ) {
      throw new KnowledgeError(
        "KNOWLEDGE_RESEARCH_INCOMPLETE",
        "A complete research report requires complete frozen-scope coverage",
      );
    }
    const report: KnowledgeResearchReport = {
      ...input.report,
      coverage,
      createdAt: now,
    };
    this.transaction(() => {
      const insertCitation = this.db.prepare(`
        INSERT INTO research_report_citations (
          run_id, ordinal, marker, evidence_id, citation_id
        ) VALUES (?, ?, ?, ?, ?)
      `);
      report.citations.forEach((citation, ordinal) => insertCitation.run(
        input.runId,
        ordinal,
        citation.marker,
        citation.evidenceId,
        citation.citationId,
      ));
      this.db.prepare(`
        INSERT INTO research_reports (run_id, synthesis_job_id, report_json, created_at)
        VALUES (?, ?, ?, ?)
      `).run(input.runId, input.jobId, safeJson(report, "research report"), now);
      const serialized = safeJson(input.report, "synthesis output");
      this.db.prepare(`
        UPDATE research_jobs SET status = 'completed', output_json = ?, completed_at = ?, error_code = NULL
        WHERE id = ? AND status = 'running'
      `).run(serialized, now, input.jobId);
      this.db.prepare(`
        UPDATE task_attempts SET status = 'completed', output_json = ?, completed_at = ?, error_code = NULL
        WHERE id = ? AND status = 'running'
      `).run(serialized, now, input.attemptId);
      this.db.prepare(`
        UPDATE research_runs
        SET state = 'completed', error_code = NULL, updated_at = ?, completed_at = ?
        WHERE run_id = ?
      `).run(now, now, input.runId);
      this.db.prepare(`
        UPDATE knowledge_runs
        SET status = 'completed', answer_text = ?, error_code = NULL, completed_at = ?
        WHERE id = ? AND status = 'running'
      `).run(report.summary, now, input.runId);
    });
    return this.getReport({ studioId: input.studioId, runId: input.runId });
  }

  getReport(input: { studioId: unknown; runId: unknown }): KnowledgeResearchReport {
    const studioId = requiredText(input.studioId, "studioId");
    const runId = requiredText(input.runId, "runId", 128);
    this.assertStudioRun(studioId, runId);
    const row = this.db.prepare("SELECT report_json FROM research_reports WHERE run_id = ?").get(runId);
    if (!row) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Research report is not available");
    const report = parseJson<KnowledgeResearchReport>(row.report_json, "research report");
    return { ...report, coverage: this.getCoverage(runId) };
  }

  getCoverage(runId: string): KnowledgeResearchCoverage {
    const manifest = this.getManifest(runId);
    const sourceTotal = manifest?.sourceCount || 0;
    const artifactTotal = manifest?.parseArtifactCount || 0;
    const unitTotal = manifest?.unitCount || 0;
    const unitCompleted = Number(this.db.prepare(`
      SELECT COUNT(*) AS count FROM analysis_units WHERE run_id = ? AND status = 'completed'
    `).get(runId).count);
    const contradictionRow = this.db.prepare(`
      SELECT total_check_count FROM contradiction_manifests WHERE run_id = ?
    `).get(runId);
    const contradictionTotal = Number(contradictionRow?.total_check_count || 0);
    const contradictionCompleted = Number(this.db.prepare(`
      SELECT COUNT(*) AS count FROM contradiction_checks WHERE run_id = ? AND status = 'completed'
    `).get(runId).count);
    const resultRows = this.db.prepare(`
      SELECT aur.result_json FROM analysis_unit_results aur
      JOIN analysis_units au ON au.id = aur.unit_id WHERE au.run_id = ?
    `).all(runId);
    let candidateTotal = 0;
    for (const row of resultRows) {
      const result = parseJson<any>(row.result_json, "analysis result");
      if (Array.isArray(result?.evidenceCandidates)) candidateTotal += result.evidenceCandidates.length;
    }
    const contradictionCandidateRows = this.db.prepare(`
      SELECT result_json FROM contradiction_checks
      WHERE run_id = ? AND status = 'completed' AND result_json IS NOT NULL
    `).all(runId);
    for (const row of contradictionCandidateRows) {
      const result = parseJson<any>(row.result_json, "contradiction result");
      if (Array.isArray(result?.matches)) candidateTotal += result.matches.length;
    }
    const verificationCandidateRows = this.db.prepare(`
      SELECT result_json FROM research_verification_cells
      WHERE status = 'completed' AND result_json IS NOT NULL
        AND step_id IN (SELECT id FROM research_verification_steps WHERE run_id = ?)
    `).all(runId);
    for (const row of verificationCandidateRows) {
      const result = parseJson<any>(row.result_json, "verification result");
      if (Array.isArray(result?.matches)) candidateTotal += result.matches.length;
    }
    const validation = this.db.prepare(`
      SELECT
        COUNT(*) AS completed,
        SUM(CASE WHEN status = 'validated' THEN 1 ELSE 0 END) AS valid,
        SUM(CASE WHEN status = 'invalid' THEN 1 ELSE 0 END) AS invalid
      FROM evidence_validations WHERE run_id = ?
    `).get(runId);
    return {
      sourceReadiness: { completed: sourceTotal, total: sourceTotal },
      extraction: { completed: artifactTotal, total: artifactTotal },
      primaryScan: { completed: unitCompleted, total: unitTotal },
      contradiction: { completed: contradictionCompleted, total: contradictionTotal },
      citationValidation: {
        completed: Number(validation.completed || 0),
        total: candidateTotal,
        valid: Number(validation.valid || 0),
        invalid: Number(validation.invalid || 0),
      },
    };
  }

  recoverRun(input: { studioId: string; runId: string }) {
    this.assertStudioRun(input.studioId, input.runId);
    const now = this.now();
    this.transaction(() => {
      this.db.prepare(`
        UPDATE task_attempts
        SET status = 'failed', error_code = 'PROCESS_RESTARTED', completed_at = ?
        WHERE run_id = ? AND status = 'running'
      `).run(now, input.runId);
      this.db.prepare(`
        UPDATE execution_batches SET status = 'pending', error_code = 'PROCESS_RESTARTED'
        WHERE run_id = ? AND status = 'running'
      `).run(input.runId);
      this.db.prepare(`
        UPDATE analysis_units SET status = 'pending', error_code = 'PROCESS_RESTARTED'
        WHERE run_id = ? AND status = 'running'
      `).run(input.runId);
      this.db.prepare(`
        UPDATE research_jobs SET status = 'pending', error_code = 'PROCESS_RESTARTED'
        WHERE run_id = ? AND status = 'running'
      `).run(input.runId);
      this.db.prepare(`
        UPDATE contradiction_checks SET status = 'pending', error_code = 'PROCESS_RESTARTED'
        WHERE run_id = ? AND status = 'running'
      `).run(input.runId);
      this.db.prepare(`
        UPDATE research_verification_attempts
        SET status = 'failed', error_code = 'PROCESS_RESTARTED', completed_at = ?
        WHERE run_id = ? AND status = 'running'
      `).run(now, input.runId);
      this.db.prepare(`
        UPDATE research_verification_cells
        SET status = 'pending', error_code = 'PROCESS_RESTARTED'
        WHERE step_id IN (SELECT id FROM research_verification_steps WHERE run_id = ?)
          AND status = 'running'
      `).run(input.runId);
      this.db.prepare(`
        UPDATE research_verification_steps
        SET status = 'pending', error_code = 'PROCESS_RESTARTED'
        WHERE run_id = ? AND status = 'running'
      `).run(input.runId);
      this.db.prepare(`
        UPDATE research_runs SET state = 'recovering', error_code = NULL, updated_at = ?, completed_at = NULL
        WHERE run_id = ?
      `).run(now, input.runId);
    });
  }

  cancelRun(input: { studioId: string; runId: string }): KnowledgeResearchRun {
    this.assertStudioRun(input.studioId, input.runId);
    const now = this.now();
    this.transaction(() => {
      this.db.prepare(`
        UPDATE analysis_units SET status = 'canceled', completed_at = ?
        WHERE run_id = ? AND status IN ('pending', 'running')
      `).run(now, input.runId);
      this.db.prepare(`
        UPDATE execution_batches SET status = 'canceled', completed_at = ?
        WHERE run_id = ? AND status IN ('pending', 'running')
      `).run(now, input.runId);
      this.db.prepare(`
        UPDATE research_jobs SET status = 'canceled', completed_at = ?
        WHERE run_id = ? AND status IN ('pending', 'running')
      `).run(now, input.runId);
      this.db.prepare(`
        UPDATE contradiction_checks SET status = 'canceled', completed_at = ?
        WHERE run_id = ? AND status IN ('pending', 'running')
      `).run(now, input.runId);
      this.db.prepare(`
        UPDATE task_attempts SET status = 'canceled', error_code = 'KNOWLEDGE_RESEARCH_CANCELED', completed_at = ?
        WHERE run_id = ? AND status = 'running'
      `).run(now, input.runId);
      this.db.prepare(`
        UPDATE research_verification_cells SET status = 'canceled', completed_at = ?
        WHERE step_id IN (SELECT id FROM research_verification_steps WHERE run_id = ?)
          AND status IN ('pending', 'running')
      `).run(now, input.runId);
      this.db.prepare(`
        UPDATE research_verification_steps SET status = 'canceled', completed_at = ?
        WHERE run_id = ? AND status IN ('pending', 'running')
      `).run(now, input.runId);
      this.db.prepare(`
        UPDATE research_verification_attempts
        SET status = 'canceled', error_code = 'KNOWLEDGE_RESEARCH_CANCELED', completed_at = ?
        WHERE run_id = ? AND status = 'running'
      `).run(now, input.runId);
      this.db.prepare(`
        UPDATE research_runs
        SET state = 'canceled', error_code = 'KNOWLEDGE_RESEARCH_CANCELED', updated_at = ?, completed_at = ?
        WHERE run_id = ?
      `).run(now, now, input.runId);
      this.db.prepare(`
        UPDATE knowledge_runs
        SET status = 'cancelled', error_code = 'KNOWLEDGE_RESEARCH_CANCELED', completed_at = ?
        WHERE id = ? AND status = 'running'
      `).run(now, input.runId);
    });
    return this.getResearchRun(input);
  }

  failRun(input: { studioId: string; runId: string; errorCode: string; partial: boolean }) {
    this.assertStudioRun(input.studioId, input.runId);
    const state = input.partial ? "partial" : "failed";
    const now = this.now();
    this.transaction(() => {
      this.db.prepare(`
        UPDATE research_runs SET state = ?, error_code = ?, updated_at = ?, completed_at = ?
        WHERE run_id = ? AND state NOT IN ('completed', 'partial', 'failed', 'canceled')
      `).run(state, input.errorCode, now, now, input.runId);
      this.db.prepare(`
        UPDATE knowledge_runs SET status = 'failed', error_code = ?, completed_at = ?
        WHERE id = ? AND status = 'running'
      `).run(input.errorCode, now, input.runId);
    });
  }
}

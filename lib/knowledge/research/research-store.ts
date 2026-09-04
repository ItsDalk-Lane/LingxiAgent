import crypto from "node:crypto";
import { DEFAULT_KNOWLEDGE_RESEARCH_BUDGET, type KnowledgeResearchBudget } from "../../../shared/knowledge-research.ts";
import type { KnowledgeCompletenessPolicy } from "../../../shared/knowledge-execution.ts";
import { KnowledgeError } from "../errors.ts";
import type { KnowledgeStore } from "../knowledge-store.ts";
import type {
  KnowledgeEvidenceItem, KnowledgeEvidenceNeedRecord, KnowledgeNeedEvidence, KnowledgeResearchAction,
  KnowledgeResearchReadReceipt, KnowledgeResearchRound, KnowledgeResearchRun,
} from "../types.ts";

type NeedInput = Pick<KnowledgeEvidenceNeedRecord, "claim" | "kind" | "required" | "minIndependentSources"
  | "requireCounterEvidence" | "requireAllRelevantUnits">;
const NEED_KEYS = ["claim", "kind", "required", "minIndependentSources", "requireCounterEvidence", "requireAllRelevantUnits"];
const LOCATION_KEYS = ["sourceId", "contentSnapshotId", "parseArtifactId", "chunkIndexVariantId", "chunkId", "blockId",
  "startOffset", "endOffset", "canonicalTextSha256"];
const JSON_FIELDS: Record<string, string> = {
  budget_json: "budget", unresolved_gaps_json: "unresolvedGaps", focus_json: "focus", heading_path_json: "headingPath",
  request_summary_json: "requestSummary", response_summary_json: "responseSummary",
};

function invalid(): never { throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Research metadata is invalid"); }
function scopeViolation(): never { throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Research identity is outside the frozen scope"); }
function text(value: unknown, max = 128): asserts value is string {
  if (typeof value !== "string" || !value.trim() || value.length > max) invalid();
}
function keys(value: unknown, allowed: string[]): asserts value is Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)
    || ![Object.prototype, null].includes(Object.getPrototypeOf(value))
    || Object.keys(value).some(key => !allowed.includes(key))) invalid();
}
function strings(value: unknown, maxLength = 128): asserts value is string[] {
  if (!Array.isArray(value)) invalid();
  for (const entry of value) text(entry, maxLength);
}
function budget(input: unknown): KnowledgeResearchBudget {
  keys(input, Object.keys(DEFAULT_KNOWLEDGE_RESEARCH_BUDGET));
  for (const key of Object.keys(DEFAULT_KNOWLEDGE_RESEARCH_BUDGET)) {
    if (!Number.isSafeInteger(input[key]) || Number(input[key]) <= 0) invalid();
  }
  return { ...input } as unknown as KnowledgeResearchBudget;
}
function record<T>(row: Record<string, unknown>): T {
  return Object.fromEntries(Object.entries(row).map(([key, value]) => {
    if (JSON_FIELDS[key]) return [JSON_FIELDS[key], value === null ? null : JSON.parse(String(value))];
    return [key.replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase()),
      ["required", "require_counter_evidence", "require_all_relevant_units"].includes(key) ? value === 1 : value];
  })) as T;
}

/** 研究持久化只保存冻结身份、证据和有限元数据；模型不能通过额外字段夹带整段提示或工具输出。 */
export class ResearchStore {
  constructor(readonly knowledgeStore: KnowledgeStore, private readonly options: {
    now?: () => string; idGenerator?: (prefix: string) => string;
  } = {}) {}

  now(): string { return this.options.now?.() ?? new Date().toISOString(); }
  newId(prefix: string): string {
    const id = this.options.idGenerator?.(prefix) ?? `${prefix}_${crypto.randomUUID()}`;
    text(id); return id;
  }
  transaction<T>(fn: () => T): T { return this.knowledgeStore.db.transaction(fn)(); }

  private insert(table: string, values: Record<string, unknown>) {
    const columns = Object.keys(values);
    this.knowledgeStore.db.prepare(`INSERT INTO ${table} (${columns.join(",")}) VALUES (${columns.map(() => "?").join(",")})`)
      .run(...Object.values(values));
  }
  private requireActive(runId: string): KnowledgeResearchRun {
    const run = this.requireRun(runId);
    if (!["planning", "running", "synthesizing"].includes(run.status)
      || this.knowledgeStore.getTurnScope({ scopeId: run.turnScopeId })?.status !== "active") scopeViolation();
    return run;
  }
  private location(input: KnowledgeResearchReadReceipt | KnowledgeEvidenceItem) {
    const run = this.requireActive(input.runId);
    const scope = this.knowledgeStore.getTurnScope({ scopeId: run.turnScopeId })!;
    const source = scope.sources.find(source => source.sourceId === input.sourceId);
    if (!source || source.contentSnapshotId !== input.contentSnapshotId || source.parseArtifactId !== input.parseArtifactId) scopeViolation();
    const artifact = this.knowledgeStore.getParseArtifact({ studioId: scope.studioId, parseArtifactId: input.parseArtifactId });
    if (artifact.contentSnapshotId !== input.contentSnapshotId || artifact.status !== "ready") scopeViolation();
    const block = this.knowledgeStore.getArtifactBlocksByIds({ studioId: scope.studioId,
      parseArtifactId: input.parseArtifactId, blockIds: [input.blockId] })[0];
    if (!block || !Number.isSafeInteger(input.startOffset) || !Number.isSafeInteger(input.endOffset)
      || input.startOffset < 0 || input.endOffset <= input.startOffset || input.endOffset > block.text.length) scopeViolation();
    const canonical = block.text.slice(input.startOffset, input.endOffset);
    if (crypto.createHash("sha256").update(canonical).digest("hex") !== input.canonicalTextSha256) invalid();
    if ("canonicalText" in input && (input.canonicalText !== canonical || canonical.length > 2000)) invalid();
  }

  createRun(input: { turnScopeId: string; turnId: string; parentSessionPath: string; question: string;
    budget?: KnowledgeResearchBudget; completenessPolicy?: KnowledgeCompletenessPolicy }): KnowledgeResearchRun {
    keys(input, ["turnScopeId", "turnId", "parentSessionPath", "question", "budget", "completenessPolicy"]);
    text(input.question, 10_000);
    const scope = this.knowledgeStore.getTurnScope({ scopeId: input.turnScopeId });
    if (!scope || scope.status !== "active" || scope.turnId !== input.turnId || scope.sessionPath !== input.parentSessionPath) scopeViolation();
    const limits = budget(input.budget ?? DEFAULT_KNOWLEDGE_RESEARCH_BUDGET), id = this.newId("krun"), now = this.now();
    this.insert("knowledge_research_runs", {
      id, turn_scope_id: input.turnScopeId, turn_id: input.turnId, parent_session_path: input.parentSessionPath,
      question: input.question, status: "planning", completeness_policy: input.completenessPolicy ?? "source_diverse",
      budget_json: JSON.stringify(limits), created_at: now, updated_at: now,
    });
    return this.requireRun(id);
  }
  getRun(runId: string): KnowledgeResearchRun | null {
    text(runId);
    const row = this.knowledgeStore.db.prepare("SELECT * FROM knowledge_research_runs WHERE id = ?").get(runId);
    if (!row) return null;
    const run = record<KnowledgeResearchRun>(row);
    const scope = this.knowledgeStore.getTurnScope({ scopeId: run.turnScopeId });
    if (!scope || scope.turnId !== run.turnId || scope.sessionPath !== run.parentSessionPath) scopeViolation();
    run.budget = budget(run.budget);
    return run;
  }
  requireRun(runId: string): KnowledgeResearchRun {
    const run = this.getRun(runId);
    if (!run) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Research run was not found");
    return run;
  }

  createNeed(runId: string, input: NeedInput): KnowledgeEvidenceNeedRecord {
    keys(input, NEED_KEYS); text(input.claim, 1000);
    for (const key of ["required", "requireCounterEvidence", "requireAllRelevantUnits"] as const) if (typeof input[key] !== "boolean") invalid();
    return this.transaction(() => {
      this.requireActive(runId);
      const id = this.newId("kneed"), now = this.now();
      const ordinal = this.knowledgeStore.db.prepare("SELECT COALESCE(MAX(ordinal), -1) + 1 AS ordinal FROM knowledge_evidence_needs WHERE run_id = ?").get(runId).ordinal;
      this.insert("knowledge_evidence_needs", { id, run_id: runId, ordinal, claim: input.claim, kind: input.kind,
        required: Number(input.required), min_independent_sources: input.minIndependentSources,
        require_counter_evidence: Number(input.requireCounterEvidence), require_all_relevant_units: Number(input.requireAllRelevantUnits),
        status: "uncovered", unresolved_gaps_json: "[]", created_at: now, updated_at: now });
      return this.getNeed(runId, id);
    });
  }
  getNeed(runId: string, needId: string): KnowledgeEvidenceNeedRecord {
    this.requireRun(runId); text(needId);
    const row = this.knowledgeStore.db.prepare("SELECT * FROM knowledge_evidence_needs WHERE run_id = ? AND id = ?").get(runId, needId);
    if (!row) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Research need was not found in this run");
    return record(row);
  }
  listNeeds(runId: string): KnowledgeEvidenceNeedRecord[] {
    this.requireRun(runId);
    return this.knowledgeStore.db.prepare("SELECT * FROM knowledge_evidence_needs WHERE run_id = ? ORDER BY ordinal, id").all(runId).map(record);
  }
  /** 只供宿主根据已验证证据重算后落库，不接受模型声明的最终需求状态。 */
  setNeedState(runId: string, needId: string, state: Pick<KnowledgeEvidenceNeedRecord, "status" | "unresolvedGaps">): KnowledgeEvidenceNeedRecord {
    this.requireActive(runId); this.getNeed(runId, needId);
    keys(state, ["status", "unresolvedGaps"]); strings(state.unresolvedGaps, 500);
    if (state.unresolvedGaps.length > 8) invalid();
    this.knowledgeStore.db.prepare("UPDATE knowledge_evidence_needs SET status = ?, unresolved_gaps_json = ?, updated_at = ? WHERE run_id = ? AND id = ?")
      .run(state.status, JSON.stringify(state.unresolvedGaps), this.now(), runId, needId);
    return this.getNeed(runId, needId);
  }

  getReceipt(runId: string, receiptId: string): KnowledgeResearchReadReceipt {
    this.requireRun(runId); text(receiptId);
    const row = this.knowledgeStore.db.prepare("SELECT * FROM knowledge_research_read_receipts WHERE run_id = ? AND id = ?").get(runId, receiptId);
    if (!row) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Research receipt was not found in this run");
    return record(row);
  }
  insertReceipt(input: KnowledgeResearchReadReceipt): KnowledgeResearchReadReceipt {
    keys(input, ["id", "runId", "actorSessionId", ...LOCATION_KEYS, "channel", "createdAt", "consumedAt"]);
    this.location(input);
    this.insert("knowledge_research_read_receipts", {
      id: input.id, run_id: input.runId, actor_session_id: input.actorSessionId, source_id: input.sourceId,
      content_snapshot_id: input.contentSnapshotId, parse_artifact_id: input.parseArtifactId,
      chunk_index_variant_id: input.chunkIndexVariantId, chunk_id: input.chunkId, block_id: input.blockId,
      start_offset: input.startOffset, end_offset: input.endOffset, canonical_text_sha256: input.canonicalTextSha256,
      channel: input.channel, created_at: input.createdAt, consumed_at: input.consumedAt,
    });
    return this.getReceipt(input.runId, input.id);
  }
  consumeReceipt(runId: string, receiptId: string): KnowledgeResearchReadReceipt {
    this.requireActive(runId); this.getReceipt(runId, receiptId);
    this.knowledgeStore.db.prepare("UPDATE knowledge_research_read_receipts SET consumed_at = COALESCE(consumed_at, ?) WHERE run_id = ? AND id = ?")
      .run(this.now(), runId, receiptId);
    return this.getReceipt(runId, receiptId);
  }

  putEvidence(input: KnowledgeEvidenceItem): KnowledgeEvidenceItem {
    keys(input, ["id", "runId", ...LOCATION_KEYS, "canonicalText", "headingPath", "pageNumber", "createdAt"]);
    this.location(input); if (input.headingPath !== null) strings(input.headingPath, 1000);
    return this.transaction(() => {
      const existing = this.knowledgeStore.db.prepare(`SELECT * FROM knowledge_evidence_items
        WHERE run_id = ? AND parse_artifact_id = ? AND block_id = ? AND start_offset = ? AND end_offset = ?`)
        .get(input.runId, input.parseArtifactId, input.blockId, input.startOffset, input.endOffset);
      if (existing) {
        if (existing.canonical_text !== input.canonicalText || existing.canonical_text_sha256 !== input.canonicalTextSha256) {
          throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Stored research evidence differs from frozen text");
        }
        return record(existing);
      }
      this.insert("knowledge_evidence_items", { id: input.id, run_id: input.runId, source_id: input.sourceId,
        content_snapshot_id: input.contentSnapshotId, parse_artifact_id: input.parseArtifactId, chunk_index_variant_id: input.chunkIndexVariantId,
        chunk_id: input.chunkId, block_id: input.blockId, start_offset: input.startOffset, end_offset: input.endOffset,
        canonical_text: input.canonicalText, canonical_text_sha256: input.canonicalTextSha256,
        heading_path_json: input.headingPath === null ? null : JSON.stringify(input.headingPath), page_number: input.pageNumber, created_at: input.createdAt });
      return record(this.knowledgeStore.db.prepare("SELECT * FROM knowledge_evidence_items WHERE id = ?").get(input.id));
    });
  }
  linkEvidence(input: KnowledgeNeedEvidence): KnowledgeNeedEvidence {
    keys(input, ["needId", "evidenceId", "relation", "rationale", "sourceIndependenceKey", "createdAt"]); text(input.rationale, 1000);
    const need = this.knowledgeStore.db.prepare("SELECT run_id FROM knowledge_evidence_needs WHERE id = ?").get(input.needId);
    const evidence = this.knowledgeStore.db.prepare("SELECT run_id, source_id FROM knowledge_evidence_items WHERE id = ?").get(input.evidenceId);
    if (!need || !evidence || need.run_id !== evidence.run_id || input.sourceIndependenceKey !== evidence.source_id) scopeViolation();
    this.requireActive(need.run_id);
    const existing = this.knowledgeStore.db.prepare("SELECT * FROM knowledge_need_evidence WHERE need_id = ? AND evidence_id = ? AND relation = ?")
      .get(input.needId, input.evidenceId, input.relation);
    if (existing) return record(existing);
    this.insert("knowledge_need_evidence", { need_id: input.needId, evidence_id: input.evidenceId, relation: input.relation,
      rationale: input.rationale, source_independence_key: evidence.source_id, created_at: input.createdAt });
    return { ...input };
  }
  listEvidence(runId: string): KnowledgeEvidenceItem[] {
    this.requireRun(runId);
    return this.knowledgeStore.db.prepare("SELECT * FROM knowledge_evidence_items WHERE run_id = ? ORDER BY created_at, id").all(runId).map(record);
  }
  listRelations(runId: string, needId?: string): KnowledgeNeedEvidence[] {
    this.requireRun(runId); if (needId !== undefined) this.getNeed(runId, needId);
    return this.knowledgeStore.db.prepare(`SELECT relation.* FROM knowledge_need_evidence relation
      JOIN knowledge_evidence_needs need ON need.id = relation.need_id WHERE need.run_id = ?
      ${needId === undefined ? "" : "AND relation.need_id = ?"} ORDER BY need.ordinal, relation.evidence_id, relation.relation`)
      .all(...(needId === undefined ? [runId] : [runId, needId])).map(record);
  }

  private actionMetadata(run: KnowledgeResearchRun, input: KnowledgeResearchAction) {
    keys(input.requestSummary, ["query", "sourceIds", "needIds", "purpose"]);
    const request = input.requestSummary;
    if (request.query !== undefined) text(request.query, 4000);
    if (request.purpose !== undefined && request.purpose !== "counterexample") invalid();
    if (request.needIds !== undefined) {
      strings(request.needIds); for (const id of request.needIds) this.getNeed(run.id, id);
    }
    if (request.sourceIds !== undefined) {
      strings(request.sourceIds);
      const sources = this.knowledgeStore.getTurnScope({ scopeId: run.turnScopeId })!.sources;
      if (request.sourceIds.some(id => !sources.some(source => source.sourceId === id))) scopeViolation();
    }
    if (input.responseSummary !== null) {
      keys(input.responseSummary, ["hitIds", "receiptIds", "count", "status", "errorCode"]);
      const response = input.responseSummary;
      for (const key of ["hitIds", "receiptIds"]) if (response[key] !== undefined) strings(response[key]);
      if (response.count !== undefined && (!Number.isSafeInteger(response.count) || Number(response.count) < 0)) invalid();
      for (const key of ["status", "errorCode"]) if (response[key] !== undefined) text(response[key]);
    }
  }
  insertAction(input: KnowledgeResearchAction): KnowledgeResearchAction {
    keys(input, ["id", "runId", "roundId", "ordinal", "actorSessionId", "actorAgentId", "actionType", "requestSummary",
      "responseSummary", "status", "startedAt", "completedAt", "errorCode"]);
    const run = this.requireActive(input.runId); this.actionMetadata(run, input);
    if (input.roundId !== null && !this.knowledgeStore.db.prepare("SELECT id FROM knowledge_research_rounds WHERE id = ? AND run_id = ?")
      .get(input.roundId, input.runId)) scopeViolation();
    this.insert("knowledge_research_actions", { id: input.id, run_id: input.runId, round_id: input.roundId, ordinal: input.ordinal,
      actor_session_id: input.actorSessionId, actor_agent_id: input.actorAgentId, action_type: input.actionType,
      request_summary_json: JSON.stringify(input.requestSummary), response_summary_json: input.responseSummary === null ? null : JSON.stringify(input.responseSummary),
      status: input.status, started_at: input.startedAt, completed_at: input.completedAt, error_code: input.errorCode });
    return record(this.knowledgeStore.db.prepare("SELECT * FROM knowledge_research_actions WHERE id = ?").get(input.id));
  }
  listActions(runId: string): KnowledgeResearchAction[] {
    const run = this.requireRun(runId);
    return this.knowledgeStore.db.prepare("SELECT * FROM knowledge_research_actions WHERE run_id = ? ORDER BY ordinal, id").all(runId)
      .map((row: Record<string, unknown>) => { const action = record<KnowledgeResearchAction>(row); this.actionMetadata(run, action); return action; });
  }
  listRounds(runId: string): KnowledgeResearchRound[] {
    this.requireRun(runId);
    return this.knowledgeStore.db.prepare("SELECT * FROM knowledge_research_rounds WHERE run_id = ? ORDER BY ordinal, id").all(runId).map(record);
  }
}

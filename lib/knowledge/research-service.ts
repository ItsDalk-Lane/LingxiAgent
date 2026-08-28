import crypto from "node:crypto";

import { KnowledgeError, isKnowledgeError } from "./errors.ts";
import { KnowledgeStore } from "./knowledge-store.ts";
import { buildAnalysisManifest } from "./research-manifest.ts";
import {
  KnowledgeResearchStore,
  type ResearchClaim,
  type ResearchEvidence,
  type ResearchJob,
  type ResearchVerificationStep,
} from "./research-store.ts";
import {
  CLAIM_BUILD_SYSTEM_PROMPT,
  CONTRADICTION_SYSTEM_PROMPT,
  RESEARCH_ANALYSIS_SYSTEM_PROMPT,
  SYNTHESIS_SYSTEM_PROMPT,
  VERIFICATION_SYSTEM_PROMPT,
  parseAnalysisOutput,
  parseClaimBuildOutput,
  parseContradictionOutput,
  parseSynthesisOutput,
  parseVerificationOutput,
  renderAnalysisPrompt,
  renderClaimBuildPrompt,
  renderContradictionPrompt,
  renderUnitPayload,
  renderVerificationPrompt,
  type AnalysisEvidenceCandidate,
  type ResearchUnitPayload,
} from "./research-worker.ts";
import type { KnowledgeTextGenerator } from "./knowledge-query-service.ts";
import type { KnowledgeResearchPriority } from "./knowledge-query-service.ts";
import type {
  KnowledgeAnalysisUnit,
  KnowledgeBlock,
  KnowledgeEpistemicBasis,
  KnowledgeResearchReport,
  KnowledgeResearchRun,
  KnowledgeResearchSpec,
  KnowledgeScopeSnapshot,
} from "./types.ts";

const MAX_ATTEMPTS = 2;
const MAX_VERIFICATION_STEPS = 1;
const CLAIM_JOB_EVIDENCE_LIMIT = 48;
const CLAIM_PACK_SIZE = 20;

interface TaskRegistryLike {
  registerHandler(type: string, handler: { abort(taskId: string): void }): void;
  register(taskId: string, input: Record<string, unknown>): unknown;
  query(taskId: string): any;
  update(taskId: string, patch: Record<string, unknown>): unknown;
  complete(taskId: string, result?: unknown): unknown;
  fail(taskId: string, error?: unknown): unknown;
  cancel(taskId: string, reason?: string): unknown;
}

interface KnowledgeResearchServiceOptions {
  store: KnowledgeStore;
  researchStore: KnowledgeResearchStore;
  generateText?: KnowledgeTextGenerator | null;
  prioritizeScope?: (input: {
    studioId: string;
    scope: KnowledgeScopeSnapshot;
    question: string;
  }) => KnowledgeResearchPriority[];
  idGenerator?: (prefix: string) => string;
}

interface ValidatedCandidate {
  citationId: string;
  contentSnapshotId: string;
  parseArtifactId: string;
  blockId: string;
  startOffset: number;
  endOffset: number;
  canonicalQuote: string;
  quoteChecksum: string;
  epistemicBasis: KnowledgeEpistemicBasis;
}

function defaultId(prefix: string): string {
  return `${prefix}_${crypto.randomUUID()}`;
}

function safeErrorCode(error: unknown): string {
  if (isKnowledgeError(error)) return error.code;
  const candidate = error as { code?: unknown; status?: unknown; name?: unknown };
  if (typeof candidate?.code === "string" && /^[A-Z0-9_]{2,80}$/u.test(candidate.code)) return candidate.code;
  if (Number(candidate?.status) === 429) return "PROVIDER_RATE_LIMITED";
  if (Number(candidate?.status) >= 500) return "PROVIDER_UNAVAILABLE";
  return "KNOWLEDGE_RESEARCH_FAILED";
}

function isAbort(error: unknown, signal: AbortSignal): boolean {
  const candidate = error as { name?: unknown; code?: unknown };
  return signal.aborted || candidate?.name === "AbortError" || candidate?.code === "ABORT_ERR";
}

function isRetryable(error: unknown, attempt: number): boolean {
  if (attempt >= MAX_ATTEMPTS) return false;
  if (isKnowledgeError(error)) return error.code === "KNOWLEDGE_MODEL_OUTPUT_INVALID";
  const candidate = error as { retryable?: unknown; status?: unknown; code?: unknown; name?: unknown; message?: unknown };
  if (candidate?.retryable === true) return true;
  const status = Number(candidate?.status);
  if (status === 429 || status >= 500) return true;
  if (["ETIMEDOUT", "ECONNRESET", "ECONNREFUSED", "EAI_AGAIN", "UND_ERR_CONNECT_TIMEOUT"].includes(String(candidate?.code))) {
    return true;
  }
  if (candidate?.name === "TimeoutError") return true;
  const message = typeof candidate?.message === "string" ? candidate.message.toLowerCase() : "";
  return /timeout|temporar|network|rate limit|connection reset/u.test(message);
}

function buildResearchSpec(question: string, scope: KnowledgeScopeSnapshot): KnowledgeResearchSpec {
  return {
    originalQuestion: question,
    scopeSnapshotId: scope.id,
    notebookIds: scope.notebooks.map(notebook => notebook.notebookId),
    goal: `Answer the user's research question from the complete frozen Notebook scope: ${question}`,
    dimensions: [
      "direct findings",
      "causes and effects",
      "supporting and conflicting evidence",
      "uncertainties and limitations",
    ],
    outputRequirements: [
      "Use only validated evidence from the frozen scope",
      "Separate support status from epistemic basis",
      "Expose contradictions, uncertainty, limitations, and coverage",
    ],
    definitions: [
      "Full scan means every AnalysisUnit primary range has completed",
      "Complete contradiction scan means every AnalysisUnit and ClaimPack cell has completed",
    ],
    assumptions: [
      "The selected Notebooks define the complete research scope",
      "Low-impact wording ambiguity is recorded rather than expanding the scope",
    ],
  };
}

function requireJobRefs(job: ResearchJob): {
  evidenceIds: string[];
  candidateClaims: Array<{ text: string; evidenceIds: string[] }>;
} {
  const refs = job.inputRefs as any;
  if (
    !refs
    || !Array.isArray(refs.evidenceIds)
    || !Array.isArray(refs.candidateClaims)
    || refs.evidenceIds.some((value: unknown) => typeof value !== "string")
  ) {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Claim job references are corrupt");
  }
  return refs;
}

/** 全文研究编排器：宿主任务只承载控制，所有可恢复事实写入 knowledge.db。 */
export class KnowledgeResearchService {
  private readonly store: KnowledgeStore;
  readonly researchStore: KnowledgeResearchStore;
  private readonly generateText: KnowledgeTextGenerator | null;
  private readonly prioritizeScope: KnowledgeResearchServiceOptions["prioritizeScope"];
  private readonly idGenerator: (prefix: string) => string;
  private taskRegistry: TaskRegistryLike | null = null;
  private readonly running = new Map<string, Promise<void>>();
  private readonly controllers = new Map<string, AbortController>();
  private readonly runOwners = new Map<string, string>();

  constructor(options: KnowledgeResearchServiceOptions) {
    this.store = options.store;
    this.researchStore = options.researchStore;
    this.generateText = options.generateText || null;
    this.prioritizeScope = options.prioritizeScope;
    this.idGenerator = options.idGenerator || defaultId;
  }

  attachTaskRegistry(taskRegistry: TaskRegistryLike) {
    if (this.taskRegistry && this.taskRegistry !== taskRegistry) {
      throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Knowledge research already has a TaskRegistry");
    }
    if (this.taskRegistry === taskRegistry) return;
    this.taskRegistry = taskRegistry;
    taskRegistry.registerHandler("knowledge-research", {
      abort: (taskId) => {
        const runId = taskId.startsWith("knowledge-research:")
          ? taskId.slice("knowledge-research:".length)
          : "";
        if (!runId) return;
        this.controllers.get(runId)?.abort();
        const studioId = this.runOwners.get(runId);
        if (studioId) {
          try {
            this.researchStore.cancelRun({ studioId, runId });
          } catch {
            // TaskRegistry 已完成中止记账；领域终态由当前执行链或下次恢复继续对账。
          }
        }
      },
    });
  }

  async startResearch(input: {
    studioId: unknown;
    notebookIds: unknown;
    question: unknown;
  }): Promise<{
    run: ReturnType<KnowledgeStore["getKnowledgeRun"]>;
    research: KnowledgeResearchRun;
    scope: KnowledgeScopeSnapshot;
  }> {
    if (!this.generateText || !this.taskRegistry) {
      throw new KnowledgeError("KNOWLEDGE_MODEL_UNAVAILABLE", "Full Research is not ready to start");
    }
    if (typeof input.question !== "string" || !input.question.trim() || input.question.length > 4000) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge question is invalid");
    }
    const scope = this.store.createScopeSnapshot({
      studioId: input.studioId,
      notebookIds: input.notebookIds,
      mode: "research",
    });
    const run = this.store.createKnowledgeRun({
      studioId: input.studioId,
      mode: "research",
      question: input.question,
      scopeSnapshotId: scope.id,
      retrievalMode: "fts",
    });
    const spec = buildResearchSpec(run.question, scope);
    this.researchStore.createResearchRun({ studioId: run.studioId, runId: run.id, spec });
    this.runOwners.set(run.id, run.studioId);

    try {
      const blocksByArtifact = this.loadBlocks(run.studioId, scope);
      const priorities = this.prioritizeScope?.({
        studioId: run.studioId,
        scope,
        question: run.question,
      }) || [];
      if (priorities.length > 0) {
        this.store.recordRunRetrievals({
          studioId: run.studioId,
          runId: run.id,
          retrievals: priorities.map(priority => ({
            chunkId: priority.chunkId,
            parseArtifactId: priority.parseArtifactId,
            score: priority.score,
          })),
        });
      }
      const manifest = buildAnalysisManifest({
        runId: run.id,
        scope,
        blocksByArtifact,
        prioritizedBlockIds: new Set(priorities.flatMap(priority => priority.blockIds)),
      });
      this.researchStore.createManifest({ runId: run.id, ...manifest });
      const research = this.researchStore.getResearchRun({ studioId: run.studioId, runId: run.id });
      this.registerHostTask(research);
      queueMicrotask(() => this.launch(run.studioId, run.id));
      return { run, research, scope };
    } catch (error) {
      this.researchStore.failRun({
        studioId: run.studioId,
        runId: run.id,
        errorCode: safeErrorCode(error),
        partial: false,
      });
      throw error;
    }
  }

  getResearchRun(input: { studioId: unknown; runId: unknown }) {
    return this.researchStore.getResearchRun(input);
  }

  getReport(input: { studioId: unknown; runId: unknown }) {
    return this.researchStore.getReport(input);
  }

  listActiveResearchRuns(input: { studioId: unknown }) {
    return this.researchStore.listActiveResearchRunsForStudio(input);
  }

  cancel(input: { studioId: string; runId: string }) {
    const research = this.researchStore.getResearchRun(input);
    this.controllers.get(input.runId)?.abort();
    this.taskRegistry?.cancel(research.hostTaskId, "Knowledge research canceled by user");
    return this.researchStore.cancelRun(input);
  }

  async resumeRecoveringRuns() {
    if (!this.taskRegistry || !this.generateText) return;
    for (const active of this.researchStore.listActiveResearchRuns()) {
      this.runOwners.set(active.runId, active.studioId);
      this.researchStore.recoverRun({ studioId: active.studioId, runId: active.runId });
      const research = this.researchStore.getResearchRun(active);
      this.registerHostTask(research, true);
      this.launch(active.studioId, active.runId);
    }
  }

  async waitForRun(runId: string): Promise<void> {
    await Promise.resolve();
    const promise = this.running.get(runId);
    if (promise) await promise;
  }

  suspendForShutdown() {
    // 正常退出不是用户取消：只中止本进程调用，数据库中的运行态留给下次启动恢复。
    for (const controller of this.controllers.values()) controller.abort();
  }

  private registerHostTask(research: KnowledgeResearchRun, recovering = false) {
    this.taskRegistry!.register(research.hostTaskId, {
      type: "knowledge-research",
      persist: true,
      meta: {
        runId: research.runId,
        state: recovering ? "recovering" : research.state,
        sourceCount: research.manifest?.sourceCount || 0,
        unitCount: research.manifest?.unitCount || 0,
      },
    });
  }

  private launch(studioId: string, runId: string) {
    if (this.running.has(runId)) return;
    const controller = new AbortController();
    this.controllers.set(runId, controller);
    const promise = this.execute(studioId, runId, controller.signal)
      .catch((error) => {
        if (isAbort(error, controller.signal)) return;
        const current = this.researchStore.getResearchRun({ studioId, runId });
        if (["completed", "partial", "failed", "canceled"].includes(current.state)) return;
        const coverage = this.researchStore.getCoverage(runId);
        const partial = coverage.primaryScan.completed > 0
          || coverage.contradiction.completed > 0
          || coverage.citationValidation.completed > 0;
        this.researchStore.failRun({
          studioId,
          runId,
          errorCode: safeErrorCode(error),
          partial,
        });
        const taskId = `knowledge-research:${runId}`;
        const task = this.taskRegistry?.query(taskId);
        if (task && !["failed", "completed", "canceled", "aborted"].includes(task.status)) {
          this.taskRegistry?.fail(taskId, safeErrorCode(error));
        }
      })
      .finally(() => {
        this.running.delete(runId);
        this.controllers.delete(runId);
      });
    this.running.set(runId, promise);
  }

  private async execute(studioId: string, runId: string, signal: AbortSignal) {
    const research = this.researchStore.getResearchRun({ studioId, runId });
    const scope = this.store.getScopeSnapshot({ studioId, scopeSnapshotId: research.spec.scopeSnapshotId });
    const blocksByArtifact = this.loadBlocks(studioId, scope);
    const blocksById = new Map([...blocksByArtifact.values()].flat().map(block => [block.id, block]));

    await this.runFullScan({ studioId, research, blocksById, signal });
    this.throwIfAborted(signal);
    await this.validateAnalysisEvidence({ studioId, research, scope, blocksById });
    this.throwIfAborted(signal);
    await this.buildClaims({ research, signal });
    this.throwIfAborted(signal);
    await this.runContradictionPass({ studioId, research, scope, blocksById, signal });
    this.throwIfAborted(signal);
    await this.synthesize({ studioId, research, scope, blocksById, signal });
    this.throwIfAborted(signal);

    const completed = this.researchStore.getResearchRun({ studioId, runId });
    this.taskRegistry!.complete(completed.hostTaskId, {
      runId,
      coverage: completed.coverage,
      reportAvailable: completed.reportAvailable,
    });
  }

  private loadBlocks(studioId: string, scope: KnowledgeScopeSnapshot): Map<string, KnowledgeBlock[]> {
    const result = new Map<string, KnowledgeBlock[]>();
    for (const artifactId of new Set(scope.sources.map(source => source.parseArtifactId))) {
      result.set(artifactId, this.store.listArtifactBlocks({ studioId, parseArtifactId: artifactId }));
    }
    return result;
  }

  private unitPayloads(runId: string, blocksById: Map<string, KnowledgeBlock>): Map<string, ResearchUnitPayload> {
    return new Map(this.researchStore.listUnits(runId).map(unit => [
      unit.id,
      renderUnitPayload(unit, blocksById),
    ]));
  }

  private async runFullScan(input: {
    studioId: string;
    research: KnowledgeResearchRun;
    blocksById: Map<string, KnowledgeBlock>;
    signal: AbortSignal;
  }) {
    this.throwIfAborted(input.signal);
    this.researchStore.setState({ studioId: input.studioId, runId: input.research.runId, state: "scanning" });
    const unitPayloads = this.unitPayloads(input.research.runId, input.blocksById);
    for (const batch of this.researchStore.listBatches(input.research.runId)) {
      if (batch.status === "completed") continue;
      this.throwIfAborted(input.signal);
      const units = batch.unitIds.map(unitId => {
        const payload = unitPayloads.get(unitId);
        if (!payload) throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "ExecutionBatch unit is missing");
        return payload;
      });
      let completed = false;
      let attemptsUsed = this.researchStore.countAttempts("scan_batch", batch.id);
      if (attemptsUsed >= MAX_ATTEMPTS) {
        this.researchStore.exhaustWork({
          workType: "scan_batch",
          workId: batch.id,
          errorCode: "RETRY_BUDGET_EXHAUSTED",
        });
        throw new KnowledgeError("KNOWLEDGE_RESEARCH_INCOMPLETE", "Research batch exhausted its retry budget");
      }
      this.researchStore.beginBatch(batch.id);
      while (attemptsUsed < MAX_ATTEMPTS && !completed) {
        const attempt = this.researchStore.beginAttempt({
          runId: input.research.runId,
          workType: "scan_batch",
          workId: batch.id,
        });
        try {
          const raw = await this.generateText!({
            runId: input.research.runId,
            operation: "research_analysis",
            systemPrompt: RESEARCH_ANALYSIS_SYSTEM_PROMPT,
            userPrompt: renderAnalysisPrompt(input.research.spec, units, attempt.attemptNumber > 1),
            attempt: attempt.attemptNumber,
            signal: input.signal,
          });
          this.throwIfAborted(input.signal);
          const parsed = parseAnalysisOutput(raw, units);
          this.researchStore.completeScanAttempt({
            attemptId: attempt.id,
            batchId: batch.id,
            results: parsed.units.map(unit => ({ unitId: unit.unitId, value: unit })),
            rawOutput: parsed,
          });
          completed = true;
        } catch (error) {
          if (isAbort(error, input.signal)) throw error;
          const retry = isRetryable(error, attempt.attemptNumber);
          this.researchStore.failAttempt({
            attemptId: attempt.id,
            errorCode: safeErrorCode(error),
            message: error instanceof Error ? error.message : String(error),
            retry,
          });
          if (!retry) throw error;
        }
        attemptsUsed = attempt.attemptNumber;
      }
      this.updateHostProgress(input.research.runId, "scanning");
    }
    const coverage = this.researchStore.getCoverage(input.research.runId);
    if (coverage.primaryScan.total === 0 || coverage.primaryScan.completed !== coverage.primaryScan.total) {
      throw new KnowledgeError("KNOWLEDGE_RESEARCH_INCOMPLETE", "Full Research did not scan every AnalysisUnit");
    }
    this.researchStore.setState({
      studioId: input.studioId,
      runId: input.research.runId,
      state: "building_claims",
    });
  }

  private validateCandidate(input: {
    studioId: string;
    scope: KnowledgeScopeSnapshot;
    unit: KnowledgeAnalysisUnit;
    payload: ResearchUnitPayload;
    candidate: AnalysisEvidenceCandidate;
  }): ValidatedCandidate {
    const anchor = input.payload.anchors.find(entry => entry.anchorRef === input.candidate.anchorRef);
    if (!anchor || anchor.kind !== "primary") {
      throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Evidence must reference a primary anchor");
    }
    let candidateStart = input.candidate.startOffset;
    let candidateEnd = input.candidate.endOffset;
    if (
      input.candidate.endOffset > anchor.text.length
      || anchor.text.slice(candidateStart, candidateEnd) !== input.candidate.quote
    ) {
      // LLM 数不准字符偏移。quote 逐字且在 anchor 内唯一出现时,由服务端定位。
      const located = anchor.text.indexOf(input.candidate.quote);
      if (located >= 0 && anchor.text.indexOf(input.candidate.quote, located + 1) === -1) {
        candidateStart = located;
        candidateEnd = located + input.candidate.quote.length;
      } else {
        throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Evidence quote does not match the frozen text");
      }
    }
    const startOffset = anchor.blockStartOffset + candidateStart;
    const endOffset = anchor.blockStartOffset + candidateEnd;
    const scopeSource = input.scope.sources.find(source => source.parseArtifactId === input.unit.parseArtifactId);
    if (!scopeSource || anchor.parseArtifactId !== scopeSource.parseArtifactId) {
      throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Evidence escaped the frozen scope");
    }
    const citation = this.store.createCitation({
      studioId: input.studioId,
      parseArtifactId: input.unit.parseArtifactId,
      blockId: anchor.blockId,
      startOffset,
      endOffset,
    });
    if (citation.canonicalText !== input.candidate.quote) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Validated citation text changed unexpectedly");
    }
    return {
      citationId: citation.id,
      contentSnapshotId: scopeSource.contentSnapshotId,
      parseArtifactId: input.unit.parseArtifactId,
      blockId: anchor.blockId,
      startOffset,
      endOffset,
      canonicalQuote: citation.canonicalText,
      quoteChecksum: citation.canonicalTextSha256,
      epistemicBasis: input.candidate.epistemicBasis,
    };
  }

  private async validateAnalysisEvidence(input: {
    studioId: string;
    research: KnowledgeResearchRun;
    scope: KnowledgeScopeSnapshot;
    blocksById: Map<string, KnowledgeBlock>;
  }) {
    const units = new Map(this.researchStore.listUnits(input.research.runId).map(unit => [unit.id, unit]));
    const payloads = this.unitPayloads(input.research.runId, input.blocksById);
    for (const entry of this.researchStore.listUnitResults(input.research.runId)) {
      if (this.researchStore.hasEvidenceValidations("analysis", entry.unitId)) continue;
      const unit = units.get(entry.unitId);
      const payload = payloads.get(entry.unitId);
      if (!unit || !payload || !Array.isArray(entry.result?.evidenceCandidates)) {
        throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Analysis result cannot be validated");
      }
      this.researchStore.transaction(() => {
        entry.result.evidenceCandidates.forEach((candidate: AnalysisEvidenceCandidate, candidateOrdinal: number) => {
          try {
            const validated = this.validateCandidate({
              studioId: input.studioId,
              scope: input.scope,
              unit,
              payload,
              candidate,
            });
            this.researchStore.recordEvidenceValidation({
              runId: input.research.runId,
              unitId: unit.id,
              originType: "analysis",
              originId: unit.id,
              candidateOrdinal,
              status: "validated",
              citationId: validated.citationId,
              canonicalQuote: validated.canonicalQuote,
              quoteChecksum: validated.quoteChecksum,
              epistemicBasis: validated.epistemicBasis,
              evidence: validated,
            });
          } catch (error) {
            if (isKnowledgeError(error) && error.code === "KNOWLEDGE_STORAGE_INVALID") throw error;
            this.researchStore.recordEvidenceValidation({
              runId: input.research.runId,
              unitId: unit.id,
              originType: "analysis",
              originId: unit.id,
              candidateOrdinal,
              status: "invalid",
              reasonCode: safeErrorCode(error),
              epistemicBasis: candidate.epistemicBasis,
            });
          }
        });
      });
    }
  }

  private createClaimJobs(runId: string) {
    if (this.researchStore.listJobs(runId, "claim_build").length > 0) return;
    const refs: Array<{
      evidenceIds: string[];
      candidateClaims: Array<{ text: string; evidenceIds: string[] }>;
    }> = [];
    let current = { evidenceIds: [] as string[], candidateClaims: [] as Array<{ text: string; evidenceIds: string[] }> };
    for (const entry of this.researchStore.listUnitResults(runId)) {
      const byCandidate = this.researchStore.getAnalysisEvidenceByCandidate(entry.unitId);
      const evidenceIds = [...byCandidate.values()].map(evidence => evidence.id);
      if (current.evidenceIds.length > 0 && current.evidenceIds.length + evidenceIds.length > CLAIM_JOB_EVIDENCE_LIMIT) {
        refs.push(current);
        current = { evidenceIds: [], candidateClaims: [] };
      }
      current.evidenceIds.push(...evidenceIds);
      for (const claim of entry.result.candidateClaims || []) {
        const claimEvidenceIds = (claim.evidenceCandidateIndexes || [])
          .map((index: number) => byCandidate.get(index)?.id)
          .filter((value: string | undefined): value is string => Boolean(value));
        if (claimEvidenceIds.length > 0) {
          current.candidateClaims.push({ text: claim.text, evidenceIds: claimEvidenceIds });
        }
      }
    }
    if (current.evidenceIds.length > 0) refs.push(current);
    if (refs.length > 0) this.researchStore.createJobs({ runId, phase: "claim_build", refs });
  }

  private async buildClaims(input: { research: KnowledgeResearchRun; signal: AbortSignal }) {
    this.throwIfAborted(input.signal);
    this.createClaimJobs(input.research.runId);
    for (const job of this.researchStore.listJobs(input.research.runId, "claim_build")) {
      if (job.status === "completed") continue;
      const refs = requireJobRefs(job);
      const evidence = refs.evidenceIds.map(id => this.researchStore.getEvidence(id));
      const evidenceRefMap = new Map(evidence.map((entry, index) => [`E${index + 1}`, entry]));
      const refByEvidenceId = new Map([...evidenceRefMap].map(([ref, entry]) => [entry.id, ref]));
      const candidateClaims = refs.candidateClaims.map(claim => ({
        text: claim.text,
        evidenceRefs: claim.evidenceIds.map(id => refByEvidenceId.get(id)).filter((ref): ref is string => Boolean(ref)),
      }));
      let completed = false;
      let attemptsUsed = this.researchStore.countAttempts("claim_job", job.id);
      if (attemptsUsed >= MAX_ATTEMPTS) {
        this.researchStore.exhaustWork({
          workType: "claim_job",
          workId: job.id,
          errorCode: "RETRY_BUDGET_EXHAUSTED",
        });
        throw new KnowledgeError("KNOWLEDGE_RESEARCH_INCOMPLETE", "Claim job exhausted its retry budget");
      }
      this.researchStore.beginJob(job.id);
      while (attemptsUsed < MAX_ATTEMPTS && !completed) {
        const attempt = this.researchStore.beginAttempt({
          runId: input.research.runId,
          workType: "claim_job",
          workId: job.id,
        });
        try {
          const raw = await this.generateText!({
            runId: input.research.runId,
            operation: "claim_build",
            systemPrompt: CLAIM_BUILD_SYSTEM_PROMPT,
            userPrompt: renderClaimBuildPrompt({
              spec: input.research.spec,
              evidence: [...evidenceRefMap].map(([evidenceRef, item]) => ({ evidenceRef, evidence: item })),
              candidateClaims,
              retry: attempt.attemptNumber > 1,
            }),
            attempt: attempt.attemptNumber,
            signal: input.signal,
          });
          this.throwIfAborted(input.signal);
          const parsed = parseClaimBuildOutput(raw, new Set(evidenceRefMap.keys()));
          this.researchStore.completeClaimJob({
            attemptId: attempt.id,
            jobId: job.id,
            runId: input.research.runId,
            claims: parsed.claims.map(claim => ({
              text: claim.text,
              supportStatus: claim.supportStatus,
              epistemicBasis: claim.epistemicBasis,
              evidence: claim.evidence.map(relation => ({
                evidenceId: evidenceRefMap.get(relation.evidenceRef)!.id,
                relation: relation.relation,
              })),
            })),
            output: parsed,
          });
          completed = true;
        } catch (error) {
          if (isAbort(error, input.signal)) throw error;
          const retry = isRetryable(error, attempt.attemptNumber);
          this.researchStore.failAttempt({
            attemptId: attempt.id,
            errorCode: safeErrorCode(error),
            message: error instanceof Error ? error.message : String(error),
            retry,
          });
          if (!retry) throw error;
        }
        attemptsUsed = attempt.attemptNumber;
      }
    }
  }

  private createContradictionManifest(runId: string) {
    if (this.researchStore.hasContradictionManifest(runId)) return;
    const claims = this.researchStore.listClaims(runId);
    const packs: Array<{ id: string; claimIds: string[] }> = [];
    for (let offset = 0; offset < claims.length; offset += CLAIM_PACK_SIZE) {
      packs.push({
        id: this.idGenerator("cpack"),
        claimIds: claims.slice(offset, offset + CLAIM_PACK_SIZE).map(claim => claim.id),
      });
    }
    this.researchStore.createContradictionManifest({ runId, packs });
  }

  private async runContradictionPass(input: {
    studioId: string;
    research: KnowledgeResearchRun;
    scope: KnowledgeScopeSnapshot;
    blocksById: Map<string, KnowledgeBlock>;
    signal: AbortSignal;
  }) {
    this.researchStore.setState({
      studioId: input.studioId,
      runId: input.research.runId,
      state: "checking_contradictions",
    });
    this.createContradictionManifest(input.research.runId);
    const units = new Map(this.researchStore.listUnits(input.research.runId).map(unit => [unit.id, unit]));
    const payloads = this.unitPayloads(input.research.runId, input.blocksById);
    const claims = new Map(this.researchStore.listClaims(input.research.runId).map(claim => [claim.id, claim]));
    const packs = new Map(this.researchStore.listClaimPacks(input.research.runId).map(pack => [pack.id, pack]));
    for (const check of this.researchStore.listContradictionChecks(input.research.runId)) {
      if (check.status === "completed") continue;
      this.throwIfAborted(input.signal);
      const unit = units.get(check.unitId);
      const payload = payloads.get(check.unitId);
      const pack = packs.get(check.claimPackId);
      if (!unit || !payload || !pack) {
        throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Contradiction manifest is incomplete");
      }
      const packClaims = pack.claimIds.map(id => {
        const claim = claims.get(id);
        if (!claim) throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Claim pack references a missing claim");
        return claim;
      });
      const claimRefMap = new Map(packClaims.map((claim, index) => [`C${index + 1}`, claim]));
      let completed = false;
      let attemptsUsed = this.researchStore.countAttempts("contradiction_check", check.id);
      if (attemptsUsed >= MAX_ATTEMPTS) {
        this.researchStore.exhaustWork({
          workType: "contradiction_check",
          workId: check.id,
          errorCode: "RETRY_BUDGET_EXHAUSTED",
        });
        throw new KnowledgeError("KNOWLEDGE_RESEARCH_INCOMPLETE", "Contradiction check exhausted its retry budget");
      }
      this.researchStore.beginContradictionCheck(check.id);
      while (attemptsUsed < MAX_ATTEMPTS && !completed) {
        const attempt = this.researchStore.beginAttempt({
          runId: input.research.runId,
          workType: "contradiction_check",
          workId: check.id,
        });
        try {
          const raw = await this.generateText!({
            runId: input.research.runId,
            operation: "contradiction_check",
            systemPrompt: CONTRADICTION_SYSTEM_PROMPT,
            userPrompt: renderContradictionPrompt({
              spec: input.research.spec,
              unit: payload,
              claimPackId: pack.id,
              claims: [...claimRefMap].map(([claimRef, claim]) => ({ claimRef, claim })),
              retry: attempt.attemptNumber > 1,
            }),
            attempt: attempt.attemptNumber,
            signal: input.signal,
          });
          this.throwIfAborted(input.signal);
          const parsed = parseContradictionOutput(raw, {
            unit: payload,
            claimPackId: pack.id,
            claimRefs: new Set(claimRefMap.keys()),
          });
          const validated = parsed.matches.flatMap(match => {
            try {
              return [{
                match,
                evidence: this.validateCandidate({
                  studioId: input.studioId,
                  scope: input.scope,
                  unit,
                  payload,
                  candidate: match,
                }),
              }];
            } catch {
              // quote 定位失败的 match 丢弃,不让整单元重试
              return [];
            }
          });
          this.researchStore.transaction(() => {
            const contradictions: Array<{
              claimId: string;
              evidenceId: string;
              relation: "contradicts" | "context";
              explanation: string;
            }> = [];
            validated.forEach(({ match, evidence }, candidateOrdinal) => {
              const stored = this.researchStore.recordEvidenceValidation({
                runId: input.research.runId,
                unitId: unit.id,
                originType: "contradiction",
                originId: check.id,
                candidateOrdinal,
                status: "validated",
                citationId: evidence.citationId,
                canonicalQuote: evidence.canonicalQuote,
                quoteChecksum: evidence.quoteChecksum,
                epistemicBasis: evidence.epistemicBasis,
                evidence,
              });
              if (!stored) throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Contradiction evidence was not stored");
              contradictions.push({
                claimId: claimRefMap.get(match.claimRef)!.id,
                evidenceId: stored.id,
                relation: match.relation,
                explanation: match.explanation,
              });
            });
            this.researchStore.completeContradictionCheck({
              attemptId: attempt.id,
              checkId: check.id,
              // 只存通过验证的 matches:coverage.total 按 result_json.matches 统计,
              // 必须与 evidence_validations 同源,否则 citationValidation 永不闭合。
              output: { ...parsed, matches: validated.map(entry => entry.match) },
              contradictions,
            });
          });
          completed = true;
        } catch (error) {
          if (isAbort(error, input.signal)) throw error;
          const retry = isRetryable(error, attempt.attemptNumber);
          this.researchStore.failAttempt({
            attemptId: attempt.id,
            errorCode: safeErrorCode(error),
            message: error instanceof Error ? error.message : String(error),
            retry,
          });
          if (!retry) throw error;
        }
        attemptsUsed = attempt.attemptNumber;
      }
      this.updateHostProgress(input.research.runId, "checking_contradictions");
    }
    const coverage = this.researchStore.getCoverage(input.research.runId);
    if (coverage.contradiction.completed !== coverage.contradiction.total) {
      throw new KnowledgeError("KNOWLEDGE_RESEARCH_INCOMPLETE", "Full contradiction pass is incomplete");
    }
    this.researchStore.applyContradictionSupportStatus(input.research.runId);
  }

  private async synthesize(input: {
    studioId: string;
    research: KnowledgeResearchRun;
    scope: KnowledgeScopeSnapshot;
    blocksById: Map<string, KnowledgeBlock>;
    signal: AbortSignal;
  }) {
    this.researchStore.setState({
      studioId: input.studioId,
      runId: input.research.runId,
      state: "synthesizing",
    });
    while (true) {
      let jobs = this.researchStore.listJobs(input.research.runId, "final_synthesis");
      if (jobs.length === 0) {
        jobs = this.researchStore.createJobs({
          runId: input.research.runId,
          phase: "final_synthesis",
          refs: [{
            claimIds: this.researchStore.listClaims(input.research.runId).map(claim => claim.id),
            evidenceIds: this.researchStore.listEvidence(input.research.runId).map(evidence => evidence.id),
          }],
        });
      }
      const job = jobs[jobs.length - 1];
      const claims = this.researchStore.listClaims(input.research.runId);
      const evidence = this.researchStore.listEvidence(input.research.runId);
      const claimRefMap = new Map(claims.map((claim, index) => [`C${index + 1}`, claim]));
      const evidenceRefMap = new Map(evidence.map((item, index) => [`E${index + 1}`, item]));
      const refToClaimId = new Map([...claimRefMap].map(([ref, claim]) => [ref, claim.id]));
      const completedVerificationSteps = this.researchStore.listVerificationSteps(input.research.runId)
        .filter(step => step.status === "completed");

      if (job.status === "completed") {
        if (this.researchStore.getResearchRun({ studioId: input.studioId, runId: input.research.runId }).reportAvailable) return;
        const parsed = parseSynthesisOutput(JSON.stringify(job.output), new Set(claimRefMap.keys()));
        if (parsed.verificationRequests.length === 0) {
          throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Completed synthesis job has no report");
        }
        let step = this.researchStore.listVerificationSteps(input.research.runId)
          .find(entry => entry.triggerSynthesisJobId === job.id);
        if (!step) {
          if (completedVerificationSteps.length >= MAX_VERIFICATION_STEPS) {
            throw new KnowledgeError("KNOWLEDGE_RESEARCH_INCOMPLETE", "Verification budget is exhausted");
          }
          step = this.researchStore.createVerificationStep({
            runId: input.research.runId,
            triggerSynthesisJobId: job.id,
            requests: parsed.verificationRequests.map(request => ({
              claimId: refToClaimId.get(request.claimRef)!,
              reason: request.reason,
            })),
          });
        }
        await this.runVerificationStep({ ...input, step });
        this.throwIfAborted(input.signal);
        const refreshedJobs = this.researchStore.listJobs(input.research.runId, "final_synthesis");
        if (refreshedJobs[refreshedJobs.length - 1].id === job.id) {
          this.researchStore.createJobs({
            runId: input.research.runId,
            phase: "final_synthesis",
            refs: [{
              claimIds: this.researchStore.listClaims(input.research.runId).map(claim => claim.id),
              evidenceIds: this.researchStore.listEvidence(input.research.runId).map(item => item.id),
            }],
          });
        }
        continue;
      }

      const warnings = [...new Set(input.scope.sources.flatMap(source => (
        this.store.getParseArtifact({ studioId: input.studioId, parseArtifactId: source.parseArtifactId }).warnings
      )))];
      const allAnalysisUncertainties = [...new Set(
        this.researchStore.listUnitResults(input.research.runId)
          .flatMap(entry => Array.isArray(entry.result?.uncertainties) ? entry.result.uncertainties : [])
          .filter((entry): entry is string => typeof entry === "string" && Boolean(entry.trim()))
          .map(entry => entry.trim()),
      )];
      const analysisUncertainties = allAnalysisUncertainties.slice(0, 100);
      const coverage = this.researchStore.getCoverage(input.research.runId);
      const limitations = [
        ...input.research.spec.assumptions,
        ...warnings.map(warning => `Extraction warning: ${warning}`),
        ...completedVerificationSteps.map(step => (
          `Verification Step ${step.ordinal + 1} rechecked all frozen AnalysisUnits for ${step.requests.length} claim(s).`
        )),
        ...(allAnalysisUncertainties.length > analysisUncertainties.length
          ? [`${allAnalysisUncertainties.length - analysisUncertainties.length} additional analysis uncertainties were retained in unit results but omitted from the concise report.`]
          : []),
        ...(coverage.citationValidation.invalid > 0
          ? [`${coverage.citationValidation.invalid} evidence candidates failed citation validation and were excluded.`]
          : []),
      ];
      const userPrompt = (retry: boolean) => JSON.stringify({
        task: "evidence_only_synthesis",
        retryInstruction: retry ? "Your previous output failed strict validation. Return the exact schema and obey the verification budget." : null,
        verificationBudgetRemaining: Math.max(0, MAX_VERIFICATION_STEPS - completedVerificationSteps.length),
        completedVerificationSteps: completedVerificationSteps.length,
        researchSpec: input.research.spec,
        claims: [...claimRefMap].map(([claimRef, claim]) => ({
          claimRef,
          text: claim.text,
          supportStatus: claim.supportStatus,
          epistemicBasis: claim.epistemicBasis,
          evidenceRefs: claim.evidence.map(relation => {
            const ref = [...evidenceRefMap].find(([, item]) => item.id === relation.evidenceId)?.[0];
            return ref ? { evidenceRef: ref, relation: relation.relation } : null;
          }).filter(Boolean),
        })),
        evidence: [...evidenceRefMap].map(([evidenceRef, item]) => ({
          evidenceRef,
          quote: item.canonicalQuote,
          epistemicBasis: item.epistemicBasis,
        })),
        contradictions: this.researchStore.listContradictions(input.research.runId).map(item => ({
          claimRef: [...claimRefMap].find(([, claim]) => claim.id === item.claimId)?.[0] || null,
          evidenceRef: [...evidenceRefMap].find(([, entry]) => entry.id === item.evidenceId)?.[0] || null,
          relation: item.relation,
          explanation: item.explanation,
        })),
        coverage,
        extractionWarnings: warnings,
        limitations,
      });

      let attemptsUsed = this.researchStore.countAttempts("synthesis_job", job.id);
      if (attemptsUsed >= MAX_ATTEMPTS) {
        this.researchStore.exhaustWork({
          workType: "synthesis_job",
          workId: job.id,
          errorCode: "RETRY_BUDGET_EXHAUSTED",
        });
        throw new KnowledgeError("KNOWLEDGE_RESEARCH_INCOMPLETE", "Synthesis exhausted its retry budget");
      }
      this.researchStore.beginJob(job.id);
      let verificationRequested = false;
      while (attemptsUsed < MAX_ATTEMPTS) {
        const attempt = this.researchStore.beginAttempt({
          runId: input.research.runId,
          workType: "synthesis_job",
          workId: job.id,
        });
        try {
          const raw = await this.generateText!({
            runId: input.research.runId,
            operation: "final_synthesis",
            systemPrompt: SYNTHESIS_SYSTEM_PROMPT,
            userPrompt: userPrompt(attempt.attemptNumber > 1),
            attempt: attempt.attemptNumber,
            signal: input.signal,
          });
          this.throwIfAborted(input.signal);
          const parsed = parseSynthesisOutput(raw, new Set(claimRefMap.keys()));
          if (parsed.verificationRequests.length > 0) {
            if (completedVerificationSteps.length >= MAX_VERIFICATION_STEPS) {
              throw new KnowledgeError(
                "KNOWLEDGE_MODEL_OUTPUT_INVALID",
                "Synthesis requested verification after the verification budget was exhausted",
              );
            }
            this.researchStore.completeJob({ attemptId: attempt.id, jobId: job.id, output: parsed });
            const step = this.researchStore.createVerificationStep({
              runId: input.research.runId,
              triggerSynthesisJobId: job.id,
              requests: parsed.verificationRequests.map(request => ({
                claimId: refToClaimId.get(request.claimRef)!,
                reason: request.reason,
              })),
            });
            await this.runVerificationStep({ ...input, step });
            this.researchStore.createJobs({
              runId: input.research.runId,
              phase: "final_synthesis",
              refs: [{
                claimIds: this.researchStore.listClaims(input.research.runId).map(claim => claim.id),
                evidenceIds: this.researchStore.listEvidence(input.research.runId).map(item => item.id),
              }],
            });
            verificationRequested = true;
            break;
          }
          const evidenceById = new Map(evidence.map(item => [item.id, item]));
          const usedEvidence = new Map<string, ResearchEvidence>();
          const mapItems = (items: typeof parsed.conclusions) => items.map(item => {
            const claimIds = item.claimRefs.map(ref => refToClaimId.get(ref)!);
            const itemEvidence = claimIds.flatMap(claimId => (
              claims.find(claim => claim.id === claimId)?.evidence || []
            )).map(relation => evidenceById.get(relation.evidenceId)).filter((entry): entry is ResearchEvidence => Boolean(entry));
            itemEvidence.forEach(entry => usedEvidence.set(entry.id, entry));
            return { text: item.text, claimIds, citationMarkers: [] as number[] };
          });
          const conclusions = mapItems(parsed.conclusions);
          const majorFindings = mapItems(parsed.majorFindings);
          const conflicts = mapItems(parsed.conflicts);
          const citations = [...usedEvidence.values()].map((item, index) => ({
            marker: index + 1,
            evidenceId: item.id,
            citationId: item.citationId,
          }));
          const markerByEvidence = new Map(citations.map(citation => [citation.evidenceId, citation.marker]));
          const attachMarkers = (items: typeof conclusions) => items.map(item => ({
            ...item,
            citationMarkers: [...new Set(item.claimIds.flatMap(claimId => (
              claims.find(claim => claim.id === claimId)?.evidence || []
            )).map(relation => markerByEvidence.get(relation.evidenceId)).filter((marker): marker is number => Boolean(marker)))],
          }));
          const report: Omit<KnowledgeResearchReport, "createdAt" | "coverage"> = {
            runId: input.research.runId,
            title: parsed.title,
            summary: parsed.summary,
            conclusions: attachMarkers(conclusions),
            majorFindings: attachMarkers(majorFindings),
            conflicts: attachMarkers(conflicts),
            uncertainties: [...new Set([...parsed.uncertainties, ...analysisUncertainties])],
            limitations: [...new Set([...parsed.limitations, ...limitations])],
            citations,
          };
          this.researchStore.completeReport({
            studioId: input.studioId,
            runId: input.research.runId,
            attemptId: attempt.id,
            jobId: job.id,
            report,
          });
          return;
        } catch (error) {
          if (isAbort(error, input.signal)) throw error;
          const retry = isRetryable(error, attempt.attemptNumber);
          this.researchStore.failAttempt({
            attemptId: attempt.id,
            errorCode: safeErrorCode(error),
            message: error instanceof Error ? error.message : String(error),
            retry,
          });
          if (!retry) throw error;
        }
        attemptsUsed = attempt.attemptNumber;
      }
      if (verificationRequested) continue;
    }
  }

  private async runVerificationStep(input: {
    studioId: string;
    research: KnowledgeResearchRun;
    scope: KnowledgeScopeSnapshot;
    blocksById: Map<string, KnowledgeBlock>;
    signal: AbortSignal;
    step: ResearchVerificationStep;
  }) {
    if (input.step.status === "completed") return;
    const step = this.researchStore.listVerificationSteps(input.research.runId)
      .find(entry => entry.id === input.step.id);
    if (!step) throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Verification Step is missing");
    if (step.status !== "running") this.researchStore.beginVerificationStep(step.id);
    const allClaims = this.researchStore.listClaims(input.research.runId);
    const claimRefById = new Map(allClaims.map((claim, index) => [claim.id, `C${index + 1}`]));
    const requestedClaims = step.requests.map(request => {
      const claim = this.researchStore.getClaim(request.claimId);
      const claimRef = claimRefById.get(claim.id);
      if (!claimRef || claim.runId !== input.research.runId) {
        throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Verification Step references a missing claim");
      }
      return { claimRef, claim, reason: request.reason };
    });
    const claimByRef = new Map(requestedClaims.map(entry => [entry.claimRef, entry.claim]));
    const payloads = this.unitPayloads(input.research.runId, input.blocksById);
    for (const cell of this.researchStore.listVerificationCells(step.id)) {
      if (cell.status === "completed") continue;
      this.throwIfAborted(input.signal);
      const unit = this.researchStore.getUnit(input.research.runId, cell.unitId);
      const payload = payloads.get(unit.id);
      if (!payload) throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Verification unit is missing");
      let attemptsUsed = this.researchStore.countVerificationAttempts(cell.id);
      if (attemptsUsed >= MAX_ATTEMPTS) {
        this.researchStore.exhaustVerificationCell({
          stepId: step.id,
          cellId: cell.id,
          errorCode: "RETRY_BUDGET_EXHAUSTED",
        });
        throw new KnowledgeError("KNOWLEDGE_RESEARCH_INCOMPLETE", "Verification cell exhausted its retry budget");
      }
      this.researchStore.beginVerificationCell(cell.id);
      let completed = false;
      while (attemptsUsed < MAX_ATTEMPTS && !completed) {
        const attempt = this.researchStore.beginVerificationAttempt({
          runId: input.research.runId,
          stepId: step.id,
          cellId: cell.id,
        });
        try {
          const raw = await this.generateText!({
            runId: input.research.runId,
            operation: "research_verification",
            systemPrompt: VERIFICATION_SYSTEM_PROMPT,
            userPrompt: renderVerificationPrompt({
              spec: input.research.spec,
              verificationStepId: step.id,
              unit: payload,
              claims: requestedClaims,
              retry: attempt.attemptNumber > 1,
            }),
            attempt: attempt.attemptNumber,
            signal: input.signal,
          });
          this.throwIfAborted(input.signal);
          const parsed = parseVerificationOutput(raw, {
            verificationStepId: step.id,
            unit: payload,
            claimRefs: new Set(claimByRef.keys()),
          });
          this.researchStore.transaction(() => {
            const relations: Array<{
              claimId: string;
              evidenceId: string;
              relation: "supports" | "contradicts" | "context";
              explanation: string;
            }> = [];
            parsed.matches.forEach((match, candidateOrdinal) => {
              try {
                const validated = this.validateCandidate({
                  studioId: input.studioId,
                  scope: input.scope,
                  unit,
                  payload,
                  candidate: match,
                });
                const evidence = this.researchStore.recordEvidenceValidation({
                  runId: input.research.runId,
                  unitId: unit.id,
                  originType: "analysis",
                  originId: cell.id,
                  candidateOrdinal,
                  status: "validated",
                  citationId: validated.citationId,
                  canonicalQuote: validated.canonicalQuote,
                  quoteChecksum: validated.quoteChecksum,
                  epistemicBasis: validated.epistemicBasis,
                  evidence: validated,
                });
                if (!evidence) throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Verification evidence was not stored");
                relations.push({
                  claimId: claimByRef.get(match.claimRef)!.id,
                  evidenceId: evidence.id,
                  relation: match.relation,
                  explanation: match.explanation,
                });
              } catch (error) {
                if (isKnowledgeError(error) && error.code === "KNOWLEDGE_STORAGE_INVALID") throw error;
                this.researchStore.recordEvidenceValidation({
                  runId: input.research.runId,
                  unitId: unit.id,
                  originType: "analysis",
                  originId: cell.id,
                  candidateOrdinal,
                  status: "invalid",
                  reasonCode: safeErrorCode(error),
                  epistemicBasis: match.epistemicBasis,
                });
              }
            });
            this.researchStore.completeVerificationCell({
              attemptId: attempt.id,
              cellId: cell.id,
              output: parsed,
              relations,
            });
          });
          completed = true;
        } catch (error) {
          if (isAbort(error, input.signal)) throw error;
          const retry = isRetryable(error, attempt.attemptNumber);
          this.researchStore.failVerificationAttempt({
            attemptId: attempt.id,
            errorCode: safeErrorCode(error),
            retry,
          });
          if (!retry) throw error;
        }
        attemptsUsed = attempt.attemptNumber;
      }
      this.updateHostProgress(input.research.runId, "verifying_evidence");
    }
    this.researchStore.completeVerificationStep(step.id);
    this.researchStore.applyVerificationSupportStatus(input.research.runId);
    this.researchStore.applyContradictionSupportStatus(input.research.runId);
  }

  private updateHostProgress(runId: string, state: string) {
    const research = this.researchStore.listActiveResearchRuns().find(entry => entry.runId === runId);
    const taskId = `knowledge-research:${runId}`;
    if (!research || !this.taskRegistry?.query(taskId)) return;
    const coverage = this.researchStore.getCoverage(runId);
    const metric = state === "checking_contradictions" ? coverage.contradiction : coverage.primaryScan;
    this.taskRegistry.update(taskId, {
      status: "running",
      progress: {
        current: metric.completed,
        total: metric.total,
        message: state,
      },
      meta: { state, coverage },
    });
  }

  private throwIfAborted(signal: AbortSignal) {
    if (signal.aborted) throw new DOMException("Knowledge research canceled", "AbortError");
  }
}

/**
 * knowledge-coverage-executor —— CoverageExecutor / CoverageLedger / CoverageGate
 * （任务书 §五十–§五十二、§五十五/§五十六、§六十三/§六十四、§六十五、§八十四–§八十七，Phase 9）。
 *
 * 职责：读取 CoverageManifest → 确定性 Sharding → 有界并发池逐 shard 调 worker
 * （依赖注入 CoverageWorkerModel，prompt 进、严格 JSON ShardResult 出）→ 失败纠错
 * 重试一次 + shard 级 bounded retry（≤2 次自动重试后终态 failed）→ 取消
 * （AbortSignal：pending 置 cancelled、running 中止、completed 结果保留诊断）→
 * 持久化 coverage_runs / coverage_shards（v14）并支持恢复（completed shard 不重跑、
 * pending 续跑、running 按 recovery policy 置回 pending）。
 *
 * 优先级扫描（§六十三）：priorityOrder 只改变执行顺序（相关性高的先扫），不改变
 * 必达性——所有 primary shard 都进入终态后才允许综合（§五十一）。Ledger 由
 * manifest + shard 终态计算（§五十五）；Gate 严格判定 complete/partial（§五十六，
 * 宁漏勿假）。不存任何 CoT（§五十三）；contradictions 只是 ShardResult 结构字段，
 * 不复活 Research 领域模型（§六十六）。
 */
import { KnowledgeError } from "./errors.ts";
import {
  KNOWLEDGE_COVERAGE_CIRCUIT_BREAK,
  KNOWLEDGE_COVERAGE_PARTIAL,
  KNOWLEDGE_COVERAGE_SHARD_FAILED,
} from "../../shared/knowledge-reason-codes.ts";
import {
  aggregateShardEvidence,
  buildShardWorkerPrompt,
  parseShardResult,
  planCoverageShards,
  shardKnownBlocks,
  type AggregateEvidence,
  type CoverageManifest,
  type CoveragePlanSummary,
  type CoverageShardPlan,
  type CoverageSourceFidelity,
  type CoverageWorkerModel,
  type ShardResult,
} from "./knowledge-coverage-manifest.ts";
import type { CoverageUnit } from "./knowledge-coverage-unit.ts";

/** 并发池默认上限（§八十七：不能 1000 shards → 1000 subagents）。 */
export const COVERAGE_EXECUTOR_DEFAULT_CONCURRENCY = 4;

/** shard 级 bounded retry 上限：1 次首跑 + 2 次自动重试后终态 failed。 */
export const COVERAGE_SHARD_MAX_ATTEMPTS = 3;

/**
 * 熔断阈值（2026-08-30 延迟加固）：run 内零成功且终态 failed 的 shard 数达到
 * 该值（默认 = 默认并发波次 4，即第一个整波全部烧完 bounded retry 仍零成功）
 * → 提前取消剩余 shard，reasonCode 记 KNOWLEDGE_COVERAGE_CIRCUIT_BREAK。
 * 按终态失败计数（而非 attempt 级）：单个 shard 的 bounded retry 语义不变，
 * 小 run（shard 数 < 阈值）永不触发。任一 shard 成功即豁免——有产出说明
 * worker 可用，后续失败留给 shard 级 bounded retry 与 PARTIAL 留痕处理。
 */
export const COVERAGE_CIRCUIT_BREAK_FAILURES = COVERAGE_EXECUTOR_DEFAULT_CONCURRENCY;

/** 每次 attempt 内的输出纠错重试次数（JSON 非法/契约不符时重发一次）。 */
const SHARD_ATTEMPT_CORRECTION_RETRIES = 1;

// ─────────────────────── 持久化记录形状（schema v14） ───────────────────────

export type CoverageRunStatus = "pending" | "running" | "complete" | "partial" | "cancelled" | "failed";
export type CoverageShardStatus = "pending" | "running" | "completed" | "failed" | "cancelled";

export interface CoverageRunRecord {
  id: string;
  turnScopeId: string;
  manifestHash: string;
  manifestJson: string;
  status: CoverageRunStatus;
  expectedUnits: number;
  processedUnits: number;
  failedUnits: number;
  createdAt: string;
  updatedAt: string;
}

export interface CoverageShardRecord {
  id: string;
  runId: string;
  ordinal: number;
  unitIds: string[];
  contextBeforeUnitIds: string[];
  contextAfterUnitIds: string[];
  status: CoverageShardStatus;
  attemptCount: number;
  resultJson: string | null;
  updatedAt: string;
}

/** executor 需要的持久化面（KnowledgeStore v14 方法的结构化子集，依赖注入）。 */
export interface CoverageRunStore {
  createCoverageRun(input: {
    turnScopeId: string;
    manifestHash: string;
    manifestJson: string;
    expectedUnits: number;
    shards: Array<{
      id: string;
      ordinal: number;
      unitIds: string[];
      contextBeforeUnitIds: string[];
      contextAfterUnitIds: string[];
    }>;
  }): { run: CoverageRunRecord; shards: CoverageShardRecord[] };
  loadResumableCoverageRun(input: { manifestHash: string }): {
    run: CoverageRunRecord;
    shards: CoverageShardRecord[];
  } | null;
  getCoverageRun(input: { runId: string }): { run: CoverageRunRecord; shards: CoverageShardRecord[] } | null;
  markCoverageRunRunning(input: { runId: string }): CoverageRunRecord;
  markCoverageShardRunning(input: { shardId: string }): CoverageShardRecord;
  completeCoverageShard(input: { shardId: string; resultJson: string }): CoverageShardRecord;
  retryCoverageShard(input: { shardId: string }): CoverageShardRecord;
  failCoverageShard(input: { shardId: string }): CoverageShardRecord;
  cancelCoverageShards(input: { runId: string }): number;
  finalizeCoverageRun(input: {
    runId: string;
    status: CoverageRunStatus;
    processedUnits: number;
    failedUnits: number;
  }): CoverageRunRecord;
}

// ─────────────────────── CoverageLedger（§五十五） ───────────────────────

export interface CoverageLedgerSourceBreakdown {
  sourceId: string;
  fidelity: CoverageSourceFidelity;
  expectedUnits: number;
  processedUnits: number;
  failedUnits: number;
  skippedUnits: number;
}

export interface CoverageLedger {
  expectedPrimaryUnits: number;
  processedPrimaryUnits: number;
  failedPrimaryUnits: number;
  /** 取消 shard 中的 unit：未处理也未失败（§八十六）。 */
  skippedPrimaryUnits: number;
  /** needs_ocr / unavailable 的源（零 unit，只进 fidelity 摘要不进分母）。 */
  unavailableSources: Array<{ sourceId: string; fidelity: CoverageSourceFidelity }>;
  perSource: CoverageLedgerSourceBreakdown[];
  sourceFidelitySummary: Record<CoverageSourceFidelity, number>;
}

/**
 * Ledger 由 manifest + shard 终态计算（§五十五）：completed shard 的 primary
 * units 计 processed，failed 计 failed，cancelled 计 skipped；context units
 * 不出现在任何 primaryUnitIds 中，天然不进分母（§四十九）。per-source 明细
 * 经 unit → source 归属拆分。
 */
export function computeCoverageLedger(input: {
  manifest: CoverageManifest;
  shardStates: Array<{ unitIds: string[]; status: CoverageShardStatus }>;
}): CoverageLedger {
  const unitToSource = new Map<string, string>();
  for (const source of input.manifest.sources) {
    for (const unit of source.coverageUnits) unitToSource.set(unit.id, source.sourceId);
  }
  const breakdowns = new Map<string, CoverageLedgerSourceBreakdown>();
  for (const source of input.manifest.sources) {
    breakdowns.set(source.sourceId, {
      sourceId: source.sourceId,
      fidelity: source.fidelity,
      expectedUnits: source.coverageUnits.length,
      processedUnits: 0,
      failedUnits: 0,
      skippedUnits: 0,
    });
  }
  let processedPrimaryUnits = 0;
  let failedPrimaryUnits = 0;
  let skippedPrimaryUnits = 0;
  for (const shard of input.shardStates) {
    const field = shard.status === "completed"
      ? "processedUnits"
      : shard.status === "failed"
        ? "failedUnits"
        : shard.status === "cancelled"
          ? "skippedUnits"
          : null;
    if (field == null) continue; // pending/running（未终态，不记账）
    for (const unitId of shard.unitIds) {
      const sourceId = unitToSource.get(unitId);
      if (sourceId == null) {
        throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Shard references a unit outside the manifest");
      }
      const breakdown = breakdowns.get(sourceId)!;
      breakdown[field] += 1;
      if (field === "processedUnits") processedPrimaryUnits += 1;
      else if (field === "failedUnits") failedPrimaryUnits += 1;
      else skippedPrimaryUnits += 1;
    }
  }
  return {
    expectedPrimaryUnits: input.manifest.totalCoverageUnits,
    processedPrimaryUnits,
    failedPrimaryUnits,
    skippedPrimaryUnits,
    unavailableSources: input.manifest.sources
      .filter(source => source.fidelity === "needs_ocr" || source.fidelity === "unavailable")
      .map(source => ({ sourceId: source.sourceId, fidelity: source.fidelity })),
    perSource: [...breakdowns.values()],
    sourceFidelitySummary: input.manifest.sourceFidelitySummary,
  };
}

// ─────────────────────── CoverageGate（§五十六/§五十七） ───────────────────────

export interface CoverageGateResult {
  /** complete 当且仅当 processed==expected>0 且 failed==0 且 skipped==0（宁漏勿假）。 */
  coverageStatus: "complete" | "partial";
  /** processed/expected；expected=0 时为 0（无可处理文本，禁止虚标 100%）。 */
  textCoverageRatio: number;
  sourceFidelitySummary: Record<CoverageSourceFidelity, number>;
  /** 调用方（下一波 injector/prompt 组装）据此决定措辞；库层不生成 prompt 文本。 */
  allowedClaim: "full_text_processed" | "partial_only";
}

export function evaluateCoverageGate(ledger: CoverageLedger): CoverageGateResult {
  const complete = ledger.expectedPrimaryUnits > 0
    && ledger.processedPrimaryUnits === ledger.expectedPrimaryUnits
    && ledger.failedPrimaryUnits === 0
    && ledger.skippedPrimaryUnits === 0;
  return {
    coverageStatus: complete ? "complete" : "partial",
    textCoverageRatio: ledger.expectedPrimaryUnits > 0
      ? ledger.processedPrimaryUnits / ledger.expectedPrimaryUnits
      : 0,
    sourceFidelitySummary: ledger.sourceFidelitySummary,
    allowedClaim: complete ? "full_text_processed" : "partial_only",
  };
}

/**
 * fidelity 维度的原始资料声明判定（§五十七/§八十五）：text coverage 完整也不
 * 代表原始文件 100% 语义。存在 needs_ocr / unavailable / semantic_only 源时，
 * 禁止声称"已完整检查所有原始资料"（structural 有 DOM 定位，允许）。
 */
export function fidelityAllowsOriginalCoverageClaim(
  summary: Record<CoverageSourceFidelity, number>,
): boolean {
  return summary.needs_ocr === 0 && summary.unavailable === 0 && summary.semantic_only === 0;
}

// ─────────────────────── CoverageExecutor（§五十–§五十二） ───────────────────────

export interface CoverageFailedShardDiagnostics {
  shardId: string;
  attempts: number;
  /** `${KNOWLEDGE_COVERAGE_SHARD_FAILED}: ${原因}`，不泄露 prompt 原文。 */
  lastError: string;
}

export interface CoverageExecutionResult {
  runId: string;
  runStatus: CoverageRunStatus;
  manifestHash: string;
  /** 全部 completed shard 的结果（复用 + 新完成，按 shard ordinal 序）。 */
  shardResults: ShardResult[];
  ledger: CoverageLedger;
  gate: CoverageGateResult;
  evidence: AggregateEvidence;
  cancelled: boolean;
  failedShards: CoverageFailedShardDiagnostics[];
  /** 终态 partial（含全失败）时的稳定留痕 code（§一百零四）。 */
  reasonCode: string | null;
}

export interface CoverageExecutorInput {
  store: CoverageRunStore;
  manifest: CoverageManifest;
  question: string;
  planSummary: CoveragePlanSummary;
  workerModel: CoverageWorkerModel;
  /** 相关性优先序（§六十三）：shard id 序；未列出的 shard 按 ordinal 续后。只影响顺序不影响必达性。 */
  priorityOrder?: string[];
  concurrency?: number;
  /** 分片预算覆盖（缺省 COVERAGE_SHARD_TOKEN_BUDGET；恢复时须与既有 run 一致，否则显式报错）。 */
  shardTokenBudget?: number;
  signal?: AbortSignal;
  /**
   * shard 终态进度回调（done/total 含恢复复用的 completed shard）。runId 在
   * run 行创建/恢复后可得，随行携带供编排层广播进度事件；接受更少参数的
   * 回调（Phase 9a 形态 `(done, total) => void`）仍兼容。
   */
  onProgress?: (done: number, total: number, runId: string) => void;
}

function describeError(error: unknown): string {
  if (error instanceof KnowledgeError) return `${error.code}: ${error.message}`;
  if (error instanceof Error) return error.message;
  return String(error);
}

function unitsIndex(manifest: CoverageManifest): {
  unitsById: Map<string, CoverageUnit>;
  snapshotIdsBySource: Map<string, string>;
} {
  const unitsById = new Map<string, CoverageUnit>();
  const snapshotIdsBySource = new Map<string, string>();
  for (const source of manifest.sources) {
    snapshotIdsBySource.set(source.sourceId, source.contentSnapshotId);
    for (const unit of source.coverageUnits) unitsById.set(unit.id, unit);
  }
  return { unitsById, snapshotIdsBySource };
}

/** 恢复校验：持久化 shard 集必须与当前确定性分片完全一致（含 unit 序列）。 */
function assertShardPlanMatches(plans: CoverageShardPlan[], persisted: CoverageShardRecord[]): void {
  if (plans.length !== persisted.length) {
    throw new KnowledgeError(
      "KNOWLEDGE_STORAGE_INVALID",
      "Persisted coverage shards do not match the deterministic shard plan",
    );
  }
  const byOrdinal = new Map(persisted.map(shard => [shard.ordinal, shard]));
  for (const plan of plans) {
    const shard = byOrdinal.get(plan.ordinal);
    if (!shard || shard.id !== plan.shardId
      || JSON.stringify(shard.unitIds) !== JSON.stringify(plan.primaryUnitIds)) {
      throw new KnowledgeError(
        "KNOWLEDGE_STORAGE_INVALID",
        "Persisted coverage shards do not match the deterministic shard plan",
      );
    }
  }
}

/**
 * 单 shard 一次 attempt：worker 调用 + 严格解析；输出非法/为空触发一次纠错
 * 重试（correction 携带错误与上次输出），再失败该 attempt 判失败。
 * 返回解析后的 ShardResult；抛错时 error 为 attempt 失败原因。
 */
async function runShardAttempt(input: {
  workerModel: CoverageWorkerModel;
  prompt: string;
  shardId: string;
  primaryUnitIds: string[];
  knownBlocks: Map<string, { sourceId: string; snapshotId: string; parseArtifactId: string }>;
  abortPromise: Promise<never> | null;
}): Promise<ShardResult> {
  let firstError = "";
  let firstOutput = "";
  for (let attempt = 0; attempt <= SHARD_ATTEMPT_CORRECTION_RETRIES; attempt += 1) {
    const call = input.workerModel({
      prompt: input.prompt,
      ...(attempt === 1 ? { correction: { error: firstError, previousOutput: firstOutput } } : {}),
    });
    if (input.abortPromise) {
      // 取消竞速落败的在途调用不允许变成 unhandled rejection。
      void call.catch(() => {});
    }
    const raw = input.abortPromise
      ? await Promise.race([call, input.abortPromise])
      : await call;
    try {
      return parseShardResult({
        raw,
        shardId: input.shardId,
        primaryUnitIds: input.primaryUnitIds,
        knownBlocks: input.knownBlocks,
      });
    } catch (error) {
      if (attempt === 0) {
        firstError = describeError(error);
        firstOutput = typeof raw === "string" ? raw.slice(0, 2000) : "";
        continue;
      }
      throw error;
    }
  }
  // 循环内必然 return/throw；仅为类型完备。
  throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Shard attempt produced no result");
}

/**
 * 覆盖执行主入口（§五十）：
 * 1. 确定性分片（同 manifest 同边界，§四十八）；
 * 2. loadResumableCoverageRun：completed shard 直接复用 result_json（不重跑）、
 *    pending 续跑、running 已被 recovery policy 置回 pending（§六十五）；
 * 3. priorityOrder 排序（§六十三）后有界并发池执行（§八十七）；
 * 4. 终态后计算 ledger + gate 并回写 run 行。
 * 取消（§八十六）：pending→cancelled、running 中止（在途调用结果丢弃）、
 * completed 保留；run 终态 cancelled、gate partial。
 */
export async function executeCoverageRun(input: CoverageExecutorInput): Promise<CoverageExecutionResult> {
  const concurrency = input.concurrency ?? COVERAGE_EXECUTOR_DEFAULT_CONCURRENCY;
  if (!Number.isSafeInteger(concurrency) || concurrency <= 0) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "concurrency must be a positive integer");
  }
  const plans = planCoverageShards({
    manifest: input.manifest,
    ...(input.shardTokenBudget != null ? { shardTokenBudget: input.shardTokenBudget } : {}),
  });
  const { unitsById, snapshotIdsBySource } = unitsIndex(input.manifest);

  let runId: string;
  let persisted: CoverageShardRecord[];
  const resumable = input.store.loadResumableCoverageRun({ manifestHash: input.manifest.manifestHash });
  if (resumable) {
    assertShardPlanMatches(plans, resumable.shards);
    runId = resumable.run.id;
    persisted = resumable.shards;
  } else {
    const created = input.store.createCoverageRun({
      turnScopeId: input.manifest.turnScopeId,
      manifestHash: input.manifest.manifestHash,
      manifestJson: JSON.stringify(input.manifest),
      expectedUnits: input.manifest.totalCoverageUnits,
      shards: plans.map(plan => ({
        id: plan.shardId,
        ordinal: plan.ordinal,
        unitIds: plan.primaryUnitIds,
        contextBeforeUnitIds: plan.contextBeforeUnitIds,
        contextAfterUnitIds: plan.contextAfterUnitIds,
      })),
    });
    runId = created.run.id;
    persisted = created.shards;
  }
  input.store.markCoverageRunRunning({ runId });

  const shardByPlan = new Map(plans.map(plan => [plan.shardId, plan]));
  const completedResults = new Map<string, ShardResult>();
  const failedShards: CoverageFailedShardDiagnostics[] = [];
  const attemptCounts = new Map<string, number>();

  // completed shard：复用持久化 result_json（§六十五），重放校验防损坏行静默进结果。
  for (const shard of persisted) {
    attemptCounts.set(shard.id, shard.attemptCount);
    if (shard.status !== "completed") continue;
    const plan = shardByPlan.get(shard.id);
    if (!plan || shard.resultJson == null) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Completed coverage shard lacks a usable result");
    }
    const known = shardKnownBlocks({ shard: plan, unitsById, snapshotIdsBySource });
    completedResults.set(shard.id, parseShardResult({
      raw: shard.resultJson,
      shardId: shard.id,
      primaryUnitIds: shard.unitIds,
      knownBlocks: known,
    }));
  }

  // 优先级排序（§六十三）：priorityOrder 命中者先行，其余按 ordinal 续后；
  // 排序只影响顺序，所有可执行 shard 都会进入池（必达性不受影响）。
  const priorityRank = new Map((input.priorityOrder ?? []).map((shardId, index) => [shardId, index]));
  const executable = persisted
    .filter(shard => shard.status === "pending")
    .sort((left, right) => {
      const leftRank = priorityRank.get(left.id);
      const rightRank = priorityRank.get(right.id);
      if (leftRank != null && rightRank != null) return leftRank - rightRank;
      if (leftRank != null) return -1;
      if (rightRank != null) return 1;
      return left.ordinal - right.ordinal;
    });

  const total = persisted.length;
  let done = persisted.filter(shard => shard.status === "completed").length;
  let cancelled = false;
  // 熔断状态（2026-08-30 延迟加固）：零成功 + 终态 failed shard 数达阈值。
  // 见 COVERAGE_CIRCUIT_BREAK_FAILURES docstring。resume 场景下既有成功 shard
  // 已计入 succeededShards，因此续跑 run 永不熔断。
  let succeededShards = persisted.filter(shard => shard.status === "completed").length;
  let circuitOpen = false;
  const tripCircuit = () => {
    circuitOpen = true;
    cancelled = true;
    input.store.cancelCoverageShards({ runId });
  };
  const shouldTripCircuit = () => succeededShards === 0
    && failedShards.length >= COVERAGE_CIRCUIT_BREAK_FAILURES;

  const abortPromise: Promise<never> | null = input.signal
    ? new Promise<never>((_, reject) => {
      if (input.signal!.aborted) reject(abortError());
      else input.signal!.addEventListener("abort", () => reject(abortError()), { once: true });
    })
    : null;
  // 取消竞速的落败 promise 不允许变成 unhandled rejection。
  if (abortPromise) abortPromise.catch(() => {});

  let active = 0;
  let queueIndex = 0;
  await new Promise<void>((resolve) => {
    const settle = () => {
      if (active === 0 && (cancelled || queueIndex >= executable.length)) resolve();
    };
    const pump = () => {
      while (!cancelled && active < concurrency && queueIndex < executable.length) {
        const shard = executable[queueIndex];
        queueIndex += 1;
        active += 1;
        void runShard(shard).finally(() => {
          active -= 1;
          pump();
          settle();
        });
      }
      settle();
    };
    const runShard = async (shard: CoverageShardRecord) => {
      const plan = shardByPlan.get(shard.id)!;
      const known = shardKnownBlocks({ shard: plan, unitsById, snapshotIdsBySource });
      const prompt = buildShardWorkerPrompt({
        question: input.question,
        planSummary: input.planSummary,
        shard: plan,
        unitsById,
        snapshotIdsBySource,
      });
      let attempts = 0;
      for (;;) {
        if (cancelled) {
          // 尚未开跑的 pending shard 随整体取消翻 cancelled（§八十六）。
          input.store.cancelCoverageShards({ runId });
          return;
        }
        input.store.markCoverageShardRunning({ shardId: shard.id });
        attempts = (attemptCounts.get(shard.id) ?? 0) + 1;
        attemptCounts.set(shard.id, attempts);
        try {
          const result = await runShardAttempt({
            workerModel: input.workerModel,
            prompt,
            shardId: shard.id,
            primaryUnitIds: shard.unitIds,
            knownBlocks: known,
            abortPromise,
          });
          input.store.completeCoverageShard({
            shardId: shard.id,
            resultJson: JSON.stringify(result),
          });
          completedResults.set(shard.id, result);
          done += 1;
          succeededShards += 1;
          try {
            input.onProgress?.(done, total, runId);
          } catch {
            // 进度回调只作呈现，不允许影响执行本体。
          }
          return;
        } catch (error) {
          if (isAbortError(error) || cancelled) {
            cancelled = true;
            input.store.cancelCoverageShards({ runId });
            return;
          }
          if (attempts >= COVERAGE_SHARD_MAX_ATTEMPTS) {
            input.store.failCoverageShard({ shardId: shard.id });
            failedShards.push({
              shardId: shard.id,
              attempts,
              lastError: `${KNOWLEDGE_COVERAGE_SHARD_FAILED}: ${describeError(error)}`,
            });
            done += 1;
            try {
              input.onProgress?.(done, total, runId);
            } catch {
              // 同上。
            }
            // 熔断检查在终态 failed 落定后：零成功 + 终态失败达阈值 → 提前
            // 取消剩余 pending shard，剩余预算不再烧在注定失败的调用上。
            // reasonCode 见 run 返回处。
            if (shouldTripCircuit()) tripCircuit();
            return;
          }
          // bounded retry：回 pending，本 shard 的下一轮 attempt 续跑。
          input.store.retryCoverageShard({ shardId: shard.id });
        }
      }
    };
    pump();
  });
  if (cancelled || (input.signal?.aborted ?? false)) {
    cancelled = true;
    input.store.cancelCoverageShards({ runId });
  }

  const finalState = input.store.getCoverageRun({ runId })!;
  const ledger = computeCoverageLedger({
    manifest: input.manifest,
    shardStates: finalState.shards.map(shard => ({ unitIds: shard.unitIds, status: shard.status })),
  });
  const gate = evaluateCoverageGate(ledger);
  const anyCancelled = finalState.shards.some(shard => shard.status === "cancelled");
  const anyCompleted = finalState.shards.some(shard => shard.status === "completed");
  const runStatus: CoverageRunStatus = anyCancelled
    ? "cancelled"
    : gate.coverageStatus === "complete"
      ? "complete"
      : anyCompleted
        ? "partial"
        : finalState.shards.length === 0
          ? "partial"
          : "failed";
  input.store.finalizeCoverageRun({
    runId,
    status: runStatus,
    processedUnits: ledger.processedPrimaryUnits,
    failedUnits: ledger.failedPrimaryUnits,
  });

  const shardResults = finalState.shards
    .filter(shard => shard.status === "completed" && completedResults.has(shard.id))
    .sort((left, right) => left.ordinal - right.ordinal)
    .map(shard => completedResults.get(shard.id)!);
  return {
    runId,
    runStatus,
    manifestHash: input.manifest.manifestHash,
    shardResults,
    ledger,
    gate,
    evidence: aggregateShardEvidence(shardResults),
    cancelled,
    failedShards,
    // 熔断优先于通用 PARTIAL 留痕（取消语义 ≠ 用户取消，stats 侧由 injector
    // 按该 code 渲染专属措辞行）。
    reasonCode: runStatus === "complete"
      ? null
      : circuitOpen
        ? KNOWLEDGE_COVERAGE_CIRCUIT_BREAK
        : KNOWLEDGE_COVERAGE_PARTIAL,
  };
}

function abortError(): Error {
  const error = new Error("coverage run aborted");
  error.name = "CoverageAbortError";
  return error;
}

function isAbortError(error: unknown): boolean {
  return (error as { name?: unknown })?.name === "CoverageAbortError";
}

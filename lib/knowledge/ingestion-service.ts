import {
  knowledgeChunkerConfigId,
  resolveKnowledgeChunkerConfig,
} from "./chunker.ts";
import { isKnowledgeError, KnowledgeError } from "./errors.ts";
import { knowledgeChunkIndexVariantId } from "./knowledge-index-store.ts";
import type { KnowledgeEmbeddingResult, KnowledgeEmbedder } from "./knowledge-query-service.ts";
import { KnowledgeQueryService } from "./knowledge-query-service.ts";
import {
  KnowledgeStore,
  resolveEffectiveChunkTargetChars,
  resolveNotebookConfig,
  type ResolvedNotebookConfig,
} from "./knowledge-store.ts";
import type { IngestionJob, KnowledgeModelRef, KnowledgeParseArtifact } from "./types.ts";

/**
 * 摄入失败重试：attempt < 3 指数退避（30s/120s/600s），attempt 达到上限后标 failed
 * （显式终态；UI 手动重试走 store.requeueIngestionJob，attempt 归零、从失败 phase 续跑）。
 */
export const KNOWLEDGE_INGESTION_MAX_ATTEMPTS = 3;
export const KNOWLEDGE_INGESTION_RETRY_BACKOFF_MS = Object.freeze([30_000, 120_000, 600_000]);

/**
 * 有界并行 worker 池的默认并发上限（任务书 §十六，Phase 5）：跨 studio 的 job 级
 * 并行度。并行的是异步 IO（嵌入 HTTP、文件读）；better-sqlite3 同步写天然串行，
 * busy_timeout/WAL 语义不变。默认保守（3），可经 KnowledgeManagerOptions 配置。
 */
export const KNOWLEDGE_INGESTION_DEFAULT_CONCURRENCY = 3;

/**
 * Provider Semaphore 默认值（§十六 embedding 限流）：per (provider, model) 的
 * 并发上限与最小请求间隔。超限排队等待——绝不静默丢弃或降级。
 */
export const KNOWLEDGE_EMBEDDING_GATE_DEFAULT_MAX_CONCURRENT = 2;
export const KNOWLEDGE_EMBEDDING_GATE_DEFAULT_MIN_INTERVAL_MS = 250;

/** 取消 running job 后等待其收尾的上限（嵌入 abort 立即失败；上限仅防意外挂死）。 */
const CANCEL_SETTLE_TIMEOUT_MS = 10_000;

/**
 * 永久性错误（重试无意义，直接 failed 不消耗退避）：解析失败/源或笔记本被删/
 * 参数与存储校验/索引重建后仍 invalid。其余错误（嵌入 HTTP 4xx/5xx、网络、超时等
 * 被 query-service 包成 KNOWLEDGE_RETRIEVAL_UNAVAILABLE 的）一律按可重试处理。
 */
const PERMANENT_INGESTION_ERROR_CODES = new Set([
  "KNOWLEDGE_PARSE_FAILED",
  "KNOWLEDGE_NOT_FOUND",
  "KNOWLEDGE_CONFLICT",
  "KNOWLEDGE_INVALID_ARGUMENT",
  "KNOWLEDGE_INDEX_INVALID",
  "KNOWLEDGE_STORAGE_INVALID",
  "KNOWLEDGE_SCHEMA_NEWER",
]);

function isPermanentIngestionError(error: unknown): boolean {
  return isKnowledgeError(error) && PERMANENT_INGESTION_ERROR_CODES.has(error.code);
}

function describeIngestionError(error: unknown): string {
  const raw = error instanceof Error ? error.message : String(error);
  const prefixed = isKnowledgeError(error) ? `${error.code}: ${raw}` : raw;
  return prefixed.slice(0, 512);
}

function sleep(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms));
}

export interface KnowledgeIngestionEmbedRequest {
  modelRef: KnowledgeModelRef;
  runId: string;
  texts: string[];
  signal?: AbortSignal;
}

/** embedding provider 限流配置（§十六）：per (provider, model) 并发上限 + 最小请求间隔。 */
export interface KnowledgeEmbeddingGateLimits {
  /** 同一 (provider, model) 同时在飞的嵌入请求数上限；默认 2。 */
  maxConcurrent?: number;
  /** 同一 (provider, model) 两次请求派发的最小间隔（ms）；默认 250。 */
  minRequestIntervalMs?: number;
  /** 按 provider 覆盖最小间隔（provider id → ms）；缺省回退全局默认。 */
  providerMinIntervals?: Record<string, number> | null;
}

interface EmbeddingGateSlot {
  active: number;
  queue: Array<() => void>;
  lastDispatchAt: number;
  dispatchTimer: ReturnType<typeof setTimeout> | null;
}

/**
 * Provider Semaphore（任务书 §十六）：per (provider, model) 的并发上限 + 最小请求
 * 间隔。acquire 超限时排队等待（禁止把限流实现成静默丢弃）；释放后按间隔节流派发
 * 下一个等待者。stop()/close() 路径 dispose()：等待中的请求以显式错误拒绝，交给
 * job 失败分类处理（不吞、不挂死事件循环）。
 */
export class KnowledgeEmbeddingProviderGate {
  private readonly maxConcurrent: number;
  private readonly minIntervalMs: number;
  private readonly providerMinIntervals: Record<string, number>;
  private readonly slots = new Map<string, EmbeddingGateSlot>();
  private disposed = false;

  constructor(limits: KnowledgeEmbeddingGateLimits = {}) {
    this.maxConcurrent = limits.maxConcurrent ?? KNOWLEDGE_EMBEDDING_GATE_DEFAULT_MAX_CONCURRENT;
    this.minIntervalMs = limits.minRequestIntervalMs ?? KNOWLEDGE_EMBEDDING_GATE_DEFAULT_MIN_INTERVAL_MS;
    this.providerMinIntervals = limits.providerMinIntervals ?? {};
    if (!Number.isSafeInteger(this.maxConcurrent) || this.maxConcurrent < 1) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "embedding gate maxConcurrent must be a positive integer");
    }
    if (!Number.isSafeInteger(this.minIntervalMs) || this.minIntervalMs < 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "embedding gate minRequestIntervalMs must be a non-negative integer");
    }
  }

  /** 诊断面：各 (provider, model) 的在飞/排队计数（测试与可观测性用）。 */
  stats(): Array<{ key: string; active: number; queued: number }> {
    return [...this.slots.entries()].map(([key, slot]) => ({
      key,
      active: slot.active,
      queued: slot.queue.length,
    }));
  }

  /** 在限流窗口内执行一次嵌入调用；排队等待不设静默上限。 */
  async run<T>(key: string, task: () => Promise<T>): Promise<T> {
    await this.acquire(key);
    try {
      return await task();
    } finally {
      this.release(key);
    }
  }

  /** 停机路径：拒绝全部排队等待者并清空节流计时器（已派发的调用自然完成）。 */
  dispose() {
    this.disposed = true;
    for (const slot of this.slots.values()) {
      if (slot.dispatchTimer) {
        clearTimeout(slot.dispatchTimer);
        slot.dispatchTimer = null;
      }
      const waiters = slot.queue.splice(0);
      for (const waiter of waiters) waiter();
    }
  }

  private intervalFor(key: string): number {
    const provider = key.split("/")[0];
    return this.providerMinIntervals[provider] ?? this.minIntervalMs;
  }

  private acquire(key: string): Promise<void> {
    if (this.disposed) {
      return Promise.reject(new KnowledgeError(
        "KNOWLEDGE_RETRIEVAL_UNAVAILABLE",
        "Knowledge embedding provider gate is closed",
      ));
    }
    let slot = this.slots.get(key);
    if (!slot) {
      slot = { active: 0, queue: [], lastDispatchAt: 0, dispatchTimer: null };
      this.slots.set(key, slot);
    }
    if (slot.active < this.maxConcurrent && this.intervalElapsed(slot, key)) {
      // 快路径：有空位且已过节流间隔，直接占位放行（dispatch 只服务排队者）。
      slot.active += 1;
      slot.lastDispatchAt = Date.now();
      return Promise.resolve();
    }
    return new Promise<void>((resolve, reject) => {
      slot!.queue.push(() => {
        if (this.disposed) {
          reject(new KnowledgeError(
            "KNOWLEDGE_RETRIEVAL_UNAVAILABLE",
            "Knowledge embedding provider gate is closed",
          ));
          return;
        }
        resolve();
      });
      this.scheduleDispatch(slot!, key);
    });
  }

  private release(key: string) {
    const slot = this.slots.get(key);
    if (!slot) return;
    slot.active = Math.max(0, slot.active - 1);
    this.scheduleDispatch(slot, key);
  }

  private intervalElapsed(slot: EmbeddingGateSlot, key: string): boolean {
    return Date.now() - slot.lastDispatchAt >= this.intervalFor(key);
  }

  private scheduleDispatch(slot: EmbeddingGateSlot, key: string) {
    if (slot.dispatchTimer || slot.queue.length === 0 || slot.active >= this.maxConcurrent) return;
    const wait = Math.max(0, slot.lastDispatchAt + this.intervalFor(key) - Date.now());
    slot.dispatchTimer = setTimeout(() => {
      slot.dispatchTimer = null;
      this.dispatch(slot);
    }, wait);
  }

  private dispatch(slot: EmbeddingGateSlot) {
    if (this.disposed || slot.active >= this.maxConcurrent || slot.queue.length === 0) return;
    const next = slot.queue.shift();
    slot.active += 1;
    slot.lastDispatchAt = Date.now();
    next?.();
  }
}

export interface KnowledgeIngestionServiceDeps {
  store: KnowledgeStore;
  queryService: KnowledgeQueryService;
  /** 绑定到 KnowledgeManager.parseSource（幂等：已有 ready/needs_ocr 产物直接返回）。 */
  parseSource: (input: { studioId: unknown; sourceId: unknown }) => Promise<KnowledgeParseArtifact>;
  /**
   * 按显式模型引用执行嵌入（engine 用现有 ModelOperationResolver/EmbeddingClient 接线，
   * 与查询侧懒构建嵌入共用同一套调用方式）。引用不可解析时返回 null —— 调用方落
   * pending_embedding（显式终态），不做模型替换之类的静默降级。
   */
  embedTextsForModel?: ((request: KnowledgeIngestionEmbedRequest) => Promise<KnowledgeEmbeddingResult | null>) | null;
  /** 同步判定某嵌入模型引用当前是否可解析（模型存在/支持 embedding/凭证就绪）。 */
  canEmbedWithModel?: ((modelRef: KnowledgeModelRef) => boolean) | null;
  /** 查嵌入模型上下文窗口（token 数）；自动分块尺寸 = 窗口 × 80%。查不到回退内置兜底。 */
  getEmbeddingModelContextWindow?: ((modelRef: KnowledgeModelRef) => number | null) | null;
  /** worker 池并发上限（默认 3）；key 冲突的 job 无论如何都会串行。 */
  concurrency?: number;
  /** embedding provider 限流（§十六 Provider Semaphore）；缺省用保守默认值。 */
  embeddingGate?: KnowledgeEmbeddingGateLimits | null;
  now?: () => string;
  pollIntervalMs?: number;
  log?: (message: string) => void;
}

interface ActiveIngestionJob {
  jobId: string;
  sourceId: string;
  lockKeys: ReadonlySet<string>;
  abort: AbortController;
  settled: Promise<void>;
}

/**
 * 摄入管线：持久化 job 表（ingestion_jobs）+ 有界并行 worker 池（Phase 5 §十六）。
 * phase 链 parse → chunk → fts_index → embed → done，每步幂等：
 * - parse 复用 parseSource 的解析身份判断（已有 ready/needs_ocr 产物直接返回）；
 * - chunk+fts_index 以 ChunkIndexVariant (parseArtifactId, chunkProfileHash) 为锚，
 *   复用 ensureChunkIndexVariant + fingerprint 判断（不匹配只重建该变体，不覆盖
 *   同 artifact 其他分块配置的变体）；
 * - embed 复用 vectorIndex.hasArtifact（VectorIndexVariant = civ + 模型身份，
 *   chunkFingerprint 命中即跳过）；Phase 3 起按 64 块/批 checkpoint 持久化
 *   （begin/upsert/complete 协议），中断/失败后重试只补缺失 chunk，
 *   已落库向量绝不删除、绝不重嵌。
 *
 * 并发模型（§十六 keyed locking）：job 认领时计算锁键集合，同键互斥、异键并行——
 * - parse 相位（artifactId 未知）：`source:<sourceId>`（contentSnapshotId+parser 身份
 *   的保守超集：同源 job 串行，解析身份天然幂等）；
 * - chunk/embed 相位（artifactId 已知）：`civ:<chunkIndexVariantId>`（= parseArtifactId
 *   + chunkProfileHash；vector 锁 (civ, modelKey) 的保守超集——同一变体的任何构建
 *   串行，不同 artifact/profile/model 的 job 并行）。
 * 计算失败（笔记本已删/工件无 blocks）回退源级粗锁，宁可保守串行不冒险并行。
 *
 * Provider Semaphore（§十六）：embed 相位的每次嵌入调用经 per (provider, model)
 * 信号量限流（并发上限 + 最小请求间隔），超限排队等待不丢弃；job 内批次串行不变。
 *
 * Phase 2（§十一/§十二）：本服务是唯一建库入口。查询路径不再懒构建——
 * Notebook → RetrievalProfile 的惰性建绑也前移到本服务（enqueue 时 artifact 已知
 * 即绑；否则 chunk 相位绑），查询发现变体缺失/未就绪时经 requestVariantBuild
 * 幂等回到本队列。
 */
export class KnowledgeIngestionService {
  private readonly deps: KnowledgeIngestionServiceDeps;
  private readonly now: () => string;
  private readonly pollIntervalMs: number;
  private readonly log: (message: string) => void;
  private readonly concurrency: number;
  private readonly embeddingGate: KnowledgeEmbeddingProviderGate;
  private stopped = false;
  private loopPromise: Promise<void> | null = null;
  private drainPromise: Promise<number> | null = null;
  /** 在跑 job 的锁键登记：key → 持有该键的 job id 集合（同键互斥的判据）。 */
  private readonly keyOwners = new Map<string, Set<string>>();
  private readonly activeJobs = new Map<string, ActiveIngestionJob>();
  /** 排空中等待「任一在跑 job 收尾」的 worker 唤醒队列。 */
  private settleWaiters: Array<() => void> = [];
  private waiter: (() => void) | null = null;
  private waiterTimer: ReturnType<typeof setTimeout> | null = null;
  private wakeRequested = false;

  constructor(deps: KnowledgeIngestionServiceDeps) {
    if (!deps?.store || !deps?.queryService || typeof deps?.parseSource !== "function") {
      throw new KnowledgeError(
        "KNOWLEDGE_INVALID_ARGUMENT",
        "KnowledgeIngestionService requires store, queryService and parseSource",
      );
    }
    this.deps = deps;
    this.now = deps.now || (() => new Date().toISOString());
    this.pollIntervalMs = deps.pollIntervalMs ?? 1000;
    this.log = deps.log || (() => {});
    this.concurrency = deps.concurrency ?? KNOWLEDGE_INGESTION_DEFAULT_CONCURRENCY;
    if (!Number.isSafeInteger(this.concurrency) || this.concurrency < 1) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "ingestion concurrency must be a positive integer");
    }
    this.embeddingGate = new KnowledgeEmbeddingProviderGate(deps.embeddingGate ?? {});
  }

  /** 启动恢复：running 残留（进程中断）重置回 queued 续跑。幂等，可单独调用。 */
  recoverInterruptedJobs(): number {
    const recovered = this.deps.store.requeueRunningIngestionJobs();
    if (recovered > 0) {
      this.log(`knowledge ingestion: recovered ${recovered} interrupted job(s)`);
    }
    return recovered;
  }

  /** 启动后台 worker 池循环（engine init 调用一次）。重复调用是 no-op。 */
  start() {
    if (this.loopPromise) return;
    this.stopped = false;
    this.recoverInterruptedJobs();
    this.loopPromise = this.loop().catch((error) => {
      // 循环本身不应退出；到这里说明出现了未被 job 级 catch 覆盖的意外，显式留痕。
      this.log(`knowledge ingestion: queue loop exited unexpectedly: ${describeIngestionError(error)}`);
    });
    this.wake();
  }

  /**
   * 优雅停池：置停止位、abort 全部在跑 job 的嵌入、拒绝 provider gate 排队者、
   * 唤醒等待中的 worker/循环立即退出。被中断的 running job 不消耗 attempt：
   * _processJob 的停止路径 best-effort 置回 queued；若库已随 close() 关闭则留
   * running 残留，由下次 start() 的恢复接管。
   */
  stop() {
    this.stopped = true;
    for (const entry of this.activeJobs.values()) entry.abort.abort();
    this.embeddingGate.dispose();
    this.notifyJobSettled();
    this.wake();
    this.loopPromise = null; // 已 catch 包裹，分离即可；close() 保持同步语义。
  }

  /** 唤醒队列（enqueue/模型就绪后置回 queued 时调用）。无等待者时记下唤醒位，避免丢失唤醒。 */
  wake() {
    this.wakeRequested = true;
    const waiter = this.waiter;
    this.waiter = null;
    if (this.waiterTimer) {
      clearTimeout(this.waiterTimer);
      this.waiterTimer = null;
    }
    waiter?.();
  }

  /**
   * 排空当前全部到期 queued job 到没有可认领者且在跑 job 全部收尾，返回处理数。
   * 后台循环与测试共用同一入口；并发调用共享同一次排空。claim 阶段的 key 冲突
   * 挑选 + claimIngestionJobById 原子认领保证 worker 池并发安全。
   */
  async drainQueue(): Promise<number> {
    if (this.drainPromise) return this.drainPromise;
    let processed = 0;
    const drain = (async () => {
      const workers = Array.from(
        { length: this.concurrency },
        () => this.drainWorker(() => { processed += 1; }),
      );
      await Promise.all(workers);
      return processed;
    })();
    this.drainPromise = drain;
    try {
      return await drain;
    } finally {
      this.drainPromise = null;
    }
  }

  /**
   * 入队单源摄入（同 notebook+source 的活跃 job 由 store 层去重 + v12 部分唯一索引兜底）。
   * chunkerConfigId 记录触发方笔记本的分块配置：parse 产物已知时按真实 blocks 计算，
   * 未知（parse 失败/未跑）时以 fixed 策略占位——执行时按真实 blocks 重算，
   * 该列只是"触发摄入时的配置"记录。
   * artifact 已知时顺带完成 Notebook → RetrievalProfile 惰性建绑（Phase 2：
   * 查询侧已纯只读，建绑职责在摄入侧）；建绑失败不阻断入队（chunk 相位会重绑）。
   */
  enqueueSourceIngestion(input: {
    studioId: unknown;
    notebookId: unknown;
    sourceId: unknown;
    artifactId?: unknown;
  }): IngestionJob {
    const config = this.resolveConfig(input.studioId, input.notebookId);
    const artifactId = input.artifactId ?? null;
    let configId: string;
    if (artifactId != null) {
      const blocks = this.deps.store.listArtifactBlocks({ studioId: input.studioId, parseArtifactId: artifactId });
      const resolved = resolveKnowledgeChunkerConfig(blocks, { targetChars: config.chunkTargetChars });
      configId = resolved.configId;
      this.bindNotebookRetrievalProfile(input.studioId, input.notebookId, resolved.strategy);
    } else {
      configId = knowledgeChunkerConfigId("fixed", config.chunkTargetChars);
    }
    const job = this.deps.store.enqueueIngestionJob({
      studioId: input.studioId,
      notebookId: input.notebookId,
      sourceId: input.sourceId,
      artifactId,
      chunkerConfigId: configId,
    });
    this.wake();
    return job;
  }

  /**
   * 查询侧后台补齐入口（§十二「后台继续 Vector ingestion」）：查询发现索引变体
   * 缺失/未就绪时调用，幂等入队（store 层活跃 job 去重，重复查询不重复排队）。
   * 去重命中 pending_embedding 且嵌入当前可解析（查询刚用该模型成功嵌入）时
   * 把该 job 置回 queued 立即补跑，不干等下一次模型就绪信号。
   */
  requestVariantBuild(input: {
    studioId: unknown;
    notebookId: unknown;
    sourceId: unknown;
    artifactId?: unknown;
  }): IngestionJob {
    const job = this.enqueueSourceIngestion(input);
    if (job.status !== "pending_embedding") return job;
    let resolvable = false;
    try {
      resolvable = this.embeddingResolvable(
        this.resolveConfig(input.studioId, input.notebookId).embeddingModelRef,
      );
    } catch {
      // 笔记本在入队后被删除等竞态：保持 pending_embedding，等模型就绪信号兜底。
      return job;
    }
    if (resolvable
      && this.deps.store.requeuePendingEmbeddingIngestionJob({ studioId: input.studioId, jobId: job.id })) {
      this.wake();
    }
    return job;
  }

  /**
   * 显式取消一个源的全部活跃 job（Phase 5 §十九 delete wins 载体）：
   * queued/pending_embedding → failed + cancelled_at（store 事务）；
   * running job 由本进程 abort 其嵌入调用并等待收尾（上限 CANCEL_SETTLE_TIMEOUT_MS，
   * 超时显式留痕后继续——相位边界检查保证 straggler 不会再推进）。
   * 调用方须已将源标记 deleted（标记后一切新 ensure 显式失败，取消与清理不会复活）。
   */
  async cancelSourceJobs(input: { studioId: unknown; sourceId: unknown }): Promise<{ cancelledJobIds: string[] }> {
    const sourceId = String(input.sourceId);
    const cancelledJobIds = this.deps.store.cancelSourceIngestionJobs({
      sourceId,
      reason: "KNOWLEDGE_SOURCE_DELETED: ingestion cancelled because the source was deleted",
    });
    if (cancelledJobIds.length > 0) {
      this.log(`knowledge ingestion: cancelled ${cancelledJobIds.length} job(s) for source ${sourceId} (source deleted)`);
    }
    const inFlight = [...this.activeJobs.values()].filter(entry => entry.sourceId === sourceId);
    if (inFlight.length > 0) {
      for (const entry of inFlight) entry.abort.abort();
      await Promise.race([
        Promise.allSettled(inFlight.map(entry => entry.settled)),
        sleep(CANCEL_SETTLE_TIMEOUT_MS).then(() => {
          this.log(
            `knowledge ingestion: timed out waiting for ${inFlight.length} cancelled job(s) of source ${sourceId} to settle; proceeding (phase checks will stop them)`,
          );
        }),
      ]);
    }
    this.wake();
    return { cancelledJobIds };
  }

  /** Notebook → RetrievalProfile 惰性建绑（Phase 2 起由摄入侧承担；查询只读）。 */
  private bindNotebookRetrievalProfile(studioId: unknown, notebookId: unknown, strategy: unknown) {
    this.deps.store.resolveNotebookRetrievalProfile({
      studioId,
      notebookId,
      strategy,
      getEmbeddingModelContextWindow: this.deps.getEmbeddingModelContextWindow,
    });
  }

  /**
   * 笔记本配置变更后的重建：该笔记本全部活跃源重新入队。
   * 身份语义（v9 起）：worker 按当前笔记本配置解析 chunkProfileHash 并 ensure
   * 对应 ChunkIndexVariant 后台 build——变体天然共存，本操作不覆盖其他 profile
   * 的变体；无需重建的部分由相位幂等自然跳过（fingerprint/hasArtifact 命中）。
   */
  enqueueNotebookRebuild(input: { studioId: unknown; notebookId: unknown }): IngestionJob[] {
    const entries = this.deps.store.listNotebookSources({
      studioId: input.studioId,
      notebookId: input.notebookId,
    });
    const jobs: IngestionJob[] = [];
    for (const entry of entries) {
      // 仅 ready 产物绑定 artifactId；其余让 worker 从 parse 相位起步（幂等）。
      const artifactId = entry.parseArtifact?.status === "ready" ? entry.parseArtifact.id : null;
      jobs.push(this.enqueueSourceIngestion({
        studioId: input.studioId,
        notebookId: input.notebookId,
        sourceId: entry.source.id,
        artifactId,
      }));
    }
    return jobs;
  }

  /**
   * 模型就绪信号（模型 init/refresh、provider 变更、嵌入偏好变更后由 engine 调用）：
   * 存在可解析的 pending_embedding job 时把 pending_embedding 批量置回 queued 并唤醒队列。
   * 批量置回里仍不可解析的 job 会经幂等相位快速回到 pending_embedding（显式终态），
   * 代价是一次空转，换来不引入按笔记本部分置回的额外 store 方法。返回置回数量。
   */
  onModelConfigMayHaveChanged(): number {
    const pending = this.deps.store.listPendingEmbeddingIngestionJobs();
    if (pending.length === 0) return 0;
    const anyResolvable = pending.some((job) => {
      try {
        return this.embeddingResolvable(this.resolveConfig(job.studioId, job.notebookId).embeddingModelRef);
      } catch {
        // 笔记本在 job 入队后被删除等：视为不可解析，跳过（job 残留不挡其他笔记本补跑）。
        return false;
      }
    });
    if (!anyResolvable) return 0;
    const requeued = this.deps.store.requeuePendingEmbeddingIngestionJobs();
    if (requeued > 0) this.wake();
    return requeued;
  }

  /**
   * 笔记本配置解析（v8 起）：仅笔记本列，无全局偏好级。chunkTargetChars 为
   * NULL（新默认）时按嵌入模型上下文窗口 ×80% 自动派生（1 token = 1 字符的
   * 最保守口径，任何语言不超嵌入窗口）；遗留显式列值仍生效。
   */
  private resolveConfig(
    studioId: unknown,
    notebookId: unknown,
  ): Omit<ResolvedNotebookConfig, "chunkTargetChars"> & { chunkTargetChars: number } {
    const config = this.deps.store.getNotebookConfig({ studioId, notebookId });
    const resolved = resolveNotebookConfig(config);
    const chunkTargetChars = resolveEffectiveChunkTargetChars(
      resolved,
      this.deps.getEmbeddingModelContextWindow,
    );
    return { ...resolved, chunkTargetChars };
  }

  /** 嵌入可解析性：引用存在 + 嵌入回调已接线 +（可选）同步判定通过。 */
  private embeddingResolvable(ref: KnowledgeModelRef | null): boolean {
    if (!ref || !this.deps.embedTextsForModel) return false;
    return this.deps.canEmbedWithModel ? this.deps.canEmbedWithModel(ref) : true;
  }

  /**
   * embed 相位的 ChunkIndexVariant 身份（chunkProfileHash = chunkerConfigId 同源值）：
   * 与 chunk 相位同一解析链（blocks → resolveKnowledgeChunkerConfig → configId）。
   */
  private resolveChunkProfileHash(studioId: string, parseArtifactId: string, chunkTargetChars: number): string {
    const blocks = this.deps.store.listArtifactBlocks({ studioId, parseArtifactId });
    return resolveKnowledgeChunkerConfig(blocks, { targetChars: chunkTargetChars }).configId;
  }

  /**
   * job 的锁键集合（§十六 keyed locking，见类头注释）。artifactId 未知（parse 相位）
   * 或配置/blocks 不可解析时取源级粗锁——保守串行，不冒险并行。
   */
  private computeJobLockKeys(studioId: string, job: IngestionJob): string[] {
    if (!job.artifactId) return [`source:${job.sourceId}`];
    try {
      const config = this.resolveConfig(studioId, job.notebookId);
      const blocks = this.deps.store.listArtifactBlocks({ studioId, parseArtifactId: job.artifactId });
      if (blocks.length === 0) return [`source:${job.sourceId}`];
      const hash = resolveKnowledgeChunkerConfig(blocks, { targetChars: config.chunkTargetChars }).configId;
      return [`civ:${knowledgeChunkIndexVariantId(job.artifactId, hash)}`];
    } catch {
      return [`source:${job.sourceId}`];
    }
  }

  /**
   * 认领下一个与在跑 job 无锁键冲突的到期 queued job（§十六）：按 created_at 序扫描
   * 候选，跳过与在跑 job 同键者；选定后 claimIngestionJobById 原子认领（与并行
   * worker 竞争失败则重扫）。无可认领者返回 null。
   */
  private claimNextCompatibleJob(): { job: IngestionJob; studioId: string; lockKeys: string[] } | null {
    if (this.stopped) return null;
    for (let attempt = 0; attempt < 4; attempt += 1) {
      const candidates = this.deps.store.listClaimableIngestionJobs({ limit: 32 });
      let picked: { job: IngestionJob & { studioId: string }; lockKeys: string[] } | null = null;
      for (const candidate of candidates) {
        const lockKeys = this.computeJobLockKeys(candidate.studioId, candidate);
        if (lockKeys.every(key => !this.keyOwners.has(key))) {
          picked = { job: candidate, lockKeys };
          break;
        }
      }
      if (!picked) return null;
      const claimed = this.deps.store.claimIngestionJobById({ jobId: picked.job.id });
      if (claimed) {
        return { job: claimed, studioId: picked.job.studioId, lockKeys: picked.lockKeys };
      }
      // 另一 worker 抢先认领了同一条：重扫候选。
    }
    return null;
  }

  /** 单个排空 worker：认领→处理→循环；无可认领且无在跑 job 时退出，有在跑则等其收尾再试。 */
  private async drainWorker(onProcessed: () => void): Promise<void> {
    while (!this.stopped) {
      const claimed = this.claimNextCompatibleJob();
      if (claimed) {
        onProcessed();
        await this.processJob(claimed.job, claimed.studioId, claimed.lockKeys);
        continue;
      }
      if (this.activeJobs.size === 0) return;
      await this.waitForJobSettle();
    }
  }

  private waitForJobSettle(): Promise<void> {
    return new Promise(resolve => {
      this.settleWaiters.push(resolve);
    });
  }

  private notifyJobSettled() {
    const waiters = this.settleWaiters.splice(0);
    for (const waiter of waiters) waiter();
  }

  private async loop() {
    while (!this.stopped) {
      let processed = 0;
      try {
        processed = await this.drainQueue();
      } catch (error) {
        // drainQueue 内的 job 错误已被 processJob 各自捕获；到这里是队列级意外，
        // 显式记录后继续循环（不吞错、不退出）。
        this.log(`knowledge ingestion: drain failed: ${describeIngestionError(error)}`);
      }
      if (this.stopped || processed > 0) continue;
      await this.waitForWake(this.pollIntervalMs);
    }
  }

  /** 退避到期由周期轮询兜底；enqueue/模型就绪通过 wake() 立即唤醒。 */
  private waitForWake(ms: number): Promise<void> {
    if (this.wakeRequested) {
      // 上次 drain 与本次等待之间已有唤醒（如 enqueue）：不再等待，立即下一轮。
      this.wakeRequested = false;
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      this.waiterTimer = setTimeout(() => {
        this.waiterTimer = null;
        this.waiter = null;
        this.wakeRequested = false;
        resolve();
      }, ms);
      this.waiter = () => {
        this.wakeRequested = false;
        resolve();
      };
    });
  }

  /**
   * 单 job 状态机：按 job.phase 续跑（每步幂等），逐步推进直到 done 或显式终态。
   * 失败分类：永久性错误直接 failed；其余 attempt < 3 退避重试，超限 failed。
   * 相位边界复查 job 状态（deleteSource 的取消在任意相位到达时立即停步，§十九）。
   */
  private async processJob(job: IngestionJob, studioId: string, lockKeys: string[]) {
    const owner = this.deps.store.getIngestionJobOwner({ jobId: job.id });
    if (!owner) {
      // notebook 已被物理删除的孤儿 job：无任何显式终态可落（store 方法都要 studio 归属），
      // 显式留痕后丢弃；外键约束下正常不会发生。
      this.log(`knowledge ingestion: dropping orphan job ${job.id}`);
      return;
    }
    const abort = new AbortController();
    let resolveSettled: () => void = () => {};
    const entry: ActiveIngestionJob = {
      jobId: job.id,
      sourceId: job.sourceId,
      lockKeys: new Set(lockKeys),
      abort,
      settled: new Promise<void>(resolve => { resolveSettled = resolve; }),
    };
    this.activeJobs.set(job.id, entry);
    for (const key of lockKeys) {
      let owners = this.keyOwners.get(key);
      if (!owners) {
        owners = new Set();
        this.keyOwners.set(key, owners);
      }
      owners.add(job.id);
    }
    try {
      let current = job;
      if (current.phase === "parse") {
        const artifact = await this.deps.parseSource({ studioId, sourceId: current.sourceId });
        if (artifact.status !== "ready") {
          // needs_ocr：解析成功但无可检索文本，重试无意义 → 显式失败终态。
          throw new KnowledgeError(
            "KNOWLEDGE_PARSE_FAILED",
            "Knowledge source produced no searchable text",
            { reason: artifact.status },
          );
        }
        current = this.deps.store.updateIngestionJobPhase({
          studioId,
          jobId: current.id,
          phase: "chunk",
          artifactId: artifact.id,
        });
      }
      // chunk 与 fts_index 在同一次幂等替换中原子完成（replaceArtifactChunks 单事务），
      // 因此 phase 从 chunk/fts_index 一步推进到 embed。chunkProfileHash（= chunker_config_id
      // 同源值）在各相位按同一解析链重算（blocks → chunker 配置），贯穿 chunk/embed 相位，
      // 保证 embed 锚定的 ChunkIndexVariant 与 chunk 相位建出的是同一个。
      if (current.phase === "chunk" || current.phase === "fts_index") {
        if (!current.artifactId) {
          throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Ingestion job has no parse artifact");
        }
        if (this.jobLeftRunning(studioId, current.id, "chunk")) return;
        const config = this.resolveConfig(studioId, current.notebookId);
        // Notebook → RetrievalProfile 惰性建绑（Phase 2：查询侧只读，建绑在摄入侧）；
        // 策略随 artifact 内容派发，与 chunk 相位同一解析链。
        const blocks = this.deps.store.listArtifactBlocks({ studioId, parseArtifactId: current.artifactId });
        const strategy = resolveKnowledgeChunkerConfig(blocks, { targetChars: config.chunkTargetChars }).strategy;
        this.bindNotebookRetrievalProfile(studioId, current.notebookId, strategy);
        this.deps.queryService.indexArtifactForIngestion(studioId, current.artifactId, {
          targetChars: config.chunkTargetChars,
        });
        current = this.deps.store.updateIngestionJobPhase({
          studioId,
          jobId: current.id,
          phase: "embed",
        });
      }
      if (current.phase === "embed") {
        if (!current.artifactId) {
          throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Ingestion job has no parse artifact");
        }
        if (this.jobLeftRunning(studioId, current.id, "embed")) return;
        const config = this.resolveConfig(studioId, current.notebookId);
        const modelRef = config.embeddingModelRef;
        if (!this.embeddingResolvable(modelRef)) {
          // 显式终态（非失败）：FTS 已可查，等模型就绪信号补跑（禁静默降级红线）。
          this.deps.store.markIngestionJobPendingEmbedding({ studioId, jobId: current.id });
          return;
        }
        const chunkProfileHash = this.resolveChunkProfileHash(studioId, current.artifactId, config.chunkTargetChars);
        // Provider Semaphore（§十六）：per (provider, model) 并发上限 + 最小请求间隔；
        // 超限排队等待（不丢弃），job 内批次保持串行。
        const gateKey = `${modelRef!.provider}/${modelRef!.id}`;
        const embedTexts: KnowledgeEmbedder = (request) => this.embeddingGate.run(gateKey, () =>
          this.deps.embedTextsForModel!({
            ...request,
            modelRef: modelRef!,
          }));
        const outcome = await this.deps.queryService.embedArtifactForIngestion({
          runId: current.id,
          parseArtifactId: current.artifactId,
          chunkProfileHash,
          embedTexts,
          signal: abort.signal,
          // 每批嵌入成功并持久化后落进度（64 块/批 ≈ 每 708 块 12 次 UPDATE）。
          // 写失败让错误沿嵌入路径抛出，走既有 handleJobFailure 分类，不吞错。
          onProgress: (done, total) => {
            this.deps.store.updateIngestionJobProgress({ studioId, jobId: current.id, done, total });
          },
        });
        // 成本观测（§七十四）：chunk 级账目落 ingestion_jobs.embedding_stats
        // （后端可查询）+ 运行日志一行；请求级 token/次数由 usageContext 台账承担。
        this.deps.store.recordIngestionJobEmbeddingStats({
          studioId,
          jobId: current.id,
          stats: outcome.embeddingStats,
        });
        const stats = outcome.embeddingStats;
        const model = stats.model ? `${stats.model.provider}/${stats.model.modelId}` : "unavailable";
        this.log(
          `knowledge ingestion: embed ${outcome.status} job ${current.id}: `
          + `newly=${stats.chunksNewlyEmbedded} resumed=${stats.chunksResumedFromCheckpoint} `
          + `reused=${stats.chunksReusedFromReadyVariant} requests=${stats.requestCount} model=${model}`,
        );
        // 显式留痕（禁静默）：指纹/维度漂移重建与断点期间换模型的旧变体放弃。
        if (stats.resetStaleVectors) {
          this.log(`knowledge ingestion: vector variant reset after fingerprint/dimension drift (job ${current.id})`);
        }
        if (stats.abandonedStaleVariantId) {
          this.log(
            `knowledge ingestion: stale building variant ${stats.abandonedStaleVariantId} `
            + `marked failed (embedding model changed while interrupted; vectors preserved)`,
          );
        }
        if (outcome.status === "unavailable") {
          // 可解析性检查与执行之间模型被摘除的竞态：仍落显式 pending_embedding。
          this.deps.store.markIngestionJobPendingEmbedding({ studioId, jobId: current.id });
          return;
        }
        this.deps.store.completeIngestionJob({ studioId, jobId: current.id });
      }
    } catch (error) {
      this.handleJobFailure(studioId, job, error);
    } finally {
      this.activeJobs.delete(job.id);
      for (const key of lockKeys) {
        const owners = this.keyOwners.get(key);
        if (owners) {
          owners.delete(job.id);
          if (owners.size === 0) this.keyOwners.delete(key);
        }
      }
      resolveSettled();
      this.notifyJobSettled();
    }
  }

  /**
   * 相位边界复查（§十九 delete wins）：job 已被取消/离开 running 态时立即停步——
   * 状态已是终态（failed+cancelled_at），后续相位写入只会撞 CONFLICT。返回 true
   * 表示已停步；显式留痕原因。
   */
  private jobLeftRunning(studioId: string, jobId: string, phase: string): boolean {
    try {
      const current = this.deps.store.getIngestionJob({ studioId, jobId });
      if (current.status !== "running") {
        this.log(
          `knowledge ingestion: job ${jobId} left running before ${phase} phase `
          + `(status=${current.status}${current.cancelledAt ? " cancelled" : ""}); stopping`,
        );
        return true;
      }
      return false;
    } catch {
      // 行已随源清理删除：同样停步。
      this.log(`knowledge ingestion: job ${jobId} row vanished before ${phase} phase; stopping`);
      return true;
    }
  }

  private handleJobFailure(studioId: string, job: IngestionJob, error: unknown) {
    if (this.stopped) {
      // stop() 中断：不消耗 attempt、不写失败状态；best-effort 置回 queued，
      // embed 相位由 requeueIngestionJobById 显式留痕 KNOWLEDGE_EMBEDDING_INTERRUPTED
      // （已落库的批级 checkpoint 向量保留，续跑只补缺失 chunk）；
      // 库已关闭时留 running 残留给下次 start() 恢复（同一幂等语义）。
      try {
        this.deps.store.requeueRunningIngestionJobById({ jobId: job.id });
        this.log(`knowledge ingestion: job ${job.id} interrupted by stop(); checkpointed embedding progress is preserved`);
      } catch {
        // 库已随 close() 关闭：由下次启动恢复接管。
      }
      return;
    }
    // delete wins（§十九）：job 被显式取消（failed+cancelled_at）或已以其他方式
    // 离开 running 时，终态保持不动，仅留痕本次失败原因。
    try {
      const current = this.deps.store.getIngestionJob({ studioId, jobId: job.id });
      if (current.status !== "running") {
        this.log(
          `knowledge ingestion: job ${job.id} already terminal (${current.status}${current.cancelledAt ? ", cancelled" : ""}); `
          + `suppressed failure: ${describeIngestionError(error)}`,
        );
        return;
      }
    } catch {
      this.log(`knowledge ingestion: job ${job.id} row unavailable after failure: ${describeIngestionError(error)}`);
      return;
    }
    const message = describeIngestionError(error);
    if (isPermanentIngestionError(error) || job.attempt >= KNOWLEDGE_INGESTION_MAX_ATTEMPTS) {
      this.deps.store.failIngestionJob({ studioId, jobId: job.id, error: message });
      return;
    }
    const backoffMs = KNOWLEDGE_INGESTION_RETRY_BACKOFF_MS[
      Math.min(job.attempt, KNOWLEDGE_INGESTION_RETRY_BACKOFF_MS.length - 1)
    ];
    const retryAfter = new Date(Date.parse(this.now()) + backoffMs).toISOString();
    this.deps.store.failIngestionJob({ studioId, jobId: job.id, error: message, retryAfter });
  }
}

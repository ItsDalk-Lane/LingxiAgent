import {
  computeAutoChunkTargetChars,
  knowledgeChunkerConfigId,
  resolveKnowledgeChunkerConfig,
} from "./chunker.ts";
import { isKnowledgeError, KnowledgeError } from "./errors.ts";
import type { KnowledgeEmbeddingResult, KnowledgeEmbedder } from "./knowledge-query-service.ts";
import { KnowledgeQueryService } from "./knowledge-query-service.ts";
import { KnowledgeStore, resolveNotebookConfig, type ResolvedNotebookConfig } from "./knowledge-store.ts";
import type { IngestionJob, KnowledgeModelRef, KnowledgeParseArtifact } from "./types.ts";

/**
 * 摄入失败重试：attempt < 3 指数退避（30s/120s/600s），attempt 达到上限后标 failed
 * （显式终态；UI 手动重试走 store.requeueIngestionJob，attempt 归零、从失败 phase 续跑）。
 */
export const KNOWLEDGE_INGESTION_MAX_ATTEMPTS = 3;
export const KNOWLEDGE_INGESTION_RETRY_BACKOFF_MS = Object.freeze([30_000, 120_000, 600_000]);

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

export interface KnowledgeIngestionEmbedRequest {
  modelRef: KnowledgeModelRef;
  runId: string;
  texts: string[];
  signal?: AbortSignal;
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
  now?: () => string;
  pollIntervalMs?: number;
  log?: (message: string) => void;
}

/**
 * 摄入管线：持久化 job 表（ingestion_jobs）+ 进程内串行队列（并发 1）。
 * phase 链 parse → chunk → fts_index → embed → done，每步幂等：
 * - parse 复用 parseSource 的解析身份判断（已有 ready/needs_ocr 产物直接返回）；
 * - chunk+fts_index 复用 fingerprint + chunkerConfigId 判断（不匹配才整体重建）；
 * - embed 复用 vectorIndex.hasArtifact（chunkFingerprint + 模型身份命中即跳过）。
 * 查询侧懒构建（KnowledgeQueryService.retrieve 的 ensure 链）保留为摄入未跑时的兜底。
 */
export class KnowledgeIngestionService {
  private readonly deps: KnowledgeIngestionServiceDeps;
  private readonly now: () => string;
  private readonly pollIntervalMs: number;
  private readonly log: (message: string) => void;
  private stopped = false;
  private loopPromise: Promise<void> | null = null;
  private jobAbort: AbortController | null = null;
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
  }

  /** 启动恢复：running 残留（进程中断）重置回 queued 续跑。幂等，可单独调用。 */
  recoverInterruptedJobs(): number {
    const recovered = this.deps.store.requeueRunningIngestionJobs();
    if (recovered > 0) {
      this.log(`knowledge ingestion: recovered ${recovered} interrupted job(s)`);
    }
    return recovered;
  }

  /** 启动后台串行循环（engine init 调用一次）。重复调用是 no-op。 */
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
   * 优雅停循环：置停止位、abort 进行中的嵌入、唤醒等待中的循环立即退出。
   * 被中断的 running job 不消耗 attempt：_processJob 的停止路径 best-effort 置回
   * queued；若库已随 close() 关闭则留 running 残留，由下次 start() 的恢复接管。
   */
  stop() {
    this.stopped = true;
    this.jobAbort?.abort();
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
   * 串行消费当前全部到期 queued job 直到队列空，返回处理数。
   * 后台循环与测试共用同一入口；claimNextIngestionJob 原子认领保证并发安全。
   */
  async drainQueue(): Promise<number> {
    let processed = 0;
    while (!this.stopped) {
      const job = this.deps.store.claimNextIngestionJob();
      if (!job) break;
      await this.processJob(job);
      processed += 1;
    }
    return processed;
  }

  /**
   * 入队单源摄入（同 notebook+source 的活跃 job 由 store 层去重）。
   * chunkerConfigId 记录触发方笔记本的分块配置：parse 产物已知时按真实 blocks 计算，
   * 未知（parse 失败/未跑）时以 fixed 策略占位——执行时按真实 blocks 重算，
   * 该列只是"触发摄入时的配置"记录。
   */
  enqueueSourceIngestion(input: {
    studioId: unknown;
    notebookId: unknown;
    sourceId: unknown;
    artifactId?: unknown;
  }): IngestionJob {
    const config = this.resolveConfig(input.studioId, input.notebookId);
    const artifactId = input.artifactId ?? null;
    const configId = artifactId != null
      ? resolveKnowledgeChunkerConfig(
        this.deps.store.listArtifactBlocks({ studioId: input.studioId, parseArtifactId: artifactId }),
        { targetChars: config.chunkTargetChars },
      ).configId
      : knowledgeChunkerConfigId("fixed", config.chunkTargetChars);
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
   * 笔记本配置变更后的全量重建：该笔记本全部活跃源重新入队。
   * 无需重建的部分由相位幂等自然跳过（fingerprint/hasArtifact 命中）。
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
    const chunkTargetChars = resolved.chunkTargetChars
      ?? computeAutoChunkTargetChars(
        resolved.embeddingModelRef
          ? this.deps.getEmbeddingModelContextWindow?.(resolved.embeddingModelRef) ?? null
          : null,
      );
    return { ...resolved, chunkTargetChars };
  }

  /** 嵌入可解析性：引用存在 + 嵌入回调已接线 +（可选）同步判定通过。 */
  private embeddingResolvable(ref: KnowledgeModelRef | null): boolean {
    if (!ref || !this.deps.embedTextsForModel) return false;
    return this.deps.canEmbedWithModel ? this.deps.canEmbedWithModel(ref) : true;
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
   */
  private async processJob(job: IngestionJob) {
    const owner = this.deps.store.getIngestionJobOwner({ jobId: job.id });
    if (!owner) {
      // notebook 已被物理删除的孤儿 job：无任何显式终态可落（store 方法都要 studio 归属），
      // 显式留痕后丢弃；外键约束下正常不会发生。
      this.log(`knowledge ingestion: dropping orphan job ${job.id}`);
      return;
    }
    const { studioId } = owner;
    const abort = new AbortController();
    this.jobAbort = abort;
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
      // 因此 phase 从 chunk/fts_index 一步推进到 embed。
      if (current.phase === "chunk" || current.phase === "fts_index") {
        if (!current.artifactId) {
          throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Ingestion job has no parse artifact");
        }
        const config = this.resolveConfig(studioId, current.notebookId);
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
        const config = this.resolveConfig(studioId, current.notebookId);
        const modelRef = config.embeddingModelRef;
        if (!this.embeddingResolvable(modelRef)) {
          // 显式终态（非失败）：FTS 已可查，等模型就绪信号补跑（禁静默降级红线）。
          this.deps.store.markIngestionJobPendingEmbedding({ studioId, jobId: current.id });
          return;
        }
        const embedTexts: KnowledgeEmbedder = (request) => this.deps.embedTextsForModel!({
          ...request,
          modelRef: modelRef!,
        });
        const outcome = await this.deps.queryService.embedArtifactForIngestion({
          runId: current.id,
          parseArtifactId: current.artifactId,
          embedTexts,
          signal: abort.signal,
          // 每批嵌入成功后落进度（64 块/批 ≈ 每 708 块 12 次 UPDATE）。
          // 写失败让错误沿嵌入路径抛出，走既有 handleJobFailure 分类，不吞错。
          onProgress: (done, total) => {
            this.deps.store.updateIngestionJobProgress({ studioId, jobId: current.id, done, total });
          },
        });
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
      if (this.jobAbort === abort) this.jobAbort = null;
    }
  }

  private handleJobFailure(studioId: string, job: IngestionJob, error: unknown) {
    if (this.stopped) {
      // stop() 中断：不消耗 attempt、不写失败状态；best-effort 置回 queued，
      // 库已关闭时留 running 残留给下次 start() 恢复（同一幂等语义）。
      try {
        this.deps.store.requeueRunningIngestionJobs();
      } catch {
        // 库已随 close() 关闭：由下次启动恢复接管。
      }
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

import crypto from "node:crypto";
import { isKnowledgeError, KnowledgeError } from "./errors.ts";
import { MODEL_OPERATION_RERANK_MAX_DOCS } from "../../shared/model-operations.ts";

export const KNOWLEDGE_RERANK_DEADLINE_MS = 15_000;
export const KNOWLEDGE_FAST_RERANK_DEADLINE_MS = 5_000;
export const KNOWLEDGE_RERANK_CLEAR_MARGIN = 0.008;
export const KNOWLEDGE_RERANK_MAX_DOCS = MODEL_OPERATION_RERANK_MAX_DOCS;

export interface KnowledgeRerankPolicy {
  readonly enabled: boolean;
  readonly marginGate: boolean;
  readonly clearMargin: number;
  readonly deadlineMs: number;
  readonly maxDocuments: number;
}

export type KnowledgeRerankPolicyInput = Pick<KnowledgeRerankPolicy, "enabled">
  & Partial<Omit<KnowledgeRerankPolicy, "enabled">>;

export type KnowledgeReranker = (request: {
  runId: string;
  query: string;
  documents: string[];
  topN: number;
  signal?: AbortSignal;
}) => Promise<{ results: Array<{ index: number; score: number }> } | null>;

export function normalizeKnowledgeRerankPolicy(input: KnowledgeRerankPolicyInput): KnowledgeRerankPolicy {
  const policy: KnowledgeRerankPolicy = {
    enabled: input.enabled,
    marginGate: input.marginGate ?? false,
    clearMargin: input.clearMargin ?? KNOWLEDGE_RERANK_CLEAR_MARGIN,
    deadlineMs: input.deadlineMs ?? KNOWLEDGE_RERANK_DEADLINE_MS,
    maxDocuments: input.maxDocuments ?? KNOWLEDGE_RERANK_MAX_DOCS,
  };
  if (typeof policy.enabled !== "boolean" || typeof policy.marginGate !== "boolean"
    || !Number.isFinite(policy.clearMargin) || policy.clearMargin < 0
    || !Number.isSafeInteger(policy.deadlineMs) || policy.deadlineMs < 1
    || !Number.isSafeInteger(policy.maxDocuments) || policy.maxDocuments < 1
    || policy.maxDocuments > KNOWLEDGE_RERANK_MAX_DOCS) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge rerank policy is invalid");
  }
  return policy;
}

export const KNOWLEDGE_RERANK_DISABLED_POLICY = Object.freeze(normalizeKnowledgeRerankPolicy({ enabled: false }));
export const KNOWLEDGE_RERANK_ENABLED_POLICY = Object.freeze(normalizeKnowledgeRerankPolicy({ enabled: true }));
export const KNOWLEDGE_FAST_RERANK_POLICY = Object.freeze(normalizeKnowledgeRerankPolicy({
  enabled: true,
  marginGate: true,
  deadlineMs: KNOWLEDGE_FAST_RERANK_DEADLINE_MS,
}));

/** 检索结果缓存只认规范化后的完整策略，避免不同重排语义复用同一份结果。 */
export function knowledgeRetrievalPolicyDigest(input: {
  channel: "fts" | "hybrid";
  rerankPolicy: KnowledgeRerankPolicy;
}): string {
  return crypto.createHash("sha256").update(JSON.stringify({
    version: 1,
    channel: input.channel,
    rerank: input.rerankPolicy,
  })).digest("hex");
}

function isAbortLike(error: unknown): boolean {
  if (!error || typeof error !== "object") return false;
  const candidate = error as { name?: unknown; type?: unknown };
  return candidate.name === "AbortError" || candidate.name === "TimeoutError" || candidate.type === "aborted";
}

async function invokeRerankerWithDeadline(input: {
  reranker: KnowledgeReranker;
  runId: string;
  question: string;
  documents: string[];
  signal?: AbortSignal;
  deadlineMs: number;
}) {
  const controller = new AbortController();
  const onExternalAbort = () => controller.abort();
  input.signal?.addEventListener("abort", onExternalAbort, { once: true });
  let timer: ReturnType<typeof setTimeout> | undefined;
  const deadline = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      controller.abort();
      const error = new Error(`rerank deadline exceeded after ${input.deadlineMs}ms`);
      error.name = "KnowledgeRerankDeadlineError";
      reject(error);
    }, input.deadlineMs);
  });
  const attempt = Promise.resolve().then(() => input.reranker({
    runId: input.runId,
    query: input.question,
    documents: input.documents,
    topN: input.documents.length,
    signal: controller.signal,
  }));
  deadline.catch(() => {});
  attempt.catch(() => {});
  try {
    return await Promise.race([attempt, deadline]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
    input.signal?.removeEventListener("abort", onExternalAbort);
  }
}

/** 两条知识检索路径共用的唯一重排决策与执行入口。 */
export async function executeKnowledgeRerankPolicy<T extends { text: string; score: number }>(input: {
  candidates: T[];
  question: string;
  runId: string;
  signal?: AbortSignal;
  reranker: KnowledgeReranker | null;
  rerankerUnavailableReason?: string;
  policy: KnowledgeRerankPolicy;
}): Promise<{
  candidates: T[];
  rerankDegradeReason?: string;
  rerankSkippedReason?: string;
  rerankMs: number;
}> {
  const started = Date.now();
  const rerankCandidates = input.candidates.slice(0, input.policy.maxDocuments);
  const rerankTail = input.candidates.slice(input.policy.maxDocuments);
  const marginGateActive = input.policy.enabled
    && input.policy.marginGate
    && rerankCandidates.length >= 2
    && rerankCandidates[0].score - rerankCandidates[1].score >= input.policy.clearMargin;
  if (marginGateActive) {
    return {
      candidates: input.candidates,
      rerankSkippedReason: `rerank skipped (margin gate: top-1 RRF score ${rerankCandidates[0].score.toFixed(4)} `
        + `leads top-2 ${rerankCandidates[1].score.toFixed(4)} ≥ ${input.policy.clearMargin}); kept RRF ranking`,
      rerankMs: Date.now() - started,
    };
  }
  if (!input.policy.enabled || rerankCandidates.length === 0) {
    return { candidates: input.candidates, rerankMs: Date.now() - started };
  }
  if (!input.reranker) {
    return {
      candidates: input.candidates,
      ...(input.rerankerUnavailableReason
        ? { rerankDegradeReason: `${input.rerankerUnavailableReason}; kept retrieval ranking` }
        : {}),
      rerankMs: Date.now() - started,
    };
  }
  let reranked: Awaited<ReturnType<KnowledgeReranker>>;
  try {
    reranked = await invokeRerankerWithDeadline({
      reranker: input.reranker,
      runId: input.runId,
      question: input.question,
      documents: rerankCandidates.map(candidate => candidate.text),
      signal: input.signal,
      deadlineMs: input.policy.deadlineMs,
    });
  } catch (error) {
    input.signal?.throwIfAborted();
    if (isAbortLike(error) || isKnowledgeError(error)) throw error;
    const cause = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
    return {
      candidates: input.candidates,
      rerankDegradeReason: `rerank degraded (${cause}); kept RRF ranking`,
      rerankMs: Date.now() - started,
    };
  }
  if (!reranked) return { candidates: input.candidates, rerankMs: Date.now() - started };
  if (!Array.isArray(reranked.results)
    || reranked.results.length !== rerankCandidates.length
    || new Set(reranked.results.map(entry => entry.index)).size !== rerankCandidates.length
    || reranked.results.some(entry => !Number.isSafeInteger(entry.index)
      || entry.index < 0 || entry.index >= rerankCandidates.length
      || typeof entry.score !== "number" || !Number.isFinite(entry.score))) {
    throw new KnowledgeError("KNOWLEDGE_RETRIEVAL_UNAVAILABLE", "Knowledge rerank response is invalid");
  }
  return {
    candidates: [
      ...reranked.results.map(entry => ({ ...rerankCandidates[entry.index], score: entry.score })),
      ...rerankTail,
    ],
    rerankMs: Date.now() - started,
  };
}

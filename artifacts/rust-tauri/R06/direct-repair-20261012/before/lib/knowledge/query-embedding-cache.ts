import type { KnowledgeEmbeddingResult } from "./knowledge-query-service.ts";

/** 查询嵌入缓存身份：按配置的模型引用与配置修订隔离。 */
export interface QueryEmbeddingCacheKey {
  normalizedQuery: string;
  provider: string;
  modelId: string;
  modelConfigurationRevision: string;
  inputType: "query";
}

export interface CacheLookup<T> { value: T; hit: boolean }

/** 有界缓存和在途共享使用相同淘汰逻辑；每个等待者各自取消，最后一个离开才终止底层。 */
export class KnowledgeQueryCache<T> {
  private readonly entries = new Map<string, { value: T; expiresAt: number }>();
  private readonly flights = new Map<string, {
    promise: Promise<T>; controller: AbortController; waiters: number;
  }>();
  private readonly capacity: number;
  private readonly ttlMs: number;
  private readonly now: () => number;

  constructor(capacity: number, ttlMs: number, now: () => number = Date.now) {
    this.capacity = capacity; this.ttlMs = ttlMs; this.now = now;
  }

  async getOrCreate(key: string, loader: (signal: AbortSignal) => Promise<T>, signal?: AbortSignal): Promise<CacheLookup<T>> {
    signal?.throwIfAborted();
    const cached = this.entries.get(key);
    if (cached && cached.expiresAt > this.now()) {
      this.entries.delete(key); this.entries.set(key, cached);
      return { value: structuredClone(cached.value), hit: true };
    }
    if (cached) this.entries.delete(key);
    let flight = this.flights.get(key);
    const hit = !!flight;
    if (!flight) {
      const controller = new AbortController();
      const created = { controller, waiters: 0, promise: null as unknown as Promise<T> };
      created.promise = Promise.resolve().then(() => {
        controller.signal.throwIfAborted();
        return loader(controller.signal);
      }).then(value => {
        if (this.flights.get(key) === created && !controller.signal.aborted) {
          this.entries.set(key, { value: structuredClone(value), expiresAt: this.now() + this.ttlMs });
          while (this.entries.size > this.capacity) this.entries.delete(this.entries.keys().next().value!);
        }
        return value;
      }).finally(() => {
        if (this.flights.get(key) === created) this.flights.delete(key);
      });
      // 所有等待者都取消后，仍消费不响应取消的底层失败，避免未处理拒绝。
      created.promise.catch(() => {});
      this.flights.set(key, created); flight = created;
    }
    flight.waiters += 1;
    const current = flight;
    return new Promise((resolve, reject) => {
      let settled = false;
      const finish = (ok: boolean, value: unknown) => {
        if (settled) return;
        settled = true;
        signal?.removeEventListener("abort", onAbort);
        current.waiters -= 1;
        if (current.waiters === 0 && this.flights.get(key) === current) {
          this.flights.delete(key); current.controller.abort();
        }
        if (!ok) reject(value);
        else resolve({ value: structuredClone(value as T), hit });
      };
      const onAbort = () => finish(false, signal!.reason);
      signal?.addEventListener("abort", onAbort, { once: true });
      current.promise.then(value => finish(true, value), error => finish(false, error));
      if (signal?.aborted) onAbort();
    });
  }

  invalidate(predicate: (key: string) => boolean): void {
    for (const key of this.entries.keys()) if (predicate(key)) this.entries.delete(key);
    for (const [key, flight] of this.flights) {
      if (predicate(key)) { this.flights.delete(key); flight.controller.abort(); }
    }
  }

  clear(): void { this.invalidate(() => true); }
}

export function normalizeKnowledgeQuery(query: string): string {
  return query.normalize("NFKC").trim().replace(/\s+/gu, " ");
}

export class QueryEmbeddingCache {
  private readonly cache: KnowledgeQueryCache<KnowledgeEmbeddingResult>;
  constructor(now?: () => number) { this.cache = new KnowledgeQueryCache(512, 10 * 60_000, now); }

  getOrCreate(key: QueryEmbeddingCacheKey, loader: (signal: AbortSignal) => Promise<KnowledgeEmbeddingResult>, signal?: AbortSignal) {
    return this.cache.getOrCreate(JSON.stringify([
      normalizeKnowledgeQuery(key.normalizedQuery), key.provider, key.modelId, key.modelConfigurationRevision, key.inputType,
    ]), loader, signal);
  }

  invalidateModel(provider: string, modelId: string): void {
    this.cache.invalidate(key => { const parts = JSON.parse(key); return parts[1] === provider && parts[2] === modelId; });
  }

  clear(): void { this.cache.clear(); }
}

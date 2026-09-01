/**
 * fact-embeddings.js — facts.db 语义检索的纯函数层
 *
 * v1 记忆库曾用 sqlite-vec（已随 store.js 退役）；v3 重新引入向量检索时
 * 刻意沿用知识库 knowledge-vector.db 的同一范式：float32 小端 BLOB + JS
 * 暴力余弦。facts 量级（千级元事实）下全表扫描是毫秒级，不值得引入
 * 向量扩展依赖；未来内置 ~600MB 本地嵌入模型落地时，本层零改动——
 * 模型身份经 model_key 分区，换模型=换分区，旧向量惰性失效。
 */

import { createHash } from "node:crypto";

/**
 * 模型身份键：对齐知识库 vectorModelIdentity 的语义（provider/model/protocol
 * 决定嵌入空间）。不把 dimensions 计入——同一模型的 MRL 截断维度变化不应
 * 静默产生两个分区，维度不匹配由检索侧显式跳过。
 */
export function factEmbeddingModelKey(modelRef: { provider: string; id: string }, protocol: string): string {
  return createHash("sha256")
    .update(JSON.stringify([modelRef.provider, modelRef.id, protocol]))
    .digest("hex");
}

/** float32 小端序列化（与 knowledge vector-index-adapter 同构） */
export function serializeVector(vector: number[]): Buffer {
  const buffer = Buffer.alloc(vector.length * 4);
  for (let i = 0; i < vector.length; i++) {
    buffer.writeFloatLE(vector[i], i * 4);
  }
  return buffer;
}

export function parseVector(buffer: Buffer): number[] {
  const vector = new Array(buffer.length / 4);
  for (let i = 0; i < vector.length; i++) {
    vector[i] = buffer.readFloatLE(i * 4);
  }
  return vector;
}

export function cosineSimilarity(a: number[], b: number[]): number {
  if (a.length !== b.length || a.length === 0) return -1;
  let dot = 0;
  let normA = 0;
  let normB = 0;
  for (let i = 0; i < a.length; i++) {
    dot += a[i] * b[i];
    normA += a[i] * a[i];
    normB += b[i] * b[i];
  }
  if (normA === 0 || normB === 0) return -1;
  return dot / (Math.sqrt(normA) * Math.sqrt(normB));
}

/** RRF 融合常量（对齐知识库 KNOWLEDGE_RRF_K） */
export const MEMORY_RRF_K = 60;

/**
 * Reciprocal Rank Fusion：多路排名列表 → 统一分数。
 * 输入是按相关度排好序的 id 列表（每路内部不重复）；同 id 多路命中分数累加。
 * 返回按融合分降序的 [id, score]。
 */
export function rrfFuse(rankLists: Array<readonly any[]>, k = MEMORY_RRF_K): Array<[any, number]> {
  const scores = new Map<any, number>();
  for (const list of rankLists) {
    if (!Array.isArray(list)) continue;
    for (let rank = 0; rank < list.length; rank++) {
      const id = list[rank];
      if (id === undefined || id === null) continue;
      scores.set(id, (scores.get(id) || 0) + 1 / (k + rank + 1));
    }
  }
  return [...scores.entries()].sort((x, y) => y[1] - x[1]);
}

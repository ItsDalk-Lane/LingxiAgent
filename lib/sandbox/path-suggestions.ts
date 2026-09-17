/**
 * path-suggestions.ts — 找不到路径时的「你是不是想找」提示
 *
 * 对照 grok path_suggestions / openclaude findSimilarFile：ENOENT 报错附上
 * 同目录下最相似的候选（大小写不同、同词干不同扩展名、编辑距离≤2、词干包含），
 * 让模型一轮改对，不用再猜。
 */

import { readdir } from "node:fs/promises";
import { basename, dirname } from "node:path";

function levenshtein(a: string, b: string, max = 2): number {
  if (a === b) return 0;
  if (Math.abs(a.length - b.length) > max) return max + 1;
  const prev = new Array(b.length + 1).fill(0).map((_, i) => i);
  for (let i = 1; i <= a.length; i++) {
    let carry = prev[0];
    prev[0] = i;
    let rowMin = prev[0];
    for (let j = 1; j <= b.length; j++) {
      const temp = prev[j];
      prev[j] = Math.min(
        prev[j] + 1,
        prev[j - 1] + 1,
        carry + (a[i - 1] === b[j - 1] ? 0 : 1),
      );
      carry = temp;
      rowMin = Math.min(rowMin, prev[j]);
    }
    if (rowMin > max) return max + 1;
  }
  return prev[b.length];
}

function splitStem(name: string) {
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return { stem: name, ext: "" };
  return { stem: name.slice(0, dot), ext: name.slice(dot) };
}

/**
 * 返回同目录下与目标名最相似的候选（最多 limit 个），找不到返回空数组。
 * 目录不可读、目标没有目录部分等情况一律静默返回空——这是纯增益提示。
 */
export async function suggestSimilarPaths(absolutePath: string, { limit = 3 }: { limit?: number } = {}): Promise<string[]> {
  const base = basename(absolutePath);
  if (!base || base === "/" || base === ".") return [];
  const dir = dirname(absolutePath);
  let entries: string[];
  try {
    entries = await readdir(dir);
  } catch {
    return [];
  }
  const lowerBase = base.toLowerCase();
  const { stem } = splitStem(base);
  const lowerStem = stem.toLowerCase();
  // 短名字的编辑距离噪声大：≥5 字符允许距离 2，4 字符只允许距离 1，更短不做模糊。
  const maxFuzzyDistance = base.length >= 5 ? 2 : base.length === 4 ? 1 : 0;
  const scored: Array<{ name: string; score: number }> = [];
  for (const entry of entries) {
    if (entry === base) continue;
    const lowerEntry = entry.toLowerCase();
    let score: number | null = null;
    if (lowerEntry === lowerBase) {
      score = 0;
    } else {
      const entryParts = splitStem(entry);
      if (entryParts.stem.toLowerCase() === lowerStem && entryParts.ext !== splitStem(base).ext) {
        score = 1;
      } else if (maxFuzzyDistance > 0 && levenshtein(lowerBase, lowerEntry, maxFuzzyDistance) <= maxFuzzyDistance) {
        score = 2;
      } else if (stem.length >= 4 && entryParts.stem.toLowerCase().includes(lowerStem)) {
        score = 3;
      }
    }
    if (score !== null) scored.push({ name: entry, score });
  }
  scored.sort((a, b) => a.score - b.score || a.name.localeCompare(b.name));
  return scored.slice(0, limit).map((item) => item.name);
}

/** 拼装可附加到报错消息后的提示段；没有候选时返回空串。 */
export function formatDidYouMean(candidates: string[], dir: string): string {
  if (candidates.length === 0) return "";
  const quoted = candidates.map((name) => `"${name}"`).join(" or ");
  return `\nDid you mean ${quoted}? (in ${dir})`;
}

/** 从错误消息/错误码判断是否是「路径不存在」类失败。 */
export function isPathMissingError(err: unknown): boolean {
  const code = (err as any)?.code;
  if (code === "ENOENT" || code === "ENOTDIR") return true;
  const message = err instanceof Error ? err.message : String(err ?? "");
  return /\bENOENT\b|no such file or directory|^Path not found:/im.test(message);
}

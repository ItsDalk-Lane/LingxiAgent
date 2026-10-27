/**
 * R00-T05 回放结果规范化：屏蔽跨次重放必然变化且已声明的字段，其余逐字节比较。
 *
 * 允许变动字段分两类（在 FIXTURE_MANIFEST.json global_normalization 声明）：
 *  1. 按键名屏蔽：timestamp / mtime / createdAt / updatedAt 等运行期时间；
 *  2. 按模式屏蔽：临时目录前缀（os.tmpdir()）、生成型 id（SDK 会话 UUID、sf_ 文件 id）、
 *     stat 派生 revision（mtime/size 签名）、子进程 pid。
 * 规范化只做「值 → 占位符」替换，不删除键、不重排键；因此 diff 里出现的任何其他差异都是失败。
 */

import os from "node:os";
import path from "node:path";

export const ALLOWED_VARYING_KEYS = new Set([
  "timestamp",
  "labelTimestamp",
  "createdAt",
  "updatedAt",
  "mtime",
  "mtimeMs",
  "pid",
]);

export interface NormalizeOptions {
  tmpRoot?: string;
}

function makePatterns(tmpRoot: string): Array<{ pattern: RegExp; placeholder: string }> {
  const escaped = tmpRoot.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return [
    { pattern: new RegExp(escaped + "[^\"\\\\]*", "g"), placeholder: "<tmp-path>" },
    { pattern: /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/, placeholder: "<uuid>" },
    { pattern: /^sf_[a-f0-9]{16}$/, placeholder: "<sf-id>" },
    { pattern: /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z$/, placeholder: "<iso-ts>" },
    { pattern: /^\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}-\d+Z$/, placeholder: "<file-ts>" },
  ];
}

export function normalizeValue(value: unknown, key: string | null, options: NormalizeOptions = {}): unknown {
  const patterns = makePatterns(options.tmpRoot ?? os.tmpdir());
  if (key !== null && ALLOWED_VARYING_KEYS.has(key)) {
    return "<varying:" + key + ">";
  }
  if (typeof value === "string") {
    let out = value;
    for (const { pattern, placeholder } of patterns) {
      out = out.replace(pattern, () => placeholder);
    }
    return out;
  }
  if (Array.isArray(value)) {
    return value.map((item) => normalizeValue(item, null, options));
  }
  if (value && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value)) {
      out[k] = normalizeValue(v, k, options);
    }
    return out;
  }
  return value;
}

/** 稳定序列化：键序按字母排序，保证 diff 只反映语义差异。 */
export function stableStringify(value: unknown): string {
  const sortDeep = (v: unknown): unknown => {
    if (Array.isArray(v)) return v.map(sortDeep);
    if (v && typeof v === "object") {
      return Object.fromEntries(
        Object.keys(v).sort().map((k) => [k, sortDeep((v as Record<string, unknown>)[k])]),
      );
    }
    return v;
  };
  return JSON.stringify(sortDeep(value), null, 2);
}

export function canonicalize(value: unknown, options: NormalizeOptions = {}): string {
  return stableStringify(normalizeValue(value, null, options));
}

/** 两个规范化结果的统一 diff 行数组（仅报告用；断言用 expect 相等）。 */
export function diffCanonical(a: string, b: string): string[] {
  const aLines = a.split("\n");
  const bLines = b.split("\n");
  const out: string[] = [];
  const max = Math.max(aLines.length, bLines.length);
  for (let i = 0; i < max; i += 1) {
    if (aLines[i] !== bLines[i]) {
      out.push(`- ${aLines[i] ?? ""}`);
      out.push(`+ ${bLines[i] ?? ""}`);
    }
  }
  return out;
}

export { path };

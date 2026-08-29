/**
 * The single character-count token estimate shared by every caller that needs
 * to price a plain string against a token budget.
 *
 * This lives in a dependency-free leaf on purpose. The compactor, the session
 * reminder budget and the tool catalog manifest all need the same number, and
 * several of those callers (display projection in the hub and the collab
 * transcript) must not pull the compaction stack in behind it.
 *
 * 语言感知口径：CJK 字符（汉字/假名/谚文/CJK 标点/全角形式）按
 * CJK_TOKENS_PER_CHAR 计，其余字符维持 4 chars/token。旧实现 chars/4 是
 * 纯英文口径，对中文低估约 4~8 倍；系数 1.1 取常见分词器（GPT-4o/Qwen
 * ≈0.6–1.0、Claude ≈1.2–1.6）的中偏保守值，保持本模块"宁可高估"的传统。
 */
export const CJK_TOKENS_PER_CHAR = 1.1;
export const NON_CJK_CHARS_PER_TOKEN = 4;

/** CJK 码点区间：谚文字母 / CJK 部首与统一表意 / 谚文音节 / 兼容表意 / 兼容形式 / 全角形式 / 扩展 B 起。 */
const CJK_RANGES: ReadonlyArray<readonly [number, number]> = [
  [0x1100, 0x11ff],
  [0x2e80, 0x9fff],
  [0xac00, 0xd7af],
  [0xf900, 0xfaff],
  [0xfe30, 0xfe4f],
  [0xff00, 0xffef],
  [0x20000, 0x2fa1f],
];

function isCjkCodePoint(codePoint: number): boolean {
  for (const [low, high] of CJK_RANGES) {
    if (codePoint >= low && codePoint <= high) return true;
  }
  return false;
}

export function estimateTextTokens(text: unknown): number {
  if (typeof text !== "string" || text.length === 0) return 0;
  let cjk = 0;
  let total = 0;
  for (let index = 0; index < text.length; index += 1) {
    const codePoint = text.codePointAt(index)!;
    if (codePoint > 0xffff) index += 1; // 代理对占 2 个 code unit，只计一次
    total += 1;
    if (isCjkCodePoint(codePoint)) cjk += 1;
  }
  return Math.ceil(cjk * CJK_TOKENS_PER_CHAR + (total - cjk) / NON_CJK_CHARS_PER_TOKEN);
}

/**
 * 返回估算恰好不超过 budgetTokens 的最长前缀（正向截断）。
 * 逐字符按各自口径（CJK=CJK_TOKENS_PER_CHAR、其余=1/4）累计，替代旧的
 * "预算×4 反解字符数"——该反解只在纯英文口径下成立。
 */
export function trimTextToTokenBudget(text: string, budgetTokens: number): string {
  if (typeof text !== "string" || text.length === 0) return "";
  if (!Number.isFinite(budgetTokens) || budgetTokens <= 0) return "";
  let used = 0;
  for (let index = 0; index < text.length; index += 1) {
    const codePoint = text.codePointAt(index)!;
    if (codePoint > 0xffff) index += 1;
    used += isCjkCodePoint(codePoint) ? CJK_TOKENS_PER_CHAR : 1 / NON_CJK_CHARS_PER_TOKEN;
    if (used > budgetTokens) return text.slice(0, index);
  }
  return text;
}

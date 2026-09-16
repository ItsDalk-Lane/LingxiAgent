/**
 * media-family.ts — 同系列媒体模型的家族匹配（纯函数，无 I/O）
 *
 * 供应商插件只为写代码时已存在的模型 ID 声明参数页（modes / ratios / …）。
 * 供应商发布同系列新款（agnes-image-2.5-flash 之于声明的 agnes-image-2.1-flash）
 * 时，结构上仍是同一家族；这里按「词元超集 + 变量槽配对」识别同家族候选，
 * 让未声明的新款继承旧款的参数页，而不是每次出新 ID 都改声明代码。
 *
 * 保守约束：
 * - 只在调用方给定的同供应商、同能力声明列表内匹配，绝不跨供应商借页；
 * - 词元必须逐字相等（大小写不敏感），只有版本 / 日期槽允许变化；
 * - 候选与锚点必须至少有一处变量差异（完全一致属于精确匹配，不属于继承）；
 * - 锚点的每个变量槽都要能在候选里找到对应，候选可以比锚点多变量槽
 *   （如火山在 seedream 版本号后追加日期段），反之不行。
 */

type VariableToken = { kind: "variable"; text: string; numeric: number; date: boolean };
type WordToken = { kind: "word"; text: string };
type Token = VariableToken | WordToken;

/** 版本槽：纯数字（4、0）或带点版本（2.1、v2.0） */
const VERSION_PATTERN = /^v?\d+(?:\.\d+)*$/;
/** 日期槽：6–8 位纯数字（250828、20260415） */
const DATE_PATTERN = /^\d{6,8}$/;

function tokenize(modelId: string): Token[] {
  return String(modelId || "")
    .split(/[-_]+/)
    .filter(Boolean)
    .map((raw) => {
      const text = raw.toLowerCase();
      if (DATE_PATTERN.test(text) || VERSION_PATTERN.test(text)) {
        const numeric = Number.parseFloat(text.replace(/^v/, ""));
        return {
          kind: "variable" as const,
          text,
          numeric: Number.isFinite(numeric) ? numeric : 0,
          date: DATE_PATTERN.test(text),
        };
      }
      return { kind: "word" as const, text };
    });
}

function wordCounts(tokens: Token[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const token of tokens) {
    if (token.kind !== "word") continue;
    counts.set(token.text, (counts.get(token.text) || 0) + 1);
  }
  return counts;
}

function containsWordSuperset(candidate: Token[], anchor: Token[]): boolean {
  const candidateCounts = wordCounts(candidate);
  for (const [text, count] of wordCounts(anchor)) {
    if ((candidateCounts.get(text) || 0) < count) return false;
  }
  return true;
}

/** 候选变量槽数须 ≥ 锚点；按位置贪心配对，返回逐位距离之和；配不上返回 null。 */
function alignVariableSlots(anchor: Token[], candidate: Token[]): number | null {
  const anchorVars = anchor.filter((token): token is VariableToken => token.kind === "variable");
  const candidateVars = candidate.filter((token): token is VariableToken => token.kind === "variable");
  if (candidateVars.length < anchorVars.length) return null;
  let distance = 0;
  for (let index = 0; index < anchorVars.length; index += 1) {
    const anchorSlot = anchorVars[index];
    const candidateSlot = candidateVars[index];
    // 日期段对日期段视为等价（同系列的不同构建日期不构成参数差异），
    // 否则日期数值差会淹没版本差异，导致择优选错锚点。
    if (anchorSlot.date && candidateSlot.date) continue;
    distance += Math.abs(anchorSlot.numeric - candidateSlot.numeric);
  }
  return distance;
}

/**
 * 差异判定：候选相对锚点存在任何新增性差异（多限定词、多变量槽、变量值不同）。
 * 词元超集与变量槽对齐才是安全性边界；这里只负责排除「与锚点完全同形」的候选。
 */
function hasAnyDifference(anchor: Token[], candidate: Token[]): boolean {
  const countWords = (tokens: Token[]) => tokens.filter((token) => token.kind === "word").length;
  if (countWords(candidate) > countWords(anchor)) return true;
  const anchorVars = anchor.filter((token) => token.kind === "variable");
  const candidateVars = candidate.filter((token) => token.kind === "variable");
  // 候选变量槽比锚点少 = 信息不足，不成其为新款（后续槽对齐也会拒绝）。
  if (candidateVars.length < anchorVars.length) return false;
  if (candidateVars.length > anchorVars.length) return true;
  return anchorVars.some((token, index) => candidateVars[index].text !== token.text);
}

export interface MediaFamilyMatch<Declaration> {
  /** 被继承的锚点声明（原对象引用） */
  declaration: Declaration;
  /** 锚点声明的模型 ID */
  inheritedFrom: string;
}

/**
 * 在声明列表里为候选 ID 找同家族锚点；找不到返回 null。
 * 多个锚点都匹配时择优（确定性）：词元更具体者优先 → 版本距离小者优先 → ID 字典序。
 */
export function matchMediaFamilyDeclaration<Declaration extends { id: string }>(
  declarations: Declaration[] | null | undefined,
  candidateId: string,
): MediaFamilyMatch<Declaration> | null {
  const candidate = tokenize(candidateId);
  if (candidate.length === 0) return null;
  const lowerCandidate = candidateId.toLowerCase();

  let best: Declaration | null = null;
  let bestKey: [number, number, string] | null = null;
  for (const declaration of declarations || []) {
    const anchorId = typeof declaration?.id === "string" ? declaration.id : "";
    if (!anchorId) continue;
    // 候选本身就是精确声明：走精确匹配链路，不做继承。
    if (anchorId.toLowerCase() === lowerCandidate) return null;
    const anchor = tokenize(anchorId);
    if (!containsWordSuperset(candidate, anchor)) continue;
    if (!hasAnyDifference(anchor, candidate)) continue;
    const distance = alignVariableSlots(anchor, candidate);
    if (distance === null) continue;
    const anchorWordCount = wordCounts(anchor).size;
    // 词元更多 = 更具体的锚点（优先）；距离更小 = 版本更接近（优先）；最后按 ID 稳定排序。
    // 注意不能用数组 < 数组：JS 会先把数组转成字符串再比较，负数位会判错。
    const key: [number, number, string] = [-anchorWordCount, distance, anchorId];
    if (!bestKey || compareTuple(key, bestKey) < 0) {
      best = declaration;
      bestKey = key;
    }
  }
  return best ? { declaration: best, inheritedFrom: best.id } : null;
}

function compareTuple(a: [number, number, string], b: [number, number, string]): number {
  if (a[0] !== b[0]) return a[0] - b[0];
  if (a[1] !== b[1]) return a[1] - b[1];
  return a[2] < b[2] ? -1 : a[2] > b[2] ? 1 : 0;
}

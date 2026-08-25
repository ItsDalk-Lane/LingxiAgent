import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const ASCII_WORD_RE = /[a-z0-9_][a-z0-9_.-]*/giu;
const CJK_RUN_RE = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]+/gu;
const PUNCTUATION_RE = /^[\p{P}\p{S}\s]+$/u;
const MAX_QUERY_TERMS = 96;

let jiebaInstance: any = null;

export class SearchTokenizerUnavailableError extends Error {
  declare code: "SEARCH_TOKENIZER_UNAVAILABLE";
  declare cause: unknown;

  constructor(cause: unknown) {
    super("search_tokenizer_unavailable");
    this.name = "SearchTokenizerUnavailableError";
    this.code = "SEARCH_TOKENIZER_UNAVAILABLE";
    this.cause = cause;
  }
}

export function normalizeSearchText(value: unknown): string {
  return String(value || "")
    .normalize("NFKC")
    .toLowerCase()
    .replace(/\s+/gu, " ")
    .trim();
}

function getJieba() {
  if (jiebaInstance) return jiebaInstance;
  try {
    const { Jieba } = require("@node-rs/jieba");
    const { dict } = require("@node-rs/jieba/dict");
    jiebaInstance = Jieba.withDict(dict);
    return jiebaInstance;
  } catch (error) {
    throw new SearchTokenizerUnavailableError(error);
  }
}

function addToken(target: Set<string>, value: unknown) {
  const token = normalizeSearchText(value);
  if (!token || PUNCTUATION_RE.test(token)) return;
  if (token.length === 1 && /^[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]$/u.test(token)) {
    return;
  }
  target.add(token);
}

function addCjkNgrams(target: Set<string>, text: string) {
  CJK_RUN_RE.lastIndex = 0;
  for (const match of text.matchAll(CJK_RUN_RE)) {
    const chars = Array.from(match[0]);
    for (const size of [2, 3]) {
      for (let index = 0; index <= chars.length - size; index += 1) {
        addToken(target, chars.slice(index, index + size).join(""));
      }
    }
  }
}

/**
 * 统一的跨语言检索分词。它只提供算法，不依赖会话、记忆或 Knowledge 业务。
 */
export function tokenizeSearchText(value: unknown, options: { includeCjkNgrams?: boolean } = {}): string[] {
  const normalized = normalizeSearchText(value);
  if (!normalized) return [];
  const tokens = new Set<string>();

  for (const match of normalized.matchAll(ASCII_WORD_RE)) addToken(tokens, match[0]);
  for (const token of getJieba().cutForSearch(normalized, true)) addToken(tokens, token);
  if (options.includeCjkNgrams !== false) addCjkNgrams(tokens, normalized);

  return [...tokens];
}

/** 写入 FTS 的派生文本；原文仍单独保留，token 只是可重建索引。 */
export function buildSearchDocumentText(value: unknown): string {
  const normalized = normalizeSearchText(value);
  if (!normalized) return "";
  return [normalized, ...tokenizeSearchText(normalized)].join(" ");
}

/** 只生成经过引号转义的 FTS5 字面量，不接受调用方拼接语法。 */
export function buildFtsLiteralQuery(value: unknown): string {
  const terms = tokenizeSearchText(value).slice(0, MAX_QUERY_TERMS);
  return terms
    .map(term => `"${term.replace(/"/gu, '""')}"`)
    .join(" OR ");
}

/**
 * edit-error-hints.ts — 编辑失败的报错定位增强
 *
 * 对照 dsh tool-str-replace-editor（多匹配报行号清单）、grok search_replace
 * （最近匹配行 + 易混字符检测）、openclaude 容错归一化：
 * - 多匹配：给出每一处出现的起始行号；
 * - 找不到：先查易混字符（弯引号/长破折号/不换行空格）——归一化后能匹配上
 *   才提示「文件里是排版变体」，绝不误导；否则用 oldText 里最长的词找最近
 *   匹配行；两者都没有时提示文件可能已被改动、先重读。
 *
 * 只增强报错文本，不改匹配语义；isError 保持 true（dsh 同款：工具级失败
 * 是错误，但消息必须可行动）。
 */

import { readFile as fsReadFile } from "node:fs/promises";

// 与 pi edit 的 normalizeToLF 同语义（行内实现，避免依赖 SDK 内部导出面）。
function normalizeToLF(text: string) {
  return text.replace(/\r\n?/g, "\n");
}

const CONFUSABLE_MAP: Array<[RegExp, string]> = [
  [/[\u2018\u2019\u201b\u02bc]/g, "'"],
  [/[\u201c\u201d\u201f\u201e]/g, '"'],
  [/[\u2013\u2014\u2012\u2015\u2212\u2500]/g, "-"],
  [/\u00a0/g, " "],
];

function normalizeConfusables(text: string) {
  let out = text;
  for (const [pattern, replacement] of CONFUSABLE_MAP) {
    out = out.replace(pattern, replacement);
  }
  return out;
}

/** 复刻 pi edit 的入参归一（edits 可能是 JSON 字符串；legacy 单段字段）。 */
export function normalizeEditList(input: any): Array<{ oldText: string; newText: string }> {
  if (!input || typeof input !== "object") return [];
  let edits = input.edits;
  if (typeof edits === "string") {
    try {
      const parsed = JSON.parse(edits);
      if (Array.isArray(parsed)) edits = parsed;
    } catch {
      // 无法解析就当没有
    }
  }
  const list = Array.isArray(edits)
    ? edits.filter((item) => typeof item?.oldText === "string" && typeof item?.newText === "string")
    : [];
  if (list.length === 0 && typeof input.oldText === "string" && typeof input.newText === "string") {
    list.push({ oldText: input.oldText, newText: input.newText });
  }
  return list;
}

function lineOfOffset(content: string, index: number) {
  let line = 1;
  for (let i = 0; i < index && i < content.length; i++) {
    if (content[i] === "\n") line++;
  }
  return line;
}

function findOccurrenceLines(content: string, needle: string, { cap = 10 } = {}) {
  const lines: number[] = [];
  let from = 0;
  while (lines.length < cap) {
    const index = content.indexOf(needle, from);
    if (index === -1) break;
    lines.push(lineOfOffset(content, index));
    from = index + Math.max(needle.length, 1);
  }
  return lines;
}

function nearestMatchHint(content: string, oldText: string): string | null {
  const tokens = ((oldText.match(/[A-Za-z0-9_\u3400-\u9fff]{4,}/g) ?? []) as string[])
    .sort((a, b) => b.length - a.length);
  if (tokens.length === 0) return null;
  const needle = tokens[0].toLowerCase();
  const lines = content.split("\n");
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].toLowerCase().includes(needle)) {
      const snippet = lines[i].trim().slice(0, 200);
      return `Nearest match: line ${i + 1}: ${snippet}`;
    }
  }
  return null;
}

/**
 * 读取文件内容并生成完整的增强消息。
 * @returns 增强后的消息；无法增强时返回 null（调用方保留原错误）。
 */
export async function buildEditErrorHintWithFile(
  message: string,
  absolutePath: string,
  edits: Array<{ oldText: string; newText: string }>,
): Promise<string | null> {
  const duplicate = message.match(/^Found (\d+) occurrences of (?:the text|edits\[(\d+)\]) in/m);
  const notFound = message.match(/^Could not find (?:the exact text|edits\[(\d+)\]) in/m);
  if (!duplicate && !notFound) return null;
  const editIndex = duplicate?.[2] !== undefined
    ? Number(duplicate[2])
    : notFound?.[1] !== undefined
      ? Number(notFound[1])
      : 0;
  const oldText = edits[editIndex]?.oldText ?? edits[0]?.oldText;
  if (typeof oldText !== "string" || oldText.length === 0) return null;

  let content: string;
  try {
    content = normalizeToLF((await fsReadFile(absolutePath, "utf-8")).toString());
  } catch {
    return null;
  }

  if (duplicate) {
    const lines = findOccurrenceLines(content, oldText);
    if (lines.length === 0) return null;
    const list = lines.join(", ");
    return `${message}\n\nOccurrences of the text start at line${lines.length === 1 ? "" : "s"} [${list}]. Include more surrounding lines in oldText to make it unique.`;
  }

  // not-found：先查易混字符——归一化后能在文件里找到才提示，绝不误导。
  // 典型场景：模型打纯 ASCII，文件里是弯引号/长破折号；文件侧归一化后才对得上。
  const normalizedFile = normalizeConfusables(content);
  const normalizedOld = normalizeConfusables(oldText);
  if (normalizedFile !== content || normalizedOld !== oldText) {
    const occurrences = findOccurrenceLines(normalizedFile, normalizedOld, { cap: 1 });
    if (occurrences.length > 0) {
      return `${message}\n\nNearest match: line ${occurrences[0]}. The file uses typographic variants (smart quotes, dashes or non-breaking spaces) that differ from your oldText — copy the exact characters from the file.`;
    }
  }
  const nearest = nearestMatchHint(content, oldText);
  if (nearest) {
    return `${message}\n\n${nearest}`;
  }
  return `${message}\n\nThe file may have changed since you last read it — re-read the file and retry with its exact current text.`;
}

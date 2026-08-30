import crypto from "node:crypto";
import { Readable } from "node:stream";

import ExcelJS from "exceljs";
import { JSDOM } from "jsdom";
import mammoth from "mammoth";

import { KnowledgeError } from "./errors.ts";
import type { KnowledgeBlockDraft } from "./source-adapters.ts";

/**
 * ProcessingArtifact 管线（任务书 §五十八/§五十九）：
 * 二进制办公格式（DOCX/XLSX/CSV）先经 processor 转换为结构化文本 + 反向定位
 * 映射（locatorMap），再进入既有 Source → ContentSnapshot → ParseArtifact →
 * Block 事实链。processor 输出的每一行对应一个 block（空块不产生），保证
 * 「输出文本第 N 行 = locatorMap 第 N 项 = 原始文档第 N 个结构单元」。
 *
 * fidelity 纪律（§五十九）：这些格式只有结构级定位（段落序号 / 单元格坐标），
 * 一律标 structural，绝不冒称 citation_grade。PPTX/EPUB/旧版 DOC/XLS 与 OCR
 * 当前没有可用 processor，导入时显式拒绝（KNOWLEDGE_IMPORT_PROCESSOR_UNAVAILABLE），
 * 不得静默降级为无定位的纯文本。
 */
export const KNOWLEDGE_PROCESSOR_VERSION = "1";

export type KnowledgeProcessorFidelity = "citation_grade" | "structural" | "semantic_only";

export type KnowledgeProcessorPlan = {
  processorId: string;
  processorVersion: string;
  processorConfigHash: string;
};

export type KnowledgeProcessorOutput = {
  plan: KnowledgeProcessorPlan;
  fidelity: KnowledgeProcessorFidelity;
  outputMime: "text/plain";
  /** 一行一个 block 的纯文本输出。 */
  output: Buffer;
  /** block ordinal → 原始文档结构定位（段落序号 / 单元格坐标）。 */
  locatorMap: Record<number, Record<string, unknown>>;
  blocks: KnowledgeBlockDraft[];
  warnings: string[];
};

export const PROCESSOR_MIME_DOCX =
  "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
export const PROCESSOR_MIME_XLSX =
  "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
export const PROCESSOR_MIME_CSV = "text/csv";

/** 单元格/行数防护上限：超限截断并显式 warning，不静默丢内容。 */
export const KNOWLEDGE_PROCESSOR_MAX_CELLS = 100_000;
export const KNOWLEDGE_PROCESSOR_MAX_ROWS = 50_000;

function processorConfigHash(processorId: string): string {
  return crypto.createHash("sha256")
    .update(JSON.stringify({
      processorId,
      version: KNOWLEDGE_PROCESSOR_VERSION,
      maxCells: KNOWLEDGE_PROCESSOR_MAX_CELLS,
      maxRows: KNOWLEDGE_PROCESSOR_MAX_ROWS,
    }), "utf8")
    .digest("hex");
}

/** 按快照 MIME 解析 processor 身份；citation-grade 四类返回 null（不走 processor）。 */
export function resolveKnowledgeProcessor(mimeType: string): KnowledgeProcessorPlan | null {
  const normalized = typeof mimeType === "string" ? mimeType.toLowerCase() : "";
  const processorId = normalized === PROCESSOR_MIME_DOCX
    ? "lingxi-office-docx"
    : normalized === PROCESSOR_MIME_XLSX
      ? "lingxi-office-xlsx"
      : normalized === PROCESSOR_MIME_CSV
        ? "lingxi-office-csv"
        : null;
  if (!processorId) return null;
  return {
    processorId,
    processorVersion: KNOWLEDGE_PROCESSOR_VERSION,
    processorConfigHash: processorConfigHash(processorId),
  };
}

function cellText(value: unknown): string {
  if (value == null) return "";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  if (value instanceof Date) return value.toISOString();
  if (typeof value === "object") {
    const record = value as Record<string, unknown>;
    if (Array.isArray(record.richText)) {
      return record.richText
        .map(part => (part && typeof part === "object" ? String((part as { text?: unknown }).text ?? "") : ""))
        .join("");
    }
    if (typeof record.text === "string") return record.text;
    if (record.result != null) return cellText(record.result);
    if (typeof record.formula === "string") return "";
  }
  return String(value);
}

function assembleOutput(
  entries: Array<{ text: string; source: Record<string, unknown> }>,
): Omit<KnowledgeProcessorOutput, "plan" | "fidelity" | "warnings"> {
  const locatorMap: Record<number, Record<string, unknown>> = {};
  const blocks: KnowledgeBlockDraft[] = [];
  const lines: string[] = [];
  let cursor = 0;
  for (const entry of entries) {
    const text = entry.text.replace(/\s+/gu, " ").trim();
    if (!text) continue;
    const ordinal = blocks.length;
    locatorMap[ordinal] = entry.source;
    blocks.push({
      ordinal,
      text,
      locatorType: "text",
      locator: {
        lineStart: ordinal + 1,
        lineEnd: ordinal + 1,
        charStart: cursor,
        charEnd: cursor + text.length,
        source: entry.source,
      },
    });
    lines.push(text);
    cursor += text.length + 1;
  }
  return { outputMime: "text/plain", output: Buffer.from(lines.join("\n"), "utf8"), locatorMap, blocks };
}

const DOCX_BLOCK_SELECTOR = "h1,h2,h3,h4,h5,h6,p,li,blockquote,pre,td,th";

async function processDocx(bytes: Buffer): Promise<Omit<KnowledgeProcessorOutput, "plan">> {
  const converted = await mammoth.convertToHtml({ buffer: bytes });
  const warnings = (converted.messages || [])
    .map(message => String((message as { message?: unknown })?.message ?? ""))
    .filter(message => message.length > 0)
    .slice(0, 100);
  const dom = new JSDOM(converted.value, { contentType: "text/html" });
  const entries: Array<{ text: string; source: Record<string, unknown> }> = [];
  const headingPath: string[] = [];
  try {
    const document = dom.window.document;
    document.querySelectorAll("script,style,noscript,template").forEach(node => node.remove());
    for (const element of document.querySelectorAll(DOCX_BLOCK_SELECTOR)) {
      if (element.querySelector(DOCX_BLOCK_SELECTOR)) continue;
      const text = (element.textContent || "").replace(/\s+/gu, " ").trim();
      if (!text) continue;
      const heading = element.localName.match(/^h([1-6])$/u);
      if (heading) {
        const level = Number(heading[1]);
        headingPath.splice(level - 1);
        headingPath[level - 1] = text;
        headingPath.splice(level);
      }
      entries.push({
        text,
        source: {
          kind: "docx_paragraph",
          paragraphIndex: entries.length,
          tag: element.localName,
          ...(headingPath.length > 0 ? { headingPath: [...headingPath] } : {}),
        },
      });
    }
  } finally {
    dom.window.close();
  }
  return { ...assembleOutput(entries), fidelity: "structural", warnings };
}

type SheetRowEntry = { text: string; source: Record<string, unknown> };

function collectSheetEntries(
  workbook: ExcelJS.Workbook,
  warnings: string[],
): SheetRowEntry[] {
  const entries: SheetRowEntry[] = [];
  let cellCount = 0;
  let rowCount = 0;
  let truncated = false;
  for (const worksheet of workbook.worksheets) {
    worksheet.eachRow({ includeEmpty: false }, (row, rowNumber) => {
      if (truncated) return;
      rowCount += 1;
      if (rowCount > KNOWLEDGE_PROCESSOR_MAX_ROWS) {
        truncated = true;
        warnings.push(`row_count_truncated_at_${KNOWLEDGE_PROCESSOR_MAX_ROWS}`);
        return;
      }
      const cells: Array<{ ref: string; text: string }> = [];
      row.eachCell({ includeEmpty: false }, (cell) => {
        cellCount += 1;
        if (cellCount > KNOWLEDGE_PROCESSOR_MAX_CELLS) {
          truncated = true;
          warnings.push(`cell_count_truncated_at_${KNOWLEDGE_PROCESSOR_MAX_CELLS}`);
          return;
        }
        const text = cellText(cell.value).replace(/\s+/gu, " ").trim();
        if (text) cells.push({ ref: String(cell.address), text });
      });
      if (truncated || cells.length === 0) return;
      entries.push({
        text: cells.map(cell => cell.text).join(" | "),
        source: {
          kind: "sheet_row",
          sheet: worksheet.name,
          row: rowNumber,
          cells: cells.map(cell => cell.ref),
        },
      });
    });
  }
  return entries;
}

async function processXlsx(bytes: Buffer): Promise<Omit<KnowledgeProcessorOutput, "plan">> {
  const warnings: string[] = [];
  const workbook = new ExcelJS.Workbook();
  await workbook.xlsx.load(bytes as unknown as ArrayBuffer);
  const entries = collectSheetEntries(workbook, warnings);
  return { ...assembleOutput(entries), fidelity: "structural", warnings };
}

async function processCsv(bytes: Buffer): Promise<Omit<KnowledgeProcessorOutput, "plan">> {
  const warnings: string[] = [];
  const workbook = new ExcelJS.Workbook();
  const sheet = await workbook.csv.read(Readable.from([bytes]));
  const entries: SheetRowEntry[] = [];
  let cellCount = 0;
  let truncated = false;
  sheet.eachRow({ includeEmpty: false }, (row, rowNumber) => {
    if (truncated) return;
    if (rowNumber > KNOWLEDGE_PROCESSOR_MAX_ROWS) {
      truncated = true;
      warnings.push(`row_count_truncated_at_${KNOWLEDGE_PROCESSOR_MAX_ROWS}`);
      return;
    }
    const cells: Array<{ ref: string; text: string }> = [];
    row.eachCell({ includeEmpty: false }, (cell, colNumber) => {
      cellCount += 1;
      if (cellCount > KNOWLEDGE_PROCESSOR_MAX_CELLS) {
        truncated = true;
        warnings.push(`cell_count_truncated_at_${KNOWLEDGE_PROCESSOR_MAX_CELLS}`);
        return;
      }
      const text = cellText(cell.value).replace(/\s+/gu, " ").trim();
      if (text) cells.push({ ref: `R${rowNumber}C${colNumber}`, text });
    });
    if (truncated || cells.length === 0) return;
    entries.push({
      text: cells.map(cell => cell.text).join(" | "),
      source: {
        kind: "csv_row",
        sheet: sheet.name,
        row: rowNumber,
        cells: cells.map(cell => cell.ref),
      },
    });
  });
  return { ...assembleOutput(entries), fidelity: "structural", warnings };
}

/**
 * 复用路径（ProcessingArtifact 已 ready）：从持久化的输出文本 + locatorMap 重建
 * blocks，与 assembleOutput 的不变量保持一致（一行一 block、无空块、
 * 第 N 行 = locatorMap 第 N 项）。输出文本无 trailing newline，按 "\n" 切分即还原。
 */
export function rebuildBlocksFromProcessorOutput(input: {
  output: Buffer;
  locatorMap: Record<string, unknown>;
}): KnowledgeBlockDraft[] {
  const lines = input.output.toString("utf8").split("\n");
  const blocks: KnowledgeBlockDraft[] = [];
  let cursor = 0;
  for (const line of lines) {
    if (!line) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Processor output line alignment is broken");
    }
    const ordinal = blocks.length;
    const source = (input.locatorMap as Record<number, Record<string, unknown> | undefined>)[ordinal];
    if (!source) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Processor locator map is incomplete");
    }
    blocks.push({
      ordinal,
      text: line,
      locatorType: "text",
      locator: {
        lineStart: ordinal + 1,
        lineEnd: ordinal + 1,
        charStart: cursor,
        charEnd: cursor + line.length,
        source,
      },
    });
    cursor += line.length + 1;
  }
  return blocks;
}

/**
 * 执行 processor 转换。抛 KnowledgeError 之外的异常由调用方归类为处理失败；
 * 永不返回空 blocks（调用方按 empty_document 处理）。
 */
export async function processKnowledgeSnapshot(input: {
  mimeType: string;
  bytes: Buffer;
}): Promise<KnowledgeProcessorOutput> {
  const plan = resolveKnowledgeProcessor(input?.mimeType);
  if (!plan) {
    throw new KnowledgeError(
      "KNOWLEDGE_IMPORT_PROCESSOR_UNAVAILABLE",
      "No knowledge processor is registered for this format",
    );
  }
  const processed = plan.processorId === "lingxi-office-docx"
    ? await processDocx(input.bytes)
    : plan.processorId === "lingxi-office-xlsx"
      ? await processXlsx(input.bytes)
      : await processCsv(input.bytes);
  return { plan, ...processed };
}

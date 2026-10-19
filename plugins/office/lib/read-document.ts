import fs from "fs/promises";
import path from "path";

import { readPdfDocument } from "./read-pdf.ts";

const TEXT_EXTS = new Set([".txt", ".md", ".markdown", ".csv", ".tsv", ".html", ".htm"]);
const DOCX_EXTS = new Set([".docx"]);
const XLSX_EXTS = new Set([".xlsx", ".xlsm"]);
const PDF_EXTS = new Set([".pdf"]);
const DEFAULT_MAX_CHARS = 20000;
const DEFAULT_SHEET_LIMIT = 8;
const DEFAULT_ROW_LIMIT = 200;
const DEFAULT_COLUMN_LIMIT = 50;

function asPositiveInt(value, fallback, max) {
  const n = Number(value);
  if (!Number.isFinite(n) || n <= 0) return fallback;
  return Math.min(Math.floor(n), max);
}

function normalizePath(input) {
  const filePath = typeof input?.filePath === "string" ? input.filePath.trim() : "";
  if (!filePath) throw new Error("office_read-document requires resource or filePath");
  if (!path.isAbsolute(filePath)) {
    throw new Error("office_read-document requires an absolute filePath");
  }
  return filePath;
}

async function resolveInputPath(input, options: any = {}) {
  if (input?.resource) {
    const resourceIO = options?.resources || options?.resourceIO || null;
    if (!resourceIO || typeof resourceIO.materialize !== "function") {
      const error = new Error("office_read-document resource input requires ctx.resources.materialize");
      (error as any).code = "OFFICE_RESOURCE_MATERIALIZE_UNAVAILABLE";
      throw error;
    }
    const materialized = await resourceIO.materialize(input.resource);
    const filePath = typeof materialized?.filePath === "string" ? materialized.filePath.trim() : "";
    if (!filePath || !path.isAbsolute(filePath)) {
      const error = new Error("office_read-document materialize result must include an absolute filePath");
      (error as any).code = "OFFICE_RESOURCE_MATERIALIZE_INVALID";
      throw error;
    }
    return {
      filePath,
      resourceKey: materialized?.resourceKey || null,
      resource: materialized?.resource || input.resource,
    };
  }
  return {
    filePath: normalizePath(input),
    resourceKey: null,
    resource: null,
  };
}

function truncateText(text, maxChars) {
  if (text.length <= maxChars) return { text, truncated: false };
  return {
    text: text.slice(0, maxChars) + `\n[Truncated at ${maxChars} characters]`,
    truncated: true,
  };
}

function cellText(cell) {
  return String(cell?.text ?? "").replace(/[\t\r\n]+/g, " ").trim();
}

async function readDocx(filePath, outputFormat) {
  const mammoth = (await import("mammoth")).default;
  const source = Buffer.isBuffer(filePath) ? { buffer: filePath } : { path: filePath };
  if (outputFormat === "html") {
    const result = await mammoth.convertToHtml(source);
    return {
      kind: "docx",
      format: "html",
      content: result.value,
      warnings: result.messages || [],
    };
  }
  const result = await mammoth.extractRawText(source);
  return {
    kind: "docx",
    format: "text",
    content: result.value,
    warnings: result.messages || [],
  };
}

async function readXlsx(filePath, input) {
  const ExcelJS = (await import("exceljs")).default;
  const workbook = new ExcelJS.Workbook();
  if (Buffer.isBuffer(filePath)) await workbook.xlsx.load(Uint8Array.from(filePath).buffer);
  else await workbook.xlsx.readFile(filePath);

  const sheetLimit = asPositiveInt(input.sheetLimit, DEFAULT_SHEET_LIMIT, 50);
  const rowLimit = asPositiveInt(input.rowLimit, DEFAULT_ROW_LIMIT, 5000);
  const columnLimit = asPositiveInt(input.columnLimit, DEFAULT_COLUMN_LIMIT, 500);
  const sheets = workbook.worksheets.slice(0, sheetLimit).map((sheet) => {
    const rows = [];
    const maxRows = Math.min(sheet.rowCount, rowLimit);
    const maxColumns = Math.min(sheet.columnCount, columnLimit);
    for (let r = 1; r <= maxRows; r += 1) {
      const row = sheet.getRow(r);
      if (!row.hasValues) continue;
      const values = [];
      for (let c = 1; c <= maxColumns; c += 1) {
        values.push(cellText(row.getCell(c)));
      }
      rows.push(values);
    }
    return {
      name: sheet.name,
      rowCount: sheet.rowCount,
      columnCount: sheet.columnCount,
      truncatedRows: sheet.rowCount > rowLimit,
      truncatedColumns: sheet.columnCount > columnLimit,
      rows,
    };
  });

  const truncatedSheets = workbook.worksheets.length > sheetLimit;
  if (input.outputFormat === "json") {
    return {
      kind: "xlsx",
      format: "json",
      workbook: {
        sheetCount: workbook.worksheets.length,
        truncatedSheets,
        sheets,
      },
    };
  }

  const lines = [];
  for (const sheet of sheets) {
    lines.push(`[Sheet: ${sheet.name}]`);
    lines.push(`Rows: ${sheet.rowCount}`);
    lines.push(`Columns: ${sheet.columnCount}`);
    for (const row of sheet.rows) {
      lines.push(row.join(" | "));
    }
    if (sheet.truncatedRows || sheet.truncatedColumns) {
      lines.push(`[Sheet truncated: rows=${sheet.truncatedRows}, columns=${sheet.truncatedColumns}]`);
    }
    lines.push("");
  }
  if (truncatedSheets) {
    lines.push(`[Workbook truncated: showing ${sheetLimit}/${workbook.worksheets.length} sheets]`);
  }

  return {
    kind: "xlsx",
    format: "text",
    content: lines.join("\n"),
  };
}

async function readTextLike(filePath, ext) {
  const content = Buffer.isBuffer(filePath) ? filePath.toString("utf-8") : await fs.readFile(filePath, "utf-8");
  return {
    kind: ext === ".html" || ext === ".htm" ? "html" : "text",
    format: "text",
    content,
  };
}

export async function readOfficeDocument(input: any = {}, options: any = {}) {
  // 正式外围 worker 只获得宿主已授权的字节；原路径入口保持现有行为。
  const { filePath, resourceKey, resource } = Buffer.isBuffer(input.bytes)
    ? { filePath: String(input.filename || "document"), resourceKey: null, resource: null }
    : await resolveInputPath(input, options);
  const source = Buffer.isBuffer(input.bytes) ? input.bytes : filePath;
  const ext = path.extname(filePath).toLowerCase();
  const outputFormat = input.outputFormat === "html" || input.outputFormat === "json" || input.outputFormat === "markdown"
    ? input.outputFormat
    : "text";
  const maxChars = asPositiveInt(input.maxChars, DEFAULT_MAX_CHARS, 200000);

  let result;
  if (DOCX_EXTS.has(ext)) {
    if (outputFormat === "json" || outputFormat === "markdown") {
      throw new Error("docx JSON/Markdown output is not supported; use text or html");
    }
    result = await readDocx(source, outputFormat);
  } else if (XLSX_EXTS.has(ext)) {
    if (outputFormat === "html" || outputFormat === "markdown") {
      throw new Error("xlsx HTML/Markdown output is not supported by office_read-document; use text or json");
    }
    result = await readXlsx(source, { ...input, outputFormat });
  } else if (PDF_EXTS.has(ext)) {
    if (outputFormat === "html" || outputFormat === "json") {
      throw new Error("pdf HTML/JSON output is not supported; use markdown or text");
    }
    result = await readPdfDocument(source, { ...input, outputFormat, maxChars });
  } else if (TEXT_EXTS.has(ext) || !ext) {
    if (outputFormat === "html" || outputFormat === "json" || outputFormat === "markdown") {
      throw new Error(`${ext || "text"} HTML/JSON/Markdown output is not supported; use text`);
    }
    result = await readTextLike(source, ext);
  } else {
    throw new Error(`unsupported Office read format "${ext || "(none)"}"; supported: .docx, .xlsx, .xlsm, .pdf, .txt, .md, .csv, .tsv, .html`);
  }

  if (typeof result.content === "string" && result.kind !== "pdf") {
    const truncated = truncateText(result.content, maxChars);
    result.content = truncated.text;
    result.truncated = truncated.truncated;
  }

  return {
    filePath,
    ...(resourceKey ? { resourceKey } : {}),
    ...(resource ? { resource } : {}),
    filename: path.basename(filePath),
    ext,
    ...result,
  };
}

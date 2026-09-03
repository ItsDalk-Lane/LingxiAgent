import fs from "fs";
import path from "path";
import { loadAnydoc, type AnydocApi } from "./anydoc-loader.ts";
import type { ExtractResult } from "./types.ts";
import { trimRenderedPage } from "./trim-page.ts";

export type { AnydocApi } from "./anydoc-loader.ts";
export type { ExtractFailure, ExtractFailureReason, ExtractResult, ExtractSuccess } from "./types.ts";

/** 单个文档的输入上限。超过这个体量的文件转成 Markdown 已经没法喂进模型上下文了。 */
export const MAX_INPUT_BYTES = 50 * 1024 * 1024;

/** 报错时告诉调用方哪些文档类型走得通，避免对方靠猜。 */
const SUPPORTED_FORMAT_HINT = "docx, pdf, xlsx, pptx, odt, ods, odp, rtf, epub, csv, html";

// 扫描版 PDF 只有图片没有文字层，转换会直接失败。上游没有给出稳定的错误码，只能按错误文案
// 归类，好让调用方知道"这份文件需要 OCR"而不是"文件坏了"。
const SCANNED_PDF_MESSAGE = /scan|image[- ]only|ocr|unsupported/i;

export interface ExtractInput {
  buffer?: Buffer;
  filePath?: string;
  filename?: string;
  ocrModelId?: string;
  ocrLanguage?: string;
  ocrMaxPages?: number;
  ocrMaxPixelsPerPage?: number;
  signal?: AbortSignal;
}

export interface ExtractDeps {
  loadApi?: () => Promise<AnydocApi>;
  ocr?: (input: {
    image: Uint8Array;
    mime: string;
    modelId?: string;
    language?: string;
    signal?: AbortSignal;
  }) => Promise<any>;
  renderPdfPages?: (bytes: Uint8Array, options: {
    maxPages: number;
    maxPixelsPerPage: number;
    signal?: AbortSignal;
  }) => Promise<Array<{ pageNumber: number; image: Uint8Array; mime: string }>>;
}

const IMAGE_EXT_MIME = new Map([
  [".png", "image/png"],
  [".jpg", "image/jpeg"],
  [".jpeg", "image/jpeg"],
  [".webp", "image/webp"],
  [".tif", "image/tiff"],
  [".tiff", "image/tiff"],
]);

function tooLarge(size: number): ExtractResult {
  return {
    ok: false,
    reason: "too-large",
    message: `document is ${size} bytes, above the ${MAX_INPUT_BYTES} byte extraction limit`,
  };
}

/** 扩展名统一去掉前导点、转小写后再交给上游查表。 */
function formatFromName(api: AnydocApi, name: string | null): string | null {
  if (!name) return null;
  const ext = path.extname(name).replace(/^\./, "").toLowerCase();
  if (!ext) return null;
  return api.formatFromExtension(ext) || null;
}

/**
 * 把一份文档转成 Markdown 文本。
 *
 * 只认字节，不碰 engine / session，任何调用方喂 buffer 或本地路径都能用；
 * 授权校验属于调用方的责任，这里不做路径判断。
 *
 * 读盘失败（文件不存在、没权限）会原样抛出，不会伪装成解析失败——那是调用方传错了路径，
 * 不是文档本身的问题。文档层面的问题一律用 ExtractResult 表达。
 */
export async function extractDocument(input: ExtractInput, deps: ExtractDeps = {}): Promise<ExtractResult> {
  const { buffer, filePath, filename } = input ?? {};
  if (!buffer && !filePath) throw new Error("extractDocument requires buffer or filePath");

  let bytes: Buffer;
  if (buffer) {
    if (buffer.length > MAX_INPUT_BYTES) return tooLarge(buffer.length);
    bytes = buffer;
  } else {
    // 先 stat 再读：超大文件不该被整个装进内存才发现装不下。
    const stat = await fs.promises.stat(filePath!);
    if (stat.size > MAX_INPUT_BYTES) return tooLarge(stat.size);
    bytes = await fs.promises.readFile(filePath!);
  }

  const imageMime = detectImageMime(bytes, filename ?? filePath ?? null);
  if (imageMime) return runOcr([{ pageNumber: 1, image: bytes, mime: imageMime }], input, deps);

  const api = await (deps.loadApi ? deps.loadApi() : loadAnydoc());
  const format = api.formatFromBytes(bytes) || formatFromName(api, filename ?? filePath ?? null);
  if (!format) {
    return {
      ok: false,
      reason: "unsupported",
      message: `could not identify the document format from its bytes or filename; supported formats include ${SUPPORTED_FORMAT_HINT}`,
    };
  }

  try {
    const markdown = await api.toMarkdownBytes(bytes, format);
    const text = typeof markdown === "string" ? markdown : "";
    const warnings = text.trim() ? [] : ["document parsed successfully but contained no text"];
    return { ok: true, markdown: text, format, warnings };
  } catch (err) {
    const message = (err as any)?.message || String(err);
    if (format === "pdf" && SCANNED_PDF_MESSAGE.test(message)) {
      if (deps.ocr) {
        try {
          const render = deps.renderPdfPages ?? renderPdfPages;
          const pages = await render(bytes, {
            maxPages: boundedInteger(input.ocrMaxPages, 1, 100, 25),
            maxPixelsPerPage: boundedInteger(input.ocrMaxPixelsPerPage, 1_000_000, 100_000_000, 16_000_000),
            signal: input.signal,
          });
          return await runOcr(pages, input, deps);
        } catch (ocrError) {
          return {
            ok: false,
            reason: "ocr-unavailable",
            message: ocrError instanceof Error ? ocrError.message : String(ocrError),
          };
        }
      }
      return { ok: false, reason: "scanned-pdf", message };
    }
    return { ok: false, reason: "parse-failed", message };
  }
}

async function runOcr(
  pages: Array<{ pageNumber: number; image: Uint8Array; mime: string }>,
  input: ExtractInput,
  deps: ExtractDeps,
): Promise<ExtractResult> {
  if (!deps.ocr) {
    return { ok: false, reason: "ocr-unavailable", message: "local OCR is not configured" };
  }
  if (pages.length === 0) {
    return { ok: false, reason: "parse-failed", message: "document contains no renderable pages" };
  }
  const blocks: string[] = [];
  const warnings = new Set<string>();
  for (const page of pages) {
    if (input.signal?.aborted) throw input.signal.reason ?? new DOMException("Aborted", "AbortError");
    const result = await deps.ocr({
      image: page.image,
      mime: page.mime,
      modelId: input.ocrModelId,
      language: input.ocrLanguage,
      signal: input.signal,
    });
    const output = result?.output ?? result;
    const markdown = typeof output?.markdown === "string" && output.markdown.trim()
      ? output.markdown.trim()
      : typeof output?.text === "string" ? output.text.trim() : "";
    if (!markdown) throw new Error(`OCR returned no text for page ${page.pageNumber}`);
    blocks.push(pages.length > 1 ? `## Page ${page.pageNumber}\n\n${markdown}` : markdown);
    const modelId = typeof result?.modelId === "string" ? result.modelId : input.ocrModelId;
    if (modelId) warnings.add(`ocr:${modelId}`);
    for (const warning of output?.warnings || []) warnings.add(String(warning));
  }
  return { ok: true, markdown: blocks.join("\n\n"), format: "ocr", warnings: [...warnings] };
}

function detectImageMime(bytes: Buffer, name: string | null): string | null {
  if (bytes.length >= 8 && bytes.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))) return "image/png";
  if (bytes.length >= 3 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff) return "image/jpeg";
  if (bytes.length >= 12 && bytes.toString("ascii", 0, 4) === "RIFF" && bytes.toString("ascii", 8, 12) === "WEBP") return "image/webp";
  if (bytes.length >= 4) {
    const signature = bytes.toString("hex", 0, 4);
    if (signature === "49492a00" || signature === "4d4d002a") return "image/tiff";
  }
  return name ? IMAGE_EXT_MIME.get(path.extname(name).toLowerCase()) ?? null : null;
}

async function renderPdfPages(
  bytes: Uint8Array,
  options: { maxPages: number; maxPixelsPerPage: number; signal?: AbortSignal },
) {
  const { definePDFJSModule, getDocumentProxy, renderPageAsImage } = await import("unpdf");
  await definePDFJSModule(() => import("pdfjs-dist/legacy/build/pdf.mjs"));
  const pdf = await getDocumentProxy(new Uint8Array(bytes));
  try {
    const count = Math.min(pdf.numPages, options.maxPages);
    const pages: Array<{ pageNumber: number; image: Uint8Array; mime: string }> = [];
    for (let pageNumber = 1; pageNumber <= count; pageNumber += 1) {
      if (options.signal?.aborted) throw options.signal.reason ?? new DOMException("Aborted", "AbortError");
      const page = await pdf.getPage(pageNumber);
      const viewport = page.getViewport({ scale: 1 });
      const basePixels = Math.max(1, viewport.width * viewport.height);
      const scale = Math.min(2, Math.sqrt(options.maxPixelsPerPage / basePixels));
      const rendered = await renderPageAsImage(pdf, pageNumber, {
        canvasImport: () => import("@napi-rs/canvas"),
        scale,
      });
      pages.push({ pageNumber, image: await trimRenderedPage(new Uint8Array(rendered), options.signal), mime: "image/png" });
    }
    return pages;
  } finally {
    await (pdf as unknown as { destroy?: () => Promise<void> | void }).destroy?.();
  }
}

function boundedInteger(value: unknown, min: number, max: number, fallback: number): number {
  return Number.isSafeInteger(value) && Number(value) >= min && Number(value) <= max ? Number(value) : fallback;
}

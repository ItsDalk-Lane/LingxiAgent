import { AsyncLocalStorage } from "async_hooks";
import path from "path";
import { createLsTool, createReadTool } from "../pi-sdk/index.ts";
import { boundedSearchPresentation } from "../pi-sdk/search-presentation.ts";
import type { ToolReadPresentation, ToolSearchPresentation } from "../../shared/tool-presentation.ts";

type ReadOperations = {
  readFile: (filePath: string) => Promise<Buffer>;
  access: (filePath: string) => Promise<void>;
  detectImageMimeType?: (filePath: string) => Promise<string | undefined>;
};

const IMAGE_EXTENSIONS = new Set([".jpg", ".jpeg", ".png", ".gif", ".webp", ".bmp", ".ico", ".tiff", ".tif"]);
const EXTRACTED_DOCUMENT_EXTENSIONS = new Set([".docx", ".xlsx"]);

export function createPresentedReadTool(cwd: string, operations: ReadOperations) {
  const active = new AsyncLocalStorage<{ read?: ToolReadPresentation; image?: boolean }>();
  const tool = createReadTool(cwd, { operations: {
    ...operations,
    ...(operations.detectImageMimeType ? { detectImageMimeType: async (filePath: string) => {
      const mime = await operations.detectImageMimeType!(filePath);
      const capture = active.getStore();
      if (capture && mime) capture.image = true;
      return mime;
    } } : {}),
    readFile: async (filePath: string) => {
      const buffer = await operations.readFile(filePath);
      const capture = active.getStore();
      const extension = path.extname(filePath).toLowerCase();
      if (capture && !capture.image && !IMAGE_EXTENSIONS.has(extension)) {
        // 统计增强读取后的同一份文本；空文件没有可展示行，其余换行规则沿用 SDK。
        let totalLines = buffer.length === 0 ? 0 : 1;
        for (let at = buffer.indexOf(10); at !== -1; at = buffer.indexOf(10, at + 1)) totalLines++;
        capture.read = {
          path: filePath,
          startLine: 1,
          totalLines,
          ...(extension ? { language: EXTRACTED_DOCUMENT_EXTENSIONS.has(extension) ? "text" : extension.slice(1) } : {}),
        };
      }
      return buffer;
    },
  } });
  return {
    ...tool,
    execute: async (...args: Parameters<typeof tool.execute>) => {
      const capture: { read?: ToolReadPresentation; image?: boolean } = {};
      const result = await active.run(capture, () => tool.execute(...args));
      if (!capture.read || capture.image || result.content.some(block => block.type === "image")) return result;
      const params = args[1];
      const startLine = params.offset ? Math.max(1, params.offset) : 1;
      const available = Math.max(0, (capture.read.totalLines ?? 0) - startLine + 1);
      const selected = params.limit === undefined ? available : Math.max(0, Math.min(available, params.limit));
      const truncation = result.details?.truncation;
      const displayedLines = truncation?.firstLineExceedsLimit ? 0 : truncation?.outputLines ?? selected;
      const read: ToolReadPresentation = {
        ...capture.read,
        startLine,
        displayedLines,
        truncated: startLine > 1 || displayedLines < (capture.read.totalLines ?? 0) || truncation?.truncated === true,
      };
      return { ...result, details: { ...result.details, read } };
    },
  };
}

type LsOperations = {
  exists: (filePath: string) => Promise<boolean>;
  stat: (filePath: string) => Promise<{ isDirectory: () => boolean }>;
  readdir: (dirPath: string) => Promise<string[]>;
};

export function createPresentedLsTool(cwd: string, operations: LsOperations) {
  type Capture = { dirPath?: string; files: NonNullable<ToolSearchPresentation["files"]>; fileCount: number; bytes: number; truncated: boolean };
  const active = new AsyncLocalStorage<Capture>();
  const tool = createLsTool(cwd, { operations: {
    ...operations,
    readdir: async (dirPath: string) => {
      const entries = await operations.readdir(dirPath);
      const capture = active.getStore();
      if (capture) capture.dirPath = dirPath;
      return entries;
    },
    stat: async (filePath: string) => {
      const stat = await operations.stat(filePath);
      const capture = active.getStore();
      // SDK 只对实际输出的条目逐项 stat；失败和超过条数上限的条目不会进入记录。
      if (capture?.dirPath && path.dirname(filePath) === capture.dirPath) {
        capture.fileCount++;
        const entryPath = path.basename(filePath) + (stat.isDirectory() ? "/" : "");
        const bytes = Buffer.byteLength(entryPath) + 32;
        if (!capture.truncated && capture.files.length < 1000 && capture.bytes + bytes <= 50 * 1024) {
          capture.files.push({ path: entryPath });
          capture.bytes += bytes;
        } else capture.truncated = true;
      }
      return stat;
    },
  } });
  return {
    ...tool,
    execute: async (...args: Parameters<typeof tool.execute>) => {
      const capture: Capture = { files: [], fileCount: 0, bytes: 0, truncated: false };
      const result = await active.run(capture, () => tool.execute(...args));
      const truncation = result.details?.truncation;
      const files = truncation?.truncated ? capture.files.slice(0, truncation.outputLines) : capture.files;
      const search = boundedSearchPresentation({
        kind: "ls",
        ...(capture.dirPath ? { basePath: capture.dirPath } : {}),
        files,
        fileCount: capture.fileCount,
        truncated: Boolean(capture.truncated || result.details?.entryLimitReached || truncation?.truncated),
      });
      return { ...result, details: { ...result.details, search } };
    },
  };
}

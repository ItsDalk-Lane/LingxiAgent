import fs from "node:fs";
import path from "node:path";

import {
  inspectLocalImportPath,
  LocalImportPathError,
} from "../../shared/file-import-security.ts";
import { KnowledgeError } from "./errors.ts";

export const DEFAULT_KNOWLEDGE_IMPORT_MAX_BYTES = 50 * 1024 * 1024;

export interface SecureImportFile {
  bytes: Buffer;
  realPath: string;
  fileName: string;
}

function sameOpenedFile(before: fs.Stats, opened: fs.Stats): boolean {
  // Windows 的 inode/dev 可能为 0；这时仍由真实路径、非软链接和打开后的
  // regular-file 检查兜底。其余平台同时核对设备与 inode，缩小竞态窗口。
  if (!before.dev || !before.ino || !opened.dev || !opened.ino) return true;
  return before.dev === opened.dev && before.ino === opened.ino;
}

async function readBounded(handle: fs.promises.FileHandle, maxBytes: number): Promise<Buffer> {
  const chunks: Buffer[] = [];
  let total = 0;
  const chunkSize = 64 * 1024;

  while (true) {
    const chunk = Buffer.allocUnsafe(Math.min(chunkSize, maxBytes - total + 1));
    const { bytesRead } = await handle.read(chunk, 0, chunk.length, null);
    if (bytesRead === 0) break;
    total += bytesRead;
    if (total > maxBytes) {
      throw new KnowledgeError(
        "KNOWLEDGE_IMPORT_TOO_LARGE",
        `Knowledge import exceeds the ${maxBytes} byte limit`,
        { maxBytes },
      );
    }
    chunks.push(chunk.subarray(0, bytesRead));
  }

  return Buffer.concat(chunks, total);
}

/**
 * 打开并读取一个外部普通文件。校验失败只返回稳定分类，不把用户绝对路径写进错误。
 */
export async function readSecureKnowledgeImportFile(options: {
  filePath: unknown;
  lingxiHome: string;
  maxBytes?: number;
}): Promise<SecureImportFile> {
  const maxBytes = options.maxBytes ?? DEFAULT_KNOWLEDGE_IMPORT_MAX_BYTES;
  if (!Number.isSafeInteger(maxBytes) || maxBytes <= 0) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "maxBytes must be a positive integer");
  }
  let inspected;
  try {
    inspected = await inspectLocalImportPath({
      filePath: options.filePath,
      lingxiHome: options.lingxiHome,
    });
  } catch (error) {
    if (!(error instanceof LocalImportPathError)) throw error;
    if (error.code === "NOT_FOUND") {
      throw new KnowledgeError("KNOWLEDGE_IMPORT_NOT_FOUND", "Knowledge import file was not found");
    }
    if (error.code === "SYMLINK") {
      throw new KnowledgeError("KNOWLEDGE_IMPORT_SYMLINK", "Symbolic links cannot be imported");
    }
    if (error.code === "PATH_BLOCKED") {
      throw new KnowledgeError("KNOWLEDGE_IMPORT_PATH_BLOCKED", "Knowledge import path is blocked");
    }
    if (error.code === "TYPE_UNSUPPORTED") {
      throw new KnowledgeError("KNOWLEDGE_IMPORT_FILE_REQUIRED", "Knowledge import requires a regular file");
    }
    throw new KnowledgeError("KNOWLEDGE_IMPORT_PATH_INVALID", "Knowledge import path must be absolute");
  }
  const realPath = inspected.realPath;
  const realStat = inspected.stat;
  if (realStat.size > maxBytes) {
    throw new KnowledgeError(
      "KNOWLEDGE_IMPORT_TOO_LARGE",
      `Knowledge import exceeds the ${maxBytes} byte limit`,
      { maxBytes },
    );
  }

  let handle: fs.promises.FileHandle | null = null;
  try {
    handle = await fs.promises.open(realPath, "r");
    const openedStat = await handle.stat();
    if (!openedStat.isFile()) {
      throw new KnowledgeError("KNOWLEDGE_IMPORT_FILE_REQUIRED", "Knowledge import requires a regular file");
    }
    if (!sameOpenedFile(realStat, openedStat)) {
      throw new KnowledgeError("KNOWLEDGE_IMPORT_PATH_INVALID", "Knowledge import file changed during validation");
    }
    const bytes = await readBounded(handle, maxBytes);
    return {
      bytes,
      realPath,
      fileName: path.basename(realPath),
    };
  } finally {
    await handle?.close().catch(() => {});
  }
}

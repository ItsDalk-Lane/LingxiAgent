import fs from "node:fs";
import path from "node:path";

import { isSensitivePath } from "./path-security.ts";

export type LocalImportPathErrorCode =
  | "PATH_INVALID"
  | "NOT_FOUND"
  | "SYMLINK"
  | "PATH_BLOCKED"
  | "TYPE_UNSUPPORTED";

export class LocalImportPathError extends Error {
  readonly code: LocalImportPathErrorCode;

  constructor(code: LocalImportPathErrorCode) {
    super(code);
    this.code = code;
    this.name = "LocalImportPathError";
  }
}

export interface InspectedLocalImportPath {
  realPath: string;
  stat: fs.Stats;
  kind: "file" | "directory";
}

/**
 * 普通上传和 Knowledge 都必须先通过这一道真实路径检查。
 * 这里只分类，不拼接用户路径到错误文案，调用方再换成自己的稳定错误码。
 */
export async function inspectLocalImportPath(options: {
  filePath: unknown;
  lingxiHome?: string;
  allowDirectories?: boolean;
}): Promise<InspectedLocalImportPath> {
  if (typeof options.filePath !== "string" || !path.isAbsolute(options.filePath)) {
    throw new LocalImportPathError("PATH_INVALID");
  }
  let inputStat: fs.Stats;
  try {
    inputStat = await fs.promises.lstat(options.filePath);
  } catch {
    throw new LocalImportPathError("NOT_FOUND");
  }
  if (inputStat.isSymbolicLink()) throw new LocalImportPathError("SYMLINK");

  let resolved: string;
  try {
    // 与 SessionFileRegistry 的路径身份保持同一套 JS realpath 语义，避免
    // macOS 大小写或 Windows 短文件名让同一来源产生两个身份。
    resolved = fs.realpathSync(options.filePath);
  } catch {
    throw new LocalImportPathError("NOT_FOUND");
  }
  if (isSensitivePath(resolved, options.lingxiHome)) {
    throw new LocalImportPathError("PATH_BLOCKED");
  }

  let stat: fs.Stats;
  try {
    stat = await fs.promises.lstat(resolved);
  } catch {
    throw new LocalImportPathError("NOT_FOUND");
  }
  if (stat.isSymbolicLink()) throw new LocalImportPathError("SYMLINK");
  if (stat.isFile()) return { realPath: resolved, stat, kind: "file" };
  if (options.allowDirectories && stat.isDirectory()) {
    return { realPath: resolved, stat, kind: "directory" };
  }
  throw new LocalImportPathError("TYPE_UNSUPPORTED");
}

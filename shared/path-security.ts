import fs from "node:fs";
import os from "node:os";
import path from "node:path";

/** 解析真实路径（跟踪软链接），失败返回 null。 */
export function realPath(value: unknown): string | null {
  if (typeof value !== "string" || !value) return null;
  try {
    return fs.realpathSync(path.resolve(value));
  } catch {
    return null;
  }
}

const SENSITIVE_DIRS = [".ssh", ".gnupg", ".aws", ".config/gcloud", ".kube"];

/** 相对路径、无法解析的路径和用户凭证目录一律按敏感路径处理。 */
export function isSensitivePath(value: unknown, lingxiHome?: string): boolean {
  if (typeof value !== "string" || !path.isAbsolute(value)) return true;
  const resolved = realPath(value);
  if (!resolved) return true;
  for (const directory of SENSITIVE_DIRS) {
    const sensitive = path.join(os.homedir(), directory);
    if (resolved === sensitive || resolved.startsWith(`${sensitive}${path.sep}`)) return true;
  }
  if (lingxiHome) {
    const realHome = realPath(lingxiHome);
    if (realHome && (resolved === realHome || resolved.startsWith(`${realHome}${path.sep}`))) return true;
  }
  return false;
}

/**
 * pinned-tenets-backup-dir.ts — 迁移/恢复收据 backupDir 的平台无关存储合同（C02）。
 *
 * 存储表示：收据里的 backupDir 永远是 "/" 分隔的规范形式
 *   memory/pinned-migration-backup
 *   memory/pinned-migration-backup/<operationId>
 * 历史 bug：Windows 端用 path.join() 的结果直接持久化，写出
 * "memory\\pinned-migration-backup\\<uuid>"；而收据校验只认 POSIX 斜杠，POSIX
 * 端的本地路径解析又把反斜杠当文件名字符——同一份合法收据跨平台不可读。
 *
 * 这里集中收据侧的构造、兼容解析与本地路径合成，迁移与恢复共用；实际文件
 * 访问仍走本机 path 规则。兼容解析先验证原始结构再转规范形式，不先 normalize
 * 消掉危险路径段：绝对路径、盘符、UNC、NUL、"."/".."、多余层级与混合分隔符
 * 一律拒绝。
 */
import path from "node:path";

export const BACKUP_DIR_STORAGE_PREFIX = "memory/pinned-migration-backup";
/** 收据校验的权威形状：规范 POSIX 表示，最多一个 operation 段。 */
const CANONICAL_BACKUP_DIR_RE = /^memory\/pinned-migration-backup(?:\/[a-zA-Z0-9-]+)?$/;
/** 既有 Windows 写入逻辑产生的反斜杠形式（仅兼容读取，不再写出）。 */
const LEGACY_WINDOWS_BACKUP_DIR_RE = /^memory\\pinned-migration-backup(?:\\[a-zA-Z0-9-]+)?$/;

/** 收据写入用：构造规范 POSIX 表示。绝不把本机 path.join 的结果直接持久化。 */
export function backupDirForReceipt(operationId: string | null): string {
  return operationId ? `${BACKUP_DIR_STORAGE_PREFIX}/${operationId}` : BACKUP_DIR_STORAGE_PREFIX;
}

/**
 * 兼容解析：接受规范 POSIX 形式与旧 Windows 反斜杠形式，产出规范形式（仅在
 * 内存中规范化，只读扫描不回写文件）。非法输入返回 null，由调用方按收据
 * 损坏处理。混合分隔符（部分斜杠部分反斜杠）视为歧义输入拒绝。
 */
export function parseReceiptBackupDir(value: unknown): string | null {
  if (typeof value !== "string" || value.length === 0 || value.includes("\0")) return null;
  if (CANONICAL_BACKUP_DIR_RE.test(value)) return value;
  if (LEGACY_WINDOWS_BACKUP_DIR_RE.test(value)) return value.replace(/\\/g, "/");
  return null;
}

/**
 * 本地文件访问用：把（可能来自旧收据的）backupDir 解析为本机路径。解析失败
 * 说明收据损坏——按明确失败处理，不猜测路径。
 */
export function receiptBackupDirLocalPath(agentDir: string, backupDir: string | null): string {
  if (backupDir === null) return path.join(agentDir, ...BACKUP_DIR_STORAGE_PREFIX.split("/"));
  const canonical = parseReceiptBackupDir(backupDir);
  if (canonical === null) {
    throw new Error(`invalid receipt backupDir: ${JSON.stringify(backupDir)}`);
  }
  return path.join(agentDir, ...canonical.split("/"));
}

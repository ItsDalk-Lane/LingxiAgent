/**
 * plan-file.ts — 计划模式约定计划文件的路径契约
 *
 * 计划模式（只读权限档）下唯一放行写的文件：会话旁 `<sessionId>.plan.md`。
 * 三处消费方共用这一条约定，改动必须同步：
 *   - core/session-permission-mode.ts 的只读豁免（写放行）；
 *   - server/plan-gate.ts 的收工检测（本轮是否写了计划）；
 *   - core/session-compactor.ts 的压缩保护（计划内容随摘要存活）。
 */
import path from "path";

export const PLAN_FILE_SUFFIX = ".plan.md";

/** 会话 JSONL 旁的计划文件绝对路径；入参不是有效会话路径时返回 null。 */
export function planFilePathForSession(sessionPath: unknown): string | null {
  const raw = typeof sessionPath === "string" ? sessionPath.trim() : "";
  if (!raw) return null;
  const base = path.basename(raw).replace(/\.jsonl$/i, "");
  if (!base) return null;
  return path.join(path.dirname(raw), `${base}${PLAN_FILE_SUFFIX}`);
}

/** targetPath 是否就是该会话的约定计划文件（相对路径不猜 cwd，落安全默认）。 */
export function isPlanFilePath(sessionPath: unknown, targetPath: unknown): boolean {
  const plan = planFilePathForSession(sessionPath);
  if (!plan || typeof targetPath !== "string" || !targetPath.trim()) return false;
  return path.resolve(targetPath) === plan;
}

const PLAN_WRITE_TOOL_NAMES = new Set(["write", "edit"]);

/** 一次工具调用是否是对本会话计划文件的写。 */
export function isPlanFileWrite(toolName: unknown, params: any, sessionPath: unknown): boolean {
  const name = typeof toolName === "string" ? toolName : "";
  if (!PLAN_WRITE_TOOL_NAMES.has(name)) return false;
  return isPlanFilePath(sessionPath, params?.path);
}

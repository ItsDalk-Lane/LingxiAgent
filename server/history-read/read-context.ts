/**
 * 历史读取 —— 读取上下文捕获（阶段 B / B06，计划 §1.2）
 *
 * 在读取任何内容之前捕获业务身份、公开 revision、内部文件身份、分支头三态与
 * locator 绑定（I01：身份先于缓存；I05：行缺失 ≠ leafId=null；I07/I08）。不为取
 * 令牌打开 SessionManager、不全读 JSONL——只做一次 stat + 引擎元数据查询。
 * stat 失败 → 返回 null（「修订点未知」：不读旧缓存、不写新缓存，I08）。
 */
import fs from "fs/promises";
import { sessionFileRevision } from "../../core/session-list-projection-cache.ts";
import type { HistoryReadContext } from "./types.ts";

export interface CaptureReadContextInput {
  sessionPath: string;
  sessionId: string | null;
  runtimeId?: string | null;
  studioId?: string | null;
}

export type CapturedBranchHead =
  | { supported: false }
  | { supported: true; rowExists: false }
  | { supported: true; rowExists: true; row: { leafId: string | null; observedTailLeafId?: string | null; reason?: string | null } | null };

/**
 * 引擎内部 manifest store 的窄探测（恢复写回用，Batch 2 directory.ts 的
 * persistBranchHead 语义）：引擎未暴露 store 时返回 undefined——恢复职责退回
 * 既有打开/写入边界的原路径（旧链路行为不受影响），不伪造写回。
 */
function probeManifestStore(engine: any): { setBranchHead: (sessionId: string, state: any) => unknown } | null {
  const store = engine?._sessionManifestStore ?? engine?._sessionCoord?._sessionManifestStore ?? null;
  return store && typeof store.setBranchHead === "function" ? store : null;
}

export async function captureReadContext(
  engine: any,
  input: CaptureReadContextInput,
): Promise<HistoryReadContext | null> {
  const { sessionPath, sessionId } = input;
  if (!sessionPath) return null;
  // ① stat 在读内容之前（竞态纪律同 readSessionFileRevision：revision 只偏旧不偏新）。
  let stat: any;
  try {
    stat = await fs.stat(sessionPath);
  } catch {
    return null; // I08：修订未知 → 不读旧缓存、不写新缓存
  }

  // ② 内部文件身份（同一次 stat；平台缺字段记录降级，B04）。
  const identityFields: string[] = ["size", "mtimeMs"];
  const fileIdentity: HistoryReadContext["fileIdentity"] = {
    size: stat.size,
    mtimeMs: stat.mtimeMs,
  };
  for (const field of ["dev", "ino", "ctimeMs"] as const) {
    if (typeof stat[field] === "number" && Number.isFinite(stat[field])) {
      (fileIdentity as any)[field] = stat[field];
      identityFields.push(field);
    }
  }
  (fileIdentity as any).identityFields = identityFields;

  // ③ 分支头三态（I05）。引擎无 head 能力（部分测试/桥接引擎）→ supported:false，
  //    目录与 probe 双侧一致跳过 head 校验，不伪造「无头行」。
  let branchHead: CapturedBranchHead = { supported: false };
  if (sessionId && typeof engine?.getSessionBranchHead === "function") {
    const row = engine.getSessionBranchHead(sessionId) || null;
    branchHead = row
      ? { supported: true, rowExists: true, row: { leafId: row.leafId ?? null, observedTailLeafId: row.observedTailLeafId ?? null, reason: row.reason ?? null } }
      : { supported: true, rowExists: false };
  }

  // ④ locator 绑定 + correlation 启用（与 loadSessionHistoryMessages 同规则：当前
  //    locator 即该路径时才带业务 sessionId）。
  const manifest = sessionId && typeof engine?.getSessionManifest === "function"
    ? engine.getSessionManifest(sessionId) || null
    : null;
  const locatorPath = typeof manifest?.currentLocator?.path === "string" && manifest.currentLocator.path
    ? manifest.currentLocator.path
    : sessionPath;

  // ⑤ 恢复写回窄回调（Batch 2 directory.ts 消费；镜像 readManifestSessionBranch 的
  //    persistRecovery 语义，不需要存活 SessionManager）。
  const store = probeManifestStore(engine);
  const persistBranchHead = store && sessionId
    ? (state: { leafId: string | null; observedTailLeafId: string | null; reason: string }) => {
        return store.setBranchHead(sessionId, state);
      }
    : undefined;

  const context: HistoryReadContext = {
    sessionPath,
    sessionId: sessionId ?? null,
    runtimeId: input.runtimeId ?? engine?.getRuntimeContext?.()?.runtimeId ?? null,
    studioId: input.studioId ?? engine?.getRuntimeContext?.()?.studioId ?? null,
    locatorPath,
    publicRevision: sessionFileRevision(stat),
    fileIdentity,
    branchHeadRowExists: branchHead.supported ? branchHead.rowExists : false,
    branchHeadRow: branchHead.supported && branchHead.rowExists ? branchHead.row : null,
    ...(persistBranchHead ? { persistBranchHead } : {}),
  };
  return context;
}

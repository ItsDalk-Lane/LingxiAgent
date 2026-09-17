/**
 * file-freshness.ts — 会话级文件新鲜度登记表（防陈旧写 + 重复读去重）
 *
 * 对照 openclaude readFileState / dsh fs-observation-policy 的设计：
 * - read 成功后记录 (mtimeMs, size) 指纹；
 * - edit/write 前发现文件在登记之后被外部（用户、linter、别的进程）改过 →
 *   拦下并教模型重读（FS_STALE_VERSION 语义），写前陈旧检查优先于字面匹配；
 * - edit/write 成功后回写指纹，连续编辑不会误报自己刚写的内容；
 * - 同一会话内紧挨着的完全相同读取（同 path+offset+limit 且 stat 未变）返回
 *   存根，指向上一份仍然有效的内容。
 *
 * 去重只对「窗口内的紧邻重复」生效（默认 30s）：Lingxi 的压缩会用摘要替换
 * 旧历史，跨很久的重读不能假设上一份内容还在上下文里——那类重读返回全文。
 * 登记表有容量上限，无定时器，会话键失活由 LRU 自然淘汰。
 */

import { stat as fsStat } from "node:fs/promises";

const DUP_READ_WINDOW_MS = 30_000;
const MAX_SESSIONS = 64;
const MAX_FILES_PER_SESSION = 256;

type FileFingerprint = { mtimeMs: number; size: number };

type SessionRecord = {
  files: Map<string, FileFingerprint>;
  lastRead: {
    key: string;
    fingerprint: FileFingerprint;
    at: number;
  } | null;
};

export type FreshnessStatResult =
  | { ok: true; fingerprint: FileFingerprint }
  | { ok: false };

export type DuplicateReadCheck = {
  duplicate: false;
} | {
  duplicate: true;
  ageSeconds: number;
  label: string;
};

export type StalenessCheck =
  | { stale: false }
  | { stale: true; changedBy: "mtime" | "size" };

export function createFileFreshnessTracker({
  statFile = fsStat,
  now = () => Date.now(),
  duplicateWindowMs = DUP_READ_WINDOW_MS,
}: {
  statFile?: typeof fsStat;
  now?: () => number;
  duplicateWindowMs?: number;
} = {}) {
  const sessions = new Map<string, SessionRecord>();

  const getSession = (sessionKey: string): SessionRecord => {
    let record = sessions.get(sessionKey);
    if (!record) {
      record = { files: new Map(), lastRead: null };
      sessions.set(sessionKey, record);
      while (sessions.size > MAX_SESSIONS) {
        const oldest = sessions.keys().next().value;
        if (oldest === undefined) break;
        sessions.delete(oldest);
      }
    }
    return record;
  };

  const fingerprintPath = async (absolutePath: string): Promise<FreshnessStatResult> => {
    try {
      const stats = await statFile(absolutePath);
      if (!stats.isFile()) return { ok: false };
      return { ok: true, fingerprint: { mtimeMs: stats.mtimeMs, size: stats.size } };
    } catch {
      return { ok: false };
    }
  };

  const touchSession = (record: SessionRecord) => {
    while (record.files.size > MAX_FILES_PER_SESSION) {
      const oldest = record.files.keys().next().value;
      if (oldest === undefined) break;
      record.files.delete(oldest);
    }
  };

  return {
    /** read 成功后登记指纹，并记住这次读取的 (path, offset, limit) 身份。 */
    async observeRead(sessionKey: string | null | undefined, absolutePath: string, readKey: string) {
      const resolved = sessionKey || "global";
      const result = await fingerprintPath(absolutePath);
      if (!result.ok) return;
      const record = getSession(resolved);
      record.files.set(absolutePath, result.fingerprint);
      record.lastRead = { key: readKey, fingerprint: result.fingerprint, at: now() };
      touchSession(record);
    },

    /**
     * 紧邻的完全相同读取（同会话、同读取身份、窗口内、stat 未变）→ 存根。
     * 窗口外或内容已变 → false（正常重读）。
     */
    async checkDuplicateRead(
      sessionKey: string | null | undefined,
      absolutePath: string,
      readKey: string,
    ): Promise<DuplicateReadCheck> {
      const record = sessions.get(sessionKey || "global");
      if (!record?.lastRead || record.lastRead.key !== readKey) return { duplicate: false };
      const ageMs = now() - record.lastRead.at;
      if (ageMs < 0 || ageMs > duplicateWindowMs) return { duplicate: false };
      const current = await fingerprintPath(absolutePath);
      if (!current.ok) return { duplicate: false };
      const observed = record.lastRead.fingerprint;
      if (current.fingerprint.mtimeMs !== observed.mtimeMs || current.fingerprint.size !== observed.size) {
        return { duplicate: false };
      }
      return { duplicate: true, ageSeconds: Math.max(1, Math.round(ageMs / 1000)), label: readKey };
    },

    /** edit/write 前：登记过且 stat 已变 → 拦截。没登记过或 stat 拿不到 → 放行。 */
    async checkFreshBeforeMutation(
      sessionKey: string | null | undefined,
      absolutePath: string,
    ): Promise<StalenessCheck> {
      const record = sessions.get(sessionKey || "global");
      const observed = record?.files.get(absolutePath);
      if (!observed) return { stale: false };
      const current = await fingerprintPath(absolutePath);
      if (!current.ok) return { stale: false };
      if (current.fingerprint.size !== observed.size) return { stale: true, changedBy: "size" };
      if (current.fingerprint.mtimeMs !== observed.mtimeMs) return { stale: true, changedBy: "mtime" };
      return { stale: false };
    },

    /** edit/write 成功后回写指纹，后续编辑不会把自己刚写的内容当外部改动。 */
    async observeMutation(sessionKey: string | null | undefined, absolutePath: string) {
      const resolved = sessionKey || "global";
      const result = await fingerprintPath(absolutePath);
      if (!result.ok) return;
      const record = getSession(resolved);
      record.files.set(absolutePath, result.fingerprint);
      touchSession(record);
    },

    forgetSession(sessionKey: string | null | undefined) {
      sessions.delete(sessionKey || "global");
    },
  };
}

export type FileFreshnessTracker = ReturnType<typeof createFileFreshnessTracker>;

/** 读取身份键：同 path+offset+limit 的重复才可能去重。 */
export function readIdentityKey(params: { path?: unknown; offset?: unknown; limit?: unknown }) {
  const path = typeof params.path === "string" ? params.path : "";
  const offset = Number.isFinite(Number(params.offset)) ? Number(params.offset) : null;
  const limit = Number.isFinite(Number(params.limit)) ? Number(params.limit) : null;
  return JSON.stringify({ path, offset, limit });
}

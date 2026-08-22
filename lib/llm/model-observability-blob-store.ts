/**
 * model-observability-blob-store.ts — privileged Blob 外置存储（Phase 7）。
 *
 * 与 ModelCallPayloadSink 完全不同的独立 contract（§六十一）：
 *
 *     raw binary →（privileged externalizer，只在显式启用 blob persistence 时）
 *              Blob Store（blobs/<shard>/<blobId>.bin + blob_objects 元数据）
 *     Payload Redactor → 只得到 external_blob descriptor（blobId/captureStatus）
 *
 * 红线：
 *   - 只有 runtime 中真实 materialized 的 Buffer/TypedArray/ArrayBuffer 才允许
 *     进入（§六十三）；绝不自动读取本地文件 / 下载 URL / fetch signed URL（§六十四）。
 *   - blobId = mb_<random>（§六十六）：不做内容 hash dedup（避免 sha256(500MB)
 *     成本与信息泄漏）。
 *   - 磁盘文件名禁止使用原文件名（§六十七）：只有 <blobId>.bin；元数据只保留
 *     mime/byteLength，不保留绝对路径。
 *   - 写入 atomic：temporary file → complete write → rename，随机 staging 名（§七十）。
 *   - write failure 不产生 dangling committed ref（§七十一）：文件先写、
 *     metadata + payload refs 后在 SQLite transaction 内提交；失败 → 调用方把
 *     descriptor 降级 store_failed。
 *   - GC 只删 0-live-reference 的 blob（§九十一）；orphan 文件按 grace period
 *     清理（§九十二）；missing blob 标记 state=missing、绝不 crash（§九十三）。
 *
 * Blob 本身可能是用户图片/音频：不做文本 Redaction（预期，§一百一十六）；
 * 依赖 explicit opt-in + private 目录权限。
 */

import fs from "fs";
import path from "path";
import { randomBytes } from "crypto";
import { SECRET_DIR_MODE, ensureSecretDirModeSync } from "../../shared/secret-fs.ts";
import { MODEL_OBSERVABILITY_BLOB_ID_PATTERN } from "../../shared/model-observability-api-contract.ts";
import {
  MODEL_OBSERVABILITY_BLOBS_DIR_NAME,
  modelObservabilityBlobsRoot,
} from "./model-observability-schema.ts";

const SUPPORTS_POSIX_MODE = process.platform !== "win32";
/** 文件内容上限（单 blob）：超过直接不保存（§七十三 size cap）。 */
export const MODEL_OBSERVABILITY_BLOB_MAX_BYTES = 64 * 1024 * 1024;
/** orphan blob 文件的回收宽限期：刚写入但 transaction 未完成的文件不能立即删（§九十二）。 */
export const MODEL_OBSERVABILITY_BLOB_ORPHAN_GRACE_MS = 24 * 60 * 60 * 1000;

export function mintModelObservabilityBlobId(random: () => string = defaultRandomToken): string {
  const blobId = `mb_${random()}`;
  if (!MODEL_OBSERVABILITY_BLOB_ID_PATTERN.test(blobId)) {
    throw new Error("generated model observability blob id is invalid");
  }
  return blobId;
}

function defaultRandomToken(): string {
  return `${Date.now().toString(36)}${randomBytes(8).toString("hex")}`;
}

export type ModelObservabilityBlobRow = {
  blob_id: string;
  created_at: string;
  byte_length: number;
  media_type: string | null;
  state: string;
  relative_path: string;
};

function assertValidBlobId(blobId: string): void {
  if (typeof blobId !== "string" || !MODEL_OBSERVABILITY_BLOB_ID_PATTERN.test(blobId)) {
    throw new Error("invalid model observability blob id");
  }
}

/** 新写入 Blob 的真实分片：跳过固定的 `mb_` 前缀，使用随机 token 前两位。 */
export function modelObservabilityBlobRelativePath(blobId: string): string {
  assertValidBlobId(blobId);
  return path.posix.join(
    MODEL_OBSERVABILITY_BLOBS_DIR_NAME,
    blobId.slice(3, 5),
    `${blobId}.bin`,
  );
}

/** Phase 7/9 历史布局固定落在 `blobs/mb/`；仅用于兼容读取与清理。 */
export function legacyModelObservabilityBlobRelativePath(blobId: string): string {
  assertValidBlobId(blobId);
  return path.posix.join(MODEL_OBSERVABILITY_BLOBS_DIR_NAME, "mb", `${blobId}.bin`);
}

function isPathInside(root: string, candidate: string): boolean {
  const resolvedRoot = path.resolve(root);
  const resolvedCandidate = path.resolve(candidate);
  return resolvedCandidate.startsWith(`${resolvedRoot}${path.sep}`);
}

/**
 * 只由闭集 blobId 推导候选路径；数据库 relative_path 永远不参与文件寻址。
 * 第一项是新分片，第二项是历史 `mb` 布局。
 */
export function modelObservabilityBlobPathCandidates(lingxiHome: string, blobId: string): string[] {
  assertValidBlobId(blobId);
  const root = modelObservabilityBlobsRoot(lingxiHome);
  const relativeCandidates = [
    modelObservabilityBlobRelativePath(blobId),
    legacyModelObservabilityBlobRelativePath(blobId),
  ];
  const candidates = relativeCandidates.map((relative) =>
    path.resolve(root, path.posix.relative(MODEL_OBSERVABILITY_BLOBS_DIR_NAME, relative))
  );
  const unique = [...new Set(candidates)];
  if (unique.some((candidate) => !isPathInside(root, candidate))) {
    throw new Error("model observability blob path escapes blob root");
  }
  return unique;
}

/**
 * 返回 blobs 根目录内真实存在的普通文件。realpath 二次校验会拒绝把分片目录
 * 换成指向外部位置的符号链接，避免读取/删除任意本地文件。
 */
export function resolveExistingModelObservabilityBlobPath(
  lingxiHome: string,
  blobId: string,
): string | null {
  let candidates: string[];
  try {
    candidates = modelObservabilityBlobPathCandidates(lingxiHome, blobId);
  } catch {
    return null;
  }
  const root = modelObservabilityBlobsRoot(lingxiHome);
  let realRoot: string;
  try {
    realRoot = fs.realpathSync(root);
  } catch {
    return null;
  }
  for (const candidate of candidates) {
    try {
      const realCandidate = fs.realpathSync(candidate);
      if (!isPathInside(realRoot, realCandidate)) continue;
      const stat = fs.statSync(realCandidate);
      if (stat.isFile()) return realCandidate;
    } catch {
      // 尝试下一个历史兼容候选。
    }
  }
  return null;
}

export function createModelObservabilityBlobStore({ lingxiHome, db, now = () => new Date().toISOString() }: {
  lingxiHome: string;
  db: any;
  now?: () => string;
}) {
  const root = modelObservabilityBlobsRoot(lingxiHome);

  function relativePathFor(blobId: string): string {
    return modelObservabilityBlobRelativePath(blobId);
  }

  function canonicalAbsolutePathFor(blobId: string): string {
    return modelObservabilityBlobPathCandidates(lingxiHome, blobId)[0];
  }

  function ensureShardDir(absFilePath: string): void {
    const dir = path.dirname(absFilePath);
    fs.mkdirSync(root, { recursive: true });
    fs.mkdirSync(dir, { recursive: true });
    const realRoot = fs.realpathSync(root);
    const realDir = fs.realpathSync(dir);
    if (realDir !== realRoot && !isPathInside(realRoot, realDir)) {
      throw new Error("model observability blob shard escapes blob root");
    }
    if (SUPPORTS_POSIX_MODE) {
      try {
        ensureSecretDirModeSync(dir);
      } catch {
        // 目录权限收紧 best-effort（同 secret-fs 语义）。
      }
    }
  }

  const insertBlobRow = db.prepare(`
    INSERT INTO blob_objects (blob_id, created_at, byte_length, media_type, state, relative_path)
    VALUES (@blob_id, @created_at, @byte_length, @media_type, 'ready', @relative_path)
    ON CONFLICT(blob_id) DO NOTHING
  `);

  return {
    root,
    relativePathFor,

    /** 是否应保存该 binary（size cap，§七十三）。 */
    isEligibleSize(byteLength: number): boolean {
      return Number.isFinite(byteLength) && byteLength >= 0 && byteLength <= MODEL_OBSERVABILITY_BLOB_MAX_BYTES;
    },

    /**
     * 写 blob 文件（atomic：随机 staging 名 → 完整写入 → rename，§七十）。
     * 文件成功后调用方才在 transaction 内 insertBlobRow + payload refs（§七十二）。
     */
    writeBlobFile(blobId: string, bytes: Uint8Array): boolean {
      if (!this.isEligibleSize(bytes.byteLength)) return false;
      try {
        const abs = canonicalAbsolutePathFor(blobId);
        ensureShardDir(abs);
        const staging = path.join(path.dirname(abs), `.${path.basename(abs)}.tmp-${randomBytes(6).toString("hex")}`);
        try {
          if (SUPPORTS_POSIX_MODE) {
            fs.writeFileSync(staging, bytes, { mode: 0o600 });
            try {
              fs.chmodSync(staging, 0o600);
            } catch { /* best-effort 二次收紧 */ }
          } else {
            fs.writeFileSync(staging, bytes);
          }
          fs.renameSync(staging, abs);
        } catch (err) {
          try { fs.rmSync(staging, { force: true }); } catch { /* staging 清理 best-effort */ }
          throw err;
        }
        return true;
      } catch {
        return false;
      }
    },

    /** blob metadata 行提交（须在 coordinator transaction 内；文件须已 durable）。 */
    insertBlobRow(blobId: string, byteLength: number, mediaType: string | null): void {
      insertBlobRow.run({
        blob_id: blobId,
        created_at: now(),
        byte_length: Number(byteLength) || 0,
        media_type: typeof mediaType === "string" && mediaType ? mediaType.slice(0, 128) : null,
        relative_path: relativePathFor(blobId),
      });
    },

    getBlobMetadata(blobId: string): ModelObservabilityBlobRow | null {
      return db.prepare(`SELECT * FROM blob_objects WHERE blob_id = ?`).get(blobId) ?? null;
    },

    /**
     * 读 blob 字节（内部读原语，§一百一十八）。文件缺失 → 标记 state=missing
     * 并返回 null（§九十三：missing blob 不导致 crash）。
     */
    readBlob(blobId: string): Buffer | null {
      const row = this.getBlobMetadata(blobId);
      if (!row) return null;
      try {
        const abs = resolveExistingModelObservabilityBlobPath(lingxiHome, blobId);
        if (!abs) throw new Error("blob file missing or outside blob root");
        return fs.readFileSync(abs);
      } catch {
        try {
          db.prepare(`UPDATE blob_objects SET state = 'missing' WHERE blob_id = ? AND state = 'ready'`).run(blobId);
        } catch { /* 标记失败不掩盖读取语义 */ }
        return null;
      }
    },

    /** 删除 blob（row + 文件）。只应作用于 0-reference 或 maintenance 决定删除的 blob。 */
    deleteBlobs(blobIds: string[]): number {
      if (blobIds.length === 0) return 0;
      const deleteRow = db.prepare(`DELETE FROM blob_objects WHERE blob_id = ?`);
      let deleted = 0;
      for (const blobId of blobIds) {
        const row = this.getBlobMetadata(blobId);
        if (!row) continue;
        deleteRow.run(blobId);
        // 新旧布局都只由 blobId 重算；数据库 relative_path 只是历史 metadata。
        let candidateCount = 0;
        try {
          candidateCount = modelObservabilityBlobPathCandidates(lingxiHome, blobId).length;
        } catch {
          // 非法历史 id 没有任何文件删除权限；metadata 行已安全移除。
        }
        for (let index = 0; index < candidateCount; index += 1) {
          try {
            const safeExisting = resolveExistingModelObservabilityBlobPath(lingxiHome, blobId);
            if (!safeExisting) break;
            fs.rmSync(safeExisting, { force: true });
          } catch { /* 文件删除失败：metadata 已删，文件成为 orphan 由 recovery 清理 */ }
        }
        deleted += 1;
      }
      return deleted;
    },

    /** refless blob GC（§九十一）：只删 0 live references 的 blob，不看 mtime。 */
    collectGarbageBlobs(): string[] {
      const refless = db.prepare(`
        SELECT b.blob_id AS blob_id FROM blob_objects b
        WHERE NOT EXISTS (SELECT 1 FROM payload_blob_refs r WHERE r.blob_id = b.blob_id)
      `).all().map((row: any) => row.blob_id);
      this.deleteBlobs(refless);
      return refless;
    },

    /**
     * Orphan recovery（§九十二）：磁盘上存在、DB 无 row、且超过 grace period 的
     * 文件（crash 时文件已写但 transaction 未 commit）。刚写的文件绝不能删。
     */
    recoverOrphanBlobFiles({ graceMs = MODEL_OBSERVABILITY_BLOB_ORPHAN_GRACE_MS }: { graceMs?: number } = {}): number {
      let removed = 0;
      const knownPaths = new Set<string>();
      for (const row of db.prepare(`SELECT blob_id FROM blob_objects`).all()) {
        try {
          for (const candidate of modelObservabilityBlobPathCandidates(lingxiHome, row.blob_id)) {
            knownPaths.add(path.relative(root, candidate).split(path.sep).join("/"));
          }
        } catch {
          // 非法历史 id 不得给任何磁盘路径授权。
        }
      }
      const walk = (dir: string): void => {
        let entries: fs.Dirent[];
        try {
          entries = fs.readdirSync(dir, { withFileTypes: true });
        } catch {
          return;
        }
        for (const entry of entries) {
          const abs = path.join(dir, entry.name);
          if (entry.isDirectory()) {
            walk(abs);
            continue;
          }
          const relative = path.relative(root, abs).split(path.sep).join("/");
          if (knownPaths.has(relative)) continue;
          let mtimeMs: number;
          try {
            mtimeMs = fs.statSync(abs).mtimeMs;
          } catch {
            continue;
          }
          if (Date.now() - mtimeMs < graceMs) continue;
          try {
            fs.rmSync(abs, { force: true });
            removed += 1;
          } catch { /* 下次 maintenance 再试 */ }
        }
      };
      try {
        fs.mkdirSync(root, { recursive: true });
        ensureSecretDirModeSyncWrapper(root);
        walk(root);
      } catch { /* maintenance best-effort */ }
      return removed;
    },

    /** Missing recovery（§九十三）：row 存在但文件不存在 → state=missing。 */
    recoverMissingBlobs(): number {
      const rows: ModelObservabilityBlobRow[] = db.prepare(
        `SELECT * FROM blob_objects WHERE state = 'ready'`,
      ).all();
      let missing = 0;
      for (const row of rows) {
        try {
          if (!resolveExistingModelObservabilityBlobPath(lingxiHome, row.blob_id)) {
            throw new Error("blob file missing or outside blob root");
          }
        } catch {
          db.prepare(`UPDATE blob_objects SET state = 'missing' WHERE blob_id = ?`).run(row.blob_id);
          missing += 1;
        }
      }
      return missing;
    },

    /** 收紧 blob 根目录权限（Unix；Windows 语义见 secret-fs 注释）。 */
    ensurePrivateRoot(): void {
      if (!SUPPORTS_POSIX_MODE) return;
      try {
        fs.mkdirSync(root, { recursive: true });
        ensureSecretDirModeSync(root);
      } catch { /* best-effort */ }
    },
  };
}

function ensureSecretDirModeSyncWrapper(dir: string): void {
  if (!SUPPORTS_POSIX_MODE) return;
  try {
    ensureSecretDirModeSync(dir);
  } catch { /* best-effort */ }
}

export type ModelObservabilityBlobStore = ReturnType<typeof createModelObservabilityBlobStore>;
export { SECRET_DIR_MODE };

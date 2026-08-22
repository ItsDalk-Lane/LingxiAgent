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
  return `mb_${random()}`;
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

export function createModelObservabilityBlobStore({ lingxiHome, db, now = () => new Date().toISOString() }: {
  lingxiHome: string;
  db: any;
  now?: () => string;
}) {
  const root = modelObservabilityBlobsRoot(lingxiHome);

  function relativePathFor(blobId: string): string {
    const shard = blobId.slice(0, 2).replace(/[^a-z0-9]/gi, "0") || "00";
    return path.posix.join(MODEL_OBSERVABILITY_BLOBS_DIR_NAME, shard, `${blobId}.bin`);
  }

  function absolutePathFor(relativePath: string): string {
    // relativePath 由本模块生成（blobs/<shard>/<id>.bin）；resolve 后仍强制落在 root 内。
    const abs = path.resolve(root, "..", relativePath);
    if (!abs.startsWith(path.resolve(root, "..") + path.sep)) {
      throw new Error("blob relative path escapes observability directory");
    }
    return abs;
  }

  function ensureShardDir(absFilePath: string): void {
    const dir = path.dirname(absFilePath);
    fs.mkdirSync(dir, { recursive: true });
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
        const relativePath = relativePathFor(blobId);
        const abs = absolutePathFor(relativePath);
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
        return fs.readFileSync(absolutePathFor(row.relative_path));
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
        try {
          fs.rmSync(absolutePathFor(row.relative_path), { force: true });
        } catch { /* 文件删除失败：metadata 已删，文件成为 orphan 由 recovery 清理 */ }
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
      const knownPaths = new Set(
        db.prepare(`SELECT relative_path FROM blob_objects`).all().map((row: any) => row.relative_path),
      );
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
          const relative = path.relative(path.resolve(root, ".."), abs).split(path.sep).join("/");
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
          fs.accessSync(absolutePathFor(row.relative_path));
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

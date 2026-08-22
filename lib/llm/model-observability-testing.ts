/**
 * model-observability-testing.ts — Phase 7 durable store 测试基建（仅测试路径）。
 *
 * 提供：
 *   - createModelObservabilityTestHarness：真实临时 LINGXI_HOME + 真实安装
 *     installModelObservabilityPersistence（真实生产 wiring，不是 mock）；
 *   - openModelObservabilityReader：第二个连接读回（Restart Roundtrip 用
 *     「关掉再打开」模拟进程重启）；
 *   - scanStoreFilesForPoison：直接扫描 .sqlite/.sqlite-wal/.sqlite-shm 文件
 *     bytes（§一百零一：不仅 SELECT，落盘字节也不得含 poison）。
 */

import fs from "fs";
import os from "os";
import path from "path";
import {
  modelObservabilityBlobsRoot,
  modelObservabilityDbPath,
  openModelObservabilityDatabase,
} from "./model-observability-schema.ts";
import { createModelObservabilityTraceStore } from "./model-observability-trace-store.ts";
import { createModelObservabilityPayloadStore } from "./model-observability-payload-store.ts";
import { createModelObservabilityBlobStore } from "./model-observability-blob-store.ts";
import {
  installModelObservabilityPersistence,
  type ModelObservabilityPersistenceHandle,
  type ModelObservabilityPersistencePolicy,
} from "./model-observability-persistence.ts";

export type ModelObservabilityTestHarness = {
  lingxiHome: string;
  dbPath: string;
  blobsRoot: string;
  handle: ModelObservabilityPersistenceHandle;
  flush(): void;
  /** 第二连接读（模拟 restart 后的 reader 进程；用完 closeReader）。 */
  openReader(): {
    db: any;
    traceStore: ReturnType<typeof createModelObservabilityTraceStore>;
    payloadStore: ReturnType<typeof createModelObservabilityPayloadStore>;
    blobStore: ReturnType<typeof createModelObservabilityBlobStore>;
    close(): void;
  };
  /** 读取 DB/WAL/SHM 文件 bytes（checkpoint 语义见 scanStoreFilesForPoison）。 */
  readStoreFileBytes(): Array<{ name: string; bytes: Buffer }>;
  close(): Promise<void>;
  cleanup(): void;
};

export function createModelObservabilityTestHarness(options: {
  policy?: ModelObservabilityPersistencePolicy;
  lingxiHome?: string;
} = {}): ModelObservabilityTestHarness {
  const tempDir = options.lingxiHome ?? fs.mkdtempSync(path.join(os.tmpdir(), "hana-model-observability-"));
  const handle = installModelObservabilityPersistence({
    lingxiHome: tempDir,
    policy: options.policy ?? { enabled: true, persistPayloads: true, persistBlobs: true },
  });
  const dbPath = modelObservabilityDbPath(tempDir);
  const blobsRoot = modelObservabilityBlobsRoot(tempDir);

  return {
    lingxiHome: tempDir,
    dbPath,
    blobsRoot,
    handle,
    flush() {
      handle.flushSync();
    },
    openReader() {
      const db = openModelObservabilityDatabase(dbPath);
      const traceStore = createModelObservabilityTraceStore({ db });
      const payloadStore = createModelObservabilityPayloadStore({ db, traceStore });
      const blobStore = createModelObservabilityBlobStore({ lingxiHome: tempDir, db });
      return {
        db,
        traceStore,
        payloadStore,
        blobStore,
        close() {
          db.close();
        },
      };
    },
    readStoreFileBytes() {
      const names = [dbPath, `${dbPath}-wal`, `${dbPath}-shm`];
      const out: Array<{ name: string; bytes: Buffer }> = [];
      for (const name of names) {
        try {
          out.push({ name: path.basename(name), bytes: fs.readFileSync(name) });
        } catch {
          // 文件不存在（例如已 checkpoint 合并）→ 跳过。
        }
      }
      return out;
    },
    async close() {
      await handle.close();
    },
    cleanup() {
      try {
        fs.rmSync(tempDir, { recursive: true, force: true });
      } catch {
        // Windows 句柄释放延迟时忽略；CI temp 最终会清理。
      }
    },
  };
}

/**
 * §一百零一：对实际存在的 .sqlite / .sqlite-wal / .sqlite-shm 文件做字节级
 * poison 扫描。调用方应先 handle.flush() + close()（或 wal_checkpoint）确保
 * 已提交内容落到可读文件。任一命中即返回 true。
 */
export function scanStoreFilesForPoison(
  files: Array<{ name: string; bytes: Buffer }>,
  poisons: string[],
): { hit: boolean; hits: Array<{ file: string; poison: string }> } {
  const hits: Array<{ file: string; poison: string }> = [];
  for (const poison of poisons) {
    for (const file of files) {
      if (file.bytes.includes(poison)) {
        hits.push({ file: file.name, poison });
      }
    }
  }
  return { hit: hits.length > 0, hits };
}

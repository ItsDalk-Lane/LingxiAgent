/**
 * 历史读取 —— 定点读取与原坐标 hydrate（阶段 B / B05）
 *
 * 按已验证的记录定位取离散字节区间：排序去重、默认只合并重叠/紧邻区域
 * （mergeGapBytes=0，合并间隙必须有界并如实报告 coalescingExtraBytes，禁止读
 * [最早依赖, 最晚依赖] 整片冒充窗口读取）；position read + bytesRead 循环补齐；
 * 打开句柄先 fstat 校验内部身份五元组（dev/ino/size/mtime/ctime，X02/X03/X04），
 * 异常 EOF 时重验身份（不把未填充 Buffer 当有效 JSON）；每条记录解析后校验
 * entryId/type 与目录一致（不符 → directory_invalid，不拿错行投影）；同一请求内
 * 重复引用只解析一次；区域并发读取受 maxConcurrent 限制；所有分支关闭句柄。
 * 超限行在内存套用 projectOversizedSessionEntry（与 scan 同形状，hanaRepair 元数据）。
 */
import { open, stat } from "fs/promises";
import {
  projectOversizedSessionEntry,
  DEFAULT_SESSION_JSONL_MAX_LINE_BYTES,
} from "../../core/session-jsonl-file.ts";
import type { HistoryReadFileIdentity, HistoryReadHook, InvalidationReason } from "./types.ts";
import type { HistoryRecordLocation } from "./page.ts";

export type { HistoryRecordLocation };

export interface ReadHistoryRecordsOptions {
  /** 区域并发读取上限；默认 4（§1.8 maxConcurrentReadsPerPage）。 */
  maxConcurrent?: number;
  /** 区域合并间隙上限字节；默认 0 = 只合并重叠/紧邻。 */
  mergeGapBytes?: number;
  /** 数据读取注入点（测试）；身份校验仍走真实 stat/fstat。 */
  readFile?: HistoryReadHook;
  /** 超限行阈值；默认 1 MiB（与 scan 一致）。 */
  maxLineBytes?: number;
}

export type ReadHistoryRecordsResult =
  | {
      ok: true;
      /** by sourceIndex；稀疏事实不重新编号（I02）。 */
      records: Map<number, any>;
      /** 实际读取的记录行总字节（含合并间隙与重叠去重后的物理读取量）。 */
      logicalReadBytes: number;
      /** 启用合并时额外读取的字节（mergeGapBytes=0 恒为重叠去重量，通常为 0）。 */
      coalescingExtraBytes: number;
    }
  | { ok: false; reason: InvalidationReason; message?: string };

interface Region {
  byteOffset: number;
  byteLength: number;
  slots: Array<{ location: HistoryRecordLocation; duplicateOf?: number }>;
}

async function identityMatches(
  handle: any,
  sessionPath: string,
  identity: HistoryReadFileIdentity,
): Promise<boolean> {
  const st = handle ? await handle.stat() : await stat(sessionPath);
  if (st.size !== identity.size) return false;
  if (identity.dev != null && st.dev !== identity.dev) return false;
  if (identity.ino != null && st.ino !== identity.ino) return false;
  if (st.mtimeMs !== identity.mtimeMs) return false;
  if (identity.ctimeMs != null && st.ctimeMs !== identity.ctimeMs) return false;
  return true;
}

export async function readHistoryRecords(
  sessionPath: string,
  identity: HistoryReadFileIdentity,
  locations: HistoryRecordLocation[],
  opts: ReadHistoryRecordsOptions = {},
): Promise<ReadHistoryRecordsResult> {
  const maxConcurrent = Math.max(1, opts.maxConcurrent ?? 4);
  const mergeGapBytes = Math.max(0, opts.mergeGapBytes ?? 0);
  const maxLineBytes = Math.max(1024, opts.maxLineBytes ?? DEFAULT_SESSION_JSONL_MAX_LINE_BYTES);

  // 排序 + 去重：同一物理位置只解析一次；同请求重复引用（相同 sourceIndex 或相同
  // 字节区间）共享同一次解析结果。
  const sorted = [...locations].sort((a, b) => a.byteOffset - b.byteOffset || a.sourceIndex - b.sourceIndex);
  const bySourceIndex = new Map<number, HistoryRecordLocation>();
  const unique: HistoryRecordLocation[] = [];
  const dedupMap = new Map<HistoryRecordLocation, HistoryRecordLocation>();
  for (const loc of sorted) {
    const seen = bySourceIndex.get(loc.sourceIndex);
    if (seen) {
      if (seen.byteOffset !== loc.byteOffset || seen.byteLength !== loc.byteLength) {
        return { ok: false, reason: "directory_invalid", message: `conflicting locations for sourceIndex ${loc.sourceIndex}` };
      }
      dedupMap.set(loc, seen);
      continue;
    }
    bySourceIndex.set(loc.sourceIndex, loc);
    unique.push(loc);
    dedupMap.set(loc, loc);
  }

  // 区域划分：合并重叠区域；「紧邻」= 间隔 ≤ 行分隔符（\n/\r\n，属文件结构而非间隙）；
  // mergeGapBytes 为分隔符之外的额外间隙上限（默认 0 = 只合并重叠/紧邻）。
  const regions: Region[] = [];
  let current: Region | null = null;
  for (const loc of unique) {
    if (current && loc.byteOffset <= current.byteOffset + current.byteLength + mergeGapBytes + 2) {
      const regionEnd = current.byteOffset + current.byteLength;
      const locEnd = loc.byteOffset + loc.byteLength;
      current.byteLength = Math.max(regionEnd, locEnd) - current.byteOffset;
      current.slots.push({ location: loc });
    } else {
      current = { byteOffset: loc.byteOffset, byteLength: loc.byteLength, slots: [{ location: loc }] };
      regions.push(current);
    }
  }

  const requestedBytes = unique.reduce((sum, loc) => sum + loc.byteLength, 0);

  const handle = opts.readFile ? null : await open(sessionPath, "r");
  try {
    // 打开句柄后的 fstat 身份校验（X02/X03/X04）：任一字段不符 → 不读数据。
    if (!(await identityMatches(handle, sessionPath, identity))) {
      return { ok: false, reason: "file_identity_changed" };
    }

    const readAt: HistoryReadHook = opts.readFile
      ? opts.readFile
      : async (buffer, offset, length, position) => {
          const result = await (handle as any).read(buffer, offset, length, position);
          return result.bytesRead;
        };

    const regionBuffers = new Map<Region, Buffer>();
    let logicalReadBytes = 0;
    let shortReadAt: Region | null = null;

    // 区域并发读取（受控信号量），保持结果确定性。
    let cursor = 0;
    const worker = async () => {
      for (;;) {
        const index = cursor;
        cursor += 1;
        if (index >= regions.length || shortReadAt) return;
        const region = regions[index];
        const buffer = Buffer.alloc(region.byteLength);
        let got = 0;
        while (got < region.byteLength) {
          const n = await readAt(buffer, got, region.byteLength - got, region.byteOffset + got);
          if (!Number.isFinite(n) || n <= 0) break;
          got += n;
        }
        if (got < region.byteLength) {
          // 异常 EOF：先重验身份（读取途中可能已发生截断/替换，X05），不把未填充
          // Buffer 当有效 JSON。
          shortReadAt = region;
          regionBuffers.set(region, buffer.subarray(0, got));
          return;
        }
        regionBuffers.set(region, buffer);
        logicalReadBytes += region.byteLength;
      }
    };
    const workers = Array.from({ length: Math.min(maxConcurrent, regions.length) }, worker);
    await Promise.all(workers);

    if (shortReadAt) {
      if (!(await identityMatches(handle, sessionPath, identity))) {
        return { ok: false, reason: "file_identity_changed" };
      }
      return { ok: false, reason: "short_read" };
    }

    const records = new Map<number, any>();
    for (const region of regions) {
      const buffer = regionBuffers.get(region)!;
      for (const slot of region.slots) {
        const loc = slot.location;
        const start = loc.byteOffset - region.byteOffset;
        const lineBytes = buffer.subarray(start, start + loc.byteLength);
        const text = lineBytes.toString("utf8");
        let parsed: any;
        try {
          parsed = JSON.parse(text);
        } catch (err: any) {
          return { ok: false, reason: "directory_invalid", message: err?.message || String(err) };
        }
        // 超限行与 parseSessionLine 同规则：按实际字节长度在内存投影（hanaRepair 形状一致）。
        if (lineBytes.length > maxLineBytes) {
          parsed = projectOversizedSessionEntry(parsed, {
            originalByteLength: lineBytes.length,
            maxLineBytes,
          });
        }
        // 索引校验：entryId 强校验；type 仅在目录给出已知 raw type 时校验。
        const entryId = typeof parsed?.id === "string" && parsed.id ? parsed.id : null;
        if (loc.entryId != null && entryId !== loc.entryId) {
          return { ok: false, reason: "directory_invalid", message: `entryId mismatch at ${loc.byteOffset}` };
        }
        if (loc.type != null && parsed?.type !== loc.type) {
          return { ok: false, reason: "directory_invalid", message: `type mismatch at ${loc.byteOffset}` };
        }
        records.set(loc.sourceIndex, parsed);
      }
    }

    const coalescingExtraBytes = logicalReadBytes - requestedBytes;
    return { ok: true, records, logicalReadBytes, coalescingExtraBytes };
  } finally {
    if (handle) await handle.close();
  }
}

/**
 * 便捷封装：把已打开的身份校验与窗口/依赖定位合成为「记录 + 位置」清单。
 * 供 Batch 4 编排层使用；这里独立导出以便测试直接驱动。
 */
export function collectLocations(
  windowRecordIndexes: number[],
  directory: HistoryDirectoryLike,
  dependencyLocations: HistoryRecordLocation[] = [],
): HistoryRecordLocation[] {
  const out: HistoryRecordLocation[] = [...dependencyLocations];
  for (const sourceIndex of windowRecordIndexes) {
    const record = directory.records.at(sourceIndex);
    if (!record?.entryId) continue;
    const physicalIndex = directory.file.byEntryId.get(record.entryId);
    if (physicalIndex == null) continue;
    out.push({
      sourceIndex,
      entryId: record.entryId,
      type: record.role === "user" || record.role === "assistant" || record.role === "toolResult" ? "message" : null,
      byteOffset: directory.file.byteOffsets.at(physicalIndex),
      byteLength: directory.file.byteLengths.at(physicalIndex),
    });
  }
  out.sort((a, b) => a.byteOffset - b.byteOffset);
  return out;
}

interface HistoryDirectoryLike {
  records: { length: number; at(index: number): any };
  file: { byEntryId: Map<string, number>; byteOffsets: { at(index: number): number }; byteLengths: { at(index: number): number } };
}

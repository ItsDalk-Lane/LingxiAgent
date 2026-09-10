/**
 * 历史读取 —— 只读扫描与完整记录边界（阶段 B / B02）
 *
 * 以原始 Buffer 统计偏移（禁用 JS 字符串长度、重新序列化长度或过滤后行数计算
 * 位置）；256 KiB 分块 + 行级字节累积，UTF-8 多字节字符只在整行字节上解码，
 * 天然不跨行截断。规则（TASKBOOK B02 / I09）：
 *  - 已完成位置（有行分隔符）JSON.parse 失败 → error.code="corrupt_record"，停止扫描，
 *    不建目录（走既有 legacy 链路，其 repair 语义原样保留）；
 *  - 无尾换行的完整末条 JSON → lastUndelimitedRow 记录特殊尾状态，正常纳入；
 *  - 截断 JSON / 不完整 UTF-8 尾 → pendingTailOffset，不 parse 进 entries，下次从其起点续读；
 *  - 超限行（> maxLineBytes）在内存套用现有纯函数 projectOversizedSessionEntry
 *    （hanaRepair 元数据形状与 repair 产物逐字段一致）；本扫描不调用
 *    repairOversizedSessionEntriesInFile、不写任何文件、不产生 .repair.json；
 *  - bytesRead 循环补齐（readFile hook 供测试注入短读/中断）；所有分支关闭句柄。
 * 物理偏移始终属于原文件：投影后条目的 byteOffset/byteLength 仍指向原始行。
 */
import { open } from "fs/promises";
import {
  projectOversizedSessionEntry,
  DEFAULT_SESSION_JSONL_MAX_LINE_BYTES,
} from "../../core/session-jsonl-file.ts";
import type {
  HistoryPhysicalEntry,
  HistoryReadHook,
  HistoryScanError,
  HistoryScanResult,
} from "./types.ts";

const DEFAULT_CHUNK_BYTES = 256 * 1024;
const LF = 0x0a;
const CR = 0x0d;

export interface ScanHistoryFileOptions {
  /** 捕获的文件长度（读取内容前的 stat.size）；扫描范围 = [startOffset, capturedLength)。 */
  capturedLength: number;
  /** 续读起点（如 pendingTailOffset）；默认 0。 */
  startOffset?: number;
  /** 分块大小；默认 256 KiB。 */
  chunkBytes?: number;
  /** 超限行阈值；默认 1 MiB（与 DEFAULT_SESSION_JSONL_MAX_LINE_BYTES 一致）。 */
  maxLineBytes?: number;
  /** 读取注入点（测试）；默认真实 fs 位置读。 */
  readFile?: HistoryReadHook;
}

export async function scanHistoryFile(
  sessionPath: string,
  opts: ScanHistoryFileOptions,
): Promise<HistoryScanResult> {
  const chunkBytes = Math.max(1, Math.floor(opts.chunkBytes ?? DEFAULT_CHUNK_BYTES));
  const maxLineBytes = Math.max(1024, opts.maxLineBytes ?? DEFAULT_SESSION_JSONL_MAX_LINE_BYTES);
  const startOffset = Math.max(0, Math.floor(opts.startOffset ?? 0));
  const capturedLength = Math.max(0, Math.floor(opts.capturedLength));
  const endOffset = Math.max(startOffset, capturedLength);

  const entries: any[] = [];
  const physical: HistoryPhysicalEntry[] = [];
  let header: { sdkSessionId: string; version: number } | null = null;
  let scanError: HistoryScanError | null = null;

  // 当前未完成行的字节累积（跨块）。
  let pending: Buffer[] = [];
  let pendingBytes = 0;
  let lineStartOffset = startOffset;
  let readPosition = startOffset;
  // 损坏行的起点（安全边界止于它，而不是它的行尾）。
  let corruptLineOffset: number | null = null;

  const handle = opts.readFile ? null : await open(sessionPath, "r");
  try {
    const readAt: HistoryReadHook = opts.readFile
      ? opts.readFile
      : async (buffer, offset, length, position) => {
          const result = await (handle as any).read(buffer, offset, length, position);
          return result.bytesRead;
        };

    while (readPosition < endOffset && !scanError) {
      const want = Math.min(chunkBytes, endOffset - readPosition);
      const chunk = Buffer.alloc(want);
      // bytesRead 循环补齐：hook 可能返回小于请求数；0 = 该位置 EOF，不读空白缓冲区。
      let got = 0;
      while (got < want) {
        const n = await readAt(chunk, got, want - got, readPosition + got);
        if (!Number.isFinite(n) || n <= 0) break;
        got += n;
      }
      if (got === 0) break;

      let searchFrom = 0;
      for (;;) {
        const nl = chunk.indexOf(LF, searchFrom);
        if (nl === -1) break;
        let lineBuf: Buffer;
        if (pendingBytes === 0) {
          lineBuf = chunk.subarray(searchFrom, nl);
        } else {
          pending.push(chunk.subarray(searchFrom, nl));
          lineBuf = Buffer.concat(pending, pendingBytes + (nl - searchFrom));
          pending = [];
          pendingBytes = 0;
        }
        searchFrom = nl + 1;

        // 行分隔符：\n 或 \r\n（\r 属分隔符，不计入记录字节长度）。
        let separatorLength = 1;
        let contentLength = lineBuf.length;
        if (contentLength > 0 && lineBuf[contentLength - 1] === CR) {
          contentLength -= 1;
          separatorLength = 2;
        }
        const content = separatorLength === 2 ? lineBuf.subarray(0, contentLength) : lineBuf;
        const byteOffset = lineStartOffset;
        lineStartOffset = readPosition + searchFrom;

        if (scanError) break;
        const text = content.toString("utf8");
        if (!text.trim()) continue; // 空行：不产生条目，偏移照常推进

        let parsed: any;
        try {
          parsed = JSON.parse(text);
        } catch (err: any) {
          // 已完成位置的损坏：明确错误、不建目录（不是「暂时半行」）。
          scanError = {
            code: "corrupt_record",
            physicalIndex: entries.length,
            message: err?.message || String(err),
          };
          corruptLineOffset = byteOffset;
          break;
        }
        let entry = parsed;
        let oversized = false;
        if (content.length > maxLineBytes) {
          entry = projectOversizedSessionEntry(parsed, {
            originalByteLength: content.length,
            maxLineBytes,
          });
          oversized = true;
        }
        entries.push(entry);
        const entryId = typeof entry?.id === "string" && entry.id ? entry.id : null;
        physical.push({
          physicalIndex: entries.length - 1,
          entryId,
          byteOffset,
          byteLength: content.length,
          separatorLength,
          type: typeof entry?.type === "string" ? entry.type : null,
          oversized,
        });
        if (!header && entry?.type === "session" && typeof entry.id === "string" && entry.id) {
          header = { sdkSessionId: entry.id, version: entry.version };
        }
      }

      if (got - searchFrom > 0) {
        pending.push(chunk.subarray(searchFrom, got));
        pendingBytes += got - searchFrom;
      }
      readPosition += got;
    }
  } finally {
    if (handle) await handle.close();
  }

  const bounds = {
    capturedLength,
    observedFileSize: readPosition,
    indexedThroughOffset: lineStartOffset,
    pendingTailOffset: null as number | null,
    lastUndelimitedRow: null as { offset: number; length: number } | null,
  };

  if (scanError) {
    // 损坏行有行分隔符，不是半行尾；安全边界止于其起点。
    return {
      entries,
      physical,
      bounds: { ...bounds, indexedThroughOffset: corruptLineOffset ?? bounds.indexedThroughOffset },
      header,
      error: scanError,
    };
  }

  if (pendingBytes > 0) {
    // 无尾换行的末行：完整 JSON → 纳入 + 记录特殊尾状态；截断 JSON / 不完整
    // UTF-8 / 尾部残余 → pendingTailOffset，不 parse 进 entries（I09）。
    const lineBuf = Buffer.concat(pending, pendingBytes);
    const byteOffset = lineStartOffset;
    const text = lineBuf.toString("utf8");
    if (text.trim()) {
      let parsed: any = null;
      try {
        parsed = JSON.parse(text);
      } catch {
        parsed = null;
      }
      if (parsed !== null) {
        let entry = parsed;
        let oversized = false;
        if (lineBuf.length > maxLineBytes) {
          entry = projectOversizedSessionEntry(parsed, {
            originalByteLength: lineBuf.length,
            maxLineBytes,
          });
          oversized = true;
        }
        entries.push(entry);
        physical.push({
          physicalIndex: entries.length - 1,
          entryId: typeof entry?.id === "string" && entry.id ? entry.id : null,
          byteOffset,
          byteLength: lineBuf.length,
          separatorLength: 0,
          type: typeof entry?.type === "string" ? entry.type : null,
          oversized,
        });
        if (!header && entry?.type === "session" && typeof entry.id === "string" && entry.id) {
          header = { sdkSessionId: entry.id, version: entry.version };
        }
        bounds.lastUndelimitedRow = { offset: byteOffset, length: lineBuf.length };
        bounds.indexedThroughOffset = byteOffset + lineBuf.length;
        return { entries, physical, bounds, header, error: null };
      }
      bounds.pendingTailOffset = byteOffset;
      bounds.indexedThroughOffset = byteOffset;
      return { entries, physical, bounds, header, error: null };
    }
    // 末行只有空白（如孤立 \r）：按无条目处理，边界推到文件尾。
    bounds.indexedThroughOffset = readPosition;
    return { entries, physical, bounds, header, error: null };
  }

  bounds.indexedThroughOffset = lineStartOffset;
  return { entries, physical, bounds, header, error: null };
}

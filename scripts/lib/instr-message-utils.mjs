/**
 * core/message-utils.ts 的计数包装再导出（仅基准/证据运行环境经 module.registerHooks
 * 重定向使用；不改动生产文件，生产路径不受影响）。
 *
 * 包装范围：
 *  - loadSessionHistoryMessages / loadSessionHistoryEvidence：请求级入口调用计数；
 *  - annotateOriginMessages / collectSessionCollabDecisions / collectModelCallReferencesBySourceIndex：
 *    全数组扫描入口，inputLen 计入 metadataVisitedCount（下界口径）。
 */
import * as real from "../../core/message-utils.ts";
import { getActiveHistoryReadCounters } from "./history-read-counters.mjs";

export * from "../../core/message-utils.ts";

function scanName(name, fn) {
  return function (...args) {
    const counters = getActiveHistoryReadCounters();
    const t0 = process.hrtime.bigint();
    try {
      return fn.apply(this, args);
    } finally {
      const ms = Number(process.hrtime.bigint() - t0) / 1e6;
      if (counters) counters.recordScan(name, Array.isArray(args[0]) ? args[0].length : null, ms);
    }
  };
}

function entryName(name, fn) {
  return function (...args) {
    const counters = getActiveHistoryReadCounters();
    const t0 = process.hrtime.bigint();
    const result = fn.apply(this, args);
    if (result && typeof result.then === "function") {
      return result.finally(() => {
        const ms = Number(process.hrtime.bigint() - t0) / 1e6;
        if (counters) counters.recordScan(name, null, ms);
      });
    }
    const ms = Number(process.hrtime.bigint() - t0) / 1e6;
    if (counters) counters.recordScan(name, null, ms);
    return result;
  };
}

export const loadSessionHistoryMessages = entryName("loadSessionHistoryMessages", real.loadSessionHistoryMessages);
export const loadSessionHistoryEvidence = entryName("loadSessionHistoryEvidence", real.loadSessionHistoryEvidence);
export const annotateOriginMessages = scanName("annotateOriginMessages", real.annotateOriginMessages);
export const collectSessionCollabDecisions = scanName("collectSessionCollabDecisions", real.collectSessionCollabDecisions);
export const collectModelCallReferencesBySourceIndex = scanName(
  "collectModelCallReferencesBySourceIndex",
  real.collectModelCallReferencesBySourceIndex,
);

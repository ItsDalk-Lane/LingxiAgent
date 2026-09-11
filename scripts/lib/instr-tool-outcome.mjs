/**
 * shared/tool-outcome.ts 的计数包装再导出（仅基准/证据运行环境使用，见 instr-message-utils.mjs）。
 * collectToolOutcomesByCallId 为全数组扫描入口，inputLen 计入 metadataVisitedCount。
 */
import * as real from "../../shared/tool-outcome.ts";
import { getActiveHistoryReadCounters } from "./history-read-counters.mjs";

export * from "../../shared/tool-outcome.ts";

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

export const collectToolOutcomesByCallId = scanName("collectToolOutcomesByCallId", real.collectToolOutcomesByCallId);

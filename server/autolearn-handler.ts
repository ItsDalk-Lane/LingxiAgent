/**
 * autolearn-handler.ts — 踩坑自动沉淀的 turn 观察缝
 *
 * 挂在与 plan-gate / loop-bus-handlers 同一条 bus 缝上（纯观察，不进轮内）：
 * turn_start 开本回合账本 → tool_execution_end 记名+成败+输出头部（ capped ）
 * → turn_end 把汇总交给 autolearn-service 的闸门链。中止轮直接丢弃账本。
 *
 * 服务内部的闸门（开关/阈值/防抖/提炼/审查）全部异步且自吞错——
 * 观察钩子绝不把失败抛回总线。
 */
import {
  AUTOLEARN_MAX_TRACE_ENTRIES,
  AUTOLEARN_TRACE_ENTRY_CHARS,
} from "../lib/autolearn/autolearn-service.ts";

/**
 * @param {object} bus   hub.eventBus（subscribe(event, sessionPath)）
 * @param {object} deps
 * @param {(summary: object) => Promise<void>} deps.observeTurn  autolearn-service.observeTurn
 * @param {(sessionPath: string) => string} deps.getPermissionMode
 * @param {object} [deps.log]
 */
export function registerAutolearnHandler(bus, deps) {
  const log = deps?.log || console;
  /** sessionPath → { toolCalls, trace: [{name, ok, head}] } */
  const turns = new Map();

  const onTurnEnd = (sessionPath, aborted) => {
    const ledger = turns.get(sessionPath);
    turns.delete(sessionPath);
    if (!ledger) return;
    const mode = deps.getPermissionMode?.(sessionPath);
    void deps.observeTurn({
      sessionPath,
      toolCalls: ledger.toolCalls,
      trace: ledger.trace,
      aborted: aborted === true,
      permissionMode: typeof mode === "string" ? mode : null,
    }).catch((err) => {
      log.warn?.(`[autolearn] observeTurn failed: ${err?.message}`);
    });
  };

  return bus.subscribe((event, sessionPath) => {
    if (!sessionPath || !event?.type) return;
    try {
      if (event.type === "turn_start") {
        turns.set(sessionPath, { toolCalls: 0, trace: [] });
      } else if (event.type === "tool_execution_end") {
        const ledger = turns.get(sessionPath);
        if (!ledger) return;
        ledger.toolCalls += 1;
        if (ledger.trace.length < AUTOLEARN_MAX_TRACE_ENTRIES) {
          const name = typeof event.toolName === "string" ? event.toolName : "unknown";
          const ok = event.isError !== true;
          // 轨迹只带输出头部给提炼模型当线索；不带 args（可能含路径/密钥）。
          const result = event.result;
          const text = typeof result === "string"
            ? result
            : Array.isArray(result?.content)
              ? result.content.find?.((b) => b?.type === "text" && typeof b.text === "string")?.text || ""
              : "";
          ledger.trace.push({ name, ok, head: text.slice(0, AUTOLEARN_TRACE_ENTRY_CHARS) });
        }
      } else if (event.type === "turn_end") {
        if (event.aborted === true) {
          turns.delete(sessionPath);
          return;
        }
        onTurnEnd(sessionPath, false);
      }
    } catch {
      // 观察钩子不允许影响主流程
    }
  });
}

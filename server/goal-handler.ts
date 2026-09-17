/**
 * goal-handler.ts — 预算引擎的总线观察缝（阶段二·10）。
 *
 * 订阅：token_usage（每 assistant run 一次的用量记账 + 超支检查）、
 * turn_start（时间预算懒检查点）、turn_end aborted（用户中止 → 自动暂停）。
 * 与 plan-gate / autolearn-handler 同一条 bus 缝，互不知晓。
 */
export function registerGoalHandler(bus, deps: {
  goalEngine: any;
  isEnabled: () => boolean;
  log?: { warn?: (msg: string) => void };
}) {
  const log = deps?.log || console;
  return bus.subscribe((event, sessionPath) => {
    if (!sessionPath || !event?.type) return;
    if (deps.isEnabled() === false) return;
    try {
      if (event.type === "token_usage" && event.usage) {
        const total = Number(event.usage.totalTokens ?? event.usage.total);
        if (Number.isFinite(total)) {
          deps.goalEngine.recordTokenUsage(sessionPath, total);
        }
      } else if (event.type === "turn_start") {
        deps.goalEngine.tickTimeBudget(sessionPath);
      } else if (event.type === "turn_end" && event.aborted === true) {
        deps.goalEngine.pauseOnAbort(sessionPath);
      }
    } catch (err) {
      log.warn?.(`[goal] bus handling failed: ${(err as any)?.message || err}`);
    }
  });
}

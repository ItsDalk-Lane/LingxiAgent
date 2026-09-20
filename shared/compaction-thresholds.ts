/**
 * 上下文压缩双阈值契约（core / server / desktop 渲染端共享）。
 *
 * 两条压缩线：
 *   - ASK 线（50%）：用量跨越后向渲染端发 `compaction_suggested`，由用户
 *     在弹窗里决定是否立即压缩。core 不等待回复，不阻塞进行中的 run。
 *   - FORCE 线（80%）：用量达到后强制自动压缩（reserve 触发点随之对齐为
 *     窗口的 80%），完成后发 `compaction_auto` 通知用户已被压缩。
 *
 * 两个阈值都被 core 的 mid-run 检查（每个 turn 边界）消费；FORCE 线同时
 * 覆盖 SDK post-run 的 reserve 检查。硬截断兜底（85%）保持不变。
 */

/** 询问线：跨越后弹窗询问用户是否压缩。 */
export const COMPACTION_ASK_RATIO = 0.5;

/** 强制线：达到后静默自动压缩，完成后通知。 */
export const COMPACTION_FORCE_RATIO = 0.8;

/** core → 渲染端：50% 询问事件。 */
export const COMPACTION_SUGGESTED_EVENT = "compaction_suggested";

/** core → 渲染端：80% 强制压缩完成事件。 */
export const COMPACTION_AUTO_EVENT = "compaction_auto";

/** 同一会话两次 50% 询问之间的最小 token 增量（占窗口比例），防止每个 turn 反复弹窗。 */
export const COMPACTION_ASK_REPROMPT_DELTA_RATIO = 0.05;

/** 视为"发生过压缩"的 token 骤降幅度（占窗口比例），用于重置询问状态。 */
export const COMPACTION_ASK_RESET_DROP_RATIO = 0.2;

export interface CompactionSuggestedPayload {
  type: typeof COMPACTION_SUGGESTED_EVENT;
  sessionPath: string;
  percent: number;
  askPercent: number;
  forcePercent: number;
}

export interface CompactionAutoPayload {
  type: typeof COMPACTION_AUTO_EVENT;
  sessionPath: string;
  percentBefore: number;
  forcePercent: number;
}

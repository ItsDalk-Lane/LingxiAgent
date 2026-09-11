/**
 * 把毫秒时长格式化成「Xs」或「XmYs」。负数 clamp 到 0（防时钟偏差导致 now < startedAt）。
 * 统一时长口径，供 WorkflowCard / ActivityPanel 等显示运行时长，避免各处内联重复。
 */
export function formatElapsed(ms: number): string {
  const totalSec = Math.max(0, Math.round(ms / 1000));
  if (totalSec >= 60) {
    return `${Math.floor(totalSec / 60)}m${totalSec % 60}s`;
  }
  return `${totalSec}s`;
}

export type DurationTranslate = (key: string, vars: Record<string, string | number>) => string;

function pad2(value: number): string {
  return String(value).padStart(2, '0');
}

/**
 * 运行中秒表：`duration.seconds` / `duration.minutes` / `duration.hours` 三档语言模板。
 *
 * 与 formatElapsed 的分工：那个是「1m5s」这类紧凑读数（卡片、面板），这里是给正在
 * 走的秒表用的本地化读数——「1分05秒」。跨过分档后秒数补零，避免 1分5秒 每秒跳宽度；
 * 不足一分钟不补零（「7秒」而不是「07秒」），与 DSH 同款状态行一致。
 */
export function formatRunningDuration(ms: number, t: DurationTranslate): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor(total / 60) % 60;
  const seconds = total % 60;
  if (hours > 0) return t('duration.hours', { hours, minutes: pad2(minutes), seconds: pad2(seconds) });
  if (minutes > 0) return t('duration.minutes', { minutes, seconds: pad2(seconds) });
  return t('duration.seconds', { seconds });
}

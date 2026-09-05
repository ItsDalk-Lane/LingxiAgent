/**
 * turn-usage.ts — 聊天 assistant 轮次「用量/用时」胶囊的聚合与格式化契约。
 *
 * 移植自 design-review/harness-usage-pills-reference/（DeepSeek Harness
 * github.com/deepseek-ai/deepseek-harness，MIT 许可；token-format.ts 与
 * message-chrome.ts 的算法 1:1 移植，仅去掉 locale seat、改用组件内中文常量）。
 * 适配点：数据源为本项目 observability 用量账本按轮聚合——
 * `POST /api/model-observability/query/calls`（since inclusive / until
 * exclusive 绑定 started_at）返回的 ModelObservabilityCallListItem[]；
 * 所有字段无事实为 null（不冒充 0）。
 */

import type { ModelObservabilityCallListItem } from '../../../../../shared/model-observability-api-contract.ts';
import type { AssistantTurnProjection, ChatListItem } from '../../stores/chat-types';

/** 单轮用量/用时的展示聚合结果；`null` 字段 = 无事实，对应弹窗行/胶囊不渲染。 */
export interface TurnUsageStats {
  /** 未缓存输入 = Σ inputUncachedTokens（known 值求和）。 */
  uncachedInputTokens: number | null;
  /** 缓存读取 = Σ cacheReadTokens。 */
  cacheReadTokens: number | null;
  /** 缓存写入 = Σ cacheWriteTokens（>0 才显示行）。 */
  cacheWriteTokens: number | null;
  /** 输出 = Σ outputTokens。 */
  outputTokens: number | null;
  /** 其中推理 = Σ reasoningTokens。 */
  reasoningTokens: number | null;
  /** 总量 = Σ totalTokens；用量胶囊的锚点字段。 */
  totalTokens: number | null;
  /** 缓存命中 = 缓存读取 ÷ (总量 − 输出)，展示字符串（防 99.95→100 失真）。 */
  cacheHitPercent: string | null;
  /** 参与聚合的调用里出现过的 provider/modelId（去重，保持首现顺序）。 */
  modelLabels: string[];
  /**
   * 本轮总用时。实时轮 = 投影 completedAt−startedAt（可信）；历史轮聊天侧只有
   * 「回复开始落盘」时刻，真实轮末不可知，改用账本事实推导：最后一个模型调用
   * ended_at − 轮开始。无事实为 null，用时胶囊整体不渲染（不渲染 0）。
   */
  runMs: number | null;
  /** 输出速度 = Σ输出token ÷ ΣdurationMs（同批 usage-present 调用）。 */
  tokensPerSecond: number | null;
  /**
   * 首 token 用时 = 本轮最早一次 provider 响应到达（first_response_at）− 轮开始
   * （用户消息时刻）。响应到达 ≈ 首 token 到达的最近似事实；无事实为 null。
   */
  ttftMs: number | null;
}

/** 轮次窗口：startedAt/completedAt 限定账本查询区间（since 含/until 不含）；
 *  runMs 交给聚合方作展示用时，历史路径省略（由账本 ended_at 推导）。 */
export interface TurnUsageWindow {
  startedAt: number;
  completedAt: number;
  runMs?: number;
}

/** completed_* 才算完成轮（streaming/failed/aborted 不出胶囊）。 */
export function isCompletedAssistantTurn(turn: AssistantTurnProjection | undefined): boolean {
  return !!turn && turn.status.startsWith('completed');
}

/**
 * 胶囊渲染资格：完成轮 + 有起止时间戳。返回轮次窗口，不符合为 null。
 * 历史会话/旧数据无 turnProjection 或缺时间戳 → 无胶囊（不渲染 0）。
 */
/**
 * 胶囊渲染资格：完成轮 + 有起止时间戳。返回轮次窗口，不符合为 null。
 * 历史会话/旧数据无 turnProjection 或缺时间戳 → 无胶囊（不渲染 0）。
 */
export function turnUsageWindow(turn: AssistantTurnProjection | undefined): TurnUsageWindow | null {
  if (!turn || !isCompletedAssistantTurn(turn)) return null;
  const { startedAt, completedAt } = turn;
  if (
    typeof startedAt !== 'number' || !Number.isFinite(startedAt)
    || typeof completedAt !== 'number' || !Number.isFinite(completedAt)
    || completedAt < startedAt
  ) return null;
  // 流式收尾写入的 completedAt 可信，直接作为展示用时。
  return { startedAt, completedAt, runMs: Math.max(0, completedAt - startedAt) };
}

/**
 * 邻居回退窗口：投影未携带时间戳时的轮次窗口推导。
 *
 * 现状（2026-09-05 实测）：三个 projectAssistantTurn 调用点只有流式收尾
 * （use-stream-buffer commitLiveRun）会带时间戳；历史重建（history-builder，
 * 非本任务白名单）永远不带。历史消息 entry timestamp = 回复**开始**落盘时刻
 * （实测 19:36:28，而轮实际 19:36:07→19:37:34），不能当上界用——会把本轮
 * 后续模型调用全切掉。因此窗口取轮次边界：startedAt=往回最近一条 user 消息
 * 时刻；completedAt=往后最近一条 user 消息时刻−1（until 不含），没有后续
 * user 消息（最后一轮）则取当前时刻，与实时收尾的 commit 时刻口径一致。
 */
export function turnUsageWindowFromNeighbors(
  items: readonly ChatListItem[],
  messageId: string,
): TurnUsageWindow | null {
  const index = items.findIndex((item) => (
    item.type === 'message' && item.data.id === messageId && item.data.role === 'assistant'
  ));
  if (index < 0) return null;
  const assistant = items[index].type === 'message' ? items[index].data : null;
  if (!assistant) return null;
  // 完成轮门槛与主路径一致；无投影的极老会话不猜状态。
  if (!isCompletedAssistantTurn(assistant.turnProjection)) return null;

  let startedAt: number | null = null;
  for (let i = index - 1; i >= 0; i -= 1) {
    const item = items[i];
    if (item.type !== 'message' || item.data.role !== 'user') continue;
    const ts = item.data.timestamp;
    startedAt = typeof ts === 'number' && Number.isFinite(ts) ? ts : null;
    break;
  }
  if (startedAt === null) return null;

  let nextUserTs: number | null = null;
  for (let i = index + 1; i < items.length; i += 1) {
    const item = items[i];
    if (item.type !== 'message' || item.data.role !== 'user') continue;
    const ts = item.data.timestamp;
    if (typeof ts === 'number' && Number.isFinite(ts)) nextUserTs = ts;
    break;
  }

  let completedAt: number;
  if (nextUserTs !== null) {
    completedAt = nextUserTs - 1;
  } else {
    // 最后一轮：截至当前时刻（本轮全部调用 START 于 startedAt 之后、now 之前，
    // 与实时路径 completedAt=收尾时刻同口径）。
    completedAt = Date.now();
  }
  if (completedAt < startedAt) return null;
  // 历史侧 completedAt 只是查询边界；展示用时由账本 ended_at 推导（runMs 省略）。
  return { startedAt, completedAt };
}

function sumKnown(values: Array<number | null | undefined>): number | null {
  let total: number | null = null;
  for (const value of values) {
    if (typeof value !== 'number' || !Number.isFinite(value)) continue;
    total = (total ?? 0) + value;
  }
  return total;
}

/**
 * 按轮聚合用量账本。仅 usage.availability=present 的调用参与求和；
 * durationMs 取同一批调用（保证 TPS 分子分母同源）。
 * 无任何 usage 事实（老会话/未记录）→ 返回 null，两个胶囊都不显示。
 *
 * 展示用时 runMs：window.runMs（实时路径，可信）优先；否则用账本事实推导
 * = 本轮最后一个调用 ended_at − startedAt（历史路径轮末不可知的最近似真值）。
 */
export function aggregateTurnUsage(
  calls: readonly ModelObservabilityCallListItem[],
  window?: TurnUsageWindow | null,
): TurnUsageStats | null {
  const presented = calls.filter((call) => call.usage?.availability === 'present' && call.usage.summary);
  if (presented.length === 0) return null;
  const summaries = presented.map((call) => call.usage.summary!);

  const uncachedInputTokens = sumKnown(summaries.map((s) => s.inputUncachedTokens));
  const cacheReadTokens = sumKnown(summaries.map((s) => s.cacheReadTokens));
  const cacheWriteTokens = sumKnown(summaries.map((s) => s.cacheWriteTokens));
  const outputTokens = sumKnown(summaries.map((s) => s.outputTokens));
  const reasoningTokens = sumKnown(summaries.map((s) => s.reasoningTokens));
  const totalTokens = sumKnown(summaries.map((s) => s.totalTokens));
  const durationTotalMs = sumKnown(presented.map((call) => call.durationMs));

  const modelLabels: string[] = [];
  for (const call of presented) {
    const provider = call.model?.provider;
    const modelId = call.model?.modelId;
    if (!provider && !modelId) continue;
    const label = `${provider ?? '?'}/${modelId ?? '?'}`;
    if (!modelLabels.includes(label)) modelLabels.push(label);
  }

  // 缓存命中 = 缓存读取 ÷ (总量 − 输出)；分母非正或无缓存读取事实 → 无该行。
  const cacheHitPercent = cacheReadTokens !== null && totalTokens !== null && outputTokens !== null
    ? formatCacheHitPercent(cacheReadTokens, totalTokens - outputTokens, 1)
    : null;

  // TPS = Σ输出 ÷ ΣdurationMs；缺输出或缺正时长 → 无该行。
  const tokensPerSecond = outputTokens !== null
    && durationTotalMs !== null && durationTotalMs > 0
    ? outputTokens / (durationTotalMs / 1000)
    : null;

  // 展示用时：window.runMs（实时收尾，可信）> 账本推导（最后一个调用 ended_at
  // − 轮开始）> null（用时胶囊整体不渲染）。
  const windowRunMs = typeof window?.runMs === 'number' && Number.isFinite(window.runMs)
    ? window.runMs
    : null;
  const windowStart = typeof window?.startedAt === 'number' && Number.isFinite(window.startedAt)
    ? window.startedAt
    : null;
  let lastCallEndedAt: number | null = null;
  for (const call of presented) {
    if (typeof call.endedAt !== 'string') continue;
    const endMs = Date.parse(call.endedAt);
    if (!Number.isFinite(endMs)) continue;
    lastCallEndedAt = lastCallEndedAt === null ? endMs : Math.max(lastCallEndedAt, endMs);
  }
  let runMs: number | null;
  if (windowRunMs !== null && windowStart !== null) {
    runMs = Math.max(0, windowRunMs);
  } else if (lastCallEndedAt !== null && windowStart !== null) {
    runMs = Math.max(0, lastCallEndedAt - windowStart);
  } else {
    runMs = null;
  }

  // 首 token 用时：本轮最早一次响应到达 − 轮开始（用户消息时刻）；任一事实缺失
  // 或为负（时钟漂移）→ null，TTFT 行整体不渲染。
  let firstResponseMs: number | null = null;
  for (const call of presented) {
    if (typeof call.firstResponseAt !== 'string') continue;
    const ms = Date.parse(call.firstResponseAt);
    if (!Number.isFinite(ms)) continue;
    firstResponseMs = firstResponseMs === null ? ms : Math.min(firstResponseMs, ms);
  }
  const ttftMs = firstResponseMs !== null && windowStart !== null && firstResponseMs >= windowStart
    ? firstResponseMs - windowStart
    : null;

  return {
    uncachedInputTokens,
    cacheReadTokens,
    cacheWriteTokens,
    outputTokens,
    reasoningTokens,
    totalTokens,
    cacheHitPercent,
    modelLabels,
    runMs,
    tokensPerSecond,
    ttftMs,
  };
}

/* ── 数字格式化（照搬 token-format.ts / message-chrome.ts 算法）────────── */

/**
 * 紧凑 token 计数：517 / 12.2K / 517K / 1.2M（胶囊标签用）。
 */
export function formatTokensCompact(value: number): string {
  const scaled = (candidate: number): string =>
    candidate >= 100 ? String(Math.round(candidate)) : String(Math.round(candidate * 10) / 10);
  if (value < 1_000) return String(value);
  if (value < 1_000_000) return `${scaled(value / 1_000)}K`;
  return `${scaled(value / 1_000_000)}M`;
}

const GROUP_SEPARATOR = ',';

/**
 * 千分位精确整数（弹窗用，不四舍五入）。
 */
export function formatExactTokens(value: number): string {
  const digits = String(value);
  const groups: string[] = [];
  for (let end = digits.length; end > 0; end -= 3) {
    groups.unshift(digits.slice(Math.max(0, end - 3), end));
  }
  return groups.join(GROUP_SEPARATOR);
}

/** Round a cache-read ratio to exact percentage units, with positive ties rounded up. */
function roundedPercentUnits(cacheReadTokens: number, denominator: number, decimalPlaces: 0 | 1): number {
  const unitsPerPercent = decimalPlaces === 0 ? 1 : 10;
  const scale = unitsPerPercent * 100;
  const doubledScale = scale * 2;
  const denominatorQuotient = Math.floor(denominator / doubledScale);
  const denominatorRemainder = denominator % doubledScale;
  let lower = 0;
  let upper = scale;
  while (lower < upper) {
    const candidate = Math.floor((lower + upper + 1) / 2);
    const factor = candidate * 2 - 1;
    const threshold = factor * denominatorQuotient
      + Math.ceil(factor * denominatorRemainder / doubledScale);
    if (cacheReadTokens >= threshold) lower = candidate;
    else upper = candidate - 1;
  }
  return lower;
}

function displayPercentUnits(units: number, decimalPlaces: 0 | 1): string {
  if (decimalPlaces === 0) return String(units);
  const whole = Math.floor(units / 10);
  const tenths = units % 10;
  return tenths === 0 ? String(whole) : `${whole}.${tenths}`;
}

/**
 * Display-ready cache-hit share without rounding a partial hit to 100%.
 * （1:1 移植：部分命中永不因四舍五入显示成 100%。）
 */
export function formatCacheHitPercent(
  cacheReadTokens: number,
  promptTokens: number,
  decimalPlaces: 0 | 1 = 0,
): string | null {
  if (promptTokens === 0) return null;
  if (cacheReadTokens >= promptTokens) return '100';

  const missedInputTokens = promptTokens - cacheReadTokens;
  if (missedInputTokens === 0) return '100';

  const roundedUnits = roundedPercentUnits(cacheReadTokens, promptTokens, decimalPlaces);
  const fullHitUnits = decimalPlaces === 0 ? 100 : 1_000;
  if (roundedUnits < fullHitUnits) return displayPercentUnits(roundedUnits, decimalPlaces);

  let distinguishingPlaces = 1;
  let scaledDoubleGap = missedInputTokens * 200;
  const denominatorTens = Math.floor(promptTokens / 10);
  while (scaledDoubleGap <= denominatorTens) {
    scaledDoubleGap *= 10;
    distinguishingPlaces += 1;
  }
  const denominatorOnes = promptTokens % 10;
  let roundedLoss = 5;
  for (let loss = 1; loss < 5; loss += 1) {
    const factor = loss * 2 + 1;
    const threshold = factor * denominatorTens + Math.floor(factor * denominatorOnes / 10);
    if (scaledDoubleGap <= threshold) {
      roundedLoss = loss;
      break;
    }
  }
  return `99.${'9'.repeat(distinguishingPlaces - 1)}${10 - roundedLoss}`;
}

/**
 * 整秒经过时长：{minutes}分{seconds}秒 / {seconds}秒（负数钳到 0）。
 */
export function formatRunDuration(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return minutes > 0
    ? `${minutes}分${String(seconds).padStart(2, '0')}秒`
    : `${seconds}秒`;
}

/**
 * 吞吐数字：≥10 取整，<10 一位小数（负数钳到 0）。
 */
export function formatTokensPerSecond(tps: number): string {
  const clamped = Math.max(0, tps);
  return clamped >= 10 ? String(Math.round(clamped)) : String(Math.round(clamped * 10) / 10);
}

/**
 * 首 token/延迟秒数：不足 10 秒一位小数，超过取整秒（单位「秒」由模板携带）。
 */
export function formatLatencySeconds(ms: number): string {
  const s = Math.max(0, ms) / 1000;
  return s < 10 ? String(Math.round(s * 10) / 10) : String(Math.round(s));
}

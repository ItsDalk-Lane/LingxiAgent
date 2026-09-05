/**
 * trajectory-record.ts — 轨迹记录数据与格式化契约。
 *
 * 移植自 dsh-desktop packages/client/ui-trajectory/src/client/trajectory-record.ts
 * （DeepSeek Harness 系源码，MIT 许可，见 dsh-desktop/THIRD_PARTY_NOTICES.md）。
 * 适配点：ConversationPromptSnapshot（dsh runtime 类型）换成本地最小快照形状；
 * 新增 observabilityCallId 把记录行关联到灵犀观测库的 model call（载荷懒加载用）。
 */

import type { HTMLAttributes } from 'react';

/** 轨迹记录 kind 闭集。 */
export type TrajectoryCellKind =
  | 'system'
  | 'user'
  | 'context'
  | 'compacted'
  | 'message'
  | 'tool'
  | 'subtool';

/** 推导 assistant 请求计时拆解（响应到达/生成/吞吐）所需的记录事实。 */
export interface AssistantMetricDetail {
  timingRecorded: boolean;
  stepStartTime: number | null;
  /** provider 响应到达时刻（epoch ms）；无事实为 null——不虚构。 */
  responseArrivalTime: number | null;
  completedTime: number | null;
  usageProvided: boolean;
  outputTokens: number | null;
}

/** 检查器里按模型顺序保留的单个内容块。 */
export interface TrajectorySourceBlock {
  type: string;
  content: string;
  imageSrc?: string;
  imageAlt?: string;
  callId?: string;
  toolName?: string;
}

/** 系统提示词/工具目录快照（载荷捕获开启时由 semantic_request 载荷喂入）。 */
export interface TrajectoryPromptSnapshot {
  system: string;
  tools: readonly {
    name: string;
    description: string;
    parameters: object;
  }[];
}

/** 单条轨迹记录的数据与可选展示属性。 */
export interface TrajectoryCellProps extends HTMLAttributes<HTMLDivElement> {
  /** 1-based 记录序号，展示为 `#N`。 */
  index: number;
  /** 无单一来源事件拥有记录生命周期时的投影稳定身份。 */
  recordId?: string;
  kind: TrajectoryCellKind;
  /** 非 Markdown 摘要或前缀；溢出时 CSS 省略。 */
  text: string;
  /** 由消费方转成单行摘要的原始 Markdown 源。 */
  previewMarkdown?: string;
  /** 该 user 记录是否开启新的模型轮次。 */
  opensTurn?: boolean;
  /** 来源会话事件 seq（跨记录导航）。 */
  sourceSeq?: number;
  /** user 角色消息或上下文注入的生产者角色与名字。 */
  messageSource?: unknown;
  /** 无可见记录的辅助请求分隔锚点。 */
  requestOnly?: boolean;
  /** 检查器用的完整请求/消息内容。 */
  inputDetail?: string;
  /** SYSTEM 记录引入的完整系统提示词/工具目录状态。 */
  promptDetail?: TrajectoryPromptSnapshot;
  /** 被 SYSTEM 更新替换的旧系统提示词/工具目录状态。 */
  previousPromptDetail?: TrajectoryPromptSnapshot;
  /** 检查器用的完整 assistant/工具结果内容。 */
  outputDetail?: string;
  /** 检查器用的完整 assistant 推理内容。 */
  thinkingDetail?: string;
  /** 检查器用的按源顺序排列的原始消息块。 */
  sourceBlocks?: readonly TrajectorySourceBlock[];
  /** 检查器用的按源顺序排列的原始工具结果块。 */
  outputBlocks?: readonly TrajectorySourceBlock[];
  /** 调用时刻模型可见的工具 schema。 */
  schemaDetail?: string;
  /** 仅 assistant 的计时与 token 事实。 */
  assistantMetrics?: AssistantMetricDetail;
  /** 与同一记录内调用配对的工具结果摘要。 */
  result?: string;
  /** 由消费方转成工具结果摘要的原始 Markdown 源。 */
  resultPreviewMarkdown?: string;
  /** 把消息源块关联到工具记录的调用 id。 */
  callId?: string;
  /** 仅工具的结果失败态。 */
  isError?: boolean;
  /** 自身耗时秒数；未知为 `null`。 */
  timeSeconds: number | null;
  /** 该操作真实开始的 Unix epoch 毫秒；未知省略。 */
  startedAt?: number | null;
  /** 仅消息的 prompt token 数。 */
  input?: number;
  /** 仅消息的缓存读 token 数。 */
  cacheRead?: number;
  /** 仅消息的缓存写 token 数。 */
  cacheWrite?: number;
  /** 仅消息的 completion token 数。 */
  output?: number;
  /** 仅消息的 reasoning token 数。 */
  think?: number;
  /** 灵犀扩展：观测库 model call id（检查器 Payload tab 懒加载关联键）。 */
  observabilityCallId?: string;
  /** 灵犀扩展：计时事实来源（session = 会话条目时间戳；observability = 观测库计时）。 */
  timingSource?: 'session' | 'observability';
  /** 遗留独立 cell 的选中态渲染。 */
  selected?: boolean;
}

/**
 * 解析能在「向前补页追加更早记录」后存活的身份。
 */
export function trajectoryRecordId(cell: TrajectoryCellProps): string {
  if (cell.recordId !== undefined) return cell.recordId;
  if (cell.callId !== undefined) return `${cell.kind}\u0000call\u0000${cell.callId}`;
  if (cell.sourceSeq !== undefined) return `${cell.kind}\u0000seq\u0000${cell.sourceSeq}`;
  return `${cell.kind}\u0000index\u0000${cell.index}`;
}

/**
 * 千分位格式化毫秒耗时。
 */
export function formatDurationMillis(milliseconds: number | null): string {
  if (milliseconds === null || !Number.isFinite(milliseconds)) return '—';
  const integer = String(Math.round(milliseconds));
  return `${integer.replace(/\B(?=(\d{3})+(?!\d))/g, ',')} ms`;
}

/**
 * 把秒耗时格式化为毫秒标签。
 */
export function formatElapsedSeconds(seconds: number | null): string {
  return formatDurationMillis(seconds === null ? null : seconds * 1000);
}

/**
 * Canonical Assistant Turn Assembler / Projector
 *
 * 唯一职责：把实时或历史的助手回合数据装配成唯一的 canonical block 集合，
 * 再按 process / answer / result / control 四区投影。渲染层只消费投影结果，
 * 不再执行 segments + legacyBlocks 双源合并去重（不变量 3）。
 *
 * Block ID 策略（不变量 4/5，任务书 §11）：
 * - 文本/推理：idPrefix + ':segment:' + 服务器 segmentId（如 assistant:1:text:0）
 * - 工具：idPrefix + ':tool_group:tools:' + toolCallId 列表
 * - mood：idPrefix + ':mood:N'（canonical 流由服务器 moodOrdinal 决定 N；
 *   legacy 流按出现顺序编号）——首次创建后绝不变更
 * - 持久化 entry（sourceEntryId / turnInputEntryId）只是 metadata，
 *   绑定时不得重写 block.id
 */

import type {
  AssistantTurnProjection,
  AssistantTurnStatus,
  ContentBlock,
  ContentLifecycle,
  ContentSurfaceRole,
} from '../stores/chat-types';
import type { LiveAssistantSegment } from '../stores/live-turn-store';
import { normalizeContentBlocks } from './content-semantics';
import { resolveAssistantTurnOutcome } from './turn-outcome';

export interface TurnProjectionDiagnostic {
  code: 'unresolved_phase_fallback';
  segmentId: string;
  fallbackPhase: 'final_answer';
}

export interface ProjectAssistantTurnInput {
  idPrefix: string;
  inputMessageId?: string | null;
  assistantMessageIds: string[];
  segments: readonly LiveAssistantSegment[];
  /** 兼容输入：旧服务器/旧历史走 legacy 管线产出的块（tool/mood/file/...）。 */
  legacyBlocks: readonly ContentBlock[];
  status: AssistantTurnStatus;
  startedAt?: number;
  completedAt?: number;
}

export interface ProjectAssistantTurnResult {
  blocks: ContentBlock[];
  projection: AssistantTurnProjection;
  diagnostics: TurnProjectionDiagnostic[];
}

const SURFACE_ORDER: Record<ContentSurfaceRole, number> = {
  process: 0,
  answer: 1,
  result: 2,
  control: 3,
};

export function segmentBlockId(idPrefix: string, segmentId: string): string {
  return `${idPrefix}:segment:${segmentId}`;
}

function projectSegment(
  segment: LiveAssistantSegment,
  input: ProjectAssistantTurnInput,
  diagnostics: TurnProjectionDiagnostic[],
): ContentBlock {
  const lifecycle: ContentLifecycle = input.status === 'streaming'
    ? segment.lifecycle
    : 'sealed';
  if (segment.semanticPhase === 'reasoning' || segment.kind === 'reasoning') {
    return {
      id: segmentBlockId(input.idPrefix, segment.id),
      type: 'thinking',
      content: segment.source,
      ...(segment.deferred ? { deferred: segment.deferred } : {}),
      sealed: lifecycle === 'sealed',
      semanticPhase: 'reasoning',
      surfaceRole: 'process',
      lifecycle,
    };
  }

  let semanticPhase: 'commentary' | 'final_answer';
  if (segment.semanticPhase === 'unresolved') {
    if (input.status === 'streaming') {
      semanticPhase = 'commentary';
    } else {
      semanticPhase = 'final_answer';
      diagnostics.push({
        code: 'unresolved_phase_fallback',
        segmentId: segment.id,
        fallbackPhase: 'final_answer',
      });
    }
  } else {
    semanticPhase = segment.semanticPhase === 'commentary' ? 'commentary' : 'final_answer';
  }
  return {
    id: segmentBlockId(input.idPrefix, segment.id),
    type: 'text',
    source: segment.source,
    ...(segment.deferred ? { deferred: segment.deferred } : {}),
    semanticPhase,
    surfaceRole: semanticPhase === 'commentary' ? 'process' : 'answer',
    lifecycle,
  };
}

/**
 * Canonical 收口合并（不变量 3）：
 * segment 文本/推理优先；legacy text/thinking 只在没有对应 canonical
 * segment 时保留（旧服务器兼容），mood/tool/file 等其余块全部保留。
 * 一份模型内容只允许一个 canonical 表示（不变量 1）。
 */
function mergeCanonicalBlocks(
  segmentBlocks: ContentBlock[],
  legacyBlocks: ContentBlock[],
): ContentBlock[] {
  const hasSemanticText = segmentBlocks.some((block) => block.type === 'text');
  const hasSemanticReasoning = segmentBlocks.some((block) => block.type === 'thinking');
  return [
    ...segmentBlocks,
    ...legacyBlocks.filter((block) => !(
      (hasSemanticText && block.type === 'text')
      || (hasSemanticReasoning && block.type === 'thinking')
    )),
  ];
}

/** 稳定排序：process -> answer -> result -> control，同区内保持到达顺序。 */
function sortBySurfaceRole(blocks: ContentBlock[]): ContentBlock[] {
  return blocks
    .map((block, index) => ({ block, index }))
    .sort((a, b) => (
      SURFACE_ORDER[a.block.surfaceRole || 'result'] - SURFACE_ORDER[b.block.surfaceRole || 'result']
      || a.index - b.index
    ))
    .map((entry) => entry.block);
}

/** 把实时或历史助手数据投影成固定的过程、答案、结果、控制四区。 */
export function projectAssistantTurn(input: ProjectAssistantTurnInput): ProjectAssistantTurnResult {
  const diagnostics: TurnProjectionDiagnostic[] = [];
  const segmentBlocks = input.segments.map((segment) => projectSegment(segment, input, diagnostics));
  const turnLifecycle: ContentLifecycle = input.status === 'streaming' ? 'streaming' : 'sealed';
  const legacyBlocks = normalizeContentBlocks(input.legacyBlocks, {
    idPrefix: input.idPrefix,
    turnLifecycle,
  });
  const allBlocks = sortBySurfaceRole(mergeCanonicalBlocks(segmentBlocks, legacyBlocks));
  const processBlocks = allBlocks.filter((block) => block.surfaceRole === 'process');
  const answerBlocks = allBlocks.filter((block) => block.surfaceRole === 'answer');
  const resultBlocks = allBlocks.filter((block) => block.surfaceRole === 'result');
  const controlBlocks = allBlocks.filter((block) => block.surfaceRole === 'control');

  // Turn Outcome 是"未生成最终回复"的唯一裁决来源（不变量 7）。
  const resolved = resolveAssistantTurnOutcome({ blocks: allBlocks, status: input.status });
  const needsTurnStatus = resolved.outcome === 'completed_without_user_output'
    || resolved.outcome === 'failed'
    || resolved.outcome === 'aborted';
  if (needsTurnStatus && !resultBlocks.some((block) => block.type === 'turn_status')) {
    const statusBlock: Extract<ContentBlock, { type: 'turn_status' }> = input.status === 'failed'
      ? {
        id: `${input.idPrefix}:turn-status:failed`,
        type: 'turn_status',
        status: 'failed',
        surfaceRole: 'result',
        lifecycle: 'sealed',
      }
      : input.status === 'aborted'
        ? {
          id: `${input.idPrefix}:turn-status:aborted`,
          type: 'turn_status',
          status: 'aborted',
          surfaceRole: 'result',
          lifecycle: 'sealed',
        }
        : {
          id: `${input.idPrefix}:turn-status:missing-final-answer`,
          type: 'turn_status',
          status: 'missing_final_answer',
          surfaceRole: 'result',
          lifecycle: 'sealed',
        };
    resultBlocks.push(statusBlock);
    allBlocks.push(statusBlock);
  }

  const projection: AssistantTurnProjection = {
    id: `${input.idPrefix}:turn`,
    inputMessageId: input.inputMessageId || null,
    assistantMessageIds: [...input.assistantMessageIds],
    processBlockIds: processBlocks.map((block) => block.id!),
    answerBlockIds: answerBlocks.map((block) => block.id!),
    resultBlockIds: resultBlocks.map((block) => block.id!),
    controlBlockIds: controlBlocks.map((block) => block.id!),
    status: input.status,
    outcome: resolved.outcome,
    ...(resolved.missingFinalAnswerReason ? { missingFinalAnswerReason: resolved.missingFinalAnswerReason } : {}),
    ...(diagnostics.length > 0 ? { diagnostics } : {}),
    ...(input.startedAt !== undefined ? { startedAt: input.startedAt } : {}),
    ...(input.completedAt !== undefined ? { completedAt: input.completedAt } : {}),
  };

  return { blocks: allBlocks, projection, diagnostics };
}

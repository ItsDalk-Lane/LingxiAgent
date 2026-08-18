/**
 * Turn Outcome Resolver - 回合结局的唯一裁决（任务书不变量 7）
 *
 * "未生成最终回复" 只能来源于 completed_without_user_output，
 * 渲染层不得再用 answerBlocks.length === 0 之类的局部条件自行猜测。
 * 纯函数：只读显式语义字段（semanticPhase / surfaceRole / type），
 * 永远不通过自然语言把 commentary 升级成 final_answer（不变量 8）。
 */

import type {
  AssistantTurnOutcome,
  AssistantTurnStatus,
  ContentBlock,
  MissingFinalAnswerReason,
} from '../stores/chat-types';

export type { AssistantTurnOutcome, MissingFinalAnswerReason };

export interface ResolvedAssistantTurnOutcome {
  outcome: AssistantTurnOutcome;
  /** 仅 completed_without_user_output 时给出可区分的诊断原因（开发可观测）。 */
  missingFinalAnswerReason?: MissingFinalAnswerReason;
}

function textBody(block: ContentBlock): string {
  if (block.type !== 'text') return '';
  // TextDecorator 兼容形态：新块写 source；旧会话可能只有 html。
  const source = (block as { source?: string }).source;
  if (typeof source === 'string') return source;
  return (block as { html?: string }).html || '';
}

function hasDisplayableAnswer(blocks: readonly ContentBlock[]): boolean {
  return blocks.some((block) => (
    block.type === 'text'
    && (block.semanticPhase === 'final_answer' || block.surfaceRole === 'answer')
    && textBody(block).trim().length > 0
  ));
}

function hasEmptyFinalAnswer(blocks: readonly ContentBlock[]): boolean {
  return blocks.some((block) => (
    block.type === 'text'
    && (block.semanticPhase === 'final_answer' || block.surfaceRole === 'answer')
  ));
}

function isPendingControlBlock(block: ContentBlock): boolean {
  switch (block.type) {
    case 'session_confirmation':
    case 'settings_confirm':
    case 'cron_confirm':
    case 'suggestion_card':
      return block.status === 'pending';
    case 'interactive_card':
      return true;
    default:
      return false;
  }
}

function isResultBlock(block: ContentBlock): boolean {
  if (block.type === 'turn_status') return false;
  return block.surfaceRole === 'result' || (
    block.surfaceRole === undefined
    && (block.type === 'file' || block.type === 'media_generation' || block.type === 'artifact' || block.type === 'screenshot')
  );
}

export function resolveAssistantTurnOutcome(
  input: { blocks: readonly ContentBlock[]; status: AssistantTurnStatus },
): ResolvedAssistantTurnOutcome {
  if (input.status === 'streaming') return { outcome: 'streaming' };
  if (input.status === 'failed') return { outcome: 'failed' };
  if (input.status === 'aborted') return { outcome: 'aborted' };

  const blocks = input.blocks.filter((block) => block.type !== 'turn_status');

  if (hasDisplayableAnswer(blocks)) return { outcome: 'completed_with_answer' };
  if (blocks.some(isResultBlock)) return { outcome: 'completed_with_result' };
  if (blocks.some(isPendingControlBlock)) return { outcome: 'completed_with_control' };

  if (blocks.length === 0) {
    return { outcome: 'completed_without_user_output', missingFinalAnswerReason: 'no_final_answer_segment' };
  }
  if (hasEmptyFinalAnswer(blocks)) {
    return { outcome: 'completed_without_user_output', missingFinalAnswerReason: 'empty_final_answer' };
  }
  return { outcome: 'completed_without_user_output', missingFinalAnswerReason: 'only_process_blocks' };
}

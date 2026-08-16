import type {
  AssistantTurnProjection,
  AssistantTurnStatus,
  ContentBlock,
  ContentLifecycle,
} from '../stores/chat-types';
import type { LiveAssistantSegment } from '../stores/live-turn-store';
import { normalizeContentBlocks } from './content-semantics';
import { renderMarkdown } from './markdown';

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

function segmentBlockId(idPrefix: string, segmentId: string): string {
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
    html: renderMarkdown(segment.source),
    semanticPhase,
    surfaceRole: semanticPhase === 'commentary' ? 'process' : 'answer',
    lifecycle,
  };
}

function turnStatusBlock(
  input: ProjectAssistantTurnInput,
): Extract<ContentBlock, { type: 'turn_status' }> | null {
  if (input.status === 'streaming') return null;
  if (input.status === 'failed') {
    return {
      id: `${input.idPrefix}:turn-status:failed`,
      type: 'turn_status',
      status: 'failed',
      surfaceRole: 'result',
      lifecycle: 'sealed',
    };
  }
  if (input.status === 'aborted') {
    return {
      id: `${input.idPrefix}:turn-status:aborted`,
      type: 'turn_status',
      status: 'aborted',
      surfaceRole: 'result',
      lifecycle: 'sealed',
    };
  }
  return {
    id: `${input.idPrefix}:turn-status:missing-final-answer`,
    type: 'turn_status',
    status: 'missing_final_answer',
    surfaceRole: 'result',
    lifecycle: 'sealed',
  };
}

/** 把实时或历史助手数据投影成固定的过程、答案、结果、控制四区。 */
export function projectAssistantTurn(input: ProjectAssistantTurnInput): ProjectAssistantTurnResult {
  const diagnostics: TurnProjectionDiagnostic[] = [];
  const segmentBlocks = input.segments.map((segment) => projectSegment(segment, input, diagnostics));
  const hasSemanticText = input.segments.some((segment) => segment.kind === 'text');
  const hasSemanticReasoning = input.segments.some((segment) => segment.kind === 'reasoning');
  const legacyBlocks = normalizeContentBlocks(
    input.legacyBlocks.filter((block) => !(
      (hasSemanticText && block.type === 'text')
      || (hasSemanticReasoning && block.type === 'thinking')
    )),
    {
      idPrefix: input.idPrefix,
      turnLifecycle: input.status === 'streaming' ? 'streaming' : 'sealed',
    },
  );
  const allBlocks = [...segmentBlocks, ...legacyBlocks];
  const processBlocks = allBlocks.filter((block) => block.surfaceRole === 'process');
  const answerBlocks = allBlocks.filter((block) => block.surfaceRole === 'answer');
  const resultBlocks = allBlocks.filter((block) => block.surfaceRole === 'result');
  const controlBlocks = allBlocks.filter((block) => block.surfaceRole === 'control');

  if (answerBlocks.length === 0 && resultBlocks.length === 0 && controlBlocks.length === 0) {
    const statusBlock = turnStatusBlock(input);
    if (statusBlock) resultBlocks.push(statusBlock);
  }

  const blocks = [...processBlocks, ...answerBlocks, ...resultBlocks, ...controlBlocks];
  const projection: AssistantTurnProjection = {
    id: `${input.idPrefix}:turn`,
    inputMessageId: input.inputMessageId || null,
    assistantMessageIds: [...input.assistantMessageIds],
    processBlockIds: processBlocks.map((block) => block.id!),
    answerBlockIds: answerBlocks.map((block) => block.id!),
    resultBlockIds: resultBlocks.map((block) => block.id!),
    controlBlockIds: controlBlocks.map((block) => block.id!),
    status: input.status,
    ...(input.startedAt !== undefined ? { startedAt: input.startedAt } : {}),
    ...(input.completedAt !== undefined ? { completedAt: input.completedAt } : {}),
  };

  return { blocks, projection, diagnostics };
}

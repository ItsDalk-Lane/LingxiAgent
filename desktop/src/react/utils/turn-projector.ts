import type {
  AssistantTurnProjection,
  AssistantTurnStatus,
  ContentBlock,
  ContentLifecycle,
} from '../stores/chat-types';
import type { LiveAssistantSegment } from '../stores/live-turn-store';
import { normalizeContentBlocks } from './content-semantics';

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
      ...(segment.deferred ? { deferred: segment.deferred } : {}),
      sealed: lifecycle === 'sealed',
      semanticPhase: 'reasoning',
      surfaceRole: 'process',
      lifecycle,
    };
  }

  // unresolved ≠ commentary：流式期身份未判明的文字是 provisional（临时区），
  // 既不混进过程折叠，也不冒称答案；只有回合终结时才按供应商回退定身份。
  if (segment.semanticPhase === 'unresolved' && input.status === 'streaming') {
    return {
      id: segmentBlockId(input.idPrefix, segment.id),
      type: 'text',
      source: segment.source,
      ...(segment.deferred ? { deferred: segment.deferred } : {}),
      semanticPhase: 'unresolved',
      surfaceRole: 'provisional',
      lifecycle,
    };
  }

  let semanticPhase: 'commentary' | 'final_answer';
  if (segment.semanticPhase === 'unresolved') {
    semanticPhase = 'final_answer';
    diagnostics.push({
      code: 'unresolved_phase_fallback',
      segmentId: segment.id,
      fallbackPhase: 'final_answer',
    });
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

function turnStatusBlock(
  input: ProjectAssistantTurnInput,
  hasToolCalls: boolean,
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
  // 任务书 §三十四：turnStatusBlock 只在 Assistant Run 真正 terminal（sealed）时被投影，
  // 此时 agent 循环已停，带工具但没有最终答复同样算 completed_without_user_output，
  // 不再豁免（旧逻辑把「有工具调用」当成「循环还会继续」是 Model Turn 层级才成立的假设）。
  void hasToolCalls;
  return {
    // id 由 Run 键派生：同一 Run 无论投影重算多少次都得到同一个 id，保证每 Run 至多一个
    id: `${input.idPrefix}:missing-final-answer`,
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
  const provisionalBlocks = allBlocks.filter((block) => block.surfaceRole === 'provisional');
  const answerBlocks = allBlocks.filter((block) => block.surfaceRole === 'answer');
  const resultBlocks = allBlocks.filter((block) => block.surfaceRole === 'result');
  const controlBlocks = allBlocks.filter((block) => block.surfaceRole === 'control');

  // provisional 是"还没判明身份"的临时文字，不算答案：终结态下它已被回退成
  // final_answer，这里的判空条件不会因为 provisional 而误免 missing_final_answer。
  const hasToolCalls = allBlocks.some((block) => (
    block.type === 'tool_group' && Array.isArray(block.tools) && block.tools.length > 0
  ));
  if (answerBlocks.length === 0 && resultBlocks.length === 0 && controlBlocks.length === 0) {
    const statusBlock = turnStatusBlock(input, hasToolCalls);
    if (statusBlock) resultBlocks.push(statusBlock);
  }

  const blocks = [...processBlocks, ...provisionalBlocks, ...answerBlocks, ...resultBlocks, ...controlBlocks];
  const projection: AssistantTurnProjection = {
    id: `${input.idPrefix}:turn`,
    inputMessageId: input.inputMessageId || null,
    assistantMessageIds: [...input.assistantMessageIds],
    processBlockIds: processBlocks.map((block) => block.id!),
    provisionalBlockIds: provisionalBlocks.map((block) => block.id!),
    answerBlockIds: answerBlocks.map((block) => block.id!),
    resultBlockIds: resultBlocks.map((block) => block.id!),
    controlBlockIds: controlBlocks.map((block) => block.id!),
    status: input.status,
    ...(input.startedAt !== undefined ? { startedAt: input.startedAt } : {}),
    ...(input.completedAt !== undefined ? { completedAt: input.completedAt } : {}),
  };

  return { blocks, projection, diagnostics };
}

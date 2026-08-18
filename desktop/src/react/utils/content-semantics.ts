import type {
  AssistantSemanticPhase,
  ContentBlock,
  ContentLifecycle,
  ContentSurfaceRole,
} from '../stores/chat-types';

export interface NormalizeContentBlocksOptions {
  idPrefix: string;
  turnLifecycle: ContentLifecycle;
  defaultTextPhase?: Extract<AssistantSemanticPhase, 'commentary' | 'final_answer'>;
}

function resolvedTextPhase(
  block: ContentBlock,
  fallback: Extract<AssistantSemanticPhase, 'commentary' | 'final_answer'>,
): Extract<AssistantSemanticPhase, 'commentary' | 'final_answer'> {
  if (block.semanticPhase === 'commentary' || block.semanticPhase === 'final_answer') {
    return block.semanticPhase;
  }
  return fallback;
}

export function resolveContentSemanticPhase(
  block: ContentBlock,
  defaultTextPhase: Extract<AssistantSemanticPhase, 'commentary' | 'final_answer'> = 'final_answer',
): AssistantSemanticPhase | undefined {
  if (block.semanticPhase) return block.semanticPhase;
  switch (block.type) {
    case 'thinking': return 'reasoning';
    case 'mood': return 'mood';
    case 'tool_group': return 'tool';
    case 'text': return resolvedTextPhase(block, defaultTextPhase);
    default: return undefined;
  }
}

function resolvedControlSurface(block: ContentBlock): ContentSurfaceRole {
  switch (block.type) {
    case 'session_confirmation':
      return block.status === 'pending' ? 'control' : 'result';
    case 'settings_confirm':
      return block.status === 'pending' ? 'control' : 'result';
    case 'cron_confirm':
      return block.status === 'pending' ? 'control' : 'result';
    case 'suggestion_card':
      return block.status === 'pending' ? 'control' : 'result';
    default:
      return 'control';
  }
}

export function resolveContentSurface(
  block: ContentBlock,
  defaultTextPhase: Extract<AssistantSemanticPhase, 'commentary' | 'final_answer'> = 'final_answer',
): ContentSurfaceRole {
  if (block.surfaceRole) return block.surfaceRole;
  switch (block.type) {
    case 'thinking':
    case 'mood':
    case 'tool_group':
    case 'subagent':
    case 'workflow':
    case 'media_generation':
      return 'process';
    case 'text':
      return resolvedTextPhase(block, defaultTextPhase) === 'commentary' ? 'process' : 'answer';
    case 'session_confirmation':
    case 'settings_confirm':
    case 'cron_confirm':
    case 'suggestion_card':
      return resolvedControlSurface(block);
    case 'interactive_card':
      return 'control';
    default:
      return 'result';
  }
}

export function resolveContentLifecycle(
  block: ContentBlock,
  turnLifecycle: ContentLifecycle,
): ContentLifecycle {
  switch (block.type) {
    case 'thinking':
      return block.sealed ? 'sealed' : 'streaming';
    case 'tool_group':
      return Array.isArray(block.tools) && block.tools.every((tool) => tool.done)
        ? 'sealed'
        : 'streaming';
    case 'media_generation':
      return block.status === 'pending' ? 'streaming' : 'sealed';
    case 'subagent':
    case 'workflow':
      return block.streamStatus === 'running' ? 'streaming' : 'sealed';
    case 'text':
    case 'mood':
      return turnLifecycle;
    default:
      return 'sealed';
  }
}

function intrinsicBlockId(block: ContentBlock): string | null {
  switch (block.type) {
    case 'tool_group': {
      const ids = (Array.isArray(block.tools) ? block.tools : [])
        .map((tool) => tool.id?.trim())
        .filter((id): id is string => !!id);
      return ids.length > 0 ? `tools:${ids.join('+')}` : null;
    }
    case 'file': return block.fileId || block.resource?.resourceId || null;
    case 'media_generation': return block.taskId;
    case 'artifact': return block.artifactId;
    case 'skill': return block.fileId || block.skillName;
    case 'cron_confirm': return block.confirmId || null;
    case 'suggestion_card': return block.confirmId || block.suggestionId || block.suggestionShortCode || null;
    case 'settings_confirm': return block.confirmId || block.settingKey;
    case 'session_confirmation': return block.confirmId;
    case 'interlude': return block.id;
    case 'subagent': return block.taskId;
    case 'workflow': return block.taskId;
    case 'plugin_card': {
      const target = block.card.route || block.card.sessionId || block.card.sessionRef?.sessionId || null;
      return target ? `${block.card.pluginId}:${target}` : block.card.pluginId || null;
    }
    case 'interactive_card': return block.cardId;
    default: return null;
  }
}

function blockId(
  block: ContentBlock,
  idPrefix: string,
  ordinal: number,
): string {
  if (block.id?.trim()) return block.id;
  const intrinsic = intrinsicBlockId(block);
  return intrinsic
    ? `${idPrefix}:${block.type}:${intrinsic}`
    : `${idPrefix}:${block.type}:${ordinal}`;
}

/** 为实时和历史新内容补齐统一语义；旧字段原样保留。 */
export function normalizeContentBlocks(
  blocks: readonly ContentBlock[],
  options: NormalizeContentBlocksOptions,
): ContentBlock[] {
  const ordinals = new Map<ContentBlock['type'], number>();
  const defaultTextPhase = options.defaultTextPhase || 'final_answer';
  return blocks.map((block) => {
    const ordinal = ordinals.get(block.type) || 0;
    ordinals.set(block.type, ordinal + 1);
    const semanticPhase = resolveContentSemanticPhase(block, defaultTextPhase);
    return {
      ...block,
      id: blockId(block, options.idPrefix, ordinal),
      lifecycle: resolveContentLifecycle(block, options.turnLifecycle),
      surfaceRole: resolveContentSurface(block, defaultTextPhase),
      ...(semanticPhase ? { semanticPhase } : {}),
    } as ContentBlock;
  });
}

/** 回合落盘后，仅把本地临时前缀替换为持久条目前缀；上游显式标识不改。 */
export function rebaseGeneratedContentBlockIds(
  blocks: readonly ContentBlock[],
  previousPrefix: string,
  nextPrefix: string,
): ContentBlock[] {
  if (!previousPrefix || !nextPrefix || previousPrefix === nextPrefix) return [...blocks];
  const generatedPrefix = `${previousPrefix}:`;
  return blocks.map((block) => (
    block.id?.startsWith(generatedPrefix)
      ? { ...block, id: `${nextPrefix}:${block.id.slice(generatedPrefix.length)}` } as ContentBlock
      : block
  ));
}

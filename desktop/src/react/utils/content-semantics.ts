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

type FileBlock = Extract<ContentBlock, { type: 'file' }>;

function filePresentationKey(block: FileBlock): string | null {
  if (block.fileId) return `file:${block.fileId}`;
  if (block.resource?.resourceId) return `resource:${block.resource.resourceId}`;
  return block.filePath ? `path:${block.filePath}` : null;
}

export function isSameFilePresentation(left: FileBlock, right: FileBlock): boolean {
  const key = filePresentationKey(left);
  if (!key || key !== filePresentationKey(right)) return false;
  // 文件身份相同但已明确变更内容时，不能把新版展示当成重复通知。
  if (left.version?.sha256 && right.version?.sha256) {
    return left.version.sha256 === right.version.sha256;
  }
  const leftSize = left.version?.size ?? left.size;
  const rightSize = right.version?.size ?? right.size;
  const leftTime = left.version?.mtimeMs ?? left.mtimeMs;
  const rightTime = right.version?.mtimeMs ?? right.mtimeMs;
  return !(leftSize != null && rightSize != null && leftSize !== rightSize)
    && !(leftTime != null && rightTime != null && leftTime !== rightTime);
}

/**
 * 输入限于同一助手回合。自动媒体交付与手动展示是同一文件的两份呈现证据，
 * 合并时保留首次展示的位置和身份；普通文件的重复交付不受影响。
 */
function coalesceMediaFilePresentations(blocks: readonly ContentBlock[]): ContentBlock[] {
  const automatic = new Map<string, FileBlock[] | null>();
  for (const block of blocks) {
    if (block.type !== 'file' || !block.replacesTaskId) continue;
    const key = filePresentationKey(block);
    if (!key) continue;
    const previous = automatic.get(key);
    // 多个独立任务指向同一文件时，不猜测它们是否应合并。
    if (previous === null || (previous && previous[0].replacesTaskId !== block.replacesTaskId)) {
      automatic.set(key, null);
    } else if (!previous) {
      automatic.set(key, [block]);
    } else if (!previous.some(file => isSameFilePresentation(file, block))) {
      previous.push(block);
    }
  }
  const presented = new Set<FileBlock>();
  return blocks.flatMap<ContentBlock>((block) => {
    if (block.type !== 'file') return [block];
    const key = filePresentationKey(block);
    const completion = key ? automatic.get(key)?.find(file => isSameFilePresentation(file, block)) : null;
    if (!key || !completion) return [block];
    if (presented.has(completion)) return [];
    presented.add(completion);
    return [{
      ...block,
      ...completion,
      ...(block.id ? { id: block.id } : {}),
      ...(block.processOrder !== undefined ? { processOrder: block.processOrder } : {}),
    }];
  });
}

/** 为实时和历史的单个助手回合补齐统一语义。 */
export function normalizeContentBlocks(
  blocks: readonly ContentBlock[],
  options: NormalizeContentBlocksOptions,
): ContentBlock[] {
  const ordinals = new Map<ContentBlock['type'], number>();
  const defaultTextPhase = options.defaultTextPhase || 'final_answer';
  return coalesceMediaFilePresentations(blocks).map((block) => {
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

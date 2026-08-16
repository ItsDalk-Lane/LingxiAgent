import { getAssistantTextPhase } from './text-signature.ts';

export interface PersistedAssistantSemanticSegment {
  id: string;
  kind: 'text' | 'reasoning';
  semanticPhase: 'reasoning' | 'commentary' | 'final_answer';
  source: string;
  lifecycle: 'sealed';
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

/** 从持久化助手内容恢复服务端流式协议对应的确定性语义分段。 */
export function extractPersistedAssistantSemanticSegments(
  content: unknown,
  messageOrdinal = 1,
): PersistedAssistantSemanticSegment[] {
  if (typeof content === 'string') {
    return content
      ? [{
          id: `assistant:${messageOrdinal}:text:default`,
          kind: 'text',
          semanticPhase: 'final_answer',
          source: content,
          lifecycle: 'sealed',
        }]
      : [];
  }
  if (!Array.isArray(content)) return [];

  const segments: PersistedAssistantSemanticSegment[] = [];
  const reasoning = content
    .map(objectValue)
    .filter((block) => block?.type === 'thinking')
    .map((block) => typeof block?.thinking === 'string' ? block.thinking : '')
    .join('\n');
  if (content.some((block) => objectValue(block)?.type === 'thinking')) {
    segments.push({
      id: `assistant:${messageOrdinal}:reasoning:default`,
      kind: 'reasoning',
      semanticPhase: 'reasoning',
      source: reasoning,
      lifecycle: 'sealed',
    });
  }

  for (let index = 0; index < content.length; index += 1) {
    const block = objectValue(content[index]);
    if (block?.type !== 'text' || typeof block.text !== 'string' || !block.text) continue;
    segments.push({
      id: `assistant:${messageOrdinal}:text:${index}`,
      kind: 'text',
      semanticPhase: getAssistantTextPhase(block) || 'final_answer',
      source: block.text,
      lifecycle: 'sealed',
    });
  }
  return segments;
}

import { getAssistantTextPhase } from './text-signature.ts';

export interface PersistedAssistantSemanticSegment {
  id: string;
  kind: 'text' | 'reasoning';
  semanticPhase: 'reasoning' | 'commentary' | 'final_answer';
  source: string;
  lifecycle: 'sealed';
  /** 该段在原始 content 数组中的位置索引：与 toolCalls 的位置索引同坐标系，
   *  供前端把思考段与工具块按真实时间线交错。 */
  processOrder?: number;
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
  const objectBlocks = content.map(objectValue);
  const firstThinkingIndex = objectBlocks.findIndex((block) => block?.type === 'thinking');
  if (firstThinkingIndex >= 0) {
    const reasoning = objectBlocks
      .filter((block) => block?.type === 'thinking')
      .map((block) => typeof block?.thinking === 'string' ? block.thinking : '')
      .join('\n');
    segments.push({
      id: `assistant:${messageOrdinal}:reasoning:default`,
      kind: 'reasoning',
      semanticPhase: 'reasoning',
      source: reasoning,
      lifecycle: 'sealed',
      processOrder: firstThinkingIndex,
    });
  }

  for (let index = 0; index < content.length; index += 1) {
    const block = objectBlocks[index];
    if (block?.type !== 'text' || typeof block.text !== 'string' || !block.text) continue;
    segments.push({
      id: `assistant:${messageOrdinal}:text:${index}`,
      kind: 'text',
      semanticPhase: getAssistantTextPhase(block) || 'final_answer',
      source: block.text,
      lifecycle: 'sealed',
      processOrder: index,
    });
  }
  return segments;
}

import type { ContentBlock, ToolCall } from '../stores/chat-types';
import { parseCardFromContent, parseMoodFromContent } from './message-parser';
import { skillInvocationName } from '../../../../shared/tool-outcome.ts';

interface AssistantBlockInput {
  content: string;
  thinking?: string | null;
  toolCalls?: Array<{
    id?: string;
    toolCallId?: string;
    name: string;
    args?: Record<string, unknown>;
    status?: 'succeeded' | 'failed' | 'unknown';
    success?: boolean;
    error?: string;
    details?: Record<string, unknown>;
  }> | null;
  extraBlocks?: ContentBlock[] | null;
}

export function buildAssistantBlocksFromContent({
  content,
  thinking = null,
  toolCalls = null,
  extraBlocks = null,
}: AssistantBlockInput): ContentBlock[] {
  const blocks: ContentBlock[] = [];
  const skillToolCalls = toolCalls?.filter((toolCall) => !!skillInvocationName({
    toolName: toolCall.name,
    args: toolCall.args,
  })) || [];
  const standardToolCalls = toolCalls?.filter((toolCall) => !skillInvocationName({
    toolName: toolCall.name,
    args: toolCall.args,
  })) || [];

  const pushToolGroup = (calls: NonNullable<AssistantBlockInput['toolCalls']>) => {
    if (!calls.length) return;
    blocks.push({
      type: 'tool_group',
      tools: calls.map<ToolCall>((tc) => ({
        id: tc.id || tc.toolCallId || undefined,
        name: tc.name,
        args: tc.args,
        done: true,
        success: tc.status === 'succeeded' || (tc.status === undefined && tc.success !== false),
        status: tc.status || (tc.success === false ? 'failed' : 'succeeded'),
        ...(tc.error ? { error: tc.error } : {}),
        ...(tc.details ? { details: tc.details } : {}),
      })),
      collapsed: calls.length > 1,
    });
  };

  if (thinking !== null && thinking !== undefined) {
    blocks.push({ type: 'thinking', content: thinking, sealed: true });
  }

  const { mood, yuan, text: afterMood } = parseMoodFromContent(content || '');
  if (mood && yuan) {
    blocks.push({ type: 'mood', yuan, text: mood });
  }

  pushToolGroup(standardToolCalls);

  const { cards, text: mainText } = parseCardFromContent(afterMood);
  if (mainText) {
    blocks.push({
      type: 'text',
      source: mainText,
    });
  }

  for (const card of cards) {
    blocks.push({ type: 'plugin_card', card });
  }

  // 模型通过 read 打开 SKILL.md 才算技能调用。历史消息里这类工具调用位于
  // 同一段可见正文之后，不能沿用旧工具组“统一置顶”的展示顺序。
  pushToolGroup(skillToolCalls);

  if (extraBlocks?.length) {
    blocks.push(...extraBlocks);
  }

  return blocks;
}

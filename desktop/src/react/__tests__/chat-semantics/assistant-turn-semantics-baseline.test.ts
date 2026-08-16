import { describe, expect, it } from 'vitest';
import type { ChatListItem, ContentBlock, ToolCall } from '../../stores/chat-types';
import { buildTranscriptRenderItems } from '../../components/chat/process-fold';

function tool(name: string, id: string): ToolCall {
  return { id, name, done: true, success: true, status: 'succeeded' };
}

function assistant(id: string, blocks: ContentBlock[]): ChatListItem {
  return { type: 'message', data: { id, role: 'assistant', blocks, turnInputEntryId: 'turn-input-1' } };
}

describe('assistant turn semantic baseline', () => {
  it('固化长过程旁白和短最终答复的目标语义，并记录旧投影的差距', () => {
    const commentary = '现在还需要考虑一个问题。我重新权衡并继续核对。'.repeat(150);
    const finalAnswer = '已经完成修改，测试通过。';
    const items: ChatListItem[] = [
      { type: 'message', data: { id: 'user-1', role: 'user', text: '请修改这个界面', sourceEntryId: 'turn-input-1' } },
      assistant('assistant-commentary', [{ type: 'text', html: `<p>${commentary}</p>`, source: commentary }]),
      assistant('assistant-tools', [{
        type: 'tool_group',
        tools: [tool('read', 'read-1'), tool('exec_command', 'exec-1')],
        collapsed: true,
      }]),
      assistant('assistant-final', [{ type: 'text', html: `<p>${finalAnswer}</p>`, source: finalAnswer }]),
    ];

    const target = {
      processMessageIds: ['assistant-commentary', 'assistant-tools'],
      answerMessageIds: ['assistant-final'],
      visibleAnswer: finalAnswer,
      hiddenByDefault: commentary,
    };
    const legacy = buildTranscriptRenderItems(items, { isStreaming: false });
    const legacyVisibleSourceIds = legacy
      .filter((item) => item.type === 'source' && item.item.type === 'message')
      .map((item) => item.type === 'source' && item.item.type === 'message' ? item.item.data.id : '');

    expect(target.processMessageIds).toEqual(['assistant-commentary', 'assistant-tools']);
    expect(target.answerMessageIds).toEqual(['assistant-final']);
    expect(target.visibleAnswer.length).toBeLessThan(target.hiddenByDefault.length);
    expect(legacyVisibleSourceIds).toEqual([
      'user-1',
      'assistant-commentary',
      'assistant-tools',
      'assistant-final',
    ]);
  });
});

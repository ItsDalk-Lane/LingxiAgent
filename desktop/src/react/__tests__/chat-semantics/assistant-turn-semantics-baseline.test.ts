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
  it('长过程旁白进入过程折叠，短最终答复保持外显', () => {
    const commentary = '现在还需要考虑一个问题。我重新权衡并继续核对。'.repeat(150);
    const finalAnswer = '已经完成修改，测试通过。';
    const items: ChatListItem[] = [
      { type: 'message', data: { id: 'user-1', role: 'user', text: '请修改这个界面', sourceEntryId: 'turn-input-1' } },
      assistant('assistant-commentary', [{
        id: 'assistant-commentary:text:0',
        type: 'text',
        html: `<p>${commentary}</p>`,
        source: commentary,
        semanticPhase: 'commentary',
        surfaceRole: 'process',
        lifecycle: 'sealed',
      }]),
      assistant('assistant-tools', [{
        type: 'tool_group',
        tools: [tool('read', 'read-1'), tool('exec_command', 'exec-1')],
        collapsed: true,
      }]),
      assistant('assistant-final', [{
        id: 'assistant-final:text:0',
        type: 'text',
        html: `<p>${finalAnswer}</p>`,
        source: finalAnswer,
        semanticPhase: 'final_answer',
        surfaceRole: 'answer',
        lifecycle: 'sealed',
      }]),
    ];

    const target = {
      processMessageIds: ['assistant-commentary', 'assistant-tools'],
      answerMessageIds: ['assistant-final'],
      visibleAnswer: finalAnswer,
      hiddenByDefault: commentary,
    };
    const rendered = buildTranscriptRenderItems(items, { isStreaming: false });
    const visibleSourceIds = rendered
      .filter((item) => item.type === 'source' && item.item.type === 'message')
      .map((item) => item.type === 'source' && item.item.type === 'message' ? item.item.data.id : '');
    const processFold = rendered.find((item) => item.type === 'process_fold');

    expect(target.processMessageIds).toEqual(['assistant-commentary', 'assistant-tools']);
    expect(target.answerMessageIds).toEqual(['assistant-final']);
    expect(target.visibleAnswer.length).toBeLessThan(target.hiddenByDefault.length);
    expect(visibleSourceIds).toEqual(['user-1', 'assistant-final']);
    expect(processFold).toMatchObject({
      type: 'process_fold',
      defaultCollapsed: true,
      refs: [
        { sourceMessageId: 'assistant-commentary' },
        { sourceMessageId: 'assistant-tools' },
      ],
    });
  });
});

import { describe, it, expect } from 'vitest';
import { projectAssistantTurn } from '../turn-projector';
import { collectTurnEditedFiles } from '../../components/chat/TurnEditedFilesCard';

/**
 * 复现实验：回合转正（commitLiveRun → projectAssistantTurn）后，
 * 卡片的数据收集是否还能拿到 tool.details.fileChange。
 * 数据形态对齐 2026-09-20 19:11 真实会话（write《秋信》）的 toolResult.details。
 */
describe('转正投影后卡片数据收集', () => {
  const liveToolGroup = {
    type: 'tool_group' as const,
    tools: [{
      id: 'call_1',
      name: 'write',
      args: { path: '/Users/x/sample.md', content: '# 秋信\n' },
      done: true,
      success: true,
      status: 'succeeded' as const,
      details: {
        fileChange: {
          path: '/Users/x/sample.md',
          added: 3,
          removed: 0,
          beforeAvailable: false,
          changeType: 'created',
        },
      },
      processOrder: 1,
    }],
    collapsed: false,
    processOrder: 1,
  };

  it('投影后 collectTurnEditedFiles 仍应收集到 1 个文件', () => {
    const projected = projectAssistantTurn({
      idPrefix: 'msg1',
      inputMessageId: null,
      assistantMessageIds: ['msg1'],
      segments: [],
      legacyBlocks: [liveToolGroup],
      status: 'completed',
    });
    const toolGroups = projected.blocks.filter(b => b.type === 'tool_group');
    console.log('tool_group count:', toolGroups.length);
    console.log('tool details:', JSON.stringify((toolGroups[0] as any)?.tools?.[0]?.details));
    const collected = collectTurnEditedFiles(projected.blocks);
    console.log('collected:', JSON.stringify(collected));
    expect(collected.length).toBe(1);
    expect(collected[0].path).toBe('/Users/x/sample.md');
  });
});

import { describe, expect, it } from 'vitest';
import type { ContentBlock } from '../../stores/chat-types';
import {
  normalizeContentBlocks,
  resolveContentLifecycle,
  resolveContentSemanticPhase,
  resolveContentSurface,
} from '../../utils/content-semantics';

describe('content semantics', () => {
  it('只按显式阶段区分长过程旁白和短最终答复', () => {
    const commentary: ContentBlock = {
      type: 'text',
      html: '<p>过程</p>',
      source: '过程'.repeat(2_000),
      semanticPhase: 'commentary',
    };
    const answer: ContentBlock = {
      type: 'text',
      html: '<p>完成。</p>',
      source: '完成。',
      semanticPhase: 'final_answer',
    };

    expect(resolveContentSurface(commentary)).toBe('process');
    expect(resolveContentSurface(answer)).toBe('answer');
    expect(resolveContentSemanticPhase(commentary)).toBe('commentary');
    expect(resolveContentSemanticPhase(answer)).toBe('final_answer');
  });

  it('把过程、结果和待操作卡映射到不同界面角色', () => {
    const blocks: ContentBlock[] = [
      { type: 'thinking', content: '思考', sealed: false },
      { type: 'mood', yuan: 'lingxi', text: '专注' },
      { type: 'tool_group', tools: [{ id: 'tool-1', name: 'read', done: false, success: false }], collapsed: false },
      { type: 'file', fileId: 'file-1', filePath: '/tmp/a.md', label: 'a.md', ext: 'md' },
      { type: 'session_confirmation', confirmId: 'confirm-1', kind: 'approval', surface: 'message', status: 'pending', title: '需要确认' },
    ];

    expect(blocks.map((block) => resolveContentSurface(block))).toEqual([
      'process',
      'process',
      'process',
      'result',
      'control',
    ]);
    expect(blocks.map((block) => resolveContentLifecycle(block, 'streaming'))).toEqual([
      'streaming',
      'streaming',
      'streaming',
      'sealed',
      'sealed',
    ]);
  });

  it('为同一顺序的实时和历史块生成相同稳定标识', () => {
    const blocks: ContentBlock[] = [
      { type: 'thinking', content: '思考', sealed: true },
      { type: 'text', html: '<p>第一段</p>', source: '第一段' },
      { type: 'tool_group', tools: [{ id: 'call-1', name: 'read', done: true, success: true }], collapsed: false },
      { type: 'text', html: '<p>第二段</p>', source: '第二段' },
      { type: 'file', fileId: 'file-1', filePath: '/tmp/a.md', label: 'a.md', ext: 'md' },
    ];

    const live = normalizeContentBlocks(blocks, { idPrefix: 'assistant-entry-1', turnLifecycle: 'sealed' });
    const history = normalizeContentBlocks(blocks.map((block) => ({ ...block })), {
      idPrefix: 'assistant-entry-1',
      turnLifecycle: 'sealed',
    });

    expect(live.map((block) => block.id)).toEqual(history.map((block) => block.id));
    expect(live.map((block) => block.id)).toEqual([
      'assistant-entry-1:thinking:0',
      'assistant-entry-1:text:0',
      'assistant-entry-1:tool_group:tools:call-1',
      'assistant-entry-1:text:1',
      'assistant-entry-1:file:file-1',
    ]);
    expect(live.map((block) => block.lifecycle)).toEqual([
      'sealed',
      'sealed',
      'sealed',
      'sealed',
      'sealed',
    ]);
  });

  it('兼容已经带稳定标识和显式角色的旧投影结果', () => {
    const [normalized] = normalizeContentBlocks([{
      id: 'provider-segment-1',
      type: 'text',
      html: '<p>保留</p>',
      source: '保留',
      semanticPhase: 'commentary',
      surfaceRole: 'answer',
      lifecycle: 'sealed',
    }], { idPrefix: 'assistant-entry-1', turnLifecycle: 'streaming' });

    expect(normalized).toMatchObject({
      id: 'provider-segment-1',
      semanticPhase: 'commentary',
      surfaceRole: 'answer',
      lifecycle: 'streaming',
    });
  });
});

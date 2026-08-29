import { describe, expect, it } from 'vitest';
import type { KnowledgeRetrievalStats } from '../../../../../shared/knowledge-refs.ts';
import type { ChatListItem, ChatMessage, ContentBlock, ToolCall } from '../../stores/chat-types';
import {
  buildProcessFoldSummary,
  buildTranscriptRenderItems,
  isProcessOnlyAssistantMessage,
} from '../../components/chat/process-fold';

function user(id: string, text = '请处理'): ChatListItem {
  return { type: 'message', data: { id, role: 'user', text } };
}

function assistant(id: string, blocks: ContentBlock[], turnInputEntryId?: string): ChatListItem {
  return {
    type: 'message',
    data: {
      id,
      role: 'assistant',
      blocks,
      ...(turnInputEntryId ? { turnInputEntryId } : {}),
    },
  };
}

function thinking(content = '想了一下'): ContentBlock {
  return { type: 'thinking', content, sealed: true };
}

function tool(name: string, success = true): ToolCall {
  return { name, args: { command: name }, done: true, success };
}

function toolGroup(tools: ToolCall[]): ContentBlock {
  return { type: 'tool_group', tools, collapsed: tools.length > 1 };
}

function textBlock(html = '<p>完成</p>', source?: string): ContentBlock {
  return { type: 'text', html, ...(source ? { source } : {}) };
}

describe('process fold grouping', () => {
  it('按显式四区把单条回合里的长过程折叠，并让短最终答复保持外显', () => {
    const longCommentary = '持续检查过程。'.repeat(300);
    const processText: ContentBlock = {
      id: 'a1:commentary',
      type: 'text',
      html: `<p>${longCommentary}</p>`,
      source: longCommentary,
      semanticPhase: 'commentary',
      surfaceRole: 'process',
      lifecycle: 'sealed',
    };
    const execBlock: ContentBlock = {
      id: 'a1:exec',
      type: 'tool_group',
      tools: [{
        id: 'call-exec',
        name: 'exec_command',
        args: { cmd: 'npm test' },
        done: true,
        success: true,
      }],
      collapsed: false,
      semanticPhase: 'tool',
      surfaceRole: 'process',
      lifecycle: 'sealed',
    };
    const answer: ContentBlock = {
      id: 'a1:answer',
      type: 'text',
      html: '<p>测试通过。</p>',
      source: '测试通过。',
      semanticPhase: 'final_answer',
      surfaceRole: 'answer',
      lifecycle: 'sealed',
    };
    const turn = assistant('a1', [processText, execBlock, answer]);
    if (turn.type !== 'message') throw new Error('expected assistant');
    turn.data.turnProjection = {
      id: 'a1:turn',
      inputMessageId: 'u1',
      assistantMessageIds: ['a1'],
      processBlockIds: ['a1:commentary', 'a1:exec'],
      answerBlockIds: ['a1:answer'],
      resultBlockIds: [],
      controlBlockIds: [],
      status: 'completed',
    };

    const rendered = buildTranscriptRenderItems([user('u1'), turn], { isStreaming: false });

    expect(rendered.map((item) => item.type)).toEqual(['source', 'process_fold', 'source']);
    expect(rendered[1]).toMatchObject({
      type: 'process_fold',
      id: 'a1:process',
      turnId: 'a1:turn',
      blockIds: ['a1:commentary', 'a1:exec'],
      status: 'completed',
      defaultCollapsed: true,
      refs: [{ sourceMessageId: 'a1', blocks: [processText, execBlock] }],
    });
    expect(rendered[2]).toMatchObject({
      type: 'source',
      item: { data: { blocks: [answer] } },
    });
  });

  it('单个过程块也形成稳定过程组，不再依赖消息数量', () => {
    const processBlock: ContentBlock = {
      id: 'a1:thinking',
      type: 'thinking',
      content: '只思考了一次',
      sealed: true,
      semanticPhase: 'reasoning',
      surfaceRole: 'process',
      lifecycle: 'sealed',
    };
    const turn = assistant('a1', [processBlock]);
    if (turn.type !== 'message') throw new Error('expected assistant');
    turn.data.turnProjection = {
      id: 'a1:turn',
      inputMessageId: 'u1',
      assistantMessageIds: ['a1'],
      processBlockIds: ['a1:thinking'],
      answerBlockIds: [],
      resultBlockIds: [],
      controlBlockIds: [],
      status: 'completed',
    };

    expect(buildTranscriptRenderItems([user('u1'), turn], { isStreaming: false })).toMatchObject([
      { type: 'source' },
      {
        type: 'process_fold',
        id: 'a1:process',
        blockIds: ['a1:thinking'],
        ownsTurnCompletion: true,
      },
    ]);
  });

  it('结果和待操作卡始终留在过程组外', () => {
    const processBlock: ContentBlock = {
      id: 'a1:thinking',
      type: 'thinking',
      content: '准备结果',
      sealed: true,
      surfaceRole: 'process',
    };
    const resultBlock: ContentBlock = {
      id: 'a1:file',
      type: 'file',
      filePath: '/workspace/result.md',
      label: 'result.md',
      ext: 'md',
      surfaceRole: 'result',
    };
    const controlBlock: ContentBlock = {
      id: 'a1:confirm',
      type: 'session_confirmation',
      confirmId: 'confirm-1',
      kind: 'approval',
      surface: 'message',
      status: 'pending',
      title: '是否继续？',
      surfaceRole: 'control',
    };
    const turn = assistant('a1', [processBlock, resultBlock, controlBlock]);
    if (turn.type !== 'message') throw new Error('expected assistant');
    turn.data.turnProjection = {
      id: 'a1:turn',
      inputMessageId: 'u1',
      assistantMessageIds: ['a1'],
      processBlockIds: ['a1:thinking'],
      answerBlockIds: [],
      resultBlockIds: ['a1:file'],
      controlBlockIds: ['a1:confirm'],
      status: 'completed',
    };

    const rendered = buildTranscriptRenderItems([user('u1'), turn], { isStreaming: false });

    expect(rendered).toMatchObject([
      { type: 'source' },
      { type: 'process_fold', refs: [{ sourceMessageId: 'a1', blocks: [processBlock] }] },
      { type: 'source', item: { data: { blocks: [resultBlock, controlBlock] } } },
    ]);
  });

  it.each(['failed', 'aborted'] as const)('%s 回合的过程组默认展开并保留失败状态', (status) => {
    const processBlock: ContentBlock = {
      id: `a1:${status}:thinking`,
      type: 'thinking',
      content: '执行过程',
      sealed: true,
      surfaceRole: 'process',
    };
    const statusBlock: ContentBlock = {
      id: `a1:${status}:status`,
      type: 'turn_status',
      status,
      surfaceRole: 'result',
    };
    const turn = assistant('a1', [processBlock, statusBlock]);
    if (turn.type !== 'message') throw new Error('expected assistant');
    turn.data.turnProjection = {
      id: `a1:${status}:turn`,
      inputMessageId: 'u1',
      assistantMessageIds: ['a1'],
      processBlockIds: [processBlock.id!],
      answerBlockIds: [],
      resultBlockIds: [statusBlock.id!],
      controlBlockIds: [],
      status,
    };

    expect(buildTranscriptRenderItems([user('u1'), turn], { isStreaming: false })[1]).toMatchObject({
      type: 'process_fold',
      status,
      defaultCollapsed: false,
    });
  });

  it('folds consecutive process-only assistant messages into one render item', () => {
    const items: ChatListItem[] = [
      user('u1'),
      assistant('a1', [thinking(), toolGroup([tool('bash')])]),
      assistant('a2', [thinking(), toolGroup([tool('read'), tool('write')])]),
      assistant('a3', [thinking(), toolGroup([tool('grep')])]),
      assistant('a4', [textBlock('<p>正文</p>')]),
    ];

    const rendered = buildTranscriptRenderItems(items, { isStreaming: false });

    expect(rendered).toHaveLength(3);
    expect(rendered[1]).toMatchObject({
      type: 'process_fold',
      id: 'a1:process',
      stats: {
        toolCount: 4,
        thinkingCount: 3,
        unsuccessfulCount: 0,
      },
    });
    expect(rendered[2]).toMatchObject({ type: 'source', item: items[4] });
  });

  it('treats mood as process semantics instead of a fold exclusion', () => {
    const moodMessage: ChatMessage = {
      id: 'mood',
      role: 'assistant',
      blocks: [thinking(), { type: 'mood', yuan: 'butter', text: 'PULSE' }],
    };

    expect(isProcessOnlyAssistantMessage(moodMessage)).toBe(true);
  });

  it('allows a model skill invocation to join the process fold', () => {
    const skillMessage: ChatMessage = {
      id: 'skill',
      role: 'assistant',
      blocks: [thinking(), toolGroup([{
        name: 'read',
        args: { path: '/skills/leader/SKILL.md' },
        done: true,
        success: true,
      }])],
    };

    expect(isProcessOnlyAssistantMessage(skillMessage)).toBe(true);
    const items: ChatListItem[] = [
      user('u1'),
      assistant('a1', [thinking(), toolGroup([tool('read')])]),
      { type: 'message', data: skillMessage },
      assistant('a3', [thinking(), toolGroup([tool('grep')])]),
    ];
    expect(buildTranscriptRenderItems(items, { isStreaming: false })).toMatchObject([
      { type: 'source' },
      { type: 'process_fold', refs: [{}, {}, {}] },
    ]);
  });

  it('allows exec_command to join the process fold with stable navigation anchors', () => {
    const execMessage: ChatMessage = {
      id: 'exec',
      role: 'assistant',
      blocks: [toolGroup([{ name: 'exec_command', args: { cmd: 'npm run dev' }, done: true, success: true }])],
    };

    expect(isProcessOnlyAssistantMessage(execMessage)).toBe(true);
    const items: ChatListItem[] = [
      user('u1'),
      assistant('a1', [thinking(), toolGroup([tool('read')])]),
      { type: 'message', data: execMessage },
      assistant('a3', [thinking(), toolGroup([tool('grep')])]),
    ];
    expect(buildTranscriptRenderItems(items, { isStreaming: false })).toMatchObject([
      { type: 'source' },
      { type: 'process_fold', refs: [{}, {}, {}] },
    ]);
  });

  it('folds exec_command while keeping its legacy narration outside as answer text', () => {
    const items: ChatListItem[] = [
      user('u1'),
      assistant('a1', [thinking(), toolGroup([tool('read')])]),
      assistant('a2', [thinking(), toolGroup([tool('write')])]),
      assistant('a2b', [thinking(), toolGroup([tool('grep')])]),
      assistant('a3', [
        thinking(),
        textBlock('<p>起个后台服务。</p>', '起个后台服务。'),
        toolGroup([{ name: 'exec_command', args: { cmd: 'npm run dev' }, done: true, success: true }]),
      ]),
      assistant('a4', [textBlock('<p>服务已启动。</p>', '服务已启动。')]),
    ];

    const rendered = buildTranscriptRenderItems(items, { isStreaming: false });

    expect(rendered.map((item) => item.type)).toEqual([
      'source',
      'process_fold',
      'source',
      'source',
    ]);
    expect(rendered[1]).toMatchObject({ id: 'a1:process' });
    expect(rendered[2]).toMatchObject({ type: 'source', item: { data: { id: 'a3' } } });
  });

  it('does not throw or fold malformed tool_group blocks', () => {
    const items: ChatListItem[] = [
      user('u1'),
      assistant('a1', [{ type: 'tool_group', collapsed: false } as unknown as ContentBlock]),
      assistant('a2', [{ type: 'tool_group', tools: null, collapsed: false } as unknown as ContentBlock]),
      assistant('a3', [toolGroup([tool('read')])]),
    ];

    expect(() => buildTranscriptRenderItems(items, { isStreaming: false })).not.toThrow();
    expect(buildTranscriptRenderItems(items, { isStreaming: false }).map((item) => item.type)).toEqual([
      'source',
      'source',
      'source',
      'source',
    ]);
  });

  it('does not throw on malformed text blocks without html/source', () => {
    const items: ChatListItem[] = [
      user('u1'),
      assistant('a1', [
        thinking(),
        { type: 'text' } as unknown as ContentBlock,
        toolGroup([tool('read')]),
      ]),
    ];

    expect(() => buildTranscriptRenderItems(items, { isStreaming: false })).not.toThrow();
    expect(buildTranscriptRenderItems(items, { isStreaming: false })[1]).toMatchObject({
      type: 'source',
      item: items[1],
    });
  });

  it('does not use short text as a process heuristic for legacy messages', () => {
    const items: ChatListItem[] = [
      user('u1'),
      assistant('a1', [
        thinking(),
        textBlock('<p>现在开始执行。</p>', '现在开始执行。'),
        toolGroup([tool('missing-file', false)]),
      ]),
      assistant('a2', [
        thinking(),
        textBlock('<p>第二步：读取真实文件。</p>', '第二步：读取真实文件。'),
        toolGroup([tool('read')]),
      ]),
      assistant('a3', [
        thinking(),
        textBlock('<p>第三步：核对结果。</p>', '第三步：核对结果。'),
        toolGroup([tool('verify')]),
      ]),
      assistant('a4', [
        thinking(),
        textBlock('<p>全部检查完成。以下是总结。</p>', '全部检查完成。以下是总结。'),
      ]),
    ];

    const rendered = buildTranscriptRenderItems(items, { isStreaming: false });

    expect(rendered).toHaveLength(6);
    expect(rendered[1]).toMatchObject({
      type: 'process_fold',
      id: 'a1:process',
      stats: {
        toolCount: 3,
        thinkingCount: 4,
        unsuccessfulCount: 1,
      },
    });
    expect(rendered.slice(2).map((item) => (
      item.type === 'source' && item.item.type === 'message' ? item.item.data.id : null
    ))).toEqual(['a1', 'a2', 'a3', 'a4']);
  });

  it('does not count card-backed tool calls in process fold stats', () => {
    const items: ChatListItem[] = [
      user('u1'),
      assistant('a1', [thinking(), toolGroup([
        tool('media_generate-image', false),
        tool('browser'),
      ])]),
      assistant('a2', [thinking(), toolGroup([
        tool('workflow'),
        tool('install_skill'),
      ])]),
      assistant('a3', [thinking(), toolGroup([
        tool('update_settings'),
        { ...tool('automation'), args: { action: 'pending_update' } },
        tool('hana_card_guide'),
        tool('show_card'),
      ])]),
    ];

    const rendered = buildTranscriptRenderItems(items, { isStreaming: false });

    expect(rendered[1]).toMatchObject({
      type: 'process_fold',
      stats: {
        toolCount: 1,
        thinkingCount: 3,
        unsuccessfulCount: 0,
      },
    });
  });

  it('keeps user steer messages as hard fold boundaries', () => {
    const items: ChatListItem[] = [
      user('u1'),
      assistant('a1', [thinking(), toolGroup([tool('read')])]),
      assistant('a2', [thinking(), toolGroup([tool('write')])]),
      assistant('a3', [thinking(), toolGroup([tool('stat')])]),
      user('u2', '先暂停一下，换个文件看'),
      assistant('a4', [thinking(), toolGroup([tool('grep')])]),
      assistant('a5', [textBlock('<p>第二轮总结。</p>', '第二轮总结。')]),
    ];

    const rendered = buildTranscriptRenderItems(items, { isStreaming: false });

    expect(rendered.map((item) => item.type)).toEqual([
      'source',
      'process_fold',
      'source',
      'process_fold',
      'source',
    ]);
    expect(rendered[1]).toMatchObject({ id: 'a1:process' });
    expect(rendered[2]).toMatchObject({ type: 'source', item: items[4] });
  });

  it('creates separate process groups across hidden custom turn inputs', () => {
    const items: ChatListItem[] = [
      user('u1'),
      assistant('a1', [thinking(), toolGroup([tool('read')])], 'hidden-input-1'),
      assistant('a2', [thinking(), toolGroup([tool('write')])], 'hidden-input-2'),
      assistant('a3', [thinking(), toolGroup([tool('verify')])], 'hidden-input-3'),
    ];

    const rendered = buildTranscriptRenderItems(items, { isStreaming: false });

    expect(rendered).toHaveLength(4);
    expect(rendered.map((item) => item.type)).toEqual([
      'source',
      'process_fold',
      'process_fold',
      'process_fold',
    ]);
  });

  it('keeps legacy final text outside each hidden-turn process group', () => {
    const shortFinal = (label: string) => [
      thinking(),
      textBlock(`<p>${label}</p>`, label),
      toolGroup([tool('verify')]),
    ];
    const items: ChatListItem[] = [
      user('u1'),
      assistant('a1', [thinking(), toolGroup([tool('read')])], 'hidden-input-1'),
      assistant('a2', shortFinal('第一轮完成'), 'hidden-input-1'),
      assistant('a3', [thinking(), toolGroup([tool('write')])], 'hidden-input-2'),
      assistant('a4', shortFinal('第二轮完成'), 'hidden-input-2'),
      assistant('a5', [thinking(), toolGroup([tool('grep')])], 'hidden-input-3'),
      assistant('a6', shortFinal('第三轮完成'), 'hidden-input-3'),
    ];

    const rendered = buildTranscriptRenderItems(items, { isStreaming: false });

    expect(rendered).toHaveLength(7);
    expect(rendered.map((item) => item.type)).toEqual([
      'source',
      'process_fold', 'source',
      'process_fold', 'source',
      'process_fold', 'source',
    ]);
  });

  it('keeps legacy text visible regardless of length while folding its process blocks', () => {
    const longText = '这段内容已经接近真正的阶段性说明，包含足够多的细节和判断，读者刷新页面以后也应该直接看见它。'.repeat(5);
    const items: ChatListItem[] = [
      user('u1'),
      assistant('a1', [thinking(), toolGroup([tool('read')])]),
      assistant('a2', [
        thinking(),
        textBlock(`<p>${longText}</p>`, longText),
        toolGroup([tool('write')]),
      ]),
      assistant('a3', [thinking(), toolGroup([tool('grep')])]),
      assistant('a4', [thinking(), toolGroup([tool('ls')])]),
      assistant('a5', [thinking(), toolGroup([tool('pwd')])]),
      assistant('a6', [textBlock('<p>最后总结。</p>', '最后总结。')]),
    ];

    const rendered = buildTranscriptRenderItems(items, { isStreaming: false });

    expect(rendered.map((item) => item.type)).toEqual([
      'source',
      'process_fold',
      'source',
      'source',
    ]);
    expect(rendered[1]).toMatchObject({ id: 'a1:process' });
    expect(rendered[2]).toMatchObject({ type: 'source', item: { data: { id: 'a2' } } });
    expect(rendered[3]).toMatchObject({ type: 'source', item: { data: { id: 'a6' } } });
  });

  it('leaves the current trailing process segment expanded while the session is streaming', () => {
    const items: ChatListItem[] = [
      user('u1'),
      assistant('old-a1', [thinking(), toolGroup([tool('bash')])]),
      assistant('old-a2', [thinking(), toolGroup([tool('read')])]),
      assistant('old-a3', [thinking(), toolGroup([tool('stat')])]),
      assistant('old-a4', [textBlock('<p>旧正文</p>')]),
      user('u2'),
      assistant('live-a1', [thinking(), toolGroup([tool('grep')])]),
      assistant('live-a2', [thinking(), toolGroup([tool('ls')])]),
    ];

    const rendered = buildTranscriptRenderItems(items, { isStreaming: true });

    // live 段也走 projectedTurnItems，只是 mode='live'：同一个 ProcessRegion 保持
    // 稳定 key，settled 只是切换展示模式（任务书 §二十四/§二十五/§二十七）。
    expect(rendered.map((item) => item.type)).toEqual([
      'source',
      'process_fold',
      'source',
      'source',
      'process_fold',
    ]);
    expect(rendered[1]).toMatchObject({ id: 'old-a1:process', mode: 'settled' });
    expect(rendered[4]).toMatchObject({ type: 'process_fold', id: 'live-a1:process', mode: 'live' });
  });

  it('formats unsuccessful attempts as light process copy', () => {
    const text = buildProcessFoldSummary(
      { toolCount: 13, thinkingCount: 5, unsuccessfulCount: 1, knowledgeCount: 0 },
      '小花',
      (key, vars) => {
        const table: Record<string, string> = {
          'processFold.summary': '✨ {name}忙活了一阵子',
          'processFold.tools': '{n} 个工具',
          'processFold.thinking': '{n} 次思考',
          'processFold.unsuccessful': '{n} 次尝试未成功',
        };
        return (table[key] || key).replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ''));
      },
    );

    expect(text).toBe('✨ 小花忙活了一阵子 · 13 个工具 · 5 次思考 · 1 次尝试未成功');
  });
});

describe('knowledge-only process fold（纯检索轮折叠）', () => {
  function retrievalStats(injectedChunks = 18): KnowledgeRetrievalStats {
    return {
      mode: 'qa',
      retrievalMode: 'hybrid',
      subQueries: ['q1'],
      subQueryHits: [18],
      degraded: false,
      fusedChunks: 197,
      injectedChunks,
      truncated: true,
      usedTokens: 5694,
      budgetTokens: 6000,
    };
  }

  function answerTurn(id: string, status: 'completed' | 'streaming' = 'completed'): ChatListItem {
    const answer: ContentBlock = {
      id: `${id}:answer`,
      type: 'text',
      html: '<p>末日真相是虚构的。</p>',
      source: '末日真相是虚构的。',
      semanticPhase: 'final_answer',
      surfaceRole: 'answer',
      lifecycle: 'sealed',
    };
    const turn = assistant(id, [answer]);
    if (turn.type !== 'message') throw new Error('expected assistant');
    turn.data.turnProjection = {
      id: `${id}:turn`,
      inputMessageId: 'u1',
      assistantMessageIds: [id],
      processBlockIds: [],
      answerBlockIds: [`${id}:answer`],
      resultBlockIds: [],
      controlBlockIds: [],
      status,
    };
    return turn;
  }

  it('无工具/思考的知识问答轮也生成 fold：检索步骤入卡、正文外显、源消息卡片被抑制', () => {
    const turn = answerTurn('a1');
    const rendered = buildTranscriptRenderItems(
      [user('u1', '世界末日'), turn],
      { isStreaming: false, knowledgeRetrievalByIndex: new Map([[1, retrievalStats()]]) },
    );

    expect(rendered.map((item) => item.type)).toEqual(['source', 'process_fold', 'source']);
    const fold = rendered[1];
    expect(fold).toMatchObject({
      type: 'process_fold',
      id: 'a1:process',
      turnId: 'a1:turn',
      blockIds: [],
      refs: [],
      originalIndex: 1,
      stats: { toolCount: 0, thinkingCount: 0, unsuccessfulCount: 0, knowledgeCount: 1 },
      status: 'completed',
      defaultCollapsed: true,
      ownsTurnCompletion: false,
      mode: 'settled',
    });
    // 正文仍在 fold 后原位渲染；「延续本轮」让头像不重复、检索卡由 fold 承载。
    expect(rendered[2]).toMatchObject({
      type: 'source',
      item: { data: { blocks: [expect.objectContaining({ id: 'a1:answer' })] } },
      continuesAssistantTurn: true,
    });
  });

  it('无检索统计的纯文本轮维持原样：不生成 fold', () => {
    const turn = answerTurn('a1');
    const rendered = buildTranscriptRenderItems([user('u1'), turn], { isStreaming: false });

    expect(rendered.map((item) => item.type)).toEqual(['source', 'source']);
    expect(rendered[1]).not.toHaveProperty('continuesAssistantTurn');
  });

  it('live 模式的纯检索 fold 不默认折叠（流式期间检索卡保持可见）', () => {
    const turn = answerTurn('a1', 'streaming');
    const rendered = buildTranscriptRenderItems(
      [user('u1'), turn],
      { isStreaming: true, liveTurnStatus: 'streaming', knowledgeRetrievalByIndex: new Map([[1, retrievalStats()]]) },
    );

    const fold = rendered.find((item) => item.type === 'process_fold');
    expect(fold).toMatchObject({ mode: 'live', defaultCollapsed: false, status: 'completed' });
  });

  it('多轮纯检索各自成 fold：id 以各自轮首消息稳定，不冲突', () => {
    const t1 = answerTurn('a1');
    const t2 = answerTurn('a2');
    const rendered = buildTranscriptRenderItems(
      [user('u1'), t1, user('u2'), t2],
      { isStreaming: false, knowledgeRetrievalByIndex: new Map([[1, retrievalStats()], [3, retrievalStats()]]) },
    );

    const folds = rendered.filter((item) => item.type === 'process_fold');
    expect(folds.map((fold) => (fold as { id: string }).id)).toEqual(['a1:process', 'a2:process']);
  });

  it('纯检索摘要用「N 次检索」；混合轮维持工具步数合并语义', () => {
    const translate = (key: string, vars?: Record<string, string | number>) => {
      const table: Record<string, string> = {
        'processFold.summary': '✨ {name}忙活了一阵子',
        'processFold.tools': '{n} 个工具',
        'processFold.knowledge': '{n} 次检索',
        'processFold.thinking': '{n} 次思考',
        'processFold.unsuccessful': '{n} 次尝试未成功',
      };
      return (table[key] || key).replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ''));
    };
    expect(buildProcessFoldSummary(
      { toolCount: 0, thinkingCount: 0, unsuccessfulCount: 0, knowledgeCount: 1 },
      '小文',
      translate,
    )).toBe('✨ 小文忙活了一阵子 · 1 次检索');
    expect(buildProcessFoldSummary(
      { toolCount: 2, thinkingCount: 1, unsuccessfulCount: 0, knowledgeCount: 1 },
      '小文',
      translate,
    )).toBe('✨ 小文忙活了一阵子 · 3 个工具 · 1 次思考');
  });
});

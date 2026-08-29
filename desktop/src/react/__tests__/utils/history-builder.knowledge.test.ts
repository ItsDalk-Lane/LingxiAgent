import { describe, expect, it } from 'vitest';
import { buildItemsFromHistory } from '../../utils/history-builder';

describe('buildItemsFromHistory 知识库注入泄漏修复', () => {
  it('带 knowledgeRefs 的 user 消息以 displayText 为正文，不泄漏 [KnowledgeContext] 注入块', () => {
    const items = buildItemsFromHistory({
      messages: [{
        id: '0',
        role: 'user',
        content: '帮我总结路线图\n\n[KnowledgeContext notebook="nb-1"]\n[1|笔记本A|块3] 第一段内容…',
        displayText: '帮我总结路线图',
        knowledgeRefs: { notebookIds: ['nb-1'], mode: 'qa' },
        timestamp: 1,
      }],
    } as any);

    const first = items[0];
    expect(first.type).toBe('message');
    if (first.type !== 'message') throw new Error('expected message');
    expect(first.data.text).toBe('帮我总结路线图');
    expect(first.data.text).not.toContain('[KnowledgeContext');
    expect(first.data.knowledgeRefs).toEqual({ notebookIds: ['nb-1'], mode: 'qa' });
  });

  it('带 knowledgeRetrieval 统计但 refs 已被剥离的历史消息同样优先 displayText', () => {
    const items = buildItemsFromHistory({
      messages: [{
        id: '0',
        role: 'user',
        content: '[KnowledgeContext notebook="nb-1"]\n[1|笔记本A|块1] 内容',
        displayText: '查一下上次的决定',
        knowledgeRetrieval: {
          mode: 'qa',
          retrievalMode: 'hybrid',
          subQueries: ['q'],
          subQueryHits: [3],
          degraded: false,
          fusedChunks: 3,
          injectedChunks: 2,
          truncated: false,
          usedTokens: 512,
          budgetTokens: 4000,
        },
        timestamp: 1,
      }],
    } as any);

    const first = items[0];
    expect(first.type).toBe('message');
    if (first.type !== 'message') throw new Error('expected message');
    expect(first.data.text).toBe('查一下上次的决定');
    expect(first.data.text).not.toContain('[KnowledgeContext');
    expect(first.data.knowledgeRetrieval?.injectedChunks).toBe(2);
  });

  it('无知识引用且无 origin 的老消息即便 displayText 存在也走 content 管道，行为不变', () => {
    const items = buildItemsFromHistory({
      messages: [{
        id: '0',
        role: 'user',
        content: '老消息正文',
        displayText: '不应被优先使用',
        timestamp: 1,
      }],
    } as any);

    const first = items[0];
    expect(first.type).toBe('message');
    if (first.type !== 'message') throw new Error('expected message');
    expect(first.data.text).toBe('老消息正文');
  });
});

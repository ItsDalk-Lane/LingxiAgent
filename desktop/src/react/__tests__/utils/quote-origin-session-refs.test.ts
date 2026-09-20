/**
 * 跨会话引用 → 来源会话引用自动附带（侧边聊天上下文补全）：
 *  - 引用来自另一个会话时，来源会话并入 sessionRefs（模型可按需读取原对话）；
 *  - 同会话引用不附加（模型本就有完整上下文）；
 *  - 来源不在会话列表（已归档等）时静默跳过；
 *  - 去重：与手输 @会话 引用同 id 不重复；多条引用同源只附一次；
 *  - 标签取来源会话标题，缺失时退回首条消息截断，再退回通用名。
 */
import { describe, expect, it, beforeEach } from 'vitest';
import { withQuoteOriginSessionRefs } from '../../components/input/composer-send';
import { useStore } from '../../stores';
import type { QuotedSelection } from '../../stores/input-slice';

function chatQuote(sourceSessionPath: string): QuotedSelection {
  return {
    text: '被引用的片段',
    sourceTitle: 'Assistant message',
    sourceKind: 'chat',
    sourceSessionPath,
    sourceMessageId: 'm1',
    sourceRole: 'assistant',
    selectionAnchorKind: 'native',
    charCount: 6,
  };
}

describe('withQuoteOriginSessionRefs', () => {
  beforeEach(() => {
    useStore.setState({
      sessions: [
        { path: '/agents/lingxi/sessions/main.jsonl', sessionId: 'sess_main', title: '主对话', firstMessage: '' },
        { path: '/agents/lingxi/sessions/no-title.jsonl', sessionId: 'sess_no_title', title: null, firstMessage: '这条对话还没有标题' },
        { path: '/agents/lingxi/sessions/bare.jsonl', sessionId: 'sess_bare', title: null, firstMessage: '' },
      ],
    } as never);
  });

  it('cross-session quote attaches the origin session reference with its title', () => {
    const merged = withQuoteOriginSessionRefs(
      [],
      [chatQuote('/agents/lingxi/sessions/main.jsonl')],
      '/agents/lingxi/sessions/side.jsonl',
    );
    expect(merged).toEqual([{ sessionId: 'sess_main', label: '主对话' }]);
  });

  it('falls back to first message then a generic label when the title is missing', () => {
    const withFirstMessage = withQuoteOriginSessionRefs(
      [],
      [chatQuote('/agents/lingxi/sessions/no-title.jsonl')],
      '/side.jsonl',
    );
    expect(withFirstMessage).toEqual([{ sessionId: 'sess_no_title', label: '这条对话还没有标题'.slice(0, 40) }]);

    const bare = withQuoteOriginSessionRefs(
      [],
      [chatQuote('/agents/lingxi/sessions/bare.jsonl')],
      '/side.jsonl',
    );
    expect(bare).toEqual([{ sessionId: 'sess_bare', label: '引用来源对话' }]);
  });

  it('same-session quotes add nothing', () => {
    const merged = withQuoteOriginSessionRefs(
      [],
      [chatQuote('/agents/lingxi/sessions/main.jsonl')],
      '/agents/lingxi/sessions/main.jsonl',
    );
    expect(merged).toEqual([]);
  });

  it('unknown origin sessions are skipped silently', () => {
    const merged = withQuoteOriginSessionRefs(
      [],
      [chatQuote('/agents/lingxi/sessions/archived.jsonl')],
      '/side.jsonl',
    );
    expect(merged).toEqual([]);
  });

  it('deduplicates against manual refs and across multiple quotes', () => {
    const merged = withQuoteOriginSessionRefs(
      [{ sessionId: 'sess_main', label: '手输引用' }],
      [
        chatQuote('/agents/lingxi/sessions/main.jsonl'),
        chatQuote('/agents/lingxi/sessions/main.jsonl'),
      ],
      '/side.jsonl',
    );
    expect(merged).toEqual([{ sessionId: 'sess_main', label: '手输引用' }]);
  });

  it('file quotes without a source session are ignored', () => {
    const merged = withQuoteOriginSessionRefs(
      [],
      [{ text: '片段', sourceTitle: 'doc.md', sourceKind: 'preview', sourceFilePath: '/tmp/doc.md', lineStart: 1, lineEnd: 2, charCount: 2 }],
      '/side.jsonl',
    );
    expect(merged).toEqual([]);
  });
});

import { describe, expect, it } from 'vitest';
import { formatQuotedSelectionForPrompt } from '../../utils/quoted-selection';
import { parseUserAttachments } from '../../utils/message-parser';

describe('formatQuotedSelectionForPrompt', () => {
  it('includes source metadata and the selected original text in the model prompt', () => {
    const result = formatQuotedSelectionForPrompt({
      text: 'ChatGPT 2022 年底刚出来的时候，大家最先玩的是什么？角色扮演。',
      sourceTitle: '脚本-Kimi多智能体.md',
      sourceKind: 'preview',
      sourceFilePath: '/Users/test/脚本-Kimi多智能体.md',
      lineStart: 17,
      lineEnd: 17,
      charCount: 34,
    });

    expect(result).toBe([
      '[引用片段] 脚本-Kimi多智能体.md（第17-17行，共34字）路径: /Users/test/脚本-Kimi多智能体.md',
      '[引用原文]',
      'ChatGPT 2022 年底刚出来的时候，大家最先玩的是什么？角色扮演。',
      '[/引用原文]',
    ].join('\n'));
  });

  it('keeps quoted original text out of the displayed user message when restoring history', () => {
    const input = [
      '有点啰嗦',
      '',
      '[引用片段] 脚本-Kimi多智能体.md（第17-17行，共34字）路径: /Users/test/脚本-Kimi多智能体.md',
      '[引用原文]',
      'ChatGPT 2022 年底刚出来的时候，大家最先玩的是什么？角色扮演。',
      '[/引用原文]',
    ].join('\n');

    const result = parseUserAttachments(input);

    expect(result.text).toBe('有点啰嗦');
    expect(result.quotedText).toBe('ChatGPT 2022 年底刚出来的时候，大家最先玩的是什么？角色扮演。');
  });

  it('chat quote marks the source role so the model knows whose message it came from', () => {
    const result = formatQuotedSelectionForPrompt({
      text: '第二部分（现在）：女孩明显长大了不少。',
      sourceTitle: 'Assistant message',
      sourceKind: 'chat',
      sourceSessionPath: '/agents/lingxi/sessions/main.jsonl',
      sourceMessageId: 'm1',
      sourceRole: 'assistant',
      selectionAnchorKind: 'native',
      charCount: 18,
    });

    expect(result).toBe([
      '[引用片段] 来自对话中的助手消息',
      '[引用原文]',
      '第二部分（现在）：女孩明显长大了不少。',
      '[/引用原文]',
    ].join('\n'));
  });

  it('chat quote without role keeps the legacy single-line format for history compatibility', () => {
    const result = formatQuotedSelectionForPrompt({
      text: '旧格式的片段',
      sourceTitle: 'Assistant message',
      sourceKind: 'chat',
      charCount: 6,
    });

    expect(result).toBe('[引用片段] 旧格式的片段');
  });

  it('new chat quote shape round-trips through history parsing like the file quote', () => {
    const input = [
      '帮我解释这段',
      '',
      '[引用片段] 来自对话中的助手消息',
      '[引用原文]',
      '第二部分（现在）：女孩明显长大了不少。',
      '[/引用原文]',
    ].join('\n');

    const result = parseUserAttachments(input);

    expect(result.text).toBe('帮我解释这段');
    expect(result.quotedText).toBe('第二部分（现在）：女孩明显长大了不少。');
  });
});

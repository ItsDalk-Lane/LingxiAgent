// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { MarkdownContent } from '../../components/chat/MarkdownContent';
import { renderMarkdown } from '../../utils/markdown';

vi.mock('../../utils/mermaid-renderer', () => ({ renderMermaidDiagrams: vi.fn() }));
vi.mock('../../components/chat/knowledge-citation-scope', async importOriginal => ({
  ...await importOriginal<typeof import('../../components/chat/knowledge-citation-scope')>(),
  useKnowledgeCitationResource: () => ({
    resolved: { source: { displayName: '制度.txt' }, citation: { canonicalText: '每年十五天。' } },
    failed: false, retry: vi.fn(),
  }),
}));
afterEach(cleanup);

const citation = '[来源 · 原文](#knowledge-citation-cite_original)';

it.each([['(', ')'], ['（', '）'], ['[', ']'], ['【', '】'], ['（[', ']）']])(
  '引用外的 %s %s 去除后仅显示数字，悬停仍显示原文', (opening, closing) => {
    const { container } = render(<MarkdownContent
      html={renderMarkdown(`回答${opening}${citation}${closing}。`)} numberKnowledgeCitations enhanceMermaid={false} />);
    expect(container.textContent?.trim()).toBe('回答1。');
    const marker = container.querySelector('a')!;
    expect(marker.textContent).toBe('1');
    fireEvent.mouseOver(marker);
    expect(screen.getByRole('tooltip').textContent).toContain('每年十五天。');
  },
);

it('保留带说明文字的括号、普通链接和代码示例，重复引用编号不变', () => {
  const { container } = render(<MarkdownContent
    html={renderMarkdown(`回答（参见${citation}），${citation}。（[网站](https://example.com)）。\`${citation}\``)}
    numberKnowledgeCitations enhanceMermaid={false} />);
  expect(container.textContent).toContain('回答（参见1），1。（网站）。');
  expect(container.querySelector('code')?.textContent).toBe(citation);
});

it('流式更新后去掉完整括号，并保留原有编号', () => {
  const { container, rerender } = render(<MarkdownContent
    html={renderMarkdown(`回答（${citation}`)} numberKnowledgeCitations enhanceMermaid={false} />);
  rerender(<MarkdownContent html={renderMarkdown(`回答（${citation}）。`)} numberKnowledgeCitations enhanceMermaid={false} />);
  expect(container.textContent?.trim()).toBe('回答1。');
});

it.each([
  `<citationMarkdown>${citation}</citationMarkdown>`,
  `（<citationMarkdown> ${citation} </citationMarkdown>）`,
  `<citationMarkdown>（${citation}）</citationMarkdown>`,
])('兼容历史回答中只包装有效引用的字段标签：%s', source => {
  const { container } = render(<MarkdownContent html={renderMarkdown(`回答${source}。`)} numberKnowledgeCitations enhanceMermaid={false} />);
  expect(container.textContent?.trim()).toBe('回答1。');
  fireEvent.mouseOver(container.querySelector('a')!);
  expect(screen.getByRole('tooltip').textContent).toContain('每年十五天。');
});

it('保留代码中的字段标签、普通标签文字以及包装说明正文的标签', () => {
  const example = `<citationMarkdown>${citation}</citationMarkdown>`;
  const source = `\`${example}\`\n\n\`\`\`text\n${example}\n\`\`\`\n\n<citationMarkdown>普通正文</citationMarkdown>\n\n<citationMarkdown>参见${citation}</citationMarkdown>\n\n<citationMarkdown>[普通链接](https://example.com)</citationMarkdown>`;
  const { container } = render(<MarkdownContent html={renderMarkdown(source)} numberKnowledgeCitations enhanceMermaid={false} />);
  expect(Array.from(container.querySelectorAll('code')).map(node => node.textContent?.trim())).toEqual([example, example]);
  expect(container.textContent).toContain('<citationMarkdown>普通正文</citationMarkdown>');
  expect(container.textContent).toContain('<citationMarkdown>参见1</citationMarkdown>');
  expect(container.textContent).toContain('<citationMarkdown>普通链接</citationMarkdown>');
});

it('流式字段标签完整后恢复纯数字，不改动缺少有效引用身份的文本', () => {
  const { container, rerender } = render(<MarkdownContent
    html={renderMarkdown(`回答<citationMarkdown>${citation}`)} numberKnowledgeCitations enhanceMermaid={false} />);
  rerender(<MarkdownContent html={renderMarkdown(`回答<citationMarkdown>${citation}</citationMarkdown>。`)} numberKnowledgeCitations enhanceMermaid={false} />);
  expect(container.textContent?.trim()).toBe('回答1。');
  rerender(<MarkdownContent html={renderMarkdown('<citationMarkdown>[来源](#knowledge-citation-unknown)</citationMarkdown>')} numberKnowledgeCitations enhanceMermaid={false} />);
  expect(container.textContent?.trim()).toBe('<citationMarkdown>来源</citationMarkdown>');
});

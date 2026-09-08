// @vitest-environment jsdom

import React from 'react';
import fs from 'node:fs';
import path from 'node:path';
import { cleanup, fireEvent, render, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { StreamingMarkdownContent } from '../../components/chat/StreamingMarkdownContent';
import { MarkdownContent } from '../../components/chat/MarkdownContent';
import { injectCopyButtons } from '../../utils/format';
import { renderMarkdown } from '../../utils/markdown';
import { renderMermaidDiagrams } from '../../utils/mermaid-renderer';

vi.mock('../../utils/mermaid-renderer', () => ({
  renderMermaidDiagrams: vi.fn(async () => undefined),
}));

vi.mock('../../utils/format', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../utils/format')>();
  return {
    ...actual,
    injectCopyButtons: vi.fn(),
  };
});

describe('StreamingMarkdownContent', () => {
  beforeEach(() => {
    vi.mocked(injectCopyButtons).mockClear();
    vi.mocked(renderMermaidDiagrams).mockClear();
    vi.spyOn(window, 'requestAnimationFrame');
    vi.spyOn(window, 'cancelAnimationFrame');
    window.matchMedia = vi.fn().mockReturnValue({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() }) as unknown as typeof window.matchMedia;
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('R05-01/09：围栏代码的转义标签在实时、终态和复制中保留原反斜杠', async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
    window.t = ((key: string) => key) as typeof window.t;
    const source = '```text\n\\<tag> 与 \\</think>\n```';
    const { container, rerender } = render(<StreamingMarkdownContent source={source} active />);

    expect(container.querySelector('code')?.textContent).toBe('\\<tag> 与 \\</think>\n');
    fireEvent.click(container.querySelectorAll<HTMLButtonElement>('.code-block-toolbar-btn')[1]);
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('\\<tag> 与 \\</think>\n'));

    rerender(<StreamingMarkdownContent source={source} active={false} />);
    expect(container.querySelector('code')?.textContent).toBe('\\<tag> 与 \\</think>\n');
  });

  it('R05-02：行内代码中的闭标签和反斜杠不被协议解析或显示预处理删除', () => {
    const source = '教学：`\\</think>` 与 `C:\\path\\<tag>`';
    const { container } = render(<StreamingMarkdownContent source={source} active />);
    expect(Array.from(container.querySelectorAll('code')).map(node => node.textContent)).toEqual([
      '\\</think>',
      'C:\\path\\<tag>',
    ]);
  });

  it('R05-03/05：正文转义标签只由 Markdown 消费且同一 source 重渲染稳定', () => {
    const source = '已知：\\<mood>平静\\</mood>；未知：\\<vendor:tag>内容\\</vendor:tag>';
    const { container, rerender } = render(<StreamingMarkdownContent source={source} active />);
    const first = container.querySelector('.md-content')?.innerHTML;
    expect(container.textContent).toContain('<mood>平静</mood>');
    expect(container.textContent).toContain('<vendor:tag>内容</vendor:tag>');
    expect(container.querySelector('mood')).toBeNull();
    expect(container.querySelector('vendor\\:tag')).toBeNull();
    rerender(<StreamingMarkdownContent source={source} active={false} />);
    expect(container.querySelector('.md-content')?.innerHTML).toBe(first);
    expect(source).toBe('已知：\\<mood>平静\\</mood>；未知：\\<vendor:tag>内容\\</vendor:tag>');
  });

  it('R05-03：旧直测反例经真实组件按 Markdown 语义显示且普通反斜杠不变', () => {
    const tagged = '\\</think>\\<b>x</b>与\\<custom/>';
    const first = render(<StreamingMarkdownContent source={tagged} active={false} />);
    expect(first.container.textContent?.trimEnd()).toBe('</think><b>x</b>与<custom/>');
    expect(first.container.querySelector('b')).toBeNull();
    first.unmount();

    const ordinary = 'C:\\path 与 1 \\< 2';
    const second = render(<StreamingMarkdownContent source={ordinary} active={false} />);
    expect(second.container.textContent?.trimEnd()).toBe('C:\\path 与 1 < 2');
    second.unmount();

    const once = '\\</vendor:tail>';
    const third = render(<StreamingMarkdownContent source={once} active={false} />);
    expect(third.container.textContent?.trimEnd()).toBe('</vendor:tail>');
    third.rerender(<StreamingMarkdownContent source={once} active={false} />);
    expect(third.container.textContent?.trimEnd()).toBe('</vendor:tail>');
  });

  it('R05-06：同一 canonical source 的每个 delta 切分经实时组件后都收敛到同一显示结果', () => {
    const canonical = '前\\<think>字面\\</think>；`\\</tag>`；后';
    const expected = render(<StreamingMarkdownContent source={canonical} active={false} />);
    const expectedHtml = expected.container.querySelector('.md-content')?.innerHTML;
    expected.unmount();

    for (let split = 1; split < canonical.length; split += 1) {
      const view = render(<StreamingMarkdownContent source={canonical.slice(0, split)} active />);
      view.rerender(<StreamingMarkdownContent source={canonical} active />);
      view.rerender(<StreamingMarkdownContent source={canonical} active={false} />);
      expect(view.container.querySelector('.md-content')?.innerHTML).toBe(expectedHtml);
      view.unmount();
    }
    expect(canonical).toBe('前\\<think>字面\\</think>；`\\</tag>`；后');
  });

  it('R05-04：连续1到4个反斜杠按 Markdown 词法处理，不由组件额外删层', () => {
    const source = String.raw`一：\<tag>；二：\\<tag>；三：\\\<tag>；四：\\\\<tag>`;
    const expected = renderMarkdown(source);
    const { container } = render(<StreamingMarkdownContent source={source} active={false} />);
    expect(container.querySelector('.md-content')?.innerHTML).toBe(expected);
    expect(source).toBe(String.raw`一：\<tag>；二：\\<tag>；三：\\\<tag>；四：\\\\<tag>`);
  });

  it('R05-10：超长纯文本降级直接转义 canonical source，旧 html-only 内容不被反推', () => {
    const source = '代码 \\<tag> 和 \\</think>';
    const active = render(<StreamingMarkdownContent source={source} active richTextCharLimit={1} />);
    expect(active.container.textContent).toBe(source);
    active.unmount();

    const legacyHtml = '<p>旧内容 &lt;tag&gt;，来源已缺失</p>';
    const direct = render(<MarkdownContent html={legacyHtml} />);
    const legacy = render(<StreamingMarkdownContent html={legacyHtml} active={false} />);
    expect(legacy.container.innerHTML).toBe(direct.container.innerHTML);
  });

  it('treats source as authoritative when a stale legacy html cache is also present', () => {
    const { container } = render(
      <StreamingMarkdownContent source="**权威原文**" html="<p>过期缓存</p>" active={false} />,
    );

    expect(container.textContent).toContain('权威原文');
    expect(container.querySelector('strong')?.textContent).toBe('权威原文');
    expect(container.textContent).not.toContain('过期缓存');
  });

  it('defers mermaid enhancement until the turn is settled', async () => {
    const source = '```mermaid\ngraph TD\n  A-->B\n```';
    const { rerender } = render(
      <StreamingMarkdownContent source={source} active />,
    );

    await Promise.resolve();
    expect(renderMermaidDiagrams).not.toHaveBeenCalled();

    rerender(<StreamingMarkdownContent source={source} active={false} />);
    await waitFor(() => expect(renderMermaidDiagrams).toHaveBeenCalledTimes(1));
  });

  it('defers KaTeX layout until the turn is settled', () => {
    const source = '$x^2 + y^2$';
    const { container, rerender } = render(
      <StreamingMarkdownContent source={source} active />,
    );

    expect(container.querySelector('.katex')).toBeNull();
    expect(container.textContent).toContain('$x^2 + y^2$');

    rerender(<StreamingMarkdownContent source={source} active={false} />);
    expect(container.querySelector('.katex')).not.toBeNull();
  });

  it('uses a configurable plain-text circuit breaker for oversized active source', () => {
    const source = '**很长但仍是权威原文**';
    const { container, rerender } = render(
      <StreamingMarkdownContent source={source} active richTextCharLimit={10} />,
    );

    expect(container.querySelector('[data-stream-plain-text="true"]')).not.toBeNull();
    expect(container.querySelector('strong')).toBeNull();
    expect(container.textContent).toContain('**很长但仍是权威原文**');

    rerender(
      <StreamingMarkdownContent source={source} active={false} richTextCharLimit={10} />,
    );
    expect(container.querySelector('[data-stream-plain-text="true"]')).toBeNull();
    expect(container.querySelector('strong')?.textContent).toBe('很长但仍是权威原文');
  });

  it('renders active prose through markdown html instead of a plain-text fallback', () => {
    const { container, rerender } = render(
      <StreamingMarkdownContent source="旧正文" html="<p>旧正文</p>" active />,
    );

    expect(container.textContent?.trim()).toBe('旧正文');
    const root = container.querySelector('.md-content');
    expect(root).not.toBeNull();
    expect(root?.getAttribute('data-stream-plain-text')).toBeNull();
    expect(root?.querySelector('p')?.textContent).toBe('旧正文');
    expect(root?.querySelector('[data-stream-tail-chunk="true"]')).toBeNull();

    rerender(
      <StreamingMarkdownContent source="旧正文新正文继续出现" html="<p>旧正文新正文继续出现</p>" active />,
    );

    expect(container.querySelector('.md-content')).toBe(root);
    expect(container.querySelector('p')?.textContent).toBe('旧正文新正文继续出现');
    expect(container.querySelector('[data-stream-tail-chunk="true"]')).toBeNull();
    expect(window.requestAnimationFrame).not.toHaveBeenCalled();
  });

  it('matches final markdown paragraph structure while prose is streaming', () => {
    const source = '第一段。\n\n第二段。';
    const html = '<p>第一段。</p>\n<p>第二段。</p>';

    const { container } = render(
      <StreamingMarkdownContent source={source} html={html} active />,
    );

    const paragraphs = Array.from(container.querySelectorAll('.md-content > p'));
    expect(paragraphs.map(p => p.textContent)).toEqual(['第一段。', '第二段。']);
  });

  it('updates prose immediately through the upstream 30Hz flush instead of local text animation debt', () => {
    const { container, rerender } = render(
      <StreamingMarkdownContent source="你好" html="<p>你好</p>" active />,
    );

    rerender(
      <StreamingMarkdownContent source="你好世界" html="<p>你好世界</p>" active />,
    );

    expect(container.textContent?.trim()).toBe('你好世界');
  });

  it('hard-catches up 80-character prose backlogs without waiting for animation debt', () => {
    const source = '开头';
    const largeTarget = `${source}${'一'.repeat(80)}`;
    const { container, rerender } = render(
      <StreamingMarkdownContent source={source} html={`<p>${source}</p>`} active />,
    );

    rerender(
      <StreamingMarkdownContent source={largeTarget} html={`<p>${largeTarget}</p>`} active />,
    );

    expect(container.textContent?.trim()).toBe(largeTarget);
    expect(container.querySelector('[data-stream-plain-text="true"]')).toBeNull();
  });

  it('renders final prose with markdown html when streaming is complete', () => {
    const { container } = render(
      <StreamingMarkdownContent source="完成正文" html="<p>完成正文</p>" active={false} />,
    );

    expect(container.querySelector('.md-content')?.getAttribute('data-stream-plain-text')).toBeNull();
    expect(container.querySelector('p')?.textContent).toBe('完成正文');
  });

  it('does not typewriter complex markdown blocks', () => {
    const source = '```ts\nconst x = 1;\n```';
    const html = '<pre><code>const x = 1;</code></pre>';

    const { container } = render(
      <StreamingMarkdownContent source={source} html={html} active />,
    );

    expect(container.textContent).toContain('const x = 1;');
    expect(container.querySelector('[data-stream-tail-chunk="true"]')).toBeNull();
    expect(container.querySelector('[class*="streamMarkdownBlockEnter"]')).not.toBeNull();
  });

  it('keeps complex markdown mounted while streaming updates arrive', () => {
    const source = '```ts\nconst x = 1;\n```';
    const html = '<pre><code>const x = 1;</code></pre>';
    const { container, rerender } = render(
      <StreamingMarkdownContent source={source} html={html} active />,
    );
    const root = container.querySelector('.md-content');

    rerender(
      <StreamingMarkdownContent
        source={`${source}\n\n后续说明`}
        html="<pre><code>const x = 1;</code></pre><p>后续说明</p>"
        active
      />,
    );

    expect(container.querySelector('.md-content')).toBe(root);
    expect(container.textContent).toContain('后续说明');
  });

  it('co-renders code block toolbar without post-render DOM injection', () => {
    const source = '```ts\nconst x = 1;\n```';
    const html = '<pre><code>const x = 1;</code></pre>';

    const { container } = render(
      <StreamingMarkdownContent source={source} html={html} active />,
    );

    expect(container.querySelector('.code-block-wrap')).not.toBeNull();
    expect(container.querySelector('.code-block-toolbar')).not.toBeNull();
    expect(container.querySelectorAll('.code-block-toolbar-btn')).toHaveLength(2);
    expect(injectCopyButtons).not.toHaveBeenCalled();
  });

  it('handles co-rendered code toolbar wrap and copy actions through React events', async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });
    window.t = ((key: string) => {
      if (key === 'attach.copy') return '复制';
      if (key === 'attach.copied') return '已复制';
      if (key === 'codeBlock.wordWrap') return '自动换行';
      return key;
    }) as typeof window.t;

    const { container } = render(
      <StreamingMarkdownContent
        source={'```ts\nconst x = 1;\n```'}
        html="<pre><code>const x = 1;</code></pre>"
        active
      />,
    );

    const wrapper = container.querySelector<HTMLDivElement>('.code-block-wrap');
    const buttons = container.querySelectorAll<HTMLButtonElement>('.code-block-toolbar-btn');
    const wrapBtn = buttons[0];
    const copyBtn = buttons[1];

    expect(wrapper, container.innerHTML).not.toBeNull();
    fireEvent.click(wrapBtn);
    expect(wrapper?.dataset.wrap).toBe('true');
    expect(wrapBtn.dataset.active).toBe('true');

    fireEvent.click(copyBtn);
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('const x = 1;\n'));
    expect(copyBtn.dataset.copied).toBe('true');
    expect(copyBtn.getAttribute('aria-label')).toBe('已复制');
  });

  it('does not typewriter backtick-sensitive inline markdown while streaming', () => {
    const source = '这里有 `inline code`，后续文字也要稳定显示。';
    const html = '<p>这里有 <code>inline code</code>，后续文字也要稳定显示。</p>';

    const { container } = render(
      <StreamingMarkdownContent source={source} html={html} active />,
    );

    expect(container.textContent).toContain('后续文字也要稳定显示。');
    expect(container.querySelector('[data-stream-tail-chunk="true"]')).toBeNull();
    expect(container.querySelector('[class*="streamMarkdownBlockEnter"]')).not.toBeNull();
  });

  it('keeps common markdown formatting rendered while streaming', () => {
    const source = [
      '## 小标题',
      '',
      '- 第一项',
      '- **重点项**',
      '',
      '> 引用',
      '',
      '[链接](https://example.com)',
    ].join('\n');
    const html = [
      '<h2>小标题</h2>',
      '<ul><li>第一项</li><li><strong>重点项</strong></li></ul>',
      '<blockquote><p>引用</p></blockquote>',
      '<p><a href="https://example.com">链接</a></p>',
    ].join('');

    const { container } = render(
      <StreamingMarkdownContent source={source} html={html} active />,
    );

    expect(container.querySelector('[data-stream-plain-text="true"]')).toBeNull();
    expect(container.querySelector('h2')?.textContent).toBe('小标题');
    expect(container.querySelectorAll('li')).toHaveLength(2);
    expect(container.querySelector('strong')?.textContent).toBe('重点项');
    expect(container.querySelector('blockquote')?.textContent).toContain('引用');
    expect(container.querySelector('a')?.getAttribute('href')).toBe('https://example.com');
  });

  it('does not fall back to plain text when rendered html contains formatting', () => {
    const source = '**重点**';
    const html = '<p><strong>重点</strong></p>';

    const { container } = render(
      <StreamingMarkdownContent source={source} html={html} active />,
    );

    expect(container.querySelector('[data-stream-plain-text="true"]')).toBeNull();
    expect(container.querySelector('strong')?.textContent).toBe('重点');
  });

  it('uses identical markdown html structure while streaming and after completion', () => {
    const source = [
      '## 小标题',
      '',
      '第一段 **重点**。',
      '',
      '- 第一项',
      '- 第二项',
      '',
      '> 引用',
    ].join('\n');
    const html = [
      '<h2>小标题</h2>',
      '<p>第一段 <strong>重点</strong>。</p>',
      '<ul><li>第一项</li><li>第二项</li></ul>',
      '<blockquote><p>引用</p></blockquote>',
    ].join('');

    const activeRender = render(
      <StreamingMarkdownContent source={source} html={html} active />,
    );
    const activeRoot = activeRender.container.querySelector('.md-content');
    expect(activeRoot?.getAttribute('data-stream-plain-text')).toBeNull();
    const activeInnerHtml = activeRoot?.innerHTML;
    activeRender.unmount();

    const finalRender = render(
      <StreamingMarkdownContent source={source} html={html} active={false} />,
    );
    const finalRoot = finalRender.container.querySelector('.md-content');

    expect(finalRoot?.getAttribute('data-stream-plain-text')).toBeNull();
    expect(activeInnerHtml).toBe(finalRoot?.innerHTML);
  });

  it('uses identical mermaid html structure while streaming and after completion', () => {
    const source = [
      '```mermaid',
      'graph TD',
      '  A-->B',
      '```',
    ].join('\n');
    const html = renderMarkdown(source);

    const activeRender = render(
      <StreamingMarkdownContent source={source} html={html} active />,
    );
    const activeRoot = activeRender.container.querySelector('.md-content');
    const activeInnerHtml = activeRoot?.innerHTML;
    activeRender.unmount();

    const finalRender = render(
      <StreamingMarkdownContent source={source} html={html} active={false} />,
    );
    const finalRoot = finalRender.container.querySelector('.md-content');

    expect(activeRoot?.querySelector('.mermaid-diagram')).not.toBeNull();
    expect(activeRoot?.querySelector('.mermaid-source code')?.textContent).toContain('graph TD');
    expect(activeRoot?.querySelector('.mermaid-rendered')).not.toBeNull();
    expect(activeInnerHtml).toBe(finalRoot?.innerHTML);
    expect(activeInnerHtml).not.toContain('language-mermaid');
  });

  it('keeps stream motion off React animation frames and limits CSS to opacity or tiny transforms', () => {
    const css = fs.readFileSync(
      path.join(process.cwd(), 'desktop/src/react/components/chat/Chat.module.css'),
      'utf8',
    );
    const animations = fs.readFileSync(
      path.join(process.cwd(), 'desktop/src/animations.css'),
      'utf8',
    );
    const tailBlock = css.match(/\.streamTailChunk\s*\{(?<body>[^}]*)\}/)?.groups?.body || '';
    const cardBlock = css.match(/\.mediaGenerationCard\s*\{(?<body>[^}]*)\}/)?.groups?.body || '';

    expect(tailBlock).toContain('hana-stream-tail-in');
    expect(tailBlock).not.toContain('requestAnimationFrame');
    expect(cardBlock).toContain('hana-chat-soft-up-in');
    expect(animations).toContain('@keyframes hana-stream-tail-in');
    expect(animations).toContain('@keyframes hana-chat-soft-down-in');
    expect(animations).toContain('@keyframes hana-chat-soft-up-in');
  });
});

import { describe, expect, it, vi } from 'vitest';
import {
  createIncrementalMarkdownCache,
  updateIncrementalMarkdownCache,
} from '../../utils/incremental-markdown';
import { observeChatPerformance, type ChatPerformanceEvent } from '../../utils/chat-performance';
import { renderMarkdown } from '../../utils/markdown';

describe('incremental markdown cache', () => {
  it('冻结解析器确认完成的前缀，只重绘最近两个活动块', () => {
    const renderFragment = vi.fn((source: string) => `<render>${source}</render>`);
    const firstSource = [
      '第一段。',
      '',
      '第二段。',
      '',
      '第三段。',
      '',
      '第四段。',
      '',
      '第五段正在生成。',
    ].join('\n');

    const first = updateIncrementalMarkdownCache(
      createIncrementalMarkdownCache(),
      firstSource,
      { active: true, renderFragment },
    );

    expect(first.frozenSourceEnd).toBeGreaterThan(0);
    expect(firstSource.slice(0, first.frozenSourceEnd)).toContain('第三段。');
    expect(firstSource.slice(first.frozenSourceEnd)).toContain('第四段。');

    const frozenHtml = first.frozenHtml;
    renderFragment.mockClear();
    const nextSource = `${firstSource}继续追加。`;
    const next = updateIncrementalMarkdownCache(first, nextSource, {
      active: true,
      renderFragment,
    });

    expect(next.frozenHtml).toBe(frozenHtml);
    expect(renderFragment).toHaveBeenCalledTimes(1);
    expect(renderFragment.mock.calls[0][0]).toBe(nextSource.slice(first.frozenSourceEnd));
    expect(renderFragment.mock.calls[0][0]).not.toContain('第一段。');
  });

  it('收口时对权威原文做一次完整富渲染', () => {
    const renderFragment = vi.fn((source: string) => `<render>${source}</render>`);
    const source = '第一段。\n\n第二段。\n\n第三段。';
    const streaming = updateIncrementalMarkdownCache(
      createIncrementalMarkdownCache(),
      source,
      { active: true, renderFragment },
    );

    renderFragment.mockClear();
    const settled = updateIncrementalMarkdownCache(streaming, source, {
      active: false,
      renderFragment,
    });

    expect(renderFragment).toHaveBeenCalledExactlyOnceWith(source);
    expect(settled.html).toBe(`<render>${source}</render>`);
    expect(settled.mode).toBe('settled');
  });

  it('一百个冻结段落后，新增量的解析输入只包含活动尾部', () => {
    const source = Array.from({ length: 101 }, (_, index) => `第 ${index + 1} 段。`).join('\n\n');
    const events: ChatPerformanceEvent[] = [];
    const stop = observeChatPerformance((event) => events.push(event));
    let cache = updateIncrementalMarkdownCache(
      createIncrementalMarkdownCache(),
      source,
      { active: true, renderFragment: renderMarkdown },
    );
    events.length = 0;

    const nextSource = `${source}继续增长。`;
    cache = updateIncrementalMarkdownCache(cache, nextSource, {
      active: true,
      renderFragment: renderMarkdown,
    });
    stop();

    const mutableTailLength = nextSource.length - cache.frozenSourceEnd;
    expect(events.filter((event) => event.name === 'markdown_incremental_boundary')).toEqual([
      expect.objectContaining({ sourceLength: mutableTailLength }),
    ]);
    expect(events.filter((event) => event.name === 'markdown_parse')).toEqual([
      expect.objectContaining({ sourceLength: mutableTailLength }),
    ]);
    expect(mutableTailLength).toBeLessThan(nextSource.length / 10);
  });
});

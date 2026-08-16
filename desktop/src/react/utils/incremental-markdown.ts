import { getStreamingMd } from './markdown';
import { measureChatPerformance } from './chat-performance';

export interface IncrementalMarkdownCache {
  source: string;
  frozenSourceEnd: number;
  frozenHtml: string;
  tailHtml: string;
  html: string;
  mode: 'streaming' | 'settled';
}

interface UpdateIncrementalMarkdownOptions {
  active: boolean;
  renderFragment: (source: string) => string;
  mutableBlockCount?: number;
}

export function createIncrementalMarkdownCache(): IncrementalMarkdownCache {
  return {
    source: '',
    frozenSourceEnd: 0,
    frozenHtml: '',
    tailHtml: '',
    html: '',
    mode: 'streaming',
  };
}

function lineOffsets(source: string): number[] {
  const offsets = [0];
  for (let index = 0; index < source.length; index += 1) {
    if (source.charCodeAt(index) === 10) offsets.push(index + 1);
  }
  return offsets;
}

/**
 * 由 markdown 解析器的顶层 token 行范围确定冻结边界。
 * 最后两个顶层块始终留在活动尾部，避免手写空行或围栏规则误判。
 */
export function markdownFrozenPrefixEnd(source: string, mutableBlockCount = 2): number {
  if (!source || mutableBlockCount < 1) return 0;
  const tokens = measureChatPerformance(
    'markdown_incremental_boundary',
    { sourceLength: source.length },
    () => getStreamingMd().parse(source, {}),
  );
  const ranges = tokens
    .filter((token) => token.level === 0 && token.map && token.map[1] > token.map[0])
    .map((token) => token.map as [number, number]);
  if (ranges.length <= mutableBlockCount) return 0;

  const mutableStartLine = ranges[ranges.length - mutableBlockCount][0];
  return lineOffsets(source)[mutableStartLine] ?? 0;
}

export function updateIncrementalMarkdownCache(
  previous: IncrementalMarkdownCache,
  source: string,
  options: UpdateIncrementalMarkdownOptions,
): IncrementalMarkdownCache {
  if (
    previous.source === source
    && previous.mode === (options.active ? 'streaming' : 'settled')
  ) return previous;

  if (!options.active) {
    const html = options.renderFragment(source);
    return {
      source,
      frozenSourceEnd: source.length,
      frozenHtml: html,
      tailHtml: '',
      html,
      mode: 'settled',
    };
  }

  const canExtendPrevious = previous.mode === 'streaming'
    && source.startsWith(previous.source)
    && previous.frozenSourceEnd <= source.length;
  let frozenSourceEnd = canExtendPrevious ? previous.frozenSourceEnd : 0;
  let frozenHtml = canExtendPrevious ? previous.frozenHtml : '';
  const mutableSource = source.slice(frozenSourceEnd);
  const relativeFrozenEnd = markdownFrozenPrefixEnd(
    mutableSource,
    options.mutableBlockCount ?? 2,
  );

  if (relativeFrozenEnd > 0) {
    frozenHtml += options.renderFragment(mutableSource.slice(0, relativeFrozenEnd));
    frozenSourceEnd += relativeFrozenEnd;
  }

  const tailSource = source.slice(frozenSourceEnd);
  const tailHtml = tailSource ? options.renderFragment(tailSource) : '';
  return {
    source,
    frozenSourceEnd,
    frozenHtml,
    tailHtml,
    html: `${frozenHtml}${tailHtml}`,
    mode: 'streaming',
  };
}

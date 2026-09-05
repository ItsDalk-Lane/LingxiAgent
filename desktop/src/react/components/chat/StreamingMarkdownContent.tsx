import { memo, useRef } from 'react';
import type { LinkOpenContext } from '../../utils/link-open';
import { renderMarkdown, renderStreamingMarkdown } from '../../utils/markdown';
import {
  createIncrementalMarkdownCache,
  updateIncrementalMarkdownCache,
} from '../../utils/incremental-markdown';
import { escapeHtml } from '../../utils/format';
import { MarkdownContent } from './MarkdownContent';
import styles from './Chat.module.css';

interface Props {
  html?: string;
  source?: string;
  active?: boolean;
  className?: string;
  linkContext?: LinkOpenContext;
  richTextCharLimit?: number;
  numberKnowledgeCitations?: boolean;
}

// 阈值取自阶段 1 的 10k/50k/100k 压力档，并允许调用方在测试或产品调优时覆盖。
export const STREAMING_MARKDOWN_RICH_TEXT_CHAR_LIMIT = 100_000;

function cx(...parts: Array<string | false | null | undefined>): string | undefined {
  const value = parts.filter(Boolean).join(' ');
  return value || undefined;
}

export const StreamingMarkdownContent = memo(function StreamingMarkdownContent({
  html,
  source,
  active = false,
  className,
  linkContext,
  richTextCharLimit = STREAMING_MARKDOWN_RICH_TEXT_CHAR_LIMIT,
  numberKnowledgeCitations = false,
}: Props) {
  const shouldAnimateStream = !!source && active;
  const cacheRef = useRef(createIncrementalMarkdownCache());
  const usePlainText = typeof source === 'string'
    && active
    && source.length > richTextCharLimit;
  const renderedHtml = usePlainText
    ? `<p data-stream-plain-text="true">${escapeHtml(source).replace(/\r?\n/g, '<br>')}</p>`
    : typeof source === 'string'
    ? (cacheRef.current = updateIncrementalMarkdownCache(cacheRef.current, source, {
        active,
        renderFragment: active ? renderStreamingMarkdown : renderMarkdown,
      })).html
    : html || '';

  return (
    <MarkdownContent
      html={renderedHtml}
      className={cx(className, shouldAnimateStream && styles.streamMarkdownBlockEnter)}
      linkContext={linkContext}
      enhanceMermaid={!active}
      numberKnowledgeCitations={numberKnowledgeCitations}
    />
  );
});

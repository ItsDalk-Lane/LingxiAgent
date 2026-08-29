/**
 * KnowledgeRetrievalFold — 知识检索步骤的工具条样式呈现。
 *
 * 数据来自配对 user 消息的 knowledgeRetrieval（Wave 1 冻结契约）：
 * - 收起：单行工具摘要（📚 已搜索 {injectedChunks} 个结果；truncated 加
 *   「超预算分片」小标；unavailableReason 时只报「知识检索不可用」）。
 * - 展开：每条一行 `#ordinal firstLine`（编号 + 首行，不是全文），
 *   行 title 悬浮 `{sourceName} · 块 {chunkOrdinal}`。
 * 视觉复用 ToolGroupBlock 的工具条 class，不另起卡片形态。
 */

import { memo, useCallback, useState } from 'react';
import { Collapse } from '@/ui';
import type { KnowledgeRetrievalStats } from '../../../../../shared/knowledge-refs.ts';
import styles from './Chat.module.css';

interface Props {
  retrieval: KnowledgeRetrievalStats;
}

type RetrievalResult = NonNullable<KnowledgeRetrievalStats['results']>[number];

function isRetrievalResult(value: unknown): value is RetrievalResult {
  return !!value && typeof value === 'object'
    && typeof (value as { ordinal?: unknown }).ordinal === 'number'
    && typeof (value as { firstLine?: unknown }).firstLine === 'string';
}

export const KnowledgeRetrievalFold = memo(function KnowledgeRetrievalFold({ retrieval }: Props) {
  const t = window.t ?? ((key: string) => key);
  const [expanded, setExpanded] = useState(false);
  const toggle = useCallback(() => setExpanded(value => !value), []);

  const unavailable = !!retrieval.unavailableReason;
  const results = Array.isArray(retrieval.results)
    ? retrieval.results.filter(isRetrievalResult)
    : [];
  const expandable = !unavailable && results.length > 0;
  const summary = unavailable
    ? t('chat.knowledgeRetrievalUnavailable')
    : t('chat.knowledgeRetrievalSearched', { n: retrieval.injectedChunks });

  return (
    <div className={styles.toolGroup} data-testid="knowledge-retrieval-fold">
      <div
        className={`${styles.toolGroupSummary}${expandable ? ` ${styles.toolGroupSummaryClickable}` : ''}`}
        onClick={expandable ? toggle : undefined}
      >
        <span className={styles.knowledgeRetrievalIcon} aria-hidden="true">📚</span>
        <span className={styles.toolGroupTitle}>{summary}</span>
        {retrieval.distilled && !unavailable && (
          <span className={styles.knowledgeRetrievalBadge}>
            {t('chat.knowledgeRetrievalDistilled', { n: retrieval.distillBatches ?? 0 })}
          </span>
        )}
        {retrieval.truncated && !unavailable && (
          <span className={styles.knowledgeRetrievalBadge}>
            {t('chat.knowledgeRetrievalTruncated')}
          </span>
        )}
        {expandable && (
          <span className={styles.toolGroupArrow} aria-hidden="true">{expanded ? '‹' : '›'}</span>
        )}
      </div>
      {expandable && (
        <Collapse open={expanded}>
          <div className={styles.toolGroupContent}>
            {results.map((result) => (
              <div
                key={`${result.ordinal}-${result.sourceName}-${result.chunkOrdinal}`}
                className={styles.toolIndicator}
                title={t('chat.knowledgeRetrievalRowTitle', {
                  source: result.sourceName,
                  chunk: result.chunkOrdinal,
                })}
              >
                <span className={styles.knowledgeRetrievalOrdinal}>#{result.ordinal}</span>
                <span className={styles.toolDesc}>{result.firstLine}</span>
              </div>
            ))}
          </div>
        </Collapse>
      )}
    </div>
  );
});

/**
 * KnowledgeRetrievalFold — 知识检索步骤的工具条样式呈现。
 *
 * 数据来自配对 user 消息的 knowledgeRetrieval（Wave 1 冻结契约）：
 * - 收起：单行工具摘要（📚 已搜索 {injectedChunks} 个结果；truncated 加
 *   「超预算分片」小标；unavailableReason 时只报「知识检索不可用」）。
 * - 展开（二次展开结构，2026-08-30）：首屏只渲染前 10 条，超出部分经
 *   「显示更多」二级放出——锚点随注入预算伸缩后大会话模型可注入上百块，
 *   一次性全展开会让列表失去可读性（用户明确要求的二级结构）。
 *   每条一行 `#ordinal firstLine`（编号 + 首行，不是全文），
 *   行 title 悬浮 `{sourceName} · 块 {chunkOrdinal}`。
 * - 滚动注入（2026-08-31）：rollup 携带分批阅读统计与模型自主补充检索的
 *   查询行（过程留痕，历史可见）；distilled 徽标仅为旧会话存量 stats 渲染。
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

/** 二次展开的首屏条数：超出部分经「显示更多」一次性放出。 */
const KNOWLEDGE_RESULTS_PREVIEW_COUNT = 10;

function isRetrievalResult(value: unknown): value is RetrievalResult {
  return !!value && typeof value === 'object'
    && typeof (value as { ordinal?: unknown }).ordinal === 'number'
    && typeof (value as { firstLine?: unknown }).firstLine === 'string';
}

export const KnowledgeRetrievalFold = memo(function KnowledgeRetrievalFold({ retrieval }: Props) {
  const t = window.t ?? ((key: string) => key);
  const [expanded, setExpanded] = useState(false);
  const [showAll, setShowAll] = useState(false);
  const toggle = useCallback(() => setExpanded(value => !value), []);
  const showMore = useCallback(() => setShowAll(true), []);

  const unavailable = !!retrieval.unavailableReason;
  const results = Array.isArray(retrieval.results)
    ? retrieval.results.filter(isRetrievalResult)
    : [];
  const rollup = retrieval.rollup ?? null;
  const supplementalQueries = rollup?.supplementalQueries ?? [];
  const expandable = !unavailable && (results.length > 0 || supplementalQueries.length > 0);
  const summary = unavailable
    ? t('chat.knowledgeRetrievalUnavailable')
    : t('chat.knowledgeRetrievalSearched', { n: retrieval.injectedChunks });
  const preview = showAll ? results : results.slice(0, KNOWLEDGE_RESULTS_PREVIEW_COUNT);
  const hiddenCount = results.length - preview.length;

  return (
    <div className={styles.toolGroup} data-testid="knowledge-retrieval-fold">
      <div
        className={`${styles.toolGroupSummary}${expandable ? ` ${styles.toolGroupSummaryClickable}` : ''}`}
        onClick={expandable ? toggle : undefined}
      >
        <span className={styles.knowledgeRetrievalIcon} aria-hidden="true">📚</span>
        <span className={styles.toolGroupTitle}>{summary}</span>
        {rollup && !unavailable && (
          <span className={styles.knowledgeRetrievalBadge}>
            {t('chat.knowledgeRetrievalRolled', { parts: rollup.parts, rounds: rollup.rounds })}
          </span>
        )}
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
            {supplementalQueries.length > 0 && (
              <div className={styles.toolIndicator} data-testid="knowledge-retrieval-supplement">
                <span className={styles.knowledgeRetrievalOrdinal}>🔍</span>
                <span className={styles.toolDesc}>
                  {t('chat.knowledgeRetrievalSupplement', { n: supplementalQueries.length })}
                </span>
              </div>
            )}
            {supplementalQueries.map((query) => (
              <div
                key={`supplement-${query}`}
                className={styles.toolIndicator}
                title={t('chat.knowledgeRetrievalSupplementTitle')}
              >
                <span className={styles.knowledgeRetrievalOrdinal}>↻</span>
                <span className={styles.toolDesc}>{query}</span>
              </div>
            ))}
            {preview.map((result) => (
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
            {hiddenCount > 0 && (
              <div
                className={styles.toolGroupSummary}
                data-testid="knowledge-retrieval-show-more"
                onClick={showMore}
              >
                <span className={styles.toolGroupTitle}>
                  {t('chat.knowledgeRetrievalShowMore', { n: hiddenCount })}
                </span>
              </div>
            )}
          </div>
        </Collapse>
      )}
    </div>
  );
});

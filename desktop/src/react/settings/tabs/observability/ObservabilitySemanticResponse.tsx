/**
 * ObservabilitySemanticResponse.tsx — Semantic Response 友好视图（Phase 9
 * §六十九～七十一）。
 *
 * 分区展示 Text / Reasoning / ToolCalls / StructuredOutput / Transcription /
 * Media / FinishReason / Completeness + Raw JSON 开关。
 *
 * 纪律：
 *   - reasoning 默认折叠（§七十）；redacted_thinking 原样展示，绝不尝试
 *     恢复/解密/推断原内容。
 *   - tool calls 只展示 name/id/arguments（已脱敏），绝不重新执行（§七十一）。
 *   - 空字段不渲染分区（但 completeness 始终显示）。
 */
import React, { useState } from 'react';
import type { ModelSemanticResponse } from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';
import { JsonValueViewer } from './JsonValueViewer';

function TextBlock({ text, labelKey, defaultCollapsed = false, long = false }: {
  text: string;
  labelKey: string;
  defaultCollapsed?: boolean;
  long?: boolean;
}) {
  const [collapsed, setCollapsed] = useState(defaultCollapsed);
  const [expanded, setExpanded] = useState(false);
  const isLong = long || text.length > 4000;
  const shown = !expanded && isLong ? text.slice(0, 4000) : text;
  return (
    <div className={styles['observability-sr-block']}>
      <button
        type="button"
        className={styles['observability-sr-block-header']}
        aria-expanded={!collapsed}
        onClick={() => setCollapsed((prev) => !prev)}
      >
        <span>{t(labelKey)}</span>
        <span aria-hidden>{collapsed ? '▸' : '▾'}</span>
      </button>
      {!collapsed && (
        <>
          <pre className={styles['observability-sr-text']} data-wrap>
            {shown}
            {!expanded && isLong && '…'}
          </pre>
          {isLong && (
            <button
              type="button"
              className={styles['observability-jsonviewer-toggle']}
              onClick={() => setExpanded((prev) => !prev)}
            >
              {expanded
                ? t('settings.observability.jsonViewer.collapse')
                : t('settings.observability.jsonViewer.showAll', { chars: text.length.toLocaleString() })}
            </button>
          )}
        </>
      )}
    </div>
  );
}

export function ObservabilitySemanticResponse({ response }: { response: ModelSemanticResponse }) {
  const [rawOpen, setRawOpen] = useState(false);
  const reasoning = typeof response.reasoning === 'string' && response.reasoning.length > 0
    ? response.reasoning
    : null;

  return (
    <div className={styles['observability-sr']}>
      <div className={styles['observability-sr-meta']}>
        <span>
          {t('settings.observability.semanticResponse.completeness')}
          {': '}
          {t(`settings.observability.values.completeness.${response.completeness}`)}
        </span>
        {response.finishReason && (
          <span>
            {t('settings.observability.semanticResponse.finishReason')}
            {': '}
            {response.finishReason}
          </span>
        )}
      </div>

      {typeof response.text === 'string' && response.text.length > 0 && (
        <TextBlock text={response.text} labelKey="settings.observability.semanticResponse.text" long={response.text.length > 4000} />
      )}

      {/* §七十：reasoning 默认折叠；redacted_thinking 原样展示（[REDACTED] 等
          占位文本就是内容本身，绝不推断原文） */}
      {reasoning && (
        <TextBlock
          text={reasoning}
          labelKey="settings.observability.semanticResponse.reasoning"
          defaultCollapsed
          long={reasoning.length > 4000}
        />
      )}

      {typeof response.transcription === 'string' && response.transcription.length > 0 && (
        <TextBlock text={response.transcription} labelKey="settings.observability.semanticResponse.transcription" long={response.transcription.length > 4000} />
      )}

      {Array.isArray(response.toolCalls) && response.toolCalls.length > 0 && (
        <div className={styles['observability-sr-block']}>
          <div className={styles['observability-sr-block-header-static']}>
            {t('settings.observability.semanticResponse.toolCalls', { count: response.toolCalls.length })}
          </div>
          {response.toolCalls.map((call, index) => (
            <div key={call.id ?? index} className={styles['observability-sr-toolcall']}>
              <div className={styles['observability-sr-toolcall-head']}>
                <span className={styles['observability-sr-toolcall-name']}>{call.name ?? '—'}</span>
                {call.id && <span className={styles['observability-ledger-muted']}>{call.id}</span>}
              </div>
              {call.arguments !== undefined && (
                <JsonValueViewer
                  value={call.arguments}
                  initialChars={2000}
                  copyAriaLabel={t('settings.observability.copy.capturedPayloadAria')}
                />
              )}
            </div>
          ))}
        </div>
      )}

      {response.structuredOutput !== undefined && response.structuredOutput !== null && (
        <div className={styles['observability-sr-block']}>
          <div className={styles['observability-sr-block-header-static']}>
            {t('settings.observability.semanticResponse.structuredOutput')}
          </div>
          <JsonValueViewer
            value={response.structuredOutput}
            copyAriaLabel={t('settings.observability.copy.capturedPayloadAria')}
          />
        </div>
      )}

      {response.media && (
        <div className={styles['observability-sr-block']}>
          <div className={styles['observability-sr-block-header-static']}>
            {t('settings.observability.semanticResponse.media')}
          </div>
          <div className={styles['observability-sr-media']}>
            {response.media.taskId != null && <span>taskId: {String(response.media.taskId)}</span>}
            {response.media.providerTaskId != null && <span>providerTaskId: {String(response.media.providerTaskId)}</span>}
            {response.media.fileCount != null && <span>files: {String(response.media.fileCount)}</span>}
            {response.media.deferred === true && <span>{t('settings.observability.semanticResponse.mediaDeferred')}</span>}
          </div>
          {response.media.files !== undefined && (
            <JsonValueViewer
              value={response.media.files}
              initialChars={2000}
              copyAriaLabel={t('settings.observability.copy.capturedPayloadAria')}
            />
          )}
        </div>
      )}

      <div className={styles['observability-sr-raw-toggle']}>
        <button
          type="button"
          className={styles['observability-jsonviewer-toggle']}
          aria-expanded={rawOpen}
          onClick={() => setRawOpen((prev) => !prev)}
        >
          {rawOpen
            ? t('settings.observability.semanticResponse.hideRawJson')
            : t('settings.observability.semanticResponse.showRawJson')}
        </button>
      </div>
      {rawOpen && (
        <JsonValueViewer
          value={response}
          copyAriaLabel={t('settings.observability.copy.capturedPayloadAria')}
        />
      )}
    </div>
  );
}

/**
 * JsonValueViewer.tsx — 通用 JSON 查看器（Phase 9 §六十五/§一百五十四）。
 *
 * Renderer 安全红线：payload 是高度不可信文本——本组件只把 JSON 序列化后
 * 当**纯文本**渲染（textContent 语义），绝不 eval / new Function /
 * innerHTML / dangerouslySetInnerHTML / HTML preview / markdown 执行。
 *
 * 功能：pretty 打印、等宽、wrap 开关、复制、长文首段 + 展开（§六十八）。
 */
import React, { useCallback, useMemo, useState } from 'react';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';

const DEFAULT_INITIAL_CHARS = 8000;

/** 防御性 stringify：server 数据理论上是 JSON-safe，异常输入给显式占位而非 throw。 */
export function safeJsonStringify(value: unknown): string {
  try {
    const text = JSON.stringify(value, null, 2);
    return text ?? String(value);
  } catch (error) {
    return t('settings.observability.jsonViewer.stringifyFailed', {
      error: error instanceof Error ? error.message : String(error),
    });
  }
}

export function JsonValueViewer({ value, initialChars = DEFAULT_INITIAL_CHARS, copyAriaLabel }: {
  value: unknown;
  initialChars?: number;
  copyAriaLabel?: string;
}) {
  const full = useMemo(() => safeJsonStringify(value), [value]);
  const [expanded, setExpanded] = useState(false);
  const [wrap, setWrap] = useState(false);
  const [copied, setCopied] = useState(false);

  const truncated = !expanded && full.length > initialChars;
  const shown = truncated ? full.slice(0, initialChars) : full;

  const copy = useCallback(() => {
    void navigator.clipboard?.writeText(full).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  }, [full]);

  return (
    <div className={styles['observability-jsonviewer']}>
      <div className={styles['observability-jsonviewer-toolbar']}>
        <button
          type="button"
          className={styles['observability-jsonviewer-toggle']}
          aria-pressed={wrap}
          onClick={() => setWrap((prev) => !prev)}
        >
          {t('settings.observability.jsonViewer.wrap')}
        </button>
        <button
          type="button"
          className={styles['observability-jsonviewer-toggle']}
          onClick={copy}
          aria-label={copyAriaLabel ?? t('settings.observability.jsonViewer.copyAria')}
        >
          {copied ? t('settings.observability.jsonViewer.copied') : t('settings.observability.jsonViewer.copy')}
        </button>
        {full.length > initialChars && (
          <button
            type="button"
            className={styles['observability-jsonviewer-toggle']}
            onClick={() => setExpanded((prev) => !prev)}
          >
            {truncated
              ? t('settings.observability.jsonViewer.showAll', { chars: full.length.toLocaleString() })
              : t('settings.observability.jsonViewer.collapse')}
          </button>
        )}
      </div>
      <pre className={styles['observability-jsonviewer-body']} data-wrap={wrap || undefined}>
        {shown}
        {truncated && <span className={styles['observability-jsonviewer-ellipsis']}>…</span>}
      </pre>
    </div>
  );
}

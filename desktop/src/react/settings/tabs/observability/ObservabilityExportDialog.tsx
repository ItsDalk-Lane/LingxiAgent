/**
 * ObservabilityExportDialog.tsx — 导出对话框（Phase 9 §一百零九～一百一十八）。
 *
 * 纪律：
 *   - 默认 includePayloads=false（metadata-only：trace/call/attempt/usage/
 *     payload metadata；§一百一十）；勾选是显式 opt-in + 安全提示（§一百一十一）。
 *   - 永远没有 includeRaw / Raw / Unredacted 选项（§一百一十三：系统根本不
 *     存在 raw payload store）。
 *   - 流式保存（§一百一十五，见 observability-export-save.ts）。
 *   - 413 export_limit → 「缩小时间范围或过滤条件」+ 实际计数；绝不自动把
 *     maxCalls 拉到 100000（§一百一十八）。
 *   - 文件名 lingxi-model-observability-YYYYMMDD-HHmm.jsonl（§一百一十七）。
 */
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  MODEL_OBSERVABILITY_EXPORT_MAX_CALLS_LIMIT,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import { Button, Overlay, Toggle } from '../../../ui';
import styles from '../../Settings.module.css';
import {
  isObservabilityErrorKind,
  ModelObservabilityRequestError,
} from './model-observability-actions';
import {
  listActiveFilterChips,
  type ObservabilityFilterState,
} from './model-observability-filter';
import { buildCallFilterInput } from './model-observability-filter';
import {
  observabilityExportCapability,
  observabilityExportFileName,
  runObservabilityExport,
  type ObservabilityExportOutcome,
} from './observability-export-save';
import { observabilityChipLabel } from './model-observability-labels';
import { formatNumber } from './model-observability-format';

type RunState =
  | { phase: 'idle' }
  | { phase: 'running'; bytesWritten: number }
  | { phase: 'done'; outcome: ObservabilityExportOutcome }
  | { phase: 'error'; message: string; matchedCalls?: number | null; maxCalls?: number | null };

export function ObservabilityExportDialog({ open, appliedFilter, onClose }: {
  open: boolean;
  appliedFilter: ObservabilityFilterState;
  onClose: () => void;
}) {
  const [includePayloads, setIncludePayloads] = useState(false);
  const [maxCallsRaw, setMaxCallsRaw] = useState('');
  const [run, setRun] = useState<RunState>({ phase: 'idle' });
  const abortRef = useRef<AbortController | null>(null);
  const capability = useMemo(() => observabilityExportCapability(), []);

  // 打开时重置会话状态（每次导出是独立事务）。
  useEffect(() => {
    if (open) {
      setIncludePayloads(false);
      setMaxCallsRaw('');
      setRun({ phase: 'idle' });
    }
  }, [open]);

  // 卸载/关闭时中止进行中的导出（abort 会删除部分文件，§一百一十六）。
  useEffect(() => () => abortRef.current?.abort(), []);
  useEffect(() => {
    if (!open && abortRef.current) abortRef.current.abort();
  }, [open]);

  const chips = listActiveFilterChips(appliedFilter);

  const maxCalls = useMemo(() => {
    const trimmed = maxCallsRaw.trim();
    if (!trimmed) return null;
    const n = Number(trimmed);
    if (!Number.isInteger(n) || n <= 0 || n > MODEL_OBSERVABILITY_EXPORT_MAX_CALLS_LIMIT) return null;
    return n;
  }, [maxCallsRaw]);
  const maxCallsValid = maxCallsRaw.trim() === '' || maxCalls !== null;

  const start = useCallback(() => {
    if (!capability || run.phase === 'running') return;
    const controller = new AbortController();
    abortRef.current = controller;
    setRun({ phase: 'running', bytesWritten: 0 });
    runObservabilityExport({
      request: {
        query: { filter: buildCallFilterInput(appliedFilter) },
        includePayloads,
        ...(maxCalls !== null ? { maxCalls } : {}),
      },
      capability,
      defaultFileName: observabilityExportFileName(),
      signal: controller.signal,
      onProgress: (bytesWritten) => setRun((prev) => (prev.phase === 'running' ? { phase: 'running', bytesWritten } : prev)),
    }).then((outcome) => {
      abortRef.current = null;
      if (outcome.outcome === 'canceled') {
        setRun({ phase: 'idle' });
        return;
      }
      setRun({ phase: 'done', outcome });
    }).catch((error: unknown) => {
      abortRef.current = null;
      if (isObservabilityErrorKind(error, 'export_limit') || (error instanceof ModelObservabilityRequestError && error.status === 413)) {
        setRun({
          phase: 'error',
          message: error.message,
          matchedCalls: error.matchedCalls,
          maxCalls: error.maxCalls,
        });
        return;
      }
      setRun({ phase: 'error', message: error instanceof Error ? error.message : String(error) });
    });
  }, [capability, run.phase, appliedFilter, includePayloads, maxCalls]);

  const cancel = useCallback(() => {
    abortRef.current?.abort();
    abortRef.current = null;
  }, []);

  return (
    <Overlay
      open={open}
      scope="inline"
      onClose={() => {
        cancel();
        onClose();
      }}
      closeOnEsc
      closeOnBackdrop={run.phase !== 'running'}
      trapFocus
      contentProps={{ role: 'dialog', 'aria-label': t('settings.observability.export.dialogAria') }}
    >
      <div className={styles['observability-export-dialog']}>
        <h3 className={styles['observability-panel-title']}>
          {t('settings.observability.export.title')}
        </h3>

        {/* 当前 filter 回显（§一百零九：导出内容 = 当前查询） */}
        <div className={styles['observability-export-filter']}>
          <div className={styles['observability-advanced-title']}>
            {t('settings.observability.export.currentFilter')}
          </div>
          <div className={styles['observability-export-filter-body']}>
            {chips.length === 0
              ? t('settings.observability.export.filterDefault')
              : chips.map((chip) => observabilityChipLabel(chip)).join(' · ')}
          </div>
        </div>

        <label className={styles['observability-settings-row']}>
          <span>
            {t('settings.observability.export.includePayloads')}
            <span className={styles['observability-settings-hint']}>
              {t('settings.observability.export.includePayloadsHint')}
            </span>
          </span>
          <Toggle
            on={includePayloads}
            disabled={run.phase === 'running'}
            onChange={setIncludePayloads}
            ariaLabel={t('settings.observability.export.includePayloads')}
          />
        </label>

        <div className={styles['observability-export-maxcalls']}>
          <div className={styles['observability-settings-row']}>
            <label
              className={styles['observability-advanced-label']}
              htmlFor="observability-export-maxcalls-input"
            >
              {t('settings.observability.export.maxCalls')}
            </label>
            <input
              id="observability-export-maxcalls-input"
              className={styles['settings-input']}
              type="number"
              min={1}
              max={MODEL_OBSERVABILITY_EXPORT_MAX_CALLS_LIMIT}
              placeholder={t('settings.observability.export.maxCallsPlaceholder')}
              value={maxCallsRaw}
              disabled={run.phase === 'running'}
              data-invalid={!maxCallsValid || undefined}
              onChange={(event) => setMaxCallsRaw(event.target.value)}
            />
          </div>
          {!maxCallsValid && (
            <span className={styles['observability-settings-invalid']} role="alert">
              {t('settings.observability.export.maxCallsInvalid', { max: formatNumber(MODEL_OBSERVABILITY_EXPORT_MAX_CALLS_LIMIT) })}
            </span>
          )}
        </div>

        {!capability && (
          <div className={styles['observability-provenance-note']}>
            {t('settings.observability.export.noCapability')}
          </div>
        )}

        {run.phase === 'running' && (
          <div className={styles['observability-export-progress']} role="status">
            {t('settings.observability.export.progress', { bytes: formatNumber(run.bytesWritten) })}
          </div>
        )}
        {run.phase === 'done' && run.outcome.outcome === 'saved' && (
          <div className={styles['observability-export-done']} role="status">
            {run.outcome.filePath
              ? t('settings.observability.export.savedTo', { path: run.outcome.filePath })
              : t('settings.observability.export.saved', { bytes: formatNumber(run.outcome.bytesWritten) })}
          </div>
        )}
        {run.phase === 'done' && run.outcome.outcome === 'aborted' && (
          <div className={styles['observability-provenance-note']}>
            {run.outcome.partialLeft
              ? t('settings.observability.export.abortedPartialLeft')
              : t('settings.observability.export.aborted')}
          </div>
        )}
        {run.phase === 'error' && (
          <div className={styles['observability-error-detail']} role="alert">
            {run.matchedCalls != null && run.maxCalls != null
              ? t('settings.observability.export.limitError', {
                matched: formatNumber(run.matchedCalls),
                max: formatNumber(run.maxCalls),
              })
              : run.message}
          </div>
        )}

        <div className={styles['observability-advanced-footer']}>
          {run.phase === 'running' ? (
            <Button variant="secondary" size="sm" onClick={cancel}>
              {t('settings.observability.actions.cancel')}
            </Button>
          ) : (
            <Button variant="secondary" size="sm" onClick={onClose}>
              {t('settings.observability.actions.close')}
            </Button>
          )}
          <Button
            variant="primary"
            size="sm"
            disabled={!capability || !maxCallsValid || run.phase === 'running'}
            onClick={start}
          >
            {t('settings.observability.export.start')}
          </Button>
        </div>
      </div>
    </Overlay>
  );
}

/**
 * ObservabilitySettingsPanel.tsx — 「设置」子标签页（2026-09-17 用户定稿，
 * 替代原「记录设置」弹窗）。
 *
 * 布局：记录状态摘要 → 保留天数（单一输入框：轨迹/正文/媒体共用同一配置，
 * 保存在行尾）→ 存储概况（已保存天数 / 数据起始日 / 占用体积 / 覆盖调用，
 * 三类体积拆分 + 存放路径 + 复制）→ 删除数据（范围 全部/按天 × 类别
 * 轨迹/正文/媒体多选，实时估算释放体积，确认弹层后执行）。
 *
 * 契约：删除与体积都是 LOCAL_ONLY 事实；删除不可恢复，执行前必须过确认层。
 */
import React, { useEffect, useMemo, useRef, useState } from 'react';
import type {
  ModelObservabilityHealthResponse,
  ModelObservabilitySettingsResponse,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import { Button } from '../../../ui';
import styles from '../../Settings.module.css';
import {
  deleteObservabilityData,
  isObservabilityAbortError,
  loadObservabilityStorage,
  updateObservabilitySettings,
  type ObservabilityDeleteInput,
  type ObservabilityStorageDay,
  type ObservabilityStorageOverview,
} from './model-observability-actions';
import { formatAxisDate, formatCompactNumber, formatNumber } from './model-observability-format';
import { queryStatusLabel, recordingStatusLabel } from './model-observability-labels';

const MIN_RETENTION_DAYS = 1;
const MAX_RETENTION_DAYS = 3650;

const DAY_MS = 86_400_000;
type DeleteCategory = 'trace' | 'payload' | 'media';

export function ObservabilitySettingsPanel({ health, settings, refreshToken, onSaved }: {
  health: ModelObservabilityHealthResponse | null;
  settings: ModelObservabilitySettingsResponse | null;
  refreshToken: number;
  /** 保存/删除后由面板内部触发；onSaved 供外壳叠加控制面重载。 */
  onSaved?: () => void;
}) {
  const [storage, setStorage] = useState<ObservabilityStorageOverview | null>(null);
  const [storageError, setStorageError] = useState<string | null>(null);
  const [retentionInput, setRetentionInput] = useState<string>('');
  const [savedFlash, setSavedFlash] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [pathCopied, setPathCopied] = useState(false);
  const [scope, setScope] = useState<'all' | 'dates'>('all');
  const [deleteWindows, setDeleteWindows] = useState<Array<{ from: string; to: string }>>([]);
  const [categories, setCategories] = useState<Record<DeleteCategory, boolean>>({
    trace: true, payload: true, media: true,
  });
  const generationRef = useRef(0);

  useEffect(() => {
    const generation = ++generationRef.current;
    const controller = new AbortController();
    loadObservabilityStorage({ signal: controller.signal }).then((overview) => {
      if (generationRef.current !== generation) return;
      setStorage(overview);
      setStorageError(null);
    }).catch((error: unknown) => {
      if (generationRef.current !== generation || isObservabilityAbortError(error)) return;
      setStorageError(error instanceof Error ? error.message : String(error));
    });
    return () => controller.abort();
  }, [refreshToken]);

  // 保留天数初始值取自 desired 配置（三类已统一；历史数据不一致时取最大值）。
  useEffect(() => {
    const retention = settings?.desired?.retention;
    if (!retention) return;
    const days = Math.max(retention.traceDays ?? 0, retention.payloadDays ?? 0, retention.blobDays ?? 0);
    if (days > 0) setRetentionInput((current) => current || String(days));
  }, [settings]);

  const copyPath = () => {
    navigator.clipboard?.writeText('~/.lingxi/model-observability/').then(() => {
      setPathCopied(true);
      setTimeout(() => setPathCopied(false), 2000);
    }).catch(() => setPathCopied(false));
  };

  const retention = settings?.desired?.retention ?? null;

  const saveRetention = async () => {
    const days = Math.max(MIN_RETENTION_DAYS, Math.min(MAX_RETENTION_DAYS, Math.floor(Number(retentionInput) || 0)));
    if (!Number.isFinite(days) || days < MIN_RETENTION_DAYS) return;
    const { updateObservabilitySettings } = await import('./model-observability-actions');
    try {
      await updateObservabilitySettings({ retention: { traceDays: days, payloadDays: days, blobDays: days } });
      setRetentionInput(String(days));
      setSavedFlash(true);
      setTimeout(() => setSavedFlash(false), 2000);
      onSaved?.();
    } catch { /* 保存失败静默：与原弹窗同口径（overlay 显示错误交由后续迭代） */ }
  };

  const selectedCategories = (['trace', 'payload', 'media'] as const).filter((key) => categories[key]);
  const dayOptions = storage?.perDay ?? [];
  const coveredKeys = useMemo(() => expandWindowKeys(deleteWindows), [deleteWindows]);

  const estimateMb = useMemo(() => {
    if (scope === 'all' || !storage) {
      return storage ? totalMb(storage) : 0;
    }
    let mb = storage.sizes.databaseBytes / 1024 / 1024
      * (Math.min(1, coveredKeys.size / Math.max(1, storage.days)));
    const mediaBytes = storage.perDay
      .filter((day) => coveredKeys.has(day.date))
      .reduce((sum, day) => sum + day.mediaBytes, 0);
    const payloadChars = storage.perDay
      .filter((day) => coveredKeys.has(day.date))
      .reduce((sum, day) => sum + day.payloadChars, 0);
    mb += mediaBytes / 1024 / 1024 + payloadChars * 2 / 1024 / 1024;
    return mb;
  }, [scope, categories, storage, coveredKeys]);

  const scopeText = scope === 'dates' && deleteWindows.length > 0
    ? t('settings.observability.storage.selectedDays', { count: formatNumber(coveredKeys.size) })
    : t('settings.observability.storage.allScope', { days: formatNumber(storage?.days ?? 0) });
  const categoryNames: Record<DeleteCategory, string> = {
    trace: t('settings.observability.storage.categoryTrace'),
    payload: t('settings.observability.storage.categoryPayload'),
    media: t('settings.observability.storage.categoryMedia'),
  };

  const executeDelete = async () => {
    setConfirmOpen(false);
    setDeleting(true);
    try {
      const input: ObservabilityDeleteInput = {
        categories: selectedCategories,
        windows: scope === 'dates' ? deleteWindows : [],
      };
      await deleteObservabilityData(input);
      onSaved?.();
    } catch { /* 删除失败静默：与面板其他取数同口径 */ } finally {
      setDeleting(false);
    }
  };

  const categoryMb = (key: DeleteCategory): string => {
    if (!storage) return '—';
    if (key === 'media') return formatMb(storage.sizes.mediaBytes);
    if (key === 'payload') return formatMb(storage.sizes.payloadEstimateChars * 2);
    return formatMb(storage.sizes.databaseBytes);
  };

  return (
    <div className={styles['observability-sub-panel']}>
      {/* 记录状态摘要 */}
      <section className={styles['observability-panel']}>
        <div className={styles['observability-status-card']}>
          {health && (
            <>
              <div>
                {t('settings.observability.recording.effectiveStatus')}: <b>{recordingStatusLabel(health.recordingStatus)}</b>
              </div>
              <div className={styles['observability-status-dim']}>
                {t('settings.observability.recording.queryStatus')}: {queryStatusLabel(health.query.queryStatus)}
                {health.query.schemaVersion !== null ? ` · schema v${health.query.schemaVersion}` : ''}
              </div>
              <div className={styles['observability-status-dim']}>{t('settings.observability.recording.persistPayloadsHint')}</div>
              <div className={styles['observability-status-dim']}>
                {t('settings.observability.recording.atRestEncryption')}: {health.atRestEncryption
                  ? t('settings.observability.tri.yes')
                  : t('settings.observability.recording.atRestEncryptionNo')}
                {health.lastSuccessfulFlushAt ? ` · ${t('settings.observability.recording.lastFlush')}: ${formatLocalStamp(health.lastSuccessfulFlushAt)}` : ''}
              </div>
            </>
          )}
        </div>
      </section>

      {/* 保留天数：单一输入框，保存在行尾 */}
      <section className={styles['observability-panel']}>
        <h3 className={styles['observability-chart-title']}>{t('settings.observability.storage.retentionTitle')}</h3>
        <div className={styles['observability-chart-hint']}>{t('settings.observability.storage.retentionHint')}</div>
        <div className={styles['observability-retention-row']}>
          <label htmlFor="observability-retention-input">{t('settings.observability.storage.retentionLabel')}</label>
          <input
            id="observability-retention-input"
            type="number"
            min="1"
            max="3650"
            value={retentionInput}
            onChange={(event) => setRetentionInput(event.target.value)}
          />
          <span className={styles['observability-retention-unit']}>{t('settings.observability.storage.retentionUnit')}</span>
          <Button variant="primary" size="sm" onClick={saveRetention} disabled={!retentionInput}>
            {t('settings.observability.storage.save')}
          </Button>
          {savedFlash && <span className={styles['observability-retention-saved']}>{t('settings.observability.storage.saved')}</span>}
        </div>
      </section>

      {/* 存储概况 */}
      <section className={styles['observability-panel']}>
        <h3 className={styles['observability-chart-title']}>{t('settings.observability.storage.overviewTitle')}</h3>
        <div className={styles['observability-chart-hint']}>{t('settings.observability.storage.overviewHint')}</div>
        {storageError ? (
          <div className={styles['observability-chart-notice']} data-kind="error">{storageError}</div>
        ) : !storage ? (
          <div className={styles['observability-chart-notice']} aria-busy="true">{t('settings.observability.charts.loading')}</div>
        ) : (
          <>
            <div className={styles['observability-storage-overview']}>
              <StorageCard label={t('settings.observability.storage.savedDays')} value={`${formatNumber(storage.days)}`} unit={t('settings.observability.storage.daysUnit')} />
              <StorageCard label={t('settings.observability.storage.startDate')} value={storage.oldestAt ? formatAxisDate(storage.oldestAt.slice(0, 10)) : '—'} />
              <StorageCard label={t('settings.observability.storage.footprint')} value={formatMb(totalMb(storage))} unit="" />
              <StorageCard label={t('settings.observability.storage.coveredCalls')} value={formatCompactNumber(storage.calls)} />
            </div>
            <div className={styles['observability-storage-breakdown']}>
              <span>{categoryNames.trace}: <b>{categoryMb('trace')}</b></span>
              <span>{categoryNames.payload}: <b>{categoryMb('payload')}</b></span>
              <span>{categoryNames.media}: <b>{categoryMb('media')}</b></span>
            </div>
            <div className={styles['observability-storage-path']}>
              <span>{t('settings.observability.storage.pathLabel')}</span>
              <code>~/.lingxi/model-observability/</code>
              <Button variant="secondary" size="sm" onClick={copyPath}>
                {pathCopied ? t('settings.observability.storage.copied') : t('settings.observability.storage.copyPath')}
              </Button>
            </div>
          </>
        )}
      </section>

      {/* 删除数据 */}
      <section className={styles['observability-panel']} data-danger="true">
        <h3 className={styles['observability-chart-title']}>{t('settings.observability.storage.deleteTitle')}</h3>
        <div className={styles['observability-chart-hint']}>{t('settings.observability.storage.deleteHint')}</div>
        <div className={styles['observability-delete-grid']}>
          <div>
            <span className={styles['observability-delete-field-label']}>{t('settings.observability.storage.scopeLabel')}</span>
            <label className={styles['observability-delete-option']}>
              <input type="radio" name="obs-del-scope" checked={scope === 'all'} onChange={() => setScope('all')} />
              <span>{t('settings.observability.storage.allScope', { days: formatNumber(storage?.days ?? 0) })}</span>
            </label>
            <label className={styles['observability-delete-option']}>
              <input type="radio" name="obs-del-scope" checked={scope === 'dates'} onChange={() => setScope('dates')} />
              <span>{t('settings.observability.storage.dayScopeOption')}</span>
            </label>
            {scope === 'dates' && (
              <div className={styles['observability-delete-day-row']}>
                <ObservabilityDayPicker
                  windows={deleteWindows}
                  perDay={dayOptions}
                  onChange={setDeleteWindows}
                />
              </div>
            )}
          </div>
          <div>
            <span className={styles['observability-delete-field-label']}>{t('settings.observability.storage.categoryLabel')}</span>
            {(['trace', 'payload', 'media'] as const).map((key) => (
              <label key={key} className={styles['observability-delete-option']}>
                <input
                  type="checkbox"
                  checked={categories[key]}
                  onChange={(event) => setCategories((prev) => ({ ...prev, [key]: event.target.checked }))}
                />
                <span>{categoryNames[key]}</span>
                <span className={styles['observability-delete-option-size']}>{categoryMb(key)}</span>
              </label>
            ))}
          </div>
        </div>
        <div className={styles['observability-delete-estimate']}>
          {selectedCategories.length === 0
            ? t('settings.observability.storage.pickCategory')
            : (
              <>
                {t('settings.observability.storage.deleteWillRemove', { scope: scopeText, categories: selectedCategories.map((key) => categoryNames[key]).join('、') })}
                <br />
                {t('settings.observability.storage.estimate', { size: formatMb(estimateMb) })}
              </>
            )}
        </div>
        <div className={styles['observability-delete-actions']}>
          <Button
            variant="primary"
            size="sm"
            disabled={selectedCategories.length === 0 || deleting || (scope === 'dates' && deleteWindows.length === 0)}
            onClick={() => setConfirmOpen(true)}
          >
            {t('settings.observability.storage.deleteButton')}
          </Button>
        </div>
        {confirmOpen && (
          <div className={styles['observability-delete-mask']} role="dialog" aria-modal="true">
            <div className={styles['observability-delete-confirm']}>
              <h4>{t('settings.observability.storage.confirmTitle')}</h4>
              <div className={styles['observability-delete-confirm-body']}>
                {t('settings.observability.storage.deleteWillRemove', { scope: scopeText, categories: selectedCategories.map((key) => categoryNames[key]).join('、') })}
                <br />
                {t('settings.observability.storage.estimate', { size: formatMb(estimateMb) })}
                <br />
                <b>{t('settings.observability.storage.irreversible')}</b>
              </div>
              <div className={styles['observability-delete-actions']}>
                <Button variant="secondary" size="sm" onClick={() => setConfirmOpen(false)}>
                  {t('settings.observability.storage.cancel')}
                </Button>
                <Button variant="primary" size="sm" onClick={executeDelete} disabled={deleting}>
                  {t('settings.observability.storage.confirmDelete')}
                </Button>
              </div>
            </div>
          </div>
        )}
        {retention && retention.traceDays !== retention.payloadDays && (
          <div className={styles['observability-chart-hint']}>{t('settings.observability.storage.retentionUnifyHint')}</div>
        )}
      </section>
    </div>
  );
}

/**
 * 日历式日期范围选择（按日期删除）：弹层小日历 + 三种选法——
 *   按天：单选——点某天只选这天（再点取消）；不能多选、不能拖拽。
 *   整月：按月格子显示（不显示每天），点一个月选中整月，再点取消。
 *   区间：只用左键——未选中处按下拖动 = 整段加选；已选中处按下拖动 = 整段取消。
 * 头部 ‹ › 翻月 + 月份/年份下拉快速定位。没有数据的日子禁用。
 */
type PickerMode = 'day' | 'month' | 'range';

function expandWindowKeys(windows: Array<{ from: string; to: string }>): Set<string> {
  const keys = new Set<string>();
  for (const window of windows) {
    const start = new Date(`${window.from}T00:00:00`);
    const end = new Date(`${window.to}T00:00:00`);
    for (let cursor = new Date(start); cursor <= end && keys.size < 5000; cursor.setDate(cursor.getDate() + 1)) {
      const pad = (n: number) => String(n).padStart(2, '0');
      keys.add(`${cursor.getFullYear()}-${pad(cursor.getMonth() + 1)}-${pad(cursor.getDate())}`);
    }
  }
  return keys;
}

function shiftDay(dateKey: string, days: number): string {
  const d = new Date(`${dateKey}T00:00:00`);
  d.setDate(d.getDate() + days);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** 合并重叠/相邻窗口并排序。 */
function mergeWindowsList(windows: Array<{ from: string; to: string }>) {
  const sorted = [...windows].sort((a, b) => (a.from < b.from ? -1 : a.from > b.from ? 1 : 0));
  const out: Array<{ from: string; to: string }> = [];
  for (const w of sorted) {
    const last = out[out.length - 1];
    if (last && w.from <= shiftDay(last.to, 1)) {
      if (w.to > last.to) last.to = w.to;
    } else {
      out.push({ ...w });
    }
  }
  return out;
}

/** 从窗口集合中抠掉一段日期。 */
function subtractSpan(
  windows: Array<{ from: string; to: string }>,
  spanFrom: string,
  spanTo: string,
) {
  const out: Array<{ from: string; to: string }> = [];
  for (const w of windows) {
    if (w.to < spanFrom || w.from > spanTo) {
      out.push({ ...w });
      continue;
    }
    if (w.from < spanFrom) out.push({ from: w.from, to: shiftDay(spanFrom, -1) });
    if (w.to > spanTo) out.push({ from: shiftDay(spanTo, 1), to: w.to });
  }
  return out;
}

function ObservabilityDayPicker({ windows, perDay, onChange }: {
  windows: Array<{ from: string; to: string }>;
  perDay: ObservabilityStorageDay[];
  onChange: (next: Array<{ from: string; to: string }>) => void;
}) {
  const [mode, setMode] = useState<PickerMode>('day');
  const dragRef = useRef<null | { type: 'add' | 'remove'; anchor: string }>(null);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLSpanElement | null>(null);
  const withData = useMemo(() => new Set(perDay.map((day) => day.date)), [perDay]);
  const selectedKeys = useMemo(() => expandWindowKeys(windows), [windows]);
  const [view, setView] = useState(() => {
    const base = new Date();
    return { year: base.getFullYear(), month: base.getMonth() };
  });

  useEffect(() => {
    if (!open) return;
    const onDown = (event: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    const onUp = () => { dragRef.current = null; };
    document.addEventListener('mousedown', onDown, true);
    document.addEventListener('keydown', onKey);
    window.addEventListener('mouseup', onUp);
    return () => {
      document.removeEventListener('mousedown', onDown, true);
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('mouseup', onUp);
    };
  }, [open]);

  const locale = (window as unknown as { i18n?: { locale?: string } }).i18n?.locale || undefined;
  const first = new Date(view.year, view.month, 1);
  const monthLabel = new Intl.DateTimeFormat(locale, { month: 'long' }).format(first);
  const monthOptions = Array.from({ length: 12 }, (_, month) => ({
    month,
    label: new Intl.DateTimeFormat(locale, { month: 'short' }).format(new Date(view.year, month, 1)),
  }));
  const years = useMemo(() => {
    const years = new Set<number>(perDay.map((day) => Number(day.date.slice(0, 4))));
    years.add(new Date().getFullYear());
    return [...years].sort((a, b) => a - b);
  }, [perDay]);
  const weekdayLabels = useMemo(() => {
    const formatter = new Intl.DateTimeFormat(locale, { weekday: 'narrow' });
    // 2023-01-02 是周一：固定基准周生成 一…日 的表头
    return Array.from({ length: 7 }, (_, index) => formatter.format(new Date(2023, 0, 2 + index)));
  }, [locale]);
  const monthFormatter = new Intl.DateTimeFormat(locale, { month: 'long' });

  /** 按当前拖拽/点击语义套用一段日期窗口（加选合并、减选抠除）。 */
  const applySpan = (type: 'add' | 'remove', a: string, b: string) => {
    const [from, to] = a <= b ? [a, b] : [b, a];
    onChange(type === 'add'
      ? mergeWindowsList([...windows, { from, to }])
      : subtractSpan(windows, from, to));
  };

  const offset = (first.getDay() + 6) % 7; // 周一为行首
  const daysInMonth = new Date(view.year, view.month + 1, 0).getDate();
  const cells: (Date | null)[] = [
    ...Array.from({ length: offset }, () => null),
    ...Array.from({ length: daysInMonth }, (_, index) => new Date(view.year, view.month, index + 1)),
  ];
  const padded = (value2: number) => String(value2).padStart(2, '0');
  const keyOf = (date: Date) => `${date.getFullYear()}-${padded(date.getMonth() + 1)}-${padded(date.getDate())}`;

  const toggleDay = (key: string) => {
    const exists = windows.some((w) => w.from === key && w.to === key);
    onChange(exists
      ? windows.filter((w) => !(w.from === key && w.to === key))
      : [...windows, { from: key, to: key }]);
  };
  const toggleMonth = (key: string) => {
    const from = `${key.slice(0, 8)}01`;
    const lastDay = new Date(Number(key.slice(0, 4)), Number(key.slice(5, 7)), 0).getDate();
    const to = `${key.slice(0, 8)}${padded(lastDay)}`;
    const exists = windows.some((w) => w.from === from && w.to === to);
    onChange(exists
      ? windows.filter((w) => !(w.from === from && w.to === to))
      : [...windows, { from, to }]);
  };
  // 按天：纯单选——点某天 = 只选这天（再点同一天 = 取消），无拖拽。
  // 区间：左键按下即选该天为锚点，按住拖动 = 锚点到当前整段加选；
  //       右键按下/按住拖动 = 整段减选。松开鼠标即定。
  const onDayMouseDown = (evt: React.MouseEvent, key: string) => {
    if (evt.button !== 0) return;
    evt.preventDefault();
    if (mode === 'day') {
      const selected = selectedKeys.has(key);
      onChange(selected ? [] : [{ from: key, to: key }]);
      return;
    }
    const type: 'add' | 'remove' = selectedKeys.has(key) ? 'remove' : 'add';
    dragRef.current = { type, anchor: key };
    applySpan(type, key, key);
  };
  const onDayMouseEnter = (key: string) => {
    const drag = dragRef.current;
    if (!drag) return;
    if (drag.type === 'add') applySpan('add', drag.anchor, key);
    else applySpan('remove', drag.anchor, key);
  };

  const selectedCount = selectedKeys.size;
  const triggerLabel = windows.length === 0
    ? t('settings.observability.storage.pickDate')
    : t('settings.observability.storage.selectedDays', { count: formatNumber(selectedCount) });

  return (
    <span className={styles['observability-daypicker']} ref={rootRef}>
      <button
        type="button"
        className={styles['observability-daypicker-trigger']}
        onClick={() => setOpen((prev) => !prev)}
      >
        {triggerLabel}
      </button>
      {open && (
        <div className={styles['observability-daypicker-pop']}>
          <div className={styles['observability-daypicker-modes']} role="group">
            {(['day', 'month', 'range'] as const).map((item) => (
              <button
                key={item}
                type="button"
                data-active={mode === item ? 'true' : undefined}
                disabled={windows.length > 0 && mode !== item}
                title={windows.length > 0 && mode !== item
                  ? t('settings.observability.storage.clearFirstHint')
                  : undefined}
                onClick={() => { setMode(item); dragRef.current = null; onChange([]); }}
              >
                {t(`settings.observability.storage.${item === 'day' ? 'modeDay' : item === 'month' ? 'modeMonth' : 'modeRange'}`)}
              </button>
            ))}
          </div>
          <div className={styles['observability-daypicker-nav']}>
            {mode === 'month' ? (
              <>
                <button type="button" aria-label="previous-year" onClick={() => setView((prev) => ({ ...prev, year: prev.year - 1 }))}>‹</button>
                <select
                  value={view.year}
                  onChange={(event) => setView((prev) => ({ ...prev, year: Number(event.target.value) }))}
                >
                  {years.map((year) => (
                    <option key={year} value={year}>{year}</option>
                  ))}
                </select>
                <button type="button" aria-label="next-year" onClick={() => setView((prev) => ({ ...prev, year: prev.year + 1 }))}>›</button>
              </>
            ) : (
              <>
                <button type="button" aria-label="previous-month" onClick={() => setView((prev) => prev.month === 0
                  ? { year: prev.year - 1, month: 11 }
                  : { ...prev, month: prev.month - 1 })}
                >
                  ‹
                </button>
                <select
                  value={view.month}
                  onChange={(event) => setView((prev) => ({ ...prev, month: Number(event.target.value) }))}
                >
                  {monthOptions.map((item) => (
                    <option key={item.month} value={item.month}>{item.label}</option>
                  ))}
                </select>
                <select
                  value={view.year}
                  onChange={(event) => setView((prev) => ({ ...prev, year: Number(event.target.value) }))}
                >
                  {years.map((year) => (
                    <option key={year} value={year}>{year}</option>
                  ))}
                </select>
                <button type="button" aria-label="next-month" onClick={() => setView((prev) => prev.month === 11
                  ? { year: prev.year + 1, month: 0 }
                  : { ...prev, month: prev.month + 1 })}
                >
                  ›
                </button>
              </>
            )}
          </div>
          {mode === 'month' ? (
            <div className={styles['observability-daypicker-months']}>
              {Array.from({ length: 12 }, (_, month) => {
                const ym = `${view.year}-${padded(month + 1)}`;
                const from = `${ym}-01`;
                const lastDay = new Date(view.year, month + 1, 0).getDate();
                const to = `${ym}-${padded(lastDay)}`;
                // 选中 = 该月被选区完整覆盖（相邻月合并成一个窗口后依然成立）；
                // 取消 = 从选区里抠除该月（自动把合并窗口切回两段）。
                const covered = windows.some((w) => w.from <= from && w.to >= to);
                return (
                  <button
                    key={ym}
                    type="button"
                    data-selected={covered ? 'true' : undefined}
                    className={styles['observability-daypicker-month']}
                    onClick={() => onChange(covered
                      ? subtractSpan(windows, from, to)
                      : mergeWindowsList([...windows, { from, to }]))}
                  >
                    {monthFormatter.format(new Date(view.year, month, 1))}
                  </button>
                );
              })}
            </div>
          ) : (
            <div
              className={styles['observability-daypicker-grid']}
              onContextMenu={(event) => event.preventDefault()}
            >
              {weekdayLabels.map((label) => (
                <span key={label} className={styles['observability-daypicker-weekday']}>{label}</span>
              ))}
              {cells.map((date, index) => {
                if (!date) return <span key={`blank-${index}`} />;
                const key = keyOf(date);
                const selected = selectedKeys.has(key);
                return (
                  <button
                    key={key}
                    type="button"
                    data-selected={selected ? 'true' : undefined}
                    className={styles['observability-daypicker-day']}
                    title={selected
                      ? t('settings.observability.storage.clickToRemove')
                      : t('settings.observability.storage.clickToAdd')}
                    onMouseDown={(evt) => onDayMouseDown(evt, key)}
                    onMouseEnter={() => {
                      const drag = dragRef.current;
                      if (drag) applySpan(drag.type, drag.anchor, key);
                    }}
                  >
                    {date.getDate()}
                  </button>
                );
              })}
            </div>
          )}
        </div>
      )}
    </span>
  );
}

function StorageCard({ label, value, unit }: { label: string; value: string; unit?: string }) {
  return (
    <div className={styles['observability-storage-card']}>
      <div className={styles['observability-storage-label']}>{label}</div>
      <div className={styles['observability-storage-value']}>
        {value}
        {unit ? <small>{unit}</small> : null}
      </div>
    </div>
  );
}

function formatMb(bytes: number): string {
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024) return `${(mb / 1024).toFixed(2)} GB`;
  return `${Math.max(1, Math.round(mb))} MB`;
}
function totalMb(storage: ObservabilityStorageOverview): number {
  return storage.sizes.databaseBytes / (1024 * 1024)
    + storage.sizes.mediaBytes / (1024 * 1024)
    + storage.sizes.payloadEstimateChars * 2 / (1024 * 1024);
}
function formatLocalStamp(value: string): string {
  const time = Date.parse(value);
  return Number.isFinite(time) ? new Intl.DateTimeFormat(undefined, {
    year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit',
  }).format(new Date(time)) : value;
}

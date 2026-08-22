/**
 * ObservabilityFilterBar.tsx — 统一 Filter Bar（Phase 9 §十七～二十七）。
 *
 * 一行：日期 / Provider / Model / Category / Status / 更多过滤 / Group By /
 * 刷新 / 导出 / 录制设置；下面一行是活动 filter chips（单独可删 + Clear All）。
 * 过滤器不是页面模式——metrics/groups/ledger 同屏共享 appliedFilter（§十四）。
 */
import React, { useCallback } from 'react';
import {
  MODEL_OBSERVABILITY_GROUP_BY_DIMENSIONS,
  MODEL_OBSERVABILITY_GROUP_BY_MAX_DIMENSIONS,
  MODEL_OBSERVABILITY_TERMINAL_STATUSES,
  type ModelObservabilityGroupByDimension,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import { Button, Tooltip } from '../../../ui';
import styles from '../../Settings.module.css';
import type { ObservabilityQueryStateApi } from './use-observability-query-state';
import {
  buildCallFilterInput,
  listActiveFilterChips,
  OBSERVABILITY_DATE_PRESETS,
  type ObservabilityDatePreset,
} from './model-observability-filter';
import { useObservabilityFacetOptions } from './use-observability-facets';
import { ObservabilityMultiSelect } from './ObservabilityMultiSelect';
import { ObservabilityFilterPopover } from './ObservabilityFilterPopover';
import { ObservabilityAdvancedFilters } from './ObservabilityAdvancedFilters';
import {
  groupByDimensionLabel,
  observabilityChipLabel,
  terminalStatusLabel,
} from './model-observability-labels';

/* ── 日期 popover ─────────────────────────────────────────────────────── */

function DateFilter({ state }: { state: ObservabilityQueryStateApi }) {
  const { appliedFilter, drafts, patchFilter, setDrafts, applyTextDrafts } = state;

  const pickPreset = useCallback((preset: ObservabilityDatePreset, close: () => void) => {
    patchFilter({ datePreset: preset });
    if (preset !== 'custom') {
      // 非 custom preset 立即生效并清掉 custom draft（§十四 draft 不残留）。
      patchFilter({ customSince: '', customUntil: '' });
      setDrafts({ customSince: '', customUntil: '' });
      close();
    }
  }, [patchFilter, setDrafts]);

  return (
    <ObservabilityFilterPopover
      label={t(`settings.observability.datePreset.${appliedFilter.datePreset}`)}
    >
      {(close) => (
        <div className={styles['observability-date-panel']}>
          {OBSERVABILITY_DATE_PRESETS.map((preset) => (
            <label key={preset} className={styles['observability-date-preset']}>
              <input
                type="radio"
                name="observability-date-preset"
                checked={appliedFilter.datePreset === preset}
                onChange={() => pickPreset(preset, close)}
              />
              <span>{t(`settings.observability.datePreset.${preset}`)}</span>
            </label>
          ))}
          {appliedFilter.datePreset === 'custom' && (
            <div className={styles['observability-date-custom']}>
              <label className={styles['observability-advanced-label']}>
                {t('settings.observability.customRange.since')}
              </label>
              <input
                type="datetime-local"
                className={styles['settings-input']}
                value={drafts.customSince}
                onChange={(event) => setDrafts({ customSince: event.target.value })}
              />
              <label className={styles['observability-advanced-label']}>
                {t('settings.observability.customRange.untilExclusive')}
              </label>
              <input
                type="datetime-local"
                className={styles['settings-input']}
                value={drafts.customUntil}
                onChange={(event) => setDrafts({ customUntil: event.target.value })}
              />
              <Button
                variant="primary"
                size="sm"
                onClick={() => {
                  applyTextDrafts();
                  close();
                }}
              >
                {t('settings.observability.advanced.apply')}
              </Button>
            </div>
          )}
        </div>
      )}
    </ObservabilityFilterPopover>
  );
}

/* ── Group By popover（§二十七：14 维全支持，主 + 可选次，上限 3）──────── */

function GroupByFilter({ state }: { state: ObservabilityQueryStateApi }) {
  const { groupBy, setGroupBy } = state;

  const toggle = useCallback((dimension: ModelObservabilityGroupByDimension) => {
    if (groupBy.includes(dimension)) {
      setGroupBy(groupBy.filter((item) => item !== dimension));
      return;
    }
    if (groupBy.length >= MODEL_OBSERVABILITY_GROUP_BY_MAX_DIMENSIONS) return;
    setGroupBy([...groupBy, dimension]);
  }, [groupBy, setGroupBy]);

  return (
    <ObservabilityFilterPopover
      label={t('settings.observability.filter.groupBy')}
      count={groupBy.length}
    >
      {() => (
        <div className={styles['observability-groupby-panel']}>
          <div className={styles['observability-advanced-title']}>
            {t('settings.observability.groupBy.hint', { max: MODEL_OBSERVABILITY_GROUP_BY_MAX_DIMENSIONS })}
          </div>
          {MODEL_OBSERVABILITY_GROUP_BY_DIMENSIONS.map((dimension) => {
            const selected = groupBy.includes(dimension);
            const disabled = !selected && groupBy.length >= MODEL_OBSERVABILITY_GROUP_BY_MAX_DIMENSIONS;
            return (
              <label
                key={dimension}
                className={styles['observability-multiselect-option']}
                data-disabled={disabled || undefined}
              >
                <input
                  type="checkbox"
                  checked={selected}
                  disabled={disabled}
                  onChange={() => toggle(dimension)}
                />
                <span className={styles['observability-multiselect-option-label']}>
                  {groupByDimensionLabel(dimension)}
                </span>
                {selected && (
                  <span className={styles['observability-groupby-order']}>
                    {groupBy.indexOf(dimension) + 1}
                  </span>
                )}
              </label>
            );
          })}
        </div>
      )}
    </ObservabilityFilterPopover>
  );
}

/* ── Chips 行（§二十五）────────────────────────────────────────────────── */

/* ── Filter Bar 主体 ──────────────────────────────────────────────────── */

type Props = {
  state: ObservabilityQueryStateApi;
  refreshing: boolean;
  onRefresh: () => void;
  onExport: () => void;
  exportAvailable: boolean;
  exportUnavailableReason: string | null;
  onOpenRecordingSettings: () => void;
};

export function ObservabilityFilterBar({
  state,
  refreshing,
  onRefresh,
  onExport,
  exportAvailable,
  exportUnavailableReason,
  onOpenRecordingSettings,
}: Props) {
  const { appliedFilter, patchFilter, removeChip, clearAllFilters } = state;
  const wireFilter = buildCallFilterInput(appliedFilter);
  const [facetActive, setFacetActive] = React.useState({ provider: false, model: false, category: false });
  const providerFacet = useObservabilityFacetOptions('provider', facetActive.provider, wireFilter);
  const modelFacet = useObservabilityFacetOptions('model', facetActive.model, wireFilter);
  const categoryFacet = useObservabilityFacetOptions('category', facetActive.category, wireFilter);

  const chips = listActiveFilterChips(appliedFilter);

  const exportButton = (
    <Button
      variant="secondary"
      size="sm"
      disabled={!exportAvailable}
      onClick={onExport}
      aria-label={t('settings.observability.export.open')}
    >
      {t('settings.observability.export.open')}
    </Button>
  );

  return (
    <div className={styles['observability-filterbar']}>
      <div className={styles['observability-filterbar-row']}>
        <DateFilter state={state} />
        <ObservabilityMultiSelect
          label={t('settings.observability.filter.provider')}
          options={providerFacet.options.map((value) => ({ value, label: value }))}
          values={appliedFilter.providers}
          onChange={(next) => patchFilter({ providers: next })}
          loading={providerFacet.loading}
          onOpen={() => setFacetActive((prev) => ({ ...prev, provider: true }))}
          emptyLabel={t('settings.observability.facets.empty')}
          loadingLabel={t('settings.observability.facets.loading')}
        />
        <ObservabilityMultiSelect
          label={t('settings.observability.filter.model')}
          options={modelFacet.options.map((value) => ({ value, label: value }))}
          values={appliedFilter.modelIds}
          onChange={(next) => patchFilter({ modelIds: next })}
          loading={modelFacet.loading}
          onOpen={() => setFacetActive((prev) => ({ ...prev, model: true }))}
          emptyLabel={t('settings.observability.facets.empty')}
          loadingLabel={t('settings.observability.facets.loading')}
        />
        <ObservabilityMultiSelect
          label={t('settings.observability.filter.category')}
          options={categoryFacet.options.map((value) => ({ value, label: value }))}
          values={appliedFilter.categories}
          onChange={(next) => patchFilter({ categories: next })}
          loading={categoryFacet.loading}
          onOpen={() => setFacetActive((prev) => ({ ...prev, category: true }))}
          emptyLabel={t('settings.observability.facets.empty')}
          loadingLabel={t('settings.observability.facets.loading')}
        />
        <ObservabilityMultiSelect
          label={t('settings.observability.filter.status')}
          options={MODEL_OBSERVABILITY_TERMINAL_STATUSES.map((value) => ({
            value,
            label: terminalStatusLabel(value),
          }))}
          values={appliedFilter.terminalStatuses}
          onChange={(next) => patchFilter({ terminalStatuses: next })}
          emptyLabel={t('settings.observability.facets.empty')}
          loadingLabel={t('settings.observability.facets.loading')}
        />
        <ObservabilityAdvancedFilters state={state} />
        <GroupByFilter state={state} />
        <div className={styles['observability-filterbar-actions']}>
          <Button
            variant="secondary"
            size="sm"
            loading={refreshing}
            onClick={onRefresh}
            aria-label={t('settings.observability.actions.refresh')}
          >
            {t('settings.observability.actions.refresh')}
          </Button>
          {exportAvailable ? exportButton : (
            <Tooltip content={exportUnavailableReason ?? ''}>
              <span>{exportButton}</span>
            </Tooltip>
          )}
          <Button variant="ghost" size="sm" onClick={onOpenRecordingSettings}>
            {t('settings.observability.recording.openSettings')}
          </Button>
        </div>
      </div>
      {chips.length > 0 && (
        <div className={styles['observability-chips']} role="list" aria-label={t('settings.observability.chips.ariaLabel')}>
          {chips.map((chip) => (
            <span key={chip.id} className={styles['observability-chip']} role="listitem">
              <span className={styles['observability-chip-label']}>{observabilityChipLabel(chip)}</span>
              <button
                type="button"
                className={styles['observability-chip-remove']}
                aria-label={t('settings.observability.chips.remove', { label: observabilityChipLabel(chip) })}
                onClick={() => removeChip(chip)}
              >
                ×
              </button>
            </span>
          ))}
          <button
            type="button"
            className={styles['observability-chips-clear']}
            onClick={clearAllFilters}
          >
            {t('settings.observability.chips.clearAll')}
          </button>
        </div>
      )}
    </div>
  );
}

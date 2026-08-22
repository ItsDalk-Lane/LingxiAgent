/**
 * ObservabilityAdvancedFilters.tsx — 「更多过滤」弹层（Phase 9 §二十四）。
 *
 * 内含：operation / callPurpose / attributionKind（facet 多选）、精确 ID
 * （session/conversation/agent/task，§二十六 高基数维度用精确输入）、
 * inputShape / provenancePrecision / payloadAvailability（闭集多选）、
 * interruptedByRestart / hasPayload（tri-state）。
 *
 * 文本输入是 draft：Enter 或底部「应用」提交（§十四）；多选立即生效。
 */
import React, { useCallback } from 'react';
import {
  MODEL_OBSERVABILITY_PAYLOAD_AVAILABILITIES,
  SEMANTIC_INPUT_SHAPES,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import { Button, SelectWidget } from '../../../ui';
import styles from '../../Settings.module.css';
import type { ObservabilityQueryStateApi } from './use-observability-query-state';
import {
  OBSERVABILITY_EXACT_FILTER_KEYS,
  type ObservabilityExactFilterKey,
} from './model-observability-filter';
import { useObservabilityFacetOptions, type ObservabilityFacetDimension } from './use-observability-facets';
import { ObservabilityMultiSelect } from './ObservabilityMultiSelect';
import { ObservabilityFilterPopover } from './ObservabilityFilterPopover';
import {
  inputShapeLabel,
  payloadAvailabilityLabel,
  provenancePrecisionLabel,
} from './model-observability-labels';
import { buildCallFilterInput } from './model-observability-filter';

const PROVENANCE_PRECISIONS = ['exact', 'partial', 'opaque'] as const;

type TriState = 'any' | 'yes' | 'no';

function triStateOf(value: boolean | null): TriState {
  return value === null ? 'any' : value ? 'yes' : 'no';
}

function triStateValue(state: TriState): boolean | null {
  return state === 'any' ? null : state === 'yes';
}

function FacetMultiSelect({
  dimension,
  active,
  filter,
  values,
  onChange,
  label,
}: {
  dimension: ObservabilityFacetDimension;
  active: boolean;
  filter: ReturnType<typeof buildCallFilterInput>;
  values: string[];
  onChange: (next: string[]) => void;
  label: string;
}) {
  const facet = useObservabilityFacetOptions(dimension, active, filter);
  return (
    <ObservabilityMultiSelect
      label={label}
      options={facet.options.map((value) => ({ value, label: value }))}
      values={values}
      onChange={onChange}
      loading={facet.loading}
      emptyLabel={t('settings.observability.facets.empty')}
      loadingLabel={t('settings.observability.facets.loading')}
    />
  );
}

export function ObservabilityAdvancedFilters({ state }: { state: ObservabilityQueryStateApi }) {
  const { appliedFilter, drafts, patchFilter, setDrafts, applyTextDrafts } = state;
  const wireFilter = buildCallFilterInput(appliedFilter);

  const appliedCount =
    appliedFilter.operations.length
    + appliedFilter.callPurposes.length
    + appliedFilter.attributionKinds.length
    + appliedFilter.inputShapes.length
    + appliedFilter.provenancePrecisions.length
    + appliedFilter.payloadAvailabilities.length
    + OBSERVABILITY_EXACT_FILTER_KEYS.filter((key) => appliedFilter[key].trim() !== '').length
    + (appliedFilter.interruptedByRestart !== null ? 1 : 0)
    + (appliedFilter.hasPayload !== null ? 1 : 0);

  const commitDrafts = useCallback(() => {
    applyTextDrafts();
  }, [applyTextDrafts]);

  const onExactKeyDown = useCallback((event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      event.preventDefault();
      commitDrafts();
    }
  }, [commitDrafts]);

  return (
    <ObservabilityFilterPopover
      label={t('settings.observability.filter.more')}
      count={appliedCount}
    >
      {() => (
        <div className={styles['observability-advanced-panel']}>
          <div className={styles['observability-advanced-group']}>
            <div className={styles['observability-advanced-title']}>
              {t('settings.observability.advanced.facetsTitle')}
            </div>
            <div className={styles['observability-advanced-row']}>
              <FacetMultiSelect
                dimension="operation"
                active
                filter={wireFilter}
                values={appliedFilter.operations}
                onChange={(next) => patchFilter({ operations: next })}
                label={t('settings.observability.filter.operation')}
              />
              <FacetMultiSelect
                dimension="callPurpose"
                active
                filter={wireFilter}
                values={appliedFilter.callPurposes}
                onChange={(next) => patchFilter({ callPurposes: next })}
                label={t('settings.observability.filter.callPurpose')}
              />
              <FacetMultiSelect
                dimension="attributionKind"
                active
                filter={wireFilter}
                values={appliedFilter.attributionKinds}
                onChange={(next) => patchFilter({ attributionKinds: next })}
                label={t('settings.observability.filter.attributionKind')}
              />
            </div>
          </div>

          <div className={styles['observability-advanced-group']}>
            <div className={styles['observability-advanced-title']}>
              {t('settings.observability.advanced.exactIdsTitle')}
            </div>
            {OBSERVABILITY_EXACT_FILTER_KEYS.map((key: ObservabilityExactFilterKey) => (
              <div className={styles['observability-advanced-field']} key={key}>
                <label className={styles['observability-advanced-label']}>
                  {t(`settings.observability.filter.${key}`)}
                </label>
                <input
                  className={styles['settings-input']}
                  value={drafts[key]}
                  placeholder={t(`settings.observability.advanced.${key}Placeholder`)}
                  onChange={(event) => setDrafts({ [key]: event.target.value })}
                  onKeyDown={onExactKeyDown}
                />
              </div>
            ))}
          </div>

          <div className={styles['observability-advanced-group']}>
            <div className={styles['observability-advanced-title']}>
              {t('settings.observability.advanced.shapesTitle')}
            </div>
            <div className={styles['observability-advanced-row']}>
              <ObservabilityMultiSelect
                label={t('settings.observability.filter.inputShape')}
                options={SEMANTIC_INPUT_SHAPES.map((value) => ({ value, label: inputShapeLabel(value) }))}
                values={appliedFilter.inputShapes}
                onChange={(next) => patchFilter({ inputShapes: next })}
                emptyLabel={t('settings.observability.facets.empty')}
                loadingLabel={t('settings.observability.facets.loading')}
              />
              <ObservabilityMultiSelect
                label={t('settings.observability.filter.provenancePrecision')}
                options={PROVENANCE_PRECISIONS.map((value) => ({ value, label: provenancePrecisionLabel(value) }))}
                values={appliedFilter.provenancePrecisions}
                onChange={(next) => patchFilter({ provenancePrecisions: next })}
                emptyLabel={t('settings.observability.facets.empty')}
                loadingLabel={t('settings.observability.facets.loading')}
              />
              <ObservabilityMultiSelect
                label={t('settings.observability.filter.payloadAvailability')}
                options={MODEL_OBSERVABILITY_PAYLOAD_AVAILABILITIES.map((value) => ({
                  value,
                  label: payloadAvailabilityLabel(value),
                }))}
                values={appliedFilter.payloadAvailabilities}
                onChange={(next) => patchFilter({ payloadAvailabilities: next })}
                emptyLabel={t('settings.observability.facets.empty')}
                loadingLabel={t('settings.observability.facets.loading')}
              />
            </div>
          </div>

          <div className={styles['observability-advanced-group']}>
            <div className={styles['observability-advanced-row']}>
              <div className={styles['observability-advanced-field']}>
                <label className={styles['observability-advanced-label']}>
                  {t('settings.observability.filter.interruptedByRestart')}
                </label>
                <SelectWidget
                  options={[
                    { value: 'any', label: t('settings.observability.tri.any') },
                    { value: 'yes', label: t('settings.observability.tri.yes') },
                    { value: 'no', label: t('settings.observability.tri.no') },
                  ]}
                  value={triStateOf(appliedFilter.interruptedByRestart)}
                  onChange={(value) => patchFilter({ interruptedByRestart: triStateValue(value as TriState) })}
                />
              </div>
              <div className={styles['observability-advanced-field']}>
                <label className={styles['observability-advanced-label']}>
                  {t('settings.observability.filter.hasPayload')}
                </label>
                <SelectWidget
                  options={[
                    { value: 'any', label: t('settings.observability.tri.any') },
                    { value: 'yes', label: t('settings.observability.tri.yes') },
                    { value: 'no', label: t('settings.observability.tri.no') },
                  ]}
                  value={triStateOf(appliedFilter.hasPayload)}
                  onChange={(value) => patchFilter({ hasPayload: triStateValue(value as TriState) })}
                />
              </div>
            </div>
          </div>

          <div className={styles['observability-advanced-footer']}>
            <Button variant="primary" size="sm" onClick={commitDrafts}>
              {t('settings.observability.advanced.apply')}
            </Button>
          </div>
        </div>
      )}
    </ObservabilityFilterPopover>
  );
}

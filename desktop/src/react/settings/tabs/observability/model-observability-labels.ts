/**
 * model-observability-labels.ts — 闭集值的 i18n 标签解析（Phase 9）。
 *
 * 所有 wire 枚举值（status/payload kind/visibility/fidelity/sanitization/
 * contentState/transformation/semantic category/role/source type/precision/
 * inputShape）经 settings.observability.values.* 翻译；未翻译时回退原始
 * wire 值（诚实展示，不静默吞）。
 */
import { t } from '../../helpers';
import type { ObservabilityFilterChip, ObservabilityMultiFilterKey } from './model-observability-filter';
import type { ModelObservabilitySourceIdentity } from '../../../../../../shared/model-observability-api-contract.ts';

function valueLabel(group: string, value: string | null | undefined): string {
  if (!value) return '—';
  const key = `settings.observability.values.${group}.${value}`;
  const label = t(key);
  return typeof label === 'string' && label !== key ? label : value;
}

export function terminalStatusLabel(status: string | null | undefined): string {
  return valueLabel('terminalStatus', status);
}

export function payloadAvailabilityLabel(value: string | null | undefined): string {
  return valueLabel('payloadAvailability', value);
}

export function usageAvailabilityLabel(value: string | null | undefined): string {
  return valueLabel('usageAvailability', value);
}

export function usageStatusLabel(value: string | null | undefined): string {
  return valueLabel('usageStatus', value);
}

export function provenancePrecisionLabel(value: string | null | undefined): string {
  return valueLabel('provenancePrecision', value);
}

export function inputShapeLabel(value: string | null | undefined): string {
  return valueLabel('inputShape', value);
}

export function payloadKindLabel(value: string | null | undefined): string {
  return valueLabel('payloadKind', value);
}

export function payloadVisibilityLabel(value: string | null | undefined): string {
  return valueLabel('payloadVisibility', value);
}

export function payloadFidelityLabel(value: string | null | undefined): string {
  return valueLabel('payloadFidelity', value);
}

export function sanitizationStatusLabel(value: string | null | undefined): string {
  return valueLabel('sanitizationStatus', value);
}

export function payloadContentStateLabel(value: string | null | undefined): string {
  return valueLabel('contentState', value);
}

export function transformationLabel(value: string | null | undefined): string {
  return valueLabel('transformation', value);
}

export function mappingPrecisionLabel(value: string | null | undefined): string {
  return valueLabel('mappingPrecision', value);
}

export function semanticCategoryLabel(value: string | null | undefined): string {
  return valueLabel('semanticCategory', value);
}

export function semanticRoleLabel(value: string | null | undefined): string {
  return valueLabel('semanticRole', value);
}

export function semanticSourceTypeLabel(value: string | null | undefined): string {
  return valueLabel('semanticSourceType', value);
}

export function semanticRootLabel(value: string | null | undefined): string {
  return valueLabel('semanticRoot', value);
}

export function persistenceCompletenessLabel(value: string | null | undefined): string {
  return valueLabel('persistenceCompleteness', value);
}

export function attributionKindLabel(value: string | null | undefined): string {
  return valueLabel('attributionKind', value);
}

/** subsystem/operation 是开放取值（非闭集）：已知值走 values.* 翻译，未知回退原文。 */
export function subsystemLabel(value: string | null | undefined): string {
  return valueLabel('subsystem', value);
}

export function operationLabel(value: string | null | undefined): string {
  return valueLabel('operation', value);
}

export function originLabel(value: string | null | undefined): string {
  return valueLabel('origin', value);
}

export function sourceIdentityKindLabel(value: ModelObservabilitySourceIdentity['kind'] | null | undefined): string {
  return valueLabel('sourceKind', value);
}

export function sourceIdentityTitle(identity: ModelObservabilitySourceIdentity | null | undefined): string {
  if (identity?.title) {
    if (identity.resolution === 'derived') {
      const localized = operationLabel(identity.title);
      return localized === identity.title
        ? identity.title.replace(/[_-]+/g, ' ').replace(/\s+/g, ' ').trim()
        : localized;
    }
    return identity.title;
  }
  return sourceIdentityKindLabel(identity?.kind ?? 'unknown');
}

export function groupByDimensionLabel(value: string | null | undefined): string {
  return valueLabel('groupByDimension', value);
}

export function queryStatusLabel(value: string | null | undefined): string {
  return valueLabel('queryStatus', value);
}

export function recordingStatusLabel(value: string | null | undefined): string {
  return valueLabel('recordingStatus', value);
}

/* ── Filter chip 文案（FilterBar / ExportDialog 共享，§二十五/§一百零九）── */

/** chip 多值字段 → filter.* 文案键（显式映射；`replace(/s$/)` 会毁掉 statuses/availabilities）。 */
const OBSERVABILITY_CHIP_FIELD_KEYS: Record<ObservabilityMultiFilterKey, string> = {
  providers: 'provider',
  modelIds: 'model',
  categories: 'category',
  terminalStatuses: 'status',
  operations: 'operation',
  callPurposes: 'callPurpose',
  attributionKinds: 'attributionKind',
  inputShapes: 'inputShape',
  provenancePrecisions: 'provenancePrecision',
  payloadAvailabilities: 'payloadAvailability',
};

export function observabilityChipLabel(chip: ObservabilityFilterChip): string {
  switch (chip.kind) {
    case 'date':
      return t('settings.observability.filter.date');
    case 'multi': {
      const field = t(`settings.observability.filter.${OBSERVABILITY_CHIP_FIELD_KEYS[chip.field]}`);
      let value = chip.value;
      if (chip.field === 'terminalStatuses') value = terminalStatusLabel(chip.value);
      else if (chip.field === 'payloadAvailabilities') value = payloadAvailabilityLabel(chip.value);
      else if (chip.field === 'provenancePrecisions') value = provenancePrecisionLabel(chip.value);
      else if (chip.field === 'inputShapes') value = inputShapeLabel(chip.value);
      else if (chip.field === 'categories') value = subsystemLabel(chip.value);
      else if (chip.field === 'operations') value = operationLabel(chip.value);
      else if (chip.field === 'attributionKinds') value = attributionKindLabel(chip.value);
      return `${field}: ${value}`;
    }
    case 'exact':
      return t(`settings.observability.filter.${chip.field}`);
    case 'flag':
      return `${t(`settings.observability.filter.${chip.field}`)}: ${
        chip.value ? t('settings.observability.tri.yes') : t('settings.observability.tri.no')
      }`;
  }
}

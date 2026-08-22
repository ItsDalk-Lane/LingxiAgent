/**
 * ObservabilityProvenance.tsx — Semantic Provenance + Provider Mapping
 * Inspector（Phase 9 §七十二～八十六）。
 *
 *   - 输入来源视图：section 列表 + 内容面板（§七十九）；每段显示
 *     ordinal/category/role/precision/source(type+id+version)/locator
 *     (root-path-span)（§七十三）。
 *   - 内容只经 locator 解析（resolveSemanticLocator；§七十六 禁内容搜索反推）。
 *     span=null / 非 exact → 「结构级，无法定位」明说，不伪造（§七十七）。
 *   - Provider mapping 视图：semanticSectionOrdinal → providerLocator.path/span
 *     + transformation + mappingPrecision（§八十一）；providerLocator=null
 *     诚实展示（§八十三；Pi null mapping 是正常态）。
 *   - 双载时 semantic→provider 交叉跳转（§八十四）；未载时给提示，不假装。
 */
import React, { useMemo, useState } from 'react';
import type {
  ModelSemanticInputProvenance,
  ProviderRequestProvenance,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';
import {
  asSemanticInputProvenance,
  resolveProviderLocator,
  resolveSemanticLocator,
  type SemanticLocatorResolution,
} from './observability-provenance-resolve';
import {
  mappingPrecisionLabel,
  provenancePrecisionLabel,
  semanticCategoryLabel,
  semanticRoleLabel,
  semanticRootLabel,
  semanticSourceTypeLabel,
  transformationLabel,
} from './model-observability-labels';

function formatLocatorPath(path: Array<number | string> | undefined): string {
  if (!path || path.length === 0) return '';
  return path.map((item) => (typeof item === 'number' ? `[${item}]` : `.${item}`)).join('');
}

function ResolutionPane({ resolution }: { resolution: SemanticLocatorResolution }) {
  if (resolution.status === 'resolved') {
    return (
      <pre className={styles['observability-provenance-content']} data-wrap>
        {resolution.text === ''
          ? t('settings.observability.provenance.emptySpan')
          : resolution.text}
      </pre>
    );
  }
  if (resolution.status === 'structural') {
    return (
      <div className={styles['observability-provenance-note']}>
        {t('settings.observability.provenance.structural', { kind: resolution.valueKind })}
      </div>
    );
  }
  return (
    <div className={styles['observability-provenance-note']}>
      {t(`settings.observability.provenance.unavailable.${resolution.reason}`)}
    </div>
  );
}

export function ObservabilitySemanticProvenance({ provenanceInput, semanticPayload, highlightOrdinal }: {
  provenanceInput: unknown;
  semanticPayload: unknown;
  /** 交叉跳转高亮（provider mapping 点过来的 ordinal，§八十四）。 */
  highlightOrdinal?: number | null;
}) {
  const provenance = useMemo(() => asSemanticInputProvenance(provenanceInput), [provenanceInput]);
  const [selectedOrdinal, setSelectedOrdinal] = useState<number | null>(null);
  const effectiveOrdinal = highlightOrdinal ?? selectedOrdinal;

  if (!provenance) {
    return (
      <div className={styles['observability-provenance-note']}>
        {t('settings.observability.provenance.absent')}
      </div>
    );
  }

  const selected = effectiveOrdinal !== null ? provenance.sections[effectiveOrdinal] ?? null : null;

  return (
    <div className={styles['observability-provenance']}>
      <div className={styles['observability-provenance-list']} role="listbox" aria-label={t('settings.observability.provenance.listAria')}>
        {provenance.sections.map((section, ordinal) => (
          <button
            key={ordinal}
            type="button"
            role="option"
            aria-selected={effectiveOrdinal === ordinal}
            className={styles['observability-provenance-item']}
            data-selected={effectiveOrdinal === ordinal || undefined}
            data-precision={section.precision}
            onClick={() => setSelectedOrdinal(ordinal)}
          >
            <span className={styles['observability-provenance-ordinal']}>#{ordinal}</span>
            <span className={styles['observability-provenance-category']}>
              {semanticCategoryLabel(section.category)}
            </span>
            <span className={styles['observability-provenance-precision']}>
              {provenancePrecisionLabel(section.precision)}
            </span>
          </button>
        ))}
      </div>
      <div className={styles['observability-provenance-detail']}>
        {!selected ? (
          <div className={styles['observability-provenance-note']}>
            {t('settings.observability.provenance.selectSection')}
          </div>
        ) : (
          <>
            <dl className={styles['observability-provenance-meta']}>
              <dt>{t('settings.observability.provenance.field.category')}</dt>
              <dd>{semanticCategoryLabel(selected.category)}</dd>
              <dt>{t('settings.observability.provenance.field.role')}</dt>
              <dd>{selected.role ? semanticRoleLabel(selected.role) : '—'}</dd>
              <dt>{t('settings.observability.provenance.field.precision')}</dt>
              <dd>{provenancePrecisionLabel(selected.precision)}</dd>
              <dt>{t('settings.observability.provenance.field.source')}</dt>
              <dd>
                {selected.source
                  ? [
                    semanticSourceTypeLabel(selected.source.type),
                    selected.source.id ?? null,
                    selected.source.version ? `v:${selected.source.version}` : null,
                  ].filter(Boolean).join(' · ')
                  : '—'}
              </dd>
              <dt>{t('settings.observability.provenance.field.locator')}</dt>
              <dd>
                <code>
                  {semanticRootLabel(selected.locator.root)}
                  {formatLocatorPath(selected.locator.path)}
                  {selected.locator.span
                    ? ` [${selected.locator.span.start}, ${selected.locator.span.end})`
                    : ''}
                </code>
              </dd>
            </dl>
            <ResolutionPane resolution={resolveSemanticLocator(semanticPayload, selected.locator)} />
          </>
        )}
      </div>
    </div>
  );
}

export function ObservabilityProviderProvenance({ provenanceInput, providerPayload, onJumpToSection }: {
  provenanceInput: unknown;
  providerPayload: unknown;
  /** semantic provenance 已加载时的交叉跳转（§八十四）。 */
  onJumpToSection?: (ordinal: number) => void;
}) {
  const provenance = useMemo(() => {
    if (!provenanceInput || typeof provenanceInput !== 'object' || Array.isArray(provenanceInput)) return null;
    const record = provenanceInput as Record<string, unknown>;
    if (!Array.isArray(record.mappings)) return null;
    return provenanceInput as ProviderRequestProvenance;
  }, [provenanceInput]);

  if (!provenance) {
    // §八十三：Pi 等 null mapping 是正常态——明说「无 mapping」，不反推。
    return (
      <div className={styles['observability-provenance-note']}>
        {t('settings.observability.providerMapping.absent')}
      </div>
    );
  }

  return (
    <div className={styles['observability-provider-mapping']}>
      <div className={styles['observability-provenance-note']}>
        {t('settings.observability.providerMapping.protocol', { protocol: provenance.protocol })}
      </div>
      {provenance.mappings.length === 0 && (
        <div className={styles['observability-provenance-note']}>
          {t('settings.observability.providerMapping.empty')}
        </div>
      )}
      {provenance.mappings.map((mapping, index) => (
        <div key={index} className={styles['observability-provider-mapping-row']}>
          <button
            type="button"
            className={styles['observability-provider-mapping-ordinal']}
            disabled={!onJumpToSection}
            title={onJumpToSection
              ? t('settings.observability.providerMapping.jump')
              : t('settings.observability.providerMapping.loadSemanticFirst')}
            onClick={() => onJumpToSection?.(mapping.semanticSectionOrdinal)}
          >
            #{mapping.semanticSectionOrdinal}
          </button>
          <span className={styles['observability-badge']} data-kind="transformation">
            {transformationLabel(mapping.transformation)}
          </span>
          <span className={styles['observability-badge']} data-kind="precision">
            {mappingPrecisionLabel(mapping.mappingPrecision)}
          </span>
          <span className={styles['observability-provider-mapping-locator']}>
            {mapping.providerLocator ? (
              <code>
                body{formatLocatorPath(mapping.providerLocator.path)}
                {mapping.providerLocator.span
                  ? ` [${mapping.providerLocator.span.start}, ${mapping.providerLocator.span.end})`
                  : ''}
              </code>
            ) : (
              /* §八十三：providerLocator=null 诚实展示，绝不重新搜索定位 */
              t('settings.observability.providerMapping.noLocator')
            )}
          </span>
          {mapping.providerLocator && (
            <ResolutionPane resolution={resolveProviderLocator(providerPayload, mapping.providerLocator)} />
          )}
        </div>
      ))}
    </div>
  );
}

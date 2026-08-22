/**
 * ObservabilityPayloadCard.tsx — 单条 payload record 卡片（Phase 9
 * §六十～七十一、§八十五～八十六）。
 *
 *   - metadata 先行：kind/visibility/fidelity/sanitization/attemptId/ordinal/
 *     capturedAt/recordSize 头部徽章；正文懒加载（点击才 GET /payloads/:id，
 *     §六十二）。
 *   - 403 → 「仅本机可看」提示，不是 call 失败（§六十三）。
 *   - contentState 四态严格区分（§六十八）：present / null_payload /
 *     opaque_or_unavailable / corrupt——绝不把 null/opaque 渲染成
 *     「Empty response」。
 *   - visibility 一等公民：OPAQUE/UNAVAILABLE/METADATA_ONLY 给显式解释
 *     （§六十五）。
 *   - fidelity/sanitization 徽章 tooltip 不暗示 byte-exact（§八十五）；
 *     stream_aggregate 明说「非原始 SSE 序列」（§八十六）。
 */
import React from 'react';
import type {
  ModelObservabilityPayloadRecordDetail,
  ModelObservabilityPayloadRecordMetadata,
  ModelSemanticResponse,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import { Tooltip } from '../../../ui';
import styles from '../../Settings.module.css';
import { ModelObservabilityRequestError } from './model-observability-actions';
import { JsonValueViewer } from './JsonValueViewer';
import { ObservabilitySemanticResponse } from './ObservabilitySemanticResponse';
import {
  ObservabilityProviderProvenance,
  ObservabilitySemanticProvenance,
} from './ObservabilityProvenance';
import { ObservabilityBlobPreview } from './ObservabilityBlobPreview';
import { formatLocalFullDateTime, formatNumber, isoTooltip, shortId } from './model-observability-format';
import {
  payloadContentStateLabel,
  payloadFidelityLabel,
  payloadKindLabel,
  payloadVisibilityLabel,
  sanitizationStatusLabel,
} from './model-observability-labels';

export type PayloadBodyState =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'loaded'; detail: ModelObservabilityPayloadRecordDetail }
  | { status: 'local_only' }
  | { status: 'error'; message: string };

export function payloadBodyErrorState(error: unknown): PayloadBodyState {
  if (error instanceof ModelObservabilityRequestError
    && (error.kind === 'local_only_route' || error.kind === 'studio_owner_required' || error.kind === 'forbidden')) {
    return { status: 'local_only' };
  }
  return { status: 'error', message: error instanceof Error ? error.message : String(error) };
}

function Badge({ kind, value, hintKey }: { kind: string; value: string; hintKey?: string }) {
  const badge = (
    <span className={styles['observability-badge']} data-kind={kind}>{value}</span>
  );
  return hintKey ? <Tooltip content={t(hintKey)}>{badge}</Tooltip> : badge;
}

function semanticResponseOf(detail: ModelObservabilityPayloadRecordDetail): ModelSemanticResponse | null {
  const payload = detail.payload;
  if (!payload || typeof payload !== 'object' || Array.isArray(payload)) return null;
  const record = payload as Record<string, unknown>;
  // semantic_response record 的 payload 即 ModelSemanticResponse 外壳；
  // completeness 必填是形状判据（§一百零五契约）。
  if (record.completeness !== 'complete' && record.completeness !== 'partial') return null;
  return payload as ModelSemanticResponse;
}

export function ObservabilityPayloadCard({
  metadata,
  body,
  isLocalOwner,
  onLoadBody,
  onJumpToSection,
  highlightOrdinal,
}: {
  metadata: ModelObservabilityPayloadRecordMetadata;
  body: PayloadBodyState;
  isLocalOwner: boolean;
  onLoadBody: (recordId: number) => void;
  onJumpToSection?: (ordinal: number) => void;
  highlightOrdinal?: number | null;
}) {
  const loaded = body.status === 'loaded' ? body.detail : null;

  return (
    <div className={styles['observability-payload-card']} data-kind={metadata.kind}>
      <div className={styles['observability-payload-head']}>
        <span className={styles['observability-badge']} data-kind="kind">
          {payloadKindLabel(metadata.kind)}
        </span>
        <Badge
          kind="visibility"
          value={payloadVisibilityLabel(metadata.visibility)}
          hintKey={`settings.observability.visibilityHint.${metadata.visibility}`}
        />
        <Badge
          kind="fidelity"
          value={payloadFidelityLabel(metadata.fidelity)}
          hintKey={`settings.observability.fidelityHint.${metadata.fidelity}`}
        />
        <Badge
          kind="sanitization"
          value={sanitizationStatusLabel(metadata.sanitizationStatus)}
          hintKey={`settings.observability.sanitizationHint.${metadata.sanitizationStatus}`}
        />
        <span className={styles['observability-payload-head-meta']}>
          {metadata.attemptId && <span title={metadata.attemptId}>attempt {shortId(metadata.attemptId)}</span>}
          {metadata.providerRequestOrdinal !== null && <span> · req #{metadata.providerRequestOrdinal}</span>}
          <span title={isoTooltip(metadata.capturedAt)}> · {formatLocalFullDateTime(metadata.capturedAt)}</span>
          {metadata.recordCharCount !== null && (
            <span title={t('settings.observability.payload.recordChars')}> · {formatNumber(metadata.recordCharCount)}ch</span>
          )}
        </span>
      </div>

      {/* §六十 blob 引用：只展示 metadata + 按需预览（§一百三十一） */}
      {metadata.blobIds.length > 0 && (
        <div className={styles['observability-payload-blobs']}>
          {metadata.blobIds.map((blobId) => (
            <ObservabilityBlobPreview key={blobId} blobId={blobId} isLocalOwner={isLocalOwner} />
          ))}
        </div>
      )}

      {body.status === 'idle' && (
        <button
          type="button"
          className={styles['observability-payload-load']}
          onClick={() => onLoadBody(metadata.id)}
        >
          {metadata.hasBody
            ? t('settings.observability.payload.loadBody')
            : t('settings.observability.payload.loadMetadataOnly')}
        </button>
      )}
      {body.status === 'loading' && (
        <div className={styles['observability-loading']} aria-busy>
          {t('settings.observability.loading.payload')}
        </div>
      )}
      {/* §六十三：403 = 仅本机可看，不是 call 失败 */}
      {body.status === 'local_only' && (
        <div className={styles['observability-provenance-note']}>
          {t('settings.observability.payload.localOnly')}
        </div>
      )}
      {body.status === 'error' && (
        <div className={styles['observability-error-detail']} role="alert">{body.message}</div>
      )}

      {loaded && (
        <div className={styles['observability-payload-body']}>
          {/* contentState 四态（§六十八） */}
          {loaded.contentState !== 'present' && (
            <div className={styles['observability-provenance-note']} data-content-state={loaded.contentState}>
              {loaded.contentState === 'opaque_or_unavailable'
                ? t(`settings.observability.payload.contentStateOpaque.${loaded.visibility}`)
                : t(`settings.observability.payload.contentState.${loaded.contentState}`)}
            </div>
          )}

          {loaded.contentState === 'present' && loaded.kind === 'semantic_response' && (() => {
            const response = semanticResponseOf(loaded);
            return response ? (
              <ObservabilitySemanticResponse response={response} />
            ) : (
              <JsonValueViewer value={loaded.payload} copyAriaLabel={t('settings.observability.copy.capturedPayloadAria')} />
            );
          })()}

          {loaded.contentState === 'present' && loaded.kind !== 'semantic_response' && (
            <JsonValueViewer value={loaded.payload} copyAriaLabel={t('settings.observability.copy.capturedPayloadAria')} />
          )}

          {/* §七十二 输入来源视图（semantic_request；provenance 随 exact payload） */}
          {loaded.kind === 'semantic_request' && loaded.hasSemanticProvenance && (
            <div className={styles['observability-payload-provenance']}>
              <div className={styles['observability-panel-subtitle']}>
                {t('settings.observability.provenance.title')}
              </div>
              <ObservabilitySemanticProvenance
                provenanceInput={loaded.semanticInputProvenance}
                semanticPayload={loaded.payload}
                highlightOrdinal={highlightOrdinal}
              />
            </div>
          )}

          {/* §八十 Provider mapping 视图（provider_request） */}
          {loaded.kind === 'provider_request' && (
            <div className={styles['observability-payload-provenance']}>
              <div className={styles['observability-panel-subtitle']}>
                {t('settings.observability.providerMapping.title')}
              </div>
              {loaded.hasProviderProvenance ? (
                <ObservabilityProviderProvenance
                  provenanceInput={loaded.providerRequestProvenance}
                  providerPayload={loaded.payload}
                  onJumpToSection={onJumpToSection}
                />
              ) : (
                <div className={styles['observability-provenance-note']}>
                  {t('settings.observability.providerMapping.absent')}
                </div>
              )}
            </div>
          )}

          <div className={styles['observability-payload-foot']}>
            <span data-state={loaded.contentState}>
              {t('settings.observability.payload.contentStateLabel')}
              {': '}
              {payloadContentStateLabel(loaded.contentState)}
            </span>
          </div>
        </div>
      )}
    </div>
  );
}

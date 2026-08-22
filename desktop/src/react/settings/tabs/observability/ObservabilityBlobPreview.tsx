/**
 * ObservabilityBlobPreview.tsx — Stored Blob 预览（Phase 9 §一百二十九～
 * 一百三十一）。
 *
 * 纪律：
 *   - 默认只显示 media type / byte length / blob id + Preview 按钮——点击才
 *     拉字节（§一百三十一）。
 *   - HEAD 探测先行：content-type/length 决定预览形态；超大 blob 先确认
 *     再下载（§一百三十一：绝不自动预览超大 blob）。
 *   - 安全 MIME（image/audio/video）→ Blob URL + 原生控件；关闭/切换时
 *     revokeObjectURL（§一百三十一 生命周期）。
 *   - application/octet-stream 不解析：metadata + 可选 Download（下载的是
 *     存储的原 blob 字节，不重新编码）。
 *   - 403（远端 owner）→ 「仅本机可看」；404 blob_missing → 显式缺失态。
 *   - 绝不展示/泄漏磁盘路径（§一百二十六）。
 */
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { MODEL_OBSERVABILITY_BLOB_SAFE_MEDIA_MAJOR } from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import { Button } from '../../../ui';
import styles from '../../Settings.module.css';
import {
  fetchObservabilityBlob,
  isObservabilityAbortError,
  ModelObservabilityRequestError,
  probeObservabilityBlob,
} from './model-observability-actions';
import { formatNumber, shortId } from './model-observability-format';

/** 超过该大小的 blob 预览前需要显式确认（§一百三十一）。 */
const PREVIEW_CONFIRM_BYTES = 8 * 1024 * 1024;
/** 超过该大小的 blob 不提供预览（仍可 Download 原字节）。 */
const PREVIEW_MAX_BYTES = 64 * 1024 * 1024;

type BlobState =
  | { status: 'idle' }
  | { status: 'probing' }
  | { status: 'need-confirm'; contentType: string | null; contentLength: number | null }
  | { status: 'loading' }
  | { status: 'preview'; objectUrl: string; contentType: string }
  | { status: 'opaque'; contentType: string | null; contentLength: number | null }
  | { status: 'local_only' }
  | { status: 'missing' }
  | { status: 'invalid' }
  | { status: 'error'; message: string };

function blobErrorState(error: unknown): BlobState {
  if (error instanceof ModelObservabilityRequestError) {
    if (error.kind === 'local_only_route' || error.kind === 'studio_owner_required' || error.kind === 'forbidden') {
      return { status: 'local_only' };
    }
    if (error.kind === 'blob_missing' || error.kind === 'not_found') return { status: 'missing' };
    if (error.kind === 'invalid_blob_id' || error.kind === 'invalid_query') return { status: 'invalid' };
    return { status: 'error', message: error.message };
  }
  return { status: 'error', message: error instanceof Error ? error.message : String(error) };
}

function isPreviewable(contentType: string | null): boolean {
  if (!contentType) return false;
  const major = contentType.split('/')[0]?.toLowerCase() ?? '';
  return (MODEL_OBSERVABILITY_BLOB_SAFE_MEDIA_MAJOR as readonly string[]).includes(major);
}

export function ObservabilityBlobPreview({ blobId, isLocalOwner }: {
  blobId: string;
  isLocalOwner: boolean;
}) {
  const [state, setState] = useState<BlobState>({ status: 'idle' });
  const generationRef = useRef(0);

  // 关闭/切换/unmount 时 revokeObjectURL（§一百三十一 生命周期纪律）。
  useEffect(() => () => {
    setState((prev) => {
      if (prev.status === 'preview') URL.revokeObjectURL(prev.objectUrl);
      return prev;
    });
    generationRef.current += 1;
  }, []);

  const revokeCurrent = useCallback(() => {
    setState((prev) => {
      if (prev.status === 'preview') URL.revokeObjectURL(prev.objectUrl);
      return prev;
    });
  }, []);

  const load = useCallback(async (expectedType: string | null) => {
    const generation = generationRef.current;
    setState({ status: 'loading' });
    try {
      const res = await fetchObservabilityBlob(blobId);
      if (generationRef.current !== generation) return;
      const contentType = res.headers.get('content-type') ?? expectedType ?? 'application/octet-stream';
      const bytes = await res.blob();
      if (generationRef.current !== generation) return;
      const objectUrl = URL.createObjectURL(bytes);
      revokeCurrent();
      setState({ status: 'preview', objectUrl, contentType });
    } catch (error) {
      if (generationRef.current !== generation || isObservabilityAbortError(error)) return;
      setState(blobErrorState(error));
    }
  }, [blobId, revokeCurrent]);

  const probe = useCallback(async () => {
    const generation = ++generationRef.current;
    setState({ status: 'probing' });
    try {
      const meta = await probeObservabilityBlob(blobId);
      if (generationRef.current !== generation) return;
      if (!isPreviewable(meta.contentType)) {
        setState({ status: 'opaque', contentType: meta.contentType, contentLength: meta.contentLength });
        return;
      }
      if (meta.contentLength !== null && meta.contentLength > PREVIEW_MAX_BYTES) {
        setState({ status: 'opaque', contentType: meta.contentType, contentLength: meta.contentLength });
        return;
      }
      if (meta.contentLength !== null && meta.contentLength > PREVIEW_CONFIRM_BYTES) {
        setState({ status: 'need-confirm', contentType: meta.contentType, contentLength: meta.contentLength });
        return;
      }
      await load(meta.contentType);
    } catch (error) {
      if (generationRef.current !== generation || isObservabilityAbortError(error)) return;
      setState(blobErrorState(error));
    }
  }, [blobId, load]);

  const close = useCallback(() => {
    generationRef.current += 1;
    setState((prev) => {
      if (prev.status === 'preview') URL.revokeObjectURL(prev.objectUrl);
      return { status: 'idle' };
    });
  }, []);

  const download = useCallback(async () => {
    const generation = ++generationRef.current;
    try {
      const res = await fetchObservabilityBlob(blobId);
      if (generationRef.current !== generation) return;
      const bytes = await res.blob();
      if (generationRef.current !== generation) return;
      const objectUrl = URL.createObjectURL(bytes);
      // 命令式下载锚点不属于渲染树，必须 createElement 后合成 click（blob 下载惯例）。
      // eslint-disable-next-line no-restricted-syntax
      const anchor = document.createElement('a');
      anchor.href = objectUrl;
      anchor.download = `${blobId}.bin`;
      anchor.click();
      setTimeout(() => URL.revokeObjectURL(objectUrl), 10_000);
    } catch (error) {
      if (generationRef.current !== generation || isObservabilityAbortError(error)) return;
      setState(blobErrorState(error));
    }
  }, [blobId]);

  return (
    <div className={styles['observability-blob']} data-state={state.status}>
      <div className={styles['observability-blob-meta']}>
        <code title={blobId}>{shortId(blobId)}</code>
        {!isLocalOwner && (
          <span className={styles['observability-ledger-muted']}>
            {t('settings.observability.blob.localOnly')}
          </span>
        )}
        {isLocalOwner && state.status === 'idle' && (
          <Button variant="ghost" size="sm" onClick={() => void probe()}>
            {t('settings.observability.blob.preview')}
          </Button>
        )}
        {(state.status === 'probing' || state.status === 'loading') && (
          <span className={styles['observability-loading']} aria-busy>
            {t('settings.observability.loading.blob')}
          </span>
        )}
        {state.status === 'need-confirm' && (
          <>
            <span className={styles['observability-ledger-muted']}>
              {t('settings.observability.blob.largeWarning', {
                size: state.contentLength !== null ? formatNumber(state.contentLength) : '—',
              })}
            </span>
            <Button variant="secondary" size="sm" onClick={() => void load(state.contentType)}>
              {t('settings.observability.blob.previewAnyway')}
            </Button>
          </>
        )}
        {state.status === 'opaque' && (
          <>
            <span className={styles['observability-ledger-muted']}>
              {t('settings.observability.blob.opaqueNote', {
                type: state.contentType ?? 'application/octet-stream',
                size: state.contentLength !== null ? formatNumber(state.contentLength) : '—',
              })}
            </span>
            <Button variant="ghost" size="sm" onClick={() => void download()}>
              {t('settings.observability.blob.download')}
            </Button>
          </>
        )}
        {state.status === 'local_only' && (
          <span className={styles['observability-ledger-muted']}>
            {t('settings.observability.blob.localOnly')}
          </span>
        )}
        {state.status === 'missing' && (
          <span className={styles['observability-ledger-muted']} data-status="missing">
            {t('settings.observability.blob.missing')}
          </span>
        )}
        {state.status === 'invalid' && (
          <span className={styles['observability-ledger-muted']}>
            {t('settings.observability.blob.invalid')}
          </span>
        )}
        {state.status === 'error' && (
          <span className={styles['observability-error-detail']} role="alert">{state.message}</span>
        )}
        {state.status === 'preview' && (
          <Button variant="ghost" size="sm" onClick={close}>
            {t('settings.observability.blob.close')}
          </Button>
        )}
      </div>
      {state.status === 'preview' && (
        <div className={styles['observability-blob-preview']}>
          {state.contentType.startsWith('image/') && (
            <img className={styles['observability-blob-media']} src={state.objectUrl} alt={blobId} />
          )}
          {state.contentType.startsWith('audio/') && (
            <audio controls src={state.objectUrl} />
          )}
          {state.contentType.startsWith('video/') && (
            <video className={styles['observability-blob-media']} controls src={state.objectUrl} />
          )}
        </div>
      )}
    </div>
  );
}

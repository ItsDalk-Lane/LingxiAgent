import { useEffect, useState } from 'react';
import type { FileRef } from '../../../types/file-ref';
import { loadMediaSource } from './media-source';
import { fileRefVersionToken } from '../../../services/resource-url';
import styles from './MediaViewer.module.css';

declare function t(key: string, vars?: Record<string, string | number>): string;

// prop 名 `file`（不可用 `ref`，React 会截获）
interface Props {
  file: FileRef;
  viewport: { width: number; height: number };
  onReady?: () => void;
  onError?: (e: unknown) => void;
}

export function VideoStage({ file, viewport, onReady, onError }: Props) {
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const fileVersionToken = fileRefVersionToken(file);

  useEffect(() => {
    let cancelled = false;
    setSrc(null);
    setFailed(false);
    loadMediaSource(file)
      .then((s) => { if (!cancelled) setSrc(s.url); })
      .catch((err) => {
        if (cancelled) return;
        setFailed(true);
        onError?.(err);
      });
    return () => { cancelled = true; };
    // 依赖稳定 id + version；file 是引用类型每次新建，onError 仅在错误时被调用。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [file.id, fileVersionToken]);

  // 解码失败必须显式暴露（HEVC 等编码 Chromium 解不动时 <video> 只会黑屏静默），
  // 并给出「用系统播放器打开」逃生门——禁止静默降级。
  if (failed) {
    return (
      <div className={styles.videoErrorCard} data-testid="video-stage-error">
        <div className={styles.videoErrorText}>{t('mediaViewer.videoDecodeError')}</div>
        {file.path && (
          <button
            type="button"
            className={styles.videoErrorAction}
            onClick={() => { window.platform?.openFile?.(file.path); }}
          >
            {t('mediaViewer.openWithSystemPlayer')}
          </button>
        )}
      </div>
    );
  }

  return (
    <div className={styles.videoWrap} style={{ maxWidth: viewport.width, maxHeight: viewport.height }}>
      {!src && <div className={styles.spinner} data-testid="video-stage-spinner" />}
      {src && (
        <video
          src={src}
          controls
          autoPlay={false}
          onLoadedData={onReady}
          onError={() => {
            setFailed(true);
            onError?.(new Error('video element failed to load or decode'));
          }}
          className={styles.videoEl}
          data-testid="video-stage-video"
        />
      )}
    </div>
  );
}

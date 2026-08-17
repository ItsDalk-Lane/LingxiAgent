import React from 'react';
import { t } from '../../helpers';
import type { Modality } from './unified-models';
import styles from '../../Settings.module.css';

/**
 * OutputModalityIcons — 模型 ID 右侧的输出模态图标。
 *
 * 语义只表达 outputs（文本/图片/视频/音频），不混入 reasoning/tools/web 等
 * 「模型能力」语义（那是 CapabilityIcon 的事）。SVG 形状可与 CapabilityIcon
 * 相同，但组件语义必须分开。
 */

const MODALITY_LABEL_KEYS: Record<Modality, string> = {
  text: 'settings.api.outputModality.text',
  image: 'settings.api.outputModality.image',
  video: 'settings.api.outputModality.video',
  audio: 'settings.api.outputModality.audio',
};

function ModalityIcon({ modality }: { modality: Modality }) {
  const label = t(`settings.api.outputModality.${modality}`);
  return (
    <span className={styles['pv-output-modality-icon']} title={label} aria-label={label} role="img">
      {modality === 'image' ? (
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
          <circle cx="8.5" cy="8.5" r="1.5" />
          <path d="M21 15l-5-5L5 21" />
        </svg>
      ) : modality === 'video' ? (
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <rect x="3" y="5" width="13" height="14" rx="2" />
          <path d="m16 9 5-3v12l-5-3" />
        </svg>
      ) : modality === 'audio' ? (
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M4 10v4" />
          <path d="M8 7v10" />
          <path d="M12 4v16" />
          <path d="M16 8v8" />
          <path d="M20 11v2" />
        </svg>
      ) : (
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
          <polyline points="14 2 14 8 20 8" />
          <line x1="16" y1="13" x2="8" y2="13" />
          <line x1="16" y1="17" x2="8" y2="17" />
          <line x1="10" y1="9" x2="8" y2="9" />
        </svg>
      )}
    </span>
  );
}

export function OutputModalityIcons({ outputs }: { outputs: Modality[] }) {
  if (!Array.isArray(outputs) || outputs.length === 0) return null;
  return (
    <span className={styles['pv-output-modality-icons']} data-output-modalities={outputs.join(' ')}>
      {outputs.map(modality => <ModalityIcon key={modality} modality={modality} />)}
    </span>
  );
}

import React from 'react';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';
import type { ProviderMediaCapabilityBinding, MediaCapabilityKind } from '../../store';

const CAPABILITY_ORDER: MediaCapabilityKind[] = ['imageGeneration', 'videoGeneration', 'speechRecognition'];

function capabilityLabel(capability: MediaCapabilityKind): string {
  switch (capability) {
    case 'imageGeneration': return t('settings.media.imageGeneration');
    case 'videoGeneration': return t('settings.media.videoGeneration');
    case 'speechRecognition': return t('settings.media.speechRecognition');
    default: return capability;
  }
}

function CapabilityIcon({ capability }: { capability: MediaCapabilityKind }) {
  const label = capabilityLabel(capability);
  return (
    <span className={styles['pv-provider-capability-icon']} title={label} aria-label={label}>
      {capability === 'imageGeneration' ? (
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
          <circle cx="8.5" cy="8.5" r="1.5" />
          <path d="M21 15l-5-5L5 21" />
        </svg>
      ) : capability === 'videoGeneration' ? (
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <rect x="3" y="5" width="13" height="14" rx="2" />
          <path d="m16 9 5-3v12l-5-3" />
        </svg>
      ) : (
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M4 10v4" />
          <path d="M8 7v10" />
          <path d="M12 4v16" />
          <path d="M16 8v8" />
          <path d="M20 11v2" />
        </svg>
      )}
    </span>
  );
}

/**
 * Provider 能力图标组：输入是 Registry 的 media_capability_bindings（不做 providerId 猜测），
 * 按 图片 → 视频 → 语音 固定顺序去重显示。
 */
export function MediaCapabilityIcons({ bindings }: { bindings?: ProviderMediaCapabilityBinding[] }) {
  if (!bindings || bindings.length === 0) return null;
  const seen = new Set<MediaCapabilityKind>();
  const capabilities: MediaCapabilityKind[] = [];
  for (const capability of CAPABILITY_ORDER) {
    if (bindings.some(b => b.capability === capability) && !seen.has(capability)) {
      seen.add(capability);
      capabilities.push(capability);
    }
  }
  if (capabilities.length === 0) return null;
  return (
    <span className={styles['pv-provider-capability-icons']}>
      {capabilities.map(capability => (
        <CapabilityIcon key={capability} capability={capability} />
      ))}
    </span>
  );
}

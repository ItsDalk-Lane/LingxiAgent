import React, { useState, useEffect } from 'react';
import { t } from '../../helpers';
import type { UseMediaSettingsDataResult, MediaConfig, MediaProvider } from '../../hooks/useMediaSettingsData';
import { ProviderMediaDefaults } from './ProviderMediaDefaults';
import type { ResolvedMediaCapability } from './provider-media-capabilities';
import styles from '../../Settings.module.css';

/**
 * ProviderMediaDefaultsModal — 「图片/视频生成默认参数」弹窗。
 *
 * 复用现有 ProviderMediaDefaults 表单逻辑（动态参数 schema / mode / provider
 * defaults 解析），只把渲染位置从 ProviderDetail inline 换成 modal body。
 *
 * 一个 capability 可能有多个 runtime binding：按钮仍只有一个，modal 顶部
 * 出现 runtime/provider selector 后再显示对应表单。
 */
export function ProviderMediaDefaultsModal({ capability, capabilities, media, onClose }: {
  capability: 'imageGeneration' | 'videoGeneration';
  capabilities: ResolvedMediaCapability[];
  media: UseMediaSettingsDataResult;
  onClose: () => void;
}) {
  const isVideo = capability === 'videoGeneration';
  const titleKey = isVideo ? 'settings.media.videoDefaultsButton' : 'settings.media.imageDefaultsButton';
  const [runtimeProviderId, setRuntimeProviderId] = useState<string>(capabilities[0]?.runtimeProviderId || '');
  const [selectValue, setSelectValue] = useState<string>(capabilities[0]?.runtimeProviderId || '');

  useEffect(() => {
    if (!capabilities.some(cap => cap.runtimeProviderId === selectValue)) {
      const first = capabilities[0]?.runtimeProviderId || '';
      setSelectValue(first);
      setRuntimeProviderId(first);
    }
  }, [capabilities, selectValue]);

  const active = capabilities.find(cap => cap.runtimeProviderId === runtimeProviderId)
    || capabilities[0];

  return (
    <>
      <div className={styles['pv-defaults-modal-overlay']} onClick={onClose} />
      <div
        className={styles['pv-defaults-modal']}
        role="dialog"
        aria-modal="true"
        aria-label={t(titleKey)}
        data-defaults-modal={capability}
      >
        <div className={styles['pv-defaults-modal-header']}>
          <span className={styles['pv-defaults-modal-title']}>{t(titleKey)}</span>
          <button className={styles['pv-defaults-modal-close']} onClick={onClose} aria-label={t('settings.api.cancel')}>×</button>
        </div>
        {capabilities.length > 1 && (
          <div className={styles['pv-defaults-modal-runtime-picker']}>
            <label className={styles['pv-model-edit-label']}>{t('settings.media.runtimeProvider')}</label>
            <select
              className={styles['pv-custom-category-select']}
              value={selectValue}
              onChange={(e) => {
                setSelectValue(e.target.value);
                setRuntimeProviderId(e.target.value);
              }}
            >
              {capabilities.map(cap => (
                <option key={cap.runtimeProviderId} value={cap.runtimeProviderId}>
                  {cap.provider?.displayName || cap.runtimeProviderId}
                </option>
              ))}
            </select>
          </div>
        )}
        <div className={styles['pv-defaults-modal-body']}>
          {active?.available && active.provider ? (
            <ProviderMediaDefaults
              capability={capability}
              runtimeProviderId={active.runtimeProviderId}
              provider={active.provider as MediaProvider}
              config={(active.config || {}) as MediaConfig}
              defaultModel={(active.config as MediaConfig | null)?.[isVideo ? 'defaultVideoModel' : 'defaultImageModel']}
              onSaveConfig={isVideo ? media.saveVideoConfig : media.saveImageConfig}
            />
          ) : (
            <div className={styles['pv-empty']}>{t('settings.media.runtimeUnavailable')}</div>
          )}
        </div>
      </div>
    </>
  );
}

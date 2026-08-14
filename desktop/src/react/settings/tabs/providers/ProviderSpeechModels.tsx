import React from 'react';
import { t } from '../../helpers';
import {
  getRunnableSpeechModels,
  type SpeechProvider,
  type SpeechConfig,
} from '../../hooks/useMediaSettingsData';
import styles from '../../Settings.module.css';

function textOrFallback(key: string, fallback: string): string {
  const value = t(key);
  return value === key ? fallback : value;
}

export function ProviderSpeechModels({ runtimeProviderId, provider, config }: {
  runtimeProviderId: string;
  provider: SpeechProvider;
  config: SpeechConfig | null;
}) {
  const runnableModels = getRunnableSpeechModels(provider);
  const isDefault = (modelId: string) =>
    config?.defaultModel?.id === modelId && config.defaultModel.provider === runtimeProviderId;

  return (
    <div className={styles['pv-models']}>
      <div className={styles['pv-fav-section']}>
        <div className={styles['pv-fav-title']}>
          {textOrFallback('settings.media.speechModels', '转录模型')}
          <span className={styles['pv-models-count']}>{runnableModels.length}</span>
        </div>
        {runnableModels.length > 0 ? (
          <div className={styles['pv-fav-list']}>
            {runnableModels.map(model => (
              <div key={model.id} className={styles['pv-fav-item']}>
                <span className={styles['pv-fav-item-name']} title={model.id}>{model.name || model.id}</span>
                <span className={styles['pv-fav-item-id']}>{model.id}</span>
                {isDefault(model.id) && (
                  <span className={styles['settings-default-badge']}>
                    {t('settings.media.default')}
                  </span>
                )}
              </div>
            ))}
          </div>
        ) : (
          <div className={styles['pv-empty']}>{t('settings.media.noProvider')}</div>
        )}
      </div>
    </div>
  );
}

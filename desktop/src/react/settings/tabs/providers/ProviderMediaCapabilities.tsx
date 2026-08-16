import React from 'react';
import { t } from '../../helpers';
import type { ProviderMediaCapabilityBinding } from '../../store';
import type {
  MediaProvider,
  SpeechProvider,
  MediaConfig,
  SpeechConfig,
  UseMediaSettingsDataResult,
} from '../../hooks/useMediaSettingsData';
import { resolveProviderMediaCapabilities } from './provider-media-capabilities';
import { ProviderMediaModels } from './ProviderMediaModels';
import { ProviderMediaDefaults } from './ProviderMediaDefaults';
import { ProviderSpeechModels } from './ProviderSpeechModels';
import styles from '../../Settings.module.css';

function imageVideoStatusMessage(provider: MediaProvider): string {
  return provider.unavailableMessage
    || provider.runtimeCapability?.error?.message
    || provider.unavailableReason
    || t('settings.media.credentialMissing');
}

export function ProviderMediaCapabilities({ bindings, media }: {
  bindings?: ProviderMediaCapabilityBinding[];
  media: UseMediaSettingsDataResult;
}) {
  const resolved = resolveProviderMediaCapabilities(bindings, media);

  return (
    <>
      {resolved.map((cap) => {
        // 凭据正常时不渲染状态行；异常/加载态仍需可见（禁止静默降级）。
        const credentialOk = cap.available && !!cap.provider?.hasCredentials;

        return (
          <div key={`${cap.capability}:${cap.runtimeProviderId}`} className={styles['media-capability-section']}>
            {!credentialOk && (
              <div className={styles['settings-credential-status']}>
                <span className={styles['settings-credential-dot']} />
                {cap.available && cap.provider
                  ? (cap.capability === 'speechRecognition'
                    ? t('settings.media.credentialMissing')
                    : imageVideoStatusMessage(cap.provider as MediaProvider))
                  : cap.loading
                    ? t('common.loading')
                    : t('settings.media.runtimeUnavailable')}
              </div>
            )}

            {cap.available && cap.capability !== 'speechRecognition' ? (
              <>
                <ProviderMediaModels
                  capability={cap.capability}
                  runtimeProviderId={cap.runtimeProviderId}
                  provider={cap.provider as MediaProvider}
                  defaultModel={(cap.config as MediaConfig)?.[cap.capability === 'videoGeneration' ? 'defaultVideoModel' : 'defaultImageModel']}
                  onRefresh={cap.capability === 'videoGeneration' ? media.refreshVideo : media.refreshImage}
                />
                <ProviderMediaDefaults
                  capability={cap.capability}
                  runtimeProviderId={cap.runtimeProviderId}
                  provider={cap.provider as MediaProvider}
                  config={cap.config as MediaConfig}
                  defaultModel={(cap.config as MediaConfig)?.[cap.capability === 'videoGeneration' ? 'defaultVideoModel' : 'defaultImageModel']}
                  onSaveConfig={cap.capability === 'videoGeneration' ? media.saveVideoConfig : media.saveImageConfig}
                />
              </>
            ) : null}

            {cap.available && cap.capability === 'speechRecognition' ? (
              <ProviderSpeechModels
                runtimeProviderId={cap.runtimeProviderId}
                provider={cap.provider as SpeechProvider}
                config={cap.config as SpeechConfig}
                onRefresh={media.refreshSpeech}
              />
            ) : null}

            {!cap.available && !cap.loading ? (
              <div className={styles['pv-empty']}>{t('settings.media.runtimeUnavailable')}</div>
            ) : null}
          </div>
        );
      })}
    </>
  );
}

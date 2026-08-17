import React from 'react';
import { t } from '../../helpers';
import { SettingsSection } from '../../components/SettingsSection';
import { SettingsRow } from '../../components/SettingsRow';
import { SelectWidget, Toggle } from '@/ui';
import { useMediaSettingsData, LOADING_SELECT_VALUE } from '../../hooks/useMediaSettingsData';
import type { MediaConfig, MediaProvider, SpeechConfig } from '../../hooks/useMediaSettingsData';

function textOrFallback(key: string, fallback: string): string {
  const value = t(key);
  return value === key ? fallback : value;
}

interface DefaultsModelOption {
  value: string;
  label: string;
  disabled?: boolean;
}

function defaultModelValue(ready: boolean, config: { default?: { id: string; provider: string } } | null) {
  if (!ready) return LOADING_SELECT_VALUE;
  if (config?.default) return `${config.default.provider}/${config.default.id}`;
  return '';
}

function buildDefaultModelOptions<T extends { id: string; name?: string; adapterAvailable?: boolean }>(args: {
  ready: boolean;
  configDefault?: { id: string; provider: string };
  models: Array<T & { provider: string }>;
  providers: Record<string, { hasCredentials?: boolean; unavailableMessage?: string | null; unavailableReason?: string | null }>;
  adapterMissingKey: string;
  credentialMissingKey: string;
}): DefaultsModelOption[] {
  const { ready, configDefault, models, providers, adapterMissingKey, credentialMissingKey } = args;
  const currentValue = configDefault ? `${configDefault.provider}/${configDefault.id}` : '';
  const staleOption: DefaultsModelOption[] = ready && configDefault && !models.some(m => `${m.provider}/${m.id}` === currentValue)
    ? [{ value: currentValue, label: `${configDefault.provider} / ${configDefault.id}`, disabled: true }]
    : [];
  const modelOptions: DefaultsModelOption[] = ready
    ? models.map(m => {
        const providerHasCredentials = providers[m.provider]?.hasCredentials === true;
        const adapterAvailable = m.adapterAvailable !== false;
        const label = `${m.provider} / ${m.name || m.id}`;
        const unavailableReason = !providerHasCredentials
          ? providers[m.provider]?.unavailableMessage
            || providers[m.provider]?.unavailableReason
            || t(credentialMissingKey)
          : !adapterAvailable
            ? t(adapterMissingKey)
            : '';
        return {
          value: `${m.provider}/${m.id}`,
          label: unavailableReason ? `${label} (${unavailableReason})` : label,
          disabled: !providerHasCredentials || !adapterAvailable,
        };
      })
    : [];
  return [
    ...(ready ? [{ value: '', label: '—' }] : [{ value: LOADING_SELECT_VALUE, label: t('common.loading'), disabled: true }]),
    ...staleOption,
    ...modelOptions,
  ];
}

export function MediaGlobalDefaultsSection() {
  const {
    image, video, speech,
    allImageModels, allVideoModels, allSpeechModels, speechEnabled,
    saveImageConfig, saveVideoConfig, saveSpeechConfig,
  } = useMediaSettingsData();

  const imageConfigReady = !image.loading && image.config !== null;
  const videoConfigReady = !video.loading && video.config !== null;
  const speechConfigReady = !speech.loading && speech.config !== null;

  const imageConfig = image.config as MediaConfig | null;
  const videoConfig = video.config as MediaConfig | null;
  const speechConfig = speech.config as SpeechConfig | null;

  const imageDefaultValue = defaultModelValue(imageConfigReady, imageConfig?.defaultImageModel ? { default: imageConfig.defaultImageModel } : null);
  const videoDefaultValue = defaultModelValue(videoConfigReady, videoConfig?.defaultVideoModel ? { default: videoConfig.defaultVideoModel } : null);
  const speechDefaultValue = defaultModelValue(speechConfigReady, speechConfig?.defaultModel ? { default: speechConfig.defaultModel } : null);

  const speechRecognitionEnabledLabel = textOrFallback('settings.media.speechRecognitionEnabled', '发送语音条时转录');
  const defaultSpeechModelLabel = textOrFallback('settings.media.defaultSpeechModel', '语音条转录模型');

  const saveImageDefault = (val: string) => {
    if (val === LOADING_SELECT_VALUE) return;
    if (!val) { void saveImageConfig({ defaultImageModel: undefined }); return; }
    const [provider, ...rest] = val.split('/');
    void saveImageConfig({ defaultImageModel: { id: rest.join('/'), provider } });
  };

  const saveVideoDefault = (val: string) => {
    if (val === LOADING_SELECT_VALUE) return;
    if (!val) { void saveVideoConfig({ defaultVideoModel: undefined }); return; }
    const [provider, ...rest] = val.split('/');
    void saveVideoConfig({ defaultVideoModel: { provider, id: rest.join('/') } });
  };

  const saveSpeechDefault = (val: string) => {
    if (val === LOADING_SELECT_VALUE) return;
    if (!val) { void saveSpeechConfig({ defaultModel: undefined }); return; }
    const [provider, ...rest] = val.split('/');
    void saveSpeechConfig({ defaultModel: { id: rest.join('/'), provider } });
  };

  return (
    <SettingsSection title={t('settings.media.globalDefault')}>
      <SettingsRow
        label={t('settings.media.defaultModel')}
        control={
          <SelectWidget
            value={imageDefaultValue}
            onChange={saveImageDefault}
            disabled={!imageConfigReady}
            options={buildDefaultModelOptions({
              ready: imageConfigReady,
              configDefault: imageConfig?.defaultImageModel,
              models: allImageModels,
              providers: image.providers as Record<string, MediaProvider>,
              adapterMissingKey: 'settings.media.adapterMissing',
              credentialMissingKey: 'settings.media.credentialMissing',
            })}
          />
        }
      />
      <SettingsRow
        label={t('settings.media.defaultVideoModel')}
        control={
          <SelectWidget
            value={videoDefaultValue}
            onChange={saveVideoDefault}
            disabled={!videoConfigReady}
            options={buildDefaultModelOptions({
              ready: videoConfigReady,
              configDefault: videoConfig?.defaultVideoModel,
              models: allVideoModels,
              providers: video.providers as Record<string, MediaProvider>,
              adapterMissingKey: 'settings.media.videoAdapterMissing',
              credentialMissingKey: 'settings.media.credentialMissing',
            })}
          />
        }
      />
      <SettingsRow
        label={speechRecognitionEnabledLabel}
        control={
          <Toggle
            ariaLabel={speechRecognitionEnabledLabel}
            on={speechConfig ? speechEnabled : undefined}
            onChange={(enabled) => { void saveSpeechConfig({ enabled }); }}
          />
        }
      />
      <SettingsRow
        label={defaultSpeechModelLabel}
        control={
          <SelectWidget
            value={speechDefaultValue}
            onChange={saveSpeechDefault}
            disabled={!speechConfigReady || !speechEnabled || (allSpeechModels.length === 0 && !speechConfig?.defaultModel)}
            options={[
              ...(speechConfigReady ? [{ value: '', label: '—' }] : [{ value: LOADING_SELECT_VALUE, label: t('common.loading'), disabled: true }]),
              ...(speechConfigReady && speechConfig?.defaultModel && !allSpeechModels.some(m => `${m.provider}/${m.id}` === speechDefaultValue)
                ? [{
                    value: speechDefaultValue,
                    label: `${speechConfig.defaultModel.provider} / ${speechConfig.defaultModel.id}`,
                    disabled: true,
                  }]
                : []),
              ...(speechConfigReady && speechEnabled ? allSpeechModels.map(m => ({
                value: `${m.provider}/${m.id}`,
                label: `${m.provider} / ${m.name || m.id}`,
              })) : []),
            ]}
          />
        }
      />
    </SettingsSection>
  );
}

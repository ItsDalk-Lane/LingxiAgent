import React, { useState } from 'react';
import { useSettingsStore, type ProviderSummary } from '../../store';
import { lingxiFetchJson } from '../../api';
import { invalidateConfigCache } from '../../../hooks/use-config';
import { t } from '../../helpers';
import { useMediaSettingsData } from '../../hooks/useMediaSettingsData';
import { OAuthCredentials } from './OAuthCredentials';
import { ApiKeyCredentials } from './ApiKeyCredentials';
import { ProviderModelList } from './ProviderModelList';
import styles from '../../Settings.module.css';

export function ProviderDetail({ providerId, summary, providerConfig, isPresetSetup, presetInfo, onRemoveDraft, onRefresh }: {
  providerId: string;
  summary: ProviderSummary;
  providerConfig?: Record<string, unknown>;
  isPresetSetup?: boolean;
  presetInfo?: { label: string; value: string; url?: string; api?: string; local?: boolean };
  onRemoveDraft?: () => void;
  onRefresh: () => Promise<void>;
}) {
  const media = useMediaSettingsData();

  // Chat 模型变化：刷新 settingsConfig + provider summary。
  const onProviderRefresh = onRefresh;
  // 凭据/OAuth 状态变化：额外刷新 image/video/speech，让能力状态立即可见。
  const onCredentialRefresh = async () => {
    await onRefresh();
    await media.refreshAll();
  };

  return (
    <div className={styles['pv-detail-inner']}>
      <div className={styles['pv-detail-header']}>
        <h2 className={styles['pv-detail-title']}>{summary.display_name || providerId}</h2>
        {onRemoveDraft ? (
          <button className={styles['pv-delete-btn']} onClick={onRemoveDraft}>
            {t('settings.providers.delete')}
          </button>
        ) : summary.can_delete && !isPresetSetup && (
          <ProviderDeleteButton providerId={providerId} onRefresh={onRefresh} />
        )}
      </div>
      {summary.config_status === 'invalid' && (
        <div className={styles['pv-config-alert']}>
          {t('settings.providers.configInvalid')}
        </div>
      )}
      {summary.config_status === 'needs_setup' && summary.can_delete && !summary.config_error && (
        <div className={styles['pv-config-alert']}>
          {t('settings.providers.configMissingFields', {
            fields: (summary.missing_fields || [])
              .map((field) => (field === 'api_key' ? 'API Key'
                : field === 'base_url' ? 'Base URL'
                : field === 'models' ? t('settings.api.addedModels')
                : field))
              .join('、'),
          })}
        </div>
      )}
      {summary.supports_oauth ? (
        <OAuthCredentials providerId={providerId} summary={summary} onRefresh={onCredentialRefresh} />
      ) : (
        <ApiKeyCredentials
          providerId={providerId}
          summary={summary}
          providerConfig={providerConfig}
          isPresetSetup={isPresetSetup}
          presetInfo={presetInfo}
          onRefresh={onCredentialRefresh}
        />
      )}
      {/* 统一模型区：chat / image / video / speech recognition 的
          已添加列表 + 添加/读取 + 默认参数弹窗 + 编辑弹窗都在这里 */}
      <ProviderModelList providerId={providerId} summary={summary} media={media} onRefresh={onProviderRefresh} />
    </div>
  );
}

function ProviderDeleteButton({ providerId, onRefresh }: { providerId: string; onRefresh: () => Promise<void> }) {
  const showToast = useSettingsStore(s => s.showToast);
  const [confirming, setConfirming] = useState(false);

  const handleDelete = async () => {
    try {
      await lingxiFetchJson('/api/config', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ providers: { [providerId]: null } }),
      });
      invalidateConfigCache();
      await onRefresh();
      useSettingsStore.setState({ selectedProviderId: null });
      setConfirming(false);
      showToast(t('settings.providers.deleted', { name: providerId }), 'success');
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      showToast(t('settings.saveFailed') + ': ' + msg, 'error');
    }
  };

  return (
    <>
      <button className={styles['pv-delete-btn']} onClick={() => setConfirming(true)}>
        {t('settings.providers.delete')}
      </button>
      {confirming && (
        <>
          <div className={styles['pv-model-edit-overlay']} onClick={() => setConfirming(false)} />
          <div className={styles['pv-confirm-dialog']}>
            <p className={styles['pv-confirm-text']}>
              {t('settings.providers.deleteConfirm', { name: providerId })}
            </p>
            <div className={styles['pv-confirm-actions']}>
              <button className={styles['pv-add-form-btn']} onClick={() => setConfirming(false)}>{t('settings.api.cancel')}</button>
              <button className={`${styles['pv-add-form-btn']} ${styles['danger']}`} onClick={handleDelete}>{t('settings.providers.delete')}</button>
            </div>
          </div>
        </>
      )}
    </>
  );
}

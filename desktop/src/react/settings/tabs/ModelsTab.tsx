import React, { useMemo } from 'react';
import { useSettingsStore } from '../store';
import { t } from '../helpers';
import { SettingsSection } from '../components/SettingsSection';
import { AuxiliaryModelsSection } from './providers/AuxiliaryModelsSection';
import { MediaGlobalDefaultsSection } from './models/MediaGlobalDefaultsSection';
import styles from '../Settings.module.css';

export function ModelsTab() {
  const settingsConfig = useSettingsStore(s => s.settingsConfig);
  const providers = useMemo<Record<string, Record<string, unknown>>>(
    () => settingsConfig?.providers || {},
    [settingsConfig?.providers],
  );

  return (
    <div className={`${styles['settings-tab-content']} ${styles['active']}`} data-tab="models">
      <SettingsSection title={t('settings.models.auxiliary')}>
        <AuxiliaryModelsSection providers={providers as Record<string, { models?: string[]; base_url?: string }>} />
      </SettingsSection>
      <MediaGlobalDefaultsSection />
    </div>
  );
}

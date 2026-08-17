import React from 'react';
import { useSettingsStore } from '../../store';
import { t, autoSaveGlobalModels } from '../../helpers';
import { SelectWidget } from '@/ui';
import styles from '../../Settings.module.css';
import {
  AUTO_SEARCH_PROVIDER,
  isFreeSearchApiProvider,
  isBrowserSearchProvider,
} from '../../../../../../shared/search-providers.ts';

export function SearchProviderSection() {
  const globalModelsConfig = useSettingsStore(s => s.globalModelsConfig);
  const searchProvider = globalModelsConfig?.search?.provider || AUTO_SEARCH_PROVIDER;

  return (
    <div className={styles['pv-model-config-row']}>
      <div className={styles['pv-model-config-label']}>
        <span className={styles['pv-model-config-title']}>{t('settings.api.searchProviderField')}</span>
      </div>
      <div className={styles['pv-model-config-control']}>
        <SelectWidget
          className={styles['pv-model-config-select']}
          options={[
            { value: AUTO_SEARCH_PROVIDER, label: 'Auto (Paid API -> AnySearch free -> Browser)' },
            { value: 'anysearch', label: 'AnySearch' },
            { value: 'anysearch_free', label: 'AnySearch (free)' },
            { value: 'bing_browser', label: 'Bing (Browser)' },
            { value: 'google_browser', label: 'Google (Browser)' },
            { value: 'duckduckgo_browser', label: 'DuckDuckGo (Browser)' },
            { value: 'tavily', label: 'Tavily' },
            { value: 'brave', label: 'Brave Search' },
            { value: 'serper', label: 'Serper (Google)' },
          ]}
          value={searchProvider}
          onChange={(val) => {
            const keyless = val === AUTO_SEARCH_PROVIDER || isBrowserSearchProvider(val) || isFreeSearchApiProvider(val);
            autoSaveGlobalModels({ search: keyless ? { provider: val, api_key: '' } : { provider: val } });
          }}
          placeholder={t('settings.api.searchProviderField')}
        />
      </div>
    </div>
  );
}

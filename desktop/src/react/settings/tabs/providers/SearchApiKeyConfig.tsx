import React, { useState, useEffect } from 'react';
import { useSettingsStore } from '../../store';
import { lingxiFetch } from '../../api';
import { t } from '../../helpers';
import { loadSettingsConfig } from '../../actions';
import { KeyInput } from '../../widgets/KeyInput';
import styles from '../../Settings.module.css';
import {
  AUTO_SEARCH_PROVIDER,
  SEARCH_API_PROVIDER_IDS,
  isFreeSearchApiProvider,
  isBrowserSearchProvider,
  isSearchApiProvider,
  normalizeSearchApiKeys,
} from '../../../../../../shared/search-providers.ts';

const SEARCH_API_PROVIDER_LABELS: Record<string, string> = {
  anysearch: 'AnySearch',
  tavily: 'Tavily',
  brave: 'Brave Search',
  serper: 'Serper (Google)',
};

function searchProviderNeedsApiKey(provider: string): boolean {
  return isSearchApiProvider(provider);
}

export function SearchApiKeyConfig() {
  const globalModelsConfig = useSettingsStore(s => s.globalModelsConfig);
  const showToast = useSettingsStore(s => s.showToast);
  const savedSearchApiKeys = normalizeSearchApiKeys(globalModelsConfig?.search?.api_keys || {});
  const savedLegacySearchKey = globalModelsConfig?.search?.api_key || '';
  const [searchApiKeys, setSearchApiKeys] = useState<Record<string, string>>({});
  const [searchKeyEdited, setSearchKeyEdited] = useState<Record<string, boolean>>({});

  // 从后端同步已保存的 key
  useEffect(() => {
    setSearchApiKeys((prev) => {
      const next = { ...prev };
      for (const provider of SEARCH_API_PROVIDER_IDS) {
        if (searchKeyEdited[provider]) continue;
        const legacyForSelectedProvider = globalModelsConfig?.search?.provider === provider ? savedLegacySearchKey : '';
        next[provider] = savedSearchApiKeys[provider] || legacyForSelectedProvider || '';
      }
      return next;
    });
  }, [globalModelsConfig?.search?.provider, savedLegacySearchKey, JSON.stringify(savedSearchApiKeys), searchKeyEdited]);

  const searchProvider = globalModelsConfig?.search?.provider || AUTO_SEARCH_PROVIDER;
  const searchIsAutoProvider = searchProvider === AUTO_SEARCH_PROVIDER;
  const searchIsKeylessProvider = isBrowserSearchProvider(searchProvider) || isFreeSearchApiProvider(searchProvider);
  const explicitSearchApiProvider = searchProviderNeedsApiKey(searchProvider) ? searchProvider : '';

  const verifySearch = async (provider: string) => {
    const apiKey = (searchApiKeys[provider] || '').trim();
    if (!provider) { showToast(t('settings.search.noProvider'), 'error'); return; }
    if (searchProviderNeedsApiKey(provider) && !apiKey) { showToast(t('settings.search.noKey'), 'error'); return; }
    try {
      const res = await lingxiFetch('/api/search/verify', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ provider, api_key: apiKey, search_provider: searchProvider }),
      });
      const data = await res.json();
      if (data.ok) {
        showToast(t('settings.search.verified'), 'success');
        setSearchKeyEdited((prev) => ({ ...prev, [provider]: false }));
        await loadSettingsConfig();
      } else {
        showToast(t('settings.search.verifyFailed') + (data.error ? ': ' + data.error : ''), 'error');
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      showToast(t('settings.saveFailed') + ': ' + msg, 'error');
    }
  };

  const updateSearchApiKey = (provider: string, value: string) => {
    setSearchApiKeys((prev) => ({ ...prev, [provider]: value }));
    setSearchKeyEdited((prev) => ({ ...prev, [provider]: true }));
  };

  const renderSearchApiKeyRow = (provider: string) => (
    <div className={styles['search-api-key-row']} key={provider}>
      <span className={styles['search-api-key-label']}>{SEARCH_API_PROVIDER_LABELS[provider] || provider}</span>
      <div className={styles['search-api-key-controls']}>
        <KeyInput
          value={searchApiKeys[provider] || ''}
          onChange={(v) => updateSearchApiKey(provider, v)}
          placeholder={t('settings.api.apiKeyPlaceholder')}
        />
        <button className={styles['search-verify-btn']} onClick={() => verifySearch(provider)}>
          {t('settings.search.verify')}
        </button>
      </div>
    </div>
  );

  return (
    <>
      {!searchIsKeylessProvider && (
        <div className={styles['search-api-key-list']}>
          {searchIsAutoProvider
            ? SEARCH_API_PROVIDER_IDS.map((provider) => renderSearchApiKeyRow(provider))
            : explicitSearchApiProvider && renderSearchApiKeyRow(explicitSearchApiProvider)}
        </div>
      )}
      {searchIsKeylessProvider && (
        <span className={styles['settings-form-hint']}>{t('settings.api.searchApiKeyNotRequired')}</span>
      )}
      {searchIsAutoProvider && (
        <span className={styles['settings-form-hint']}>{t('settings.api.searchApiKeysAutoHint')}</span>
      )}
      {!searchIsKeylessProvider && !searchIsAutoProvider && (
        <span className={styles['settings-form-hint']}>{t('settings.api.searchApiKeyHint')}</span>
      )}
    </>
  );
}

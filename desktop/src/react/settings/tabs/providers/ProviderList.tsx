import React, { useState } from 'react';
import { useSettingsStore, type ProviderMediaCapabilityBinding } from '../../store';
import { lingxiFetchJson } from '../../api';
import { t, API_FORMAT_OPTIONS } from '../../helpers';
import { loadSettingsConfig } from '../../actions';
import { SelectWidget, ProviderIcon } from '@/ui';
import { KeyInput } from '../../widgets/KeyInput';
import { parseProviderHeaderLines, ProviderHeadersField } from './ProviderHeadersField';
import { MediaCapabilityIcons } from './MediaCapabilityIcons';
import styles from '../../Settings.module.css';

export interface ProviderPickerItem {
  id: string;
  label: string;
  dim?: boolean;
  hasCredentials?: boolean;
  count?: number;
  bindings?: ProviderMediaCapabilityBinding[];
}

export function ProviderPickerOverlay({ items, onSelect, onAddCustom, onCancel }: {
  items: ProviderPickerItem[];
  onSelect: (id: string) => void;
  onAddCustom: () => void;
  onCancel: () => void;
}) {
  return (
    <div className={styles['pv-add-overlay']}>
      <div className={styles['pv-add-overlay-header']}>
        <button className={styles['pv-add-overlay-back']} onClick={onCancel} aria-label={t('settings.api.cancel')}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="15 18 9 12 15 6" />
          </svg>
          <span>{t('settings.api.cancel')}</span>
        </button>
        <div className={styles['pv-add-overlay-title']}>{t('settings.providers.pickTitle')}</div>
      </div>
      {/* 3 列网格：第一格「自定义供应商」，后续服务商按顺序一行三个 */}
      <div className={styles['pv-picker-grid']}>
        <button type="button" className={styles['pv-picker-add-custom']} onClick={onAddCustom}>
          <span className={styles['pv-picker-add-custom-icon']}>+</span>
          <span>{t('settings.providers.addCustom')}</span>
        </button>
        {items.map(item => (
          <button
            key={item.id}
            type="button"
            className={`${styles['pv-picker-item']}${item.dim ? ' ' + styles['dim'] : ''}`}
            onClick={() => onSelect(item.id)}
          >
            <span className={`${styles['pv-status-dot']}${item.hasCredentials ? ' ' + styles['on'] : ''}`} />
            <ProviderIcon provider={item.id} className={styles['pv-list-item-icon']} />
            <span className={styles['pv-picker-item-name']}>{item.label}</span>
            <MediaCapabilityIcons bindings={item.bindings} />
          </button>
        ))}
      </div>
    </div>
  );
}

export function AddProviderOverlay({ onDone, onCancel }: { onDone: () => Promise<void>; onCancel: () => void }) {
  return (
    <div className={styles['pv-add-overlay']}>
      <div className={styles['pv-add-overlay-header']}>
        <button className={styles['pv-add-overlay-back']} onClick={onCancel} aria-label={t('settings.api.cancel')}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="15 18 9 12 15 6" />
          </svg>
          <span>{t('settings.api.cancel')}</span>
        </button>
        <div className={styles['pv-add-overlay-title']}>{t('settings.providers.addCustom')}</div>
      </div>
      <div className={styles['pv-add-overlay-body']}>
        <AddProviderForm onDone={onDone} />
      </div>
    </div>
  );
}

function AddProviderForm({ onDone }: { onDone: () => Promise<void> }) {
  const showToast = useSettingsStore(s => s.showToast);
  const [name, setName] = useState('');
  const [url, setUrl] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [headersText, setHeadersText] = useState('');
  const [api, setApi] = useState('openai-completions');

  const submit = async () => {
    const n = name.trim().toLowerCase();
    const u = url.trim();
    if (!n) { showToast(t('settings.providers.nameRequired'), 'error'); return; }
    if (!u) { showToast(t('settings.providers.urlRequired'), 'error'); return; }
    try {
      const headers = parseProviderHeaderLines(headersText);
      await lingxiFetchJson('/api/config', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ providers: { [n]: {
          base_url: u,
          api_key: apiKey.trim(),
          headers,
          api,
          models: [] as string[],
        } } }),
      });
      await loadSettingsConfig();
      const state = useSettingsStore.getState();
      if (state.settingsConfigStatus === 'error') {
        throw new Error(state.settingsConfigError || t('settings.refreshFailed'));
      }
      useSettingsStore.setState({ selectedProviderId: n });
      await onDone();
      showToast(t('settings.providers.added', { name: n }), 'success');
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      showToast(t('settings.saveFailed') + ': ' + msg, 'error');
    }
  };

  return (
    <div className={styles['pv-add-form']}>
      <div className={styles['pv-add-form-field']}>
        <label className={styles['pv-add-form-label']}>{t('settings.providers.customName')}</label>
        <input className={styles['settings-input']} type="text" value={name} onChange={(e) => setName(e.target.value)} placeholder="my-provider" />
      </div>
      <div className={styles['pv-add-form-field']}>
        <label className={styles['pv-add-form-label']}>Base URL</label>
        <input className={styles['settings-input']} type="text" value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://api.example.com/v1" />
      </div>
      <div className={styles['pv-add-form-field']}>
        <label className={styles['pv-add-form-label']}>{t('settings.api.apiKey')}</label>
        <KeyInput value={apiKey} onChange={setApiKey} placeholder={t('settings.api.apiKeyPlaceholder')} />
      </div>
      <div className={styles['pv-add-form-field']}>
        <label className={styles['pv-add-form-label']}>Headers</label>
        <ProviderHeadersField value={headersText} onChange={setHeadersText} />
      </div>
      <div className={styles['pv-add-form-field']}>
        <label className={styles['pv-add-form-label']}>{t('settings.providers.apiFormat')}</label>
        <SelectWidget options={API_FORMAT_OPTIONS} value={api} onChange={setApi} placeholder="API Format" />
      </div>
      <div className={styles['pv-add-form-actions']}>
        <button className={`${styles['pv-add-form-btn']} ${styles['primary']}`} onClick={submit}>{t('settings.providers.addBtn')}</button>
      </div>
    </div>
  );
}

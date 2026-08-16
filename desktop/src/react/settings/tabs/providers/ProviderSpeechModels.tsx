import React, { useState, useRef, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { useSettingsStore } from '../../store';
import { lingxiFetch } from '../../api';
import { invalidateConfigCache } from '../../../hooks/use-config';
import { t } from '../../helpers';
import { useAnchoredDropdown } from '../../hooks/useAnchoredDropdown';
import type { SpeechProvider, SpeechConfig } from '../../hooks/useMediaSettingsData';
import styles from '../../Settings.module.css';

function textOrFallback(key: string, fallback: string): string {
  const value = t(key);
  return value === key ? fallback : value;
}

export function ProviderSpeechModels({ runtimeProviderId, provider, config, onRefresh }: {
  runtimeProviderId: string;
  provider: SpeechProvider;
  config: SpeechConfig | null;
  onRefresh: () => Promise<void>;
}) {
  const showToast = useSettingsStore(s => s.showToast);
  const isDefault = (modelId: string) =>
    config?.defaultModel?.id === modelId && config.defaultModel.provider === runtimeProviderId;

  const addModel = async (modelId: string) => {
    try {
      const candidate = allCandidates.find(m => m.id === modelId) || { id: modelId };
      await lingxiFetch(`/api/speech-recognition/providers/${encodeURIComponent(runtimeProviderId)}/models`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ model: candidate }),
      });
      invalidateConfigCache();
      setSearch('');
      setDropdownOpen(false);
      await onRefresh();
    } catch (err: any) {
      showToast(err.message || 'Failed', 'error');
    }
  };

  const removeModel = async (modelId: string) => {
    try {
      await lingxiFetch(`/api/speech-recognition/providers/${encodeURIComponent(runtimeProviderId)}/models/${encodeURIComponent(modelId)}`, {
        method: 'DELETE',
      });
      invalidateConfigCache();
      await onRefresh();
    } catch (err: any) {
      showToast(err.message || 'Failed', 'error');
    }
  };

  const [dropdownOpen, setDropdownOpen] = useState(false);
  const [search, setSearch] = useState('');
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const closeDropdown = useCallback(() => setDropdownOpen(false), []);

  const addedModels = provider.models.filter(m => m.adapterAvailable !== false);
  const addedIds = new Set(addedModels.map(m => m.id));
  const allCandidates = [
    ...addedModels.map(m => ({ id: m.id, name: m.displayName || m.name || m.id })),
    // 按 id 求并集：候选目录若仍含已添加 id（旧服务端/缓存数据），不再拼进下拉。
    ...(provider.catalogModels || []).filter(m => !addedIds.has(m.id)),
  ];
  const trimmedSearch = search.trim();
  const query = trimmedSearch.toLowerCase();
  const filtered = query
    ? allCandidates.filter(m => m.id.toLowerCase().includes(query) || m.name.toLowerCase().includes(query))
    : allCandidates;
  const hasExactCandidate = allCandidates.some(m => m.id.toLowerCase() === query);
  const canAddCustom = !!trimmedSearch && !hasExactCandidate && !addedIds.has(trimmedSearch);

  const panelStyle = useAnchoredDropdown({
    open: dropdownOpen,
    triggerRef,
    panelRef,
    onClose: closeDropdown,
    widthOffset: 80,
  });

  return (
    <div className={styles['pv-models']}>
      {addedModels.length > 0 && (
        <div className={styles['pv-fav-section']}>
          <div className={styles['pv-fav-title']}>
            {textOrFallback('settings.media.speechModels', '转录模型')}
            <span className={styles['pv-models-count']}>{addedModels.length}</span>
          </div>
          <div className={styles['pv-fav-list']}>
            {addedModels.map(model => (
              <div key={model.id} className={styles['pv-fav-item']}>
                <span className={styles['pv-fav-item-name']} title={model.id}>{model.displayName || model.name || model.id}</span>
                <span className={styles['pv-fav-item-id']}>{model.id}</span>
                {isDefault(model.id) && (
                  <span className={styles['settings-default-badge']}>
                    {t('settings.media.default')}
                  </span>
                )}
                <div className={styles['pv-fav-item-actions']}>
                  <button className={styles['pv-fav-item-remove']} onClick={() => removeModel(model.id)} title={t('settings.api.removeModel')}>
                    <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
                    </svg>
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      <div className={styles['pv-models-action-row']}>
        <button ref={triggerRef} className={styles['pv-model-dropdown-trigger']} onClick={() => setDropdownOpen(!dropdownOpen)}>
          <span>{t('settings.media.addSpeechModel')}</span>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="6 9 12 15 18 9" />
          </svg>
        </button>
      </div>

      {dropdownOpen && createPortal(
        <div
          className={styles['pv-model-dropdown-panel']}
          ref={panelRef}
          style={panelStyle}
          data-speech-model-dropdown="true"
        >
          <input
            className={styles['pv-model-dropdown-search']}
            type="text"
            placeholder={t('settings.api.searchModel')}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            autoFocus
          />
          <div className={styles['pv-model-dropdown-list']}>
            {filtered.map(m => {
              const isAdded = addedIds.has(m.id);
              return (
                <button
                  key={m.id}
                  className={`${styles['pv-model-dropdown-option']}${isAdded ? ' ' + styles['added'] : ''}`}
                  onClick={() => { if (!isAdded) addModel(m.id); }}
                >
                  <span className={styles['pv-model-dropdown-option-name']}>{m.name || m.id}</span>
                  {isAdded && <span className={styles['pv-model-dropdown-option-check']}>{'\u2713'}</span>}
                </button>
              );
            })}
            {filtered.length === 0 && (
              <div className={styles['pv-model-dropdown-empty']}>{t('settings.providers.noModels')}</div>
            )}
            {canAddCustom && (
              <button
                className={styles['pv-model-dropdown-option']}
                onClick={() => addModel(trimmedSearch)}
              >
                <span className={styles['pv-model-dropdown-option-name']}>{trimmedSearch}</span>
              </button>
            )}
          </div>
        </div>,
        document.body,
      )}
    </div>
  );
}

import React, { useState, useRef, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { useSettingsStore } from '../../store';
import { lingxiFetch } from '../../api';
import { invalidateConfigCache } from '../../../hooks/use-config';
import { t } from '../../helpers';
import { useAnchoredDropdown } from '../../hooks/useAnchoredDropdown';
import type { MediaProvider } from '../../hooks/useMediaSettingsData';
import styles from '../../Settings.module.css';

interface Props {
  capability: 'imageGeneration' | 'videoGeneration';
  runtimeProviderId: string;
  provider: MediaProvider;
  defaultModel?: { id: string; provider: string };
  onRefresh: () => Promise<void>;
}

export function ProviderMediaModels({ capability, runtimeProviderId, provider, defaultModel, onRefresh }: Props) {
  const showToast = useSettingsStore(s => s.showToast);
  const mediaRoute = capability === 'videoGeneration' ? 'video' : 'image';

  const isDefault = (modelId: string) =>
    defaultModel?.id === modelId && defaultModel?.provider === runtimeProviderId;

  const addModel = async (modelId: string) => {
    try {
      const candidate = allModels.find(m => m.id === modelId) || { id: modelId };
      await lingxiFetch(`/api/media/${mediaRoute}/providers/${encodeURIComponent(runtimeProviderId)}/models`, {
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
      await lingxiFetch(`/api/media/${mediaRoute}/providers/${encodeURIComponent(runtimeProviderId)}/models/${encodeURIComponent(modelId)}`, {
        method: 'DELETE',
      });
      invalidateConfigCache();
      await onRefresh();
    } catch (err: any) {
      showToast(err.message || 'Failed', 'error');
    }
  };

  // ── Dropdown state (same pattern as ProviderModelList) ──

  const [dropdownOpen, setDropdownOpen] = useState(false);
  const [search, setSearch] = useState('');
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const closeDropdown = useCallback(() => setDropdownOpen(false), []);

  const addedIds = new Set(provider.models.map(m => m.id));
  // 按 id 求并集：候选目录若仍含已添加 id（旧服务端/缓存数据），不再拼进下拉。
  const allModels = [...provider.models, ...provider.availableModels.filter(m => !addedIds.has(m.id))];
  const trimmedSearch = search.trim();
  const query = trimmedSearch.toLowerCase();
  const filtered = query ? allModels.filter(m => m.id.toLowerCase().includes(query) || (m.name || m.id).toLowerCase().includes(query)) : allModels;
  const hasExactCandidate = allModels.some(m => m.id.toLowerCase() === query);
  const canAddCustom = !!trimmedSearch && !hasExactCandidate && !addedIds.has(trimmedSearch);
  const modelsLabel = capability === 'videoGeneration'
    ? t('settings.media.videoModels')
    : t('settings.media.models');
  const addModelLabel = capability === 'videoGeneration'
    ? t('settings.media.addVideoModel')
    : t('settings.media.addModel');
  const runtimeDiscovered = !!provider.runtimeCapability;

  const panelStyle = useAnchoredDropdown({
    open: dropdownOpen,
    triggerRef,
    panelRef,
    onClose: closeDropdown,
    widthOffset: 80,
  });

  return (
    <div className={styles['pv-models']}>
      {/* Added model list */}
      {provider.models.length > 0 && (
          <div className={styles['pv-fav-section']}>
            <div className={styles['pv-fav-title']}>
            {modelsLabel}
            <span className={styles['pv-models-count']}>{provider.models.length}</span>
          </div>
          <div className={styles['pv-fav-list']}>
            {provider.models.map(m => (
              <div key={m.id} className={styles['pv-fav-item']}>
                <span className={styles['pv-fav-item-name']} title={m.id}>{m.name || m.id}</span>
                <span className={styles['pv-fav-item-id']}>{m.id}</span>
                {isDefault(m.id) && (
                  <span className={styles['settings-default-badge']}>
                    {t('settings.media.default')}
                  </span>
                )}
                {!runtimeDiscovered && (
                  <div className={styles['pv-fav-item-actions']}>
                    <button className={styles['pv-fav-item-remove']} onClick={() => removeModel(m.id)} title={t('settings.api.removeModel')}>
                      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                        <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
                      </svg>
                    </button>
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Add model dropdown */}
      {!runtimeDiscovered && (
        <div className={styles['pv-models-action-row']}>
          <button ref={triggerRef} className={styles['pv-model-dropdown-trigger']} onClick={() => setDropdownOpen(!dropdownOpen)}>
            <span>{addModelLabel}</span>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <polyline points="6 9 12 15 18 9" />
            </svg>
          </button>
        </div>
      )}

      {!runtimeDiscovered && dropdownOpen && createPortal(
        <div
          className={styles['pv-model-dropdown-panel']}
          ref={panelRef}
          style={panelStyle}
          data-media-model-dropdown="true"
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

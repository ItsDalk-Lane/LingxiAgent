import React, { useState, useCallback, useEffect } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { useSettingsStore, type ProviderSummary } from '../store';
import { lingxiFetch } from '../api';
import { t, PROVIDER_PRESETS } from '../helpers';
import { loadSettingsConfig } from '../actions';
import { ProviderDetail } from './providers/ProviderDetail';
import { ProviderPickerOverlay, AddProviderOverlay, type ProviderPickerItem } from './providers/ProviderList';
import { OtherModelsSection } from './providers/OtherModelsSection';
import { SearchApiKeyConfig } from './providers/SearchApiKeyConfig';
import { UsageLedgerSection } from './providers/UsageLedgerSection';
import { SettingsSection } from '../components/SettingsSection';
import { ProviderIcon } from '@/ui';
import styles from '../Settings.module.css';

type ProviderSubTab = 'api' | 'models' | 'usage';

const PROVIDER_SUB_TABS: { key: ProviderSubTab; labelKey: string }[] = [
  { key: 'api', labelKey: 'settings.providers.subtab.api' },
  { key: 'models', labelKey: 'settings.providers.subtab.models' },
  { key: 'usage', labelKey: 'settings.providers.subtab.usage' },
];

export function ProvidersTab() {
  const { providersSummary, selectedProviderId, settingsConfig } = useSettingsStore(
    useShallow(s => ({ providersSummary: s.providersSummary, selectedProviderId: s.selectedProviderId, settingsConfig: s.settingsConfig }))
  );
  const providers = settingsConfig?.providers || {};
  const [addingProvider, setAddingProvider] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);
  // 通过「添加服务商」界面点选加入的服务商（每点一个添加一行）。
  // 初始纳入 store 里残留的 selectedProviderId（如从聊天模型设置跳转而来），
  // 保证左栏与右栏一致，避免「左栏空、右栏却显示配置」。
  const [pickedProviderIds, setPickedProviderIds] = useState<string[]>(() => {
    const seed = useSettingsStore.getState().selectedProviderId;
    return seed ? [seed] : [];
  });
  const [subTab, setSubTab] = useState<ProviderSubTab>('api');

  const loadSummary = useCallback(async () => {
    try {
      const res = await lingxiFetch('/api/providers/summary');
      const data = await res.json();
      useSettingsStore.setState({ providersSummary: data.providers || {} });
    } catch { /* swallow */ }
  }, []);

  useEffect(() => { loadSummary(); }, [loadSummary]);

  const providerIds = Object.keys(providersSummary);
  const presetValues = new Set(PROVIDER_PRESETS.map(p => p.value));
  const selected = selectedProviderId;

  const selectProvider = (id: string) => {
    useSettingsStore.setState({ selectedProviderId: id });
  };

  const providerLabel = (id: string) => {
    const p = providersSummary[id];
    const preset = PROVIDER_PRESETS.find(pr => pr.value === id);
    return preset?.label || p?.display_name || id;
  };

  // 左栏行：用户通过「添加服务商」界面点选加入的服务商（每点一个添加一行）
  const renderPickedRow = (id: string) => {
    const p = providersSummary[id];
    const modelCount = p ? (p.models || []).length : 0;
    return (
      <button
        key={id}
        className={`${styles['pv-list-item']}${selected === id ? ' ' + styles['selected'] : ''}`}
        onClick={() => selectProvider(id)}
      >
        <span className={`${styles['pv-status-dot']}${p?.has_credentials ? ' ' + styles['on'] : ''}`} />
        <ProviderIcon provider={id} className={styles['pv-list-item-icon']} />
        <span className={styles['pv-list-item-name']}>{providerLabel(id)}</span>
        <span className={styles['pv-list-item-count']}>{modelCount}</span>
      </button>
    );
  };

  // 添加服务商界面候选：全部服务商平铺（预设按顺序在前，其次已注册的自定义/registry-only），已配置的带绿点
  const pickerItems: ProviderPickerItem[] = [
    ...PROVIDER_PRESETS.map(p => ({
      id: p.value,
      label: p.label,
      hasCredentials: providersSummary[p.value]?.has_credentials,
      count: (providersSummary[p.value]?.models || []).length,
    })),
    ...providerIds
      .filter(id => !presetValues.has(id))
      .map(id => ({
        id,
        label: providersSummary[id]?.display_name || id,
        hasCredentials: providersSummary[id]?.has_credentials,
        count: (providersSummary[id]?.models || []).length,
      })),
  ];

  const handlePick = (id: string) => {
    setPickedProviderIds(prev => (prev.includes(id) ? prev : [...prev, id]));
    selectProvider(id);
    setPickerOpen(false);
  };

  return (
    <div className={`${styles['settings-tab-content']} ${styles['active']}`} data-tab="providers">
      {/* 子标签页导航 */}
      <div className={styles['provider-sub-tabs']} role="tablist">
        {PROVIDER_SUB_TABS.map(tab => (
          <button
            key={tab.key}
            type="button"
            role="tab"
            aria-selected={subTab === tab.key}
            className={`${styles['provider-sub-tab']}${subTab === tab.key ? ` ${styles.active}` : ''}`}
            onClick={() => setSubTab(tab.key)}
          >
            {t(tab.labelKey)}
          </button>
        ))}
      </div>

      {/* 子标签页内容 */}
      <div className={styles['provider-sub-tab-content']}>
        {subTab === 'api' && (
          <div className={styles['provider-sub-panel']}>
            <SettingsSection variant="double-column">
              <div className={styles['pv-layout']}>
                {/* ── 左栏：添加服务商按钮 + 已添加的服务商行（初始为空） ── */}
                <div className={styles['pv-list']}>
                  <button
                    type="button"
                    className={styles['pv-add-provider-btn']}
                    onClick={() => setPickerOpen(true)}
                  >
                    <span className={styles['pv-add-provider-btn-icon']}>+</span>
                    <span>{t('settings.providers.addService')}</span>
                  </button>

                  {pickedProviderIds.map(renderPickedRow)}
                </div>

                {/* ── 右栏：服务商配置详情（仅显示已点选列表内的服务商，否则空白） ── */}
                <div className={styles['pv-detail']}>
                  {selected && pickedProviderIds.includes(selected) ? (() => {
                    const existing = providersSummary[selected];
                    const preset = PROVIDER_PRESETS.find(p => p.value === selected);
                    const isRegistryOnlySetup = existing?.is_configured === false;
                    const summary: ProviderSummary = existing || {
                      type: 'api-key' as const,
                      auth_type: 'api-key' as const,
                      display_name: preset?.label || selected,
                      base_url: preset?.url || '',
                      api: preset?.api || '',
                      api_key: '',
                      headers: {},
                      models: [],
                      custom_models: [],
                      has_credentials: false,
                      supports_oauth: false,
                      can_delete: false,
                    };
                    return (
                      <ProviderDetail
                        key={selected}
                        providerId={selected}
                        summary={summary}
                        providerConfig={providers[selected]}
                        isPresetSetup={!existing || isRegistryOnlySetup}
                        presetInfo={preset || (isRegistryOnlySetup ? {
                          label: summary.display_name || selected,
                          value: selected,
                          url: summary.base_url,
                          api: summary.api,
                          local: summary.auth_type === 'none',
                        } : undefined)}
                        onRefresh={async () => { await loadSettingsConfig(); await loadSummary(); }}
                      />
                    );
                  })() : null}
                </div>

                {/* 添加服务商选择弹层（3 列网格） */}
                {pickerOpen && (
                  <ProviderPickerOverlay
                    items={pickerItems}
                    onSelect={handlePick}
                    onAddCustom={() => {
                      setPickerOpen(false);
                      setAddingProvider(true);
                    }}
                    onCancel={() => setPickerOpen(false)}
                  />
                )}

                {/* 新建自定义供应商 overlay */}
                {addingProvider && (
                  <AddProviderOverlay
                    onDone={() => { setAddingProvider(false); loadSummary(); }}
                    onCancel={() => setAddingProvider(false)}
                  />
                )}
              </div>
            </SettingsSection>

            {/* 搜索 API 配置（区域标题 + 每行左标题右输入框） */}
            <SettingsSection title={t('settings.api.searchSection')} surface="plain" className={styles['pv-search-config']}>
              <SearchApiKeyConfig />
            </SettingsSection>
          </div>
        )}

        {subTab === 'models' && (
          <div className={styles['provider-sub-panel']}>
            <OtherModelsSection providers={providers} />
          </div>
        )}

        {subTab === 'usage' && (
          <div className={styles['provider-sub-panel']}>
            <UsageLedgerSection />
          </div>
        )}
      </div>
    </div>
  );
}

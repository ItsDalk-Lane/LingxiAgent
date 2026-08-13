import React, { useState, useCallback, useEffect, useMemo } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { useSettingsStore, type ProviderSummary } from '../store';
import { lingxiFetchJson } from '../api';
import { t, PROVIDER_PRESETS } from '../helpers';
import { loadSettingsConfig } from '../actions';
import { ProviderDetail } from './providers/ProviderDetail';
import { ProviderPickerOverlay, AddProviderOverlay, type ProviderPickerItem } from './providers/ProviderList';
import { OtherModelsSection } from './providers/OtherModelsSection';
import { SearchApiKeyConfig } from './providers/SearchApiKeyConfig';
import { SettingsSection } from '../components/SettingsSection';
import { ProviderIcon } from '@/ui';
import styles from '../Settings.module.css';

type ProviderSubTab = 'api' | 'models';

const PROVIDER_SUB_TABS: { key: ProviderSubTab; labelKey: string }[] = [
  { key: 'api', labelKey: 'settings.providers.subtab.api' },
  { key: 'models', labelKey: 'settings.providers.subtab.models' },
];

export function ProvidersTab() {
  const { providersSummary, selectedProviderId, settingsConfig } = useSettingsStore(
    useShallow(s => ({ providersSummary: s.providersSummary, selectedProviderId: s.selectedProviderId, settingsConfig: s.settingsConfig }))
  );
  const providers = useMemo<Record<string, Record<string, unknown>>>(
    () => settingsConfig?.providers || {},
    [settingsConfig?.providers],
  );
  const [addingProvider, setAddingProvider] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);
  // 点击候选只打开当前页面的临时配置入口；真正保存后才进入 Provider Catalog。
  // 持久成员始终从 settingsConfig.providers 恢复，不能由当前选择或点击历史推导。
  const [draftProviderIds, setDraftProviderIds] = useState<string[]>([]);
  const [subTab, setSubTab] = useState<ProviderSubTab>('api');

  const loadSummary = useCallback(async () => {
    const data = await lingxiFetchJson<{ providers?: Record<string, ProviderSummary> }>('/api/providers/summary');
    useSettingsStore.setState({ providersSummary: data.providers || {} });
  }, []);

  useEffect(() => { void loadSummary().catch(() => {}); }, [loadSummary]);

  const refreshProviderState = useCallback(async () => {
    await loadSettingsConfig();
    const state = useSettingsStore.getState();
    if (state.settingsConfigStatus === 'error') {
      throw new Error(state.settingsConfigError || t('settings.refreshFailed'));
    }
    await loadSummary();
  }, [loadSummary]);

  const providerIds = Object.keys(providersSummary);
  const persistedProviderIds = useMemo(() => Object.keys(providers), [providers]);
  const persistedProviderIdSet = useMemo(() => new Set(persistedProviderIds), [persistedProviderIds]);
  const displayedProviderIds = useMemo(
    () => [...persistedProviderIds, ...draftProviderIds.filter(id => !persistedProviderIdSet.has(id))],
    [draftProviderIds, persistedProviderIdSet, persistedProviderIds],
  );
  const presetValues = new Set(PROVIDER_PRESETS.map(p => p.value));
  const selected = selectedProviderId;

  // 首次保存会让 draft 出现在权威目录中，此时只完成状态迁移，不保留双份成员。
  useEffect(() => {
    setDraftProviderIds(previous => {
      const next = previous.filter(id => !persistedProviderIdSet.has(id));
      return next.length === previous.length ? previous : next;
    });
  }, [persistedProviderIdSet]);

  const selectProvider = (id: string) => {
    useSettingsStore.setState({ selectedProviderId: id });
  };

  const providerLabel = (id: string) => {
    const p = providersSummary[id];
    const preset = PROVIDER_PRESETS.find(pr => pr.value === id);
    return preset?.label || p?.display_name || id;
  };

  const renderProviderRow = (id: string) => {
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
    if (!persistedProviderIdSet.has(id)) {
      setDraftProviderIds(prev => (prev.includes(id) ? prev : [...prev, id]));
    }
    selectProvider(id);
    setPickerOpen(false);
  };

  const removeDraftProvider = (id: string) => {
    setDraftProviderIds(previous => previous.filter(providerId => providerId !== id));
    if (useSettingsStore.getState().selectedProviderId === id) {
      useSettingsStore.setState({ selectedProviderId: null });
    }
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
                {/* ── 左栏：Provider Catalog 持久成员 + 当前页面临时草稿 ── */}
                <div className={styles['pv-list']}>
                  <button
                    type="button"
                    className={styles['pv-add-provider-btn']}
                    onClick={() => setPickerOpen(true)}
                  >
                    <span className={styles['pv-add-provider-btn-icon']}>+</span>
                    <span>{t('settings.providers.addService')}</span>
                  </button>

                  {displayedProviderIds.map(renderProviderRow)}
                </div>

                {/* ── 右栏：当前编辑对象；成员集合由上面的独立状态决定 ── */}
                <div className={styles['pv-detail']}>
                  {selected && displayedProviderIds.includes(selected) ? (() => {
                    const existing = providersSummary[selected];
                    const preset = PROVIDER_PRESETS.find(p => p.value === selected);
                    const isRegistryOnlySetup = existing?.is_configured === false;
                    const isDraft = draftProviderIds.includes(selected) && !persistedProviderIdSet.has(selected);
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
                        onRemoveDraft={isDraft ? () => removeDraftProvider(selected) : undefined}
                        presetInfo={preset || (isRegistryOnlySetup ? {
                          label: summary.display_name || selected,
                          value: selected,
                          url: summary.base_url,
                          api: summary.api,
                          local: summary.auth_type === 'none',
                        } : undefined)}
                        onRefresh={refreshProviderState}
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
                    onDone={async () => {
                      // 自定义供应商提交成功后这里才被调用，配置已落盘。
                      // 刷新摘要失败也不能让 overlay 卡住：关闭浮层，让已挂载的
                      // 订阅和下次进入设置时重试加载。
                      try {
                        await loadSummary();
                      } catch {
                        /* loadSummary 已在 catch 里上报；不阻塞关闭 overlay */
                      }
                      setAddingProvider(false);
                    }}
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
      </div>
    </div>
  );
}

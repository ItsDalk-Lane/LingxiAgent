import React, { useState } from 'react';
import { useSettingsStore } from '../../store';
import { lingxiFetch } from '../../api';
import {
  t, lookupModelMeta, formatContext, autoSaveGlobalModels,
} from '../../helpers';
import { Toggle, SelectWidget } from '@/ui';
import { ModelWidget } from '../../widgets/ModelWidget';
import styles from '../../Settings.module.css';
import {
  AUTO_SEARCH_PROVIDER,
  isFreeSearchApiProvider,
  isBrowserSearchProvider,
} from '../../../../../../shared/search-providers.ts';

type ModelRef = { id: string; provider: string };

function ToolModelTestBtn({ modelRef }: { modelRef: unknown }) {
  const [status, setStatus] = useState<'idle' | 'testing' | 'ok' | 'fail'>('idle');

  const ref = typeof modelRef === 'object' && modelRef !== null
    ? {
        id: String((modelRef as any).id || ''),
        provider: String((modelRef as any).provider || ''),
      }
    : { id: String(modelRef || ''), provider: '' };
  const hasRef = !!ref.id;

  const test = async () => {
    if (!hasRef) return;
    setStatus('testing');
    try {
      const res = await lingxiFetch('/api/models/health', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ modelId: ref.id, provider: ref.provider }),
      });
      const data = await res.json();
      setStatus(data.ok ? 'ok' : 'fail');
    } catch {
      setStatus('fail');
    }
    setTimeout(() => setStatus('idle'), 3000);
  };

  if (!hasRef) return null;

  return (
    <button className={`${styles['pv-tool-test-btn']} ${styles[status] || ''}`} onClick={test} disabled={status === 'testing'}>
      {status === 'testing' ? (
        <svg className={styles['spinning']} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="23 4 23 10 17 10" /><polyline points="1 20 1 14 7 14" />
          <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
        </svg>
      ) : status === 'ok' ? (
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="20 6 9 17 4 12" />
        </svg>
      ) : status === 'fail' ? (
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
        </svg>
      ) : (
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" /><polyline points="22 4 12 14.01 9 11.01" />
        </svg>
      )}
    </button>
  );
}

export function OtherModelsSection({ providers }: { providers: Record<string, { models?: string[]; base_url?: string }> }) {
  const globalModelsConfig = useSettingsStore(s => s.globalModelsConfig);

  const utilityVal = toModelRef(globalModelsConfig?.models?.utility);
  const utilityLargeVal = toModelRef(globalModelsConfig?.models?.utility_large);
  const visionVal = toModelRef(globalModelsConfig?.models?.vision);
  const visionAuxiliaryEnabled = globalModelsConfig ? globalModelsConfig.models?.vision_enabled === true : undefined;
  const imageCapableOnly = (model: { input?: string[] }) => (
    Array.isArray(model.input) && model.input.includes('image')
  );
  const searchProvider = globalModelsConfig?.search?.provider || AUTO_SEARCH_PROVIDER;

  return (
    <div className={styles['pv-model-config']}>
      {/* 每行：左侧标题 + 提示，右侧模型选择菜单 */}
      <div className={styles['pv-model-config-row']}>
        <div className={styles['pv-model-config-label']}>
          <span className={styles['pv-model-config-title']}>{t('settings.api.utilityModel')}</span>
          <span className={styles['settings-form-hint']}>{t('settings.api.utilityModelHint')}</span>
        </div>
        <div className={styles['pv-model-config-control']}>
          <div className={styles['pv-tool-model-row']}>
            <ModelWidget
              providers={providers}
              value={utilityVal}
              onSelect={(ref) => {
                autoSaveGlobalModels({ models: { utility: ref } });
              }}
              lookupModelMeta={lookupModelMeta}
              formatContext={formatContext}
            />
            <ToolModelTestBtn modelRef={globalModelsConfig?.models?.utility || ''} />
          </div>
        </div>
      </div>

      <div className={styles['pv-model-config-row']}>
        <div className={styles['pv-model-config-label']}>
          <span className={styles['pv-model-config-title']}>{t('settings.api.utilityLargeModel')}</span>
          <span className={styles['settings-form-hint']}>{t('settings.api.utilityLargeModelHint')}</span>
        </div>
        <div className={styles['pv-model-config-control']}>
          <div className={styles['pv-tool-model-row']}>
            <ModelWidget
              providers={providers}
              value={utilityLargeVal}
              onSelect={(ref) => {
                autoSaveGlobalModels({ models: { utility_large: ref } });
              }}
              lookupModelMeta={lookupModelMeta}
              formatContext={formatContext}
            />
            <ToolModelTestBtn modelRef={globalModelsConfig?.models?.utility_large || ''} />
          </div>
        </div>
      </div>

      <div className={styles['pv-model-config-row']}>
        <div className={styles['pv-model-config-label']}>
          <span className={styles['pv-model-config-title']}>{t('settings.api.visionModel')}</span>
          <span className={styles['settings-form-hint']}>{t('settings.api.visionModelHint')}</span>
          <span className={styles['settings-form-hint']}>{t('settings.api.visionModelMissingHint')}</span>
        </div>
        <div className={styles['pv-model-config-control']}>
          <div className={styles['settings-toggle-row']}>
            <Toggle
              on={visionAuxiliaryEnabled}
              onChange={(on) => {
                autoSaveGlobalModels({ models: { vision_enabled: on } });
              }}
              label={t('settings.api.visionAuxiliaryToggle')}
            />
          </div>
          <div className={styles['pv-tool-model-row']}>
            <ModelWidget
              providers={providers}
              value={visionVal}
              onSelect={(ref) => {
                autoSaveGlobalModels({ models: { vision: ref } });
              }}
              lookupModelMeta={lookupModelMeta}
              formatContext={formatContext}
              filterModel={imageCapableOnly}
            />
            <ToolModelTestBtn modelRef={globalModelsConfig?.models?.vision || ''} />
          </div>
        </div>
      </div>

      {/* 搜索引擎选择：与模型行对齐（左标题，右选择器） */}
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
    </div>
  );
}

// 工具模型配置可能来自老数据。展示层可读裸 id；保存路径必须重新选择成 {id, provider}。
function toModelRef(raw: unknown): ModelRef | null {
  if (!raw) return null;
  if (typeof raw === 'object' && (raw as any).id) {
    return {
      id: String((raw as any).id || ''),
      provider: String((raw as any).provider || ''),
    };
  }
  const s = String(raw || '').trim();
  if (!s) return null;
  const slashIdx = s.indexOf('/');
  if (slashIdx > 0 && slashIdx < s.length - 1) {
    return { provider: s.slice(0, slashIdx), id: s.slice(slashIdx + 1) };
  }
  return { id: s, provider: '' };
}

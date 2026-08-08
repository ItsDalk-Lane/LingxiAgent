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
import {
  AUXILIARY_SLOT_IDS,
  type AuxiliarySlot,
} from '../../../../../../shared/auxiliary-slot-ids.ts';

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
  const imageCapableOnly = (model: { input?: string[] }) => (
    Array.isArray(model.input) && model.input.includes('image')
  );
  const searchProvider = globalModelsConfig?.search?.provider || AUTO_SEARCH_PROVIDER;

  // UI-only metadata keyed by canonical Slot id。Slot 身份来自 shared 单一真理源
  // （shared/auxiliary-slot-ids.ts），不再手写 field 字符串数组；新增第 7 个 Slot 时，
  // 这里的 Record<AuxiliarySlot, ...> 会因缺 key 直接编译失败，强迫 UI 补齐。
  const UI_METADATA: Record<AuxiliarySlot, {
    titleKey: string;
    hintKey: string;
    fallbackKey: string;
    followKey: string;
    imageOnly?: boolean;
  }> = {
    title: { titleKey: 'settings.api.auxTitleModel', hintKey: 'settings.api.auxTitleModelHint', fallbackKey: 'settings.api.auxFallbackChat', followKey: 'settings.api.auxFollowMain' },
    summarize: { titleKey: 'settings.api.auxSummarizeModel', hintKey: 'settings.api.auxSummarizeModelHint', fallbackKey: 'settings.api.auxFallbackChat', followKey: 'settings.api.auxFollowMain' },
    memory: { titleKey: 'settings.api.auxMemoryModel', hintKey: 'settings.api.auxMemoryModelHint', fallbackKey: 'settings.api.auxFallbackChat', followKey: 'settings.api.auxFollowMain' },
    vision: { titleKey: 'settings.api.visionModel', hintKey: 'settings.api.visionModelHint', fallbackKey: 'settings.api.auxFallbackVision', followKey: 'settings.api.auxFollowMain', imageOnly: true },
    approval: { titleKey: 'settings.api.auxApprovalModel', hintKey: 'settings.api.auxApprovalModelHint', fallbackKey: 'settings.api.auxFallbackApproval', followKey: 'settings.api.auxFollowDisabled' },
    guard: { titleKey: 'settings.api.auxGuardModel', hintKey: 'settings.api.auxGuardModelHint', fallbackKey: 'settings.api.auxFallbackGuard', followKey: 'settings.api.auxFollowDisabled' },
  };
  const slots = AUXILIARY_SLOT_IDS.map((id) => ({ field: id, ...UI_METADATA[id] }));

  const visionAuxiliaryEnabled = globalModelsConfig ? globalModelsConfig.models?.vision_enabled === true : undefined;

  return (
    <div className={styles['pv-model-config']}>
      {slots.map((slot) => {
        const val = toModelRef(globalModelsConfig?.models?.[slot.field]);
        const isVision = slot.field === 'vision';
        return (
          <div key={slot.field} className={styles['pv-model-config-row']}>
            <div className={styles['pv-model-config-label']}>
              <span className={styles['pv-model-config-title']}>{t(slot.titleKey)}</span>
              <span className={styles['settings-form-hint']}>{t(slot.hintKey)}</span>
              <span className={styles['settings-form-hint']}>{t(slot.fallbackKey)}</span>
              {isVision && <span className={styles['settings-form-hint']}>{t('settings.api.visionModelMissingHint')}</span>}
            </div>
            <div className={styles['pv-model-config-control']}>
              {isVision && (
                <div className={styles['settings-toggle-row']}>
                  <Toggle
                    on={visionAuxiliaryEnabled}
                    onChange={(on) => {
                      autoSaveGlobalModels({ models: { vision_enabled: on } });
                    }}
                    label={t('settings.api.visionAuxiliaryToggle')}
                  />
                </div>
              )}
              <div className={styles['pv-tool-model-row']}>
                <ModelWidget
                  providers={providers}
                  value={val}
                  followLabel={t(slot.followKey)}
                  onSelect={(ref) => {
                    autoSaveGlobalModels({ models: { [slot.field]: ref } });
                  }}
                  lookupModelMeta={lookupModelMeta}
                  formatContext={formatContext}
                  filterModel={slot.imageOnly ? imageCapableOnly : undefined}
                />
                <ToolModelTestBtn modelRef={globalModelsConfig?.models?.[slot.field] || ''} />
              </div>
            </div>
          </div>
        );
      })}

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

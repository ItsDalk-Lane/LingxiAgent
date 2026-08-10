/**
 * MDW（模型下拉组件）的 React 版本
 * 消费设置窗口共享的 Runtime Model Catalog，按 provider 分组、支持搜索和自定义输入。
 * 下拉列表通过 AnchoredPortal 悬浮渲染（portal 到 body，position: fixed），
 * 内容少时按内容自适应高度，超过统一最大高度时内部滚动。
 */
import React, { useState, useRef, useEffect, useMemo } from 'react';
import { ProviderIcon, AnchoredPortal } from '@/ui';
import { useSettingsStore, type RuntimeModelInfo } from '../store';
import styles from '../Settings.module.css';

interface ModelRef {
  id: string;
  provider: string;
}

interface ModelWidgetProps {
  /** @deprecated 不再使用，保留兼容签名 */
  providers?: Record<string, { models?: string[]; base_url?: string }>;
  value?: ModelRef | null;
  /** 选中模型时传 {id, provider}；选中"跟随主模型/默认"时传 null */
  onSelect: (ref: ModelRef | null) => void;
  placeholder?: string;
  lookupModelMeta?: (id: string) => any;
  formatContext?: (n: number) => string;
  filterModel?: (model: RuntimeModelInfo) => boolean;
  /** 传入时在下拉列表顶部显示一个特殊选项（如"跟随主模型"），点击后 onSelect(null) */
  followLabel?: string;
}

export function ModelWidget({
  value, onSelect,
  placeholder, formatContext, filterModel, followLabel,
}: ModelWidgetProps) {
  const t = window.t || ((k: string) => k);
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState('');
  const [customInput, setCustomInput] = useState('');
  const models = useSettingsStore(state => state.runtimeModels);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setSearch('');
      requestAnimationFrame(() => searchRef.current?.focus());
    }
  }, [open]);

  const query = search.toLowerCase();
  const visibleModels = useMemo(
    () => (filterModel ? models.filter(filterModel) : models),
    [models, filterModel],
  );
  const valueKey = value?.id && value?.provider ? `${value.provider}/${value.id}` : '';
  const selectedModel = valueKey
    ? visibleModels.find(m => m.id === value?.id && m.provider === value?.provider)
    : null;
  const displayValue = selectedModel?.name
    || (value?.id ? (value.provider ? `${value.provider}/${value.id}` : value.id) : '');

  // 按 provider 分组
  const grouped = useMemo(() => {
    const groups: Record<string, RuntimeModelInfo[]> = {};
    for (const m of visibleModels) {
      if (query && !m.id.toLowerCase().includes(query) && !m.name.toLowerCase().includes(query)) continue;
      const g = m.provider || '';
      if (!groups[g]) groups[g] = [];
      groups[g].push(m);
    }
    return groups;
  }, [visibleModels, query]);

  const handleCustomSubmit = () => {
    const val = customInput.trim();
    if (!val) return;
    const slashIdx = val.indexOf('/');
    if (slashIdx <= 0 || slashIdx >= val.length - 1) return;
    const provider = val.slice(0, slashIdx).trim();
    const id = val.slice(slashIdx + 1).trim();
    if (!provider || !id) return;
    onSelect({ id, provider });
    setCustomInput('');
    setOpen(false);
  };

  return (
    <div className={styles['mdw']}>
      <button
        ref={triggerRef}
        className={styles['mdw-trigger']}
        type="button"
        data-open={open}
        onClick={(e) => { e.stopPropagation(); setOpen(!open); }}
      >
        {value?.provider && (
          <ProviderIcon provider={value.provider} className={styles['mdw-provider-icon']} />
        )}
        <span className={styles['mdw-value']}>{displayValue || (followLabel && !valueKey ? followLabel : `— ${placeholder || t('settings.api.selectModel')} —`)}</span>
        <span className={styles['mdw-arrow']}>▾</span>
      </button>
      <AnchoredPortal
        open={open}
        anchorRef={triggerRef}
        align="start"
        offset={4}
        minWidth={280}
        className={styles['mdw-popup']}
        onClose={() => setOpen(false)}
        role="listbox"
      >
        <input
          ref={searchRef}
          className={styles['mdw-search']}
          type="text"
          placeholder={t('settings.api.searchModel')}
          spellCheck={false}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          onClick={(e) => e.stopPropagation()}
        />
        <div className={styles['mdw-options']}>
          {followLabel && (
            <button
              className={`${styles['mdw-option']} ${styles['mdw-option-follow']}${!valueKey ? ' ' + styles['selected'] : ''}`}
              type="button"
              onClick={() => { onSelect(null); setOpen(false); }}
            >
              <span className={styles['mdw-option-name']}>{followLabel}</span>
            </button>
          )}
          {Object.entries(grouped).map(([provider, items]) => (
            <div key={provider || '__none'}>
              {provider && <div className={styles['mdw-group-header']}>{provider}</div>}
              {items.map(m => (
                <button
                  key={`${m.provider}/${m.id}`}
                  className={`${styles['mdw-option']}${`${m.provider}/${m.id}` === valueKey ? ' ' + styles['selected'] : ''}`}
                  type="button"
                  onClick={() => { onSelect({ id: m.id, provider: m.provider }); setOpen(false); }}
                >
                  <span className={styles['mdw-option-name']}>{m.name || m.id}</span>
                  {m.contextWindow && formatContext && (
                    <span className={styles['mdw-option-ctx']}>{formatContext(m.contextWindow)}</span>
                  )}
                </button>
              ))}
            </div>
          ))}
          <div className={styles['mdw-custom-row']}>
            <input
              type="text"
              className={styles['mdw-custom-input']}
              placeholder={t('settings.api.customInput')}
              spellCheck={false}
              value={customInput}
              onChange={(e) => setCustomInput(e.target.value)}
              onClick={(e) => e.stopPropagation()}
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleCustomSubmit();
                e.stopPropagation();
              }}
            />
            <button
              type="button"
              className={styles['mdw-custom-confirm']}
              onClick={(e) => { e.stopPropagation(); handleCustomSubmit(); }}
            >
              ↵
            </button>
          </div>
        </div>
      </AnchoredPortal>
    </div>
  );
}

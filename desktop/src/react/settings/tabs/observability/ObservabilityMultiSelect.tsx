/**
 * ObservabilityMultiSelect.tsx — Filter Bar 内部的多选下拉（Phase 9 §十七～二十四）。
 *
 * 场景：provider/model/category/status 等多值过滤。SelectWidget 是单选，
 * 不满足「字段内 OR」的多值语义，故用 checkbox 列表自建（纯 ui 组件组合，
 * 不引入新依赖）。键盘可用：trigger 是 button、选项是原生 checkbox。
 */
import React, { useCallback, useEffect, useRef, useState } from 'react';
import styles from '../../Settings.module.css';

export type ObservabilityMultiSelectOption = {
  value: string;
  label: string;
  title?: string;
};

type Props = {
  label: string;
  options: ObservabilityMultiSelectOption[];
  values: string[];
  onChange: (next: string[]) => void;
  /** 选项加载中（facet query in flight）——打开时显示加载占位。 */
  loading?: boolean;
  disabled?: boolean;
  /** 打开时回调（facet 懒加载触发点，§二十六）。 */
  onOpen?: () => void;
  emptyLabel: string;
  loadingLabel: string;
};

export function ObservabilityMultiSelect({
  label,
  options,
  values,
  onChange,
  loading = false,
  disabled = false,
  onOpen,
  emptyLabel,
  loadingLabel,
}: Props) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  const close = useCallback(() => setOpen(false), []);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) close();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') close();
    };
    document.addEventListener('mousedown', onPointerDown, true);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('mousedown', onPointerDown, true);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [open, close]);

  const toggleValue = useCallback((value: string) => {
    onChange(values.includes(value) ? values.filter((item) => item !== value) : [...values, value]);
  }, [onChange, values]);

  const handleTriggerClick = useCallback(() => {
    if (disabled) return;
    setOpen((prev) => {
      const next = !prev;
      if (next) onOpen?.();
      return next;
    });
  }, [disabled, onOpen]);

  return (
    <div className={styles['observability-multiselect']} ref={rootRef}>
      <button
        type="button"
        className={styles['observability-filter-trigger']}
        aria-haspopup="listbox"
        aria-expanded={open}
        disabled={disabled}
        onClick={handleTriggerClick}
      >
        <span className={styles['observability-filter-trigger-label']}>{label}</span>
        {values.length > 0 && (
          <span className={styles['observability-filter-count']}>{values.length}</span>
        )}
        <span className={styles['observability-filter-caret']} aria-hidden>▾</span>
      </button>
      {open && (
        <div className={styles['observability-multiselect-panel']} role="listbox" aria-multiselectable>
          {loading && options.length === 0 && (
            <div className={styles['observability-multiselect-hint']}>{loadingLabel}</div>
          )}
          {!loading && options.length === 0 && (
            <div className={styles['observability-multiselect-hint']}>{emptyLabel}</div>
          )}
          {options.map((option) => (
            <label
              key={option.value}
              className={styles['observability-multiselect-option']}
              title={option.title}
            >
              <input
                type="checkbox"
                checked={values.includes(option.value)}
                onChange={() => toggleValue(option.value)}
              />
              <span className={styles['observability-multiselect-option-label']}>{option.label}</span>
            </label>
          ))}
        </div>
      )}
    </div>
  );
}

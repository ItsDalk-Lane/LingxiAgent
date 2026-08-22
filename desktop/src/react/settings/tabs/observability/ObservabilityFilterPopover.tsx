/**
 * ObservabilityFilterPopover.tsx — Filter Bar 通用弹层（日期/更多过滤/Group By）。
 *
 * button trigger + 绝对定位面板；outside click / Escape 关闭；面板内容由
 * children(renderProp) 提供并拿到 close()。纯展示组件，无网络。
 */
import React, { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import styles from '../../Settings.module.css';

type Props = {
  label: ReactNode;
  /** 已选数量徽标（>0 才显示）。 */
  count?: number;
  disabled?: boolean;
  onOpen?: () => void;
  children: (close: () => void) => ReactNode;
};

export function ObservabilityFilterPopover({ label, count = 0, disabled = false, onOpen, children }: Props) {
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
        aria-haspopup="dialog"
        aria-expanded={open}
        disabled={disabled}
        onClick={handleTriggerClick}
      >
        <span className={styles['observability-filter-trigger-label']}>{label}</span>
        {count > 0 && <span className={styles['observability-filter-count']}>{count}</span>}
        <span className={styles['observability-filter-caret']} aria-hidden>▾</span>
      </button>
      {open && (
        <div className={styles['observability-popover-panel']} role="dialog">
          {children(close)}
        </div>
      )}
    </div>
  );
}

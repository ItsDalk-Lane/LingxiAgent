import { useCallback, useEffect, useLayoutEffect, useState, type CSSProperties, type RefObject } from 'react';

interface UseAnchoredDropdownOptions {
  open: boolean;
  triggerRef: RefObject<HTMLElement | null>;
  panelRef: RefObject<HTMLElement | null>;
  onClose: () => void;
  widthOffset?: number;
  gap?: number;
  viewportPadding?: number;
  zIndex?: number;
}

export function useAnchoredDropdown({
  open,
  triggerRef,
  panelRef,
  onClose,
  widthOffset = 0,
  gap = 4,
  viewportPadding = 8,
  zIndex = 9999,
}: UseAnchoredDropdownOptions): CSSProperties {
  const [panelStyle, setPanelStyle] = useState<CSSProperties>({});

  const updatePosition = useCallback(() => {
    const trigger = triggerRef.current;
    if (!open || !trigger) return;

    const rect = trigger.getBoundingClientRect();
    const width = rect.width + widthOffset;
    const maxLeft = window.innerWidth - width - viewportPadding;
    const left = Math.max(viewportPadding, Math.min(rect.left, maxLeft));

    setPanelStyle({
      position: 'fixed',
      left,
      width,
      top: rect.bottom + gap,
      maxHeight: Math.max(120, window.innerHeight - rect.bottom - viewportPadding - gap),
      zIndex,
    });
  }, [gap, open, triggerRef, viewportPadding, widthOffset, zIndex]);

  useLayoutEffect(() => {
    updatePosition();
  }, [updatePosition]);

  useEffect(() => {
    if (!open) return;
    window.addEventListener('resize', updatePosition);
    return () => window.removeEventListener('resize', updatePosition);
  }, [open, updatePosition]);

  useEffect(() => {
    if (!open) return;
    const handler = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.defaultPrevented) return;
      // 在焦点仍停留于触发按钮时，也只关闭当前菜单。
      event.preventDefault();
      event.stopPropagation();
      onClose();
      triggerRef.current?.focus();
    };
    const trigger = triggerRef.current;
    const panel = panelRef.current;
    trigger?.addEventListener('keydown', handler);
    panel?.addEventListener('keydown', handler);
    document.addEventListener('keydown', handler);
    return () => {
      trigger?.removeEventListener('keydown', handler);
      panel?.removeEventListener('keydown', handler);
      document.removeEventListener('keydown', handler);
    };
  }, [onClose, open, panelRef, triggerRef]);

  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      const target = e.target as Node;
      if (triggerRef.current?.contains(target)) return;
      if (panelRef.current?.contains(target)) return;
      onClose();
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [onClose, open, panelRef, triggerRef]);

  useEffect(() => {
    if (!open) return;
    const handler = (e: Event) => {
      const target = e.target as Node;
      if (triggerRef.current?.contains(target)) return;
      if (panelRef.current?.contains(target)) return;
      onClose();
    };
    window.addEventListener('scroll', handler, true);
    return () => window.removeEventListener('scroll', handler, true);
  }, [onClose, open, panelRef, triggerRef]);

  return panelStyle;
}

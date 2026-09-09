import React from 'react';
import { useShallow } from 'zustand/react/shallow';
import { useSettingsStore } from './store';
import styles from './Settings.module.css';

export function Toast() {
  const { toastMessage, toastType, toastVisible } = useSettingsStore(
    useShallow(s => ({ toastMessage: s.toastMessage, toastType: s.toastType, toastVisible: s.toastVisible }))
  );
  const cls = [styles['settings-toast']];
  if (toastType) cls.push(styles[toastType]);
  if (toastVisible) cls.push(styles['show']);
  return (
    <div
      className={cls.join(' ')}
      role={toastType === 'error' ? 'alert' : 'status'}
      aria-live={toastType === 'error' ? 'assertive' : 'polite'}
      aria-atomic="true"
    >
      {toastVisible && (
        <>
          <svg className={styles['settings-toast-icon']} aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            {toastType === 'error'
              ? <><circle cx="12" cy="12" r="9" /><path d="M12 7v6M12 17h.01" /></>
              : <><circle cx="12" cy="12" r="9" /><path d="m8 12 3 3 5-6" /></>}
          </svg>
          <span>{toastMessage}</span>
        </>
      )}
    </div>
  );
}

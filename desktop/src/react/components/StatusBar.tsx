import { useEffect } from 'react';
import { useStore } from '../stores';
import { manualReconnect } from '../services/websocket';
import styles from './StatusBar.module.css';

declare function t(key: string, vars?: Record<string, string | number>): string;

export function StatusBar() {
  const wsState = useStore((s) => s.wsState);
  const attempt = useStore((s) => s.wsReconnectAttempt);
  const failureReasonKey = useStore((s) => s.wsFailureReasonKey);
  const recoveryNotice = useStore((s) => s.wsRecoveryNotice);

  useEffect(() => {
    if (wsState !== 'connected' || !recoveryNotice) return;
    const timer = setTimeout(() => useStore.setState({ wsRecoveryNotice: false }), 5000);
    return () => clearTimeout(timer);
  }, [wsState, recoveryNotice]);

  if (wsState === 'connected' && !recoveryNotice && !failureReasonKey) return null;

  return (
    <div className={styles.bar}>
      {wsState === 'connected' && recoveryNotice && (
        <span className={styles.text}>{t('status.reconnected')}</span>
      )}
      {wsState === 'reconnecting' && (
        <span className={styles.text}>{t('status.reconnecting')} ({attempt})</span>
      )}
      {wsState === 'disconnected' && (
        <>
          <span className={styles.text}>{t('status.disconnected')}</span>
          <button className={styles.reconnect} onClick={() => manualReconnect()}>
            {t('status.reconnect')}
          </button>
        </>
      )}
      {failureReasonKey && (wsState !== 'connected' || failureReasonKey === 'status.rustCoreUnavailable') && (
        <span className={styles.text}>{t(failureReasonKey)}</span>
      )}
    </div>
  );
}

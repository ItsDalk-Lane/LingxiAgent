import React, { useState, useRef, useEffect } from 'react';
import { useSettingsStore, type ProviderSummary } from '../../store';
import { captureSettingsConnection, ownsSettingsConnection, type SettingsConnectionOwner } from '../../connection-owner';
import { resolveServerConnection } from '../../../services/server-connection';
import { startOAuth, completeOAuth, pollOAuth, logoutOAuth, type OAuthAttempt } from './oauth-service';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';

export function OAuthCredentials({ providerId, summary, onRefresh }: {
  providerId: string;
  summary: ProviderSummary;
  onRefresh: () => Promise<void>;
}) {
  const platform = window.platform;
  const showToast = useSettingsStore(s => s.showToast);
  const [codeInput, setCodeInput] = useState('');
  const [showCodeInput, setShowCodeInput] = useState(false);
  const [deviceCode, setDeviceCode] = useState<string | null>(null);
  const [polling, setPolling] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  type Operation = { owner: SettingsConnectionOwner; controller: AbortController };
  const operation = useRef<Operation | null>(null);
  const attemptRef = useRef<OAuthAttempt | null>(null);
  const live = (op: Operation) => operation.current === op && !op.controller.signal.aborted && ownsSettingsConnection(op.owner);
  const reset = () => {
    operation.current?.controller.abort(); operation.current = null; attemptRef.current = null;
    setShowCodeInput(false); setDeviceCode(null); setPolling(false); setBusy(false); setCodeInput('');
  };
  useEffect(() => {
    const unsubscribe = useSettingsStore.subscribe(() => {
      const op = operation.current;
      if (op && !ownsSettingsConnection(op.owner)) { reset(); setNotice(null); }
    });
    return () => { unsubscribe(); operation.current?.controller.abort(); operation.current = null; attemptRef.current = null; };
  }, [providerId]);
  useEffect(() => { reset(); setNotice(null); }, [providerId]);
  const begin = (): Operation => {
    reset(); setNotice(null); setBusy(true);
    const op = { owner: captureSettingsConnection(), controller: new AbortController() };
    operation.current = op;
    return op;
  };
  const rust = () => resolveServerConnection(useSettingsStore.getState())?.serverNodeKind === 'lingxi-service';
  const fail = (op: Operation, error: unknown) => {
    if (!live(op)) return;
    const message = t('settings.oauth.failed') + ': ' + (error instanceof Error ? error.message : String(error));
    reset(); setNotice(message); showToast(message, 'error');
  };
  const completed = async (op: Operation, persisted: boolean, loggedOut = false) => {
    if (!live(op)) return;
    attemptRef.current = null; setShowCodeInput(false); setDeviceCode(null); setPolling(false);
    try {
      await onRefresh();
      if (!live(op)) return;
      const message = !persisted && !loggedOut ? t('settings.oauth.notPersisted') : t(loggedOut ? 'settings.oauth.loggedOut' : 'settings.oauth.success');
      reset(); setNotice(!persisted && !loggedOut ? message : null); showToast(message, !persisted && !loggedOut ? 'error' : 'success');
    } catch (error) {
      if (!live(op)) return;
      const message = (!persisted && !loggedOut ? t('settings.oauth.notPersisted') + ' ' : '') + t(loggedOut ? 'settings.oauth.logoutRefreshFailed' : 'settings.oauth.loginRefreshFailed') + ': ' + (error instanceof Error ? error.message : String(error));
      reset(); setNotice(message); showToast(message, 'error');
    }
  };
  const wait = (op: Operation, ms: number) => new Promise<void>(resolve => {
    const done = () => { clearTimeout(timer); op.controller.signal.removeEventListener('abort', done); resolve(); };
    const timer = setTimeout(done, ms);
    op.controller.signal.addEventListener('abort', done, { once: true });
    if (op.controller.signal.aborted) done();
  });
  const pollLogin = async (op: Operation, attempt: OAuthAttempt) => {
    let interval = attempt.intervalMs;
    try {
      while (live(op)) {
        const left = attempt.expiresAt - Date.now();
        if (left <= 0) throw new Error(t('settings.mcp.oauthTimeout'));
        await wait(op, Math.min(interval, left));
        if (!live(op)) return;
        if (Date.now() >= attempt.expiresAt) throw new Error(t('settings.mcp.oauthTimeout'));
        const outcome = await pollOAuth(attempt, op.controller.signal);
        if (!live(op)) return;
        if (outcome.done) { await completed(op, outcome.persisted); return; }
        interval = outcome.intervalMs ?? interval;
      }
    } catch (error) { fail(op, error); }
  };
  const login = async () => {
    const op = begin(); const isRust = rust();
    try {
      const attempt = await startOAuth(providerId, isRust, op.controller.signal);
      if (!live(op)) return;
      attemptRef.current = attempt;
      if (!platform?.openExternal) throw new Error(t('settings.oauth.browserUnavailable'));
      const opened = await platform.openExternal(attempt.url);
      if (opened && opened.ok === false) throw new Error(opened.error || t('settings.oauth.browserUnavailable'));
      if (!live(op)) return;
      setBusy(false); setDeviceCode(attempt.deviceCode); setPolling(attempt.polling); setShowCodeInput(!attempt.polling);
      if (attempt.polling) void pollLogin(op, attempt);
    } catch (error) { fail(op, error); }
  };
  const submitCode = async () => {
    const code = codeInput.trim(); const op = operation.current; const attempt = attemptRef.current;
    if (!code || !op || !attempt || !live(op) || busy) return;
    setBusy(true);
    try {
      if (Date.now() >= attempt.expiresAt) throw new Error(t('settings.mcp.oauthTimeout'));
      const outcome = await completeOAuth(attempt, code, op.controller.signal);
      if (!live(op)) return;
      if (!outcome.done) throw new Error(t('settings.oauth.waiting'));
      await completed(op, outcome.persisted);
    } catch (error) { fail(op, error); }
  };
  const logout = async () => {
    const op = begin(); const isRust = rust();
    try {
      await logoutOAuth(providerId, isRust, op.controller.signal);
      if (live(op)) await completed(op, true, true);
    } catch (error) { fail(op, error); }
  };
  const cancel = () => { reset(); setNotice(t('settings.oauth.waitCancelled')); };

  return (
    <div className={styles['pv-credentials']}>
      <div className={styles['pv-cred-row']}>
        <span className={styles['pv-cred-label']}>OAuth</span>
        {summary.logged_in ? (
          <div className={styles['pv-oauth-status']}>
            <span className={styles['oauth-status-badge']}>{t('settings.oauth.loggedIn')}</span>
            <button className={styles['oauth-logout-btn']} onClick={logout} disabled={busy}>{t('settings.oauth.logout')}</button>
          </div>
        ) : (
          <button className={styles['oauth-login-btn']} onClick={login} disabled={busy || showCodeInput || polling}>{t('settings.oauth.login')}</button>
        )}
      </div>

      {(busy || showCodeInput || polling) && <button onClick={cancel}>{t('common.cancel')}</button>}
      {notice && <div role="alert">{notice}</div>}
      {showCodeInput && (
        <div className={styles['oauth-code-section']}>
          <input
            className={`${styles['settings-input']} ${styles['oauth-code-input']}`}
            type="text"
            placeholder={t('settings.oauth.codePlaceholder')}
            value={codeInput}
            disabled={busy}
            onChange={(e) => setCodeInput(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter') submitCode(); }}
            autoFocus
          />
          <button className={styles['oauth-code-submit']} onClick={submitCode} disabled={busy}>{t('settings.oauth.submit')}</button>
        </div>
      )}
      {deviceCode && (
        <div className={`${styles['oauth-code-section']} ${styles['oauth-device-code']}`}>
          <div
            className={styles['oauth-user-code']}
            title={t('settings.oauth.clickToCopy')}
            onClick={() => { const op=operation.current; if(op) void navigator.clipboard.writeText(deviceCode).then(() => { if(live(op)) showToast(t('settings.oauth.codeCopied'), 'success'); }).catch(error => fail(op,error)); }}
          >
            {deviceCode}
          </div>
          <div className={styles['oauth-device-hint']}>{t('settings.oauth.deviceHint')}</div>
          <div className={styles['oauth-device-spinner']}>{t('settings.oauth.waiting')}</div>
        </div>
      )}
      {polling && !deviceCode && !showCodeInput && (
        <div className={styles['oauth-code-section']}>
          <div className={styles['oauth-device-spinner']}>{t('settings.oauth.waiting')}</div>
        </div>
      )}
    </div>
  );
}

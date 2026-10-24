import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useSettingsStore } from '../store';
import { lingxiFetch } from '../api';
import { captureSettingsConnection, ownsSettingsConnection, type SettingsConnectionOwner } from '../connection-owner';
import { resolveServerConnection } from '../../services/service-client';
import { t } from '../helpers';
import { updateSettingsSnapshot } from '../actions';
import { SettingsSection } from '../components/SettingsSection';
import { SettingsRow } from '../components/SettingsRow';
import { SelectWidget, Toggle } from '@/ui';
import {
  normalizeBrowserPreferences,
  type BrowserAgentOpenBehavior,
  type BrowserPreferences,
} from '../../../../../shared/browser-preferences.ts';
import styles from '../Settings.module.css';

function requireBrowserPreferences(value: unknown): BrowserPreferences {
  if (!value || typeof value !== 'object' || !('acceptCookies' in value) || typeof value.acceptCookies !== 'boolean' || !('agentOpenBehavior' in value) || !['smart', 'current_tab', 'new_tab'].includes(String(value.agentOpenBehavior))) throw new Error('browser_preferences_invalid');
  return normalizeBrowserPreferences(value);
}

function browserBehaviorForControl(value: BrowserAgentOpenBehavior): 'current_tab' | 'new_tab' {
  return value === 'new_tab' ? 'new_tab' : 'current_tab';
}

export function BrowserTab() {
  const connection = useSettingsStore(s => s.activeServerConnection);
  const usesRustBrowser = useSettingsStore(s => resolveServerConnection(s)?.serverNodeKind === 'lingxi-service');
  const snapshotBrowser = useSettingsStore(s => s.settingsSnapshot.data?.preferences?.browser);
  const showToast = useSettingsStore(s => s.showToast);
  const [prefs, setPrefs] = useState<BrowserPreferences | null>(() => {
    const snapshot = useSettingsStore.getState().settingsSnapshot.data?.preferences?.browser;
    return snapshot ? normalizeBrowserPreferences(snapshot) : null;
  });
  const prefsOwner = useRef<SettingsConnectionOwner | null>(prefs ? captureSettingsConnection() : null);
  const [saving, setSaving] = useState(false);
  const [clearing, setClearing] = useState(false);

  useEffect(() => {
    const owner = captureSettingsConnection();
    const abort = new AbortController();
    setSaving(false); setClearing(false);
    if (snapshotBrowser) {
      prefsOwner.current = owner;
      setPrefs(normalizeBrowserPreferences(snapshotBrowser));
      return undefined;
    }
    let alive = true;
    prefsOwner.current = null;
    setPrefs(null);
    lingxiFetch('/api/preferences/browser', { signal: abort.signal })
      .then(res => res.json())
      .then((data) => {
        if (!alive || !ownsSettingsConnection(owner)) return;
        const loaded = requireBrowserPreferences(data?.browser);
        prefsOwner.current = owner;
        setPrefs(loaded);
      })
      .catch((err) => {
        if (!alive || !ownsSettingsConnection(owner)) return;
        showToast(t('settings.saveFailed') + ': ' + (err?.message || String(err)), 'error');
      });
    return () => {
      alive = false; abort.abort();
    };
  }, [showToast, snapshotBrowser, connection]);

  const behaviorOptions = useMemo(() => ([
    { value: 'current_tab', label: t('settings.browser.agentOpenBehaviorCurrentTab') },
    { value: 'new_tab', label: t('settings.browser.agentOpenBehaviorNewTab') },
  ]), []);

  const saveBrowserPreferences = useCallback(async (patch: Partial<BrowserPreferences>) => {
    const owner = prefsOwner.current;
    if (!prefs || !owner || !ownsSettingsConnection(owner)) return;
    const previous = prefs;
    const next = normalizeBrowserPreferences({ ...prefs, ...patch });
    setPrefs(next);
    setSaving(true);
    try {
      const res = await lingxiFetch('/api/preferences/browser', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ browser: next }),
      });
      const data = await res.json();
      if (data?.error) throw new Error(data.error);
      if (!ownsSettingsConnection(owner)) return;
      const saved = requireBrowserPreferences(data?.browser);
      setPrefs(saved);
      updateSettingsSnapshot(snapshot => ({
        ...snapshot,
        preferences: { ...snapshot.preferences, browser: saved },
      }));
    } catch (err: any) {
      if (!ownsSettingsConnection(owner)) return;
      setPrefs(previous);
      showToast(t('settings.saveFailed') + ': ' + (err?.message || String(err)), 'error');
    } finally {
      if (ownsSettingsConnection(owner)) setSaving(false);
    }
  }, [prefs, showToast]);

  const clearCookies = useCallback(async () => {
    const owner = prefsOwner.current;
    if (!owner || !ownsSettingsConnection(owner)) return;
    setClearing(true);
    try {
      const res = await lingxiFetch('/api/preferences/browser/clear-cookies', { method: 'POST' });
      const data = await res.json();
      if (!ownsSettingsConnection(owner)) return;
      if (!res.ok || data?.ok !== true || data?.error) throw new Error(data?.error || res.statusText || 'Failed');
      showToast(t('settings.browser.clearCookiesSuccess'), 'success');
    } catch (err: any) {
      if (!ownsSettingsConnection(owner)) return;
      showToast(t('settings.saveFailed') + ': ' + (err?.message || String(err)), 'error');
    } finally {
      if (ownsSettingsConnection(owner)) setClearing(false);
    }
  }, [showToast]);

  const agentOpenBehavior = browserBehaviorForControl(prefs?.agentOpenBehavior || 'current_tab');

  return (
    <div className={`${styles['settings-tab-content']} ${styles['active']}`} data-tab="browser">
      <SettingsSection title={t('settings.browser.cookiesTitle')}>
        <SettingsRow
          label={t('settings.browser.acceptCookies')}
          hint={<>
            {t('settings.browser.acceptCookiesHint')}
            {usesRustBrowser && <><br /><span>{t('settings.browser.cookiePolicyChangeHint')}</span></>}
          </>}
          control={
            <Toggle
              on={prefs?.acceptCookies !== false}
              onChange={(on) => void saveBrowserPreferences({ acceptCookies: on })}
              ariaLabel={t('settings.browser.acceptCookies')}
              disabled={saving || !prefs}
            />
          }
        />
        <SettingsRow
          label={t('settings.browser.clearCookies')}
          hint={t('settings.browser.clearCookiesHint')}
          control={
            <button
              type="button"
              className={styles['quick-chat-reset-button']}
              onClick={() => void clearCookies()}
              disabled={clearing || !prefs}
            >
              {clearing ? t('common.loading') : t('settings.browser.clearCookiesButton')}
            </button>
          }
        />
      </SettingsSection>

      <SettingsSection title={t('settings.browser.agentTitle')}>
        <SettingsRow
          label={t('settings.browser.agentOpenBehavior')}
          hint={t('settings.browser.agentOpenBehaviorHint')}
          control={
            <SelectWidget
              options={behaviorOptions}
              value={agentOpenBehavior}
              onChange={(value) => void saveBrowserPreferences({
                agentOpenBehavior: value === 'new_tab' ? 'new_tab' : 'current_tab',
              })}
              placeholder={t('common.loading')}
              disabled={saving || !prefs}
            />
          }
        />
      </SettingsSection>
    </div>
  );
}

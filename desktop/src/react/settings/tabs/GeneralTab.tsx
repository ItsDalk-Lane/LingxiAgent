import React, { useCallback, useEffect, useState } from 'react';
import { useSettingsStore } from '../store';
import { lingxiFetch } from '../api';
import { autoSaveConfig, t } from '../helpers';
import { loadSettingsConfig, updateSettingsSnapshot } from '../actions';
import { SettingsSection } from '../components/SettingsSection';
import { SettingsRow } from '../components/SettingsRow';
import { SelectWidget, Toggle } from '@/ui';
import { readConfigBoolean } from '../resource-state';
import type { AutoLaunchStatus } from '../../types';
import {
  normalizeNotificationPreferences as normalizeSharedNotificationPreferences,
  normalizeBackgroundCompletionNotificationMode,
  normalizeChatCompletionNotificationMode,
} from '../../../../../shared/notification-preferences.ts';
import {
  DEFAULT_QUICK_CHAT_REUSE_TIMEOUT_MINUTES,
  normalizeQuickChatPreferences,
} from '../../../../../shared/quick-chat-preferences.ts';
import styles from '../Settings.module.css';

type ChatCompletionNotificationMode = 'never' | 'when_unfocused' | 'when_session_unfocused';
type BackgroundCompletionNotificationMode = 'never' | 'when_unfocused' | 'always';

interface NotificationPreferences {
  chatCompletion: ChatCompletionNotificationMode;
  scheduledTaskCompletion: BackgroundCompletionNotificationMode;
  patrolCompletion: BackgroundCompletionNotificationMode;
}

interface QuickChatPreferences {
  shortcut: string;
  reuseTimeoutMinutes: number;
}

function ReuseTimeoutInput({
  value,
  saving,
  onChange,
}: {
  value: number | null;
  saving: boolean;
  onChange: (value: number) => void;
}) {
  return (
    <div className={styles['quick-chat-timeout-control']}>
      <input
        className={styles['quick-chat-timeout-input']}
        type="number"
        min={0}
        max={120}
        step={1}
        inputMode="numeric"
        aria-label={t('settings.general.quickChat.reuseTimeout')}
        value={typeof value === 'number' && Number.isFinite(value) ? value : ''}
        disabled={saving}
        onChange={(event) => onChange(Number(event.currentTarget.value))}
      />
      <span className={styles['quick-chat-timeout-unit']}>{t('settings.general.quickChat.minutes')}</span>
    </div>
  );
}

function normalizeChatCompletionMode(value: unknown): ChatCompletionNotificationMode {
  return normalizeChatCompletionNotificationMode(value) as ChatCompletionNotificationMode;
}

function normalizeBackgroundCompletionMode(value: unknown): BackgroundCompletionNotificationMode {
  return normalizeBackgroundCompletionNotificationMode(value) as BackgroundCompletionNotificationMode;
}

function normalizeNotificationPreferences(value: unknown): NotificationPreferences {
  return normalizeSharedNotificationPreferences(value) as NotificationPreferences;
}

export function GeneralTab() {
  const hana = window.hana;
  const settingsConfig = useSettingsStore(s => s.settingsConfig);
  const snapshotQuickChat = useSettingsStore(s => s.settingsSnapshot.data?.preferences?.quickChat);
  const snapshotNotifications = useSettingsStore(s => s.settingsSnapshot.data?.preferences?.notifications);
  const showToast = useSettingsStore(s => s.showToast);
  const [autoLaunch, setAutoLaunch] = useState<AutoLaunchStatus | null>(null);
  const [autoLaunchSaving, setAutoLaunchSaving] = useState(false);
  const [keepAwakeSaving, setKeepAwakeSaving] = useState(false);
  const [quickChatPrefs, setQuickChatPrefs] = useState<QuickChatPreferences | null>(() => {
    const snapshot = useSettingsStore.getState().settingsSnapshot.data?.preferences?.quickChat;
    return snapshot ? normalizeQuickChatPreferences(snapshot) : null;
  });
  const [quickChatSaving, setQuickChatSaving] = useState(false);
  const [notificationSaving, setNotificationSaving] = useState(false);
  const [notificationPrefs, setNotificationPrefs] = useState<NotificationPreferences | null>(() => {
    const snapshot = useSettingsStore.getState().settingsSnapshot.data?.preferences?.notifications;
    return snapshot ? normalizeNotificationPreferences(snapshot) : null;
  });
  const keepAwake = readConfigBoolean(settingsConfig, cfg => cfg.keep_awake, false);

  useEffect(() => {
    let alive = true;
    hana?.getAutoLaunchStatus?.()
      .then((status) => {
        if (alive && status) setAutoLaunch(status);
      })
      .catch(() => {
        if (alive) setAutoLaunch(null);
      });
    return () => {
      alive = false;
    };
  }, [hana]);

  useEffect(() => {
    if (snapshotQuickChat) {
      setQuickChatPrefs(normalizeQuickChatPreferences(snapshotQuickChat));
      return undefined;
    }
    let alive = true;
    lingxiFetch('/api/preferences/quick-chat')
      .then(res => res.json())
      .then((data) => {
        if (!alive) return;
        setQuickChatPrefs(normalizeQuickChatPreferences(data?.quickChat));
      })
      .catch((err) => {
        if (!alive) return;
        showToast(t('settings.saveFailed') + ': ' + (err?.message || String(err)), 'error');
      });
    return () => {
      alive = false;
    };
  }, [showToast, snapshotQuickChat]);

  useEffect(() => {
    if (snapshotNotifications) {
      setNotificationPrefs(normalizeNotificationPreferences(snapshotNotifications));
      return undefined;
    }
    let alive = true;
    lingxiFetch('/api/preferences/notifications')
      .then(res => res.json())
      .then((data) => {
        if (!alive) return;
        setNotificationPrefs(normalizeNotificationPreferences(data?.notifications));
      })
      .catch((err) => {
        if (!alive) return;
        showToast(t('settings.saveFailed') + ': ' + (err?.message || String(err)), 'error');
      });
    return () => {
      alive = false;
    };
  }, [showToast, snapshotNotifications]);

  const saveQuickChatPreferences = useCallback(async (
    patch: Partial<QuickChatPreferences>,
    options: { reloadShortcut?: boolean; eventName?: string } = {},
  ) => {
    if (!quickChatPrefs) return;
    const previous = quickChatPrefs;
    const next = normalizeQuickChatPreferences({ ...quickChatPrefs, ...patch });
    setQuickChatPrefs(next);
    setQuickChatSaving(true);
    try {
      const res = await lingxiFetch('/api/preferences/quick-chat', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ quickChat: next }),
      });
      const data = await res.json();
      if (data?.error) throw new Error(data.error);
      const saved = normalizeQuickChatPreferences(data?.quickChat);
      setQuickChatPrefs(saved);
      updateSettingsSnapshot(snapshot => ({
        ...snapshot,
        preferences: { ...snapshot.preferences, quickChat: saved },
      }));
      if (options.reloadShortcut) {
        const registration = await hana?.quickChatReloadShortcut?.();
        if (registration && registration.ok === false) {
          throw new Error(registration.error || t('settings.general.quickChat.registrationFailed'));
        }
      }
      if (options.eventName) hana?.settingsChanged?.(options.eventName, { quickChat: saved });
    } catch (err: any) {
      setQuickChatPrefs(previous);
      try {
        await lingxiFetch('/api/preferences/quick-chat', {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ quickChat: previous }),
        });
        if (options.reloadShortcut) await hana?.quickChatReloadShortcut?.();
      } catch {}
      showToast(t('settings.saveFailed') + ': ' + (err?.message || String(err)), 'error');
    } finally {
      setQuickChatSaving(false);
    }
  }, [hana, quickChatPrefs, showToast]);

  const saveQuickChatReuseTimeout = useCallback((reuseTimeoutMinutes: number) => saveQuickChatPreferences(
    { reuseTimeoutMinutes },
    { eventName: 'quick-chat-preferences-changed' },
  ), [saveQuickChatPreferences]);

  const handleAutoLaunchToggle = useCallback(async (on: boolean) => {
    if (!hana?.setAutoLaunchEnabled) return;
    const previous = autoLaunch;
    setAutoLaunchSaving(true);
    try {
      const next = await hana.setAutoLaunchEnabled(on);
      setAutoLaunch(next || previous);
    } catch {
      setAutoLaunch(previous);
    } finally {
      setAutoLaunchSaving(false);
    }
  }, [autoLaunch, hana]);

  const handleKeepAwakeToggle = useCallback(async (on: boolean) => {
    if (!hana?.setKeepAwakeEnabled) return;
    const previous = keepAwake === true;
    setKeepAwakeSaving(true);
    try {
      const saved = await autoSaveConfig({ keep_awake: on }, { silent: true });
      if (saved === false) return;
      await hana.setKeepAwakeEnabled(on);
    } catch (err: any) {
      if (previous !== on) {
        await autoSaveConfig({ keep_awake: previous }, { silent: true });
        await loadSettingsConfig();
      }
      showToast(t('settings.saveFailed') + ': ' + (err?.message || String(err)), 'error');
    } finally {
      setKeepAwakeSaving(false);
    }
  }, [hana, keepAwake, showToast]);

  const saveNotificationPreference = useCallback(async (
    key: keyof NotificationPreferences,
    value: string,
  ) => {
    if (!notificationPrefs) return;
    const previous = notificationPrefs;
    const next = {
      ...notificationPrefs,
      [key]: key === 'chatCompletion'
        ? normalizeChatCompletionMode(value)
        : normalizeBackgroundCompletionMode(value),
    };
    setNotificationPrefs(next);
    setNotificationSaving(true);
    try {
      const res = await lingxiFetch('/api/preferences/notifications', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ notifications: { [key]: next[key] } }),
      });
      const data = await res.json();
      if (data?.error) throw new Error(data.error);
      const saved = normalizeNotificationPreferences(data?.notifications);
      setNotificationPrefs(saved);
      updateSettingsSnapshot(snapshot => ({
        ...snapshot,
        preferences: { ...snapshot.preferences, notifications: saved },
      }));
    } catch (err: any) {
      setNotificationPrefs(previous);
      showToast(t('settings.saveFailed') + ': ' + (err?.message || String(err)), 'error');
    } finally {
      setNotificationSaving(false);
    }
  }, [notificationPrefs, showToast]);

  return (
    <div className={`${styles['settings-tab-content']} ${styles['active']}`} data-tab="general">
      <SettingsSection title={t('settings.general.startup.title')}>
        {autoLaunch?.supported && (
          <SettingsRow
            label={t('settings.general.launchAtLogin')}
            control={
              <Toggle
                on={autoLaunch.openAtLogin}
                onChange={handleAutoLaunchToggle}
                ariaLabel={t('settings.general.launchAtLogin')}
                disabled={autoLaunchSaving}
              />
            }
          />
        )}
        <SettingsRow
          label={t('settings.general.keepAwake')}
          control={
            <Toggle
              on={keepAwake}
              onChange={handleKeepAwakeToggle}
              ariaLabel={t('settings.general.keepAwake')}
              disabled={keepAwakeSaving || !hana?.setKeepAwakeEnabled}
            />
          }
        />
      </SettingsSection>

      <SettingsSection title={t('settings.general.quickChat.title')}>
        <SettingsRow
          label={t('settings.general.quickChat.reuseTimeout')}
          hint={t('settings.general.quickChat.reuseTimeoutHint')}
          control={
            <ReuseTimeoutInput
              value={quickChatPrefs?.reuseTimeoutMinutes ?? null}
              saving={quickChatSaving || !quickChatPrefs}
              onChange={(value) => void saveQuickChatReuseTimeout(value)}
            />
          }
        />
      </SettingsSection>

      <SettingsSection title={t('settings.general.notifications.title')}>
        <SettingsRow
          data-testid="chat-completion-notification-row"
          label={t('settings.general.notifications.chatCompletion')}
          control={
            <SelectWidget
              options={[
                { value: 'never', label: t('settings.general.notifications.never') },
                { value: 'when_unfocused', label: t('settings.general.notifications.whenUnfocused') },
                { value: 'when_session_unfocused', label: t('settings.general.notifications.whenSessionUnfocused') },
              ]}
              value={notificationPrefs?.chatCompletion || ''}
              onChange={(value) => void saveNotificationPreference('chatCompletion', value)}
              placeholder={t('common.loading')}
              disabled={notificationSaving || !notificationPrefs}
            />
          }
        />
        <SettingsRow
          data-testid="scheduled-task-completion-notification-row"
          label={t('settings.general.notifications.scheduledTaskCompletion')}
          control={
            <SelectWidget
              options={[
                { value: 'never', label: t('settings.general.notifications.never') },
                { value: 'when_unfocused', label: t('settings.general.notifications.whenUnfocused') },
                { value: 'always', label: t('settings.general.notifications.always') },
              ]}
              value={notificationPrefs?.scheduledTaskCompletion || ''}
              onChange={(value) => void saveNotificationPreference('scheduledTaskCompletion', value)}
              placeholder={t('common.loading')}
              disabled={notificationSaving || !notificationPrefs}
            />
          }
        />
        <SettingsRow
          data-testid="patrol-completion-notification-row"
          label={t('settings.general.notifications.patrolCompletion')}
          control={
            <SelectWidget
              options={[
                { value: 'never', label: t('settings.general.notifications.never') },
                { value: 'when_unfocused', label: t('settings.general.notifications.whenUnfocused') },
                { value: 'always', label: t('settings.general.notifications.always') },
              ]}
              value={notificationPrefs?.patrolCompletion || ''}
              onChange={(value) => void saveNotificationPreference('patrolCompletion', value)}
              placeholder={t('common.loading')}
              disabled={notificationSaving || !notificationPrefs}
            />
          }
        />
      </SettingsSection>
    </div>
  );
}

/**
 * KeybindingsTab — 设置 → 快捷键。
 *
 * 项目内所有可配置快捷键的统一管理界面：
 * - 每条命令展示名称、描述、当前键位（可多组）、作用域（全局/应用/局部）；
 * - 点键位进入录制态直接按新组合改键；每条绑定可单独移除，命令可恢复默认；
 * - 顶部支持按名称搜索与「全部恢复默认」。
 *
 * 键位与作用域定义来自 shared/keybindings-preferences（与主进程、dispatcher
 * 同源）；保存走 PUT /api/preferences/keybindings，全局命令变更后通知主进程
 * 重注册 globalShortcut。
 */

import React, { useCallback, useEffect, useState } from 'react';
import { useSettingsStore } from '../store';
import { lingxiFetch } from '../api';
import { t } from '../helpers';
import { updateSettingsSnapshot } from '../actions';
import { SettingsSection } from '../components/SettingsSection';
import {
  KEYBINDING_COMMAND_VIEWS,
  comboFromEvent,
  comboLabelParts,
  conflictsFor,
  effectiveBindings,
} from '../../keybindings/commands';
import { normalizeKeybindings } from '../../../../../shared/keybindings-preferences.ts';
import styles from '../Settings.module.css';

type StoredKeybindings = Record<string, string[]>;

/** 全局键位占用状态：checking 探测中 / ok 可用 / taken 被占用 / unavailable 无桥接（桌面应用外）。 */
type GlobalAvailability = 'checking' | 'ok' | 'taken' | 'unavailable';

interface RecordingTarget {
  commandId: string;
  index: number;
}

const MAX_BINDINGS = 3;

function isMacPlatform(): boolean {
  return typeof navigator !== 'undefined' && navigator.platform.toLowerCase().includes('mac');
}

function scopeLabelKey(scope: string): string {
  if (scope === 'global') return 'settings.keybindings.scope.global';
  if (scope === 'local') return 'settings.keybindings.scope.local';
  return 'settings.keybindings.scope.app';
}

function KeycapRow({ combo }: { combo: string }) {
  return (
    <span className={styles['shortcut-keycaps']}>
      {comboLabelParts(combo).map((part, index) => (
        <span key={`${part}-${index}`} className={styles['shortcut-keycap']}>{part}</span>
      ))}
    </span>
  );
}

function PencilIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M17 3a2.828 2.828 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z" />
    </svg>
  );
}

function TrashIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <polyline points="3 6 5 6 21 6" />
      <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
    </svg>
  );
}

function UndoIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <polyline points="1 4 1 10 7 10" />
      <path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10" />
    </svg>
  );
}

export function KeybindingsTab() {
  const hana = window.hana;
  const showToast = useSettingsStore(s => s.showToast);
  const snapshotKeybindings = useSettingsStore(s => s.settingsSnapshot.data?.preferences?.keybindings);
  const [stored, setStored] = useState<StoredKeybindings>(() =>
    normalizeKeybindings(useSettingsStore.getState().settingsSnapshot.data?.preferences?.keybindings ?? {}),
  );
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [recording, setRecording] = useState<RecordingTarget | null>(null);
  const [recordingHint, setRecordingHint] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  // 全局键位占用状态（combo 字符串 → 状态），加载与保存后探测刷新。
  const [availability, setAvailability] = useState<Record<string, GlobalAvailability>>({});

  const probeGlobalCombo = useCallback(async (combo: string): Promise<GlobalAvailability> => {
    const test = hana?.keybindingsTestRegister;
    if (typeof test !== 'function') return 'unavailable'; // 浏览器/dev 环境：无主进程桥，显式标注无法检测
    try {
      const result = await test(combo);
      return result?.ok ? 'ok' : 'taken';
    } catch {
      return 'unavailable';
    }
  }, [hana]);

  const refreshGlobalAvailability = useCallback((bindingsMap: Record<string, string[]>) => {
    const combos: string[] = [];
    for (const command of KEYBINDING_COMMAND_VIEWS) {
      if (command.scope !== 'global') continue;
      for (const combo of bindingsMap[command.id] || []) {
        if (combo && !combos.includes(combo)) combos.push(combo);
      }
    }
    if (combos.length === 0) return;
    setAvailability((prev) => {
      const next = { ...prev };
      for (const combo of combos) next[combo] = 'checking';
      return next;
    });
    void Promise.all(combos.map(probeGlobalCombo)).then((results) => {
      setAvailability((prev) => {
        const next = { ...prev };
        combos.forEach((combo, index) => { next[combo] = results[index]; });
        return next;
      });
    });
  }, [probeGlobalCombo]);

  useEffect(() => {
    if (snapshotKeybindings) {
      setStored(normalizeKeybindings(snapshotKeybindings));
      return;
    }
    let alive = true;
    setLoading(true);
    lingxiFetch('/api/preferences/keybindings')
      .then(res => res.json())
      .then((data) => {
        if (alive) setStored(normalizeKeybindings(data?.keybindings ?? {}));
      })
      .catch((err) => {
        if (!alive) return;
        // 读取失败（比如服务端尚未支持该路由的旧构建）时保持默认键位可用，
        // 用「读取失败」提示而不是误报「保存失败」。
        showToast(t('settings.keybindings.loadFailed') + ': ' + (err?.message || String(err)), 'error');
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [showToast, snapshotKeybindings]);

  // 初次加载（以及 snapshot 同步）后，对全局命令的当前键位做占用探测。
  useEffect(() => {
    if (!stored) return;
    refreshGlobalAvailability(effectiveBindings(stored));
  }, [stored, refreshGlobalAvailability]);

  const saveCommandBindings = useCallback(async (commandId: string, nextKeys: string[]) => {
    if (!stored) return;
    const deduped: string[] = [];
    for (const key of nextKeys) {
      if (key && !deduped.includes(key)) deduped.push(key);
    }
    while (deduped.length < nextKeys.length) deduped.push('');
    const activeKeys = deduped.filter(Boolean);
    const conflicts = conflictsFor(commandId, activeKeys, stored);
    if (conflicts.length > 0) {
      const conflictNames = conflicts
        .map(id => KEYBINDING_COMMAND_VIEWS.find(c => c.id === id)?.labelKey)
        .filter(Boolean)
        .map(key => t(key as string));
      showToast(t('settings.keybindings.conflict') + (conflictNames.length ? `: ${conflictNames.join(', ')}` : ''), 'error');
      return;
    }

    const command = KEYBINDING_COMMAND_VIEWS.find(c => c.id === commandId);
    if (!command) return;

    // 全局键位保存前先探测系统占用：被别的应用占用的组合键注册不了，
    // 与其让主进程注册失败再报英文错误，不如在写入前给出明确提醒。
    if (command.scope === 'global' && typeof hana?.keybindingsTestRegister === 'function') {
      for (const combo of activeKeys) {
        let probe: { ok?: boolean } | undefined;
        try {
          probe = await hana.keybindingsTestRegister(combo);
        } catch {
          probe = undefined; // 探测失败不阻塞保存，保存后的重注册仍会拦一道
        }
        if (probe && probe.ok === false) {
          showToast(t('settings.keybindings.systemTaken').replace('{combo}', combo), 'error');
          return;
        }
      }
    }

    const previous = stored;
    // 等于默认键位的覆盖项剔除（删覆盖即恢复默认）；显式空数组保留（= 未绑定）。
    const sameAsDefault =
      deduped.filter(Boolean).length === command.defaultKeys.length &&
      command.defaultKeys.every((key, index) => deduped[index] === key);
    const nextStored: StoredKeybindings = { ...previous };
    if (sameAsDefault) delete nextStored[commandId];
    else nextStored[commandId] = deduped.filter(Boolean);

    setStored(nextStored);
    setSaving(true);
    try {
      const res = await lingxiFetch('/api/preferences/keybindings', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ keybindings: nextStored }),
      });
      const data = await res.json();
      if (data?.error) throw new Error(data.error);
      const saved = normalizeKeybindings(data?.keybindings ?? nextStored);
      setStored(saved);
      updateSettingsSnapshot(snapshot => ({
        ...snapshot,
        preferences: { ...snapshot.preferences, keybindings: saved },
      }));
      if (command.scope === 'global') {
        const registration = await hana?.keybindingsReloadGlobal?.() ?? await hana?.quickChatReloadShortcut?.();
        if (registration && registration.ok === false) {
          throw new Error(t('settings.keybindings.registrationFailed'));
        }
      }
      hana?.settingsChanged?.('keybindings-changed', { keybindings: saved });
      if (command.scope === 'global') refreshGlobalAvailability(saved);
      showToast(t('settings.keybindings.saved'), 'success');
    } catch (err: any) {
      setStored(previous);
      showToast(t('settings.saveFailed') + ': ' + (err?.message || String(err)), 'error');
    } finally {
      setSaving(false);
    }
  }, [hana, showToast, stored, refreshGlobalAvailability]);

  // 录制态：捕获阶段截取按键，Esc 取消，其余交给 comboFromEvent 判定合法性。
  // 无效按键（纯修饰键 / 无修饰符的普通键）不再静默忽略，内联提示替代。
  useEffect(() => {
    if (!recording) {
      setRecordingHint(null);
      return undefined;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === 'Escape') {
        setRecording(null);
        return;
      }
      const combo = comboFromEvent(event);
      if (!combo) {
        const modifierOnly = ['Shift', 'Control', 'Alt', 'Meta'].includes(event.key);
        setRecordingHint(modifierOnly
          ? t('settings.keybindings.recordingModifierOnly')
          : t('settings.keybindings.recordingNeedsModifier'));
        return;
      }
      const target = recording;
      setRecording(null);
      setRecordingHint(null);
      const command = KEYBINDING_COMMAND_VIEWS.find(c => c.id === target.commandId);
      if (!command) return;
      const current = effectiveBindings(stored)[command.id] || [];
      const next = current.slice();
      next[target.index] = combo;
      void saveCommandBindings(command.id, next);
    };
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [recording, stored, saveCommandBindings]);

  const removeBinding = useCallback((commandId: string, index: number) => {
    const command = KEYBINDING_COMMAND_VIEWS.find(c => c.id === commandId);
    if (!command || !stored) return;
    const current = effectiveBindings(stored)[commandId] || [];
    const next = current.slice();
    next.splice(index, 1);
    void saveCommandBindings(commandId, next);
  }, [saveCommandBindings, stored]);

  const addBinding = useCallback((commandId: string) => {
    const command = KEYBINDING_COMMAND_VIEWS.find(c => c.id === commandId);
    if (!command) return;
    const current = effectiveBindings(stored)[commandId] || [];
    if (current.length >= MAX_BINDINGS) {
      showToast(t('settings.keybindings.maxBindings'), 'error');
      return;
    }
    setRecording({ commandId, index: current.length });
  }, [showToast, stored]);

  const restoreCommand = useCallback((commandId: string) => {
    if (!stored || !(commandId in stored)) return;
    void saveCommandBindings(commandId, KEYBINDING_COMMAND_VIEWS.find(c => c.id === commandId)?.defaultKeys || []);
  }, [saveCommandBindings, stored]);

  const resetAll = useCallback(async () => {
    if (!stored) return;
    const previous = stored;
    setStored({});
    setSaving(true);
    try {
      const res = await lingxiFetch('/api/preferences/keybindings', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ keybindings: {} }),
      });
      const data = await res.json();
      if (data?.error) throw new Error(data.error);
      const saved = normalizeKeybindings(data?.keybindings ?? {});
      setStored(saved);
      updateSettingsSnapshot(snapshot => ({
        ...snapshot,
        preferences: { ...snapshot.preferences, keybindings: saved },
      }));
      await hana?.keybindingsReloadGlobal?.();
      hana?.settingsChanged?.('keybindings-changed', { keybindings: saved });
      refreshGlobalAvailability(saved);
      showToast(t('settings.keybindings.resetDone'), 'success');
    } catch (err: any) {
      setStored(previous);
      showToast(t('settings.saveFailed') + ': ' + (err?.message || String(err)), 'error');
    } finally {
      setSaving(false);
    }
  }, [hana, showToast, stored, refreshGlobalAvailability]);

  const effective = stored ? effectiveBindings(stored) : {};
  const normalizedQuery = query.trim().toLowerCase();
  const visibleCommands = KEYBINDING_COMMAND_VIEWS.filter((command) => {
    if (!normalizedQuery) return true;
    const label = t(command.labelKey).toLowerCase();
    const hint = t(command.hintKey).toLowerCase();
    return label.includes(normalizedQuery) || hint.includes(normalizedQuery) || command.id.toLowerCase().includes(normalizedQuery);
  });
  const hasOverrides = stored ? Object.keys(stored).length > 0 : false;

  return (
    <div className={`${styles['settings-tab-content']} ${styles['active']}`} data-tab="keybindings">
      <SettingsSection title={t('settings.keybindings.title')}>
        <div className={styles['kb-toolbar']}>
          <div className={styles['kb-search']}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <circle cx="11" cy="11" r="8" />
              <line x1="21" y1="21" x2="16.65" y2="16.65" />
            </svg>
            <input
              type="text"
              value={query}
              placeholder={t('settings.keybindings.searchPlaceholder')}
              aria-label={t('settings.keybindings.searchPlaceholder')}
              onChange={(event) => setQuery(event.currentTarget.value)}
            />
          </div>
          <button
            type="button"
            className={styles['kb-reset-all']}
            onClick={() => void resetAll()}
            disabled={saving || loading || !hasOverrides}
            title={!hasOverrides && !saving && !loading ? t('settings.keybindings.alreadyDefault') : undefined}
          >
            {t('settings.keybindings.resetAll')}
          </button>
        </div>

        <div className={styles['kb-table']} role="table" aria-label={t('settings.keybindings.title')}>
          <div className={styles['kb-head']} role="row">
            <span role="columnheader">{t('settings.keybindings.column.command')}</span>
            <span role="columnheader">{t('settings.keybindings.column.binding')}</span>
            <span role="columnheader">{t('settings.keybindings.column.scope')}</span>
            <span role="columnheader">{t('settings.keybindings.column.actions')}</span>
          </div>
          {visibleCommands.map((command) => {
            const bindings = effective[command.id] || [];
            const isRecordingThis = recording?.commandId === command.id;
            return (
              <div key={command.id} className={styles['kb-row']} role="row" data-command={command.id}>
                <div className={styles['kb-cell-command']} role="cell">
                  <span className={styles['kb-command-name']}>{t(command.labelKey)}</span>
                  <span className={styles['kb-command-hint']}>{t(command.hintKey)}</span>
                </div>
                <div className={styles['kb-cell-bindings']} role="cell">
                  {bindings.length === 0 && !isRecordingThis && (
                    <span className={styles['kb-unbound']}>{t('settings.keybindings.bindingEmpty')}</span>
                  )}
                  {bindings.map((combo, index) => {
                    const recordingHere = isRecordingThis && recording?.index === index;
                    const state = command.scope === 'global' ? availability[combo] : undefined;
                    return (
                      <div key={`${combo}-${index}`} className={styles['kb-binding-line']}>
                        {recordingHere ? (
                          <span className={`${styles['kb-recording-slot']} ${recordingHint ? styles['kb-recording-hinted'] : ''}`}>
                            {recordingHint || t('settings.keybindings.recording')}
                          </span>
                        ) : (
                          <>
                            <KeycapRow combo={combo} />
                            {command.scope === 'global' && state && (
                              <span
                                className={`${styles['kb-availability']} ${styles[`kb-avail-${state}`]}`}
                                title={t(`settings.keybindings.availability.${state}`)}
                              >
                                {t(`settings.keybindings.availability.${state}`)}
                              </span>
                            )}
                          </>
                        )}
                        <span className={styles['kb-binding-actions']}>
                          <button
                            type="button"
                            className={styles['kb-icon-btn']}
                            aria-label={t('settings.keybindings.editBinding')}
                            title={t('settings.keybindings.editBinding')}
                            disabled={saving}
                            onClick={() => { setRecordingHint(null); setRecording({ commandId: command.id, index }); }}
                          >
                            <PencilIcon />
                          </button>
                          <button
                            type="button"
                            className={styles['kb-icon-btn']}
                            aria-label={t('settings.keybindings.removeBinding')}
                            title={t('settings.keybindings.removeBinding')}
                            disabled={saving}
                            onClick={() => removeBinding(command.id, index)}
                          >
                            <TrashIcon />
                          </button>
                        </span>
                      </div>
                    );
                  })}
                  {isRecordingThis && recording && recording.index >= bindings.length && (
                    <div className={styles['kb-binding-line']}>
                      <span className={`${styles['kb-recording-slot']} ${recordingHint ? styles['kb-recording-hinted'] : ''}`}>
                        {recordingHint || t('settings.keybindings.recording')}
                      </span>
                    </div>
                  )}
                  {bindings.length < MAX_BINDINGS && !isRecordingThis && (
                    <button
                      type="button"
                      className={styles['kb-add-binding']}
                      disabled={saving || recording !== null}
                      onClick={() => { setRecordingHint(null); addBinding(command.id); }}
                    >
                      + {t('settings.keybindings.addBinding')}
                    </button>
                  )}
                </div>
                <div className={styles['kb-cell-scope']} role="cell">
                  <span className={`${styles['kb-scope-badge']} ${styles[`kb-scope-${command.scope}`]}`} title={t(`settings.keybindings.scope.${command.scope}Hint`)}>
                    {t(scopeLabelKey(command.scope))}
                  </span>
                </div>
                <div className={styles['kb-cell-ops']} role="cell">
                  <button
                    type="button"
                    className={styles['kb-icon-btn']}
                    aria-label={t('settings.keybindings.restoreCommand')}
                    title={command.id in (stored || {}) ? t('settings.keybindings.restoreCommand') : t('settings.keybindings.alreadyDefault')}
                    disabled={saving || !(command.id in (stored || {}))}
                    onClick={() => restoreCommand(command.id)}
                  >
                    <UndoIcon />
                  </button>
                </div>
              </div>
            );
          })}
          {visibleCommands.length === 0 && (
            <div className={styles['kb-empty']}>{t('settings.keybindings.noResults')}</div>
          )}
        </div>
        <p className={styles['kb-footnote']}>
          {isMacPlatform()
            ? t('settings.keybindings.footnoteMac')
            : t('settings.keybindings.footnote')}
        </p>
      </SettingsSection>
    </div>
  );
}

import React, { useState, useCallback, useEffect } from 'react';
import { useSettingsStore } from '../store';
import { autoSaveConfig, t } from '../helpers';
import { lingxiFetch } from '../api';
import { loadSettingsConfig } from '../actions';
import { SelectWidget, Toggle } from '@/ui';
import { readConfigBoolean } from '../resource-state';
import { SettingsSection } from '../components/SettingsSection';
import { SettingsRow } from '../components/SettingsRow';
import { ExpandableRow } from '../components/ExpandableRow';
import styles from '../Settings.module.css';

interface Checkpoint {
  id: string;
  ts: number;
  tool: string;
  path: string;
  size: number;
}

const RETENTION_OPTIONS = [
  { value: 1, key: 'settings.security.retention1d' },
  { value: 3, key: 'settings.security.retention3d' },
  { value: 7, key: 'settings.security.retention7d' },
];

type NetworkProxyMode = 'system' | 'manual' | 'direct';

interface NetworkProxyConfig {
  mode: NetworkProxyMode;
  httpProxy: string;
  httpsProxy: string;
  wsProxy: string;
  wssProxy: string;
  noProxy: string;
}

type NetworkProxyTextField = Exclude<keyof NetworkProxyConfig, 'mode'>;

const DEFAULT_NETWORK_PROXY: NetworkProxyConfig = {
  mode: 'system',
  httpProxy: '',
  httpsProxy: '',
  wsProxy: '',
  wssProxy: '',
  noProxy: 'localhost, 127.0.0.1, ::1',
};

function normalizeNetworkProxyDraft(value: Partial<NetworkProxyConfig> | null | undefined): NetworkProxyConfig {
  const mode = value?.mode === 'manual' || value?.mode === 'direct' ? value.mode : 'system';
  return {
    ...DEFAULT_NETWORK_PROXY,
    ...(value || {}),
    mode,
    httpProxy: value?.httpProxy || '',
    httpsProxy: value?.httpsProxy || '',
    wsProxy: value?.wsProxy || '',
    wssProxy: value?.wssProxy || '',
    noProxy: value?.noProxy || DEFAULT_NETWORK_PROXY.noProxy,
  };
}

export function SecurityTab() {
  const settingsConfig = useSettingsStore(s => s.settingsConfig);
  const platformName = useSettingsStore(s => s.platformName);
  const showToast = useSettingsStore(s => s.showToast);
  // 默认开（!== false）：和后端 preferences-manager.getSandboxNetwork / engine.getSandboxNetwork 保持一致。
  // 见 core/preferences-manager.ts:86 和 commit 51ecc435。
  const sandboxEnabled = readConfigBoolean(settingsConfig, cfg => cfg.sandbox, true);
  const isWindows = platformName === 'win32';
  const sandboxNetworkEnabled = settingsConfig
    ? isWindows || readConfigBoolean(settingsConfig, cfg => cfg.sandbox_network, true)
    : undefined;
  const sandboxNetworkDisabled = sandboxEnabled !== true || isWindows;
  // 文件备份恒开：唯一可调参数是保留时长，旧配置残留的开关/大小上限字段不再读取。
  const fileBackup = settingsConfig?.file_backup || { retention_days: 1 };

  const [checkpoints, setCheckpoints] = useState<Checkpoint[]>([]);
  const [loading, setLoading] = useState(false);
  const [proxyDraft, setProxyDraft] = useState<NetworkProxyConfig>(
    () => normalizeNetworkProxyDraft(settingsConfig?.network_proxy),
  );

  useEffect(() => {
    setProxyDraft(normalizeNetworkProxyDraft(settingsConfig?.network_proxy));
  }, [settingsConfig?.network_proxy]);

  const handleSandboxToggle = useCallback(async (on: boolean) => {
    await autoSaveConfig({ sandbox: on }, { silent: true });
    await loadSettingsConfig();
  }, []);

  const handleSandboxNetworkToggle = useCallback(async (on: boolean) => {
    await autoSaveConfig({ sandbox_network: on }, { silent: true });
    await loadSettingsConfig();
  }, []);

  const handleRetentionChange = useCallback(async (value: string) => {
    const days = parseInt(value, 10);
    const current = useSettingsStore.getState().settingsConfig?.file_backup || {};
    await autoSaveConfig({ file_backup: { ...current, retention_days: days } }, { silent: true });
    await loadSettingsConfig();
  }, []);

  const handleProxyFieldChange = useCallback((field: NetworkProxyTextField, value: string) => {
    setProxyDraft(prev => ({ ...prev, [field]: value }));
  }, []);

  const handleProxyModeChange = useCallback((mode: string) => {
    setProxyDraft(prev => ({ ...prev, mode: mode as NetworkProxyMode }));
  }, []);

  const handleProxySave = useCallback(async () => {
    const saved = await autoSaveConfig({ network_proxy: proxyDraft }, { silent: true });
    if (!saved) return;
    const latest = useSettingsStore.getState().settingsConfig?.network_proxy || proxyDraft;
    window.platform?.settingsChanged?.('network-proxy-changed', { network_proxy: latest });
    showToast(t('settings.autoSaved'), 'success');
    await loadSettingsConfig();
  }, [proxyDraft, showToast]);

  const loadCheckpoints = useCallback(async () => {
    setLoading(true);
    try {
      const res = await lingxiFetch('/api/checkpoints');
      const data = await res.json();
      setCheckpoints(data.checkpoints || []);
    } catch {
      setCheckpoints([]);
    } finally {
      setLoading(false);
    }
  }, []);

  const formatTime = (ts: number) => {
    const d = new Date(ts);
    return d.toLocaleString();
  };

  const formatPath = (p: string) => {
    const parts = p.split('/').filter(Boolean);
    if (parts.length <= 2) return p;
    return '.../' + parts.slice(-2).join('/');
  };

  const formatSize = (bytes: number) => {
    if (!Number.isFinite(bytes) || bytes < 0) return '';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`;
  };

  return (
    <div className={`${styles['settings-tab-content']} ${styles['active']}`} data-tab="security">
      <SettingsSection title={t('settings.security.sandbox')}>
        <SettingsRow
          label={t('settings.security.sandbox')}
          hint={t('settings.security.sandboxDesc')}
          control={<Toggle on={sandboxEnabled} onChange={handleSandboxToggle} />}
        />
        <SettingsRow
          label={t('settings.security.sandboxNetwork')}
          hint={isWindows
            ? t('settings.security.sandboxNetworkWin32Unsupported')
            : sandboxEnabled
            ? t('settings.security.sandboxNetworkDesc')
            : t('settings.security.sandboxNetworkDisabledDesc')}
          control={
            <Toggle
              on={sandboxNetworkEnabled}
              onChange={handleSandboxNetworkToggle}
              disabled={sandboxNetworkDisabled}
            />
          }
        />
        {sandboxEnabled === false && (
          <SettingsSection.Warning>
            {t('settings.security.sandboxWarning')}
          </SettingsSection.Warning>
        )}
      </SettingsSection>

      <SettingsSection title={t('settings.security.fileBackup')}>
        <SettingsRow
          label={t('settings.security.retention')}
          hint={t('settings.security.fileBackupDesc')}
          control={
            <SelectWidget
              value={String(fileBackup.retention_days)}
              onChange={handleRetentionChange}
              options={RETENTION_OPTIONS.map(opt => ({ value: String(opt.value), label: t(opt.key) }))}
            />
          }
        />

        <ExpandableRow
          label={t('settings.security.viewBackups')}
          count={checkpoints.length || undefined}
          onToggle={(expanded) => {
            if (expanded) loadCheckpoints();
          }}
        >
          {loading ? (
            <span className={styles['capability-row-desc']}>...</span>
          ) : checkpoints.length === 0 ? (
            <span className={styles['capability-row-desc']}>{t('settings.security.noBackups')}</span>
          ) : (
            checkpoints.map(cp => (
              <div key={cp.id} className={styles['settings-backup-item']}>
                <span className={styles['settings-backup-time']}>{formatTime(cp.ts)}</span>
                <span className={styles['settings-backup-path']}>{formatPath(cp.path)}</span>
                <span className={styles['settings-backup-size']}>{formatSize(cp.size)}</span>
              </div>
            ))
          )}
        </ExpandableRow>
      </SettingsSection>

      <SettingsSection title={t('settings.security.networkProxy')}>
        <SettingsRow
          label={t('settings.security.networkProxyMode')}
          hint={t('settings.security.networkProxyModeDesc')}
          control={
            <SelectWidget
              value={proxyDraft.mode}
              onChange={handleProxyModeChange}
              options={[
                { value: 'system', label: t('settings.security.networkProxySystem') },
                { value: 'manual', label: t('settings.security.networkProxyManual') },
                { value: 'direct', label: t('settings.security.networkProxyDirect') },
              ]}
            />
          }
        />

        {proxyDraft.mode === 'manual' && (
          <SettingsRow
            label={t('settings.security.networkProxyManualTitle')}
            hint={t('settings.security.networkProxyManualDesc')}
            layout="stacked"
            control={
              <div className={styles['settings-proxy-grid']}>
                <input
                  className={styles['settings-input']}
                  value={proxyDraft.httpProxy}
                  onChange={(e) => handleProxyFieldChange('httpProxy', e.target.value)}
                  placeholder={t('settings.security.networkProxyHttp')}
                />
                <input
                  className={styles['settings-input']}
                  value={proxyDraft.httpsProxy}
                  onChange={(e) => handleProxyFieldChange('httpsProxy', e.target.value)}
                  placeholder={t('settings.security.networkProxyHttps')}
                />
                <input
                  className={styles['settings-input']}
                  value={proxyDraft.wsProxy}
                  onChange={(e) => handleProxyFieldChange('wsProxy', e.target.value)}
                  placeholder={t('settings.security.networkProxyWs')}
                />
                <input
                  className={styles['settings-input']}
                  value={proxyDraft.wssProxy}
                  onChange={(e) => handleProxyFieldChange('wssProxy', e.target.value)}
                  placeholder={t('settings.security.networkProxyWss')}
                />
                <input
                  className={`${styles['settings-input']} ${styles['settings-proxy-wide']}`}
                  value={proxyDraft.noProxy}
                  onChange={(e) => handleProxyFieldChange('noProxy', e.target.value)}
                  placeholder={t('settings.security.networkProxyNoProxy')}
                />
              </div>
            }
          />
        )}
        <SettingsSection.Footer>
          <button className={styles['settings-btn-primary']} onClick={handleProxySave}>
            {t('settings.security.networkProxySave')}
          </button>
        </SettingsSection.Footer>
      </SettingsSection>
    </div>
  );
}

import { useEffect, useRef, useState } from 'react';
import { useSettingsStore } from '../store';
import { lingxiFetchJson } from '../api';
import { captureSettingsConnection, ownsSettingsConnection, settingsConnectionRevision } from '../connection-owner';
import { loadSettingsConfig } from '../actions';
import { t } from '../helpers';
import { SettingsSection } from './SettingsSection';
import { SettingsRow } from './SettingsRow';
import type { SettingsArchive } from '../../../../../contracts/generated/ts/lingxi-protocol';

export function SettingsTransferSection() {
  const native = useSettingsStore(s => s.activeServerConnection?.serverNodeKind === 'lingxi-service');
  const showToast = useSettingsStore(s => s.showToast);
  const connectionRevision = useSettingsStore(() => settingsConnectionRevision());
  const operation = useRef(0);
  const input = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const download = useRef<{ url: string; timer: ReturnType<typeof setTimeout> } | null>(null);
  useEffect(() => {
    // 换源后立即恢复新连接的控件，旧操作不得释放新操作的等待。
    operation.current++;
    setBusy(false);
    if (input.current) input.current.value = '';
    const releaseDownload = () => {
      if (!download.current) return;
      clearTimeout(download.current.timer);
      URL.revokeObjectURL(download.current.url);
      download.current = null;
    };
    releaseDownload();
    return releaseDownload;
  }, [connectionRevision]);
  if (!native) return null;

  const exportSettings = async () => {
    setBusy(true);
    const owner = captureSettingsConnection();
    const currentOperation = ++operation.current;
    const ownsOperation = () => operation.current === currentOperation && ownsSettingsConnection(owner);
    try {
      const archive = await lingxiFetchJson<SettingsArchive>('/api/settings/export');
      if (!ownsOperation()) return;
      const url = URL.createObjectURL(new Blob([JSON.stringify(archive, null, 2)], { type: 'application/json' }));
      // 临时下载链接只存在于本次操作，下载触发后释放内存。
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = 'lingxi-settings.json';
      if (download.current) {
        clearTimeout(download.current.timer);
        URL.revokeObjectURL(download.current.url);
      }
      download.current = { url, timer: setTimeout(() => {
        URL.revokeObjectURL(url);
        download.current = null;
      }, 1000) };
      // 即使浏览器拒绝下载，已经登记的临时链接仍会定时或在退出时释放。
      anchor.click();
    } catch (error) {
      if (!ownsOperation()) return;
      showToast(error instanceof Error ? error.message : t('settings.saveFailed'), 'error');
    } finally {
      if (ownsOperation()) setBusy(false);
    }
  };
  const importSettings = async (file: File) => {
    setBusy(true);
    const owner = captureSettingsConnection();
    const currentOperation = ++operation.current;
    const ownsOperation = () => operation.current === currentOperation && ownsSettingsConnection(owner);
    try {
      if (file.size > 2 * 1024 * 1024) throw new Error(t('settings.transfer.tooLarge'));
      const text = await file.text();
      if (!ownsOperation()) return;
      const archive = JSON.parse(text) as SettingsArchive;
      await lingxiFetchJson('/api/settings/import', {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(archive),
      });
      if (!ownsOperation()) return;
      await loadSettingsConfig();
      if (!ownsOperation()) return;
      showToast(t('settings.saved'), 'success');
    } catch (error) {
      if (!ownsOperation()) return;
      showToast(error instanceof Error ? error.message : t('settings.saveFailed'), 'error');
    } finally {
      if (ownsOperation()) setBusy(false);
      if (ownsOperation() && input.current) input.current.value = '';
    }
  };
  return (
    <SettingsSection title={t('settings.transfer.title')}>
      <SettingsRow label={t('settings.transfer.title')} hint={t('settings.transfer.hint')} control={(
        <>
          <button type="button" disabled={busy} onClick={() => void exportSettings()}>{t('settings.transfer.export')}</button>
          <button type="button" disabled={busy} onClick={() => input.current?.click()}>{t('settings.transfer.import')}</button>
          <input ref={input} hidden type="file" accept="application/json,.json" aria-label={t('settings.transfer.import')} onChange={event => {
            const file = event.currentTarget.files?.[0];
            if (file) void importSettings(file);
          }} />
        </>
      )} />
    </SettingsSection>
  );
}

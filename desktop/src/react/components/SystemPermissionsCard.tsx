/**
 * R09-T04：主窗口系统权限状态与请求卡片（仅 Tauri 宿主显示）。
 *
 * 背景：Rust/Tauri 版聊天界面在没有历史会话时不会出现“录音”入口，macOS 因此
 * 没有触发麦克风/语音识别 TCC 的用户手势路径；设置窗口的权限命令按安全设计仅
 * 允许主窗口调用（speech/notification caller=main），所以权限请求放在主窗口。
 *
 * 行为：仅在 kind==='tauri' 时渲染；按钮分别触发
 *   - 麦克风：WebView getUserMedia（Info.plist 已带 NSMicrophoneUsageDescription）
 *   - 语音识别：host 命令 speech_permission_request（release 构建才弹 TCC）
 *   - 通知：host show_notification（macOS 首次会请求通知授权）
 * 每个按钮都是真实系统请求；失败显式展示，不伪造成功。
 */
import { useCallback, useEffect, useState } from 'react';
import { getDesktopHost } from '../host/desktop-host';
import type { SpeechPermissionState } from '../types';
import { t } from '../settings/helpers';

const DISMISS_KEY = 'lingxi.system-permissions.dismissed';

type Status = 'idle' | 'pending' | 'ok' | 'denied' | 'unsupported' | 'error';

export function SystemPermissionsCard() {
  const hostKind = getDesktopHost().kind;
  const supportedHost = hostKind === 'tauri' || hostKind === 'electron';
  const [dismissed, setDismissed] = useState(() => {
    try { return localStorage.getItem(DISMISS_KEY) === '1'; } catch { return false; }
  });
  const [speech, setSpeech] = useState<SpeechPermissionState | null>(null);
  const [mic, setMic] = useState<Status>('idle');
  const [notice, setNotice] = useState<Status>('idle');
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    if (!supportedHost) return;
    const api = getDesktopHost().api;
    if (!api.speechPermissionStatus) return;
    let cancelled = false;
    api.speechPermissionStatus()
      .then((state) => { if (!cancelled) setSpeech(state); })
      .catch(() => { if (!cancelled) setSpeech('bridge_unavailable'); });
    return () => { cancelled = true; };
  }, [supportedHost]);

  const requestMic = useCallback(async () => {
    setBusy('mic');
    setMessage(null);
    try {
      if (!navigator.mediaDevices?.getUserMedia) {
        setMic('unsupported');
        setMessage(t('systemPermissions.micUnsupported'));
        return;
      }
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      stream.getTracks().forEach((track) => track.stop());
      setMic('ok');
      setMessage(t('systemPermissions.micGranted'));
    } catch {
      setMic('denied');
      setMessage(t('systemPermissions.micDenied'));
    } finally {
      setBusy(null);
    }
  }, []);

  const requestSpeech = useCallback(async () => {
    setBusy('speech');
    setMessage(null);
    try {
      const api = getDesktopHost().api;
      const state = await api.speechPermissionRequest?.();
      setSpeech(state ?? 'bridge_unavailable');
      setMessage(state === 'authorized'
        ? t('systemPermissions.speechGranted')
        : t('systemPermissions.speechResult'));
    } catch {
      setSpeech('denied');
      setMessage(t('systemPermissions.speechDenied'));
    } finally {
      setBusy(null);
    }
  }, []);

  const testNotification = useCallback(async () => {
    setBusy('notice');
    setMessage(null);
    try {
      const api = getDesktopHost().api;
      if (!api.showNotification) {
        setNotice('unsupported');
        return;
      }
      const receipt = await api.showNotification(
        t('systemPermissions.notificationTitle'),
        t('systemPermissions.notificationBody'),
      );
      const shown = typeof receipt === 'object' && receipt !== null && 'shown' in receipt
        ? (receipt as { shown?: boolean }).shown !== false
        : true;
      setNotice(shown ? 'ok' : 'error');
      setMessage(shown ? t('systemPermissions.notificationSent') : t('systemPermissions.notificationFailed'));
    } catch {
      setNotice('error');
      setMessage(t('systemPermissions.notificationFailed'));
    } finally {
      setBusy(null);
    }
  }, []);

  if (!supportedHost || dismissed) return null;

  const statusText = (status: Status) => {
    if (status === 'ok') return t('systemPermissions.granted');
    if (status === 'denied') return t('systemPermissions.denied');
    if (status === 'unsupported') return t('systemPermissions.unsupported');
    if (status === 'error') return t('systemPermissions.failed');
    return t('systemPermissions.notRequested');
  };

  return (
    <div
      data-testid="system-permissions-card"
      style={{
        position: 'fixed', right: 18, bottom: 18, zIndex: 2147483000,
        width: 330, padding: 14, borderRadius: 12, fontSize: 12,
        background: 'rgba(30,30,30,0.96)', color: '#f2f2f2',
        boxShadow: '0 8px 28px rgba(0,0,0,0.35)', lineHeight: 1.5,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
        <strong style={{ fontSize: 13 }}>{t('systemPermissions.title')}</strong>
        <button
          type="button"
          aria-label={t('systemPermissions.dismiss')}
          onClick={() => {
            try { localStorage.setItem(DISMISS_KEY, '1'); } catch { /* 忽略存储失败 */ }
            setDismissed(true);
          }}
          style={{ background: 'transparent', border: 0, color: '#aaa', cursor: 'pointer', fontSize: 14 }}
        >
          ×
        </button>
      </div>
      <div style={{ marginTop: 6, color: '#bbb' }}>{t('systemPermissions.description')}</div>

      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginTop: 10 }}>
        <span>{t('systemPermissions.microphone')}: {statusText(mic)}</span>
        <button type="button" disabled={busy !== null} onClick={requestMic}>
          {t('systemPermissions.request')}
        </button>
      </div>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginTop: 6 }}>
        <span>
          {t('systemPermissions.speech')}: {speech === 'authorized' ? t('systemPermissions.granted') : (speech ?? '…')}
        </span>
        <button type="button" disabled={busy !== null} onClick={requestSpeech}>
          {t('systemPermissions.request')}
        </button>
      </div>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginTop: 6 }}>
        <span>{t('systemPermissions.notification')}: {statusText(notice)}</span>
        <button type="button" disabled={busy !== null} onClick={testNotification}>
          {t('systemPermissions.testNotification')}
        </button>
      </div>
      {message && <div style={{ marginTop: 8, color: '#9fd3a6' }}>{message}</div>}
    </div>
  );
}

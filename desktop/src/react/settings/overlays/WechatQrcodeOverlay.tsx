import { useState, useEffect, useRef, useCallback } from 'react';
import { lingxiFetch } from '../api';
import { t } from '../helpers';
import { useSettingsStore } from '../store';
import { bridgeConnectionKey, bridgePublicConnectionKey, readBridgeResponse } from '../../services/bridge-response';
import { Overlay } from '../../ui';
import styles from './WechatQrcodeOverlay.module.css';

type QrStatus = 'loading' | 'waiting' | 'scanned' | 'confirmed' | 'expired' | 'error';
interface LoginFlow { agentId: string; connectionKey: string; abort: AbortController; timers: Set<ReturnType<typeof setTimeout>> }

export function WechatQrcodeOverlay() {
  const [visible, setVisible] = useState(false);
  const [qrcodeUrl, setQrcodeUrl] = useState('');
  const [status, setStatus] = useState<QrStatus>('loading');
  const [error, setError] = useState('');
  const activeRef = useRef<LoginFlow | null>(null);
  const connectionKey = useSettingsStore(state => bridgeConnectionKey(state));
  const stop = useCallback(() => {
    const previous = activeRef.current;
    activeRef.current = null;
    previous?.abort.abort();
    previous?.timers.forEach(timer => clearTimeout(timer));
  }, []);
  const close = useCallback(() => { stop(); setVisible(false); setQrcodeUrl(''); setStatus('loading'); setError(''); }, [stop]);
  const owns = (flow: LoginFlow) => activeRef.current === flow && !flow.abort.signal.aborted
    && flow.connectionKey === bridgeConnectionKey(useSettingsStore.getState());

  const begin = useCallback(async (agentId: string) => {
    stop(); setVisible(true); setStatus('loading'); setError(''); setQrcodeUrl('');
    if (!agentId) { setStatus('error'); setError('请先选择要绑定的助手'); return; }
    const flow: LoginFlow = { agentId, connectionKey: bridgeConnectionKey(useSettingsStore.getState()), abort: new AbortController(), timers: new Set() };
    activeRef.current = flow;
    const query = `?agentId=${encodeURIComponent(flow.agentId)}`;
    const post = async (path: string, body?: unknown, timeout?: number) => {
      if (!owns(flow)) throw new Error('连接已切换');
      const result = await readBridgeResponse(await lingxiFetch(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, ...(body === undefined ? {} : { body: JSON.stringify(body) }), signal: flow.abort.signal, timeout }));
      if (!owns(flow)) throw new Error('连接已切换');
      return result;
    };
    const wait = () => new Promise<void>(resolve => {
      const onAbort = () => { clearTimeout(timer); flow.timers.delete(timer); resolve(); };
      const timer = setTimeout(() => { flow.abort.signal.removeEventListener('abort', onAbort); flow.timers.delete(timer); resolve(); }, 1000);
      flow.timers.add(timer);
      flow.abort.signal.addEventListener('abort', onAbort, { once: true });
    });
    try {
      for (let refresh = 0; refresh < 3 && owns(flow); refresh++) {
        const qr = await post(`/api/bridge/wechat/qrcode${query}`);
        if (qr.ok !== true || typeof qr.qrcodeUrl !== 'string' || typeof qr.qrcodeId !== 'string') throw new Error('二维码返回内容无效');
        setQrcodeUrl(qr.qrcodeUrl); setStatus('waiting');
        while (owns(flow)) {
          const result = await post(`/api/bridge/wechat/qrcode-status${query}`, { qrcodeId: qr.qrcodeId }, 45_000);
          if (result.status === 'confirmed') {
            if (typeof result.botToken !== 'string' || !result.botToken || typeof result.userId !== 'string' || !result.userId) throw new Error('扫码结果缺少账号信息，尚未启用');
            // 先保留为停用，再绑定主人；两步均真实成功后才启用接收，避免短暂误判访客。
            await post(`/api/bridge/config${query}`, { platform: 'wechat', credentials: { botToken: result.botToken }, enabled: false });
            await post(`/api/bridge/owner${query}`, { platform: 'wechat', userId: result.userId });
            await post(`/api/bridge/config${query}`, { platform: 'wechat', credentials: null, enabled: true });
            setStatus('confirmed');
            window.dispatchEvent(new CustomEvent('hana-bridge-reload', { detail: { agentId: flow.agentId, connectionKey: bridgePublicConnectionKey(useSettingsStore.getState()) } }));
            const timer = setTimeout(() => { flow.timers.delete(timer); if (owns(flow)) close(); }, 1200);
            flow.timers.add(timer);
            return;
          }
          if (result.status === 'expired') { setStatus('expired'); break; }
          if (result.status !== 'waiting' && result.status !== 'scanned') throw new Error(result.error || '扫码状态读取失败');
          setStatus(result.status);
          await wait();
        }
      }
      if (owns(flow)) { setStatus('error'); setError(t('settings.bridge.wechatExpired')); }
    } catch (failure) {
      if (owns(flow)) { setStatus('error'); setError(failure instanceof Error ? failure.message : t('settings.bridge.wechatLoginFailed')); }
    }
  }, [close, stop]);

  useEffect(() => {
    if (activeRef.current && activeRef.current.connectionKey !== connectionKey) {
      stop(); setStatus('error'); setError('连接已切换，请重新扫码');
    }
  }, [connectionKey, stop]);
  useEffect(() => {
    const show = (event: Event) => { void begin((event as CustomEvent<{ agentId?: string }>).detail?.agentId || ''); };
    window.addEventListener('hana-show-wechat-qrcode', show);
    return () => { window.removeEventListener('hana-show-wechat-qrcode', show); stop(); };
  }, [begin, stop]);

  const statusLabel = (() => {
    switch (status) {
      case 'loading': return t('settings.bridge.wechatScanning');
      case 'waiting': return t('settings.bridge.wechatScanning');
      case 'scanned': return t('settings.bridge.wechatScanned');
      case 'confirmed': return t('settings.bridge.wechatLoginSuccess');
      case 'expired': return t('settings.bridge.wechatExpired');
      case 'error': return error || t('settings.bridge.wechatLoginFailed');
      default: return '';
    }
  })();

  const statusClass = status === 'confirmed' ? styles.success
    : (status === 'error' || status === 'expired') ? styles.error
    : '';

  return (
    <Overlay
      scope="inline"
      open={visible}
      onClose={close}
      backdrop="blur"
      zIndex={100}
      className={styles.card}
      disableContainerAnimation
    >
        <button className={styles.closeBtn} onClick={close} aria-label="close">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>

        <div className={styles.title}>{t('settings.bridge.wechat')}</div>

        <div className={styles.qrcodeContainer}>
          {status === 'loading' && <span className={styles.loading}>...</span>}
          {qrcodeUrl && status !== 'loading' && (
            <img className={styles.qrcodeImg} src={qrcodeUrl} alt="WeChat QR Code" />
          )}
        </div>

        <div role={status === 'error' ? 'alert' : 'status'} className={`${styles.statusText} ${statusClass}`}>{statusLabel}</div>
    </Overlay>
  );
}

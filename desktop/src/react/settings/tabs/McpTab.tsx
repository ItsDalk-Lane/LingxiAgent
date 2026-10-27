import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useSettingsStore } from '../store';
import { t } from '../helpers';
import { captureSettingsConnection, ownsSettingsConnection } from '../connection-owner';
import { resolveServerConnection } from '../../services/service-client';
import { SettingsSection } from '../components/SettingsSection';
import { ConfirmDialog, Toggle } from '@/ui';
import { ConnectorDetail } from './mcp/ConnectorDetail';
import { DeferSettings } from './mcp/DeferSettings';
import { ConnectorForm } from './mcp/ConnectorForm';
import { ConnectorImport } from './mcp/ConnectorImport';
import { ConnectorList } from './mcp/ConnectorList';
import {
  EMPTY_MCP_STATE,
  addMcpConnector,
  addMcpConnectorsBulk,
  cancelMcpOAuth,
  loadMcpState,
  logoutMcpOAuth,
  pollMcpOAuth,
  removeMcpConnector,
  runMcpConnectorAction,
  setAgentMcpConnector,
  setAgentMcpTools,
  setMcpDeferSettings,
  setMcpEnabled,
  startMcpOAuth,
  updateMcpConnector,
  updateMcpConnectorPolicy,
} from './mcp/mcp-api';
import type {
  McpConnectorInput,
  McpPermissionMode,
  McpToolPermission,
} from './mcp/types';
import styles from '../Settings.module.css';
import { usePlatform } from '../../hooks/use-platform';

const platform = usePlatform();

/** How long a browser OAuth round trip is waited on before it is given up as stale. */
const OAUTH_POLL_INTERVAL_MS = 3000;
const OAUTH_POLL_ATTEMPTS = 100;

export function McpTab() {
  const currentAgentId = useSettingsStore(s => s.currentAgentId);
  const connection = useSettingsStore(s => resolveServerConnection(s));
  const previousConnectionRef = useRef(connection);
  const showToast = useSettingsStore(s => s.showToast);
  const [viewAgentId, setViewAgentId] = useState<string | null>(currentAgentId);
  const viewAgentIdRef = useRef(viewAgentId);
  viewAgentIdRef.current = viewAgentId;

  const [state, setState] = useState(EMPTY_MCP_STATE);
  const [loadingState, setLoadingState] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  // One key per in-flight mutation. Two connectors can be busy at once without
  // one disabling the other's controls, and edit no longer shares remove's key.
  const [busyKeys, setBusyKeys] = useState<ReadonlySet<string>>(() => new Set());
  const [openConnectorId, setOpenConnectorId] = useState<string | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const [pendingRemoveId, setPendingRemoveId] = useState<string | null>(null);
  const [oauthWaitingId, setOAuthWaitingId] = useState<string | null>(null);
  const oauthCancelledRef = useRef<Set<string>>(new Set());
  const operationSequenceRef = useRef(0);
  const operationOwnersRef = useRef(new Map<string, number>());
  const viewRevisionRef = useRef(0);
  const beginOperation = (key: string) => {
    const token = ++operationSequenceRef.current;
    operationOwnersRef.current.set(key, token);
    const owner = captureSettingsConnection();
    const agent = viewAgentIdRef.current;
    const revision = viewRevisionRef.current;
    return () => ownsSettingsConnection(owner) && viewAgentIdRef.current === agent
      && viewRevisionRef.current === revision && operationOwnersRef.current.get(key) === token;
  };
  // Only the newest load may write state; a slower earlier response must not
  // overwrite a fresher one after concurrent mutations.
  const loadSequenceRef = useRef(0);

  useEffect(() => {
    if (!viewAgentId && currentAgentId) setViewAgentId(currentAgentId);
  }, [currentAgentId, viewAgentId]);

  const loadState = useCallback(async () => {
    const agentId = viewAgentIdRef.current;
    const sequence = loadSequenceRef.current + 1;
    loadSequenceRef.current = sequence;
    setLoadingState(true);
    const owner = captureSettingsConnection();
    try {
      const data = await loadMcpState(agentId);
      if (loadSequenceRef.current !== sequence || viewAgentIdRef.current !== agentId || !ownsSettingsConnection(owner)) return false;
      setState(data);
      setLoadError(null);
      return true;
    } catch (err) {
      console.error('[mcp] load failed:', err);
      if (loadSequenceRef.current === sequence && viewAgentIdRef.current === agentId && ownsSettingsConnection(owner)) {
        setLoadError(err instanceof Error ? err.message : String(err));
      }
      return false;
    } finally {
      if (loadSequenceRef.current === sequence) setLoadingState(false);
    }
  }, []);

  useEffect(() => {
    viewRevisionRef.current += 1;
    operationOwnersRef.current.clear();
    oauthCancelledRef.current.clear();
    setBusyKeys(new Set());
    setOAuthWaitingId(null);
    setAddOpen(false);
    setImportOpen(false);
    setPendingRemoveId(null);
    // 换连接/助手先撤回旧权限投影；读取失败保留错误，不能伪装没有连接器。
    if (previousConnectionRef.current !== connection) {
      previousConnectionRef.current = connection;
      setState(EMPTY_MCP_STATE);
      setOpenConnectorId(null);
    } else {
      setState(previous => ({ ...previous, agentConfig: { connectors: {} } }));
    }
    setLoadError(null);
    void loadState();
  }, [loadState, viewAgentId, connection]);

  useEffect(() => {
    // 只在真实后台启动尚未结束时更新状态；离开页面或换连接即回收定时器。
    if (loadingState || loadError || !state.connectors.some(connector => connector.status === 'connecting')) return;
    const timer = window.setTimeout(() => { void loadState(); }, 500);
    return () => window.clearTimeout(timer);
  }, [state, loadingState, loadError, loadState, connection, viewAgentId]);

  useEffect(() => () => {
    viewRevisionRef.current += 1;
    operationOwnersRef.current.clear();
    loadSequenceRef.current += 1;
  }, []);

  const run = async (key: string, action: (current: () => boolean) => Promise<void>) => {
    const current = beginOperation(key);
    setBusyKeys(current => new Set(current).add(key));
    try {
      await action(current);
      if (!current()) return;
      const loaded = await loadState();
      if (!current()) return;
      if (!loaded) throw new Error('MCP 设置可能已保存，但状态刷新未确认');
      showToast(t('settings.autoSaved'), 'success');
    } catch (err: unknown) {
      if (current()) {
        // 启动失败等动作可能已保存用户意图，仍须读取真实结果。
        await loadState();
        if (!current()) return;
        showToast(t('settings.saveFailed') + ': ' + (err instanceof Error ? err.message : String(err)), 'error');
      }
    } finally {
      if (current()) {
        operationOwnersRef.current.delete(key);
        setBusyKeys(keys => { const next = new Set(keys); next.delete(key); return next; });
      }
    }
  };

  const requireAgentId = () => {
    const agentId = viewAgentIdRef.current;
    if (!agentId) throw new Error('agentId is required');
    return agentId;
  };

  const toggleGlobal = (enabled: boolean) => run('global', () => setMcpEnabled(enabled));
  const toggleGlobalFromRow = () => {
    if (loadingState || loadError || busyKeys.has('global')) return;
    toggleGlobal(!state.enabled);
  };

  const addConnector = (input: McpConnectorInput) => run('add', async current => {
    await addMcpConnector(input);
    if (current()) setAddOpen(false);
  });

  const updateConnector = (connectorId: string, input: McpConnectorInput) =>
    run(`update-${connectorId}`, () => updateMcpConnector(connectorId, input));

  const importConnectors = async (connectors: McpConnectorInput[]) => {
    const current = beginOperation('import-json');
    setBusyKeys(keys => new Set(keys).add('import-json'));
    try {
      const results = await addMcpConnectorsBulk(connectors);
      if (!current()) throw new Error('连接已改变，旧导入回执不能用于当前页面');
      const loaded = await loadState();
      if (!current()) throw new Error('连接已改变，旧导入状态不能用于当前页面');
      if (!loaded) throw Object.assign(new Error('导入结果已返回，但状态刷新未确认'), { results });
      return results;
    } finally {
      if (current()) {
        operationOwnersRef.current.delete('import-json');
        setBusyKeys(keys => { const next = new Set(keys); next.delete('import-json'); return next; });
      }
    }
  };

  const connectorAction = (connectorId: string, action: 'start' | 'stop' | 'refresh-tools') =>
    run(`${action}-${connectorId}`, () => runMcpConnectorAction(connectorId, action));

  const removeConnector = (connectorId: string) => run(`remove-${connectorId}`, async current => {
    await removeMcpConnector(connectorId);
    if (!current()) return;
    setPendingRemoveId(null);
    // The page it was opened on no longer describes anything.
    setOpenConnectorId(current => (current === connectorId ? null : current));
  });

  const setAgentConnector = (connectorId: string, enabled: boolean) =>
    run(`agent-connector-${connectorId}`, () => setAgentMcpConnector(requireAgentId(), connectorId, enabled));

  const setAgentTools = (connectorId: string, tools: Record<string, boolean>) =>
    run(`tools-${connectorId}`, () => setAgentMcpTools(requireAgentId(), connectorId, tools));

  const setToolPermission = (connectorId: string, toolName: string, permission: McpToolPermission) =>
    run(`policy-${connectorId}`, () => {
      const connector = state.connectors.find(item => item.id === connectorId);
      return updateMcpConnectorPolicy(connectorId, {
        toolPermissions: { ...(connector?.toolPermissions || {}), [toolName]: permission },
      });
    });

  const setToolPinned = (connectorId: string, toolName: string, pinned: boolean) =>
    run(`policy-${connectorId}`, () => {
      const connector = state.connectors.find(item => item.id === connectorId);
      const pinnedTools = { ...(connector?.pinnedTools || {}) };
      if (pinned) pinnedTools[toolName] = true;
      else delete pinnedTools[toolName];
      return updateMcpConnectorPolicy(connectorId, { pinnedTools });
    });

  const setPermissionMode = (connectorId: string, permissionMode: McpPermissionMode) =>
    run(`policy-${connectorId}`, () => updateMcpConnectorPolicy(connectorId, { permissionMode }));

  const setTrustReadOnly = (connectorId: string, trustReadOnlyHint: boolean) =>
    run(`policy-${connectorId}`, () => updateMcpConnectorPolicy(connectorId, { trustReadOnlyHint }));

  const changeDefer = (patch: { deferEnabled?: boolean; deferThreshold?: number; builtinDeferEnabled?: boolean }) =>
    run('defer', () => setMcpDeferSettings(patch));

  const connectOAuth = async (connectorId: string) => {
    const key = `oauth-${connectorId}`;
    const current = beginOperation(key);
    oauthCancelledRef.current.delete(connectorId);
    setOAuthWaitingId(connectorId);
    const owner = captureSettingsConnection();
    try {
      const { sessionId, url } = await startMcpOAuth(connectorId);
      if (!current() || oauthCancelledRef.current.has(connectorId)) return;
      try {
        if (!platform?.openExternal) throw new Error(t('settings.oauth.browserUnavailable'));
        const opened = await platform.openExternal(url);
        if (opened && opened.ok === false) throw new Error(opened.error || t('settings.oauth.browserUnavailable'));
      } catch (error) {
        if (current() && !oauthCancelledRef.current.has(connectorId)) {
          try { await cancelMcpOAuth(connectorId); }
          catch (cleanupError) {
            throw new Error(`${error instanceof Error ? error.message : String(error)}；登录等待取消未确认：${cleanupError instanceof Error ? cleanupError.message : String(cleanupError)}`);
          }
        }
        throw error;
      }
      if (!current() || oauthCancelledRef.current.has(connectorId)) return;
      if (!await waitForOAuth(sessionId, owner, () => current() && !oauthCancelledRef.current.has(connectorId))) return;
      if (!current() || oauthCancelledRef.current.has(connectorId)) return;
      const loaded = await loadState();
      if (!current() || oauthCancelledRef.current.has(connectorId)) return;
      if (!loaded) throw new Error('MCP登录可能已保存，但状态刷新未确认');
      showToast(t('settings.autoSaved'), 'success');
    } catch (err: unknown) {
      if (current() && !oauthCancelledRef.current.has(connectorId)) {
        showToast(t('settings.saveFailed') + ': ' + (err instanceof Error ? err.message : String(err)), 'error');
      }
    } finally {
      if (current()) {
        operationOwnersRef.current.delete(key);
        oauthCancelledRef.current.delete(connectorId);
        setOAuthWaitingId(previous => (previous === connectorId ? null : previous));
      }
    }
  };

  const cancelOAuth = async (connectorId: string) => {
    const owner = captureSettingsConnection();
    const revision = viewRevisionRef.current;
    const original = operationOwnersRef.current.get(`oauth-${connectorId}`);
    const current = () => ownsSettingsConnection(owner) && viewRevisionRef.current === revision
      && operationOwnersRef.current.get(`oauth-${connectorId}`) === original;
    oauthCancelledRef.current.add(connectorId);
    setOAuthWaitingId(null);
    try {
      await cancelMcpOAuth(connectorId);
    } catch (err: unknown) {
      if (current()) showToast(t('settings.saveFailed') + ': ' + (err instanceof Error ? err.message : String(err)), 'error');
    }
  };

  const disconnectOAuth = (connectorId: string) =>
    run(`oauth-logout-${connectorId}`, () => logoutMcpOAuth(connectorId));

  const openConnector = state.connectors.find(connector => connector.id === openConnectorId) || null;
  const errorNotice = loadError && <div role="alert">{t('settings.refreshFailed')}: {loadError} <button type="button" onClick={() => { void loadState(); }}>{t('action.retry')}</button></div>;

  if (openConnector) {
    return (
      <div className={`${styles['settings-tab-content']} ${styles['active']}`} data-tab="mcp">
        {errorNotice}
        <ConnectorDetail
          connector={openConnector}
          globalEnabled={state.enabled && !loadingState && !loadError}
          busyKeys={busyKeys}
          viewAgentId={viewAgentId}
          agentConfig={state.agentConfig}
          oauthWaiting={oauthWaitingId === openConnector.id}
          onBack={() => setOpenConnectorId(null)}
          onUpdate={updateConnector}
          onAction={connectorAction}
          onAgentChange={setViewAgentId}
          onConnectorToggle={setAgentConnector}
          onToolsEnabledChange={setAgentTools}
          onToolPermissionChange={setToolPermission}
          onToolPinnedChange={setToolPinned}
          onPermissionModeChange={setPermissionMode}
          onTrustReadOnlyChange={setTrustReadOnly}
          onOAuthStart={connectOAuth}
          onOAuthCancel={cancelOAuth}
          onOAuthLogout={disconnectOAuth}
        />
      </div>
    );
  }

  return (
    <div className={`${styles['settings-tab-content']} ${styles['active']}`} data-tab="mcp">
      {errorNotice}
      <SettingsSection title={t('settings.mcp.masterTitle')}>
        <div
          className={styles['skills-list-item']}
          tabIndex={busyKeys.has('global') ? -1 : 0}
          onClick={toggleGlobalFromRow}
          onKeyDown={(e) => {
            if (e.key !== 'Enter' && e.key !== ' ') return;
            e.preventDefault();
            toggleGlobalFromRow();
          }}
        >
          <div className={styles['skills-list-info']}>
            <div className={styles['skills-list-name']}>{t('settings.mcp.masterName')}</div>
            <div className={styles['skills-list-desc']}>{t('settings.mcp.masterDesc')}</div>
          </div>
          <div className={styles['skills-list-actions']}>
            <Toggle
              on={loadingState ? undefined : state.enabled}
              onChange={toggleGlobal}
              disabled={busyKeys.has('global') || loadingState || !!loadError}
              label={loadingState ? t('status.loading') : state.enabled ? t('common.on') : t('common.off')}
            />
          </div>
        </div>
        <DeferSettings
          deferEnabled={state.deferEnabled}
          deferThreshold={state.deferThreshold}
          builtinDeferEnabled={state.builtinDeferEnabled}
          busy={busyKeys.has('defer') || loadingState || !!loadError || !state.enabled}
          onChange={changeDefer}
        />
      </SettingsSection>

      <SettingsSection title={t('settings.mcp.connectorsTitle')} surface="plain">
        <div className={styles['pv-add-form-actions']}>
          <button
            className={styles['pv-add-form-btn']}
            type="button"
            disabled={loadingState || !!loadError}
            onClick={() => { setAddOpen(open => !open); setImportOpen(false); }}
          >
            {t('settings.mcp.addConnector')}
          </button>
          <button
            className={styles['pv-add-form-btn']}
            type="button"
            disabled={loadingState || !!loadError}
            onClick={() => { setImportOpen(open => !open); setAddOpen(false); }}
          >
            {t('settings.mcp.importJson')}
          </button>
        </div>

        {importOpen && (
          <ConnectorImport
            busy={busyKeys.has('import-json')}
            onImport={importConnectors}
            onDone={() => setImportOpen(false)}
            onCancel={() => setImportOpen(false)}
          />
        )}

        {addOpen && (
          <ConnectorForm
            disabled={busyKeys.has('add')}
            editingConnector={null}
            onAdd={addConnector}
            onCancelEdit={() => setAddOpen(false)}
          />
        )}

        {(!loadError || state.connectors.length > 0) && <ConnectorList
          connectors={state.connectors}
          globalEnabled={state.enabled}
          loading={loadingState}
          busyKeys={busyKeys}
          agentConfig={state.agentConfig}
          onOpen={id => {
            if (!viewAgentId) { showToast(t('settings.mcp.selectAgentForDetails'), 'error'); return; }
            setOpenConnectorId(id);
          }}
          onAction={connectorAction}
          onRemove={setPendingRemoveId}
        />}
      </SettingsSection>

      <ConfirmDialog
        open={pendingRemoveId !== null}
        scope="window"
        title={t('settings.mcp.removeTitle')}
        confirmLabel={t('common.remove')}
        cancelLabel={t('common.cancel')}
        confirmTone="danger"
        busy={pendingRemoveId ? busyKeys.has(`remove-${pendingRemoveId}`) : false}
        onConfirm={() => pendingRemoveId && removeConnector(pendingRemoveId)}
        onCancel={() => setPendingRemoveId(null)}
      >
        {t('settings.mcp.removeConfirm')}
      </ConfirmDialog>
    </div>
  );
}

async function waitForOAuth(sessionId: string, owner: ReturnType<typeof captureSettingsConnection>, current: () => boolean): Promise<boolean> {
  for (let i = 0; i < OAUTH_POLL_ATTEMPTS; i += 1) {
    await new Promise(resolve => setTimeout(resolve, OAUTH_POLL_INTERVAL_MS));
    if (!current()) return false;
    if (!ownsSettingsConnection(owner)) throw new Error('连接已改变，原MCP登录等待已结束');
    const status = await pollMcpOAuth(sessionId);
    if (!current()) return false;
    if (!ownsSettingsConnection(owner)) throw new Error('连接已改变，原MCP登录回执已丢弃');
    if (status.status === 'done') return true;
    if (status.status === 'cancelled') return false;
    if (status.status === 'missing') throw new Error(t('settings.mcp.oauthFailed'));
    if (status.status === 'error') throw new Error(status.error || t('settings.mcp.oauthFailed'));
  }
  throw new Error(t('settings.mcp.oauthTimeout'));
}

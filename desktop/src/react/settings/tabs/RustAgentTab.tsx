import { captureSettingsConnection, ownsSettingsConnection, settingsConnectionRevision } from '../connection-owner';
import { useEffect, useState } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { useSettingsStore } from '../store';
import { lingxiFetchJson, settingsConnectionKey } from '../api';
import { loadRustAgents } from '../rust-agents';
import { t } from '../helpers';
import { SettingsSection } from '../components/SettingsSection';
import { AgentTab } from './AgentTab';
import { AgentDeletionStatus } from './agent/AgentDeletionStatus';
import { browseAgent, loadSettingsConfig, loadSettingsModels, loadProvidersSummary } from '../actions';
import styles from '../Settings.module.css';

/** 当前React桌面设置入口的Rust助手管理页，复用现役删除确认弹窗。 */
export function RustAgentTab() {
  const { agents, selectedId } = useSettingsStore(useShallow(state => ({
    agents: state.agents, selectedId: state.settingsAgentId || state.currentAgentId,
  })));
  const connectionKey = settingsConnectionKey();
  const revision = settingsConnectionRevision();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  useEffect(() => {
    const owner = captureSettingsConnection();
    const controller = new AbortController();
    setLoading(true);
    setError('');
    void (async () => {
      try {
        const identity = await lingxiFetchJson<{ serverNodeKind: string }>('/lingxi/v1/server/identity', { signal: controller.signal });
        if (identity.serverNodeKind !== 'lingxi-service') throw new Error('助手服务身份不匹配');
        if (!ownsSettingsConnection(owner) || controller.signal.aborted) return;
        await loadRustAgents(controller.signal);
        if (!controller.signal.aborted) await Promise.all([loadSettingsConfig(), loadSettingsModels()]);
        if (!controller.signal.aborted) void loadProvidersSummary().catch(() => {});
        if (!controller.signal.aborted && useSettingsStore.getState().toastMessage === t('settings.rustCoreUnavailable')) {
          useSettingsStore.setState({ toastVisible: false });
        }
      } catch (failure) {
        if (!controller.signal.aborted) setError(`${t('settings.rustCoreUnavailable')}: ${failure instanceof Error ? failure.message : String(failure)}`);
      } finally {
        if (!controller.signal.aborted) setLoading(false);
      }
    })();
      return () => controller.abort();
  }, [connectionKey, revision]);
  const { settingsAgentsOwner, settingsDataOwner, settingsDataAgentId, settingsConfig, settingsConfigError, agentsError, runtimeModelsError, providersSummaryError } = useSettingsStore();
  const owned = !!settingsConfig && ownsSettingsConnection(settingsDataOwner) && settingsDataAgentId === selectedId;
  const guard = (event: React.SyntheticEvent) => {
    if (!ownsSettingsConnection(settingsAgentsOwner) || useSettingsStore.getState().settingsAgentsOwner !== settingsAgentsOwner) { event.preventDefault(); event.stopPropagation(); }
  };
  return (
    <SettingsSection title={t('settings.agent.title')}>
      {[error, settingsConfigError, agentsError, runtimeModelsError, providersSummaryError].filter(Boolean).map((message, index) => <div role="alert" key={index}>{message}</div>)}
      <AgentDeletionStatus />
      {!error && ownsSettingsConnection(settingsAgentsOwner) && (
        <>
          <select className={styles['settings-input']} aria-label={t('settings.agent.title')}
            value={selectedId || ''} disabled={loading}
            onChange={event => { void browseAgent(event.target.value); }}>
            {agents.map(agent => <option key={agent.id} value={agent.id}>{agent.name}</option>)}
          </select>
          {agents.length < 2 && !loading && <p>{t('settings.agent.lastAgent')}</p>}
          <div onClickCapture={guard} onKeyDownCapture={guard} onPointerDownCapture={guard} onPointerMoveCapture={guard} onPointerUpCapture={guard} onChangeCapture={guard}><AgentTab configurationReady={owned} /></div>
        </>
      )}
    </SettingsSection>
  );
}

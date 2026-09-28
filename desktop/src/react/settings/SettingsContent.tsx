import React, { useCallback, useEffect, useRef } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { useSettingsStore, type SettingsSnapshot, type SettingsStore } from './store';
import { createRemoteResource } from './resource-state';
import { lingxiFetch } from './api';
import {
  createLocalServerConnection,
  readPersistedServerConnectionState,
  refreshLocalServerConnectionState,
  upsertServerConnection,
  validateRestartTransport,
  LOCAL_CONNECTION_ID,
  type ServerConnection,
} from '../services/server-connection';
import { t } from './helpers';
import { loadAgents, loadAvatars, loadProvidersSummary, loadSettingsModels, loadSettingsSnapshot } from './actions';
import { ErrorBoundary } from '../components/ErrorBoundary';
import { SettingsNav } from './SettingsNav';
import { Toast } from './Toast';
import { AgentTab } from './tabs/AgentTab';
import { MeTab } from './tabs/MeTab';
import { InterfaceTab } from './tabs/InterfaceTab';
import { GeneralTab } from './tabs/GeneralTab';
import { KeybindingsTab } from './tabs/KeybindingsTab';
import { BrowserTab } from './tabs/BrowserTab';
import { WorkTab } from './tabs/WorkTab';
import { SkillsTab } from './tabs/SkillsTab';
import { McpTab } from './tabs/McpTab';
import { BridgeTab } from './tabs/BridgeTab';
import { ProvidersTab } from './tabs/ProvidersTab';
import { ModelsTab } from './tabs/ModelsTab';
import { UsageTab } from './tabs/UsageTab';
import { AboutTab } from './tabs/AboutTab';
import { ExperimentsTab } from './tabs/ExperimentsTab';
import { SecurityTab } from './tabs/SecurityTab';
import { SharingTab } from './tabs/SharingTab';
import { AccessTab } from './tabs/AccessTab';
import { EnvDepsTab } from './tabs/EnvDepsTab';
import { CropOverlay } from './overlays/CropOverlay';
import { AgentCreateOverlay } from './overlays/AgentCreateOverlay';
import { AgentDeleteOverlay } from './overlays/AgentDeleteOverlay';
import { MemoryViewer } from './overlays/MemoryViewer';
import { CompiledMemoryViewer } from './overlays/CompiledMemoryViewer';
import { ClearMemoryConfirm } from './overlays/ClearMemoryConfirm';
import { BridgeTutorial } from './overlays/BridgeTutorial';
import { WechatQrcodeOverlay } from './overlays/WechatQrcodeOverlay';
import { InputContextMenu } from '../components/InputContextMenu';
import { SettingsPage } from './components/SettingsPrimitives';
import styles from './Settings.module.css';

const TAB_COMPONENTS: Record<string, React.ComponentType> = {
  agent: AgentTab,
  me: MeTab,
  interface: InterfaceTab,
  keybindings: KeybindingsTab,
  general: GeneralTab,
  browser: BrowserTab,
  work: WorkTab,
  skills: SkillsTab,
  mcp: McpTab,
  bridge: BridgeTab,
  providers: ProvidersTab,
  models: ModelsTab,
  usage: UsageTab,
  sharing: SharingTab,
  access: AccessTab,
  experiments: ExperimentsTab,
  security: SecurityTab,
  envdeps: EnvDepsTab,
  about: AboutTab,
};

function connectionState(connection: ServerConnection | null) {
  const persisted = readPersistedServerConnectionState();
  const serverConnections = connection
    ? upsertServerConnection(persisted.serverConnections, connection)
    : persisted.serverConnections;
  const persistedActive = persisted.activeServerConnectionId
    ? serverConnections[persisted.activeServerConnectionId] || null
    : null;
  const activeServerConnection = persistedActive || connection || null;
  return {
    serverConnections,
    activeServerConnectionId: activeServerConnection?.connectionId ?? null,
    activeServerConnection,
  };
}

function markRustCoreUnavailable(store: SettingsStore): void {
  const reason = t('settings.rustCoreUnavailable');
  store.set({
    rustSettingsUnavailable: true,
    activeTab: 'access',
    agents: [],
    currentAgentId: null,
    settingsAgentId: null,
    settingsConfig: null,
    settingsConfigKey: null,
    settingsConfigStatus: 'error',
    settingsConfigError: reason,
    settingsSnapshot: { ...createRemoteResource<SettingsSnapshot>(), status: 'error', error: reason },
    globalModelsConfig: null,
    runtimeModels: [],
    providersSummary: {},
  });
}

/** Tab 顶部大标题（对应左栏导航 label），所有 tab 都会显示 */
const TAB_TITLE_KEYS: Record<string, string> = {
  agent: 'settings.tabs.agent',
  me: 'settings.tabs.me',
  interface: 'settings.tabs.interface',
  general: 'settings.tabs.general',
  browser: 'settings.tabs.browser',
  work: 'settings.tabs.work',
  skills: 'settings.tabs.skills',
  mcp: 'settings.tabs.mcp',
  bridge: 'settings.tabs.bridge',
  providers: 'settings.tabs.providers',
  models: 'settings.tabs.models',
  usage: 'settings.tabs.usage',
  sharing: 'settings.tabs.sharing',
  access: 'settings.tabs.access',
  experiments: 'settings.tabs.experiments',
  security: 'settings.tabs.security',
  about: 'settings.tabs.about',
};

const TAB_DESCRIPTION_KEYS: Record<string, string> = {
  experiments: 'settings.experiments.description',
};

/** 页内自带首行导航的 tab 不再渲染页内大标题（标题只在弹窗头部/导航出现）。 */
const HEADINGLESS_TABS: ReadonlySet<string> = new Set(['usage']);

export function normalizeSettingsTab(tab: string): string {
  return tab;
}

interface SettingsContentProps {
  variant: 'window' | 'modal';
  onClose?: () => void;
  onActiveTabChange?: (tab: string) => void;
  listenToWindowTabSwitch?: boolean;
}

export function SettingsContent({
  variant,
  onClose,
  onActiveTabChange,
  listenToWindowTabSwitch = false,
}: SettingsContentProps) {
  const { activeTab, ready, rustSettingsUnavailable } = useSettingsStore(
    useShallow(s => ({ activeTab: s.activeTab, ready: s.ready, rustSettingsUnavailable: s.rustSettingsUnavailable }))
  );
  const set = useSettingsStore(s => s.set);
  const lastReportedActiveTabRef = useRef<string | null>(null);
  const settingsMainRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    initSettings();
  }, []);

  useEffect(() => {
    if (!listenToWindowTabSwitch) return;
    const platform = window.platform;
    if (!platform?.onSwitchTab) return;
    const unsubscribe = platform.onSwitchTab((tab: string) => {
      const nextTab = normalizeSettingsTab(tab);
      set({ activeTab: nextTab });
    });
    return typeof unsubscribe === 'function' ? unsubscribe : undefined;
  }, [listenToWindowTabSwitch, set]);

  useEffect(() => {
    const platform = window.platform;
    if (!platform?.onSettingsChanged) return;
    const unsubscribe = platform.onSettingsChanged((type: string, data: unknown) => {
      if (type === 'skills-changed') {
        window.dispatchEvent(new CustomEvent('hana-skills-changed', { detail: data || {} }));
      } else if (type === 'models-changed') {
        if (!useSettingsStore.getState().rustSettingsUnavailable) void loadSettingsModels();
      }
    });
    return typeof unsubscribe === 'function' ? unsubscribe : undefined;
  }, []);

  useEffect(() => {
    const refreshModels = () => {
      if (!useSettingsStore.getState().rustSettingsUnavailable) void loadSettingsModels();
    };
    window.addEventListener('hana-models-changed', refreshModels);
    return () => window.removeEventListener('hana-models-changed', refreshModels);
  }, []);

  useEffect(() => {
    const nextTab = normalizeSettingsTab(activeTab);
    if (nextTab !== activeTab) {
      set({ activeTab: nextTab });
      lastReportedActiveTabRef.current = nextTab;
      onActiveTabChange?.(nextTab);
    }
  }, [activeTab, set, onActiveTabChange]);

  // Server 重启后用新端口重新加载数据
  useEffect(() => {
    const platform = window.platform;
    if (!platform?.onServerRestarted) return;
    let lastLocalIdentity: ServerConnection | null = null;
    const unsubscribe = platform.onServerRestarted((data: { port: number; token?: string | null; serverNodeKind?: string | null; serverNodeTransport?: string | null }) => {
      const store = useSettingsStore.getState();
      const priorToken = store.serverToken;
      const fromEvent = validateRestartTransport(data?.port, data?.token);
      if (fromEvent && fromEvent.port === String(store.serverPort) && fromEvent.token === priorToken
        && (data.serverNodeKind === 'lingxi-service') === (store.activeServerConnection?.serverNodeKind === 'lingxi-service')
        && (data.serverNodeTransport || 'http') === (store.activeServerConnection?.serverNodeTransport || 'http')) {
        return;
      }
      const previousLocal = store.serverConnections[LOCAL_CONNECTION_ID]
        ?? (store.activeServerConnection?.connectionId === LOCAL_CONNECTION_ID ? store.activeServerConnection : null)
        ?? lastLocalIdentity;
      const expectedRust = previousLocal?.serverNodeKind === 'lingxi-service';
      if (previousLocal) lastLocalIdentity = { ...previousLocal, token: null };
      const previousActiveId = store.activeServerConnectionId;
      const cleared = refreshLocalServerConnectionState({
        serverConnections: store.serverConnections,
        activeServerConnectionId: previousActiveId,
        activeServerConnection: store.activeServerConnection,
        serverPort: null,
        serverToken: null,
      });
      store.set({ serverPort: null, serverToken: null, ...cleared });
      const applyFresh = (transport: { port: string; token: string }) => {
        const verified = createLocalServerConnection({
          serverPort: transport.port,
          serverToken: transport.token,
          serverNodeKind: data.serverNodeKind,
          serverNodeTransport: data.serverNodeTransport,
        });
        if (!verified) throw new Error('invalid restarted local server connection');
        const current = useSettingsStore.getState();
        const serverConnections = previousLocal
          ? { ...current.serverConnections, [LOCAL_CONNECTION_ID]: previousLocal }
          : current.serverConnections;
        const next = refreshLocalServerConnectionState({
          serverConnections,
          activeServerConnectionId: previousActiveId,
          activeServerConnection: current.activeServerConnection,
          serverPort: transport.port,
          serverToken: transport.token,
          serverNodeKind: verified.serverNodeKind,
          serverNodeTransport: verified.serverNodeTransport,
        });
        current.set({ serverPort: Number(transport.port), serverToken: transport.token, ...next });
        if (verified.serverNodeKind === 'lingxi-service') {
          markRustCoreUnavailable(useSettingsStore.getState());
          current.showToast(t('settings.rustCoreUnavailable'), 'error');
          return;
        }
        current.set({ rustSettingsUnavailable: false });
        const agentsReload = loadAgents().catch(() => {});
        // snapshot 依赖 agentId，必须等 agents 完成后再发。
        agentsReload.then(() => { loadSettingsSnapshot().catch(() => {}); });
        loadSettingsModels().catch(() => {});
        loadProvidersSummary().catch(() => {});
      };
      if (fromEvent && fromEvent.token !== priorToken
        && (!expectedRust || data.serverNodeKind === 'lingxi-service')) {
        try {
          applyFresh(fromEvent);
          return;
        } catch { /* 拒绝坏协议字段 */ }
      }
      store.showToast(t('status.serverRestartInvalid'), 'error');
      // 桥事件缺字段或仍给旧令牌时保持断开，等待下一次有效重启事件。
    });
    return () => {
      if (typeof unsubscribe === 'function') unsubscribe();
    };
  }, []);

  const effectiveActiveTab = normalizeSettingsTab(activeTab);
  const ActiveTab = TAB_COMPONENTS[effectiveActiveTab] || AgentTab;
  const isModal = variant === 'modal';

  // 切换页签时把主内容区滚回顶部。settings-main 是唯一滚动容器，页签内容在
  // 同一容器里原地替换，scrollTop 会带着上一个页签的滚动位置（如在供应商页
  // 滚到下方保存配置后切回助手页），导致新页签第一个元素顶到容器上沿、
  // 上半截再被 modal 顶部的 sticky 渐变遮罩盖住（settings-main::before）。
  useEffect(() => {
    // 直接赋 scrollTop（而非 scrollTo）：jsdom 未实现 Element.scrollTo，
    // 测试环境里 scrollTo 会直接抛 TypeError。
    if (settingsMainRef.current) settingsMainRef.current.scrollTop = 0;
  }, [effectiveActiveTab]);

  const tabTitleKey = TAB_TITLE_KEYS[effectiveActiveTab];
  const activeTabTitle = tabTitleKey ? t(tabTitleKey) : '';
  const activeTabDescriptionKey = TAB_DESCRIPTION_KEYS[effectiveActiveTab];
  const activeTabDescription = activeTabDescriptionKey ? t(activeTabDescriptionKey) : '';
  const reportActiveTabChange = useCallback((tab: string) => {
    const nextTab = normalizeSettingsTab(tab);
    lastReportedActiveTabRef.current = nextTab;
    onActiveTabChange?.(nextTab);
  }, [onActiveTabChange]);

  useEffect(() => {
    if (lastReportedActiveTabRef.current === null) {
      lastReportedActiveTabRef.current = effectiveActiveTab;
      return;
    }
    if (lastReportedActiveTabRef.current === effectiveActiveTab) return;
    lastReportedActiveTabRef.current = effectiveActiveTab;
    onActiveTabChange?.(effectiveActiveTab);
  }, [effectiveActiveTab, onActiveTabChange]);

  return (
    <ErrorBoundary region="settings">
      <div className={styles['settings-content-root']} data-input-ctx-zone="settings">
        <div
          className={`settings-panel ${isModal ? styles['settings-panel-modal'] : ''}`}
          id="settingsPanel"
        >
          <div className={`settings-header ${isModal ? styles['settings-header-modal'] : ''}`}>
            {isModal ? (
              <>
                <div className={styles['settings-title-group']}>
                  <button
                    type="button"
                    className={styles['settings-return-btn']}
                    onClick={onClose}
                    aria-label={t('settings.back')}
                    data-settings-return
                  >
                    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M15 18l-6-6 6-6" />
                    </svg>
                  </button>
                  <h1 className={styles['settings-title']}>{t('settings.title')}</h1>
                </div>
                <h1 className={styles['settings-header-tab-title']}>{activeTabTitle}</h1>
              </>
            ) : (
              <h1 className={styles['settings-title']}>{t('settings.title')}</h1>
            )}
          </div>
          <div className={styles['settings-body']}>
            <SettingsNav onTabChange={reportActiveTabChange} />
            <div className={styles['settings-main']} ref={settingsMainRef} data-settings-main>
              {!isModal && !HEADINGLESS_TABS.has(effectiveActiveTab) && (
                <div className={styles['settings-tab-heading']}>
                  <h1 className={styles['settings-tab-title']}>{activeTabTitle}</h1>
                  {activeTabDescription && (
                    <p className={styles['settings-tab-description']}>{activeTabDescription}</p>
                  )}
                </div>
              )}
              <ErrorBoundary region={effectiveActiveTab} resetKeys={[effectiveActiveTab]}>
                <SettingsPage tab={effectiveActiveTab} layout={effectiveActiveTab === 'providers' ? 'fill' : 'flow'}>
                  {rustSettingsUnavailable && effectiveActiveTab !== 'access'
                    ? <div role="alert">{t('settings.rustCoreUnavailable')}</div>
                    : <ActiveTab />}
                </SettingsPage>
              </ErrorBoundary>
            </div>
          </div>
          <CompiledMemoryViewer />
        </div>

        <Toast />
        <CropOverlay />
        <AgentCreateOverlay />
        <AgentDeleteOverlay />
        <MemoryViewer />
        <ClearMemoryConfirm />
        <BridgeTutorial />
        <WechatQrcodeOverlay />
        {/* 独立设置窗口需要自己的右键菜单；应用内 modal 复用 App 已挂载的那份，避免叠两层 */}
        {variant === 'window' && <InputContextMenu />}

        {!ready && (
          <div className="settings-loading-mask" id="settingsLoadingMask">
            <div className={styles['settings-loading-text']}>
              loading...
            </div>
          </div>
        )}
      </div>
    </ErrorBoundary>
  );
}

/** 初始化：连接三要素并行 → 连接就绪后全部数据加载并行（含供应商摘要） */
async function initSettings() {
  const platform = window.platform;
  const store = useSettingsStore.getState();
  let requestedRust = false;
  // store 是模块级 singleton：重开设置时上次的完整数据还在。有缓存就静默后台
  // 刷新、直接渲染旧数据，绝不用全屏 loading mask 挡住用户；只有冷启动（连
  // settingsConfig 都没有）才显示 mask。
  const hasCachedConfig = store.settingsConfig != null;
  if (!hasCachedConfig) store.set({ ready: false });

  // 超时保护：15 秒后强制显示，防止无限白屏
  const timeout = setTimeout(() => {
    if (!useSettingsStore.getState().ready) {
      console.warn('[settings] init timeout (15s), forcing ready');
      useSettingsStore.getState().set({ ready: true });
    }
  }, 15_000);

  try {
    const [connectionInfo, platformName] = await Promise.all([
      typeof platform?.getServerConnectionInfo === 'function'
        ? platform.getServerConnectionInfo()
        : Promise.all([
          typeof platform?.getServerPort === 'function' ? platform.getServerPort() : null,
          typeof platform?.getServerToken === 'function' ? platform.getServerToken() : null,
        ]).then(([port, token]) => ({ port, token, serverNodeKind: null, serverNodeTransport: 'http' })),
      (async () => {
        try {
          return typeof platform?.getPlatform === 'function' ? await platform.getPlatform() : null;
        } catch {
          return null;
        }
      })(),
    ]);
    const rawServerPort = connectionInfo.port;
    requestedRust = connectionInfo.serverNodeKind === 'lingxi-service';
    const serverToken = connectionInfo.token;
    const serverPort = rawServerPort === null || rawServerPort === undefined
      ? null
      : Number(rawServerPort);
    const localConnection = createLocalServerConnection({
      serverPort, serverToken,
      serverNodeKind: connectionInfo.serverNodeKind,
      serverNodeTransport: connectionInfo.serverNodeTransport,
    });
    store.set({
      serverPort,
      serverToken,
      platformName,
      ...connectionState(localConnection),
    });
    requestedRust = useSettingsStore.getState().activeServerConnection?.serverNodeKind === 'lingxi-service';

    if (requestedRust) {
      await window.i18n.load('zh-CN');
      markRustCoreUnavailable(useSettingsStore.getState());
      const identityResponse = await lingxiFetch('/lingxi/v1/server/identity');
      const identity = await identityResponse.json();
      if (identity?.serverNodeKind !== 'lingxi-service') {
        throw new Error('Rust settings server identity does not match');
      }
      store.set({ ready: true });
      store.showToast(t('settings.rustCoreUnavailable'), 'error');
      return;
    }
    store.set({ rustSettingsUnavailable: false });

    // i18n（依赖 /api/config 的 locale，内部全容错，绝不阻塞 ready）
    const i18nReady = (async () => {
      const i18n = window.i18n;
      try {
        const cfgRes = await lingxiFetch('/api/config');
        const cfg = await cfgRes.json();
        const locale = cfg.locale || 'zh-CN';
        await i18n.load(locale);
      } catch {
        try { await i18n.load('zh-CN'); } catch { /* i18n fallback failed, continue */ }
      }
    })();

    // 连接已就绪：互不依赖的数据加载并行。供应商摘要由 init 统一发起（原来
    // ProvidersTab 挂载时抢跑 fetch，连接未就绪必败且静默无重试，供应商页数据残缺）。
    // snapshot / avatars 依赖 agentId（getSettingsAgentId 读 loadAgents 的落库结果）：
    // 必须串在 agents 之后。曾经与 agents 并行，冷启动时 agentId 必为 null，快照以
    // 「No settings agent selected」必败且无重试，settingsConfig 恒 null——关于页
    // 「自动检查更新 / 接收测试版更新」两个 Toggle 永久卡 loading 脉冲且点不动。
    const agentsReady = loadAgents();
    await Promise.all([
      i18nReady,
      agentsReady,
      // retainSameKeyData：重开设置时保留同 key 旧数据直到新数据到达，避免内容区闪空。
      agentsReady.then(() => loadSettingsSnapshot({ retainSameKeyData: true })),
      agentsReady.then(() => loadAvatars()),
      loadSettingsModels(),
    ]);

    store.set({ ready: true });

    // 供应商摘要不阻塞 ready：列表可先由 settingsConfig 渲染，计数/凭证点随后补齐；
    // 失败不拖垮整个 init（保持原 ProvidersTab 的容错语义）。
    void loadProvidersSummary().catch(() => {});
  } catch (err) {
    console.error('[settings] init failed:', err);
    if (requestedRust) {
      markRustCoreUnavailable(useSettingsStore.getState());
      store.showToast(err instanceof Error ? err.message : String(err), 'error');
    }
    store.set({ ready: true }); // 即使失败也移除 mask，让用户能操作
  } finally {
    clearTimeout(timeout);
  }
}

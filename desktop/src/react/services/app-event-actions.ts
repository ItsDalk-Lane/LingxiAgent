import { applyBrowserStatus } from './browser-events';
import { useStore } from '../stores';
import { useTenetStore } from '../stores/tenet-store';
import { lingxiFetch } from '../hooks/use-hana-fetch';
import { applyAgentIdentity, loadAgents } from '../stores/agent-actions';
import { loadMessages, loadSessions, switchSession } from '../stores/session-actions';
import { loadModels } from '../utils/ui-helpers';
import { activateWorkspaceDesk } from '../stores/desk-actions';
import { loadChannels } from '../stores/channel-actions';
import { applyChatLayout } from '../chat/layout';
import { applyEditorTypography } from '../editor/typography';
import { mergeWorkspaceHistory } from '../../../../shared/workspace-history.ts';
import { resolveServerConnection } from './server-connection';
import { bridgePublicConnectionKey } from './bridge-response';
import { handleConversationEvent } from './conversation-events';
import { applyRustMediaJobs } from './rust-media';
import { captureSessionConnection } from './session-connection-owner';
import { sessionScopedKey, sessionScopedValue } from '../stores/session-slice';
import { bumpMessageLiveVersion } from '../stores/message-live-version';
import { usePlatform } from '../hooks/use-platform';

declare const i18n: {
  locale: string;
  defaultName: string;
  load(locale: string): Promise<void>;
};

/* eslint-disable @typescript-eslint/no-explicit-any -- app events cross IPC/WS boundaries */

// Race guard: rapid agent switches (A→B→C) can cause stale async responses
// from earlier switches to overwrite current state. Same pattern as
// _switchVersion in session-actions.ts.
let _agentSwitchVersion = 0;
let requestContextUsage: (sessionPath: string) => void = () => {};

interface AppEventOptions {
  source?: string;
  isCurrent?: () => boolean;
}

export function configureAppEventActions(options: {
  requestContextUsage?: (sessionPath: string) => void;
}): void {
  requestContextUsage = options.requestContextUsage || (() => {});
}

function normalizeWorkspacePath(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

export function readConfigHomeFolder(config: any): string | null {
  return normalizeWorkspacePath(config?.desk?.home_folder ?? config?.deskHome);
}

export function readConfigCwdHistory(config: any): string[] {
  const history = Array.isArray(config?.cwd_history)
    ? config.cwd_history
    : Array.isArray(config?.cwdHistory)
      ? config.cwdHistory
      : [];
  return mergeWorkspaceHistory(history, []);
}

export function readConfigMemoryMasterEnabled(config: any): boolean {
  return config?.memory?.enabled !== false;
}

function handleAgentWorkspaceChanged(data: any): void {
  const state = useStore.getState();
  if (!data?.agentId || data.agentId !== state.currentAgentId) return;

  const previousHomeFolder = state.homeFolder || null;
  const previousSelectedFolder = state.selectedFolder || null;
  const nextHomeFolder = normalizeWorkspacePath(data.homeFolder);
  const selectedFollowedDefault = !state.selectedWorkspaceMountId
    && (!previousSelectedFolder || previousSelectedFolder === previousHomeFolder);
  const nextSelectedFolder = selectedFollowedDefault ? nextHomeFolder : previousSelectedFolder;
  const deskWasShowingDefault =
    !state.deskWorkspaceMountId && (
      state.pendingNewSession ||
      !state.currentSessionPath ||
      !state.deskBasePath ||
      (!!previousHomeFolder && state.deskBasePath === previousHomeFolder)
    );

  useStore.setState({
    homeFolder: nextHomeFolder,
    selectedFolder: nextSelectedFolder,
    selectedWorkspaceMountId: selectedFollowedDefault ? null : state.selectedWorkspaceMountId,
    selectedWorkspaceLabel: selectedFollowedDefault ? null : state.selectedWorkspaceLabel,
    workspaceFolders: [],
  });

  if (deskWasShowingDefault) {
    void activateWorkspaceDesk(nextHomeFolder, { mountId: null });
  }
}

export function handleAppEvent(type: string, data: any = {}, options: AppEventOptions = {}): void | Promise<void> {
  if (options.isCurrent && !options.isCurrent()) return;
  switch (type) {
    case 'goal_over_budget': {
      if (typeof data?.sessionId !== 'string' || !data.sessionId || typeof data?.name !== 'string'
        || !['tokens', 'time'].includes(data.kind)) break;
      const label = data.kind === 'time' ? '用时' : '用量';
      useStore.getState().addToast(`目标“${data.name}”已超出${label}预算，请查看对应对话。`, 'warning', 8_000, {
        dedupeKey: `goal-budget:${data.sessionId}:${data.name}:${data.kind}`,
      });
      break;
    }
    case 'agent_activity': {
      const entry = data?.entry;
      if (entry && typeof entry.id === 'string' && entry.id && typeof entry.sessionId === 'string'
        && entry.sessionId && entry.sessionPath === `native:${entry.sessionId}`) {
        useStore.getState().upsertAgentActivity(entry);
      }
      break;
    }
    case 'workflow_progress': {
      const sessionId = typeof data?.sessionId === 'string' ? data.sessionId : '';
      const taskId = typeof data?.taskId === 'string' ? data.taskId : '';
      if (!sessionId || !taskId || (data.sessionPath && data.sessionPath !== `native:${sessionId}`)) break;
      const state = useStore.getState();
      const entries = Object.values(state.agentActivitiesBySession || {}).flat();
      const entry = entries.find(item => item.id === taskId && item.sessionId === sessionId);
      const phaseLabel = typeof data.title === 'string' ? data.title : typeof data.message === 'string' ? data.message : null;
      if (entry && phaseLabel !== null) state.upsertAgentActivity({ ...entry, phaseLabel });
      break;
    }
    case 'jian_update': {
      // 便笺从当前工作区正式接口重读，不能将其他工作区的正文覆盖到焦点页面。
      return import('../stores/desk-actions').then(actions => {
        if (!options.isCurrent || options.isCurrent()) return actions.loadJianContent();
      });
    }
    case 'media_generation_update': {
      const owner = captureSessionConnection();
      if (owner && typeof data?.sessionId === 'string') applyRustMediaJobs(owner, data.sessionId, [data.mediaGeneration]);
      break;
    }
    case 'voice_transcription_update': {
      // Rust 服务（POST /lingxi/v1/media/asr/transcribe）的转写事件以 sessionId
      // 标识（现役 WS 通道以 sessionPath 标识，两条通道各自解析，互不冒充）。
      // 语义与现役 applyVoiceTranscriptionUpdate 一致：附件按 fileId 就地挂
      // transcription；找不到会话或附件时不虚构更新。
      const sessionId = typeof data?.sessionId === 'string' ? data.sessionId : '';
      const fileId = typeof data?.fileId === 'string' ? data.fileId : '';
      const transcription = data?.transcription && typeof data.transcription === 'object' ? data.transcription : null;
      if (!sessionId || !fileId || !transcription) break;
      const sessionPath = `native:${sessionId}`;
      let changed = false;
      useStore.setState((s: any) => {
        const session = sessionScopedValue<any>(s, s.chatSessions, sessionPath);
        if (!session) return {};
        const items = session.items.map((item: any) => {
          if (item?.type !== 'message' || !Array.isArray(item.data?.attachments)) return item;
          let itemChanged = false;
          const attachments = item.data.attachments.map((attachment: any) => {
            if (attachment?.fileId !== fileId) return attachment;
            itemChanged = true;
            return { ...attachment, transcription };
          });
          if (!itemChanged) return item;
          changed = true;
          return { ...item, data: { ...item.data, attachments } };
        });
        if (!changed) return {};
        const sessionKey = sessionScopedKey(s, sessionPath) || sessionPath;
        return { chatSessions: { ...s.chatSessions, [sessionKey]: { ...session, items } } };
      });
      if (changed) bumpMessageLiveVersion(sessionPath);
      break;
    }
    case 'browser_status':
    case 'browser_update':
      applyBrowserStatus(data);
      break;
    case 'session_collab_decision': {
      const sessionId = typeof data?.sourceSessionId === 'string' ? data.sourceSessionId : data?.sessionId;
      if (typeof sessionId !== 'string' || !sessionId || (data?.sessionId && data.sessionId !== sessionId)) break;
      const state = useStore.getState();
      const path = state.currentSessionPath;
      if (path && (path === `native:${sessionId}` || state.sessions.some(session => session.path === path && session.sessionId === sessionId))) {
        return loadMessages(path);
      }
      break;
    }
    case 'channel_new_message':
    case 'channel_created':
    case 'dm_new_message':
    case 'conversation_agent_activity':
      handleConversationEvent({ ...data, type });
      break;
    case 'bridge_status': {
      if (typeof data?.agentId !== 'string' || !data.agentId) break;
      useStore.getState().triggerBridgeReload();
      const connectionKey = bridgePublicConnectionKey(useStore.getState());
      window.dispatchEvent(new CustomEvent('hana-bridge-reload', { detail: { agentId: data.agentId, connectionKey } }));
      if (options.source === 'server') usePlatform()?.settingsChanged?.('bridge-status-changed', { agentId: data.agentId, connectionKey });
      break;
    }
    case 'bridge_message': {
      const message = data?.message;
      if (!message || typeof message.agentId !== 'string' || typeof message.sessionKey !== 'string'
        || typeof message.platform !== 'string' || typeof message.text !== 'string') break;
      useStore.getState().addBridgeMessage(message);
      break;
    }
    case 'bridge_rc_attached':
    case 'rc_attached': {
      if (typeof data?.sessionPath !== 'string' || typeof data.sessionKey !== 'string' || !data.sessionPath || !data.sessionKey) break;
      useStore.setState(state => ({ sessions: state.sessions.map(session => session.path === data.sessionPath
        ? { ...session, rcAttachment: { sessionKey: data.sessionKey, platform: data.platform || 'bridge', title: data.title || null } } : session) }));
      break;
    }
    case 'bridge_rc_detached':
    case 'rc_detached': {
      if (typeof data?.sessionPath !== 'string' || typeof data.sessionKey !== 'string') break;
      useStore.setState(state => ({ sessions: state.sessions.map(session => session.path === data.sessionPath && session.rcAttachment?.sessionKey === data.sessionKey
        ? { ...session, rcAttachment: null } : session) }));
      break;
    }
    case 'activity_update': {
      const incoming = data?.activity;
      if (!incoming || typeof incoming !== 'object' || typeof incoming.id !== 'string' || !incoming.id) {
        console.warn('[app-event] invalid activity update');
        break;
      }
      useStore.setState(current => {
        const activities = Array.isArray(current.activities) ? current.activities : [];
        const existing = activities.find(activity => activity.id === incoming.id);
        return { activities: [{ ...existing, ...incoming }, ...activities.filter(activity => activity.id !== incoming.id)].slice(0, 500) };
      });
      break;
    }
    case 'cron_job_done': {
      const connectionKey = JSON.stringify(resolveServerConnection(useStore.getState()));
      const isCurrent = () => (!options.isCurrent || options.isCurrent())
        && JSON.stringify(resolveServerConnection(useStore.getState())) === connectionKey;
      return lingxiFetch('/api/desk/cron').then(response => response.json()).then(value => {
        if (!isCurrent()) return;
        if (!Array.isArray(value?.jobs)) throw new Error('invalid automation list');
        useStore.setState({ automationCount: value.jobs.length });
      }).catch(() => {
        if (isCurrent()) console.warn('[app-event] automation refresh failed; existing count retained');
      });
    }
    case 'agent-switched': {
      const myVersion = ++_agentSwitchVersion;

      // 用户原则（tenets）：切换/启动时拉一次待审列表，聊天审批横幅据此出现
      if (typeof data.agentId === 'string' && data.agentId) {
        void useTenetStore.getState().refresh(data.agentId);
      }

      applyAgentIdentity({
        agentName: data.agentName,
        agentId: data.agentId,
      });
      const homeFolder = normalizeWorkspacePath(data.homeFolder);
      const cwd = normalizeWorkspacePath(data.cwd) || homeFolder;
      useStore.setState({
        homeFolder,
        selectedFolder: homeFolder || cwd,
        workspaceFolders: Array.isArray(data.workspaceFolders)
          ? data.workspaceFolders.filter((p: unknown): p is string => typeof p === 'string' && !!p.trim())
          : [],
        ...(Array.isArray(data.cwdHistory)
          ? { cwdHistory: mergeWorkspaceHistory(data.cwdHistory, []) }
          : {}),
        ...(typeof data.memoryMasterEnabled === 'boolean'
          ? { memoryMasterEnabled: data.memoryMasterEnabled }
          : {}),
      });
      if (data.sessionPath) {
        void switchSession(data.sessionPath);
      } else {
        void activateWorkspaceDesk(cwd);
      }
      loadSessions();

      // Reset channel state for new agent
      useStore.setState({
        currentChannel: null,
        channelMessages: [],
        channelMembers: [],
        channelTotalUnread: 0,
        channelHeaderName: '',
        channelHeaderMembersText: '',
        channelInfoName: '',
        channelIsDM: false,
      });
      loadChannels();

      // Reload models. Keep the current non-legacy UI value until the session
      // switch/model-default response installs the target Agent's level.
      loadModels();

      // Reload automation count and clear activities
      lingxiFetch('/api/desk/cron').then(r => r.json()).then((d: any) => {
        if (myVersion !== _agentSwitchVersion) return; // stale
        useStore.setState({ automationCount: d.jobs?.length || 0 });
      }).catch(() => {});
      useStore.setState({ activities: [] });
      break;
    }
    case 'locale-changed':
      i18n.load(data.locale).then(() => {
        i18n.defaultName = useStore.getState().agentName;
        useStore.setState({ locale: i18n.locale });
      });
      break;
    case 'models-changed': {
      loadModels();
      // 设置页可能是主窗口里已经挂载的 modal。主进程广播会排除事件发送者，
      // 所以这里同时通知当前 renderer；独立设置窗口仍走既有 IPC 通道。
      window.dispatchEvent(new CustomEvent('hana-models-changed', { detail: data || {} }));
      if (options.source === 'server') {
        usePlatform()?.settingsChanged?.('models-changed', data || {});
      }
      // 模型配置变更可能改变 contextWindow（用户把 1M 模型改成 256k 等），
      // 主动补发一次 context_usage 让 ContextRing 立即吃到新分母。
      const sp = useStore.getState().currentSessionPath;
      if (sp) {
        requestContextUsage(sp);
      }
      break;
    }
    case 'skills-changed': {
      useStore.setState((state: any) => ({
        skillCatalogVersion: (Number(state.skillCatalogVersion) || 0) + 1,
      }));
      window.dispatchEvent(new CustomEvent('hana-skills-changed', { detail: data || {} }));
      if (options.source === 'server') {
        usePlatform()?.settingsChanged?.('skills-changed', data || {});
      }
      break;
    }
    case 'agent-created':
      loadAgents();
      loadChannels();
      break;
    case 'agent-deleted': {
      // If the currently open conversation is a DM with the deleted agent, clear it
      const deletedDmId = data.agentId ? `dm:${data.agentId}` : null;
      if (deletedDmId && useStore.getState().currentChannel === deletedDmId) {
        useStore.setState({
          currentChannel: null,
          channelMessages: [],
          channelHeaderName: '',
          channelHeaderMembersText: '',
          channelIsDM: false,
        });
      }
      loadAgents();
      loadChannels();
      break;
    }
    case 'agent-updated': {
      const currentAgentId = useStore.getState().currentAgentId;
      if (data.agentId && data.agentId !== currentAgentId) {
        loadAgents();
        break;
      }
      applyAgentIdentity({
        agentName: data.agentName,
        agentId: data.agentId,
        yuan: data.yuan,
        ui: { settings: false },
      });
      break;
    }
    case 'memory-master-changed': {
      const state = useStore.getState();
      if (!data.agentId) break;
      const enabled = data.enabled !== false;
      const patch: any = {};
      if (Array.isArray(state.agents)) {
        patch.agents = state.agents.map((agent: any) =>
          agent.id === data.agentId ? { ...agent, memoryMasterEnabled: enabled } : agent,
        );
      }
      if (data.agentId === state.currentAgentId) {
        patch.memoryMasterEnabled = enabled;
      }
      if (Object.keys(patch).length) {
        useStore.setState(patch);
      }
      break;
    }
    case 'agent-workspace-changed':
      handleAgentWorkspaceChanged(data);
      break;
    case 'session-authorized-folders-updated':
      if (typeof data.sessionPath === 'string' && data.sessionPath) {
        useStore.getState().setSessionAuthorizedFolders(
          data.sessionPath,
          Array.isArray(data.authorizedFolders)
            ? data.authorizedFolders.filter((p: unknown): p is string => typeof p === 'string' && !!p.trim())
            : [],
        );
      }
      break;
    case 'theme-changed':
      window.setTheme(data.theme);
      break;
    case 'font-changed':
      window.setSerifFont(data.serif);
      break;
    case 'editor-typography-changed':
      applyEditorTypography(data.editor ?? data);
      break;
    case 'chat-layout-changed':
      applyChatLayout(data.chat ?? data);
      break;
    case 'tenets-changed': {
      // 用户原则变更（新提案/审批/增删）：刷新 tenet store，聊天审批横幅与设置页随之更新
      const tenetAgentId = typeof data.agentId === 'string' && data.agentId ? data.agentId : null;
      if (tenetAgentId) {
        void useTenetStore.getState().refresh(tenetAgentId);
      }
      break;
    }
    case 'experiment-changed':
      window.dispatchEvent(new CustomEvent('hana-settings', {
        detail: { type: 'experiment-changed', id: data.id, value: data.value },
      }));
      break;
    case 'network-proxy-changed':
      if (options.source === 'server') {
        usePlatform()?.settingsChanged?.('network-proxy-changed', data);
      }
      break;
    case 'keep-awake-changed':
      if (options.source === 'server') {
        usePlatform()?.settingsChanged?.('keep-awake-changed', data);
      }
      break;
    case 'paper-texture-changed':
      window.setPaperTexture(data.enabled);
      break;
    case 'sidebar-ui-changed':
      useStore.getState().applySidebarUiPrefs(data);
      break;
    case 'leaves-overlay-changed':
      window.dispatchEvent(new CustomEvent('hana-settings', {
        detail: { type: 'leaves-overlay-changed', enabled: data.enabled },
      }));
      break;
  }
}

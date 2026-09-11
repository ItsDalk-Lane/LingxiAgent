import type { ReactNode } from 'react';
import { useStore } from '../../stores';
import { ActivityPanel } from '../ActivityPanel';
import { AutomationPanel } from '../AutomationPanel';
import { BridgePanel } from '../BridgePanel';
import { SkillsPanel } from '../SkillsPanel';
import { PreviewPanel } from '../PreviewPanel';
import { SideChatPanel } from '../side-chat/SideChatPanel';
import { ChannelMessages, ChannelMembers, ChannelInput, ChannelReadonly, ChannelAgentActivityPanel, ChannelAgentSettingsPanel, ChannelExportPanel } from '../ChannelsPanel';
import { ChannelHeader } from '../channels/ChannelHeader';
import { MainContent } from '../../MainContent';
import { ChatPage } from './ChatPage';
import { KnowledgePage } from '../knowledge/KnowledgePage';

const tr = (key: string, vars?: Record<string, string | number>) => window.t?.(key, vars) ?? key;

function ChannelInputArea() {
  const currentChannel = useStore(s => s.currentChannel);
  const isDM = useStore(s => s.channelIsDM);

  if (!currentChannel) return null;

  if (isDM) {
    return (
      <div className="channel-readonly-notice">
        <ChannelReadonly />
      </div>
    );
  }

  return (
    <div className="channel-input-area">
      <ChannelInput />
    </div>
  );
}

function ChannelInspectorShell({ children }: { children: ReactNode }) {
  return (
    <aside className="channel-inspector-rail" id="channelInspector" data-channel-inspector="">
      <div className="resize-handle resize-handle-left" id="channelInspectorResizeHandle"></div>
      {children}
    </aside>
  );
}

function ChannelInspectorPanel() {
  const channelInfoName = useStore(s => s.channelInfoName);
  const isDM = useStore(s => s.channelIsDM);
  const currentChannel = useStore(s => s.currentChannel);

  if (!currentChannel) return null;

  if (isDM) {
    return (
      <ChannelInspectorShell>
        <div className="channel-info-stack">
          <div className="universal-card">
            <div className="channel-info-section">
              <div className="channel-info-label">{tr('channel.dmLabel')}</div>
              <div className="channel-members-list">
                <ChannelMembers />
              </div>
            </div>
          </div>
          <ChannelAgentSettingsPanel />
          <ChannelAgentActivityPanel />
          <ChannelExportPanel />
        </div>
      </ChannelInspectorShell>
    );
  }

  return (
    <ChannelInspectorShell>
      <div className="channel-info-stack">
        <div className="universal-card">
          <div className="channel-info-section">
            <div className="channel-info-label">{tr('channel.info')}</div>
            <div className="channel-info-name">{channelInfoName}</div>
          </div>
          <div className="channel-info-section">
            <div className="channel-info-label">{tr('channel.members')}</div>
            <div className="channel-members-list">
              <ChannelMembers />
            </div>
          </div>
        </div>
        <ChannelAgentSettingsPanel />
        <ChannelAgentActivityPanel />
        <ChannelExportPanel />
      </div>
    </ChannelInspectorShell>
  );
}

function ChannelPage() {
  const currentChannel = useStore(s => s.currentChannel);

  return (
    <div className="channel-page">
      <div className="channel-view active">
        {currentChannel ? (
          <>
            <ChannelHeader />
            <div className="channel-messages">
              <ChannelMessages />
            </div>
            <ChannelInputArea />
          </>
        ) : (
          <div className="channel-select-empty">
            {tr('channel.selectHint')}
          </div>
        )}
      </div>
      <ChannelInspectorPanel />
    </div>
  );
}

export function AppPages() {
  const currentTab = useStore(s => s.currentTab);
  const sideChatOpen = useStore(s => s.sideChat.open);

  return (
    <>
      {/* 侧边对话打开时：预览面板先让位（排到侧栏左侧，空间不足时被挤出视口），
          保证右侧栏与主对话的输入区始终可见。 */}
      {currentTab === 'chat' && (
        <div className={sideChatOpen ? 'preview-panel-slot preview-panel-slot-yield' : 'preview-panel-slot'}>
          <PreviewPanel />
        </div>
      )}

      <MainContent>
        {currentTab === 'chat' && <ChatPage />}
        {currentTab === 'knowledge' && <KnowledgePage />}
        {currentTab === 'channels' && <ChannelPage />}
        <ActivityPanel />
        <AutomationPanel />
        <SkillsPanel />
        <BridgePanel />
      </MainContent>

      {currentTab === 'chat' && <SideChatPanel />}
    </>
  );
}

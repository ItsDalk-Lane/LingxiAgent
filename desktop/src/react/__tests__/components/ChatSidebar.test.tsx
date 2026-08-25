// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { ChatSidebarContent } from '../../components/app/ChatSidebar';

vi.mock('../../components/channels/ChannelList', () => ({
  ChannelListSidebar: () => <section data-testid="channel-list-sidebar" />,
}));

vi.mock('../../components/RegionalErrorBoundary', () => ({
  RegionalErrorBoundary: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));

vi.mock('../../components/SessionList', () => ({
  SessionList: () => <section data-testid="session-list" />,
}));

vi.mock('../../components/notices/SidebarNoticeSlot', () => ({
  SidebarNoticeSlot: () => null,
}));

vi.mock('../../components/right-workspace/WorkspaceStableBody', () => ({
  WorkspaceStableBody: () => <section data-testid="workspace-stable-body" />,
}));

vi.mock('../../stores/browser-slice', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../stores/browser-slice')>();
  return {
    ...actual,
    useAnyBrowserRunning: () => false,
  };
});

describe('ChatSidebarContent', () => {
  beforeEach(() => {
    window.t = ((key: string) => ({
      'sidebar.title': '对话',
      'sidebar.newChat': '新对话',
      'sidebar.collapse': '收起',
      'sidebar.bridgeShort': '社交平台',
      'sidebar.activity': '助手活动',
      'automation.title': '任务计划',
      'skills.panel.title': 'Skills',
      'browser.background': '浏览器',
      'browser.backgroundHint': '浏览器',
      'settings.title': '设置',
    }[key] || key)) as typeof window.t;
    useStore.setState({
      automationCount: 3,
      bridgeDotConnected: true,
      currentAgentId: 'agent-a',
      currentTab: 'chat',
    } as never);
  });

  afterEach(() => {
    cleanup();
  });

  it('drops the legacy header title, new-chat button and collapse button', () => {
    render(<ChatSidebarContent onTogglePanel={vi.fn()} />);

    expect(screen.queryByText('对话')).not.toBeInTheDocument();
    expect(screen.queryByTitle('新对话')).not.toBeInTheDocument();
    expect(screen.queryByTitle('收起')).not.toBeInTheDocument();
    expect(document.querySelector('.sidebar-header')).not.toBeInTheDocument();
    expect(document.querySelector('.sidebar-activity-bar')).not.toBeInTheDocument();
  });

  it('keeps Bridge/Activity/Automation/Skills/Settings as icon buttons in one top row', () => {
    const onTogglePanel = vi.fn();
    const onOpenSettings = vi.fn();

    render(<ChatSidebarContent onTogglePanel={onTogglePanel} onOpenSettings={onOpenSettings} />);

    const row = document.querySelector('.sidebar-function-row');
    expect(row).toBeInTheDocument();
    const buttons = within(row as HTMLElement).getAllByRole('button');
    expect(buttons.map(b => b.getAttribute('title'))).toEqual(['社交平台', '助手活动', '任务计划', 'Skills', '设置']);

    // Bridge 在线状态点与 Automation 数量角标继续工作
    expect(row!.querySelector('.sidebar-bridge-dot.connected')).toBeInTheDocument();
    expect(row!.querySelector('.automation-count-badge')).toHaveTextContent('3');

    fireEvent.click(screen.getByTitle('社交平台'));
    expect(onTogglePanel).toHaveBeenCalledWith('bridge');
    fireEvent.click(screen.getByTitle('助手活动'));
    expect(onTogglePanel).toHaveBeenCalledWith('activity');
    fireEvent.click(screen.getByTitle('任务计划'));
    expect(onTogglePanel).toHaveBeenCalledWith('automation');
    fireEvent.click(screen.getByTitle('Skills'));
    expect(onTogglePanel).toHaveBeenCalledWith('skills');
    fireEvent.click(screen.getByTitle('设置'));
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
  });

  it('lays out the session list and the workspace section as the two 50% regions', () => {
    render(<ChatSidebarContent onTogglePanel={vi.fn()} />);

    const sessionRegion = document.querySelector('.session-list');
    const workspaceRegion = document.querySelector('[data-sidebar-workspace-section]');
    expect(sessionRegion).toBeInTheDocument();
    expect(workspaceRegion).toBeInTheDocument();
    expect(screen.getByTestId('session-list')).toBeInTheDocument();
    expect(screen.getByTestId('workspace-stable-body')).toBeInTheDocument();
    expect(
      sessionRegion!.compareDocumentPosition(workspaceRegion as HTMLElement) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it('omits the workspace section and the whole function row when disabled (mobile)', () => {
    render(
      <ChatSidebarContent
        showSettingsButton={false}
        showActivityBars={false}
        showWorkspaceSection={false}
      />,
    );

    expect(document.querySelector('.sidebar-function-row')).not.toBeInTheDocument();
    expect(document.querySelector('[data-sidebar-workspace-section]')).not.toBeInTheDocument();
    expect(screen.getByTestId('session-list')).toBeInTheDocument();
  });
});

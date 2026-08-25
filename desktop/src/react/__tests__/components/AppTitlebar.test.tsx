// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AppTitlebar } from '../../components/app/AppTitlebar';

vi.mock('../../components/channels/ChannelTabBar', () => ({
  ChannelTabBar: () => <div data-testid="channel-tabs" />,
}));

vi.mock('../../components/plugin/WidgetButtons', () => ({
  WidgetButtons: () => <div data-testid="widget-buttons" />,
}));

vi.mock('../../components/WindowControls', () => ({
  WindowControls: () => <div data-testid="window-controls" />,
}));

describe('AppTitlebar', () => {
  beforeEach(() => {
    window.t = ((key: string) => key) as typeof window.t;
  });

  afterEach(() => {
    cleanup();
  });

  it('does not render the file preview toggle by default on desktop', () => {
    render(
      <AppTitlebar
        sidebarOpen={false}
        jianOpen={false}
        onToggleSidebar={vi.fn()}
        onToggleJian={vi.fn()}
      />,
    );

    expect(screen.queryByTitle('preview.toggle')).not.toBeInTheDocument();
    expect(screen.getByTitle('sidebar.jian')).toBeInTheDocument();
  });

  it('does not render the right workspace toggle when onToggleJian is omitted (desktop)', () => {
    render(
      <AppTitlebar
        sidebarOpen={false}
        onToggleSidebar={vi.fn()}
      />,
    );

    expect(screen.queryByTitle('sidebar.jian')).not.toBeInTheDocument();
    expect(screen.getByTitle('sidebar.toggle')).toBeInTheDocument();
  });

  it('renders a file preview toggle next to the right workspace toggle when enabled', () => {
    const onTogglePreview = vi.fn();

    render(
      <AppTitlebar
        sidebarOpen={false}
        jianOpen={false}
        previewOpen={false}
        showPreviewToggle
        onToggleSidebar={vi.fn()}
        onToggleJian={vi.fn()}
        onTogglePreview={onTogglePreview}
      />,
    );

    const previewToggle = screen.getByTitle('preview.toggle');
    const rightToggle = screen.getByTitle('sidebar.jian');
    expect(previewToggle.compareDocumentPosition(rightToggle) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(previewToggle).not.toHaveClass('active');

    fireEvent.click(previewToggle);
    expect(onTogglePreview).toHaveBeenCalledTimes(1);
  });

  it('marks the file preview toggle active while the preview panel is open', () => {
    render(
      <AppTitlebar
        sidebarOpen={false}
        jianOpen={false}
        previewOpen={true}
        showPreviewToggle
        onToggleSidebar={vi.fn()}
        onToggleJian={vi.fn()}
        onTogglePreview={vi.fn()}
      />,
    );

    expect(screen.getByTitle('preview.toggle')).toHaveClass('active');
  });

  it('renders an icon-only chat search toggle next to the sidebar toggle when the handler is passed', () => {
    const onOpenChatSearch = vi.fn();

    render(
      <AppTitlebar
        sidebarOpen={false}
        onToggleSidebar={vi.fn()}
        onOpenChatSearch={onOpenChatSearch}
      />,
    );

    const searchToggle = screen.getByTitle('titlebar.search');
    const sidebarToggle = screen.getByTitle('sidebar.toggle');
    // 放大镜在左栏 toggle 旁（其后方）。
    expect(sidebarToggle.compareDocumentPosition(searchToggle) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(searchToggle).toHaveClass('tb-toggle');
    // 纯图标：按钮内没有常驻文字。
    expect(searchToggle.textContent).toBe('');

    fireEvent.click(searchToggle);
    expect(onOpenChatSearch).toHaveBeenCalledTimes(1);
  });

  it('marks the chat search toggle active while the search overlay is open', () => {
    render(
      <AppTitlebar
        sidebarOpen={false}
        onToggleSidebar={vi.fn()}
        onOpenChatSearch={vi.fn()}
        chatSearchOpen
      />,
    );

    expect(screen.getByTitle('titlebar.search')).toHaveClass('active');
  });

  it('does not render the chat search toggle when the handler is omitted (mobile)', () => {
    render(
      <AppTitlebar
        sidebarOpen={false}
        onToggleSidebar={vi.fn()}
      />,
    );

    expect(screen.queryByTitle('titlebar.search')).not.toBeInTheDocument();
  });
});

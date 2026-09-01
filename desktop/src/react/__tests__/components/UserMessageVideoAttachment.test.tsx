// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { UserMessage } from '../../components/chat/UserMessage';
import { useStore } from '../../stores';
import * as filePreview from '../../utils/file-preview';

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: vi.fn(async () => new Response('{}', { status: 200 })),
  lingxiUrl: (path: string) => `http://127.0.0.1:3210${path}`,
}));

vi.mock('../../utils/screenshot', () => ({
  takeScreenshot: vi.fn(),
}));

describe('user message video attachment card', () => {
  const openFilePreviewSpy = vi.fn();
  const getFileUrl = vi.fn((filePath: string) => `file://${filePath}`);

  beforeEach(() => {
    window.t = ((key: string) => key) as typeof window.t;
    window.platform = {
      getFileUrl,
    } as unknown as typeof window.platform;
    useStore.setState({
      activeServerConnection: null,
      sessionRegistryFilesByPath: {},
      chatSessions: {},
    } as any);
    openFilePreviewSpy.mockClear();
    getFileUrl.mockClear();
    vi.spyOn(filePreview, 'openFilePreview').mockImplementation(openFilePreviewSpy);
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  function renderVideoAttachment(overrides: Record<string, unknown> = {}) {
    render(
      <UserMessage
        viewerIdentity={{ name: '我', avatarUrl: null }}
        readOnly
        isStreaming={false}
        isSelected={false}
        showAvatar={false}
        sessionPath="/sessions/main.jsonl"
        message={{
          id: 'u1',
          role: 'user',
          text: '视频内容是什么？',
          timestamp: Date.now(),
          attachments: [{
            fileId: 'sf_clip',
            path: '/cache/session-files/clip.mp4',
            name: 'clip.mp4',
            isDir: false,
            mimeType: 'video/mp4',
            ...overrides,
          }],
        }}
      />,
    );
  }

  it('renders a poster video with a play badge instead of a dead file chip', () => {
    renderVideoAttachment();

    const video = screen.getByRole('button', { name: 'clip.mp4' }).querySelector('video');
    expect(video).not.toBeNull();
    expect(video).toHaveAttribute('src', 'file:///cache/session-files/clip.mp4');
    expect(video).toHaveAttribute('preload', 'metadata');
    expect(screen.getByText('clip.mp4')).toBeInTheDocument();
  });

  it('opens the media viewer through openFilePreview on click', () => {
    renderVideoAttachment();

    fireEvent.click(screen.getByRole('button', { name: 'clip.mp4' }));

    expect(openFilePreviewSpy).toHaveBeenCalledTimes(1);
    expect(openFilePreviewSpy).toHaveBeenCalledWith(
      '/cache/session-files/clip.mp4',
      'clip.mp4',
      'mp4',
      {
        origin: 'session',
        sessionPath: '/sessions/main.jsonl',
        messageId: 'u1',
      },
    );
  });

  it('falls back to a file chip when the poster source cannot be resolved', () => {
    // 远程/无 getFileUrl 场景：海报拿不到源，但仍应可点击进入 MediaViewer。
    window.platform = {} as unknown as typeof window.platform;
    renderVideoAttachment();

    const card = screen.getByRole('button', { name: 'clip.mp4' });
    expect(card.querySelector('video')).toBeNull();
    fireEvent.click(card);
    expect(openFilePreviewSpy).toHaveBeenCalledTimes(1);
  });

  it('falls back to the file icon when the poster fails to decode', () => {
    renderVideoAttachment();

    const card = screen.getByRole('button', { name: 'clip.mp4' });
    const video = card.querySelector('video');
    expect(video).not.toBeNull();
    fireEvent.error(video!);

    expect(card.querySelector('video')).toBeNull();
    // 播放角标与名称仍在，点击仍可进 MediaViewer。
    expect(screen.getByText('clip.mp4')).toBeInTheDocument();
    fireEvent.click(card);
    expect(openFilePreviewSpy).toHaveBeenCalledTimes(1);
  });

  it('keeps the expired presentation on the chip branch', () => {
    renderVideoAttachment({ status: 'expired' });

    expect(screen.queryByRole('button', { name: 'clip.mp4' })).toBeNull();
    expect(screen.getByText(/clip\.mp4/)).toBeInTheDocument();
  });
});

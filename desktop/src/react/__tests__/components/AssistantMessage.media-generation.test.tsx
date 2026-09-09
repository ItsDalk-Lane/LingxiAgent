// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AssistantMessage } from '../../components/chat/AssistantMessage';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import { useStore } from '../../stores';

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: vi.fn(async () => new Response('{}', { status: 200 })),
  lingxiUrl: (path: string) => `http://127.0.0.1:3210${path}`,
}));

vi.mock('../../utils/screenshot', () => ({
  takeScreenshot: vi.fn(),
}));

describe('AssistantMessage media generation placeholder', () => {
  beforeEach(() => {
    window.t = ((key: string) => key) as typeof window.t;
    (window as any).platform = {
      getFileUrl: (filePath: string) => `file://${filePath}`,
      startDrag: vi.fn(),
    };
    useStore.setState({
      serverPort: 3210,
      serverToken: null,
      serverConnections: {},
      activeServerConnectionId: null,
      activeServerConnection: null,
      chatSessions: {},
      sessionRegistryFilesByPath: {},
      agents: [],
      agentName: 'Hanako',
      agentYuan: 'lingxi',
      mediaViewer: null,
      streamingSessions: [],
      selectedMessageIdsBySession: {},
    } as never);
  });

  afterEach(() => {
    cleanup();
    delete (window as any).platform;
    useStore.setState({ serverPort: null, serverToken: null, serverConnections: {}, activeServerConnectionId: null, activeServerConnection: null, chatSessions: {}, sessionRegistryFilesByPath: {} } as never);
    vi.restoreAllMocks();
  });

  it('renders a grey image placeholder with inline status text and cycling dot slot', () => {
    const { container } = render(
      <AssistantMessage
        agentDisplay={{ id: 'hana', displayName: 'Hana', avatarUrl: null, fallbackAvatar: null, yuan: 'hana', isUser: false }}
        isStreaming={false}
        isSelected={false}
        showAvatar={false}
        sessionPath="/sessions/main.jsonl"
        readOnly
        message={{
          id: 'a1',
          role: 'assistant',
          blocks: [{
            type: 'media_generation',
            taskId: 'task-img',
            kind: 'image',
            status: 'pending',
            prompt: 'Low-poly 3D illustration of a Chinese college student character sitting at the front row of a classroom',
          }],
        }}
      />,
    );

    expect(screen.getByLabelText('chat.media.generationInProgress...')).toBeInTheDocument();
    expect(container.querySelector('[class*="mediaGenerationDots"]')).toBeInTheDocument();
    expect(screen.getByText(/^Low-poly 3D illustration/)).toBeInTheDocument();
  });

  it('retries a failed image placeholder in place without sending a new agent turn', async () => {
    const resolveBlockByTaskId = vi.fn(() => true);
    useStore.setState({
      resolveBlockByTaskId,
    } as never);
    vi.mocked(lingxiFetch).mockResolvedValueOnce(new Response(JSON.stringify({
      ok: true,
      placeholder: {
        type: 'media_generation',
        taskId: 'task-img',
        kind: 'image',
        status: 'pending',
        prompt: 'same prompt',
      },
    }), { status: 200 }));

    render(
      <AssistantMessage
        agentDisplay={{ id: 'hana', displayName: 'Hana', avatarUrl: null, fallbackAvatar: null, yuan: 'hana', isUser: false }}
        isStreaming={false}
        isSelected={false}
        showAvatar={false}
        sessionPath="/sessions/main.jsonl"
        message={{
          id: 'a1',
          role: 'assistant',
          blocks: [{
            type: 'media_generation',
            taskId: 'task-img',
            kind: 'image',
            status: 'failed',
            reason: 'API returned no images',
            prompt: 'same prompt',
          }],
        }}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'chat.media.retryLabel' }));

    await waitFor(() => {
      expect(lingxiFetch).toHaveBeenCalledWith('/api/media/tasks/task-img/retry', {
        method: 'POST',
      });
    });
    expect(resolveBlockByTaskId).toHaveBeenCalledWith('/sessions/main.jsonl', 'task-img', expect.objectContaining({
      type: 'media_generation',
      taskId: 'task-img',
      kind: 'image',
      status: 'pending',
      prompt: 'same prompt',
    }));
  });

  it('renders generated video files as media cards that open the media viewer and drag out the file', async () => {
    const startDrag = vi.fn();
    (window as any).platform = {
      getFileUrl: (filePath: string) => `file://${filePath}`,
      startDrag,
    };

    render(
      <AssistantMessage
        agentDisplay={{ id: 'hana', displayName: 'Hana', avatarUrl: null, fallbackAvatar: null, yuan: 'hana', isUser: false }}
        isStreaming={false}
        isSelected={false}
        showAvatar={false}
        sessionPath="/sessions/main.jsonl"
        message={{
          id: 'a1',
          role: 'assistant',
          blocks: [{
            type: 'file',
            fileId: 'sf_video',
            filePath: '/tmp/generated/agnes.mp4',
            label: 'agnes.mp4',
            ext: 'mp4',
            mime: 'video/mp4',
            kind: 'video',
          }],
        }}
      />,
    );

    const card = await screen.findByTestId('video-output-card');
    expect(card.querySelector('video')).toBeInTheDocument();

    fireEvent.click(card);
    await waitFor(() => {
      expect(useStore.getState().mediaViewer?.currentId).toContain('/tmp/generated/agnes.mp4');
    });

    fireEvent.dragStart(card);
    expect(startDrag).toHaveBeenCalledWith('/tmp/generated/agnes.mp4');
  });

  function mediaMessage(ext: 'png' | 'mp4', resourceId?: string) {
    return <AssistantMessage
      agentDisplay={{ id: 'hana', displayName: 'Hana', avatarUrl: null, fallbackAvatar: null, yuan: 'hana', isUser: false }}
      isStreaming={false}
      isSelected={false}
      showAvatar={false}
      sessionPath="/sessions/main.jsonl"
      message={{
        id: 'a-media', role: 'assistant', blocks: [{
          type: 'file', fileId: 'sf_generated', filePath: `/tmp/generated/output.${ext}`,
          label: `output.${ext}`, ext,
          ...(resourceId ? { resource: {
            schemaVersion: 1 as const, resourceId, studioId: 'studio_local',
            name: `output.${ext}`, type: 'file', source: 'session_file',
            lifecycle: { status: 'available', missingAt: null },
            storage: { provider: 'session_file' },
            links: { self: `/api/resources/${resourceId}`, content: `/api/resources/${resourceId}/content` },
          } } : {}),
        }],
      }}
    />;
  }

  it.each(['png', 'mp4'] as const)('尚未进入文件登记的 %s 使用块内受控地址直接预览', (ext) => {
    const { container } = render(mediaMessage(ext, 'res_generated'));
    const media = container.querySelector(ext === 'png' ? 'img' : 'video');
    expect(media).toHaveAttribute('src', 'http://127.0.0.1:3210/api/resources/res_generated/content');
  });

  it.each(['png', 'mp4'] as const)('%s 旧地址失败后新资源地址到达能恢复预览', (ext) => {
    const { container, rerender } = render(mediaMessage(ext));
    const selector = ext === 'png' ? 'img' : 'video';
    fireEvent.error(container.querySelector(selector)!);
    expect(container.querySelector(selector)).toBeNull();
    rerender(mediaMessage(ext, 'res_ready'));
    expect(container.querySelector(selector)).toHaveAttribute('src', 'http://127.0.0.1:3210/api/resources/res_ready/content');
  });

  it('视频在聊天内提供播放器控件，点击视频不会另开查看器', () => {
    const { container } = render(mediaMessage('mp4', 'res_generated'));
    const video = container.querySelector('video')!;
    expect(video).toHaveAttribute('controls');
    expect(video.muted).toBe(false);
    fireEvent.click(video);
    expect(useStore.getState().mediaViewer).toBeNull();
  });

  it('图片仍可点击放大查看', async () => {
    const { container } = render(mediaMessage('png', 'res_generated'));
    fireEvent.click(container.querySelector('img')!);
    await waitFor(() => expect(useStore.getState().mediaViewer?.currentId).toContain('/tmp/generated/output.png'));
    expect(useStore.getState().mediaViewer?.files[0].resource?.links.content).toBe('/api/resources/res_generated/content');
  });

  it('isolates a malformed rich block without hiding sibling message blocks', () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);

    expect(() => render(
      <AssistantMessage
        agentDisplay={{ id: 'hana', displayName: 'Hana', avatarUrl: null, fallbackAvatar: null, yuan: 'hana', isUser: false }}
        isStreaming={false}
        isSelected={false}
        showAvatar={false}
        sessionPath="/sessions/main.jsonl"
        readOnly
        message={{
          id: 'a1',
          role: 'assistant',
          blocks: [
            { type: 'text', html: '<p>before bad block</p>' },
            { type: 'plugin_card' } as never,
            { type: 'text', html: '<p>after bad block</p>' },
          ],
        }}
      />,
    )).not.toThrow();

    expect(screen.getByText('before bad block')).toBeInTheDocument();
    expect(screen.getByText('after bad block')).toBeInTheDocument();
    expect(errorSpy).toHaveBeenCalled();
  });
});
